//! The Problems tab: every diagnostic in the project, grouped by file.
//! Rows are rebuilt only when diagnostics change and rendered with a
//! `uniform_list`, so only the visible rows are laid out.

use std::path::{Path, PathBuf};

use arcode_editor::DiagnosticSeverity;
use arcode_lsp::DiagnosticStore;
use arcode_lsp::lsp_types::Position;
use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::file_icons::file_icon;
use crate::lsp_bridge::{LspBridge, convert};
use crate::project::relative_display;

const ROW_HEIGHT: f32 = 22.;

/// Emitted when the user picks a problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemsEvent {
    /// Open `path` and move the cursor to `position` (in `server`'s
    /// position encoding).
    Open {
        /// File to open.
        path: PathBuf,
        /// Server that reported the problem.
        server: String,
        /// Start of the problem.
        position: Position,
    },
}

/// One list row.
#[derive(Clone, Debug, PartialEq)]
pub enum ProblemRow {
    /// A file header with its problem count.
    File {
        /// Absolute path.
        path: PathBuf,
        /// Path relative to the project root.
        label: SharedString,
        /// Number of problems in the file.
        count: usize,
    },
    /// One diagnostic.
    Problem {
        /// Absolute path.
        path: PathBuf,
        /// Reporting server.
        server: String,
        /// Severity.
        severity: DiagnosticSeverity,
        /// First line of the message.
        message: SharedString,
        /// Source label (e.g. `rustc`).
        source: SharedString,
        /// Start position as reported.
        position: Position,
    },
}

/// Flattens the store into header and problem rows: files sorted by path,
/// problems within a file by severity (errors first), then position.
pub fn build_rows(store: &DiagnosticStore, root: Option<&Path>) -> Vec<ProblemRow> {
    let mut rows = Vec::new();
    for path in store.paths() {
        let mut entries = store.for_path(path);
        entries.sort_by_key(|(_, d)| {
            (
                convert::severity(d),
                d.range.start.line,
                d.range.start.character,
            )
        });
        let label = match root {
            Some(root) => relative_display(root, path),
            None => path.display().to_string(),
        };
        rows.push(ProblemRow::File {
            path: path.to_path_buf(),
            label: label.into(),
            count: entries.len(),
        });
        for (server, d) in entries {
            rows.push(ProblemRow::Problem {
                path: path.to_path_buf(),
                server: server.to_string(),
                severity: convert::severity(d),
                message: convert::first_line(&d.message).to_string().into(),
                source: arcode_lsp::source_label(server, d).to_string().into(),
                position: d.range.start,
            });
        }
    }
    rows
}

/// The Problems list view.
pub struct ProblemsPanel {
    bridge: Entity<LspBridge>,
    root: Option<PathBuf>,
    rows: Vec<ProblemRow>,
    count: usize,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _observe: Subscription,
}

