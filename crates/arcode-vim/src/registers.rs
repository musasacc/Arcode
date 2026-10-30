//! Registers: unnamed `"`, yank `0`, deletes `1`-`9`, small delete `-`,
//! named `a`-`z` (`A`-`Z` append), black hole `_`, clipboard `+`/`*`
//! (mirrored locally; the editor syncs the system clipboard) and the
//! read-only `.` (last insert), `:` (last ex command), `/` (last search).

use std::collections::HashMap;

use crate::types::{Register, RegisterKind};

/// True for a name valid after `"`.
pub(crate) fn valid_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '"' | '-' | '_' | '+' | '*' | '.' | ':' | '/')
}

/// True for the clipboard registers.
pub(crate) fn is_clipboard(c: char) -> bool {
    c == '+' || c == '*'
}

/// All register contents.
#[derive(Debug, Clone, Default)]
pub(crate) struct Registers {
    map: HashMap<char, Register>,
}

impl Registers {
    /// Register `name` (`"` = unnamed, uppercase reads the lowercase one).
    pub(crate) fn get(&self, name: char) -> Option<&Register> {
        self.map.get(&name.to_ascii_lowercase())
    }

    /// Sets register `name` directly (no unnamed/numbered side effects).
    pub(crate) fn set(&mut self, name: char, reg: Register) {
        self.map.insert(name.to_ascii_lowercase(), reg);
    }

    /// Stores yanked (`yank`) or deleted text the way Vim does for
    /// register `name` (`None` = no register given).
    pub(crate) fn store(&mut self, name: Option<char>, reg: Register, yank: bool) {
        match name {
            Some('_') => {}
            Some(c) if c.is_ascii_uppercase() => {
                let lower = c.to_ascii_lowercase();
                let combined = match self.map.remove(&lower) {
                    Some(old) => append(old, reg),
                    None => reg,
                };
                self.map.insert(lower, combined.clone());
                self.map.insert('"', combined);
            }
            Some(c) if c != '"' => {
                self.map.insert(c, reg.clone());
                self.map.insert('"', reg);
            }
            _ => {
                if yank {
                    self.map.insert('0', reg.clone());
                } else if reg.kind == RegisterKind::Charwise && !reg.text().contains('\n') {
                    self.map.insert('-', reg.clone());
                } else {
                    for n in (1..9u8).rev() {
                        let from = char::from(b'0' + n);
                        if let Some(r) = self.map.remove(&from) {
                            self.map.insert(char::from(b'1' + n), r);
                        }
                    }
                    self.map.insert('1', reg.clone());
                }
                self.map.insert('"', reg);
            }
        }
    }
}

/// `old` with `new` appended (linewise if either is).
fn append(old: Register, new: Register) -> Register {
    if old.kind == RegisterKind::Charwise && new.kind == RegisterKind::Charwise {
        return Register::new(old.text() + &new.text(), RegisterKind::Charwise);
    }
    let mut text = old.text();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&new.text());
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Register::new(text, RegisterKind::Linewise)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cw(s: &str) -> Register {
        Register::new(s, RegisterKind::Charwise)
    }

    #[test]
    fn store_rules() {
        let mut r = Registers::default();
        r.store(None, cw("y"), true);
        assert_eq!(r.get('0'), Some(&cw("y")));
        r.store(None, cw("small"), false);
        assert_eq!(r.get('-'), Some(&cw("small")));
        assert_eq!(r.get('"'), Some(&cw("small")));
        assert_eq!(r.get('0'), Some(&cw("y")));
        let line = |s: &str| Register::new(s, RegisterKind::Linewise);
        r.store(None, line("one\n"), false);
        r.store(None, line("two\n"), false);
        assert_eq!(r.get('1'), Some(&line("two\n")));
        assert_eq!(r.get('2'), Some(&line("one\n")));
        r.store(Some('a'), cw("x"), true);
        r.store(Some('A'), cw("z"), true);
        assert_eq!(r.get('a'), Some(&cw("xz")));
        r.store(Some('A'), line("l\n"), true);
        assert_eq!(r.get('a'), Some(&line("xz\nl\n")));
        r.store(Some('_'), cw("gone"), false);
        assert_eq!(r.get('"'), Some(&line("xz\nl\n")));
        assert!(valid_name('a') && valid_name('+') && !valid_name('!'));
    }
}
