//! The commands: `send`, `select-environment`, and the panel's
//! "Open in editor" button.

use arcode_plugin::prelude::*;

use crate::env::{self, Environments};
use crate::exchange::{self, BodyKind, Context, Settings, classify};
use crate::history::{self, Entry};
use crate::vars::{Dynamic, SystemDynamic};
use crate::view::{Exchange, Outcome};
use crate::{HttpClient, SendArgs};

impl HttpClient {
    /// Sends the request around `args.line` (or the cursor line of the
    /// current document). Problems with the request itself are shown in
    /// the panel; only a missing document is an error.
    pub(crate) fn send(&mut self, args: Option<SendArgs>) -> Result<()> {
        let (doc, line) = match args {
            Some(a) => (a.doc, a.line),
            None => cursor()?,
        };
        let info = editor::document_info(doc)
            .map_err(|e| Error::msg(format!("cannot read the document: {e:?}")))?;
        let file = self.parsed_for(&info)?;
        let Some(block) = file.request_at(line) else {
            host::notify(
                host::Level::Info,
                &format!("No request at line {}.", line + 1),
            );
            return Ok(());
        };
        self.last_args = Some(SendArgs {
            doc,
            line: block.line,
        });
        let failed = |message: String| Exchange {
            name: block.name.clone(),
            method: block.method.clone(),
            url: block.url.clone(),
            outcome: Outcome::Failed(message),
        };
        let envs = match Environments::load(&self.project, info.path.as_deref()) {
            Ok(envs) => envs,
            Err(message) => return self.show(failed(message)),
        };
        let selected = env::selected().filter(|n| envs.contains(n));
        let mut vars = envs.variables(selected.as_deref());
        vars.extend(file.variables.iter().cloned());
        let ctx = Context {
            vars: &vars,
            project: &self.project,
            doc_path: info.path.as_deref(),
            settings: host::settings_as::<Settings>().unwrap_or_default(),
        };
        let mut dynamic = SystemDynamic::default();
        let prepared = match exchange::build(block, &ctx, &mut dynamic) {
            Ok(p) => p,
            Err(message) => return self.show(failed(message)),
        };
        let request = prepared.request;
        let mut ex = Exchange {
            name: prepared.name,
            method: request.method.clone(),
            url: request.url.clone(),
            outcome: Outcome::Sending,
        };
        // A visible, cancellable task; the CPU budget rises to task-seconds.
        host::begin_task(&format!("Sending {}", ex.title()));
        if host::cancelled() {
            ex.outcome = Outcome::Cancelled;
            return self.finish(ex, &mut dynamic);
        }
        self.show(ex.clone())?;
        ex.outcome = match http::send(&request) {
            Ok(response) => Outcome::Response(response),
            Err(http::HttpError::Cancelled) => Outcome::Cancelled,
            Err(e) => Outcome::Failed(describe(&e, &request)),
        };
        self.finish(ex, &mut dynamic)
    }

    /// Records `ex` in the history and shows it.
    fn finish(&mut self, ex: Exchange, dynamic: &mut dyn Dynamic) -> Result<()> {
        let (status, error, total_ms, size) = match &ex.outcome {
            Outcome::Response(r) => (Some(r.status), None, r.timing.total_ms, r.body.len()),
            Outcome::Failed(m) => (None, Some(m.clone()), 0, 0),
            Outcome::Cancelled => (None, Some("cancelled".to_owned()), 0, 0),
            Outcome::Sending => (None, None, 0, 0),
        };
        let entry = Entry {
            time_ms: dynamic.now_ms(),
            name: ex.name.clone(),
            method: ex.method.clone(),
            url: ex.url.clone(),
            status,
            error,
            total_ms,
            size: size as u64,
        };
        if let Err(message) = history::push(entry) {
            host::log(host::Level::Warn, &message);
        }
        self.show(ex)
    }

    /// Picks the environment with a quick pick and remembers it.
    pub(crate) fn select_environment(&mut self) -> Result<()> {
        let doc_path = editor::current_document().and_then(|d| d.path);
        let envs = match Environments::load(&self.project, doc_path.as_deref()) {
            Ok(envs) => envs,
            Err(message) => {
                host::notify(host::Level::Warn, &message);
                return Ok(());
            }
        };
        let names = envs.names();
        if names.is_empty() {
            host::notify(
                host::Level::Info,
                "No environments: add http-client.env.json next to your .http files.",
            );
            return Ok(());
        }
        let current = env::selected();
        let mut items = vec!["No environment".to_owned()];
        items.extend(names.iter().map(|n| {
            if current.as_deref() == Some(n.as_str()) {
                format!("{n} (current)")
            } else {
                n.clone()
            }
        }));
        let picked = host::quick_pick("Select HTTP environment", &items);
        let result = match picked {
            None => return Ok(()),
            Some(0) => env::select(None),
            Some(i) => match names.get(i as usize - 1) {
                Some(name) => env::select(Some(name)),
                None => return Ok(()),
            },
        };
        result.map_err(Error::msg)?;
        self.update_status();
        Ok(())
    }

    /// Opens the last response body in an untitled editor buffer.
    pub(crate) fn open_in_editor(&mut self) -> Result<()> {
        let Some(ex) = &self.last else {
            return Ok(());
        };
        let Outcome::Response(r) = &ex.outcome else {
            return Ok(());
        };
        let (language, text) = match classify(&r.headers, &r.body) {
            BodyKind::Json => ("json", crate::pretty::pretty_json(&r.body)),
            BodyKind::Text(language) => (
                language.unwrap_or("text"),
                String::from_utf8_lossy(&r.body).into_owned(),
            ),
            BodyKind::Binary => ("text", exchange::hex_dump(&r.body, 1024 * 1024)),
            BodyKind::Empty => return Ok(()),
        };
        editor::open_scratch(&ex.title(), language, &text)
            .map_err(|e| Error::msg(format!("cannot open the body: {e:?}")))?;
        Ok(())
    }
}

/// The current document and the line of its first cursor.
fn cursor() -> Result<(editor::DocId, u32)> {
    let doc = editor::current_document().ok_or_else(|| Error::msg("no .http document is open"))?;
    let line = editor::selections(doc.id)
        .ok()
        .and_then(|s| s.first().map(|r| r.start.line))
        .unwrap_or(0);
    Ok((doc.id, line))
}

/// A user-facing sentence for a failed send.
pub fn describe(error: &http::HttpError, request: &http::Request) -> String {
    use http::HttpError as E;
    let host = host_of(&request.url);
    match error {
        E::Denied => format!(
            "The request to {host} was not allowed. You can change this in Plugins → HTTP Client → Permissions."
        ),
        E::NoUserGesture => "Requests are only sent when you click Send.".to_owned(),
        E::Cancelled => "Cancelled.".to_owned(),
        E::Timeout => format!("No response from {host} within {} ms.", request.timeout_ms),
        E::InvalidRequest(m) => format!("Invalid request: {m}"),
        E::Dns(m) => format!("Cannot resolve {host}: {m}"),
        E::Connect(m) => format!("Cannot connect to {host}: {m}"),
        E::Tls(m) => format!("TLS error with {host}: {m}"),
        E::Protocol(m) => format!("Protocol error: {m}"),
        E::Io(m) => format!("I/O error: {m}"),
    }
}

/// The authority of a URL (`host[:port]`), for messages.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}
