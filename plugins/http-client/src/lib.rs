//! `arcode.http-client`: send requests from `.http` / `.rest` files
//! (docs/PLUGINS.md §9). An ordinary sandboxed plugin with no private host
//! hooks.
//!
//! Flow: a "▶ Send request" lens sits on every request line. Clicking it
//! runs `send` as a user gesture: the plugin resolves variables (file
//! `@vars`, the selected environment, `{{$uuid}}` & co.), turns the call
//! into a cancellable task, shows a spinner in the Response panel and
//! blocks in `http.send` (the host asks for consent per host). The
//! response fills the panel and a summary goes into the project history.
//!
//! Modules: [`parse`] (file syntax), [`vars`] (substitution), [`env`]
//! (environment files), [`exchange`] (request building, response
//! analysis), [`history`] (kv), [`view`] (the panel) and `commands`.

mod commands;
pub mod env;
pub mod exchange;
pub mod history;
pub mod parse;
pub mod vars;
pub mod view;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use arcode_plugin::prelude::*;
use arcode_plugin::ui::{Element, EventKind};
use serde::{Deserialize, Serialize};

use crate::parse::HttpFile;
use crate::view::Exchange;

/// Lens title (§9).
pub const LENS_TITLE: &str = "▶ Send request";
/// Documents with more lines get no lenses (keeps the 100 ms lens budget).
const MAX_LENS_LINES: u32 = 50_000;
/// Parsed documents kept, by id.
const MAX_CACHED_DOCS: usize = 16;

/// Arguments of the `send` command (from a lens; none from the palette or
/// the keybinding, which use the cursor line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendArgs {
    /// The `.http` document.
    pub doc: editor::DocId,
    /// A zero-based line inside the request's block.
    pub line: u32,
}

/// The plugin's state; lost on an idle unload or crash, except what lives
/// in kv (environment, history).
pub struct HttpClient {
    /// The WASI preopen (`/project`); tests point it at a temp dir.
    project: PathBuf,
    /// Parses by document, valid for one version.
    parsed: BTreeMap<editor::DocId, (u64, Rc<HttpFile>)>,
    /// The Response panel, once opened.
    panel: Option<Panel>,
    /// The last exchange shown.
    last: Option<Exchange>,
    /// What "Send again" re-sends.
    last_args: Option<SendArgs>,
    /// The tab the user picked (Body, Headers, Raw, History).
    active_tab: u32,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::with_project(workspace::PROJECT_DIR)
    }
}

impl HttpClient {
    /// A client whose project files live under `project`.
    pub fn with_project(project: impl Into<PathBuf>) -> Self {
        Self {
            project: project.into(),
            parsed: BTreeMap::new(),
            panel: None,
            last: None,
            last_args: None,
            active_tab: 0,
        }
    }

    /// The parse of `doc` at its current version (cached).
    fn parsed_for(&mut self, doc: &DocInfo) -> Result<Rc<HttpFile>> {
        if let Some((version, file)) = self.parsed.get(&doc.id)
            && *version == doc.version
        {
            return Ok(file.clone());
        }
        let text = editor::text(doc.id, None)
            .map_err(|e| Error::msg(format!("cannot read the document: {e:?}")))?;
        let file = Rc::new(parse::parse(&text));
        if self.parsed.len() >= MAX_CACHED_DOCS
            && !self.parsed.contains_key(&doc.id)
            && let Some(&oldest) = self.parsed.keys().next()
        {
            self.parsed.remove(&oldest);
        }
        self.parsed.insert(doc.id, (doc.version, file.clone()));
        Ok(file)
    }

    /// Opens (or focuses) the Response panel, keeping its diff state when
    /// the host returns the same panel.
    fn ensure_panel(&mut self, title: &str) -> Result<()> {
        let opened = Panel::open("response", title, None)?;
        if self.panel.as_ref().map(Panel::id) != Some(opened.id()) {
            self.panel = Some(opened);
        }
        if let Some(panel) = &self.panel {
            panel.set_title(title);
        }
        Ok(())
    }

    /// Re-renders the panel from the current state (diffed by the SDK).
    fn redraw(&mut self) -> Result<()> {
        let Some(panel) = self.panel.as_mut() else {
            return Ok(());
        };
        let root: Element = match &self.last {
            None => view::empty(),
            Some(ex) => view::render(ex, self.active_tab, &history::load()),
        };
        panel.rerender(&root)
    }

    /// Shows `ex` in the (opened or focused) panel.
    fn show(&mut self, ex: Exchange) -> Result<()> {
        let title = panel_title(&ex);
        self.last = Some(ex);
        self.ensure_panel(&title)?;
        self.redraw()
    }

    /// The status item: the selected environment; clicking switches it.
    fn update_status(&self) {
        let label = match env::selected() {
            Some(name) => format!("HTTP env: {name}"),
            None => "HTTP env: none".to_owned(),
        };
        host::set_status(Some(&label), Some("select-environment"));
    }
}

/// `Response: <name or METHOD url>`, capped.
fn panel_title(ex: &Exchange) -> String {
    let title: String = ex.title().chars().take(60).collect();
    format!("Response: {title}")
}

impl Plugin for HttpClient {
    fn activate(&mut self, _reason: Activation) -> Result<()> {
        self.update_status();
        Ok(())
    }

    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        match id {
            "send" => self.send(args.parse()?),
            "select-environment" => self.select_environment(),
            _ => Err(Error::msg(format!("unknown command `{id}`"))),
        }
    }

    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
        if doc.line_count > MAX_LENS_LINES {
            return Vec::new();
        }
        let Ok(file) = self.parsed_for(doc) else {
            return Vec::new();
        };
        file.requests
            .iter()
            .map(|r| {
                let args = SendArgs {
                    doc: doc.id,
                    line: r.line,
                };
                CodeLens::new(r.line, LENS_TITLE, "send", &args)
            })
            .collect()
    }

    fn open_view(&mut self, panel: PanelId, contribution: &str, _args: Json) -> Result<()> {
        if contribution != "response" {
            return Err(Error::msg(format!("no view for `{contribution}`")));
        }
        // Shown from the UI, or replayed after a restart: draw what we have.
        if self.panel.as_ref().map(Panel::id) != Some(panel) {
            self.panel = Some(Panel::attach(panel));
        }
        self.redraw()
    }

    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        let Some(p) = self.panel.as_ref().filter(|p| p.id() == panel) else {
            return Ok(());
        };
        let is = |key: &str| p.node_id(key) == Some(event.node);
        match event.kind {
            EventKind::Click if is(view::OPEN_IN_EDITOR) => self.open_in_editor(),
            EventKind::Click if is(view::SEND_AGAIN) => match self.last_args {
                Some(args) => self.send(Some(args)),
                None => Ok(()),
            },
            EventKind::Tab(i) if is(view::TABS) => {
                self.active_tab = i;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn view_closed(&mut self, panel: PanelId) {
        if self.panel.as_ref().map(Panel::id) == Some(panel) {
            self.panel = None;
        }
    }
}

// Wasm only: natively the component export names (`arcode:plugin/guest@...`)
// are not valid linker symbols, and tests drive `HttpClient` directly.
#[cfg(target_arch = "wasm32")]
arcode_plugin::export_plugin!(HttpClient);
