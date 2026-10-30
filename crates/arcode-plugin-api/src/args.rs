//! Typed JSON arguments and the SDK error type.
//!
//! WIT carries command arguments, code-lens arguments and settings as JSON
//! text (`types.json`). [`Json`] wraps that text and (de)serializes it with
//! serde so plugin code works with ordinary Rust structs.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bindings::exports::arcode::plugin::guest::CodeLens;

/// Result type used by every [`crate::Plugin`] method.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// An error returned to the host as a plain message.
///
/// Like `anyhow::Error`, any `Display` type converts into it with `?`. It
/// deliberately does not implement `Display` itself (that would collide with
/// the blanket conversion); use [`Error::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// Creates an error from a message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// The message shown to the user.
    pub fn message(&self) -> &str {
        &self.0
    }

    /// Consumes the error, returning its message.
    pub fn into_message(self) -> String {
        self.0
    }
}

impl<E: std::fmt::Display> From<E> for Error {
    fn from(error: E) -> Self {
        Self(error.to_string())
    }
}

/// JSON text as it crosses the WIT boundary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Json(String);

impl Json {
    /// The JSON `null` literal, used when a call carries no arguments.
    pub fn null() -> Self {
        Self("null".to_owned())
    }

    /// Serializes `value`. Serialization of plain data cannot fail; if it
    /// does (for example a map with non-string keys) this yields `null`.
    pub fn from_value<T: Serialize + ?Sized>(value: &T) -> Self {
        Self(serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()))
    }

    /// Deserializes the text into `T`. Empty text is treated as `null`.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        let text = if self.0.trim().is_empty() {
            "null"
        } else {
            &self.0
        };
        serde_json::from_str(text).map_err(|e| Error::msg(format!("invalid arguments: {e}")))
    }

    /// The raw JSON text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper, returning the raw JSON text.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for Json {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl CodeLens {
    /// A lens on zero-based `line` that runs the plugin's `command` with
    /// `args` serialized as JSON when clicked.
    pub fn new<A: Serialize + ?Sized>(line: u32, title: &str, command: &str, args: &A) -> Self {
        Self {
            line,
            title: title.to_owned(),
            command: command.to_owned(),
            args: Json::from_value(args).into_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct SendArgs {
        doc: u64,
        line: u32,
    }

    #[test]
    fn json_round_trips_typed_args() {
        let json = Json::from_value(&SendArgs { doc: 7, line: 3 });
        assert_eq!(json.parse::<SendArgs>(), Ok(SendArgs { doc: 7, line: 3 }));
    }

    #[test]
    fn empty_json_is_null() {
        assert_eq!(Json::default().parse::<Option<u32>>(), Ok(None));
        assert_eq!(Json::null().parse::<Option<u32>>(), Ok(None));
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        let err = Json::from("{".to_owned()).parse::<SendArgs>().unwrap_err();
        assert!(err.message().starts_with("invalid arguments"));
    }

    #[test]
    fn code_lens_serializes_args() {
        let lens = CodeLens::new(4, "Send", "send", &SendArgs { doc: 1, line: 4 });
        assert_eq!(lens.args, r#"{"doc":1,"line":4}"#);
        assert_eq!(lens.command, "send");
    }

    #[test]
    fn errors_convert_from_display_types() {
        fn fails() -> Result<()> {
            Err(std::fmt::Error)?;
            Ok(())
        }
        assert_eq!(
            fails().unwrap_err().message(),
            "an error occurred when formatting an argument"
        );
    }
}