impl EventEmitter<ProblemsEvent> for ProblemsPanel {}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ProblemsPanel {
    /// Creates the panel; it follows `bridge`'s diagnostics.
    pub fn new(bridge: Entity<LspBridge>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&bridge, |panel, _, cx| panel.rebuild(cx));
        let mut panel = Self {
            bridge,
            root: None,
            rows: Vec::new(),
            count: 0,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _observe: observe,
        };
        panel.rebuild(cx);
        panel
    }

    /// Sets the folder file paths are shown relative to.
    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.root = root;
        self.rebuild(cx);
    }

    /// Number of errors and warnings (the tab badge; hints and infos are
    /// listed but not counted).
    pub fn count(&self) -> usize {
        self.count
    }

    /// The rows as displayed.
    pub fn rows(&self) -> &[ProblemRow] {
        &self.rows
    }

    /// Opens the problem at row `ix` (headers are ignored).
    pub fn open_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        if let Some(ProblemRow::Problem {
            path,
            server,
            position,
            ..
        }) = self.rows.get(ix)
        {
            cx.emit(ProblemsEvent::Open {
                path: path.clone(),
                server: server.clone(),
                position: *position,
            });
        }
        cx.notify();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let store = self.bridge.read(cx).store();
        let rows = build_rows(store, self.root.as_deref());
        // The badge counts what needs attention: errors and warnings.
        let counts = store.counts();
        let count = counts.errors + counts.warnings;
        if rows != self.rows || count != self.count {
            self.rows = rows;
            self.count = count;
            self.selected = None;
            cx.notify();
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let base = div()
            .id(ix)
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(ROW_HEIGHT))
            .px_2()
            .w_full()
            .overflow_hidden()
            .whitespace_nowrap();
        match row {
            ProblemRow::File { label, count, .. } => {
                let (file_icon_name, file_icon_color) = file_icon(label, &c);
                base.child(icon(file_icon_name).size(px(13.)).color(file_icon_color))
                    .child(div().text_color(c.text).child(label.clone()))
                    .child(
                        div()
                            .px_1p5()
                            .rounded_full()
                            .bg(c.surface_raised)
                            .text_xs()
                            .text_color(c.text_muted)
                            .child(count.to_string()),
                    )
                    .into_any_element()
            }
            ProblemRow::Problem {
                severity,
                message,
                source,
                position,
                ..
            } => {
                let selected = self.selected == Some(ix);
                base.pl(px(26.))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(c.selection))
                    .hover(|s| s.bg(c.hover))
                    .child(
                        icon(severity.icon())
                            .size(px(13.))
                            .color(severity.color(&c)),
                    )
                    .child(
                        div()
                            .flex_shrink()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(c.text)
                            .child(message.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(c.text_muted)
                            .child(source.clone()),
                    )
                    .child(div().flex_none().text_color(c.text_disabled).child(format!(
                        "[Ln {}, Col {}]",
                        position.line + 1,
                        position.character + 1
                    )))
                    .on_click(cx.listener(move |panel, _, _, cx| panel.open_row(ix, cx)))
                    .into_any_element()
            }
        }
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let root = div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .text_sm();
        if self.rows.is_empty() {
            return root
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(c.text_muted)
                .child(
                    icon(IconName::CircleCheck)
                        .size(px(24.))
                        .color(c.text_disabled),
                )
                .child("No problems");
        }
        root.py_1().child(
            uniform_list(
                "problems",
                self.rows.len(),
                cx.processor(|panel, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| panel.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_lsp::lsp_types::{Diagnostic, DiagnosticSeverity as Sev, Range};

    fn diag(line: u32, sev: Sev, msg: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, 4), Position::new(line, 9)),
            severity: Some(sev),
            source: Some("rustc".into()),
            message: msg.into(),
            ..Default::default()
        }
    }

    #[test]
    fn rows_are_grouped_by_file() {
        let mut store = DiagnosticStore::new();
        let root = PathBuf::from("/p");
        store.update(
            "ra",
            root.join("src/main.rs"),
            vec![
                diag(3, Sev::WARNING, "unused\nmore detail"),
                diag(1, Sev::ERROR, "mismatched types"),
            ],
        );
        store.update("ra", root.join("src/a.rs"), vec![diag(0, Sev::HINT, "h")]);
        let rows = build_rows(&store, Some(&root));
        assert_eq!(rows.len(), 5);
        assert!(
            matches!(&rows[0], ProblemRow::File { label, count: 1, .. } if label == "src/a.rs")
        );
        assert!(
            matches!(&rows[2], ProblemRow::File { label, count: 2, .. } if label == "src/main.rs")
        );
        match &rows[3] {
            ProblemRow::Problem {
                severity,
                message,
                source,
                position,
                ..
            } => {
                assert_eq!(*severity, DiagnosticSeverity::Error);
                assert_eq!(message.as_ref(), "mismatched types");
                assert_eq!(source.as_ref(), "rustc");
                assert_eq!(*position, Position::new(1, 4));
            }
            other => panic!("unexpected row {other:?}"),
        }
        assert!(
            matches!(&rows[4], ProblemRow::Problem { message, .. } if message.as_ref() == "unused")
        );
        assert!(build_rows(&DiagnosticStore::new(), None).is_empty());
    }

    #[test]
    fn problems_sort_by_severity_then_position() {
        let mut store = DiagnosticStore::new();
        let path = PathBuf::from("/p/a.rs");
        store.update(
            "ra",
            path.clone(),
            vec![
                diag(1, Sev::HINT, "hint"),
                diag(2, Sev::WARNING, "warn late"),
                diag(9, Sev::ERROR, "error"),
                diag(0, Sev::WARNING, "warn early"),
            ],
        );
        let messages: Vec<String> = build_rows(&store, None)
            .iter()
            .filter_map(|r| match r {
                ProblemRow::Problem { message, .. } => Some(message.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(messages, ["error", "warn early", "warn late", "hint"]);
    }
}
