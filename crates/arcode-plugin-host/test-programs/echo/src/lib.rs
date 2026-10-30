//! `echo` fixture: every export answers deterministically and every import
//! can be driven from the host, so host tests can check that each WIT type
//! survives the round trip.
//!
//! Results are reported through `host.log` at `info` level as one JSON
//! object per line: `{"export": "...", ...}` or `{"op": "...", ...}`.
//!
//! The single command `echo` takes `{"op": <name>, ...}`; see [`run_op`].

use arcode_plugin::editor::{Position, Range, TextEdit};
use arcode_plugin::prelude::*;
use arcode_plugin::ui::{self, Axis, Cell, Tone, TreeItem};
use serde_json::{Value, json};

#[derive(Default)]
struct Echo {
    panel: Option<Panel>,
}

fn report(value: Value) {
    host::log(host::Level::Info, &value.to_string());
}

fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn u64_arg(args: &Value, key: &str) -> u64 {
    args.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn u32_arg(args: &Value, key: &str) -> u32 {
    u32::try_from(u64_arg(args, key)).unwrap_or(u32::MAX)
}

fn pos(args: &Value, key: &str) -> Position {
    let p = &args[key];
    Position {
        line: u32_arg(p, "line"),
        column: u32_arg(p, "column"),
    }
}

/// A view that uses every widget variant once.
fn every_widget(title: &str) -> ui::Element {
    let item = |key: &str, depth| TreeItem {
        key: key.into(),
        label: key.into(),
        detail: Some("detail".into()),
        icon: Some("file".into()),
        depth,
        expandable: depth == 0,
        expanded: depth == 0,
    };
    ui::vstack([
        ui::text(title).key("title").bold().tone(Tone::Accent),
        ui::badge("badge", Tone::Warning).key("badge"),
        ui::hstack([
            ui::button("Run").key("run").primary().icon("play"),
            ui::spacer(),
        ]),
        ui::input("SELECT 1", "query")
            .key("query")
            .multiline()
            .language("sql")
            .submit_on_enter(),
        ui::select(["a".to_owned(), "b".to_owned()], Some(1)).key("select"),
        ui::checkbox("check", true).key("check"),
        ui::key_values([("k".to_owned(), "v".to_owned())]),
        ui::split(
            Axis::Horizontal,
            0.25,
            ui::tree(vec![item("root", 0), item("child", 1)]).key("tree"),
            ui::tabs(
                0,
                [
                    (
                        "Data".to_owned(),
                        ui::table(vec![ui::column("n"), ui::column("s")], Some(100), 1)
                            .key("table"),
                    ),
                    (
                        "Code".to_owned(),
                        ui::scroll(ui::code("fn main() {}", Some("rust".into()))),
                    ),
                    ("Json".to_owned(), ui::json(r#"{"a": [1, 2]}"#)),
                ],
            ),
        )
        .grow(),
        ui::divider(),
        ui::spinner("loading"),
        ui::empty_state("Nothing", "hint"),
    ])
}

fn run_op(state: &mut Echo, args: &Value) -> Result<()> {
    let op = str_arg(args, "op");
    let result = match op {
        "api-version" => json!(host::api_version()),
        "log" => {
            host::log(host::Level::Warn, str_arg(args, "message"));
            json!(null)
        }
        "notify" => {
            host::notify(host::Level::Error, str_arg(args, "message"));
            json!(null)
        }
        "status" => {
            host::set_status(args["text"].as_str(), args["command"].as_str());
            json!(null)
        }
        "quick-pick" => {
            let items: Vec<String> = ["one", "two", "three"].map(String::from).to_vec();
            json!(host::quick_pick("Pick one", &items))
        }
        "settings" => json!(host::settings()),
        "cancelled" => json!(host::cancelled()),
        "begin-task" => json!(host::begin_task("Echo task")),
        "kv" => {
            let scope = if args["project"].as_bool() == Some(true) {
                kv::Scope::Project
            } else {
                kv::Scope::Global
            };
            let key = str_arg(args, "key");
            let set = kv::set(scope, key, str_arg(args, "value").as_bytes())
                .map_err(|e| format!("{e:?}"));
            let got = kv::get(scope, key).map(|v| String::from_utf8_lossy(&v).into_owned());
            let keys = kv::keys(scope, "");
            kv::delete(scope, "never-set");
            json!({"set": set.err(), "get": got, "keys": keys})
        }
        "kv-get" => {
            let scope = if args["project"].as_bool() == Some(true) {
                kv::Scope::Project
            } else {
                kv::Scope::Global
            };
            let got = kv::get(scope, str_arg(args, "key"))
                .map(|v| String::from_utf8_lossy(&v).into_owned());
            json!({"get": got, "keys": kv::keys(scope, "")})
        }
        "root-name" => json!(workspace::root_name()),
        "find-files" => {
            match workspace::find_files(str_arg(args, "glob"), u32_arg(args, "limit")) {
                Ok(files) => json!({"ok": files}),
                Err(e) => json!({"err": format!("{e:?}")}),
            }
        }
        "read-file" => match std::fs::read_to_string(format!("/project/{}", str_arg(args, "path")))
        {
            Ok(text) => json!({"ok": text}),
            Err(e) => json!({"err": e.kind().to_string()}),
        },
        "editor" => editor_op(args),
        "http" => http_op(args),
        "render" => {
            let panel =
                state
                    .panel
                    .get_or_insert(Panel::open("main", "Echo", Some(ui::Location::Bottom))?);
            panel.rerender(&every_widget(str_arg(args, "title")))?;
            json!(panel.id())
        }
        "close" => {
            if let Some(panel) = state.panel.take() {
                panel.close();
            }
            json!(null)
        }
        "fail" => return Err(Error::msg(str_arg(args, "message"))),
        "panic" => panic!("echo fixture asked to panic"),
        other => return Err(Error::msg(format!("unknown op `{other}`"))),
    };
    report(json!({"op": op, "result": result}));
    Ok(())
}

fn editor_op(args: &Value) -> Value {
    let doc = u64_arg(args, "doc");
    let range = args.get("start").map(|_| Range {
        start: pos(args, "start"),
        end: pos(args, "end"),
    });
    let info = editor::current_document().map(|d| {
        json!({"id": d.id, "path": d.path, "language": d.language,
        "version": d.version, "lines": d.line_count})
    });
    let text = editor::text(doc, range).map_err(|e| format!("{e:?}"));
    let selections = editor::selections(doc)
        .map(|s| {
            s.iter()
                .map(|r| [r.start.line, r.start.column, r.end.line, r.end.column])
                .collect::<Vec<_>>()
        })
        .map_err(|e| format!("{e:?}"));
    let edit = args.get("replace").and_then(Value::as_str).map(|replace| {
        let origin = Position { line: 0, column: 0 };
        let range = range.unwrap_or(Range {
            start: origin,
            end: origin,
        });
        let edits = [TextEdit {
            range,
            text: replace.to_owned(),
        }];
        editor::apply_edits(doc, u64_arg(args, "version"), &edits).map_err(|e| format!("{e:?}"))
    });
    let scratch = args
        .get("scratch")
        .and_then(Value::as_str)
        .map(|t| editor::open_scratch("Scratch", "json", t).map_err(|e| format!("{e:?}")));
    let opened = args.get("open").and_then(Value::as_str).map(|p| {
        editor::open_file(p, Some(Position { line: 1, column: 2 })).map_err(|e| format!("{e:?}"))
    });
    let info_by_id = editor::document_info(doc)
        .map(|d| d.version)
        .map_err(|e| format!("{e:?}"));
    json!({"current": info, "info": info_by_id, "text": text, "selections": selections, "edit": edit,
        "scratch": scratch, "open": opened})
}

fn http_op(args: &Value) -> Value {
    let request = http::Request {
        method: str_arg(args, "method").to_owned(),
        url: str_arg(args, "url").to_owned(),
        headers: vec![("x-echo".into(), "1".into())],
        body: str_arg(args, "body").as_bytes().to_vec(),
        timeout_ms: 5_000,
        follow_redirects: args["follow"].as_bool().unwrap_or(true),
        max_response_bytes: u64_arg(args, "max"),
    };
    match http::send(&request) {
        Ok(r) => {
            json!({"status": r.status, "reason": r.reason, "version": r.version, "final-url": r.final_url,
            "remote-addr": r.remote_addr, "headers": r.headers, "body": String::from_utf8_lossy(&r.body),
            "truncated": r.truncated, "headers-ms": r.timing.headers_ms, "total-ms": r.timing.total_ms})
        }
        Err(e) => json!({"err": format!("{e:?}")}),
    }
}

impl Plugin for Echo {
    fn activate(&mut self, reason: Activation) -> Result<()> {
        report(json!({"export": "activate", "reason": format!("{reason:?}")}));
        Ok(())
    }

    fn deactivate(&mut self) {
        report(json!({"export": "deactivate"}));
    }

    fn command(&mut self, id: &str, args: Json) -> Result<()> {
        if id != "echo" {
            return Err(Error::msg(format!("unknown command `{id}`")));
        }
        let args: Value = args.parse()?;
        run_op(self, &args)
    }

    fn code_lenses(&mut self, doc: &DocInfo) -> Vec<CodeLens> {
        let title = format!(
            "{}|{:?}|{}|{}|{}",
            doc.id, doc.path, doc.language, doc.version, doc.line_count
        );
        vec![CodeLens::new(
            doc.line_count.saturating_sub(1),
            &title,
            "echo",
            &json!({"op": "log", "message": "lens"}),
        )]
    }

    fn open_view(&mut self, panel: PanelId, contribution: &str, args: Json) -> Result<()> {
        report(
            json!({"export": "open-view", "panel": panel, "contribution": contribution, "args": args.as_str()}),
        );
        Panel::attach(panel).rerender(&every_widget(contribution))
    }

    fn ui_event(&mut self, panel: PanelId, event: Event) -> Result<()> {
        report(
            json!({"export": "handle-ui-event", "panel": panel, "node": event.node,
            "kind": format!("{:?}", event.kind), "values": event.values}),
        );
        Ok(())
    }

    fn fetch_rows(
        &mut self,
        panel: PanelId,
        table: NodeId,
        start: u64,
        count: u32,
    ) -> Result<RowPage> {
        report(
            json!({"export": "fetch-rows", "panel": panel, "table": table, "start": start, "count": count}),
        );
        let end = (start + u64::from(count)).min(100);
        let rows = (start..end)
            .map(|i| {
                let n = i64::try_from(i).unwrap_or(i64::MAX);
                vec![
                    Cell::Empty,
                    Cell::Int(-n),
                    Cell::Real(n as f64 / 2.0),
                    Cell::Text(format!("row {i}")),
                    Cell::Boolean(i % 2 == 0),
                    Cell::Blob(i * 10),
                ]
            })
            .collect();
        Ok(RowPage {
            start,
            rows,
            at_end: end >= 100,
        })
    }

    fn view_closed(&mut self, panel: PanelId) {
        report(json!({"export": "view-closed", "panel": panel}));
    }
}

arcode_plugin::export_plugin!(Echo);
