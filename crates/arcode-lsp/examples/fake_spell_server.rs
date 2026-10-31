//! A tiny language server for trying several servers on one file (e.g.
//! next to rust-analyzer) without installing a real second server.
//!
//! It flags the misspelling `teh` (a warning with source `fake-spell`),
//! answers hover on such a word, offers a quick fix replacing it and one
//! completion item, `thesaurus`. Build it with
//! `cargo build -p arcode-lsp --example fake_spell_server` and configure:
//!
//! ```toml
//! [language.rust]
//! servers = ["rust-analyzer", "fake-spell"]
//! [lsp.fake-spell]
//! command = "/path/to/target/debug/examples/fake_spell_server"
//! ```

use std::collections::HashMap;

use arcode_lsp::transport::{read_message, write_message};
use serde_json::{Value, json};
use tokio::io::{AsyncWrite, BufReader};

const WORD: &str = "teh";

async fn send<W: AsyncWrite + Unpin>(out: &mut W, msg: Value) {
    if write_message(out, &msg.to_string()).await.is_err() {
        std::process::exit(1);
    }
}

/// `(line, start, end)` of every misspelling (UTF-16 columns; ASCII word).
fn misspellings(text: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (line, content) in text.lines().enumerate() {
        let units: Vec<u16> = content.encode_utf16().collect();
        let word: Vec<u16> = WORD.encode_utf16().collect();
        let is_word = |u: u16| char::from_u32(u as u32).is_some_and(|c| c.is_alphanumeric());
        let mut i = 0;
        while i + word.len() <= units.len() {
            let before = i == 0 || !is_word(units[i - 1]);
            let after = units.get(i + word.len()).is_none_or(|u| !is_word(*u));
            if units[i..i + word.len()] == word[..] && before && after {
                out.push((line, i, i + word.len()));
                i += word.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

fn range(line: usize, start: usize, end: usize) -> Value {
    json!({ "start": { "line": line, "character": start }, "end": { "line": line, "character": end } })
}

async fn publish<W: AsyncWrite + Unpin>(out: &mut W, uri: &Value, text: &str) {
    let diagnostics: Vec<Value> = misspellings(text)
        .into_iter()
        .map(|(l, s, e)| {
            json!({
                "range": range(l, s, e),
                "severity": 2,
                "source": "fake-spell",
                "message": format!("Unknown word `{WORD}`; did you mean `the`?"),
            })
        })
        .collect();
    let params = json!({ "uri": uri, "diagnostics": diagnostics });
    let msg =
        json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params });
    send(out, msg).await;
}

fn at(text: &str, params: &Value) -> Option<(usize, usize, usize)> {
    let line = params["position"]["line"].as_u64()? as usize;
    let col = params["position"]["character"].as_u64()? as usize;
    misspellings(text)
        .into_iter()
        .find(|&(l, s, e)| l == line && (s..=e).contains(&col))
}

fn answer(id: Value, msg: &str, params: &Value, docs: &HashMap<String, String>) -> Value {
    let uri = params["textDocument"]["uri"].clone();
    let text = docs
        .get(uri.as_str().unwrap_or_default())
        .map_or("", String::as_str);
    let result = match msg {
        "initialize" => json!({
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": 1 },
                "hoverProvider": true,
                "completionProvider": {},
                "codeActionProvider": true,
            },
            "serverInfo": { "name": "fake-spell" },
        }),
        "textDocument/hover" => match at(text, params) {
            Some((l, s, e)) => json!({
                "contents": { "kind": "markdown", "value": "**fake-spell**: `teh` is not a word. Did you mean `the`?" },
                "range": range(l, s, e),
            }),
            None => Value::Null,
        },
        "textDocument/completion" => json!([
            { "label": "thesaurus", "kind": 1, "detail": "fake-spell word" }
        ]),
        "textDocument/codeAction" => {
            let line = params["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
            let fixes: Vec<Value> = misspellings(text)
                .into_iter()
                .filter(|&(l, _, _)| l == line)
                .map(|(l, s, e)| {
                    let edit = json!({ "changes": { uri.as_str().unwrap_or_default(): [
                        { "range": range(l, s, e), "newText": "the" }
                    ] } });
                    json!({ "title": "Change `teh` to `the`", "kind": "quickfix", "edit": edit })
                })
                .collect();
            json!(fixes)
        }
        _ => Value::Null,
    };
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        eprintln!("fake-spell: started");
        let mut input = BufReader::new(tokio::io::stdin());
        let mut out = tokio::io::stdout();
        let mut docs: HashMap<String, String> = HashMap::new();
        while let Ok(Some(text)) = read_message(&mut input).await {
            let Ok(msg) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let method = msg["method"].as_str().unwrap_or_default().to_string();
            let params = &msg["params"];
            let uri = params["textDocument"]["uri"].clone();
            let key = uri.as_str().unwrap_or_default().to_string();
            match (method.as_str(), msg.get("id").cloned()) {
                ("exit", _) => std::process::exit(0),
                ("textDocument/didOpen", None) => {
                    let text = params["textDocument"]["text"].as_str().unwrap_or_default();
                    eprintln!("fake-spell: opened {key}");
                    docs.insert(key, text.to_string());
                    publish(&mut out, &uri, text).await;
                }
                ("textDocument/didChange", None) => {
                    let text = params["contentChanges"][0]["text"]
                        .as_str()
                        .unwrap_or_default();
                    docs.insert(key, text.to_string());
                    publish(&mut out, &uri, text).await;
                }
                ("textDocument/didClose", None) => {
                    docs.remove(&key);
                    publish(&mut out, &uri, "").await;
                }
                (_, Some(id)) => send(&mut out, answer(id, &method, params, &docs)).await,
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_whole_words_only() {
        let found = misspellings("teh cat\n// see teh, not tehx or ateh\n");
        assert_eq!(found, vec![(0, 0, 3), (1, 7, 10)]);
    }
}
