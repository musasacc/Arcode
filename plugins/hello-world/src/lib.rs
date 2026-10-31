//! `example.hello-world`: a small but complete Arcode plugin, meant to be
//! copied as the starting point for your own.
//!
//! It shows the three most common things a plugin does:
//!
//! - **A palette command** (`say-hello`) that shows a notification.
//! - **A code lens** above every `TODO` comment. Clicking it runs the
//!   `explain-todo` command with arguments the lens carries.
//! - **A panel** in the sidebar (`hello`) with a text input, a button and
//!   some text. The click count is kept in the plugin's key-value storage,
//!   so it survives restarts.
//!
//! Everything the plugin contributes is declared in `plugin.toml`; this
//! file only implements what happens when Arcode calls in. The plugin
//! guide in `docs/plugins/` explains every piece in more detail.
//!
//! Layout: [`todo`] finds TODO comments (pure logic, easy to test),
//! [`view`] builds the panel's widget tree, and this file glues them to
//! the host through the [`Plugin`] trait.

pub mod todo;
pub mod view;

#[cfg(test)]
mod tests;

// `prelude` brings the `Plugin` trait, `Json`, `Result`, `Error`, `Panel`,
// `CodeLens`, `DocInfo` and the host modules (`host`, `editor`, `kv`, ...).
use arcode_plugin::prelude::*;
use serde::{Deserialize, Serialize};

use crate::view::PanelState;

/// The panel contribution id from `plugin.toml`.
pub const PANEL: &str = "hello";

/// The kv key holding the click count.
const CLICKS_KEY: &str = "greet-clicks";

/// Documents with more lines get no lenses. Lenses have a 100 ms CPU
/// budget per call; a plugin that keeps running over it loses its lenses
/// for the session.
const MAX_LENS_LINES: u32 = 20_000;

/// Arguments a TODO lens passes to `explain-todo`. Any serde type works:
/// the SDK sends it as JSON and `args.parse()` turns it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoArgs {
    /// Zero-based line of the TODO.
    pub line: u32,
    /// The TODO text as the lens saw it.
    pub text: String,
}

/// The plugin's settings, as declared under `[[contributes.settings]]`.
/// `host::settings_as` returns the manifest defaults merged with whatever
/// the user changed.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct Settings {
    greeting: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            greeting: "Hello".to_owned(),
        }
    }
}

/// The plugin's state. Arcode creates it with `Default` when the plugin
/// starts and drops it when the plugin is unloaded (after 10 idle minutes
/// without an open panel, or when the user disables it). Anything that
/// must outlive that goes into `kv`.
#[derive(Default)]
pub struct HelloWorld {
    /// The open panel, if any. `Panel` remembers the last tree it sent.
    panel: Option<Panel>,
    /// The last name typed into the panel.
    name: String,
    /// The last greeting shown in the panel.
    greeting: Option<String>,
}

impl Plugin for HelloWorld {
    /// Every contributed command lands here with its manifest-local id.
    /// Commands run as a *user gesture*: they may start a task or send an
    /// HTTP request (if the manifest asks for `network`).
    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        match id {
            "say-hello" => self.say_hello(),
            "show-panel" => self.show_panel(),
            // From a lens, `args` is a `TodoArgs`; from the palette it is
            // `null`, which parses as `None`.
            "explain-todo" => self.explain_todo(args.parse()?),
            other => Err(Error::msg(format!("unknown command `{other}`"))),
        }
    }

    /// Called for each visible document of a language listed under
    /// `[[contributes.code-lenses]]`, and again shortly after edits.
    /// Return quickly: the budget is 100 ms.
    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
        if doc.line_count > MAX_LENS_LINES {
            return Vec::new();
        }
        // With `editor = "context"` the plugin may read the document this
        // call is about. Errors (for example a closed document) just mean
        // "no lenses".
        let Ok(text) = editor::text(doc.id, None) else {
            return Vec::new();
        };
        todo::find(&text)
            .into_iter()
            .map(|todo| {
                let args = TodoArgs {
                    line: todo.line,
                    text: todo.text,
                };
                CodeLens::new(todo.line, "Hello: explain this TODO", "explain-todo", &args)
            })
            .collect()
    }

    /// Arcode shows a contributed panel (here: the user clicked the
    /// activity-bar entry, or the plugin restarted with the panel open).
    /// `panel` is the host's id for it; draw something right away.
    fn open_view(&mut self, panel: PanelId, contribution: &str, _args: Json) -> Result<()> {
        if contribution != PANEL {
            return Err(Error::msg(format!("no view for `{contribution}`")));
        }
        self.attach(panel);
        self.redraw()
    }

    /// Clicks, submits, selections and other widget events.
    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        let Some(p) = self.panel.as_ref().filter(|p| p.id() == panel) else {
            return Ok(());
        };
        // Match the event's node against the keys used in `view::render`.
        let greet = p.node_id(view::GREET_BUTTON);
        let input = p.node_id(view::NAME_INPUT);
        // Every event carries the current text of each input the user
        // edited since the previous event.
        if let Some((_, value)) = event.values.iter().find(|(node, _)| Some(*node) == input) {
            self.name = value.trim().to_owned();
        }
        let pressed = match event.kind {
            EventKind::Click => Some(event.node) == greet,
            EventKind::Submit => Some(event.node) == input,
            _ => false,
        };
        if pressed {
            self.greet()?;
        }
        Ok(())
    }

    /// The user closed the panel. Forget it, so the next `show-panel`
    /// opens a fresh one.
    fn view_closed(&mut self, panel: PanelId) {
        if self.panel.as_ref().map(Panel::id) == Some(panel) {
            self.panel = None;
        }
    }
}

