use arcode_plugin_host::bindings::ui::{
    Cell, Column, InputProps, Node, Patch, RowPage, StackProps, TableProps, TabsProps, TextProps,
    Tone, View,
};
use arcode_plugin_host::rows::PageRequest;
use arcode_plugin_host::ui_tree::PanelTree;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::cells::{cell_text, column_width, index_width};
use super::table::TableState;
use super::*;

fn node(id: NodeId, widget: Widget, children: Vec<NodeId>) -> Node {
    Node {
        id,
        widget,
        children,
        grow: false,
    }
}

fn text(t: &str) -> Widget {
    Widget::Text(TextProps {
        text: t.into(),
        tone: Tone::Normal,
        mono: false,
        bold: false,
        wrap: false,
    })
}

fn tabs(active: u32) -> Node {
    node(
        2,
        Widget::Tabs(TabsProps {
            titles: vec!["Data".into(), "Query".into()],
            active,
        }),
        vec![3, 4],
    )
}

fn input(value: &str) -> Node {
    node(
        5,
        Widget::Input(InputProps {
            value: value.into(),
            placeholder: "SQL".into(),
            multiline: false,
            language: None,
            submit_on_enter: true,
            notify_changes: false,
        }),
        vec![],
    )
}

/// root stack(1) -> [tabs(2) -> [text(3), text(4)], input(5)]
fn first_view() -> View {
    View {
        root: 1,
        nodes: vec![
            node(
                1,
                Widget::Stack(StackProps {
                    axis: Axis::Vertical,
                    gap: 1,
                    padding: 1,
                }),
                vec![2, 5],
            ),
            tabs(0),
            node(3, text("data"), vec![]),
            node(4, text("query"), vec![]),
            input("select 1"),
        ],
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<PanelView>, &mut VisualTestContext, PanelTree) {
    crate::test_support::init(cx);
    let (client, _requests) = PluginClient::new();
    let plugin = PluginId::parse("acme.tools").unwrap();
    let mut tree = PanelTree::new();
    let (snapshot, changes) = tree.render(first_view()).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| PanelView::new(client, plugin, 1, None, cx));
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    cx.run_until_parked();
    (view, cx, tree)
}

#[gpui::test]
fn tabs_keep_the_users_choice_until_the_plugin_moves_them(cx: &mut TestAppContext) {
    let (view, cx, mut tree) = open(cx);
    view.update(cx, |v, _| {
        assert_eq!(v.tabs.get(&2), Some(&0));
        v.tabs.insert(2, 1);
    });
    // Re-sent unchanged: the user's tab stays.
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(tabs(0))]).unwrap();
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    view.update(cx, |v, _| assert_eq!(v.tabs.get(&2), Some(&1)));
    // The plugin switches tabs: that wins.
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(tabs(1))]).unwrap();
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    view.update(cx, |v, _| assert_eq!(v.tabs.get(&2), Some(&1)));
    view.update(cx, |v, _| {
        v.tabs.insert(2, 0);
    });
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(tabs(0))]).unwrap();
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    view.update(cx, |v, _| assert_eq!(v.tabs.get(&2), Some(&0)));
}

#[gpui::test]
fn edited_inputs_ride_along_and_plugin_values_overwrite(cx: &mut TestAppContext) {
    let (view, cx, mut tree) = open(cx);
    // Drawn once: the field exists with the plugin's value.
    let field = view.update(cx, |v, cx| {
        assert_eq!(v.inputs[&5].text(cx), "select 1");
        assert!(v.take_values(cx).is_empty());
        match &v.inputs[&5].kind {
            form::InputKind::Field(f) => f.clone(),
            form::InputKind::Code(_) => panic!("single-line input"),
        }
    });
    field.update(cx, |f, cx| f.set_text("select 2", cx));
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        assert_eq!(v.take_values(cx), vec![(5, "select 2".to_owned())]);
        assert!(v.take_values(cx).is_empty(), "sent once");
    });
    // A re-render that does not change `value` keeps the user's text.
    field.update(cx, |f, cx| f.set_text("select 3", cx));
    cx.run_until_parked();
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(tabs(0))]).unwrap();
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    view.update(cx, |v, cx| assert_eq!(v.inputs[&5].text(cx), "select 3"));
    // The plugin sets a new value: it replaces the text.
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(input("select 9"))]).unwrap();
    assert_eq!(changes.inputs, vec![5]);
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    cx.run_until_parked();
    view.update(cx, |v, cx| {
        assert_eq!(v.inputs[&5].text(cx), "select 9");
        assert!(v.take_values(cx).is_empty());
    });
}

