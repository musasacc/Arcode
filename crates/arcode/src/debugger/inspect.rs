//! Threads, frames, scopes, variables, watches and the REPL.

use std::collections::HashSet;

use arcode_dap::{EvaluateContext, Scope, StackFrame, Variable};
use gpui::Context;

use super::console::LineKind;
use super::session::{NodePath, WatchValue, child_path, first_frame_with_source, frame_location};
use super::{Debugger, DebuggerEvent};

/// Which tree nodes are open. Scopes are open unless the adapter marks
/// them expensive or the user closed them; other nodes are closed unless
/// opened. Kept by path, so expansion survives steps.
#[derive(Debug, Default)]
pub struct TreeState {
    opened: HashSet<NodePath>,
    closed: HashSet<NodePath>,
}

impl TreeState {
    /// Whether `path` is open (`default`: open when never toggled).
    pub fn is_open(&self, path: &str, default: bool) -> bool {
        if default {
            !self.closed.contains(path)
        } else {
            self.opened.contains(path)
        }
    }

    /// Opens or closes `path`; returns the new state.
    pub fn toggle(&mut self, path: &str, default: bool) -> bool {
        let open = !self.is_open(path, default);
        if open {
            self.closed.remove(path);
            self.opened.insert(path.to_string());
        } else {
            self.opened.remove(path);
            self.closed.insert(path.to_string());
        }
        open
    }
}

/// The path of a scope node.
pub fn scope_path(scope: &Scope) -> NodePath {
    scope.name.clone()
}

/// Whether a scope starts open: locals and arguments do, registers and
/// expensive scopes do not.
pub fn scope_open_by_default(scope: &Scope) -> bool {
    let registers = scope.presentation_hint.as_deref() == Some("registers")
        || scope.name.to_lowercase().contains("register");
    !scope.expensive && !registers
}

/// The path of a watch node.
pub fn watch_path(ix: usize) -> NodePath {
    format!("\u{1}watch{ix}")
}

impl Debugger {
    /// Whether a variables-tree node is open.
    pub fn is_open(&self, path: &str, default: bool) -> bool {
        self.tree.is_open(path, default)
    }

