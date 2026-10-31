//! A broken `snippets.toml` (loaded at startup and whenever it is saved)
//! is shown once as a toast with the parse error, not only logged.

use arcode_editor::completion::{SnippetStore, take_user_snippets_error};
use gpui::{Context, Subscription};

use super::Workspace;

impl Workspace {
    /// Shows the snippet file's error now (it may have loaded before this
    /// window opened) and after every reload of the file.
    pub(super) fn watch_snippet_errors(cx: &mut Context<Self>) -> Subscription {
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            this.update(cx, |ws, cx| ws.report_snippets_error(cx)).ok();
        });
        cx.observe_global::<SnippetStore>(|ws, cx| ws.report_snippets_error(cx))
    }

    fn report_snippets_error(&mut self, cx: &mut Context<Self>) {
        if let Some(error) = take_user_snippets_error(cx) {
            self.show_toast(format!("snippets.toml: {}", one_line(&error)), cx);
        }
    }
}

/// A TOML error on one line: its first line (where) and its last one
/// (what), without the quoted source in between.
fn one_line(error: &str) -> String {
    let lines: Vec<&str> = error
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('|') && !is_source_line(l))
        .collect();
    match lines.as_slice() {
        [] => error.trim().to_string(),
        [only] => only.to_string(),
        [first, .., last] => format!("{first}: {last}"),
    }
}

/// `12 | text`: a quoted line of the file.
fn is_source_line(line: &str) -> bool {
    line.split_once('|')
        .is_some_and(|(n, _)| !n.trim().is_empty() && n.trim().chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn toml_errors_fit_on_one_line() {
        let error =
            arcode_completion::load_snippets("[[snippet]]\nprefix = 1\n").expect_err("broken");
        let line = one_line(&format!("{error:#}"));
        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("line 2"), "{line}");
        assert!(line.contains("invalid type"), "{line}");
        assert_eq!(
            one_line("snippet with an empty prefix"),
            "snippet with an empty prefix"
        );
    }

    #[gpui::test]
    fn a_broken_snippet_file_is_shown_once(cx: &mut gpui::TestAppContext) {
        let (ws, cx) = super::super::tests::open(cx);
        cx.run_until_parked();
        let toast = |cx: &mut gpui::VisualTestContext| {
            ws.read_with(cx, |ws, _| ws.toast.as_ref().map(|t| t.to_string()))
        };
        assert_eq!(toast(cx), None);
        let _ = cx.update(|_, cx| {
            arcode_editor::completion::load_user_snippets("[[snippet]]\nprefix = 1\n", cx)
        });
        cx.run_until_parked();
        let shown = toast(cx).expect("a toast");
        assert!(shown.starts_with("snippets.toml: "), "{shown}");
        // Taken: another window (or a second look) does not show it again.
        ws.update(cx, |ws, cx| {
            ws.toast = None;
            ws.report_snippets_error(cx);
        });
        assert_eq!(toast(cx), None);
    }
}
