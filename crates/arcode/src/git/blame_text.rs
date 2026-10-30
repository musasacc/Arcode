//! Formatting blame lines for inline blame and the blame column.

use arcode_git::BlameLine;
use unicode_width::UnicodeWidthChar;

/// Display cells of the author name shown in the blame column.
const AUTHOR_WIDTH: usize = 14;
/// Total width (in display cells) of a blame column entry.
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
    let summary = truncate(&line.summary, 72, &unicode_cells);
    format!(
        "{}, {} • {summary}",
        line.author,
        relative_time(now, line.author_time)
    )
}

/// A fixed-width blame column entry: `abc1234 Alice Smith    2 days ago`.
/// Lines continuing the block of the line above (same commit) are blank,
/// so each change reads as one block. Widths are Unicode display widths
/// (wide CJK characters take two cells).
pub fn column_text(lines: &[BlameLine], ix: usize, now: i64) -> String {
    column_text_with(lines, ix, now, &unicode_cells)
}

/// Like [`column_text`] with the width of each char (in cells of the
/// editor font) given by `cells`: the column is inline text before the
/// code, so to line the code up the padding must follow the glyphs' real
/// advances, which for fallback fonts (CJK) are not whole cells.
pub fn column_text_with(
    lines: &[BlameLine],
    ix: usize,
    now: i64,
    cells: &dyn Fn(char) -> f32,
) -> String {
    let pad = |text: &str, width: usize| pad(text, width, cells);
    let truncate = |text: &str, max: usize| truncate(text, max, cells);
    let Some(line) = lines.get(ix) else {
        return " ".repeat(COLUMN_WIDTH);
    };
    let continues = ix > 0 && lines.get(ix - 1).is_some_and(|p| p.commit == line.commit);
    let text = if continues {
        String::new()
    } else if line.is_uncommitted {
        format!("{:<7} {}", "", pad("Uncommitted", AUTHOR_WIDTH))
    } else {
        let date = short_age(now, line.author_time);
        format!(
            "{:<7} {} {date:>10}",
            truncate(&line.commit, 7),
            pad(&line.author, AUTHOR_WIDTH),
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

/// Display cells of `c` in a monospace font (wide CJK characters take two).
pub fn unicode_cells(c: char) -> f32 {
    UnicodeWidthChar::width(c).unwrap_or(0) as f32
}

fn width_of(text: &str, cells: &dyn Fn(char) -> f32) -> f32 {
    text.chars().map(cells).sum()
}

/// Cuts `text` to at most `max` cells, ending with `…` when cut.
fn truncate(text: &str, max: usize, cells: &dyn Fn(char) -> f32) -> String {
    let max = max as f32;
    if width_of(text, cells) <= max + 0.01 {
        return text.to_owned();
    }
    let room = max - cells('…');
    let mut out = String::new();
    let mut used = 0.0;
    for c in text.chars() {
        let w = cells(c);
        if used + w > room + 0.01 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Pads (or cuts) `text` to `width` cells, so the code after a blame column
/// entry lines up whatever the author's script. When glyphs are not whole
/// cells (CJK in a fallback font), ideographic spaces (as wide as those
/// glyphs) are mixed in to land as close to `width` as possible.
fn pad(text: &str, width: usize, cells: &dyn Fn(char) -> f32) -> String {
    let mut out = truncate(text, width, cells);
    let need = width as f32 - width_of(&out, cells);
    let ideo = cells(IDEOGRAPHIC_SPACE);
    let mut best = (need.round().max(0.0) as usize, 0);
    let mut best_err = (need - best.0 as f32).abs();
    if ideo > 0.0 {
        for wide in 1..=4 {
            let rest = need - wide as f32 * ideo;
            if rest < -0.5 {
                break;
            }
            let spaces = rest.round().max(0.0) as usize;
            let err = (rest - spaces as f32).abs();
            if err + 0.01 < best_err {
                (best, best_err) = ((spaces, wide), err);
            }
        }
    }
    out.extend(std::iter::repeat_n(IDEOGRAPHIC_SPACE, best.1));
    out.extend(std::iter::repeat_n(' ', best.0));
    out
}

const IDEOGRAPHIC_SPACE: char = '\u{3000}';

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

    #[test]
    fn column_padding_follows_measured_widths() {
        // A fallback font whose CJK glyphs are 1.6 cells wide.
        let cells = |c: char| if c.is_ascii() { 1.0 } else { 1.6 };
        let lines = [line("abc1234", "张伟张伟", 0), line("def5678", "Bob", 0)];
        let cjk = column_text_with(&lines, 0, 0, &cells);
        let ascii = column_text_with(&lines, 1, 0, &cells);
        let width = |t: &str| t.chars().map(cells).sum::<f32>();
        assert_eq!(width(&ascii), COLUMN_WIDTH as f32);
        assert!((width(&cjk) - COLUMN_WIDTH as f32).abs() < 0.05, "{cjk:?}");
        let long = [line("abc1234", "张伟张伟张伟张伟张伟", 0)];
        let cut = column_text_with(&long, 0, 0, &cells);
        assert!(cut.contains('…'), "{cut:?}");
        assert!((width(&cut) - COLUMN_WIDTH as f32).abs() <= 0.5, "{cut:?}");
    }
}
