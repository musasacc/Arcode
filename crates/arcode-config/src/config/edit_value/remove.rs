//! Removing a key from `config.toml` without losing the comment lines
//! written above it.

use toml_edit::{DocumentMut, Item, Table, TableLike};

/// Removes `key` from the table at `tables` (which exists). The comment
/// lines above it move to the value that follows it in the table, so
/// resetting a setting keeps the template's explanation of the next one
/// (comments sit before the key they describe, and parsing attaches them
/// to that key). When no value follows, they move in front of the next
/// table header, or to the end of the file, where they were written.
pub(super) fn remove_keeping_comments(
    doc: &mut DocumentMut,
    tables: &[&str],
    key: &str,
) -> Result<(), String> {
    let header = header_position(doc, tables);
    let mut table = doc.as_table_mut() as &mut dyn TableLike;
    for name in tables {
        table = super::table_entry(table, name)?;
    }
    let Some(comments) = remove_from_table(table, key) else {
        return Ok(());
    };
    let next = header.and_then(|after| {
        let mut best = None;
        next_header(doc.as_table(), after, &mut best);
        best
    });
    match next.and_then(|position| header_at(doc.as_table_mut(), position)) {
        Some(next) => {
            let decor = next.decor_mut();
            let own = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
            let prefix = format!("{comments}{own}");
            decor.set_prefix(prefix);
        }
        None => {
            let trailing = format!("{comments}{}", doc.trailing().as_str().unwrap_or(""));
            doc.set_trailing(trailing);
        }
    }
    Ok(())
}

/// Removes `key` from `table`, moving its comments to the value after it.
/// Returns the comments when no value follows to take them.
fn remove_from_table(table: &mut dyn TableLike, key: &str) -> Option<String> {
    let comments = table
        .key(key)
        .and_then(|k| k.leaf_decor().prefix())
        .and_then(|p| p.as_str())
        .filter(|p| p.contains('#'))
        .map(str::to_owned);
    let names: Vec<String> = table.iter().map(|(k, _)| k.to_owned()).collect();
    let next = names
        .iter()
        .position(|k| k == key)
        .and_then(|i| names.get(i + 1))
        .filter(|next| table.get(next).is_some_and(Item::is_value))
        .cloned();
    table.remove(key);
    let comments = comments?;
    match next.as_deref().and_then(|next| table.key_mut(next)) {
        Some(mut next) => {
            let decor = next.leaf_decor_mut();
            let own = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
            let prefix = format!("{comments}{own}");
            decor.set_prefix(prefix);
            None
        }
        None => Some(comments),
    }
}

/// Whether `table` is written with a `[header]` of its own.
fn has_header(table: &Table) -> bool {
    !table.is_implicit() && !table.is_dotted()
}

/// The position of the header the keys of `tables` are written under;
/// `None` when that table was not read from the file.
fn header_position(doc: &DocumentMut, tables: &[&str]) -> Option<isize> {
    let mut table = doc.as_table();
    // Keys of the root come before every header.
    let mut position = Some(isize::MIN);
    for name in tables {
        let Some(next) = table.get(name).and_then(Item::as_table) else {
            break;
        };
        if has_header(next) {
            position = next.position();
        }
        table = next;
    }
    position
}

/// The tables directly under `table`, array-of-tables entries included.
fn children(table: &Table) -> impl Iterator<Item = &Table> {
    table.iter().flat_map(|(_, item)| -> Vec<&Table> {
        match item {
            Item::Table(t) => vec![t],
            Item::ArrayOfTables(array) => array.iter().collect(),
            _ => Vec::new(),
        }
    })
}

/// The smallest header position after `after` among the tables under
/// `table`.
fn next_header(table: &Table, after: isize, best: &mut Option<isize>) {
    for child in children(table) {
        if has_header(child)
            && let Some(position) = child.position()
            && position > after
            && best.is_none_or(|b| position < b)
        {
            *best = Some(position);
        }
        next_header(child, after, best);
    }
}

/// The table whose header is at `position`.
fn header_at(table: &mut Table, position: isize) -> Option<&mut Table> {
    for (_, item) in table.iter_mut() {
        let tables: Vec<&mut Table> = match item {
            Item::Table(t) => vec![t],
            Item::ArrayOfTables(array) => array.iter_mut().collect(),
            _ => continue,
        };
        for child in tables {
            if has_header(child) && child.position() == Some(position) {
                return Some(child);
            }
            if let Some(found) = header_at(child, position) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::{SettingEdit, apply_edits};

    fn remove(source: &str, path: &str) -> String {
        apply_edits(source, &[SettingEdit::remove(path)]).unwrap()
    }

    #[test]
    fn comments_move_to_the_next_value() {
        let source = "[terminal]\n# Shell.\nshell = \"zsh\"\n# Args.\nargs = []\n";
        assert_eq!(
            remove(source, "terminal.shell"),
            "[terminal]\n# Shell.\n# Args.\nargs = []\n"
        );
    }

    #[test]
    fn comments_of_the_last_key_stay_before_the_next_table() {
        let source = "[workspace]\nconfirm_delete = true\n# Where I keep my code.\nprojects_folder = \"~/code\"\n\n# Search.\n[search]\nmax_results = 10\n";
        assert_eq!(
            remove(source, "workspace.projects_folder"),
            "[workspace]\nconfirm_delete = true\n# Where I keep my code.\n\n# Search.\n[search]\nmax_results = 10\n"
        );
    }

    #[test]
    fn comments_before_a_sub_table_stay_before_it() {
        let source = "[editor]\ntab_size = 2\n# Blink.\ncursor_blink = false\n\n[editor.completion]\ndelay_ms = 9\n\n[ui]\ntheme = \"dark\"\n";
        assert_eq!(
            remove(source, "editor.cursor_blink"),
            "[editor]\ntab_size = 2\n# Blink.\n\n[editor.completion]\ndelay_ms = 9\n\n[ui]\ntheme = \"dark\"\n"
        );
    }

    #[test]
    fn comments_of_the_last_key_in_the_file_stay_at_its_end() {
        let source = "[a]\nx = 1\n\n[workspace]\n# Mine.\nprojects_folder = \"~/code\" # synced\n# The end.\n";
        assert_eq!(
            remove(source, "workspace.projects_folder"),
            "[a]\nx = 1\n\n[workspace]\n# Mine.\n# The end.\n"
        );
        // Keys before the first table.
        assert_eq!(
            remove("# Top.\nk = 1\n[b]\ny = 2\n", "k"),
            "# Top.\n[b]\ny = 2\n"
        );
    }

    #[test]
    fn tables_written_later_in_the_file_are_not_skipped() {
        // `[x.y]` comes after `[z]` in the file although it is under `[x]`.
        let source = "[x]\n# About k.\nk = 1\n[z]\nq = 1\n[x.y]\nr = 1\n";
        assert_eq!(
            remove(source, "x.k"),
            "[x]\n# About k.\n[z]\nq = 1\n[x.y]\nr = 1\n"
        );
    }
}
