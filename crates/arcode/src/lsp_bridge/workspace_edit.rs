//! Turns an LSP `WorkspaceEdit` into an ordered list of file operations
//! (text edits per file, create / rename / delete) and converts text edits
//! to buffer points. Applying them is the workspace's job
//! (`crate::refactor`), since it may open files and owns the tabs.

use std::ops::Range;
use std::path::PathBuf;

use arcode_buffer::{Buffer, Point};
use arcode_lsp::lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, ResourceOp, TextEdit, WorkspaceEdit,
};
use arcode_lsp::{PositionEncoding, uri_to_path};

use super::convert::to_point;

/// One step of a workspace edit, in the order the server gave.
#[derive(Debug, Clone, PartialEq)]
pub enum FileOp {
    /// Text edits of one file (non-overlapping, positions in the server's
    /// encoding against the file's current text).
    Edit {
        path: PathBuf,
        /// The document version the edits were computed for, if the server
        /// said.
        version: Option<i32>,
        edits: Vec<TextEdit>,
    },
    /// Create a file.
    Create {
        path: PathBuf,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    /// Rename (move) a file or folder.
    Rename {
        from: PathBuf,
        to: PathBuf,
        overwrite: bool,
        ignore_if_exists: bool,
    },
    /// Delete a file or folder.
    Delete {
        path: PathBuf,
        recursive: bool,
        ignore_if_not_exists: bool,
    },
}

impl FileOp {
    /// The file a text edit changes.
    pub fn edited_path(&self) -> Option<&PathBuf> {
        match self {
            FileOp::Edit { path, .. } => Some(path),
            _ => None,
        }
    }
}

fn path_of(uri: &arcode_lsp::lsp_types::Uri) -> Result<PathBuf, String> {
    uri_to_path(uri).ok_or_else(|| format!("not a file: {}", uri.as_str()))
}

fn plain_edits(edits: Vec<OneOf<TextEdit, arcode_lsp::lsp_types::AnnotatedTextEdit>>) -> Vec<TextEdit> {
    edits
        .into_iter()
        .map(|e| match e {
            OneOf::Left(edit) => edit,
            OneOf::Right(annotated) => annotated.text_edit,
        })
        .collect()
}

/// The operations of `edit`. `documentChanges` wins over `changes` (as
/// the specification says); `changes` are ordered by path.
pub fn plan(edit: WorkspaceEdit) -> Result<Vec<FileOp>, String> {
    let mut ops = Vec::new();
    if let Some(changes) = edit.document_changes {
        let list = match changes {
            DocumentChanges::Edits(edits) => edits
                .into_iter()
                .map(DocumentChangeOperation::Edit)
                .collect(),
            DocumentChanges::Operations(list) => list,
        };
        for change in list {
            ops.push(match change {
                DocumentChangeOperation::Edit(e) => FileOp::Edit {
                    path: path_of(&e.text_document.uri)?,
                    version: e.text_document.version,
                    edits: plain_edits(e.edits),
                },
                DocumentChangeOperation::Op(ResourceOp::Create(c)) => {
                    let o = c.options.unwrap_or_default();
                    FileOp::Create {
                        path: path_of(&c.uri)?,
                        overwrite: o.overwrite.unwrap_or(false),
                        ignore_if_exists: o.ignore_if_exists.unwrap_or(false),
                    }
                }
                DocumentChangeOperation::Op(ResourceOp::Rename(r)) => {
                    let o = r.options.unwrap_or_default();
                    FileOp::Rename {
                        from: path_of(&r.old_uri)?,
                        to: path_of(&r.new_uri)?,
                        overwrite: o.overwrite.unwrap_or(false),
                        ignore_if_exists: o.ignore_if_exists.unwrap_or(false),
                    }
                }
                DocumentChangeOperation::Op(ResourceOp::Delete(d)) => {
                    let o = d.options.unwrap_or_default();
                    FileOp::Delete {
                        path: path_of(&d.uri)?,
                        recursive: o.recursive.unwrap_or(false),
                        ignore_if_not_exists: o.ignore_if_not_exists.unwrap_or(false),
                    }
                }
            });
        }
        return Ok(ops);
    }
    let mut changes: Vec<_> = edit.changes.unwrap_or_default().into_iter().collect();
    changes.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    for (uri, edits) in changes {
        ops.push(FileOp::Edit {
            path: path_of(&uri)?,
            version: None,
            edits,
        });
    }
    Ok(ops)
}

/// `edits` as buffer ranges of `buffer`, sorted. Overlapping edits are an
/// error (the specification forbids them).
pub fn buffer_edits(
    buffer: &Buffer,
    edits: &[TextEdit],
    encoding: PositionEncoding,
) -> Result<Vec<(Range<Point>, String)>, String> {
    let mut out: Vec<(Range<Point>, String)> = edits
        .iter()
        .map(|e| {
            let start = to_point(buffer, e.range.start, encoding);
            let end = to_point(buffer, e.range.end, encoding);
            (start.min(end)..start.max(end), e.new_text.clone())
        })
        .collect();
    // Stable: inserts at one position keep the server's order.
    out.sort_by_key(|(r, _)| (r.start, r.end));
    for pair in out.windows(2) {
        if pair[1].0.start < pair[0].0.end {
            return Err("the server sent overlapping edits".into());
        }
    }
    Ok(out)
}

/// The edits of `ops` to `path` merged (the plain `changes` form has one
/// list per file; `documentChanges` may have several).
pub fn edits_for<'a>(ops: &'a [FileOp], path: &'a std::path::Path) -> Vec<&'a TextEdit> {
    ops.iter()
        .filter_map(|op| match op {
            FileOp::Edit { path: p, edits, .. } if p == path => Some(edits.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// A short summary like `3 files` for messages.
pub fn files_touched(ops: &[FileOp]) -> usize {
    let mut paths: Vec<&PathBuf> = ops.iter().filter_map(FileOp::edited_path).collect();
    paths.sort();
    paths.dedup();
    paths.len()
}
