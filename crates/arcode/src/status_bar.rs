//! The status bar: branch, language, LSP status, cursor position,
//! diagnostic counts and (optionally) startup time and memory use.

use std::time::{Duration, Instant};

use arcode_ui_kit::{IconName, icon, settings, theme};
use gpui::{
    App, Context, Global, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window,
    div, prelude::*, px,
};

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
}

/// The status bar view.
pub struct StatusBar {
    info: StatusInfo,
    /// Resident memory in MiB, when perf stats are on.
    memory_mib: Option<u64>,
    _memory_task: Option<Task<()>>,
}

impl StatusBar {
    /// Creates the status bar; starts the memory sampler if
    /// `ui.show_perf_stats` is on.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let show = settings(cx).ui.show_perf_stats;
        let mut bar = Self {
            info: StatusInfo::default(),
            memory_mib: None,
            _memory_task: None,
        };
        if show {
            bar.memory_mib = sample_memory();
            bar._memory_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(MEMORY_REFRESH).await;
                    let sample = sample_memory();
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
        bar
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

    /// Sets the language server status text.
    pub fn set_lsp_status(&mut self, status: Option<SharedString>, cx: &mut Context<Self>) {
        let info = StatusInfo {
            lsp_status: status,
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

/// `Ln 12, Col 4` from a zero-based position.
pub fn cursor_label(line: usize, column: usize) -> String {
    format!("Ln {}, Col {}", line + 1, column + 1)
}

fn item(content: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .h_full()
        .child(content)
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let info = &self.info;
        let perf = settings(cx).ui.show_perf_stats;
        let ready = cx.try_global::<Startup>().and_then(|s| s.ready);
        let icon_size = px(12.);
        let left =
            div()
                .flex()
                .items_center()
                .h_full()
                .min_w_0()
                .when_some(info.branch.clone(), |el, branch| {
                    el.child(
                        item(
                            icon(IconName::GitBranch)
                                .size(icon_size)
                                .color(c.text_muted),
                        )
                        .child(branch),
                    )
                })
                .child(
                    item(
                        icon(IconName::CircleX)
                            .size(icon_size)
                            .color(if info.errors > 0 {
                                c.error
                            } else {
                                c.text_muted
                            }),
                    )
                    .id("status-errors")
                    .child(info.errors.to_string())
                    .child(icon(IconName::TriangleAlert).size(icon_size).color(
                        if info.warnings > 0 {
                            c.warning
                        } else {
                            c.text_muted
                        },
                    ))
                    .child(info.warnings.to_string()),
                );
        let right = div()
            .flex()
            .items_center()
            .h_full()
            .flex_none()
            .when_some(info.cursor, |el, (line, col)| {
                el.child(item(cursor_label(line, col)))
            })
            .when_some(info.language.clone(), |el, lang| el.child(item(lang)))
            .when_some(info.lsp_status.clone(), |el, status| {
                el.child(item(status).text_color(c.text_muted))
            })
            .when(perf, |el| {
                el.when_some(ready, |el, ready| {
                    el.child(
                        item(format!("ready in {} ms", ready.as_millis())).text_color(c.text_muted),
                    )
                })
                .when_some(self.memory_mib, |el, mib| {
                    el.child(item(format!("{mib} MB")).text_color(c.text_muted))
                })
            });
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(24.))
            .px_1()
            .bg(c.status_bar)
            .border_t_1()
            .border_color(c.border)
            .text_xs()
            .text_color(c.text_muted)
            .child(left)
            .child(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_label_is_one_based() {
        assert_eq!(cursor_label(0, 0), "Ln 1, Col 1");
        assert_eq!(cursor_label(11, 3), "Ln 12, Col 4");
    }
}
