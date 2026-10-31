//! Editors the debugger decorates: breakpoint dots, run buttons and the
//! stopped line, and what clicking them does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_editor::{
    ColorKey, DotStyle, Editor, EditorEvent, GutterDecoration, GutterLane, GutterMark,
};
use arcode_project::Runnable;
use arcode_ui_kit::{IconName, settings};
use gpui::{AppContext, Context, Entity, Subscription, Task, WeakEntity};

use super::breakpoints::BpKind;
use super::{Debugger, DebuggerEvent};

/// Gutter set of breakpoint dots.
pub const BREAKPOINT_SET: &str = "debugger-breakpoints";
/// Gutter set of run buttons.
pub const RUN_SET: &str = "debugger-run";
/// Gutter and highlight set of the stopped line.
pub const STOPPED_SET: &str = "debugger-stopped";
/// Popover key of the run menu.
pub const RUN_MENU: &str = "debugger-run-menu";
/// Wait after typing before run buttons are recomputed.
const RUNNABLES_DELAY: Duration = Duration::from_millis(400);

/// An editor the debugger follows.
pub(super) struct TrackedEditor {
    pub editor: WeakEntity<Editor>,
    pub runnables: Vec<Runnable>,
    /// The text (`line_follow::line_key`) of each breakpoint's line (and of the
    /// stopped line) as of the last edit, to find moved lines.
    pub keys: HashMap<u64, String>,
    task: Option<Task<()>>,
    _sub: Subscription,
}

