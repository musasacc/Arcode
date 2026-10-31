//! The file list of the source control view: groups, rows and status
//! letters (pure), plus the row element.

use std::collections::HashSet;
use std::path::Path;

use arcode_git::{FileStatus, StatusCode};
use arcode_ui_kit::metrics::{ROW_RADIUS, SECTION_H, WEIGHT_SEMIBOLD, icon_size};
use arcode_ui_kit::{
    IconName, ThemeColors, badge, icon, icon_button, theme, truncated, type_scale,
};
use gpui::{App, Context, Div, Hsla, SharedString, div, prelude::*, px};

use super::GitPanel;
use crate::file_icons::file_icon;

/// Height of a list row.
pub const ROW_HEIGHT: f32 = 24.;

/// A group of the file list, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Unmerged paths.
    Conflicts,
    /// Changes in the index.
    Staged,
    /// Changes of tracked files not in the index.
    Changes,
    /// Files git does not track.
    Untracked,
}

impl Group {
    /// Every group in display order.
    pub const ALL: [Group; 4] = [
        Group::Conflicts,
        Group::Staged,
        Group::Changes,
        Group::Untracked,
    ];

    /// The header title.
    pub fn title(self) -> &'static str {
        match self {
            Group::Conflicts => "Merge conflicts",
            Group::Staged => "Staged changes",
            Group::Changes => "Changes",
            Group::Untracked => "Untracked",
        }
    }

    /// Whether `entry` is listed in this group (a partly staged file is in
    /// both Staged and Changes).
    pub fn contains(self, entry: &FileStatus) -> bool {
        match self {
            Group::Conflicts => entry.is_conflicted(),
            Group::Staged => entry.is_staged(),
            Group::Changes => entry.is_unstaged(),
            Group::Untracked => entry.index == StatusCode::Untracked,
        }
    }
}

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A group header with its file count.
    Header(Group, usize),
    /// A file in a group.
    File(Group, FileStatus),
}

