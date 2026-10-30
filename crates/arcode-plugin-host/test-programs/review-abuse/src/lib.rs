//! `review-abuse` fixture (security review): drives host imports the way a
//! hostile plugin would, one operation per `run` command.
//!
//! The command `run` takes `{"op": <name>, ...}` (see [`run_op`]) and
//! reports `{"op": <name>, "result": ...}` as one JSON line via `host.log`
//! (except the log floods, which report through `host.log` afterwards).

use std::io::Write;

use arcode_plugin::bindings::arcode::plugin::ui as raw;
use arcode_plugin::prelude::*;
use serde_json::{Value, json};

/// Files kept open across calls (`hold-files`).
#[derive(Default)]
struct Abuse {
    held: Vec<std::fs::File>,
}

fn report(op: &str, result: Value) {
    host::log(
        host::Level::Info,
        &json!({"op": op, "result": result}).to_string(),
    );
}

fn u64_arg(args: &Value, key: &str) -> u64 {
    args[key].as_u64().unwrap_or_default()
}

fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().unwrap_or_default()
}

fn text_node(id: u32, text: String) -> raw::Node {
    raw::Node {
        id,
        widget: raw::Widget::Text(raw::TextProps {
            text,
            tone: raw::Tone::Normal,
            mono: false,
            bold: false,
            wrap: false,
        }),
        children: Vec::new(),
        grow: false,
    }
}

fn stack_node(children: Vec<u32>) -> raw::Node {
    raw::Node {
        id: 1,
        widget: raw::Widget::Stack(raw::StackProps {
            axis: raw::Axis::Vertical,
            gap: 0,
            padding: 0,
        }),
        children,
        grow: false,
    }
}

/// Renders a stack, then adds one `mib`-MiB text node per call to
/// `apply-patches`; the guest keeps only one copy of the text.
fn panel_grow(args: &Value) -> Value {
    let calls = u64_arg(args, "calls") as u32;
    let mib = u64_arg(args, "mib") as usize;
    let panel = match raw::open_panel("main", "Abuse", None) {
        Ok(p) => p,
        Err(e) => return json!({"err": format!("{e:?}")}),
    };
    let view = raw::View {
        root: 1,
        nodes: vec![stack_node(vec![2]), text_node(2, String::new())],
    };
    if let Err(e) = raw::render(panel, &view) {
        return json!({"err": format!("{e:?}")});
    }
    let mut accepted = 0u32;
    for i in 0..calls {
        let id = 3 + i;
        let patches = [
            raw::Patch::Upsert(text_node(id, "x".repeat(mib * 1024 * 1024))),
            raw::Patch::Upsert(stack_node((2..=id).collect())),
        ];
        match raw::apply_patches(panel, &patches) {
            Ok(()) => accepted += 1,
            Err(e) => return json!({"accepted": accepted, "err": format!("{e:?}")}),
        }
    }
    json!({"accepted": accepted})
}

fn run_op(state: &mut Abuse, args: &Value) -> Result<()> {
    let op = str_arg(args, "op");
    let result = match op {
        "sleep" => {
            std::thread::sleep(std::time::Duration::from_millis(u64_arg(args, "ms")));
            json!({"cancelled": host::cancelled()})
        }
        "hold-files" => {
            let path = str_arg(args, "path");
            let max = u64_arg(args, "max") as usize;
            let mut error = None;
            while state.held.len() < max {
                match std::fs::File::open(path) {
                    Ok(f) => state.held.push(f),
                    Err(e) => {
                        error = Some(e.to_string());
                        break;
                    }
                }
            }
            json!({"held": state.held.len(), "err": error})
        }
        "release-files" => {
            state.held.clear();
            json!(null)
        }
        "symlink" => {
            // `soft_link` is the stable spelling of WASI `symlink-at`.
            #[allow(deprecated)]
            let made = std::fs::soft_link(str_arg(args, "target"), str_arg(args, "link"));
            match made {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"err": e.to_string()}),
            }
        }
        "open-file" => match editor::open_file(str_arg(args, "path"), None) {
            Ok(()) => json!({"ok": true}),
            Err(e) => json!({"err": format!("{e:?}")}),
        },
        "editor-text" => match editor::text(u64_arg(args, "doc"), None) {
            Ok(text) => json!({"ok": text}),
            Err(e) => json!({"err": format!("{e:?}")}),
        },
        "stdout-newlines" => {
            let chunk = vec![b'\n'; 64 * 1024];
            let mut out = std::io::stdout().lock();
            for _ in 0..u64_arg(args, "mib") * 16 {
                let _ = out.write_all(&chunk);
            }
            let _ = out.flush();
            json!(null)
        }
        "log-empty" => {
            for _ in 0..u64_arg(args, "count") {
                host::log(host::Level::Info, "");
            }
            json!(null)
        }
        "status-flood" => {
            let text = "s".repeat(u64_arg(args, "bytes") as usize);
            for _ in 0..u64_arg(args, "count") {
                host::set_status(Some(&text), None);
            }
            json!(null)
        }
        "panel-grow" => panel_grow(args),
        "find-loop" => {
            let mut calls = 0u64;
            while !host::cancelled() {
                let _ = workspace::find_files("**", 1);
                calls += 1;
            }
            json!({"calls": calls})
        }
        other => return Err(Error::msg(format!("unknown op `{other}`"))),
    };
    report(op, result);
    Ok(())
}

impl Plugin for Abuse {
    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        if id != "run" {
            return Err(Error::msg(format!("unknown command `{id}`")));
        }
        let args: Value = args.parse()?;
        run_op(self, &args)
    }
}

arcode_plugin::export_plugin!(Abuse);
