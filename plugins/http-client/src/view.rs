//! The Response panel (docs/PLUGINS.md §9, step 6 of the flow):
//!
//! - a summary row: a status badge toned by status class, total and
//!   headers time, size, and "Send again" / "Open in editor" buttons;
//! - tabs: **Body** (the host's JSON viewer for JSON, highlighted `code`
//!   otherwise, a hex summary for binary data), **Headers**, **Raw**, and
//!   **History** (the last 50 exchanges in this project).
//!
//! Every function here is pure: it builds an element tree from data.

use arcode_plugin::http;
use arcode_plugin::ui::*;

use crate::exchange::{BodyKind, classify, format_size, hex_dump, raw_text, truncate};
use crate::history::Entry;
use crate::vars::iso8601;

/// Key of the "Open in editor" button.
pub const OPEN_IN_EDITOR: &str = "open-in-editor";
/// Key of the "Send again" button.
pub const SEND_AGAIN: &str = "send-again";
/// Key of the tabs node.
pub const TABS: &str = "tabs";
/// Body text shown in the panel; "Open in editor" has all of it. Keeps
/// a render well under the host's 8 MiB string cap.
pub const MAX_BODY_VIEW: usize = 3 * 1024 * 1024;
/// Raw text shown in the panel.
pub const MAX_RAW_VIEW: usize = 1024 * 1024;
/// Binary bytes shown as hex.
const MAX_HEX_BYTES: usize = 4096;

/// What happened to the last request.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// In flight.
    Sending,
    /// A response arrived (any status).
    Response(http::Response),
    /// No response: the message says why.
    Failed(String),
    /// The user cancelled.
    Cancelled,
}

/// One request and its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// `# @name`.
    pub name: Option<String>,
    /// Method.
    pub method: String,
    /// Resolved URL.
    pub url: String,
    /// Outcome.
    pub outcome: Outcome,
}

impl Exchange {
    /// "name" or "GET url", for titles.
    pub fn title(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("{} {}", self.method, self.url))
    }
}

/// The panel before anything was sent.
pub fn empty() -> Element {
    vstack([empty_state(
        "No response yet",
        "Click ▶ Send request above a request in a .http file.",
    )
    .key("empty")])
    .key("root")
}

/// The whole panel for `ex`. `active` is the selected tab.
pub fn render(ex: &Exchange, active: u32, history: &[Entry]) -> Element {
    let request = text(format!("{} {}", ex.method, ex.url))
        .mono()
        .key("request");
    let body = match &ex.outcome {
        Outcome::Sending => vec![
            request,
            spinner(format!("Sending {}…", ex.title())).key("sending"),
        ],
        Outcome::Cancelled => vec![
            hstack([badge("Cancelled", Tone::Muted)]).key("summary"),
            request,
            history_list(history).key("history"),
        ],
        Outcome::Failed(message) => vec![
            hstack([badge("Failed", Tone::Danger)]).key("summary"),
            request,
            text(message.clone()).tone(Tone::Danger).wrap().key("error"),
            history_list(history).key("history"),
        ],
        Outcome::Response(r) => vec![
            summary(r).key("summary"),
            request,
            response_tabs(r, active, history).key(TABS).grow(),
        ],
    };
    vstack(body).key("root")
}

/// `200 OK` badge, timing, size, buttons.
fn summary(r: &http::Response) -> Element {
    let mut row = vec![
        badge(format!("{} {}", r.status, r.reason), status_tone(r.status)).key("status"),
        text(timing_text(&r.timing)).tone(Tone::Muted).key("timing"),
        text(format_size(r.body.len() as u64))
            .tone(Tone::Muted)
            .key("size"),
    ];
    if r.truncated {
        row.push(badge("truncated", Tone::Warning).key("truncated"));
    }
    row.push(spacer().key("spacer"));
    row.push(button("Send again").icon("refresh").key(SEND_AGAIN));
    row.push(
        button("Open in editor")
            .icon("file")
            .enabled(!r.body.is_empty())
            .key(OPEN_IN_EDITOR),
    );
    hstack(row)
}

/// `12 ms (headers 10 ms)`.
pub fn timing_text(t: &http::Timing) -> String {
    format!("{} ms (headers {} ms)", t.total_ms, t.headers_ms)
}

/// Success for 2xx, accent for 1xx/3xx, warning for 4xx, danger for 5xx.
pub fn status_tone(status: u16) -> Tone {
    match status {
        200..=299 => Tone::Success,
        400..=499 => Tone::Warning,
        500..=599 => Tone::Danger,
        _ => Tone::Accent,
    }
}

fn response_tabs(r: &http::Response, active: u32, history: &[Entry]) -> Element {
    let headers = key_values(r.headers.iter().cloned());
    let raw = code(raw_text(r, MAX_RAW_VIEW), None);
    tabs(
        active.min(3),
        [
            ("Body".to_owned(), body_view(r)),
            ("Headers".to_owned(), scroll(headers)),
            ("Raw".to_owned(), raw),
            ("History".to_owned(), history_list(history)),
        ],
    )
}

/// The Body tab.
fn body_view(r: &http::Response) -> Element {
    match classify(&r.headers, &r.body) {
        BodyKind::Empty => empty_state("No body", ""),
        BodyKind::Binary => vstack([
            text(format!(
                "{} of binary data; first {} shown",
                format_size(r.body.len() as u64),
                format_size(r.body.len().min(MAX_HEX_BYTES) as u64)
            ))
            .tone(Tone::Muted),
            code(hex_dump(&r.body, MAX_HEX_BYTES), None).grow(),
        ]),
        BodyKind::Json => {
            let text = std::str::from_utf8(&r.body).unwrap_or_default();
            match truncate(text, MAX_BODY_VIEW) {
                (all, false) => json(all),
                (part, true) => cut_code(part, Some("json")),
            }
        }
        BodyKind::Text(language) => {
            let text = std::str::from_utf8(&r.body).unwrap_or_default();
            match truncate(text, MAX_BODY_VIEW) {
                (all, false) => code(all, language.map(str::to_owned)),
                (part, true) => cut_code(part, language),
            }
        }
    }
}

/// A body too large for the panel: its start, plus a hint.
fn cut_code(part: &str, language: Option<&str>) -> Element {
    vstack([
        text("Showing the first 3 MB; use Open in editor for the whole body.").tone(Tone::Warning),
        code(part, language.map(str::to_owned)).grow(),
    ])
}

/// The History tab: newest first. The key column is narrow (the host
/// cuts it), so it holds only the time; the request goes with its outcome.
fn history_list(entries: &[Entry]) -> Element {
    if entries.is_empty() {
        return empty_state("No history yet", "Sent requests appear here.");
    }
    let rows = entries.iter().map(|e| {
        let when = iso8601(e.time_ms);
        let key = when[..19].replace('T', " ");
        let outcome = match (e.status, &e.error) {
            (Some(s), _) => format!("{s} · {} ms · {}", e.total_ms, format_size(e.size)),
            (None, Some(err)) => err.clone(),
            (None, None) => "no response".to_owned(),
        };
        (key, format!("{} {} → {outcome}", e.method, e.url))
    });
    scroll(key_values(rows))
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