/// The rows for `entries`, skipping empty groups and the files of
/// collapsed ones. Files are sorted by path.
pub fn build_rows(entries: &[FileStatus], collapsed: &HashSet<Group>) -> Vec<Row> {
    let mut rows = Vec::new();
    for group in Group::ALL {
        let mut files: Vec<&FileStatus> = entries.iter().filter(|e| group.contains(e)).collect();
        if files.is_empty() {
            continue;
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        rows.push(Row::Header(group, files.len()));
        if !collapsed.contains(&group) {
            rows.extend(files.into_iter().map(|f| Row::File(group, f.clone())));
        }
    }
    rows
}

/// The status letter of `entry` in `group` and the theme color for it.
/// Letters are text, so they use the text-safe status colors (`info`
/// modified, `success` added or untracked, `error` deleted or conflicted;
/// docs/DESIGN.md 2.4 rule 3), never the `diff_*` bar colors.
pub fn status_letter(entry: &FileStatus, group: Group, c: &ThemeColors) -> (&'static str, Hsla) {
    let code = match group {
        Group::Conflicts => return ("!", c.error),
        Group::Untracked => return ("U", c.success),
        Group::Staged => entry.index,
        Group::Changes => entry.worktree,
    };
    code_letter(code, c)
}

/// The letter and text color of one status code (modified for anything
/// that is not added or deleted).
pub fn code_letter(code: StatusCode, c: &ThemeColors) -> (&'static str, Hsla) {
    match code {
        StatusCode::Added => ("A", c.success),
        StatusCode::Untracked => ("U", c.success),
        StatusCode::Deleted => ("D", c.error),
        StatusCode::Renamed => ("R", c.info),
        StatusCode::Copied => ("C", c.info),
        StatusCode::TypeChanged => ("T", c.info),
        _ => ("M", c.info),
    }
}

/// `name` and the folder it is in (`src/git`), for display.
pub fn split_path(path: &Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    (name, dir)
}

impl GitPanel {
    pub(super) fn render_header_row(
        &self,
        ix: usize,
        group: Group,
        count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = theme(cx).colors.clone();
        let collapsed = self.collapsed.contains(&group);
        let actions = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(30.))
            .flex()
            .items_center()
            .gap_0p5()
            .bg(c.hover)
            .invisible()
            .group_hover("git-row", |s| s.visible())
            .when(group == Group::Staged, |el| {
                el.child(
                    icon_button(("unstage-all", ix), IconName::Minus)
                        .size(px(20.))
                        .tooltip("Unstage all")
                        .on_click(cx.listener(|p, _, _, cx| {
                            cx.stop_propagation();
                            p.unstage_all(cx)
                        })),
                )
            })
            .when(matches!(group, Group::Changes | Group::Untracked), |el| {
                el.child(
                    icon_button(("discard-all", ix), IconName::Undo)
                        .size(px(20.))
                        .tooltip("Discard all changes…")
                        .on_click(cx.listener(move |p, _, window, cx| {
                            cx.stop_propagation();
                            p.confirm_discard_group(group, window, cx)
                        })),
                )
                .child(
                    icon_button(("stage-all", ix), IconName::Plus)
                        .size(px(20.))
                        .tooltip("Stage all")
                        .on_click(cx.listener(move |p, _, _, cx| {
                            cx.stop_propagation();
                            p.stage_group(group, cx)
                        })),
                )
            });
        let row = div()
            .id(("git-header", ix))
            .group("git-row")
            .relative()
            .w_full()
            .flex()
            .items_center()
            .gap_1p5()
            .h(SECTION_H)
            .px_2()
            .rounded(ROW_RADIUS)
            .cursor_pointer()
            .text_size(type_scale(cx).meta)
            .font_weight(WEIGHT_SEMIBOLD)
            .text_color(c.text)
            .hover(|s| s.bg(c.hover))
            .child(
                icon(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(px(14.))
                .color(c.text_muted),
            )
            .child(div().flex_1().child(group.title()))
            .child(actions)
            .child(badge(count.to_string()))
            .on_click(cx.listener(move |p, _, _, cx| p.toggle_group(group, cx)));
        crate::sidebar::inset(row)
    }

    pub(super) fn render_file_row(
        &self,
        ix: usize,
        group: Group,
        entry: &FileStatus,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = theme(cx).colors.clone();
        let (name, dir) = split_path(&entry.path);
        let (letter, letter_color) = status_letter(entry, group, &c);
        let (file_icon_name, icon_color) = file_icon(&name, &c);
        let path = entry.path.clone();
        let deleted = matches!(letter, "D");
        let p1 = path.clone();
        let p2 = path.clone();
        let p3 = path.clone();
        let actions = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(24.))
            .flex()
            .items_center()
            .gap_0p5()
            .bg(c.hover)
            .invisible()
            .group_hover("git-row", |s| s.visible())
            .when(!deleted, |el| {
                el.child(
                    icon_button(("open-file", ix), IconName::FileText)
                        .size(px(20.))
                        .tooltip("Open file")
                        .on_click(cx.listener(move |p, _, _, cx| {
                            cx.stop_propagation();
                            p.open_file(&p1, cx)
                        })),
                )
            })
            .when(matches!(group, Group::Changes | Group::Untracked), |el| {
                el.child(
                    icon_button(("discard", ix), IconName::Undo)
                        .size(px(20.))
                        .tooltip("Discard changes…")
                        .on_click(cx.listener(move |p, _, window, cx| {
                            cx.stop_propagation();
                            p.confirm_discard(vec![p2.clone()], window, cx)
                        })),
                )
            })
            .child(if group == Group::Staged {
                icon_button(("unstage", ix), IconName::Minus)
                    .size(px(20.))
                    .tooltip("Unstage")
                    .on_click(cx.listener(move |p, _, _, cx| {
                        cx.stop_propagation();
                        p.unstage_file(&p3, cx)
                    }))
            } else {
                icon_button(("stage", ix), IconName::Plus)
                    .size(px(20.))
                    .tooltip(if group == Group::Conflicts {
                        "Mark resolved (stage)"
                    } else {
                        "Stage"
                    })
                    .on_click(cx.listener(move |p, _, _, cx| {
                        cx.stop_propagation();
                        p.stage_file(&p3, cx)
                    }))
            });
        let tooltip: SharedString = entry.path.display().to_string().into();
        let row = div()
            .id(("git-file", ix))
            .group("git-row")
            .relative()
            .w_full()
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(ROW_HEIGHT))
            .pl(px(28.))
            .pr_2()
            .rounded(ROW_RADIUS)
            .cursor_pointer()
            .hover(|s| s.bg(c.hover))
            .child(icon(file_icon_name).size(icon_size::ROW).color(icon_color))
            // A long name ends in "…"; the folder after it gives way first.
            .child(crate::sidebar::name_then_hint(
                div()
                    .when(deleted, |el| el.line_through())
                    .child(truncated(name)),
                div()
                    .text_size(type_scale(cx).meta)
                    .text_color(c.text_muted)
                    .child(truncated(dir).start()),
            ))
            .child(actions)
            .child(
                div()
                    .w(px(12.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(crate::sidebar::git_letter(letter, letter_color, cx)),
            )
            .tooltip(move |_, cx| arcode_ui_kit::Tooltip::text(tooltip.clone(), cx))
            .on_click(cx.listener(move |p, _, _, cx| p.open_diff(&path, group, cx)));
        crate::sidebar::inset(row)
    }
}

/// A muted line of text in the list area.
pub fn note(text: impl Into<SharedString>, cx: &App) -> Div {
    let c = theme(cx).colors.clone();
    div()
        .px_3()
        .py_2()
        .text_size(type_scale(cx).body)
        .text_color(c.text_muted)
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn entry(path: &str, index: StatusCode, worktree: StatusCode) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            index,
            worktree,
            orig_path: None,
            conflict: None,
        }
    }

    #[test]
    fn rows_group_and_collapse() {
        use StatusCode::*;
        let entries = vec![
            entry("b.rs", Modified, Modified),
            entry("a.rs", Unmodified, Modified),
            entry("new.txt", Untracked, Untracked),
        ];
        let rows = build_rows(&entries, &HashSet::new());
        let headers: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Header(g, n) => Some((*g, *n)),
                _ => None,
            })
            .collect();
        assert_eq!(
            headers,
            [
                (Group::Staged, 1),
                (Group::Changes, 2),
                (Group::Untracked, 1)
            ]
        );
        assert!(matches!(&rows[3], Row::File(Group::Changes, e) if e.path == Path::new("a.rs")));
        let collapsed = HashSet::from([Group::Changes]);
        assert_eq!(build_rows(&entries, &collapsed).len(), 5);
        assert_eq!(
            split_path(Path::new("src/git/mod.rs")),
            ("mod.rs".into(), "src/git".into())
        );
    }
}
