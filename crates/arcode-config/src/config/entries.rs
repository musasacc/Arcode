//! Helpers for the `[language.<id>]` and `[lsp.<name>]` tables: names
//! matched ignoring case, and each entry of a table of tables.

use std::collections::BTreeMap;

use toml::Value;

/// The key of `map` equal to `name` ignoring case (exact match preferred).
pub(super) fn find_key_ignore_case<V>(map: &BTreeMap<String, V>, name: &str) -> Option<String> {
    if map.contains_key(name) {
        return Some(name.to_owned());
    }
    let lower = name.to_lowercase();
    map.keys().find(|k| k.to_lowercase() == lower).cloned()
}

/// Warns when two tables of one file (e.g. `language.Rust` and
/// `language.rust`) merge into the same entry.
pub(super) fn warn_same_entry(
    seen: &mut BTreeMap<String, String>,
    key: &str,
    path: &str,
    warnings: &mut Vec<String>,
) {
    if let Some(first) = seen.insert(key.to_owned(), path.to_owned()) {
        warnings.push(format!(
            "`{path}` and `{first}` name the same entry (names are case-insensitive); merged"
        ));
    }
}

pub(super) fn for_each_entry(
    value: &Value,
    section: &str,
    warnings: &mut Vec<String>,
    mut f: impl FnMut(&str, &Value, &mut Vec<String>),
) {
    let Some(table) = value.as_table() else {
        warnings.push(format!(
            "`{section}` must be a table of tables, found {}; ignored",
            value.type_str()
        ));
        return;
    };
    for (name, entry) in table {
        if entry.is_table() {
            f(name, entry, warnings);
        } else {
            warnings.push(format!(
                "`{section}.{name}` must be a table, found {}; ignored",
                entry.type_str()
            ));
        }
    }
}
