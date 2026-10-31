//! Just enough YAML for `pnpm-lock.yaml` and `pnpm-workspace.yaml`: block
//! mappings of block mappings and scalars. Lists and flow collections are
//! kept as raw text (or skipped); anything unexpected is skipped, never an
//! error.

/// A parsed node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    /// `key: value` pairs in file order.
    Map(Vec<(String, Node)>),
    /// A scalar (unquoted), or the raw text of a flow collection.
    Scalar(String),
    /// A block list: its items' raw scalar text.
    List(Vec<String>),
}

impl Node {
    /// The value of `key` in a map.
    pub(crate) fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The pairs of a map (empty for other nodes).
    pub(crate) fn entries(&self) -> &[(String, Node)] {
        match self {
            Node::Map(pairs) => pairs,
            _ => &[],
        }
    }

    /// The text of a scalar.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar(s) => Some(s),
            _ => None,
        }
    }
}

struct Line<'a> {
    indent: usize,
    text: &'a str,
}

/// Parses `text` into a map (empty when nothing could be read).
pub(crate) fn parse(text: &str) -> Node {
    let lines: Vec<Line> = text
        .lines()
        .filter_map(|raw| {
            let trimmed = raw.trim_end();
            let text = trimmed.trim_start();
            if text.is_empty() || text.starts_with('#') || text == "---" || text == "..." {
                return None;
            }
            Some(Line {
                indent: trimmed.len() - text.len(),
                text,
            })
        })
        .collect();
    let mut i = 0;
    let indent = lines.first().map_or(0, |l| l.indent);
    Node::Map(parse_map(&lines, &mut i, indent))
}

fn is_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn parse_map(lines: &[Line], i: &mut usize, indent: usize) -> Vec<(String, Node)> {
    let mut out = Vec::new();
    while *i < lines.len() {
        let line = &lines[*i];
        if line.indent < indent {
            break;
        }
        if line.indent > indent || is_item(line.text) {
            *i += 1;
            continue;
        }
        *i += 1;
        let Some((key, rest)) = split_key(line.text) else {
            continue;
        };
        let node = if !rest.is_empty() {
            Node::Scalar(unquote(rest).to_string())
        } else {
            match lines.get(*i) {
                Some(next) if is_item(next.text) && next.indent >= indent => {
                    Node::List(parse_list(lines, i, next.indent))
                }
                Some(next) if next.indent > indent => Node::Map(parse_map(lines, i, next.indent)),
                _ => Node::Scalar(String::new()),
            }
        };
        out.push((key, node));
    }
    out
}

fn parse_list(lines: &[Line], i: &mut usize, indent: usize) -> Vec<String> {
    let mut items = Vec::new();
    while *i < lines.len() {
        let line = &lines[*i];
        if line.indent < indent || (line.indent == indent && !is_item(line.text)) {
            break;
        }
        if line.indent == indent {
            items.push(unquote(line.text[1..].trim()).to_string());
        }
        *i += 1;
    }
    items
}

/// `key: value` → (`key`, `value`); keys may be quoted.
fn split_key(text: &str) -> Option<(String, &str)> {
    let first = text.chars().next()?;
    if first == '"' || first == '\'' {
        let close = text[1..].find(first)? + 1;
        let key = text[1..close].to_string();
        let rest = text[close + 1..].trim_start().strip_prefix(':')?;
        return Some((key, rest.trim()));
    }
    if let Some(pos) = text.find(": ") {
        return Some((text[..pos].trim().to_string(), text[pos + 2..].trim()));
    }
    let key = text.strip_suffix(':')?;
    Some((key.trim().to_string(), ""))
}

fn unquote(text: &str) -> &str {
    let t = text.trim();
    let quoted = t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')));
    if quoted { &t[1..t.len() - 1] } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scalars_and_lists() {
        let doc = parse(
            "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      '@types/node':\n        specifier: ^20\n        version: 20.1.0\npackages:\n  - 'apps/*'\n  - \"libs/*\"\nflow: {a: 1}\n",
        );
        assert_eq!(
            doc.get("lockfileVersion").and_then(Node::as_str),
            Some("9.0")
        );
        let dep = doc
            .get("importers")
            .and_then(|n| n.get("."))
            .and_then(|n| n.get("dependencies"))
            .and_then(|n| n.get("@types/node"))
            .expect("dependency");
        assert_eq!(dep.get("version").and_then(Node::as_str), Some("20.1.0"));
        assert_eq!(
            doc.get("packages"),
            Some(&Node::List(vec!["apps/*".into(), "libs/*".into()]))
        );
        assert_eq!(doc.get("flow").and_then(Node::as_str), Some("{a: 1}"));
    }

    #[test]
    fn garbage_does_not_panic() {
        for text in [
            "",
            ":",
            "- a\n- b",
            "a:\n    b: 1\n  c: 2\nd",
            "'unterminated: x",
            "\t\tx: y",
        ] {
            let _ = parse(text);
        }
    }
}
