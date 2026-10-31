//! Snippet variables (`$TM_FILENAME`, `$CURRENT_YEAR`, ...).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A calendar date and time (UTC unless the caller supplies local time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Full year, e.g. 2026.
    pub year: i64,
    /// Month 1-12.
    pub month: u32,
    /// Day of month 1-31.
    pub day: u32,
    /// Hour 0-23.
    pub hour: u32,
    /// Minute 0-59.
    pub minute: u32,
    /// Second 0-59.
    pub second: u32,
}

impl DateTime {
    /// The date for a Unix timestamp in seconds (UTC).
    pub fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // Howard Hinnant's civil_from_days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = yoe + era * 400 + i64::from(month <= 2);
        DateTime {
            year,
            month,
            day,
            hour: (rem / 3600) as u32,
            minute: (rem % 3600 / 60) as u32,
            second: (rem % 60) as u32,
        }
    }

    /// The current time in UTC.
    pub fn now_utc() -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        DateTime::from_unix(secs)
    }
}

/// Editor state used to resolve snippet variables.
#[derive(Debug, Clone, Default)]
pub struct VariableContext {
    /// Path of the document, if saved.
    pub file_path: Option<PathBuf>,
    /// Zero-based line of the cursor.
    pub line_index: usize,
    /// Selected text (empty when nothing is selected).
    pub selected_text: String,
    /// Text of the cursor line.
    pub current_line: String,
    /// Word under the cursor.
    pub current_word: String,
    /// Clipboard text, if the app supplies it.
    pub clipboard: Option<String>,
    /// Time for `CURRENT_*`; `None` uses [`DateTime::now_utc`].
    pub now: Option<DateTime>,
}

/// Variable names [`VariableContext::resolve`] knows.
pub const KNOWN_VARIABLES: &[&str] = &[
    "TM_FILENAME",
    "TM_FILENAME_BASE",
    "TM_DIRECTORY",
    "TM_FILEPATH",
    "TM_LINE_INDEX",
    "TM_LINE_NUMBER",
    "TM_SELECTED_TEXT",
    "TM_CURRENT_LINE",
    "TM_CURRENT_WORD",
    "CLIPBOARD",
    "CURRENT_YEAR",
    "CURRENT_YEAR_SHORT",
    "CURRENT_MONTH",
    "CURRENT_DATE",
    "CURRENT_HOUR",
    "CURRENT_MINUTE",
    "CURRENT_SECOND",
    "UUID",
    "RANDOM",
    "RANDOM_HEX",
];

impl VariableContext {
    /// Resolves a variable. `None` means unknown (it becomes a
    /// placeholder); `Some("")` means known but empty (the default is used).
    pub fn resolve(&self, name: &str) -> Option<String> {
        let path = self.file_path.as_deref();
        let os = |s: Option<&std::ffi::OsStr>| s.map(|s| s.to_string_lossy().into_owned());
        let now = || self.now.unwrap_or_else(DateTime::now_utc);
        let value = match name {
            "TM_FILENAME" => os(path.and_then(|p| p.file_name())).unwrap_or_default(),
            "TM_FILENAME_BASE" => os(path.and_then(|p| p.file_stem())).unwrap_or_default(),
            "TM_DIRECTORY" => path
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "TM_FILEPATH" => path
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "TM_LINE_INDEX" => self.line_index.to_string(),
            "TM_LINE_NUMBER" => (self.line_index + 1).to_string(),
            "TM_SELECTED_TEXT" => self.selected_text.clone(),
            "TM_CURRENT_LINE" => self.current_line.clone(),
            "TM_CURRENT_WORD" => self.current_word.clone(),
            "CLIPBOARD" => self.clipboard.clone().unwrap_or_default(),
            "CURRENT_YEAR" => now().year.to_string(),
            "CURRENT_YEAR_SHORT" => format!("{:02}", now().year.rem_euclid(100)),
            "CURRENT_MONTH" => format!("{:02}", now().month),
            "CURRENT_DATE" => format!("{:02}", now().day),
            "CURRENT_HOUR" => format!("{:02}", now().hour),
            "CURRENT_MINUTE" => format!("{:02}", now().minute),
            "CURRENT_SECOND" => format!("{:02}", now().second),
            "UUID" => uuid_v4(),
            "RANDOM" => format!("{:06}", random_u64() % 1_000_000),
            "RANDOM_HEX" => format!("{:06x}", random_u64() & 0xff_ffff),
            _ => return None,
        };
        Some(value)
    }
}

/// Non-cryptographic random number from the std hasher's random keys.
fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
        h.write_u128(d.as_nanos());
    }
    h.finish()
}

/// A random (version 4) UUID string.
pub fn uuid_v4() -> String {
    let hi = random_u64();
    let lo = random_u64();
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&hi.to_be_bytes());
    b[8..].copy_from_slice(&lo.to_be_bytes());
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        let d = DateTime::from_unix(0);
        assert_eq!((d.year, d.month, d.day, d.hour), (1970, 1, 1, 0));
        // 2024-02-29T12:34:56Z
        let d = DateTime::from_unix(1_709_210_096);
        assert_eq!(
            (d.year, d.month, d.day, d.hour, d.minute, d.second),
            (2024, 2, 29, 12, 34, 56)
        );
        let d = DateTime::from_unix(-1);
        assert_eq!((d.year, d.month, d.day, d.second), (1969, 12, 31, 59));
    }

    #[test]
    fn file_variables() {
        let ctx = VariableContext {
            file_path: Some(PathBuf::from("/a/b/main.rs")),
            line_index: 4,
            ..Default::default()
        };
        assert_eq!(ctx.resolve("TM_FILENAME").as_deref(), Some("main.rs"));
        assert_eq!(ctx.resolve("TM_FILENAME_BASE").as_deref(), Some("main"));
        assert_eq!(ctx.resolve("TM_LINE_NUMBER").as_deref(), Some("5"));
        assert_eq!(ctx.resolve("TM_LINE_INDEX").as_deref(), Some("4"));
        assert!(
            ctx.resolve("TM_DIRECTORY")
                .is_some_and(|d| d.ends_with('b'))
        );
        assert_eq!(ctx.resolve("NOPE"), None);
        assert_eq!(ctx.resolve("CLIPBOARD").as_deref(), Some(""));
    }

    #[test]
    fn dates_use_override() {
        let ctx = VariableContext {
            now: Some(DateTime::from_unix(1_709_210_096)),
            ..Default::default()
        };
        assert_eq!(ctx.resolve("CURRENT_YEAR").as_deref(), Some("2024"));
        assert_eq!(ctx.resolve("CURRENT_YEAR_SHORT").as_deref(), Some("24"));
        assert_eq!(ctx.resolve("CURRENT_MONTH").as_deref(), Some("02"));
        assert_eq!(ctx.resolve("CURRENT_DATE").as_deref(), Some("29"));
        assert_eq!(ctx.resolve("CURRENT_SECOND").as_deref(), Some("56"));
    }

    #[test]
    fn uuid_shape() {
        let u = uuid_v4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(u, uuid_v4());
        let ctx = VariableContext::default();
        assert!(KNOWN_VARIABLES.iter().all(|v| ctx.resolve(v).is_some()));
    }
}
