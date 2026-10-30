//! Line-ending detection and normalization.

use std::borrow::Cow;

/// The line ending a file uses on disk. In memory every buffer uses `\n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineEnding {
    /// Unix style `\n`.
    #[default]
    Lf,
    /// Windows style `\r\n`.
    CrLf,
}

impl LineEnding {
    /// The platform's native line ending (`CrLf` on Windows, `Lf` elsewhere).
    pub fn native() -> Self {
        if cfg!(windows) {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }

    /// The textual form of this line ending.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }

    /// Detects the dominant line ending of `text`.
    ///
    /// `\r\n` wins only if it occurs more often than a bare `\n`; texts
    /// without any line break are reported as [`LineEnding::Lf`].
    pub fn detect(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut crlf = 0usize;
        let mut lf = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                if i > 0 && bytes[i - 1] == b'\r' {
                    crlf += 1;
                } else {
                    lf += 1;
                }
            }
        }
        if crlf > lf {
            LineEnding::CrLf
        } else {
            LineEnding::Lf
        }
    }
}

/// Converts `\r\n` and lone `\r` to `\n`. Borrows when nothing changes.
pub fn normalize(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_line_endings() {
        assert_eq!(LineEnding::detect("a\nb\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\r\n"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\n"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\r\nb\nc\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("no newline"), LineEnding::Lf);
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }

    #[test]
    fn normalize_variants() {
        assert!(matches!(normalize("a\nb"), Cow::Borrowed(_)));
        assert_eq!(normalize("a\r\nb\rc\n"), "a\nb\nc\n");
        assert_eq!(normalize("\r\r\n"), "\n\n");
        assert_eq!(normalize("é\r\n😀"), "é\n😀");
    }

    #[test]
    fn as_str_and_native() {
        assert_eq!(LineEnding::Lf.as_str(), "\n");
        assert_eq!(LineEnding::CrLf.as_str(), "\r\n");
        let native = LineEnding::native();
        if cfg!(windows) {
            assert_eq!(native, LineEnding::CrLf);
        } else {
            assert_eq!(native, LineEnding::Lf);
        }
    }
}
