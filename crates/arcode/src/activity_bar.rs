//! The narrow vertical bar of activity icons on the left edge.

use arcode_ui_kit::{IconName, Tooltip, icon, theme};
use gpui::{
    AnyView, App, ClickEvent, ElementId, IntoElement, SharedString, Window, div, prelude::*, px,
};

/// Width of the activity bar.
pub const ACTIVITY_BAR_WIDTH: f32 = 44.;

/// A sidebar activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// The project file tree.
    Files,
    /// Project-wide search.
    Search,
    /// Source control.
    Git,
    /// Debugger.
    Debug,
    /// Plugins.
    Plugins,
    /// TODO/FIXME list.
    Todo,
    /// Package managers and dependencies.
    Packages,
    /// A sidebar view contributed by a plugin (which one is kept by the
    /// workspace); its entry comes after the built-in ones.
    Extension,
}

impl Activity {
    /// Every activity in display order.
    pub const ALL: [Activity; 7] = [
        Activity::Files,
        Activity::Search,
        Activity::Git,
        Activity::Debug,
        Activity::Plugins,
        Activity::Todo,
        Activity::Packages,
    ];

    /// The icon shown in the activity bar.
    pub fn icon(self) -> IconName {
        match self {
            Activity::Files => IconName::Files,
            Activity::Search => IconName::Search,
            Activity::Git => IconName::GitBranch,
            Activity::Debug => IconName::Bug,
            Activity::Plugins => IconName::Puzzle,
            Activity::Todo => IconName::ListTodo,
            Activity::Packages => IconName::Package,
            Activity::Extension => IconName::Puzzle,
        }
    }

    /// Display name (tooltip and sidebar title).
    pub fn label(self) -> &'static str {
        match self {
            Activity::Files => "Explorer",
            Activity::Search => "Search",
            Activity::Git => "Source Control",
            Activity::Debug => "Run and Debug",
            Activity::Plugins => "Plugins",
            Activity::Todo => "TODO",
            Activity::Packages => "Packages",
            Activity::Extension => "Plugin",
        }
    }
}

/// Click handler receiving the chosen activity.
pub type OnSelect = std::rc::Rc<dyn Fn(Activity, &mut Window, &mut App)>;

/// An activity-bar entry after the built-in ones (a plugin's sidebar
/// panel).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtraActivity {
    /// The icon.
    pub icon: IconName,
    /// Tooltip.
    pub label: SharedString,
    /// Drawn as the shown sidebar view.
    pub selected: bool,
}

/// Click handler receiving the index of the chosen extra entry.
pub type OnSelectExtra = std::rc::Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// What clicking an entry does.
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One entry of the bar.
fn entry(
    id: ElementId,
    icon_name: IconName,
    label: SharedString,
    selected: bool,
    on_click: ClickHandler,
    cx: &App,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(40.))
        .cursor_pointer()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(32.))
                .rounded_md()
                .when(selected, |el| el.bg(c.hover))
                .hover(|s| s.bg(c.hover))
                .child(icon(icon_name).size(px(20.)).color(if selected {
                    c.text
                } else {
                    c.text_muted
                })),
        )
        .when(selected, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(8.))
                    .bottom(px(8.))
                    .w(px(2.))
                    .rounded_sm()
                    .bg(c.accent),
            )
        })
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(label.clone(), cx) })
        .on_click(move |e, window, cx| on_click(e, window, cx))
}

/// Renders the extra entries (plugin sidebar panels) below the built-in
/// ones; the caller adds them to [`render`]'s column.
pub fn render_extras(
    extras: Vec<ExtraActivity>,
    on_select: OnSelectExtra,
    cx: &App,
) -> Vec<gpui::AnyElement> {
    extras
        .into_iter()
        .enumerate()
        .map(|(ix, extra)| {
            let on_select = on_select.clone();
            entry(
                ElementId::NamedInteger("activity-extra".into(), ix as u64),
                extra.icon,
                extra.label,
                extra.selected,
                Box::new(move |_, window, cx| on_select(ix, window, cx)),
                cx,
            )
            .into_any_element()
        })
        .collect()
}

/// Renders the activity bar. `active` is highlighted when the sidebar is
/// visible; `on_select` receives clicks; `extras` (see [`render_extras`])
/// follow the built-in entries.
pub fn render(
    active: Activity,
    sidebar_visible: bool,
    on_select: OnSelect,
    extras: Vec<gpui::AnyElement>,
    cx: &App,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    div()
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap_1()
        .w(px(ACTIVITY_BAR_WIDTH))
        .h_full()
        .pt_1()
        .bg(c.activity_bar)
        .border_r_1()
        .border_color(c.border)
        .children(Activity::ALL.into_iter().map(|activity| {
            let selected = sidebar_visible && activity == active;
            let on_select = on_select.clone();
            entry(
                SharedString::new_static(activity.label()).into(),
                activity.icon(),
                SharedString::new_static(activity.label()),
                selected,
                Box::new(move |_: &ClickEvent, window, cx| on_select(activity, window, cx)),
                cx,
            )
        }))
        .children(extras)
}
