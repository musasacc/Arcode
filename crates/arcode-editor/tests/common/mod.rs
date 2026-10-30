//! Shared setup for the completion tests.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use arcode_buffer::{Buffer, Point};
use arcode_config::{Config, ThemeRegistry};
use arcode_editor::{
    CompletionItem, CompletionList, CompletionProvider, CompletionTrigger, Editor, SignatureHelp,
    SignatureRequest,
};
use gpui::{Context, Entity, Focusable, Task, TestAppContext, VisualTestContext};

/// Settings with no completion delay (test timers do not run by
/// themselves).
pub fn config() -> Config {
    let mut config = Config::default();
    config.editor.completion.delay_ms = 0;
    config
}

/// Opens an editor on `text` with the cursor at the end, path `path`.
pub fn open_at<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    path: &str,
    config: Config,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        arcode_ui_kit::set_settings(config, cx);
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(arcode_config::Appearance::Dark).clone();
        arcode_ui_kit::set_theme(Arc::new(theme), cx);
        arcode_editor::init(cx);
    });
    let text = text.to_string();
    let path = path.to_string();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let mut buffer = Buffer::new(&text);
        buffer.set_path(path);
        Editor::new(buffer, cx)
    });
    cx.update(|window, cx| {
        window.focus(&editor.focus_handle(cx));
        editor.update(cx, |e, cx| {
            let end = e.line_count() - 1;
            let col = e.buffer().line_len(end);
            e.select_ranges(vec![Point::new(end, col)..Point::new(end, col)], cx);
        });
    });
    cx.run_until_parked();
    (editor, cx)
}

/// Opens `text` as `test.rs` with [`config`].
pub fn open<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    open_at(cx, text, "test.rs", config())
}

pub fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.buffer().to_string())
}

pub fn labels(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| e.completion_labels())
}

pub fn visible(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |e, _| e.completion_visible())
}

type Respond = dyn Fn(&Editor, &CompletionTrigger, usize) -> (CompletionList, Duration);
type Resolve = dyn Fn(&CompletionItem) -> CompletionItem;
type Signature = dyn Fn(&Editor, &SignatureRequest) -> Option<SignatureHelp>;

/// A scripted provider that records its calls.
pub struct Fake {
    pub calls: Rc<RefCell<Vec<CompletionTrigger>>>,
    pub resolves: Rc<RefCell<usize>>,
    pub triggers: Vec<String>,
    respond: Box<Respond>,
    resolve: Option<Box<Resolve>>,
    signature: Option<Box<Signature>>,
}

impl Fake {
    pub fn new(
        respond: impl Fn(&Editor, &CompletionTrigger, usize) -> (CompletionList, Duration) + 'static,
    ) -> Self {
        Fake {
            calls: Rc::default(),
            resolves: Rc::default(),
            triggers: vec![".".into()],
            respond: Box::new(respond),
            resolve: None,
            signature: None,
        }
    }

    pub fn with_resolve(mut self, f: impl Fn(&CompletionItem) -> CompletionItem + 'static) -> Self {
        self.resolve = Some(Box::new(f));
        self
    }

    pub fn with_signature(
        mut self,
        f: impl Fn(&Editor, &SignatureRequest) -> Option<SignatureHelp> + 'static,
    ) -> Self {
        self.signature = Some(Box::new(f));
        self
    }
}

impl CompletionProvider for Fake {
    fn trigger_characters(&self, _: &Editor, _: &gpui::App) -> Vec<String> {
        self.triggers.clone()
    }

    fn completions(
        &self,
        editor: &Editor,
        trigger: &CompletionTrigger,
        cx: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionList>>> {
        let n = self.calls.borrow().len();
        self.calls.borrow_mut().push(trigger.clone());
        let (list, delay) = (self.respond)(editor, trigger, n);
        Some(cx.spawn(async move |_, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            Ok(list)
        }))
    }

    fn resolve(
        &self,
        _: &Editor,
        item: &CompletionItem,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<CompletionItem>>> {
        let f = self.resolve.as_ref()?;
        *self.resolves.borrow_mut() += 1;
        Some(Task::ready(Ok(f(item))))
    }

    fn signature_help(
        &self,
        editor: &Editor,
        request: &SignatureRequest,
        _: &mut Context<Editor>,
    ) -> Option<Task<anyhow::Result<Option<SignatureHelp>>>> {
        let f = self.signature.as_ref()?;
        Some(Task::ready(Ok(f(editor, request))))
    }
}

/// Installs `provider` on `editor`.
pub fn install(editor: &Entity<Editor>, provider: Fake, cx: &mut VisualTestContext) {
    let provider: Rc<dyn CompletionProvider> = Rc::new(provider);
    editor.update(cx, |e, _| e.set_completion_provider(Some(provider)));
}
