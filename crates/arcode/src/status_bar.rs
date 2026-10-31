//! The status bar, drawn as the footer (see [`footer`]): branch, language,
//! LSP status, cursor position, diagnostic counts, the last run while the
//! bottom panel is hidden and (optionally) startup time and memory use.

mod footer;

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use arcode_ui_kit::settings;
use gpui::{App, AppContext, Context, Entity, Global, SharedString, Task};

use crate::terminal_panel::TerminalPanel;

pub use footer::{item as footer_item, welcome_footer};

/// How often the memory figure may refresh.
const MEMORY_REFRESH: Duration = Duration::from_secs(2);

/// Process start and the time the first window was ready.
pub struct Startup {
    /// When `main` started.
    pub started: Instant,
    /// Time from start until the workspace first rendered.
    pub ready: Option<Duration>,
}

impl Global for Startup {}

/// Records the first render, once.
pub fn mark_ready(cx: &mut App) {
    if let Some(startup) = cx.try_global::<Startup>()
        && startup.ready.is_none()
    {
        let ready = startup.started.elapsed();
        log::info!("ready in {} ms", ready.as_millis());
        cx.global_mut::<Startup>().ready = Some(ready);
    }
}

/// Everything shown on the left/middle of the status bar. The workspace
/// fills the editor fields; LSP wiring fills `lsp_status` and the counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusInfo {
    /// Current git branch.
    pub branch: Option<SharedString>,
    /// Language of the active editor.
    pub language: Option<SharedString>,
    /// Language server state, e.g. `rust-analyzer: ready`.
    pub lsp_status: Option<SharedString>,
    /// Primary cursor of the active editor (zero-based line, column).
    pub cursor: Option<(usize, usize)>,
    /// Number of errors in the project.
    pub errors: usize,
    /// Number of warnings in the project.
    pub warnings: usize,
    /// Merge conflicts in the active editor: the one at the cursor (if
    /// any) and the total.
    pub conflicts: Option<(Option<usize>, usize)>,
}

/// The status bar view.
pub struct StatusBar {
    info: StatusInfo,
    /// Resident memory in MiB, when perf stats are on.
    memory_mib: Option<u64>,
    _memory_task: Option<Task<()>>,
    _settings_sub: Option<gpui::Subscription>,
    /// Shown instead of `lsp_status` when set: one entry per language
    /// server with a menu (see `lsp_bridge::status_view`).
    lsp_view: Option<gpui::AnyView>,
    /// Vim mode of the active editor, shown first.
    vim: Option<arcode_editor::VimStatus>,
    /// Indentation, encoding and line ending of the active editor.
    doc: Option<crate::languages::status_items::DocStatus>,
    /// Plugin status items and task chips (see `plugins::status`).
    plugin_view: Option<gpui::AnyView>,
    /// The terminals whose last run is shown, and whether the bottom panel
    /// is hidden (the workspace sets it as it draws; the footer shows the
    /// run only then).
    run_source: Option<(Entity<TerminalPanel>, Rc<Cell<bool>>)>,
}

