//! A scripted [`LanguageServices`] provider for the language feature
//! tests.

#![allow(dead_code)]

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use arcode_buffer::Point;
use arcode_editor::{
    CodeAction, Editor, InlayHint, InlayHintKind, LanguageServices, RenameTarget, TextEdits,
};
use gpui::{Context, Entity, Task, VisualTestContext};

/// The buffer most tests start from.
pub const CODE: &str = "fn main() {\n    let x=1;\n    let y = x;\n}\n";

/// What the fake provider was asked and did.
#[derive(Default)]
pub struct Log {
    pub hint_requests: usize,
    pub applied: Vec<String>,
    pub renamed: Vec<(Point, String)>,
    pub formats: Vec<Option<Range<Point>>>,
}

pub struct FakeServices {
    log: Rc<RefCell<Log>>,
    /// Formatting never answers (for the save timeout).
    hang_format: bool,
}

impl LanguageServices for FakeServices {
    fn inlay_hints(
        &self,
        editor: &Editor,
        _lines: Range<usize>,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<InlayHint>>>> {
        self.log.borrow_mut().hint_requests += 1;
        // A type hint after every `let x`.
        let text = editor.buffer().to_string();
        let hints = text
            .lines()
            .enumerate()
            .filter_map(|(line, l)| {
                let col = l.find("let ")? + 5;
                Some(InlayHint {
                    position: Point::new(line, col),
                    label: ": i32".into(),
                    kind: InlayHintKind::Type,
                    padding_left: false,
                    padding_right: false,
                })
            })
            .collect();
        Some(Task::ready(Ok(hints)))
    }

    fn code_actions(
        &self,
        _: &Editor,
        range: Range<Point>,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Vec<CodeAction>>>> {
        if range.start.line != 1 {
            return Some(Task::ready(Ok(Vec::new())));
        }
        let mut fix = CodeAction::new("Add missing semicolon", Some("quickfix"));
        fix.preferred = true;
        Some(Task::ready(Ok(vec![
            CodeAction::new("Extract into variable", Some("refactor.extract")),
            fix,
            CodeAction::new("Organize imports", Some("source.organizeImports")),
        ])))
    }

    fn apply_code_action(
        &self,
        _: &Editor,
        action: CodeAction,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        self.log.borrow_mut().applied.push(action.title);
        Some(Task::ready(Ok(())))
    }

    fn prepare_rename(
        &self,
        editor: &Editor,
        at: Point,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<RenameTarget>>>> {
        let target = editor.word_at(at).map(|(range, word)| RenameTarget {
            range,
            placeholder: word,
        });
        Some(Task::ready(Ok(target)))
    }

    fn rename(
        &self,
        _: &Editor,
        at: Point,
        new_name: String,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<()>>> {
        self.log.borrow_mut().renamed.push((at, new_name));
        Some(Task::ready(Ok(())))
    }

    fn format(
        &self,
        editor: &Editor,
        range: Option<Range<Point>>,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<TextEdits>>> {
        self.log.borrow_mut().formats.push(range);
        if self.hang_format {
            return Some(cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(3600))
                    .await;
                Ok(Vec::new())
            }));
        }
        // Rewrites the whole document with `x=1` spaced out.
        let text = editor.buffer().to_string();
        let end = editor.offset_to_point(editor.buffer().len_chars());
        let formatted = text.replace("x=1", "x = 1");
        Some(Task::ready(Ok(vec![(Point::new(0, 0)..end, formatted)])))
    }
}

pub fn install_services(
    editor: &Entity<Editor>,
    hang_format: bool,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Log>> {
    let log = Rc::new(RefCell::new(Log::default()));
    let services = Rc::new(FakeServices {
        log: log.clone(),
        hang_format,
    });
    editor.update(cx, |e, cx| e.set_language_services(Some(services), cx));
    cx.run_until_parked();
    log
}

pub fn settle(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
}

pub fn move_to(editor: &Entity<Editor>, p: Point, cx: &mut VisualTestContext) {
    editor.update(cx, |e, cx| e.select_ranges(vec![p..p], cx));
    cx.run_until_parked();
}

pub fn menu(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    editor.read_with(cx, |e, cx| e.menu_labels(cx))
}
