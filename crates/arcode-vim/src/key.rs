//! Keystrokes forwarded by the editor and a parser for Vim key notation.

/// A keystroke as forwarded by the editor view.
///
/// `key` is either the **text the key produces** (a single char such as
/// `"a"`, `"A"`, `"$"`, `"<"`; with GPUI pass `key_char` when present) or a
/// key name: `escape`, `enter`, `backspace`, `tab`, `delete`, `left`,
/// `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`, `space`.
/// Names are case-insensitive. For a single lowercase letter with `shift`
/// set, the uppercase letter is used; `shift` is otherwise ignored for
/// single chars (they already carry it).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Key {
    /// The produced char or the key name.
    pub key: String,
    /// Control held.
    pub ctrl: bool,
    /// Alt / Option held.
    pub alt: bool,
    /// Shift held.
    pub shift: bool,
}

/// A normalized key, as the state machine sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tok {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Anything Vim does not handle (alt combos, function keys, ...).
    Other,
}

impl Key {
    /// A plain key (`"a"`, `"escape"`, ...).
    pub fn new(key: impl Into<String>) -> Self {
        Key {
            key: key.into(),
            ..Key::default()
        }
    }

    /// A key with Control held (`Key::ctrl("d")` is Ctrl-D).
    pub fn ctrl(key: impl Into<String>) -> Self {
        Key {
            key: key.into(),
            ctrl: true,
            ..Key::default()
        }
    }

    /// A key producing `c`.
    pub fn char(c: char) -> Self {
        Key::new(c.to_string())
    }

    /// Parses Vim key notation: plain chars plus `<esc>`, `<cr>`/`<enter>`,
    /// `<bs>`, `<tab>`, `<del>`, `<left>`/`<right>`/`<up>`/`<down>`,
    /// `<home>`, `<end>`, `<pageup>`, `<pagedown>`, `<space>`, `<lt>` (a
    /// literal `<`), `<bar>`, `<c-x>` (Ctrl), `<a-x>`/`<m-x>` (Alt) and
    /// `<s-x>` (Shift). A `<` that does not start a known name is literal.
    pub fn parse_keys(notation: &str) -> Vec<Key> {
        let chars: Vec<char> = notation.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '<'
                && let Some(close) = chars[i + 1..].iter().take(12).position(|&c| c == '>')
            {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                if let Some(key) = named(&inner) {
                    out.push(key);
                    i += close + 2;
                    continue;
                }
            }
            out.push(Key::char(chars[i]));
            i += 1;
        }
        out
    }

    /// The normalized form used by the state machine.
    pub(crate) fn tok(&self) -> Tok {
        let mut chars = self.key.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if self.alt {
                return Tok::Other;
            }
            let c = if self.shift && c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            };
            if self.ctrl {
                return match c.to_ascii_lowercase() {
                    '[' | 'c' => Tok::Esc,
                    l => Tok::Ctrl(l),
                };
            }
            return Tok::Char(c);
        }
        let name = self.key.to_ascii_lowercase();
        if self.alt || (self.ctrl && name != "space") {
            return Tok::Other;
        }
        match name.as_str() {
            "escape" | "esc" => Tok::Esc,
            "enter" | "return" => Tok::Enter,
            "backspace" => Tok::Backspace,
            "tab" if !self.shift => Tok::Tab,
            "delete" => Tok::Delete,
            "left" => Tok::Left,
            "right" => Tok::Right,
            "up" => Tok::Up,
            "down" => Tok::Down,
            "home" => Tok::Home,
            "end" => Tok::End,
            "pageup" => Tok::PageUp,
            "pagedown" => Tok::PageDown,
            "space" if !self.ctrl => Tok::Char(' '),
            _ => Tok::Other,
        }
    }
}

/// The key for a `<...>` name, if known.
fn named(inner: &str) -> Option<Key> {
    let lower = inner.to_ascii_lowercase();
    let plain = |s: &str| Some(Key::new(s));
    match lower.as_str() {
        "esc" => return plain("escape"),
        "cr" | "enter" | "return" => return plain("enter"),
        "bs" => return plain("backspace"),
        "tab" => return plain("tab"),
        "del" => return plain("delete"),
        "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown" => {
            return plain(&lower);
        }
        "space" => return plain(" "),
        "lt" => return plain("<"),
        "bar" => return plain("|"),
        _ => {}
    }
    let (prefix, rest) = lower.split_once('-')?;
    if rest.is_empty() {
        return None;
    }
    // Keep the case of a single-char rest (`<c-R>` stays `R`).
    let rest_orig = &inner[prefix.len() + 1..];
    let mut key = named(rest_orig).unwrap_or_else(|| Key::new(rest_orig));
    if key.key.chars().count() != 1 && named(rest_orig).is_none() {
        return None;
    }
    match prefix {
        "c" => key.ctrl = true,
        "a" | "m" => key.alt = true,
        "s" => key.shift = true,
        _ => return None,
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation() {
        let keys = Key::parse_keys("d<esc><c-d><lt>x<bogus>");
        let toks: Vec<Tok> = keys.iter().map(Key::tok).collect();
        assert_eq!(
            toks,
            vec![
                Tok::Char('d'),
                Tok::Esc,
                Tok::Ctrl('d'),
                Tok::Char('<'),
                Tok::Char('x'),
                Tok::Char('<'),
                Tok::Char('b'),
                Tok::Char('o'),
                Tok::Char('g'),
                Tok::Char('u'),
                Tok::Char('s'),
                Tok::Char('>'),
            ]
        );
    }

    #[test]
    fn modifiers() {
        let shifted = Key {
            key: "a".into(),
            shift: true,
            ..Key::default()
        };
        assert_eq!(shifted.tok(), Tok::Char('A'));
        assert_eq!(Key::ctrl("[").tok(), Tok::Esc);
        assert_eq!(Key::ctrl("c").tok(), Tok::Esc);
        assert_eq!(Key::ctrl("left").tok(), Tok::Other);
        assert_eq!(Key::new("Escape").tok(), Tok::Esc);
        assert_eq!(Key::new("space").tok(), Tok::Char(' '));
        let alt = Key {
            key: "x".into(),
            alt: true,
            ..Key::default()
        };
        assert_eq!(alt.tok(), Tok::Other);
        assert_eq!(Key::parse_keys("<a-x>")[0], alt);
        assert_eq!(Key::parse_keys("<s-tab>")[0].tok(), Tok::Other);
        assert_eq!(Key::parse_keys("<cr>")[0].tok(), Tok::Enter);
    }
}
