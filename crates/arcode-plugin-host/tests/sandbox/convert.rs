//! Converts the guest SDK's view types (wit-bindgen) into the host's
//! (wasmtime bindgen). Both come from the same WIT, so this is a 1:1 map;
//! it lets host tests feed SDK diff output straight into `PanelTree`.

use arcode_plugin::ui as g;
use arcode_plugin_host::bindings::ui as h;

fn tone(t: g::Tone) -> h::Tone {
    match t {
        g::Tone::Normal => h::Tone::Normal,
        g::Tone::Muted => h::Tone::Muted,
        g::Tone::Accent => h::Tone::Accent,
        g::Tone::Success => h::Tone::Success,
        g::Tone::Warning => h::Tone::Warning,
        g::Tone::Danger => h::Tone::Danger,
    }
}

fn axis(a: g::Axis) -> h::Axis {
    match a {
        g::Axis::Horizontal => h::Axis::Horizontal,
        g::Axis::Vertical => h::Axis::Vertical,
    }
}

fn widget(w: &g::Widget) -> h::Widget {
    match w.clone() {
        g::Widget::Stack(p) => h::Widget::Stack(h::StackProps {
            axis: axis(p.axis),
            gap: p.gap,
            padding: p.padding,
        }),
        g::Widget::Split(p) => h::Widget::Split(h::SplitProps {
            axis: axis(p.axis),
            ratio: p.ratio,
        }),
        g::Widget::Tabs(p) => h::Widget::Tabs(h::TabsProps {
            titles: p.titles,
            active: p.active,
        }),
        g::Widget::Scroll => h::Widget::Scroll,
        g::Widget::Text(p) => h::Widget::Text(h::TextProps {
            text: p.text,
            tone: tone(p.tone),
            mono: p.mono,
            bold: p.bold,
            wrap: p.wrap,
        }),
        g::Widget::Badge(p) => h::Widget::Badge(h::BadgeProps {
            text: p.text,
            tone: tone(p.tone),
        }),
        g::Widget::Button(p) => h::Widget::Button(h::ButtonProps {
            label: p.label,
            icon: p.icon,
            primary: p.primary,
            enabled: p.enabled,
        }),
        g::Widget::Input(p) => h::Widget::Input(h::InputProps {
            value: p.value,
            placeholder: p.placeholder,
            multiline: p.multiline,
            language: p.language,
            submit_on_enter: p.submit_on_enter,
            notify_changes: p.notify_changes,
        }),
        g::Widget::Select(p) => h::Widget::Select(h::SelectProps {
            options: p.options,
            selected: p.selected,
        }),
        g::Widget::Checkbox(p) => h::Widget::Checkbox(h::CheckboxProps {
            label: p.label,
            checked: p.checked,
        }),
        g::Widget::KeyValues(kv) => h::Widget::KeyValues(kv),
        g::Widget::Table(p) => h::Widget::Table(h::TableProps {
            columns: p
                .columns
                .into_iter()
                .map(|c| h::Column {
                    title: c.title,
                    width: c.width,
                    numeric: c.numeric,
                    sortable: c.sortable,
                })
                .collect(),
            row_count: p.row_count,
            generation: p.generation,
        }),
        g::Widget::Tree(items) => h::Widget::Tree(
            items
                .into_iter()
                .map(|i| h::TreeItem {
                    key: i.key,
                    label: i.label,
                    detail: i.detail,
                    icon: i.icon,
                    depth: i.depth,
                    expandable: i.expandable,
                    expanded: i.expanded,
                })
                .collect(),
        ),
        g::Widget::Code(p) => h::Widget::Code(h::CodeProps {
            text: p.text,
            language: p.language,
            line_numbers: p.line_numbers,
        }),
        g::Widget::Json(j) => h::Widget::Json(j),
        g::Widget::Spinner(s) => h::Widget::Spinner(s),
        g::Widget::EmptyState(t) => h::Widget::EmptyState(t),
        g::Widget::Divider => h::Widget::Divider,
        g::Widget::Spacer => h::Widget::Spacer,
    }
}

/// One node.
pub fn node(n: &g::Node) -> h::Node {
    h::Node {
        id: n.id,
        widget: widget(&n.widget),
        children: n.children.clone(),
        grow: n.grow,
    }
}

/// A whole view.
pub fn view(v: &g::View) -> h::View {
    h::View {
        root: v.root,
        nodes: v.nodes.iter().map(node).collect(),
    }
}

/// A patch list.
pub fn patches(ps: &[g::Patch]) -> Vec<h::Patch> {
    ps.iter()
        .map(|p| match p {
            g::Patch::Upsert(n) => h::Patch::Upsert(node(n)),
            g::Patch::Remove(id) => h::Patch::Remove(*id),
            g::Patch::SetRoot(id) => h::Patch::SetRoot(*id),
            g::Patch::RowsChanged(r) => h::Patch::RowsChanged(*r),
        })
        .collect()
}