    /// Lists threads after a stop and shows the stopped one.
    pub(super) fn load_threads(&mut self, focus: Option<i64>, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let request = session.handle.threads();
        let generation = session.generation;
        cx.spawn(async move |this, cx| {
            let threads = request.await;
            this.update(cx, |d, cx| {
                let Some(s) = d.session.as_mut().filter(|s| s.generation == generation) else {
                    return;
                };
                match threads {
                    Ok(threads) => s.threads = threads,
                    Err(e) => return d.info(LineKind::Error, format!("Threads: {e}"), cx),
                }
                let thread = focus
                    .or(s.thread)
                    .filter(|t| s.threads.iter().any(|x| x.id == *t))
                    .or_else(|| s.threads.first().map(|t| t.id));
                s.thread = thread;
                if let Some(thread) = thread {
                    d.load_frames(thread, true, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows the frames of `thread` (selecting the first one with a source
    /// file, and jumping there when `reveal`).
    pub fn load_frames(&mut self, thread: i64, reveal: bool, cx: &mut Context<Self>) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.thread = Some(thread);
        let request = session.handle.stack_trace(thread);
        let generation = session.generation;
        cx.spawn(async move |this, cx| {
            let frames = request.await;
            this.update(cx, |d, cx| {
                let Some(s) = d.session.as_mut().filter(|s| s.generation == generation) else {
                    return;
                };
                if s.thread != Some(thread) {
                    return;
                }
                match frames {
                    Ok(frames) => {
                        let first = first_frame_with_source(&frames).cloned();
                        s.frames = frames;
                        if let Some(frame) = first {
                            d.select_frame(&frame, reveal, cx);
                        }
                    }
                    Err(e) => d.info(LineKind::Error, format!("Call stack: {e}"), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Selects a frame: marks its line, loads its scopes and evaluates the
    /// watches there.
    pub fn select_frame(&mut self, frame: &StackFrame, reveal: bool, cx: &mut Context<Self>) {
        let Some(session) = &mut self.session else {
            return;
        };
        session.frame = Some(frame.id);
        session.scopes.clear();
        session.children.clear();
        session.loading.clear();
        let request = session.handle.scopes(frame.id);
        let generation = session.generation;
        self.stopped_at = frame_location(frame);
        self.refresh_all_editors(cx);
        if reveal
            && let Some((path, line)) = self.stopped_at.clone()
            && path.is_file()
        {
            cx.emit(DebuggerEvent::Open {
                path,
                line,
                focus: false,
            });
        }
        let frame_id = frame.id;
        cx.spawn(async move |this, cx| {
            let scopes = request.await;
            this.update(cx, |d, cx| {
                let Some(s) = d.session.as_mut().filter(|s| s.generation == generation) else {
                    return;
                };
                if s.frame != Some(frame_id) {
                    return;
                }
                match scopes {
                    Ok(scopes) => {
                        s.scopes = scopes.clone();
                        for scope in scopes {
                            if d.tree
                                .is_open(&scope_path(&scope), scope_open_by_default(&scope))
                            {
                                d.load_children(scope.variables_reference, scope_path(&scope), cx);
                            }
                        }
                    }
                    Err(e) => d.info(LineKind::Error, format!("Scopes: {e}"), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.evaluate_watches(cx);
        cx.notify();
    }

    /// Loads the children of `reference` (shown at `path`) and, below
    /// them, of every node that is open.
    pub(super) fn load_children(&mut self, reference: i64, path: NodePath, cx: &mut Context<Self>) {
        let Some(s) = &mut self.session else {
            return;
        };
        if reference <= 0 || s.children.contains_key(&reference) || !s.loading.insert(reference) {
            return;
        }
        let request = s.handle.variables(reference);
        let generation = s.generation;
        cx.spawn(async move |this, cx| {
            let vars = request.await;
            this.update(cx, |d, cx| {
                let Some(s) = d.session.as_mut().filter(|s| s.generation == generation) else {
                    return;
                };
                s.loading.remove(&reference);
                let vars: Vec<Variable> = match vars {
                    Ok(vars) => vars,
                    Err(e) => vec![Variable {
                        name: "error".into(),
                        value: e.to_string(),
                        ..Default::default()
                    }],
                };
                s.children.insert(reference, vars.clone());
                for v in vars {
                    let p = child_path(&path, &v.name);
                    if v.variables_reference > 0 && d.tree.is_open(&p, false) {
                        d.load_children(v.variables_reference, p, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens or closes a tree node; opening loads its children.
    pub fn toggle_node(
        &mut self,
        path: NodePath,
        reference: i64,
        default: bool,
        cx: &mut Context<Self>,
    ) {
        if self.tree.toggle(&path, default) {
            self.load_children(reference, path, cx);
        }
        cx.notify();
    }

    /// Adds a watch expression.
    pub fn add_watch(&mut self, expression: String, cx: &mut Context<Self>) {
        let expression = expression.trim().to_string();
        if expression.is_empty() {
            return;
        }
        self.watches.push((expression, WatchValue::Pending));
        self.evaluate_watches(cx);
        cx.notify();
    }

    /// Removes watch `ix`.
    pub fn remove_watch(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.watches.len() {
            self.watches.remove(ix);
            cx.notify();
        }
    }

    fn evaluate_watches(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.session.as_ref().filter(|s| s.is_stopped()) else {
            return;
        };
        let (frame, generation) = (s.frame, s.generation);
        for (ix, (expression, _)) in self.watches.iter().enumerate() {
            let request = s
                .handle
                .evaluate(expression.clone(), frame, EvaluateContext::Watch);
            let expression = expression.clone();
            cx.spawn(async move |this, cx| {
                let result = request.await;
                this.update(cx, |d, cx| {
                    if d.session
                        .as_ref()
                        .is_none_or(|s| s.generation != generation)
                    {
                        return;
                    }
                    let Some(slot) = d.watches.get_mut(ix).filter(|w| w.0 == expression) else {
                        return;
                    };
                    slot.1 = match result {
                        Ok(body) => WatchValue::Value {
                            value: body.result,
                            type_: body.type_,
                            reference: body.variables_reference,
                        },
                        Err(e) => WatchValue::Error(e.to_string()),
                    };
                    if let WatchValue::Value { reference, .. } = slot.1
                        && d.tree.is_open(&watch_path(ix), false)
                    {
                        d.load_children(reference, watch_path(ix), cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Evaluates REPL input in the selected frame and prints the result.
    pub fn evaluate_repl(&mut self, input: String, cx: &mut Context<Self>) {
        let input = input.trim().to_string();
        if input.is_empty() {
            return;
        }
        self.console.remember(&input);
        self.console.line(LineKind::Input, format!("> {input}"));
        self.console_changed(cx);
        let Some(s) = self.session.as_ref().filter(|s| !s.is_terminated()) else {
            self.info(LineKind::Error, "No debug session is running".into(), cx);
            return;
        };
        let (handle, frame) = (s.handle.clone(), s.frame);
        let request = handle.evaluate(input.clone(), frame, EvaluateContext::Repl);
        cx.spawn(async move |this, cx| {
            let mut result = request.await;
            // GDB runs REPL input as a GDB command: input that is not one
            // (`a + b`) is evaluated as an expression instead.
            if let Err(e) = &result
                && is_not_a_debugger_command(&e.to_string())
            {
                result = handle.evaluate(input, frame, EvaluateContext::Watch).await;
            }
            this.update(cx, |d, cx| match result {
                Ok(body) => d.info(LineKind::Result, body.result, cx),
                Err(e) => d.info(LineKind::Error, e.to_string(), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Previous REPL input.
    pub fn history_up(&mut self) -> Option<String> {
        self.console.history_up().map(str::to_string)
    }

    /// Next REPL input (`Some("")` past the newest).
    pub fn history_down(&mut self) -> Option<String> {
        self.console.history_down().map(str::to_string)
    }
}

/// Whether an adapter rejected REPL input because it is not one of its
/// commands (GDB: `Undefined command: "x"`, `Ambiguous command "a + b"`).
fn is_not_a_debugger_command(message: &str) -> bool {
    message.contains("Undefined command") || message.contains("Ambiguous command")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gdb_command_errors_are_recognised() {
        assert!(is_not_a_debugger_command(
            "evaluate failed: Ambiguous command \"a + b\": actions, ..."
        ));
        assert!(is_not_a_debugger_command(
            "Undefined command: \"nosuchvar\".  Try \"help\"."
        ));
        assert!(!is_not_a_debugger_command(
            "No symbol \"x\" in current context."
        ));
    }

    #[test]
    fn scopes_default_open_and_variables_default_closed() {
        let mut t = TreeState::default();
        assert!(t.is_open("Locals", true));
        assert!(!t.is_open("Locals/x", false));
        assert!(!t.toggle("Locals", true));
        assert!(!t.is_open("Locals", true));
        assert!(t.toggle("Locals/x", false));
        assert!(t.is_open("Locals/x", false));
        assert!(t.toggle("Locals", true));
        assert!(t.is_open("Locals", true));
    }
}
