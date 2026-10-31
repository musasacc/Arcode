//! What Vim's ex commands need from the workspace: `:q` / `:q!` / `:wq`
//! close the editor's tab, `:e <file>` opens a file. The editor runs
//! everything else itself (see `arcode_editor::vim`).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use arcode_editor::{Editor, VimRequest};
use gpui::{App, Context, Entity, Window};

use crate::workspace::Workspace;

/// Installs the handler (once, at startup).
pub fn init(cx: &mut App) {
    arcode_editor::set_vim_request_handler(
        Rc::new(|editor, request, window, cx| {
            let Some(Some(workspace)) = window.root::<Workspace>() else {
                return;
            };
            let editor = editor.clone();
            // The keystroke is still being dispatched: act right after it.
            window.defer(cx, move |window, cx| {
                workspace.update(cx, |ws, cx| handle(ws, &editor, request, window, cx));
            });
        }),
        cx,
    );
}

fn handle(
    ws: &mut Workspace,
    editor: &Entity<Editor>,
    request: VimRequest,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    match request {
        VimRequest::Close { force } => {
            // Prefer the active pane when the editor is shown in several.
            let mut panes = ws.panes().to_vec();
            let active = ws.active_pane().clone();
            panes.sort_by_key(|p| *p != active);
            for pane in panes {
                let index = pane.read(cx).items().iter().position(|e| e == editor);
                if let Some(index) = index {
                    pane.update(cx, |pane, cx| {
                        if force {
                            pane.close_discarding(index, window, cx);
                        } else {
                            pane.close(index, window, cx);
                        }
                    });
                    return;
                }
            }
        }
        VimRequest::Open(typed) => {
            let root = ws.project().map(|p| p.root.clone());
            let current = editor.read(cx).path().map(Path::to_path_buf);
            let path = resolve(&typed, root.as_deref(), current.as_deref());
            if path.is_file() {
                ws.open_file(&path, window, cx);
            } else {
                ws.show_toast(format!("E447: Can't find file \"{typed}\""), cx);
            }
        }
    }
}

/// The file `:e typed` means: `~/` is the home folder, an absolute path
/// stays, anything else is relative to the project root (or, without a
/// project, to the current file's folder).
pub fn resolve(typed: &str, root: Option<&Path>, current: Option<&Path>) -> PathBuf {
    if let Some(rest) = typed.strip_prefix("~/")
        && let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
    {
        return PathBuf::from(home).join(rest);
    }
    let path = Path::new(typed);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let base = root
        .map(Path::to_path_buf)
        .or_else(|| current.and_then(Path::parent).map(Path::to_path_buf));
    match base {
        Some(base) => base.join(path),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_against_the_project() {
        let root = Path::new("/p");
        assert_eq!(
            resolve("src/a.rs", Some(root), None),
            PathBuf::from("/p/src/a.rs")
        );
        assert_eq!(
            resolve("b.rs", None, Some(Path::new("/x/y/a.rs"))),
            PathBuf::from("/x/y/b.rs")
        );
        let abs = std::env::temp_dir().join("c.rs");
        assert_eq!(resolve(abs.to_str().unwrap(), Some(root), None), abs);
    }
}
