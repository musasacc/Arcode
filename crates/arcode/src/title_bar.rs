//! The title bar (docs/DESIGN.md 6.2), drawn on the canvas with no fill of
//! its own: the spark logo over the rail column, the project and branch
//! chips, the search field, and at the right the Paused chip, the Run split
//! button and the Debug and Settings buttons.

mod run;

use arcode_git::{FileStatus, StatusCode};
use arcode_ui_kit::metrics::{
    CONTROL_H, CONTROL_RADIUS, RAIL_W, TITLE_BAR_H, WEIGHT_SEMIBOLD, icon_size,
};
use arcode_ui_kit::{
    IconName, Tooltip, icon, icon_button, kbd, logo, readout, theme, truncated, type_scale,
};
use gpui::{AnyView, Context, IntoElement, SharedString, Window, div, prelude::*, px};

use crate::actions::{OpenSettings, ToggleFinder, shortcut_label};
use crate::prefs::tour::TourTarget;
use crate::workspace::Workspace;

/// Height of the title bar.
pub const TITLE_BAR_HEIGHT: f32 = 40.;

/// Width of the search field, and its width while the right cluster is
/// wider (paused: the Paused chip and Continue).
const SEARCH_W: f32 = 400.;
const SEARCH_W_NARROW: f32 = 320.;

/// Changed files of a repository by kind: `~2 +1` in the branch chip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GitCounts {
    /// Modified, deleted, renamed or conflicted tracked files.
    pub modified: usize,
    /// Added (staged new) and untracked files.
    pub added: usize,
}

impl GitCounts {
    /// Counts `entries` (ignored paths do not count).
    pub fn of(entries: &[FileStatus]) -> Self {
        let mut counts = Self::default();
        for entry in entries {
            match (entry.index, entry.worktree) {
                (StatusCode::Ignored, _) => {}
                (StatusCode::Untracked | StatusCode::Added, _) => counts.added += 1,
                _ => counts.modified += 1,
            }
        }
        counts
    }

    /// Every changed file.
    pub fn total(self) -> usize {
        self.modified + self.added
    }
}

/// Renders the title bar for `ws`.
pub fn render(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let c = theme(cx).colors.clone();
    let scale = type_scale(cx);
    let project = ws.project();
    let name: SharedString = project
        .map(|p| p.name.clone().into())
        .unwrap_or_else(|| "No project".into());
    let branch = ws.branch_label(cx);
    let counts = ws.git_counts(cx);
    let menu_open = ws.project_menu_open();
    let anchors = ws.tour_anchors().clone();
    let paused = ws.paused_readout().cloned();

    let dropdown = div()
        .id("project-dropdown")
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(CONTROL_H)
        .px(px(8.))
        .rounded(CONTROL_RADIUS)
        .cursor_pointer()
        .text_size(scale.title)
        .font_weight(WEIGHT_SEMIBOLD)
        .text_color(c.text)
        .hover(|s| s.bg(c.hover))
        .when(menu_open, |el| el.bg(c.hover))
        .child(
            div()
                .max_w(px(220.))
                .min_w_0()
                .overflow_hidden()
                .child(truncated(name)),
        )
        .child(
            icon(IconName::ChevronDown)
                .size(icon_size::TOOLBAR)
                .color(c.text_muted),
        )
        .child(anchors.recorder(TourTarget::ProjectDropdown))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_project_menu(cx)));

    let branch_chip = branch.map(|branch| {
        div()
            .id("branch")
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .min_w_0()
            .h(CONTROL_H)
            .rounded(CONTROL_RADIUS)
            .cursor_pointer()
            .hover(|s| s.bg(c.hover))
            .text_size(scale.body)
            .text_color(c.text_muted)
            .child(
                icon(IconName::GitBranch)
                    .size(icon_size::TOOLBAR)
                    .color(c.text_muted),
            )
            .child(div().min_w_0().overflow_hidden().child(truncated(branch)))
            .when(counts.modified > 0, |el| {
                el.child(readout(format!("~{}", counts.modified)).color(c.info))
            })
            .when(counts.added > 0, |el| {
                el.child(readout(format!("+{}", counts.added)).color(c.success))
            })
            .tooltip(|_, cx| -> AnyView { Tooltip::text("Switch branch", cx) })
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::actions::SwitchBranch), cx)
            })
    });

    // The spark sits centred over the rail column (no tile behind it).
    let left = div()
        .flex()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(RAIL_W)
                .h_full()
                .child(logo(px(22.))),
        )
        .child(dropdown)
        .children(branch_chip);

    let search_w = if paused.is_some() {
        SEARCH_W_NARROW
    } else {
        SEARCH_W
    };
    let search = div()
        .id("search-box")
        .relative()
        .flex()
        .items_center()
        .gap(px(8.))
        .w(px(search_w))
        .min_w(px(160.))
        .flex_shrink()
        .h(CONTROL_H)
        .pl(px(10.))
        .pr(px(5.))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(c.border)
        .bg(c.panel())
        .text_size(scale.body)
        .text_color(c.text_muted)
        .cursor_pointer()
        .hover(|s| s.border_color(c.border_strong))
        .child(
            icon(IconName::Search)
                .size(icon_size::TOOLBAR)
                .color(c.text_muted),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child("Search files"),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_size(scale.meta)
                .child("> commands"),
        )
        .child(kbd(shortcut_label("secondary-p")))
        .child(anchors.recorder(TourTarget::SearchBox))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleFinder), cx));

    let settings_active = ws.settings_tab_active(cx);
    let right = div()
        .flex()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .items_center()
        .justify_end()
        .gap(px(4.))
        .when(project.is_some(), |el| {
            el.when_some(paused.clone(), |el, at| {
                el.child(run::paused_chip(at, cx)).child(div().w(px(4.)))
            })
            .child(run::run_split(ws, paused.is_some(), cx))
            .child(div().w(px(6.)))
            .child(
                icon_button("debug", IconName::Bug)
                    .tooltip("Run and debug")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(
                            Box::new(crate::debugger::actions::ShowRunAndDebug),
                            cx,
                        )
                    }),
            )
        })
        .child(
            div()
                .relative()
                .child(
                    icon_button("settings", IconName::Settings)
                        .when(settings_active, |b| b.icon_color(c.accent))
                        .tooltip(format!("Settings ({})", shortcut_label("secondary-,")))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenSettings), cx)
                        }),
                )
                .child(anchors.recorder(TourTarget::SettingsButton)),
        );

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(TITLE_BAR_H)
        .pr(px(8.))
        .child(left)
        .child(search)
        .child(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn status(index: StatusCode, worktree: StatusCode) -> FileStatus {
        FileStatus {
            path: PathBuf::from("f"),
            index,
            worktree,
            orig_path: None,
            conflict: None,
        }
    }

    #[test]
    fn git_counts_split_modified_and_added() {
        use StatusCode::*;
        let entries = [
            status(Unmodified, Modified),
            status(Modified, Modified),
            status(Deleted, Unmodified),
            status(Untracked, Untracked),
            status(Added, Modified),
            status(Ignored, Ignored),
            status(Unmerged, Unmerged),
        ];
        let counts = GitCounts::of(&entries);
        assert_eq!(
            counts,
            GitCounts {
                modified: 4,
                added: 2
            }
        );
        assert_eq!(counts.total(), 6);
        assert_eq!(GitCounts::of(&[]).total(), 0);
    }

    #[test]
    fn height_matches_the_metrics() {
        assert_eq!(px(TITLE_BAR_HEIGHT), TITLE_BAR_H);
    }
}
