//! Key-by-key application of TOML tables onto settings structs.
//!
//! Nothing here fails: a value of the wrong type or an unknown key becomes a
//! warning and the previous (default) value is kept.

use serde::de::DeserializeOwned;
use toml::Value;

/// A settings struct that can be updated one TOML key at a time.
pub(crate) trait Section {
    /// Every key the section understands, in documentation order.
    const KEYS: &'static [&'static str];

    /// Applies one key. `None` means the key is unknown; `Some(Err)` means the
    /// value could not be converted (the field is left unchanged).
    fn apply_key(&mut self, key: &str, value: &Value) -> Option<Result<(), String>>;
}

/// Converts `value` into `T` and stores it in `slot` on success.
pub(crate) fn assign<T: DeserializeOwned>(slot: &mut T, value: &Value) -> Result<(), String> {
    let parsed = value
        .clone()
        .try_into::<T>()
        .map_err(|err| one_line(&err.to_string()))?;
    *slot = parsed;
    Ok(())
}

/// Applies every key of `value` (which must be a table) to `section`.
/// `path` is the dotted section name used in warnings (e.g. `editor`).
pub(crate) fn apply_table<S: Section>(
    section: &mut S,
    value: &Value,
    path: &str,
    warnings: &mut Vec<String>,
) {
    let Some(table) = value.as_table() else {
        warnings.push(format!(
            "`{path}` must be a table, found {}; using defaults",
            value.type_str()
        ));
        return;
    };
    for (key, value) in table {
        match section.apply_key(key, value) {
            None => warnings.push(format!("unknown key `{path}.{key}` (ignored)")),
            Some(Err(err)) => warnings.push(format!(
                "invalid value for `{path}.{key}`: {err}; using default"
            )),
            Some(Ok(())) => {}
        }
    }
}

/// Collapses a possibly multi-line error message into one line.
pub(crate) fn one_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Implements [`Section`] for a struct by listing its TOML-visible fields.
/// Each key is the field name.
macro_rules! impl_section {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::lenient::Section for $ty {
            const KEYS: &'static [&'static str] = &[$(stringify!($field)),*];

            fn apply_key(
                &mut self,
                key: &str,
                value: &toml::Value,
            ) -> Option<Result<(), String>> {
                match key {
                    $(stringify!($field) => Some($crate::lenient::assign(&mut self.$field, value)),)*
                    _ => None,
                }
            }
        }
    };
}
pub(crate) use impl_section;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Demo {
        size: f32,
        name: String,
    }
    impl_section!(Demo { size, name });

    #[test]
    fn applies_valid_and_reports_invalid() {
        let value: Value = toml::from_str::<toml::Table>("size = 3\nname = 5\nzzz = 1")
            .map(Value::Table)
            .unwrap();
        let mut demo = Demo {
            size: 1.0,
            name: "keep".into(),
        };
        let mut warnings = Vec::new();
        apply_table(&mut demo, &value, "demo", &mut warnings);
        assert_eq!(demo.size, 3.0, "integers are accepted for floats");
        assert_eq!(demo.name, "keep");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("demo.name")));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown key `demo.zzz`"))
        );
    }

    #[test]
    fn non_table_section_warns() {
        let mut demo = Demo::default();
        let mut warnings = Vec::new();
        apply_table(&mut demo, &Value::Integer(1), "demo", &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert_eq!(Demo::KEYS, &["size", "name"]);
    }
}
