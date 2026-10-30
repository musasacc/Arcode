//! `fs-escape` fixture: tries to leave the `/project` preopen and to write
//! into it, reporting every outcome as one JSON line through `host.log`:
//! `{"path": ..., "op": "read"|"write"|"list", "ok": ..., "errno": ..., "kind": ...}`.

use std::io::ErrorKind;

use arcode_plugin::prelude::*;
use serde_json::{Value, json};

#[derive(Default)]
struct FsEscape;

fn report(op: &str, path: &str, result: std::io::Result<String>) {
    let line = match result {
        Ok(text) => json!({"op": op, "path": path, "ok": text}),
        Err(e) => json!({"op": op, "path": path, "ok": null, "errno": e.raw_os_error(),
            "kind": format!("{:?}", e.kind()), "denied": e.kind() == ErrorKind::PermissionDenied}),
    };
    host::log(host::Level::Info, &line.to_string());
}

impl Plugin for FsEscape {
    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        for path in args["read"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            report("read", path, std::fs::read_to_string(path));
        }
        for path in args["list"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let listing = std::fs::read_dir(path).map(|entries| {
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                names.join(",")
            });
            report("list", path, listing);
        }
        for path in args["write"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            report("write", path, std::fs::write(path, b"written").map(|()| String::new()));
        }
        Ok(())
    }
}

arcode_plugin::export_plugin!(FsEscape);
