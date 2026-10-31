//! The icon rail on the left edge (docs/DESIGN.md 6.3): activity tiles
//! drawn straight onto the canvas, with count badges, and the sidebar and
//! bottom panel toggles at the bottom.

use arcode_ui_kit::metrics::{RAIL_PITCH, RAIL_TILE, RAIL_TILE_RADIUS, RAIL_W, icon_size};
use arcode_ui_kit::{IconName, Tooltip, badge, icon, theme};
use gpui::{
    AnyView, App, ClickEvent, ElementId, IntoElement, SharedString, Window, div, prelude::*, px,
};

use crate::actions::{ToggleBottomPanel, ToggleSidebar, shortcut_label};

/// Width of the icon rail (the left gutter of the body).
pub const ACTIVITY_BAR_WIDTH: f32 = 46.;

/// A sidebar activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activity {
    /// The project file tree.
    #[default]
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

    /// Display name (tooltip and sidebar title), in sentence case.
    pub fn label(self) -> &'static str {
        match self {
            Activity::Files => "Files",
            Activity::Search => "Search",
            Activity::Git => "Source control",
            Activity::Debug => "Run and debug",
            Activity::Plugins => "Plugins",
            Activity::Todo => "Todo",
            Activity::Packages => "Packages",
            Activity::Extension => "Plugin",
        }
    }
}

/// What the rail shows besides its tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RailState {
    /// The activity whose sidebar view is chosen.
    pub active: Activity,
    /// The sidebar is shown (the active tile is lit only then).
    pub sidebar_visible: bool,
    /// The bottom panel is shown.
    pub bottom_visible: bool,
    /// Changed files in the repository (badge on Source control).
    pub git_changes: usize,
    /// TODO comments in the project (badge on Todo).
    pub todos: usize,
}

impl RailState {
    /// The badge of `activity`, if it has a non-zero count.
    pub fn badge(&self, activity: Activity) -> Option<String> {
        let count = match activity {
            Activity::Git => self.git_changes,
            Activity::Todo => self.todos,
            _ => 0,
        };
        (count > 0).then(|| badge_text(count))
    }
}

/// A rail badge's text: the count, `99+` above 99 so it fits the tile.
pub fn badge_text(count: usize) -> String {
    if count > 99 {
        "99+".into()
    } else {
        count.to_string()
    }
}

/// Click handler receiving the chosen activity.
pub type OnSelect = std::rc::Rc<dyn Fn(Activity, &mut Window, &mut App)>;

/// A rail entry after the built-in ones (a plugin's sidebar panel).
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

/// A 34 px rail tile. Inactive: muted icon, no fill, `hover` fill and
/// `text` icon on hover. Active: `pill` fill, `accent` icon. No indicator
/// bar. `lit` draws the icon in `text` without a fill (the panel toggles
/// while their part is shown).
fn tile(
    id: ElementId,
    icon_name: IconName,
    tip: SharedString,
    selected: bool,
    lit: bool,
    on_click: ClickHandler,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let c = theme(cx).colors.clone();
    let group = SharedString::from(format!("rail-{id:?}"));
    let color = if selected {
        c.accent
    } else if lit {
        c.text
    } else {
        c.text_muted
    };
    div()
        .id(id)
        .group(group.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(RAIL_TILE)
        .rounded(RAIL_TILE_RADIUS)
        .cursor_pointer()
        .when(selected, |el| el.bg(c.pill))
        .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
        .child(
            icon(icon_name)
                .size(icon_size::RAIL)
                .color(color)
                .group_hover_color(group, if selected { c.accent } else { c.text }),
        )
        .tooltip(move |_, cx| -> AnyView { Tooltip::text(tip.clone(), cx) })
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
            tile(
                ElementId::NamedInteger("activity-extra".into(), ix as u64),
                extra.icon,
                extra.label,
                extra.selected,
                false,
                Box::new(move |_, window, cx| on_select(ix, window, cx)),
                cx,
            )
            .into_any_element()
        })
        .collect()
}

/// A panel toggle at the bottom of the rail.
fn toggle(
    id: &'static str,
    icon_name: IconName,
    what: &str,
    keys: &str,
    shown: bool,
    action: fn() -> Box<dyn gpui::Action>,
    cx: &App,
) -> impl IntoElement + use<> {
    let verb = if shown { "Hide" } else { "Show" };
    let selector = id;
    tile(
        id.into(),
        icon_name,
        format!("{verb} {what} ({})", shortcut_label(keys)).into(),
        false,
        shown,
        Box::new(move |_, window, cx| window.dispatch_action(action(), cx)),
        cx,
    )
    .debug_selector(move || selector.into())
}

/// Renders the rail: one tile per activity (the active one lit while the
/// sidebar is visible), `extras` (see [`render_extras`]) after them, and
/// the sidebar and bottom panel toggles at the bottom. No fill and no
/// border: it is drawn on the canvas.
pub fn render(
    state: RailState,
    on_select: OnSelect,
    extras: Vec<gpui::AnyElement>,
    cx: &App,
) -> impl IntoElement + use<> {
    let gap = RAIL_PITCH - RAIL_TILE;
    let tiles = Activity::ALL.into_iter().map(|activity| {
        let selected = state.sidebar_visible && activity == state.active;
        let on_select = on_select.clone();
        let label = SharedString::new_static(activity.label());
        tile(
            label.clone().into(),
            activity.icon(),
            label,
            selected,
            false,
            Box::new(move |_: &ClickEvent, window, cx| on_select(activity, window, cx)),
            cx,
        )
        .when_some(state.badge(activity), |el, text| {
            el.child(
                div()
                    .absolute()
                    .right(px(1.))
                    .bottom(px(3.))
                    .child(badge(text).rail()),
            )
        })
    });
    div()
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(gap)
        .w(RAIL_W)
        .h_full()
        .pt(px(2.))
        .pb(px(2.))
        .children(tiles)
        .children(extras)
        .child(div().flex_1().min_h_0())
        .child(toggle(
            "rail-toggle-sidebar",
            IconName::PanelLeft,
            "sidebar",
            "secondary-b",
            state.sidebar_visible,
            || Box::new(ToggleSidebar),
            cx,
        ))
        .child(toggle(
            "rail-toggle-panel",
            IconName::PanelBottom,
            "panel",
            "secondary-j",
            state.bottom_visible,
            || Box::new(ToggleBottomPanel),
            cx,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_show_non_zero_counts_only() {
        let state = RailState {
            git_changes: 3,
            todos: 0,
            ..Default::default()
        };
        assert_eq!(state.badge(Activity::Git).as_deref(), Some("3"));
        assert_eq!(state.badge(Activity::Todo), None);
        assert_eq!(state.badge(Activity::Files), None);
        assert_eq!(badge_text(120), "99+");
    }

    #[test]
    fn labels_are_sentence_case() {
        for activity in Activity::ALL {
            let label = activity.label();
            assert_ne!(label, label.to_uppercase(), "{label}");
            let rest: String = label.chars().skip(1).collect();
            assert!(!rest.split(' ').any(|w| w.starts_with(char::is_uppercase)));
        }
        assert_eq!(RAIL_W, px(ACTIVITY_BAR_WIDTH));
    }
}
