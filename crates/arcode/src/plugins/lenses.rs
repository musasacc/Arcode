//! Plugin code lenses in the editor (docs/PLUGINS.md §9 flow, §5 "Queue
//! hygiene"): the answers of every lens provider for one document become
//! one set of above-line [`CodeLens`] rows, drawn through the editor's
//! extension points under [`LENS_SET`]. A click arrives as
//! `EditorEvent::CodeLensClicked`, and its id finds the plugin command
//! (with its JSON args) to run as a user gesture.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use arcode_editor::{CodeLens, LensItem, LensPlacement};
use arcode_plugin_host::bindings::guest;
use arcode_plugin_host::{PluginId, QualifiedId};

/// The editor decoration set plugin lenses use.
pub const LENS_SET: &str = "plugins";

/// Lenses are asked for this long after the last edit (§5).
pub const LENS_DEBOUNCE: Duration = Duration::from_millis(300);

/// Longest lens label shown (chars).
const MAX_LABEL_CHARS: usize = 80;

/// What a lens click runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensTarget {
    /// Namespaced command.
    pub command: QualifiedId,
    /// JSON args from the plugin.
    pub args: String,
}

/// The plugin lenses of one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LensSet {
    /// Rows for [`arcode_editor::Editor::set_code_lenses`].
    pub lenses: Vec<CodeLens>,
    /// Click targets by [`LensItem::id`].
    pub targets: HashMap<u64, LensTarget>,
}

/// A label as shown: one line, control characters replaced, capped.
fn label(title: &str) -> String {
    let mut out: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_LABEL_CHARS)
        .collect();
    if title.chars().count() > MAX_LABEL_CHARS {
        out.push('…');
    }
    out
}

/// Builds the lens rows for `answers` (each plugin's lenses, in plugin
/// order). Lenses on the same line share one row; lines at or past
/// `line_count` and lenses whose command is not a valid id are dropped.
pub fn build(answers: Vec<(PluginId, Vec<guest::CodeLens>)>, line_count: usize) -> LensSet {
    let mut by_line: BTreeMap<usize, Vec<LensItem>> = BTreeMap::new();
    let mut targets = HashMap::new();
    let mut next = 1u64;
    for (plugin, lenses) in answers {
        for lens in lenses {
            let line = lens.line as usize;
            if line >= line_count {
                continue;
            }
            let Some(command) = QualifiedId::parse(plugin.qualify(&lens.command).as_str()) else {
                continue;
            };
            let id = next;
            next += 1;
            targets.insert(
                id,
                LensTarget {
                    command,
                    args: lens.args,
                },
            );
            by_line.entry(line).or_default().push(LensItem {
                id,
                label: label(&lens.title).into(),
            });
        }
    }
    let lenses = by_line
        .into_iter()
        .map(|(line, items)| CodeLens {
            line,
            placement: LensPlacement::Above,
            items,
        })
        .collect();
    LensSet { lenses, targets }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(line: u32, title: &str, command: &str) -> guest::CodeLens {
        guest::CodeLens {
            line,
            title: title.into(),
            command: command.into(),
            args: format!("{{\"line\":{line}}}"),
        }
    }

    #[test]
    fn lenses_group_by_line_and_map_ids_to_commands() {
        let http = PluginId::parse("arcode.http-client").unwrap();
        let other = PluginId::parse("acme.tools").unwrap();
        let set = build(
            vec![
                (
                    http.clone(),
                    vec![lens(3, "▶ Send request", "send"), lens(0, "Env", "env")],
                ),
                (other, vec![lens(3, "Copy as curl", "curl")]),
            ],
            10,
        );
        assert_eq!(set.lenses.len(), 2);
        assert_eq!(set.lenses[0].line, 0);
        assert_eq!(set.lenses[1].line, 3);
        assert_eq!(set.lenses[1].placement, LensPlacement::Above);
        let labels: Vec<&str> = set.lenses[1]
            .items
            .iter()
            .map(|i| i.label.as_ref())
            .collect();
        assert_eq!(labels, ["▶ Send request", "Copy as curl"]);
        let send = &set.targets[&set.lenses[1].items[0].id];
        assert_eq!(send.command.as_str(), "arcode.http-client/send");
        assert_eq!(send.args, "{\"line\":3}");
        let curl = &set.targets[&set.lenses[1].items[1].id];
        assert_eq!(curl.command.as_str(), "acme.tools/curl");
    }

    #[test]
    fn bad_lenses_are_dropped_and_labels_capped() {
        let p = PluginId::parse("acme.tools").unwrap();
        let long = "x".repeat(200);
        let set = build(
            vec![(
                p,
                vec![
                    lens(99, "past the end", "run"),
                    lens(1, "bad id", "Not A Command!"),
                    lens(2, &format!("a\nb{long}"), "run"),
                ],
            )],
            5,
        );
        assert_eq!(set.lenses.len(), 1);
        let label = &set.lenses[0].items[0].label;
        assert!(label.starts_with("a b") && label.ends_with('…'));
        assert_eq!(label.chars().count(), MAX_LABEL_CHARS + 1);
        assert_eq!(set.targets.len(), 1);
    }
}