impl Debugger {
    /// Starts decorating `editor` (breakpoints, run buttons).
    pub fn register_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.editors.retain(|t| t.editor.upgrade().is_some());
        if self.editors.iter().any(|t| t.editor == editor.downgrade()) {
            return;
        }
        let sub = cx.subscribe(editor, |d, editor, event: &EditorEvent, cx| {
            d.on_editor_event(&editor, event, cx)
        });
        self.editors.push(TrackedEditor {
            editor: editor.downgrade(),
            runnables: Vec::new(),
            keys: HashMap::new(),
            task: None,
            _sub: sub,
        });
        self.refresh_editor(editor, cx);
        self.schedule_runnables(editor, Duration::ZERO, cx);
    }

    pub(super) fn tracked(&mut self, editor: &Entity<Editor>) -> Option<&mut TrackedEditor> {
        let weak = editor.downgrade();
        self.editors.iter_mut().find(|t| t.editor == weak)
    }

    /// Live editors with their paths.
    pub(super) fn open_editors(&self, cx: &gpui::App) -> Vec<(Entity<Editor>, PathBuf)> {
        self.editors
            .iter()
            .filter_map(|t| {
                let editor = t.editor.upgrade()?;
                let path = editor.read(cx).path()?.to_path_buf();
                Some((editor, path))
            })
            .collect()
    }

    fn on_editor_event(
        &mut self,
        editor: &Entity<Editor>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Edited { edits, .. } => {
                self.follow_edits(editor, edits, cx);
                editor.update(cx, |e, cx| {
                    e.hide_popover(super::editors::RUN_MENU, cx);
                });
                self.schedule_runnables(editor, RUNNABLES_DELAY, cx);
            }
            EditorEvent::Saved => {
                let path = editor.read(cx).path().map(Path::to_path_buf);
                if let (Some(path), Some(root)) = (path, self.root.clone())
                    && is_project_config(&path, &root)
                {
                    self.reload_choices(cx);
                }
            }
            EditorEvent::SelectionsChanged => {
                editor.update(cx, |e, cx| {
                    e.hide_popover(RUN_MENU, cx);
                });
            }
            EditorEvent::GutterClicked {
                line, lane, tag, ..
            } => match lane {
                GutterLane::Breakpoints => {
                    if let Some(path) = editor.read(cx).path().map(Path::to_path_buf) {
                        self.toggle_breakpoint(&path, *line, cx);
                    }
                }
                GutterLane::Runnables => {
                    if let Some(ix) = tag {
                        self.show_run_menu(editor, *ix as usize, cx);
                    }
                }
                GutterLane::Changes => {}
            },
            EditorEvent::GutterContextClicked { line, lane, .. } => {
                if *lane == GutterLane::Breakpoints
                    && let Some(path) = editor.read(cx).path().map(Path::to_path_buf)
                {
                    cx.emit(DebuggerEvent::EditBreakpoint { path, line: *line });
                }
            }
            _ => {}
        }
    }

    /// Redraws the breakpoints of `path` in its editors, sends them to the
    /// session and asks for the session to be saved.
    pub(super) fn breakpoints_changed(&mut self, path: &Path, cx: &mut Context<Self>) {
        for (editor, p) in self.open_editors(cx) {
            if p == path {
                self.refresh_editor(&editor, cx);
            }
        }
        self.send_breakpoints(path, cx);
        cx.emit(DebuggerEvent::BreakpointsChanged);
        cx.notify();
    }

    /// Redraws every editor's marks.
    pub(super) fn refresh_all_editors(&mut self, cx: &mut Context<Self>) {
        for (editor, _) in self.open_editors(cx) {
            self.refresh_editor(&editor, cx);
        }
    }

    /// Sets the breakpoint, run-button and stopped-line marks of `editor`.
    pub(super) fn refresh_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let path = editor.read(cx).path().map(Path::to_path_buf);
        let language = editor.read(cx).language().id;
        let debuggable = !arcode_dap::adapters_for_language(language).is_empty();
        let session_live = self.session.as_ref().is_some_and(|s| !s.is_terminated());
        let stopped = self
            .stopped_at
            .as_ref()
            .filter(|(p, _)| Some(p) == path.as_ref())
            .map(|(_, line)| *line);
        let marks: Vec<GutterDecoration> = path
            .as_deref()
            .map(|path| {
                self.book
                    .in_file(path)
                    // The paused-frame dot replaces the dot on its line.
                    .filter(|bp| Some(bp.line) != stopped)
                    .map(|bp| {
                        let unverified = session_live && bp.verified == Some(false);
                        let (color, style) = match (bp.enabled && !unverified, bp.kind()) {
                            (false, _) => ("text_muted", DotStyle::Ring),
                            (true, BpKind::Plain) => ("error", DotStyle::Filled),
                            (true, BpKind::Conditional) => ("error", DotStyle::Conditional),
                            (true, BpKind::LogPoint) => ("info", DotStyle::Conditional),
                        };
                        GutterDecoration {
                            line: bp.line,
                            lane: GutterLane::Breakpoints,
                            mark: GutterMark::Dot {
                                color: ColorKey::new(color),
                                style,
                            },
                            tag: bp.id,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let run_marks: Vec<GutterDecoration> = if settings(cx).debug.run_buttons {
            self.tracked(editor)
                .map(|t| {
                    t.runnables
                        .iter()
                        .enumerate()
                        .map(|(ix, r)| GutterDecoration {
                            line: r.line,
                            lane: GutterLane::Runnables,
                            mark: GutterMark::Icon {
                                icon: IconName::Play,
                                color: ColorKey::new("success"),
                            },
                            tag: ix as u64,
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if let Some(path) = &path {
            self.remember_lines(editor, path, cx);
        }
        let paused = stopped.map(|line| {
            let buffer = editor.read(cx).buffer();
            let text = if line < buffer.line_count() {
                buffer.line(line).to_string()
            } else {
                String::new()
            };
            self.paused_line(line, &text)
        });
        editor.update(cx, |e, cx| {
            e.set_paused_line(paused, cx);
            let show_lane = path.is_some() && (debuggable || !marks.is_empty());
            e.set_gutter_lane_visible(GutterLane::Breakpoints, show_lane, cx);
            if e.gutter_decorations(BREAKPOINT_SET) != marks.as_slice() {
                e.set_gutter_decorations(BREAKPOINT_SET, marks, cx);
            }
            if e.gutter_decorations(RUN_SET) != run_marks.as_slice() {
                e.set_gutter_decorations(RUN_SET, run_marks, cx);
            }
            match stopped {
                Some(line) => {
                    let arrow = vec![GutterDecoration {
                        line,
                        lane: GutterLane::Breakpoints,
                        // DESIGN.md 6.7: always the `error` circle, on a
                        // breakpoint and after a step alike.
                        mark: GutterMark::Dot {
                            color: ColorKey::new("error"),
                            style: DotStyle::PausedFrame,
                        },
                        tag: u64::MAX,
                    }];
                    // The editor draws the paused band from the paused line.
                    if e.gutter_decorations(STOPPED_SET) != arrow.as_slice() {
                        e.set_gutter_decorations(STOPPED_SET, arrow, cx);
                    }
                }
                None => {
                    if !e.gutter_decorations(STOPPED_SET).is_empty() {
                        e.clear_decorations(STOPPED_SET, cx);
                    }
                }
            }
        });
    }

    /// Recomputes the run buttons of `editor` after `delay`.
    fn schedule_runnables(
        &mut self,
        editor: &Entity<Editor>,
        delay: Duration,
        cx: &mut Context<Self>,
    ) {
        let e = editor.read(cx);
        let language = e.language();
        let config = settings(cx);
        let eligible = match (self.root.clone(), e.path().map(Path::to_path_buf)) {
            (Some(root), Some(path))
                if config.debug.run_buttons
                    && !e.is_large_file()
                    && config.language_enabled(language.id)
                    && arcode_project::RUNNABLE_LANGUAGES.contains(&language.id)
                    && path.starts_with(&root) =>
            {
                Some((root, path))
            }
            _ => None,
        };
        let Some((root, path)) = eligible else {
            // No run buttons for this file (any more): drop the ones found
            // before (a disabled language pack, another language, a file
            // moved out of the project).
            let cleared = self.tracked(editor).is_some_and(|t| {
                t.task = None;
                !std::mem::take(&mut t.runnables).is_empty()
            });
            if cleared {
                self.refresh_editor(editor, cx);
            }
            return;
        };
        let text = editor.read(cx).buffer().text().clone();
        let weak = editor.downgrade();
        let task = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let found = cx
                .background_spawn(async move {
                    super::runnables::find_runnables(language, &text, &path, &root)
                })
                .await;
            this.update(cx, |d, cx| {
                let Some(editor) = weak.upgrade() else {
                    return;
                };
                if let Some(t) = d.tracked(&editor) {
                    if t.runnables == found {
                        return;
                    }
                    t.runnables = found;
                }
                d.refresh_editor(&editor, cx);
            })
            .ok();
        });
        if let Some(t) = self.tracked(editor) {
            t.task = Some(task);
        }
    }

    /// The runnable behind run button `ix` of `editor`.
    pub fn runnable(&self, editor: &Entity<Editor>, ix: usize) -> Option<Runnable> {
        let weak = editor.downgrade();
        self.editors
            .iter()
            .find(|t| t.editor == weak)?
            .runnables
            .get(ix)
            .cloned()
    }

    /// Files changed on disk (the project watcher): `launch.toml` or a
    /// package file may have changed (`overflow`: anything may have).
    pub fn paths_changed(&mut self, paths: &[PathBuf], overflow: bool, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if overflow || paths.iter().any(|p| is_project_config(p, &root)) {
            self.reload_choices(cx);
        }
    }

    /// The language of `editor` changed (picked by hand, a file
    /// association, a rename): its run buttons and breakpoint lane are
    /// computed again for the new language.
    pub fn language_changed(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.schedule_runnables(editor, Duration::ZERO, cx);
        self.refresh_editor(editor, cx);
    }

    /// Settings changed: run buttons may have been turned on or off.
    pub fn settings_changed(&mut self, cx: &mut Context<Self>) {
        for (editor, _) in self.open_editors(cx) {
            self.schedule_runnables(&editor, Duration::ZERO, cx);
            self.refresh_editor(&editor, cx);
        }
    }
}

/// Whether saving `path` can change the launch configurations.
fn is_project_config(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel == Path::new(arcode_dap::LAUNCH_FILE)
        || matches!(
            rel.to_str(),
            Some(
                "Cargo.toml"
                    | "package.json"
                    | "go.mod"
                    | "pyproject.toml"
                    | "Makefile"
                    | "justfile"
            )
        )
}
