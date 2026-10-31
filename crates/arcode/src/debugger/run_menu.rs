//! The small Run / Debug menu opened by a gutter run button.

use arcode_ui_kit::Elevated;
use std::path::Path;

use arcode_buffer::Point;
use arcode_editor::{Editor, EditorPopover};
use arcode_project::Runnable;
use arcode_ui_kit::{IconName, icon, theme};
use gpui::{
    AppContext, Context, Entity, IntoElement, MouseButton, Render, WeakEntity, Window, div,
    prelude::*, px,
};

use super::editors::RUN_MENU;
use super::launch::LaunchChoice;
use super::{Debugger, DebuggerEvent};

/// The menu's content.
pub struct RunMenu {
    runnable: Runnable,
    debugger: WeakEntity<Debugger>,
    editor: WeakEntity<Editor>,
    selected: usize,
}

impl Debugger {
    /// Opens the run menu of run button `ix` in `editor`.
    pub(super) fn show_run_menu(
        &mut self,
        editor: &Entity<Editor>,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(runnable) = self.runnable(editor, ix) else {
            return;
        };
        if editor.read(cx).has_popover(RUN_MENU) {
            editor.update(cx, |e, cx| {
                e.hide_popover(RUN_MENU, cx);
            });
            return;
        }
        let line = runnable.line;
        let debugger = cx.entity().downgrade();
        let menu = cx.new(|_| RunMenu {
            runnable,
            debugger,
            editor: editor.downgrade(),
            selected: 0,
        });
        let keys = menu.downgrade();
        let popover = EditorPopover::new(Point::new(line, 0), menu.into()).on_key(
            move |_, keystroke, window, cx| {
                keys.update(cx, |m, cx| m.key(&keystroke.key, window, cx))
                    .unwrap_or(false)
            },
        );
        editor.update(cx, |e, cx| e.show_popover(RUN_MENU, popover, cx));
    }

    /// Runs a runnable in a terminal named after it, or debugs it.
    pub fn run_runnable(
        &mut self,
        runnable: &Runnable,
        debug: bool,
        file: Option<&Path>,
        cx: &mut Context<Self>,
    ) {
        let title = runnable_title(runnable, file);
        if !debug {
            cx.emit(DebuggerEvent::RunCommand {
                title,
                command: runnable.command.clone(),
            });
            return;
        }
        match LaunchChoice::for_command(&runnable.command) {
            Some(mut choice) => {
                choice.name = title;
                self.start(choice, file.map(Path::to_path_buf), cx)
            }
            None => {
                let language = file
                    .and_then(|f| f.extension())
                    .and_then(|e| e.to_str())
                    .map(|ext| match ext {
                        "ts" | "tsx" | "mts" | "cts" => "typescript",
                        "js" | "jsx" | "mjs" | "cjs" => "javascript",
                        other => other,
                    })
                    .unwrap_or("this");
                cx.emit(DebuggerEvent::Message(arcode_dap::missing_adapter_message(
                    language,
                )))
            }
        }
    }
}

/// A name for a runnable's terminal tab and debug session: `test
/// tests::adds`, or `run main.rs` for a main function.
pub fn runnable_title(runnable: &Runnable, file: Option<&Path>) -> String {
    if runnable.kind == arcode_project::RunnableKind::Main {
        let name = file
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| runnable.name.clone());
        format!("run {name}")
    } else {
        runnable.command.label.clone()
    }
}

impl RunMenu {
    fn pick(&mut self, debug: bool, cx: &mut Context<Self>) {
        let runnable = self.runnable.clone();
        let editor = self.editor.upgrade();
        if let Some(editor) = &editor {
            editor.update(cx, |e, cx| {
                e.hide_popover(RUN_MENU, cx);
            });
        }
        let file = editor.and_then(|e| e.read(cx).path().map(Path::to_path_buf));
        self.debugger
            .update(cx, |d, cx| {
                d.run_runnable(&runnable, debug, file.as_deref(), cx)
            })
            .ok();
    }

    fn key(&mut self, key: &str, _: &mut Window, cx: &mut Context<Self>) -> bool {
        match key {
            "up" | "down" => {
                self.selected = 1 - self.selected;
                cx.notify();
            }
            "enter" => self.pick(self.selected == 1, cx),
            "escape" => {
                if let Some(editor) = self.editor.upgrade() {
                    editor.update(cx, |e, cx| {
                        e.hide_popover(RUN_MENU, cx);
                    });
                }
            }
            _ => return false,
        }
        true
    }
}

impl Render for RunMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let label = self.runnable.command.command_line();
        let row = |ix: usize, glyph: IconName, verb: &'static str, cx: &mut Context<Self>| {
            let selected = self.selected == ix;
            div()
                .id(("run-menu-row", ix))
                .flex()
                .items_center()
                .gap_2()
                .h(px(24.))
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .when(selected, |el| el.bg(c.hover))
                .hover(|s| s.bg(c.hover))
                .child(
                    icon(glyph)
                        .size(px(13.))
                        .color(if ix == 0 { c.success } else { c.warning }),
                )
                .child(div().flex_none().child(verb))
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(360.))
                        .truncate()
                        .text_color(c.text_muted)
                        .child(label.clone()),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |m, _, _, cx| m.pick(ix == 1, cx)))
        };
        div()
            .occlude()
            .flex()
            .flex_col()
            .p_1()
            .min_w(px(180.))
            .rounded_md()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .text_color(c.text)
            .child(row(0, IconName::Play, "Run", cx))
            .child(row(1, IconName::Bug, "Debug", cx))
    }
}
