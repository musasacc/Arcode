//! Formatting blame lines for inline blame and the blame column.

use arcode_git::BlameLine;

/// Characters of the author name shown in the blame column.
const AUTHOR_WIDTH: usize = 14;
/// Total width (in chars) of a blame column entry.
pub const COLUMN_WIDTH: usize = 7 + 1 + AUTHOR_WIDTH + 1 + 10 + 2;

/// Seconds since the Unix epoch, now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `5 minutes ago`, `3 days ago`, `2 years ago`.
pub fn relative_time(now: i64, then: i64) -> String {
    let secs = (now - then).max(0);
    let units = [
        (365 * 24 * 3600, "year"),
        (30 * 24 * 3600, "month"),
        (7 * 24 * 3600, "week"),
        (24 * 3600, "day"),
        (3600, "hour"),
        (60, "minute"),
    ];
    for (size, name) in units {
        let n = secs / size;
        if n >= 1 {
            let plural = if n == 1 { "" } else { "s" };
            return format!("{n} {name}{plural} ago");
        }
    }
    "just now".into()
}

/// `Alice, 3 days ago • Fix the parser` for the end of the cursor line.
pub fn inline_text(line: &BlameLine, now: i64) -> String {
    if line.is_uncommitted {
        return "You • Uncommitted changes".into();
    }
    let summary = truncate(&line.summary, 72);
    format!(
        "{}, {} • {summary}",
        line.author,
        relative_time(now, line.author_time)
    )
}

/// A fixed-width blame column entry: `abc1234 Alice Smith    2 days ago`.
/// Lines continuing the block of the line above (same commit) are blank,
/// so each change reads as one block.
pub fn column_text(lines: &[BlameLine], ix: usize, now: i64) -> String {
    let Some(line) = lines.get(ix) else {
        return " ".repeat(COLUMN_WIDTH);
    };
    let continues = ix > 0 && lines.get(ix - 1).is_some_and(|p| p.commit == line.commit);
    let text = if continues {
        String::new()
    } else if line.is_uncommitted {
        format!("{:<7} {:<w$}", "", "Uncommitted", w = AUTHOR_WIDTH)
    } else {
        let date = short_age(now, line.author_time);
        format!(
            "{:<7} {:<w$} {date:>10}",
            truncate(&line.commit, 7),
            truncate(&line.author, AUTHOR_WIDTH),
            w = AUTHOR_WIDTH
        )
    };
    pad(&text, COLUMN_WIDTH)
}

/// `2d ago`-style ages that fit the column.
fn short_age(now: i64, then: i64) -> String {
    let full = relative_time(now, then);
    full.replace(" minutes", "m")
        .replace(" minute", "m")
        .replace(" hours", "h")
        .replace(" hour", "h")
        .replace(" days", "d")
        .replace(" day", "d")
        .replace(" weeks", "w")
        .replace(" week", "w")
        .replace(" months", "mo")
        .replace(" month", "mo")
        .replace(" years", "y")
        .replace(" year", "y")
}

/// Cuts `text` to at most `max` chars, ending with `…` when cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Pads (or cuts) `text` to exactly `width` chars.
fn pad(text: &str, width: usize) -> String {
    let mut out = truncate(text, width);
    let len = out.chars().count();
    out.extend(std::iter::repeat_n(' ', width - len));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(commit: &str, author: &str, time: i64) -> BlameLine {
        BlameLine {
            line: 0,
            commit: commit.into(),
            author: author.into(),
            author_time: time,
            summary: "Fix the parser".into(),
            is_uncommitted: commit.bytes().all(|b| b == b'0'),
        }
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(100, 90), "just now");
        assert_eq!(relative_time(3600 * 5, 0), "5 hours ago");
        assert_eq!(relative_time(86_400, 0), "1 day ago");
        assert_eq!(relative_time(86_400 * 800, 0), "2 years ago");
        assert_eq!(relative_time(0, 100), "just now", "future dates clamp");
    }

    #[test]
    fn inline_and_column_text() {
        let now = 86_400 * 3;
        let a = line("abc1234", "Alice", 0);
        assert_eq!(inline_text(&a, now), "Alice, 3 days ago • Fix the parser");
        assert_eq!(
            inline_text(&line("0000000", "Not Committed Yet", 0), now),
            "You • Uncommitted changes"
        );
        let lines = [a.clone(), a, line("def5678", "A very long author name", 0)];
        let first = column_text(&lines, 0, now);
        assert_eq!(first.chars().count(), COLUMN_WIDTH);
        assert!(first.starts_with("abc1234 Alice "), "{first:?}");
        assert!(first.trim_end().ends_with("3d ago"), "{first:?}");
        assert_eq!(column_text(&lines, 1, now).trim(), "");
        assert!(column_text(&lines, 2, now).contains("A very long a…"));
        assert_eq!(column_text(&lines, 9, now).chars().count(), COLUMN_WIDTH);
    }
}
