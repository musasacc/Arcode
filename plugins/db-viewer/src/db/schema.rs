//! Schema reads: tables and views from `sqlite_schema` (with
//! `pragma_table_list` for the rowid flag), columns from
//! `pragma_table_xinfo`.

use rusqlite::Connection;

use crate::sql::is_numeric_type;

/// Table or view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary or virtual table.
    Table,
    /// A view.
    View,
}

/// A table or view of the main schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Name (unquoted).
    pub name: String,
    /// Table or view.
    pub kind: Kind,
    /// `WITHOUT ROWID` table (paged with `LIMIT/OFFSET`).
    pub without_rowid: bool,
}

impl Object {
    /// True when rows can be paged by rowid.
    pub fn has_rowid(&self) -> bool {
        self.kind == Kind::Table && !self.without_rowid
    }
}

/// One column of a table or view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    /// Name.
    pub name: String,
    /// Declared type, as written (may be empty).
    pub declared: String,
    /// Part of the primary key.
    pub primary_key: bool,
    /// `NOT NULL`.
    pub not_null: bool,
    /// Hidden (virtual-table) column: not part of `SELECT *`.
    pub hidden: bool,
}

impl ColumnInfo {
    /// The tree item's detail text, e.g. `INTEGER · PK · NOT NULL`.
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if !self.declared.is_empty() {
            parts.push(self.declared.as_str());
        }
        if self.primary_key {
            parts.push("PK");
        }
        if self.not_null {
            parts.push("NOT NULL");
        }
        parts.join(" · ")
    }

    /// Numeric affinity (right-aligned in the grid).
    pub fn numeric(&self) -> bool {
        is_numeric_type(&self.declared)
    }
}

/// Tables first, then views, each by name; FTS shadow tables are hidden.
pub fn objects(conn: &Connection) -> rusqlite::Result<Vec<Object>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.type, coalesce(l.wr, 0)
           FROM sqlite_schema AS s
           LEFT JOIN pragma_table_list AS l ON l.schema = 'main' AND l.name = s.name
          WHERE s.type IN ('table', 'view') AND coalesce(l.type, '') <> 'shadow'
          ORDER BY s.type = 'view', s.name COLLATE NOCASE",
    )?;
    stmt.query_map([], |row| {
        let kind: String = row.get(1)?;
        Ok(Object {
            name: row.get(0)?,
            kind: if kind == "view" {
                Kind::View
            } else {
                Kind::Table
            },
            without_rowid: row.get::<_, i64>(2)? != 0,
        })
    })?
    .collect()
}

/// Columns of `object`, in declaration order.
pub fn columns(conn: &Connection, object: &str) -> rusqlite::Result<Vec<ColumnInfo>> {
    let mut stmt =
        conn.prepare("SELECT name, type, \"notnull\", pk, hidden FROM pragma_table_xinfo(?1)")?;
    stmt.query_map([object], |row| {
        Ok(ColumnInfo {
            name: row.get(0)?,
            declared: row.get(1)?,
            not_null: row.get::<_, i64>(2)? != 0,
            primary_key: row.get::<_, i64>(3)? != 0,
            // 1 = hidden virtual-table column; 2 and 3 are generated
            // columns, which `SELECT *` does include.
            hidden: row.get::<_, i64>(4)? == 1,
        })
    })?
    .collect()
}
