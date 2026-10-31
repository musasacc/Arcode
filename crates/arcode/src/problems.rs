//! The Problems tab: every diagnostic in the project, grouped by file.
//! Rows are rebuilt only when diagnostics change and rendered with a
//! `uniform_list`, so only the visible rows are laid out.

use std::path::{Path, PathBuf};

use arcode_editor::DiagnosticSeverity;
use arcode_lsp::DiagnosticStore;
use arcode_lsp::lsp_types::Position;
use arcode_ui_kit::metrics::{ROW_INSET, WEIGHT_SEMIBOLD, type_scale};
use arcode_ui_kit::{IconName, badge, icon, readout, spacer, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::file_icons::file_icon;
use crate::lsp_bridge::{LspBridge, convert};
use crate::project::relative_display;

/// Row height (docs/DESIGN.md 6.8).
const ROW_HEIGHT: f32 = 26.;

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

/// Base pause before a low-end rebuild (tripled by `debounce`).
const REBUILD_DEBOUNCE_MS: u64 = 150;

/// The Problems list view.
pub struct ProblemsPanel {
    bridge: Entity<LspBridge>,
    root: Option<PathBuf>,
    rows: Vec<ProblemRow>,
    count: usize,
    /// Some of the counted problems are errors.
    errors: bool,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    /// A rebuild waiting for the low-end debounce.
    pending: Option<gpui::Task<()>>,
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
        let observe = cx.observe(&bridge, |panel, _, cx| panel.schedule_rebuild(cx));
        let mut panel = Self {
            bridge,
            root: None,
            rows: Vec::new(),
            count: 0,
            errors: false,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            pending: None,
            _observe: observe,
        };
        panel.rebuild(cx);
        panel
    }

    /// Rebuilds at once, or after a pause in low-end mode (several
    /// diagnostics updates then cost one rebuild).
    fn schedule_rebuild(&mut self, cx: &mut Context<Self>) {
        if !arcode_ui_kit::low_end(cx) {
            self.rebuild(cx);
            return;
        }
        if self.pending.is_some() {
            return;
        }
        let delay = arcode_ui_kit::debounce(REBUILD_DEBOUNCE_MS, cx);
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |panel, cx| {
                panel.pending = None;
                panel.rebuild(cx);
            })
            .ok();
        }));
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

    /// Whether any problem is an error (the badge turns `error`).
    pub fn has_errors(&self) -> bool {
        self.errors
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
        let errors = counts.errors > 0;
        if rows != self.rows || count != self.count || errors != self.errors {
            self.rows = rows;
            self.count = count;
            self.errors = errors;
            self.selected = None;
            cx.notify();
        }
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let base = div()
            .id(ix)
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .h(px(ROW_HEIGHT))
            .px(px(8.))
            .rounded(px(7.))
            .overflow_hidden()
            .whitespace_nowrap();
        let chip = match row {
            ProblemRow::File { label, count, .. } => {
                let (dir, name) = split_label(label);
                let (file_icon_name, file_icon_color) = file_icon(name, &c);
                base.gap(px(6.))
                    .text_size(scale.meta)
                    .child(
                        icon(IconName::ChevronDown)
                            .size(px(13.))
                            .color(c.text_muted),
                    )
                    .child(icon(file_icon_name).size(px(14.)).color(file_icon_color))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(WEIGHT_SEMIBOLD)
                            .text_color(c.text)
                            .child(SharedString::from(name.to_string())),
                    )
                    .when(!dir.is_empty(), |el| {
                        el.child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(c.text_muted)
                                .child(SharedString::from(dir.to_string())),
                        )
                    })
                    .child(badge(count.to_string()))
            }
            ProblemRow::Problem {
                severity,
                message,
                source,
                position,
                ..
            } => {
                let selected = self.selected == Some(ix);
                base.pl(px(36.))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(c.pill))
                    .when(!selected, |el| el.hover(|s| s.bg(c.hover)))
                    .child(
                        icon(severity.icon())
                            .size(px(14.))
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
                            .text_size(scale.meta)
                            .text_color(c.text_muted)
                            .child(source.clone()),
                    )
                    .child(spacer())
                    .child(readout(format!(
                        "{}:{}",
                        position.line + 1,
                        position.character + 1
                    )))
                    .on_click(cx.listener(move |panel, _, _, cx| panel.open_row(ix, cx)))
            }
        };
        // Rows are chips inset from the panel edges (docs/DESIGN.md 5.3).
        div()
            .flex()
            .w_full()
            .px(ROW_INSET)
            .child(chip)
            .into_any_element()
    }
}

/// A file row's label split into its folder and name (`src`, `main.rs`).
fn split_label(label: &str) -> (&str, &str) {
    match label.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", label),
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
            .text_size(type_scale(cx).body);
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
#[path = "problems_tests.rs"]
mod tests;