#[gpui::test]
fn removed_nodes_drop_their_state(cx: &mut TestAppContext) {
    let (view, cx, mut tree) = open(cx);
    let stack = node(
        1,
        Widget::Stack(StackProps {
            axis: Axis::Vertical,
            gap: 0,
            padding: 0,
        }),
        vec![2],
    );
    let (snapshot, changes) = tree.apply(vec![Patch::Upsert(stack)]).unwrap();
    assert!(changes.removed.contains(&5));
    view.update(cx, |v, cx| v.apply(snapshot, changes, cx));
    view.update(cx, |v, _| assert!(!v.inputs.contains_key(&5)));
}

#[test]
fn cells_and_widths() {
    assert_eq!(cell_text(&Cell::Empty), "NULL");
    assert_eq!(cell_text(&Cell::Int(-4)), "-4");
    assert_eq!(cell_text(&Cell::Real(1.5)), "1.5");
    assert_eq!(cell_text(&Cell::Text("a\nb".into())), "a b");
    assert_eq!(cell_text(&Cell::Boolean(true)), "true");
    assert_eq!(cell_text(&Cell::Blob(12)), "<blob 12 bytes>");
    let long = cell_text(&Cell::Text("x".repeat(500)));
    assert!(long.ends_with('…') && long.chars().count() == 201);
    let col = |width, numeric| Column {
        title: "c".into(),
        width,
        numeric,
        sortable: false,
    };
    assert_eq!(column_width(&col(None, false)), 180.);
    assert_eq!(column_width(&col(None, true)), 110.);
    assert_eq!(column_width(&col(Some(5), false)), 40.);
    assert_eq!(column_width(&col(Some(5000), false)), 800.);
    assert_eq!(index_width(0), 48.);
    assert!(index_width(100_000) > index_width(999));
}

#[test]
fn unknown_row_counts_grow_with_the_pages() {
    let mut state = TableState::new_for_test(None);
    assert_eq!(state.item_count(), 200);
    state.cache_mut().set_viewport(0, 10);
    let request: PageRequest = state.cache_mut().next_fetch().expect("first page");
    let rows = vec![vec![Cell::Int(1)]; 200];
    state.cache_mut().page_arrived(
        request,
        RowPage {
            start: 0,
            rows,
            at_end: false,
        },
    );
    assert_eq!(state.item_count(), 400);
    let mut known = TableState::new_for_test(Some(100_000));
    assert_eq!(known.item_count(), 100_000);
    known.cache_mut().set_viewport(50_000, 50_030);
    let request = known.cache_mut().next_fetch().expect("a page");
    assert!(request.start <= 50_000 && request.start + u64::from(request.count) > 50_030);
}

/// Ctrl+End (Cmd+End) jumps to the last of 100,000 rows without clicking
/// the table first; Ctrl+Home goes back to the first.
#[gpui::test]
fn ctrl_end_jumps_to_the_last_row(cx: &mut TestAppContext) {
    crate::test_support::init(cx);
    let (client, _requests) = PluginClient::new();
    let plugin = PluginId::parse("acme.tools").unwrap();
    let table = Widget::Table(TableProps {
        columns: vec![Column {
            title: "a".into(),
            width: None,
            numeric: false,
            sortable: true,
        }],
        row_count: Some(100_000),
        generation: 0,
    });
    let mut tree = PanelTree::new();
    let view = View {
        root: 1,
        nodes: vec![node(1, table, vec![])],
    };
    let (snapshot, changes) = tree.render(view).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| PanelView::new(client, plugin, 1, None, cx));
    view.update_in(cx, |v, window, cx| {
        v.apply(snapshot, changes, cx);
        window.focus(&v.focus);
    });
    cx.run_until_parked();
    let selected = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.tables[&1].selected());
    cx.simulate_keystrokes("secondary-end");
    assert_eq!(selected(cx), Some(99_999));
    cx.simulate_keystrokes("secondary-home");
    assert_eq!(selected(cx), Some(0));
}
