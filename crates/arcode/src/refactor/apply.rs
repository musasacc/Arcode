//! Running the operations of one workspace edit.

use std::path::{Path, PathBuf};

use arcode_buffer::Point;
use arcode_editor::Editor;
use gpui::{Context, Entity, Window};

use super::Summary;
use super::files::{create_file, delete_path, rename_path};
use super::preflight::check_resources;
use crate::lsp_bridge::edits::EditRequest;
use crate::lsp_bridge::lsp_version;
use crate::lsp_bridge::workspace_edit::{FileOp, buffer_edits};
use crate::workspace::Workspace;

/// `path` as editors store it (canonical when it exists).
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// State of one request while it runs.
#[derive(Default)]
struct Run {
    summary: Summary,
    /// Editors whose text this request changed (in an undo group), and
    /// whether each had unsaved changes before.
    edited: Vec<(Entity<Editor>, bool)>,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl Workspace {
    /// The open editor of `path`, in any pane.
    fn editor_of(&self, path: &Path, cx: &gpui::App) -> Option<Entity<Editor>> {
        let canonical = canonical(path);
        self.editors(cx).into_iter().find(|e| {
            e.read(cx)
                .path()
                .is_some_and(|p| p == canonical || p == path)
        })
    }

    /// Open editors showing `path` or a file below it.
    fn editors_under(&self, path: &Path, cx: &gpui::App) -> Vec<(Entity<Editor>, PathBuf)> {
        let canonical = canonical(path);
        self.editors(cx)
            .into_iter()
            .filter_map(|e| {
                let p = e.read(cx).path()?.to_path_buf();
                (p.starts_with(&canonical) || p.starts_with(path)).then_some((e, p))
            })
            .collect()
    }

    /// Refuses the edit when an open document changed since the server
    /// computed it.
    fn check_versions(&self, ops: &[FileOp], cx: &gpui::App) -> Result<(), String> {
        for op in ops {
            if let FileOp::Edit {
                path,
                version: Some(version),
                ..
            } = op
                && let Some(editor) = self.editor_of(path, cx)
                && lsp_version(editor.read(cx).buffer().version()) != *version
            {
                return Err(format!(
                    "{} changed since the edit was computed",
                    file_name(path)
                ));
            }
        }
        Ok(())
    }

    /// Refuses a delete of a path whose open editor has unsaved changes
    /// (they would be lost), before anything changes.
    fn check_deletes(&self, ops: &[FileOp], cx: &gpui::App) -> Result<(), String> {
        for op in ops {
            if let FileOp::Delete { path, .. } = op
                && let Some((_, dirty)) = self
                    .editors_under(path, cx)
                    .into_iter()
                    .find(|(e, _)| e.read(cx).is_dirty())
            {
                return Err(format!("{} has unsaved changes", file_name(&dirty)));
            }
        }
        Ok(())
    }

    /// Applies `request`; see the module docs of [`crate::refactor`].
    pub(crate) fn apply_edit_request(
        &mut self,
        request: &EditRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Summary, String> {
        self.check_versions(&request.ops, cx)?;
        self.check_deletes(&request.ops, cx)?;
        check_resources(&request.ops, |path| self.editor_of(path, cx).is_some())?;
        let original = self.active_editor(cx);
        let focused = window.focused(cx);
        let mut run = Run::default();
        let result = request
            .ops
            .iter()
            .try_for_each(|op| self.apply_op(op, request, &mut run, window, cx));
        for (editor, _) in &run.edited {
            editor.update(cx, |e, _| e.end_edit_group());
        }
        if result.is_ok() && run.summary.resources > 0 {
            // Resource operations are already on disk; save the text that
            // goes with them so the project on disk stays consistent.
            // Files with earlier unsaved changes stay unsaved.
            for (editor, was_dirty) in &run.edited {
                if !was_dirty && let Err(err) = editor.update(cx, |e, cx| e.save(cx)) {
                    log::warn!("cannot save {}: {err}", editor.read(cx).title());
                }
            }
            run.summary.saved = true;
        }
        if run.summary.opened > 0
            && let Some(original) = original
        {
            // Opening files activated them; the user stays where they were.
            for pane in self.panes().to_vec() {
                let ix = pane.read(cx).items().iter().position(|e| *e == original);
                if let Some(ix) = ix {
                    pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
                    break;
                }
            }
        }
        if let Some(focused) = focused {
            // Activating tabs focused editors; give focus back (e.g. to the
            // terminal the user was typing in).
            window.focus(&focused);
        }
        result.map(|()| run.summary)
    }

    /// The editor of `path` for a text edit: opened in a tab when needed,
    /// in this request's undo group.
    fn edit_target(
        &mut self,
        path: &Path,
        run: &mut Run,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<Editor>, String> {
        let editor = match self.editor_of(path, cx) {
            Some(editor) => editor,
            None => {
                if !path.is_file() {
                    return Err(format!("{} does not exist", path.display()));
                }
                self.open_file(path, window, cx);
                run.summary.opened += 1;
                self.editor_of(path, cx)
                    .ok_or_else(|| format!("cannot open {}", path.display()))?
            }
        };
        if editor.read(cx).is_read_only() {
            return Err(format!("{} is read-only", file_name(path)));
        }
        if !run.edited.iter().any(|(e, _)| *e == editor) {
            let dirty = editor.read(cx).is_dirty();
            editor.update(cx, |e, _| e.begin_edit_group());
            run.edited.push((editor.clone(), dirty));
            run.summary.files += 1;
        }
        Ok(editor)
    }

    fn apply_op(
        &mut self,
        op: &FileOp,
        request: &EditRequest,
        run: &mut Run,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match op {
            FileOp::Edit { path, edits, .. } => {
                let editor = self.edit_target(path, run, window, cx)?;
                let edits = buffer_edits(editor.read(cx).buffer(), edits, request.encoding)?;
                editor.update(cx, |e, cx| e.apply_edits(edits, cx));
            }
            FileOp::Create {
                path,
                overwrite,
                ignore_if_exists,
            } => {
                let open = self
                    .editor_of(path, cx)
                    .filter(|_| *overwrite && path.exists());
                if create_file(path, *overwrite, *ignore_if_exists)? {
                    run.summary.resources += 1;
                    if open.is_some() {
                        // The file was emptied: so is its editor, so the
                        // edits that follow apply to the new (empty) text.
                        let editor = self.edit_target(path, run, window, cx)?;
                        editor.update(cx, |e, cx| {
                            let end = e.offset_to_point(e.buffer().len_chars());
                            e.apply_edits(vec![(Point::new(0, 0)..end, String::new())], cx)
                        });
                    }
                }
            }
            FileOp::Rename {
                from,
                to,
                overwrite,
                ignore_if_exists,
            } => {
                let open = self.editors_under(from, cx);
                let from_canonical = canonical(from);
                if rename_path(from, to, *overwrite, *ignore_if_exists)? {
                    run.summary.resources += 1;
                    let to = canonical(to);
                    for (editor, old) in open {
                        let rest = old
                            .strip_prefix(&from_canonical)
                            .or_else(|_| old.strip_prefix(from))
                            .map(Path::to_path_buf)
                            .unwrap_or_default();
                        let new = if rest.as_os_str().is_empty() {
                            to.clone()
                        } else {
                            to.join(rest)
                        };
                        editor.update(cx, |e, cx| e.set_path(new, cx));
                        self.lsp()
                            .clone()
                            .update(cx, |lsp, cx| lsp.editor_path_changed(&editor, cx));
                    }
                }
            }
            FileOp::Delete {
                path,
                recursive,
                ignore_if_not_exists,
            } => {
                let open: Vec<Entity<Editor>> = self
                    .editors_under(path, cx)
                    .into_iter()
                    .map(|(e, _)| e)
                    .collect();
                if delete_path(path, *recursive, *ignore_if_not_exists)? {
                    run.summary.resources += 1;
                    for editor in &open {
                        editor.update(cx, |e, _| e.end_edit_group());
                    }
                    run.edited.retain(|(e, _)| !open.contains(e));
                    self.close_editors(&open, window, cx);
                }
            }
        }
        Ok(())
    }

    /// Closes the tabs of `editors` (asking first when one is dirty).
    fn close_editors(
        &mut self,
        editors: &[Entity<Editor>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for editor in editors {
            for pane in self.panes().to_vec() {
                let ix = pane.read(cx).items().iter().position(|e| e == editor);
                if let Some(ix) = ix {
                    pane.update(cx, |pane, cx| pane.close(ix, window, cx));
                }
            }
        }
    }
}
