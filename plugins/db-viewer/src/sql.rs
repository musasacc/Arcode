//! Small, pure SQL text helpers: identifier quoting, `file:` URIs and the
//! rowid alias of a table.

/// How a database file is opened (§10 "Opening files").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// `mode=ro&nolock=1`: the default. WASI has no `fcntl` locks.
    ReadOnly,
    /// `immutable=1`: WAL databases. SQLite ignores the `-wal` file (WAL
    /// needs shared memory, which WASI lacks), so this is a snapshot.
    Snapshot,
    /// `mode=rw&nolock=1`: only after the user turned writes on. SQLite
    /// falls back to read-only when the preopen is read-only.
    ReadWrite,
}

/// Quotes an identifier for SQLite: `"name"`, with inner quotes doubled.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The `file:` URI that opens `path` (an absolute sandbox path such as
/// `/project/data/app.db`) in `mode`.
///
/// `?`, `#` and `%` are percent-encoded so a file name can never add URI
/// parameters of its own.
pub fn file_uri(path: &str, mode: OpenMode) -> String {
    let mut uri = String::with_capacity(path.len() + 32);
    uri.push_str("file:");
    for c in path.chars() {
        match c {
            '?' => uri.push_str("%3f"),
            '#' => uri.push_str("%23"),
            '%' => uri.push_str("%25"),
            _ => uri.push(c),
        }
    }
    uri.push_str(match mode {
        OpenMode::ReadOnly => "?mode=ro&nolock=1",
        OpenMode::Snapshot => "?mode=ro&immutable=1",
        OpenMode::ReadWrite => "?mode=rw&nolock=1",
    });
    uri
}

/// The first of `rowid`, `_rowid_`, `oid` that no column of the table
/// shadows, or `None` when all three are real column names (then the table
/// is paged with `LIMIT/OFFSET`, like a view).
pub fn rowid_alias<'a>(columns: impl IntoIterator<Item = &'a str> + Clone) -> Option<&'static str> {
    ["rowid", "_rowid_", "oid"].into_iter().find(|alias| {
        !columns
            .clone()
            .into_iter()
            .any(|c| c.eq_ignore_ascii_case(alias))
    })
}

/// True when a declared column type has numeric affinity (SQLite §3.1
/// rules, simplified): right-align these cells.
pub fn is_numeric_type(declared: &str) -> bool {
    let t = declared.to_ascii_uppercase();
    if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") || t.contains("BLOB") {
        return false;
    }
    ["INT", "REAL", "FLOA", "DOUB", "NUM", "DEC"]
        .iter()
        .any(|k| t.contains(k))
}

/// The first SQL keyword of `sql`, upper-cased, skipping whitespace and
/// comments. Used only for messages ("N rows changed").
pub fn first_keyword(sql: &str) -> String {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("--") {
            rest = after.split_once('\n').map_or("", |(_, r)| r);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, r)| r);
        } else {
            break;
        }
    }
    rest.chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_quoted_and_inner_quotes_doubled() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_ident("a b; DROP"), "\"a b; DROP\"");
    }

    #[test]
    fn uris_escape_parameter_characters() {
        assert_eq!(
            file_uri("/project/a.db", OpenMode::ReadOnly),
            "file:/project/a.db?mode=ro&nolock=1"
        );
        assert_eq!(
            file_uri("/project/x?mode=rw#%.db", OpenMode::Snapshot),
            "file:/project/x%3fmode=rw%23%25.db?mode=ro&immutable=1"
        );
        assert!(file_uri("/p/a.db", OpenMode::ReadWrite).ends_with("?mode=rw&nolock=1"));
    }

    #[test]
    fn rowid_alias_avoids_shadowing_columns() {
        assert_eq!(rowid_alias(["id", "name"]), Some("rowid"));
        assert_eq!(rowid_alias(["RowId", "name"]), Some("_rowid_"));
        assert_eq!(rowid_alias(["rowid", "_ROWID_"]), Some("oid"));
        assert_eq!(rowid_alias(["rowid", "_rowid_", "oid"]), None);
    }

    #[test]
    fn numeric_affinity() {
        for t in [
            "INTEGER",
            "int",
            "REAL",
            "double precision",
            "NUMERIC(10,2)",
        ] {
            assert!(is_numeric_type(t), "{t}");
        }
        for t in ["TEXT", "VARCHAR(20)", "BLOB", "", "CHARINT"] {
            assert!(!is_numeric_type(t), "{t}");
        }
    }

    #[test]
    fn first_keyword_skips_comments() {
        assert_eq!(first_keyword("  -- hi\n/* x */ insert into t"), "INSERT");
        assert_eq!(first_keyword("select 1"), "SELECT");
        assert_eq!(first_keyword("-- only a comment"), "");
    }
}