impl StatusBar {
    /// Creates the status bar; runs the memory sampler while
    /// `ui.show_perf_stats` is on (it follows the setting).
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut bar = Self {
            info: StatusInfo::default(),
            memory_mib: None,
            _memory_task: None,
            _settings_sub: None,
            lsp_view: None,
            vim: None,
            doc: None,
            plugin_view: None,
            run_source: None,
        };
        bar.sync_sampler(cx);
        bar._settings_sub =
            Some(cx.observe_global::<arcode_ui_kit::Settings>(|bar, cx| bar.sync_sampler(cx)));
        bar
    }

    /// Starts or stops the memory sampler to match the setting. Samples are
    /// read on the background executor; the bar redraws only when the shown
    /// value (whole MiB) changes.
    fn sync_sampler(&mut self, cx: &mut Context<Self>) {
        let show = settings(cx).ui.show_perf_stats;
        if show == self._memory_task.is_some() {
            return;
        }
        if !show {
            self._memory_task = None;
            if self.memory_mib.take().is_some() {
                cx.notify();
            }
            return;
        }
        self._memory_task = Some(cx.spawn(async move |this, cx| {
            let mut first = true;
            loop {
                if !first {
                    cx.background_executor().timer(MEMORY_REFRESH).await;
                }
                first = false;
                let sample = cx.background_spawn(async { sample_memory() }).await;
                let alive = this.update(cx, |bar, cx| {
                    if bar.memory_mib != sample {
                        bar.memory_mib = sample;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    /// The memory figure shown (MiB), if any.
    pub fn memory_mib(&self) -> Option<u64> {
        self.memory_mib
    }

    /// The displayed information.
    pub fn info(&self) -> &StatusInfo {
        &self.info
    }

    /// Replaces the displayed information (redraws only on change).
    pub fn set_info(&mut self, info: StatusInfo, cx: &mut Context<Self>) {
        if self.info != info {
            self.info = info;
            cx.notify();
        }
    }

    /// Updates the editor-derived fields, keeping LSP status and counts.
    pub fn set_editor_info(
        &mut self,
        branch: Option<SharedString>,
        language: Option<SharedString>,
        cursor: Option<(usize, usize)>,
        cx: &mut Context<Self>,
    ) {
        let info = StatusInfo {
            branch,
            language,
            cursor,
            ..self.info.clone()
        };
        self.set_info(info, cx);
    }

    /// The indentation / encoding / line ending entries shown, if any.
    pub fn doc_status(&self) -> Option<&crate::languages::status_items::DocStatus> {
        self.doc.as_ref()
    }

    /// Sets the indentation / encoding / line ending entries (`None`
    /// without an active editor).
    pub fn set_doc_status(
        &mut self,
        doc: Option<crate::languages::status_items::DocStatus>,
        cx: &mut Context<Self>,
    ) {
        if self.doc != doc {
            self.doc = doc;
            cx.notify();
        }
    }

    /// The Vim mode entry shown, if any.
    pub fn vim_status(&self) -> Option<&arcode_editor::VimStatus> {
        self.vim.as_ref()
    }

    /// Sets the Vim mode entry (`None` outside Vim mode).
    pub fn set_vim_status(
        &mut self,
        vim: Option<arcode_editor::VimStatus>,
        cx: &mut Context<Self>,
    ) {
        if self.vim != vim {
            self.vim = vim;
            cx.notify();
        }
    }

    /// Shows `view` (the language server entries) instead of the status
    /// text.
    pub fn set_lsp_view(&mut self, view: gpui::AnyView, cx: &mut Context<Self>) {
        self.lsp_view = Some(view);
        cx.notify();
    }

    /// Shows `view` (plugin status items and task chips) after the counts.
    pub fn set_plugin_view(&mut self, view: gpui::AnyView, cx: &mut Context<Self>) {
        self.plugin_view = Some(view);
        cx.notify();
    }

    /// Shows the last run of `terminals` while `panel_hidden` holds.
    pub fn set_run_source(
        &mut self,
        terminals: Entity<TerminalPanel>,
        panel_hidden: Rc<Cell<bool>>,
    ) {
        self.run_source = Some((terminals, panel_hidden));
    }

    /// Sets the language server status text.
    pub fn set_lsp_status(&mut self, status: Option<SharedString>, cx: &mut Context<Self>) {
        let info = StatusInfo {
            lsp_status: status,
            ..self.info.clone()
        };
        self.set_info(info, cx);
    }

    /// Sets the merge conflict counter of the active editor.
    pub fn set_conflicts(
        &mut self,
        conflicts: Option<(Option<usize>, usize)>,
        cx: &mut Context<Self>,
    ) {
        let info = StatusInfo {
            conflicts,
            ..self.info.clone()
        };
        self.set_info(info, cx);
    }

    /// Sets the project-wide diagnostic counts.
    pub fn set_diagnostic_counts(
        &mut self,
        errors: usize,
        warnings: usize,
        cx: &mut Context<Self>,
    ) {
        let info = StatusInfo {
            errors,
            warnings,
            ..self.info.clone()
        };
        self.set_info(info, cx);
    }
}

fn sample_memory() -> Option<u64> {
    memory_stats::memory_stats().map(|m| (m.physical_mem / (1024 * 1024)) as u64)
}

/// `Conflict 2 of 3` at a conflict, `3 conflicts` elsewhere.
pub fn conflict_label(current: Option<usize>, total: usize) -> String {
    match current {
        Some(ix) => format!("Conflict {} of {total}", ix + 1),
        None if total == 1 => "1 conflict".into(),
        None => format!("{total} conflicts"),
    }
}

/// `Ln 12, Col 4` from a zero-based position.
pub fn cursor_label(line: usize, column: usize) -> String {
    format!("Ln {}, Col {}", line + 1, column + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_label_is_one_based() {
        assert_eq!(cursor_label(0, 0), "Ln 1, Col 1");
        assert_eq!(cursor_label(11, 3), "Ln 12, Col 4");
    }

    #[test]
    fn conflict_labels() {
        assert_eq!(conflict_label(Some(1), 3), "Conflict 2 of 3");
        assert_eq!(conflict_label(None, 1), "1 conflict");
        assert_eq!(conflict_label(None, 4), "4 conflicts");
    }
}
