//! `{{variable}}` substitution (docs/PLUGINS.md §9).
//!
//! Names are looked up in one map: the selected environment's values with
//! the file's `@name = value` definitions on top (file wins, as in VS Code
//! REST Client). Values may reference other variables; cycles are an error.
//!
//! Dynamic variables are fresh for every occurrence:
//! - `{{$uuid}}`, `{{$random.uuid}}`, `{{$guid}}`: a random v4 UUID;
//! - `{{$timestamp}}`: Unix time in seconds; `{{$isoTimestamp}}` (also
//!   `{{$datetime iso8601}}`): UTC, millisecond precision;
//! - `{{$randomInt}}`: 0..=1000; `{{$randomInt min max}}` and
//!   `{{$random.integer(min, max)}}`: `min..max`.
//!
//! Randomness and time come from WASI (`random`, `clocks`) through
//! [`Dynamic`]; tests pass a deterministic one.

use std::collections::BTreeMap;

/// How deep variable values may reference other variables.
const MAX_DEPTH: usize = 16;

/// Why a text could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarError {
    /// `{{name}}` is not defined.
    Unknown(String),
    /// A value references itself, directly or not.
    Cycle(String),
    /// A dynamic variable this sandbox cannot provide (`$processEnv`, ...).
    Unsupported(String),
    /// Bad arguments to a dynamic variable.
    BadArguments(String),
}

impl VarError {
    /// The message shown to the user.
    pub fn message(&self) -> String {
        match self {
            Self::Unknown(n) => {
                format!(
                    "unknown variable {{{{{n}}}}}: define it with `@{n} = ...` or in http-client.env.json"
                )
            }
            Self::Cycle(n) => format!("variable {{{{{n}}}}} references itself"),
            Self::Unsupported(n) => format!("{{{{{n}}}}} is not available in the plugin sandbox"),
            Self::BadArguments(n) => format!("invalid arguments in {{{{{n}}}}}"),
        }
    }
}

/// Time and randomness for dynamic variables.
pub trait Dynamic {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&mut self) -> u64;
    /// 64 random bits.
    fn random_u64(&mut self) -> u64;
}

/// The real source: WASI clocks, and WASI random through std's
/// randomly keyed SipHash (no extra crate needed).
#[derive(Debug, Default)]
pub struct SystemDynamic {
    counter: u64,
}

impl Dynamic for SystemDynamic {
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn random_u64(&mut self) -> u64 {
        use std::hash::{BuildHasher, Hasher};
        self.counter = self.counter.wrapping_add(1);
        // `RandomState::new()` keys come from the OS (WASI `random`).
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(self.counter);
        h.finish()
    }
}

/// Resolves every `{{...}}` in `text`. An unterminated `{{` is kept as is.
pub fn resolve(
    text: &str,
    vars: &BTreeMap<String, String>,
    dynamic: &mut dyn Dynamic,
) -> Result<String, VarError> {
    let mut stack = Vec::new();
    resolve_inner(text, vars, dynamic, &mut stack)
}

fn resolve_inner(
    text: &str,
    vars: &BTreeMap<String, String>,
    dynamic: &mut dyn Dynamic,
    stack: &mut Vec<String>,
) -> Result<String, VarError> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else {
            break;
        };
        out.push_str(&rest[..open]);
        let inner = rest[open + 2..open + 2 + close].trim();
        rest = &rest[open + 2 + close + 2..];
        if inner.starts_with('$') {
            out.push_str(&dynamic_value(inner, dynamic)?);
            continue;
        }
        let value = vars
            .get(inner)
            .ok_or_else(|| VarError::Unknown(inner.to_owned()))?;
        if stack.iter().any(|s| s == inner) || stack.len() >= MAX_DEPTH {
            return Err(VarError::Cycle(inner.to_owned()));
        }
        stack.push(inner.to_owned());
        out.push_str(&resolve_inner(value, vars, dynamic, stack)?);
        stack.pop();
    }
    out.push_str(rest);
    Ok(out)
}

/// The value of one `$...` variable.
fn dynamic_value(expr: &str, dynamic: &mut dyn Dynamic) -> Result<String, VarError> {
    let (name, args) = match expr.find(|c: char| c.is_whitespace() || c == '(') {
        Some(i) => (&expr[..i], expr[i..].trim()),
        None => (expr, ""),
    };
    let bad = || VarError::BadArguments(expr.to_owned());
    match (name, args) {
        ("$uuid" | "$random.uuid" | "$guid", "") => Ok(uuid_v4(dynamic)),
        ("$timestamp", "") => Ok((dynamic.now_ms() / 1000).to_string()),
        ("$isoTimestamp", "") | ("$datetime", "iso8601") => Ok(iso8601(dynamic.now_ms())),
        ("$randomInt", "") => Ok((dynamic.random_u64() % 1001).to_string()),
        ("$randomInt", args) => {
            let bounds: Vec<&str> = args.split_whitespace().collect();
            random_between(&bounds, dynamic).ok_or_else(bad)
        }
        ("$random.integer", args) => {
            let inner = args
                .strip_prefix('(')
                .and_then(|a| a.strip_suffix(')'))
                .ok_or_else(bad)?;
            let bounds: Vec<&str> = inner.split(',').map(str::trim).collect();
            random_between(&bounds, dynamic).ok_or_else(bad)
        }
        ("$uuid" | "$random.uuid" | "$guid" | "$timestamp" | "$isoTimestamp", _) => Err(bad()),
        _ => Err(VarError::Unsupported(name.to_owned())),
    }
}

/// A random integer in `min..max` (`max` exclusive).
fn random_between(bounds: &[&str], dynamic: &mut dyn Dynamic) -> Option<String> {
    let [min, max] = bounds else { return None };
    let (min, max): (i64, i64) = (min.parse().ok()?, max.parse().ok()?);
    let span = u64::try_from(i128::from(max) - i128::from(min)).ok()?;
    if span == 0 {
        return None;
    }
    let offset = i64::try_from(dynamic.random_u64() % span).ok()?;
    Some(min.checked_add(offset)?.to_string())
}

/// A lower-case random (version 4, variant 1) UUID.
pub fn uuid_v4(dynamic: &mut dyn Dynamic) -> String {
    let hi = (dynamic.random_u64() & !0xF000) | 0x4000;
    let lo = (dynamic.random_u64() & !(0b11 << 62)) | (0b10 << 62);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        hi & 0xFFFF,
        lo >> 48,
        lo & 0xFFFF_FFFF_FFFF
    )
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for milliseconds since the epoch.
pub fn iso8601(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        ms % 1000
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian
/// (H. Hinnant's algorithm, for non-negative day counts).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
#[path = "vars_tests.rs"]
mod tests;
