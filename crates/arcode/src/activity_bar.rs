//! The narrow vertical bar of activity icons on the left edge.

use arcode_ui_kit::{IconName, Tooltip, icon, theme};
use gpui::{AnyView, App, ClickEvent, IntoElement, SharedString, Window, div, prelude::*, px};

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
}

impl Activity {
    /// Every activity in display order.
    pub const ALL: [Activity; 6] = [
        Activity::Files,
        Activity::Search,
        Activity::Git,
        Activity::Debug,
        Activity::Plugins,
        Activity::Todo,
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
        }
    }
}

/// Click handler receiving the chosen activity.
pub type OnSelect = std::rc::Rc<dyn Fn(Activity, &mut Window, &mut App)>;

/// Renders the activity bar. `active` is highlighted when the sidebar is
/// visible; `on_select` receives clicks.
pub fn render(
    active: Activity,
    sidebar_visible: bool,
    on_select: OnSelect,
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
            let label = SharedString::new_static(activity.label());
            div()
                .id(SharedString::new_static(activity.label()))
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
                        .child(icon(activity.icon()).size(px(20.)).color(if selected {
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
                .on_click(move |_: &ClickEvent, window, cx| on_select(activity, window, cx))
        }))
}
