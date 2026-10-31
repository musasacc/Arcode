//! Keys while the completion list is shown: up/down/page to move, Enter or
//! Tab to accept (Shift+Enter replaces the whole word), Escape to close,
//! and commit characters.

use gpui::{Context, Keystroke};

use crate::Editor;

impl Editor {
    /// Handles a keystroke for the completion list; `true` consumes it.
    pub(crate) fn completion_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(session) = &mut self.completion.session else {
            return false;
        };
        // An empty list (waiting for a re-request) is not shown: the keys
        // belong to the editor.
        if session.menu.ranked.is_empty() {
            return false;
        }
        let m = &ks.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return false;
        }
        let selected = session.menu.selected;
        let consumed = match (ks.key.as_str(), m.shift) {
            ("up", false) => {
                session.menu.move_by(-1);
                true
            }
            ("down", false) => {
                session.menu.move_by(1);
                true
            }
            ("pageup", false) => {
                session.menu.page(false);
                true
            }
            ("pagedown", false) => {
                session.menu.page(true);
                true
            }
            ("enter", replace) => return self.completion_confirm_at(selected, replace, cx),
            ("tab", false) => return self.completion_confirm_at(selected, false, cx),
            ("escape", _) => {
                self.hide_completions(cx);
                return true;
            }
            _ => {
                // Test platforms leave `key_char` empty for plain keys.
                let typed = ks
                    .key_char
                    .clone()
                    .or_else(|| (!m.shift && ks.key.chars().count() == 1).then(|| ks.key.clone()));
                let commits = typed.as_ref().is_some_and(|ch| {
                    session
                        .selected_item()
                        .is_some_and(|item| item.commit_characters.iter().any(|c| c == ch))
                });
                if commits {
                    // Accept, then let the character be typed as usual.
                    self.completion_confirm_at(selected, false, cx);
                }
                return false;
            }
        };
        if consumed {
            self.completion_push_rows(cx);
            self.completion_resolve_selected(cx);
        }
        consumed
    }

    /// Selects the next (`forward`) or previous completion row.
    pub fn select_completion(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(session) = &mut self.completion.session else {
            return;
        };
        session.menu.move_by(if forward { 1 } else { -1 });
        self.completion_push_rows(cx);
        self.completion_resolve_selected(cx);
    }

    /// Selects row `row` (a mouse click selects before accepting).
    pub(crate) fn completion_click(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.completion.session.is_some() {
            self.completion_confirm_at(row, false, cx);
        }
    }

    /// Resolves the selected item for its documentation, once: a resolve
    /// already in flight for it is kept.
    pub(crate) fn completion_resolve_selected(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.completion.session else {
            return;
        };
        let Some(ranked) = session.menu.ranked.get(session.menu.selected) else {
            return;
        };
        let (id, index) = (session.id, ranked.index);
        let item = &session.items[index];
        let in_flight = self.completion.resolving == Some((id, index));
        if !item.needs_resolve || in_flight {
            return;
        }
        let Some(provider) = self.completion.provider.clone() else {
            return;
        };
        let Some(task) = provider.resolve(self, item, cx) else {
            return;
        };
        self.completion.resolving = Some((id, index));
        self.completion.resolve_task = Some(cx.spawn(async move |this, cx| {
            let resolved = task.await;
            this.update(cx, |editor, cx| {
                if editor.completion.resolving == Some((id, index)) {
                    editor.completion.resolving = None;
                }
                match resolved {
                    Ok(mut resolved) => {
                        resolved.needs_resolve = false;
                        editor.completion_replace_item(id, index, resolved, cx);
                    }
                    Err(err) => {
                        // Not asked again for this list.
                        log::debug!("completion resolve failed: {err:#}");
                        if let Some(s) = editor.completion.session.as_mut().filter(|s| s.id == id)
                            && let Some(item) = s.items.get_mut(index)
                        {
                            item.needs_resolve = false;
                        }
                    }
                }
            })
            .ok();
        }));
    }
}
