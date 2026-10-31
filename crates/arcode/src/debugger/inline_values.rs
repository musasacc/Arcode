//! Inline debug values: the paused line's lens label and the values of the
//! variables named on that line (`r = Rect { w: 3, h: 4 }  ·  total = 6`).

use arcode_dap::{SessionState, Variable};
use arcode_editor::PausedLine;

use super::Debugger;
use super::inspect::scope_open_by_default;

/// At most this many values follow the paused lens.
const MAX_VALUES: usize = 4;
/// Values longer than this are cut with an ellipsis.
const MAX_VALUE_CHARS: usize = 48;

/// The lens label for a stop `reason` (`"breakpoint"`, `"step"`, ...).
pub fn paused_label(reason: Option<&str>) -> String {
    match reason.map(str::trim).filter(|r| !r.is_empty()) {
        Some(reason) => format!("paused · {reason}"),
        None => "paused".to_string(),
    }
}

/// Identifiers on `line` in order of first appearance, skipping string
/// literals, comments and fields or methods after a `.`.
fn identifiers(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            match b {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if line[i..].starts_with("//") {
            break;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let after_dot = line[..start].trim_end().ends_with('.');
            let word = &line[start..i];
            if !after_dot && !out.contains(&word) {
                out.push(word);
            }
            continue;
        }
        i += 1;
    }
    out
}

/// A value as one short line. A struct summary that is only a type and an
/// address (lldb's `demo::shapes::Rect @ 0x7ffc…`) becomes `Rect {…}`.
fn compact_value(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((ty, addr)) = value.rsplit_once(" @ ")
        && addr.starts_with("0x")
        && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
        && !ty.contains(' ')
    {
        let short = ty.rsplit("::").next().unwrap_or(ty);
        return format!("{short} {{…}}");
    }
    value
}

/// `name = value` for the variables in `scopes` (innermost first) that
/// `line` names, in the order the line names them.
pub fn inline_values(line: &str, scopes: &[&[Variable]]) -> Vec<String> {
    identifiers(line)
        .into_iter()
        .filter_map(|word| {
            let var = scopes
                .iter()
                .find_map(|vars| vars.iter().find(|v| v.name == word))?;
            let value = compact_value(&var.value);
            if value.is_empty() {
                return None;
            }
            let value = if value.chars().count() > MAX_VALUE_CHARS {
                let cut: String = value.chars().take(MAX_VALUE_CHARS - 1).collect();
                format!("{cut}…")
            } else {
                value
            };
            Some(format!("{word} = {value}"))
        })
        .take(MAX_VALUES)
        .collect()
}

impl Debugger {
    /// The paused line for an editor stopped at `line` whose text is
    /// `text`: the stop reason and the loaded locals named on the line.
    pub(super) fn paused_line(&self, line: usize, text: &str) -> PausedLine {
        let session = self.session.as_ref();
        let reason = session.and_then(|s| match &s.state {
            SessionState::Stopped { reason, .. } => Some(reason.as_str()),
            _ => None,
        });
        let scopes: Vec<&[Variable]> = session
            .map(|s| {
                s.scopes
                    .iter()
                    .filter(|scope| scope_open_by_default(scope))
                    .filter_map(|scope| s.children.get(&scope.variables_reference))
                    .map(Vec::as_slice)
                    .collect()
            })
            .unwrap_or_default();
        PausedLine {
            line,
            label: paused_label(reason).into(),
            values: inline_values(text, &scopes)
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, value: &str) -> Variable {
        Variable {
            name: name.into(),
            value: value.into(),
            ..Default::default()
        }
    }

    #[test]
    fn values_follow_the_names_on_the_line() {
        let locals = [
            var("total", "6"),
            var("r", "Rect { w: 3, h: 4 }"),
            var("numbers", "vec![1, 2, 3]"),
        ];
        let line = r#"    println!("area {} total {} numbers", r.area(), total);"#;
        assert_eq!(
            inline_values(line, &[&locals]),
            vec!["r = Rect { w: 3, h: 4 }", "total = 6"]
        );
        // Fields and methods after a dot are not variables; comments end it.
        let args = [var("area", "1"), var("x", "2")];
        assert_eq!(
            inline_values("let y = r.area(); // x", &[&args]),
            Vec::<String>::new()
        );
        // The innermost scope wins; long values are cut.
        let outer = [var("x", "outer")];
        let long = "a".repeat(80);
        let inner = [var("x", &long)];
        let values = inline_values("x + 1", &[&inner, &outer]);
        assert_eq!(values.len(), 1);
        assert!(values[0].ends_with('…') && values[0].chars().count() == 4 + MAX_VALUE_CHARS);
    }

    #[test]
    fn address_summaries_become_the_type() {
        assert_eq!(
            compact_value("demo::shapes::Rect @ 0x7fffffffc028"),
            "Rect {…}"
        );
        assert_eq!(
            compact_value("Rect { w: 3,\n  h: 4 }"),
            "Rect { w: 3, h: 4 }"
        );
        assert_eq!(compact_value("\"at @ 0xzz\""), "\"at @ 0xzz\"");
    }

    #[test]
    fn label_names_the_reason() {
        assert_eq!(paused_label(Some("breakpoint")), "paused · breakpoint");
        assert_eq!(paused_label(Some(" ")), "paused");
        assert_eq!(paused_label(None), "paused");
    }
}