impl HelloWorld {
    /// `say-hello`: a notification that mentions the current document.
    fn say_hello(&mut self) -> Result<()> {
        let settings: Settings = host::settings_as().unwrap_or_default();
        let message = match editor::current_document().and_then(|doc| doc.path) {
            Some(path) => format!(
                "{} from a plugin! You are editing {path}.",
                settings.greeting
            ),
            None => format!("{} from a plugin!", settings.greeting),
        };
        // Notifications are plain text and rate-limited (5 per minute).
        host::notify(host::Level::Info, &message);
        // Log lines show up under "Plugins: Show Log" and the plugin's Log
        // button in the Plugins view.
        host::log(host::Level::Info, "said hello");
        Ok(())
    }

    /// `show-panel`: open (or focus) the sidebar panel and draw it.
    fn show_panel(&mut self) -> Result<()> {
        // `None` keeps the location from the manifest.
        let opened = Panel::open(PANEL, "Hello", None)?;
        self.attach(opened.id());
        self.redraw()
    }

    /// `explain-todo`: from a lens with its arguments, or from the palette
    /// for the line the cursor is on.
    fn explain_todo(&mut self, args: Option<TodoArgs>) -> Result<()> {
        let todo = match args {
            Some(args) => Some(args),
            None => todo_at_cursor()?,
        };
        let message = match todo {
            Some(todo) => format!("Line {}: {}", todo.line + 1, todo.text),
            None => "There is no TODO comment on this line.".to_owned(),
        };
        host::notify(host::Level::Info, &message);
        Ok(())
    }

    /// The Greet button: count the click, remember it in kv, redraw.
    fn greet(&mut self) -> Result<()> {
        let settings: Settings = host::settings_as().unwrap_or_default();
        let who = if self.name.is_empty() {
            "world"
        } else {
            &self.name
        };
        self.greeting = Some(format!("{}, {who}!", settings.greeting));
        let clicks = load_clicks() + 1;
        // kv writes can fail (the quota is 1 MiB per scope). A lost count
        // is harmless here, so log it instead of failing the click.
        if let Err(e) = kv::set(kv::Scope::Global, CLICKS_KEY, clicks.to_string().as_bytes()) {
            host::log(host::Level::Warn, &format!("cannot save the count: {e:?}"));
        }
        self.redraw()
    }

    /// Keeps `self.panel` pointing at the host's panel `id`. A `Panel`
    /// remembers the last tree it sent; a new id needs a fresh `Panel` so
    /// the first draw is a full render.
    fn attach(&mut self, id: PanelId) {
        if self.panel.as_ref().map(Panel::id) != Some(id) {
            self.panel = Some(Panel::attach(id));
        }
    }

    /// Rebuilds the panel tree; the SDK sends only the differences.
    fn redraw(&mut self) -> Result<()> {
        let state = PanelState {
            greeting: self.greeting.clone(),
            clicks: load_clicks(),
        };
        match self.panel.as_mut() {
            Some(panel) => panel.rerender(&view::render(&state)),
            None => Ok(()),
        }
    }
}

/// The TODO on the line of the first cursor in the current document.
fn todo_at_cursor() -> Result<Option<TodoArgs>> {
    let Some(doc) = editor::current_document() else {
        return Ok(None);
    };
    let line = editor::selections(doc.id)
        .ok()
        .and_then(|s| s.first().map(|r| r.start.line))
        .unwrap_or(0);
    let text = editor::text(doc.id, None)
        .map_err(|e| Error::msg(format!("cannot read the document: {e:?}")))?;
    let found = text
        .lines()
        .nth(line as usize)
        .and_then(todo::todo_in_line)
        .map(|text| TodoArgs { line, text });
    Ok(found)
}

/// The click count from kv (0 when unset or unreadable).
fn load_clicks() -> u64 {
    kv::get(kv::Scope::Global, CLICKS_KEY)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

// Export the plugin type as the component's entry points. Only in wasm:
// natively the export names are not valid linker symbols, and the tests
// call `HelloWorld` directly against the SDK's fake host instead.
#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(HelloWorld);
