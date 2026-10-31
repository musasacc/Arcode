//! JSON-RPC 2.0 message construction and classification.

use serde_json::{Value, json};

use crate::error::{LspError, Result};

/// A decoded message received from the server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    /// Answer to one of our requests.
    Response {
        /// The id we sent (number or string as the server echoed it).
        id: Value,
        /// The `result`, or the `error` converted to [`LspError::Rpc`].
        result: Result<Value>,
    },
    /// A request the server sends to us; it expects a response.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A notification from the server.
    Notification { method: String, params: Value },
}

/// Classifies one message body.
pub(crate) fn parse_incoming(text: &str) -> Result<Incoming> {
    let mut value: Value = serde_json::from_str(text)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| LspError::Protocol("message is not a JSON object".into()))?;
    let id = obj.remove("id");
    let params = obj.remove("params").unwrap_or(Value::Null);
    if let Some(method) = obj.get("method").and_then(Value::as_str).map(str::to_owned) {
        return Ok(match id {
            Some(id) if !id.is_null() => Incoming::Request { id, method, params },
            _ => Incoming::Notification { method, params },
        });
    }
    let id = id.ok_or_else(|| LspError::Protocol("message has neither method nor id".into()))?;
    let result = match obj.remove("error") {
        Some(err) if !err.is_null() => Err(LspError::Rpc {
            code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        }),
        _ => Ok(obj.remove("result").unwrap_or(Value::Null)),
    };
    Ok(Incoming::Response { id, result })
}

/// Interprets a response id as one of our numeric ids.
pub(crate) fn numeric_id(id: &Value) -> Option<i64> {
    match id {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Serializes a request. `params: null` is omitted as allowed by JSON-RPC.
pub(crate) fn request(id: i64, method: &str, params: Value) -> String {
    if params.is_null() {
        json!({ "jsonrpc": "2.0", "id": id, "method": method }).to_string()
    } else {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }
}

/// Serializes a notification. `params: null` is omitted as allowed by JSON-RPC.
pub(crate) fn notification(method: &str, params: Value) -> String {
    if params.is_null() {
        json!({ "jsonrpc": "2.0", "method": method }).to_string()
    } else {
        json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
    }
}

/// Serializes a successful response.
pub(crate) fn response_ok(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// Serializes an error response.
pub(crate) fn response_err(id: &Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_messages() {
        let r = parse_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"a":1}}"#).unwrap();
        assert_eq!(
            r,
            Incoming::Response {
                id: json!(3),
                result: Ok(json!({"a":1}))
            }
        );
        let r =
            parse_incoming(r#"{"jsonrpc":"2.0","id":"7","error":{"code":-32601,"message":"no"}}"#)
                .unwrap();
        match r {
            Incoming::Response { id, result } => {
                assert_eq!(numeric_id(&id), Some(7));
                assert_eq!(
                    result,
                    Err(LspError::Rpc {
                        code: -32601,
                        message: "no".into()
                    })
                );
            }
            other => panic!("{other:?}"),
        }
        let r = parse_incoming(r#"{"jsonrpc":"2.0","id":1,"method":"m","params":[1]}"#).unwrap();
        assert!(matches!(r, Incoming::Request { ref method, .. } if method == "m"));
        let r = parse_incoming(r#"{"jsonrpc":"2.0","method":"n"}"#).unwrap();
        assert_eq!(
            r,
            Incoming::Notification {
                method: "n".into(),
                params: Value::Null
            }
        );
        let r = parse_incoming(r#"{"jsonrpc":"2.0","id":1}"#).unwrap();
        assert!(matches!(
            r,
            Incoming::Response {
                result: Ok(Value::Null),
                ..
            }
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_incoming("[1,2]").is_err());
        assert!(parse_incoming("{").is_err());
        assert!(parse_incoming(r#"{"jsonrpc":"2.0"}"#).is_err());
    }

    #[test]
    fn serializes() {
        let v: Value = serde_json::from_str(&request(1, "x", json!({}))).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "x");
        let v: Value = serde_json::from_str(&notification("exit", Value::Null)).unwrap();
        assert!(v.get("params").is_none());
        let v: Value = serde_json::from_str(&response_err(&json!("a"), -1, "m")).unwrap();
        assert_eq!(v["error"]["code"], -1);
        assert_eq!(v["id"], "a");
    }
}
