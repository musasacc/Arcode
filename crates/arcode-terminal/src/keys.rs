//! Encoding of key presses into the byte sequences xterm-compatible
//! programs expect on their input.
//!
//! Key names follow GPUI conventions (`"enter"`, `"left"`, `"f5"`, `"a"`).
//! For printable text prefer sending the typed text itself (GPUI's
//! `key_char`) and use [`encode_key`] for named keys and control chords.

/// Modifier keys held during a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyMods {
    /// Control.
    pub ctrl: bool,
    /// Alt / Option (sent as an ESC prefix).
    pub alt: bool,
    /// Shift.
    pub shift: bool,
}

impl KeyMods {
    /// No modifiers.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };
    /// Control only.
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };
    /// Alt only.
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
    };
    /// Shift only.
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };

    /// Whether any modifier is held.
    pub fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }

    /// The xterm modifier parameter (`1 + shift + 2*alt + 4*ctrl`).
    pub fn xterm_param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

const ESC: u8 = 0x1b;

/// Encodes a key press for the PTY.
///
/// `app_cursor` is the terminal's application cursor mode (DECCKM), see
/// `TermModeFlags::app_cursor`. Returns `None` for keys that produce no
/// input (lone modifiers, unknown names).
pub fn encode_key(key: &str, mods: KeyMods, app_cursor: bool) -> Option<Vec<u8>> {
    if let Some(seq) = encode_named(key, mods, app_cursor) {
        return Some(seq);
    }
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(encode_char(ch, mods))
}

/// Cursor-style keys: `CSI 1;m X` with modifiers, `SS3 X` in app cursor
/// mode, `CSI X` otherwise.
fn cursor_key(final_byte: u8, mods: KeyMods, app_cursor: bool) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.xterm_param(), char::from(final_byte)).into_bytes()
    } else if app_cursor {
        vec![ESC, b'O', final_byte]
    } else {
        vec![ESC, b'[', final_byte]
    }
}

/// Tilde keys: `CSI n ~` or `CSI n;m ~`.
fn tilde_key(code: u8, mods: KeyMods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{code};{}~", mods.xterm_param()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

/// F1-F4: `SS3 X` or `CSI 1;m X`.
fn ss3_function_key(final_byte: u8, mods: KeyMods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.xterm_param(), char::from(final_byte)).into_bytes()
    } else {
        vec![ESC, b'O', final_byte]
    }
}

/// Prepends ESC when alt is held.
fn with_alt(mods: KeyMods, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if mods.alt {
        out.push(ESC);
    }
    out.extend_from_slice(bytes);
    out
}

fn encode_named(key: &str, mods: KeyMods, app_cursor: bool) -> Option<Vec<u8>> {
    let seq = match key {
        "enter" => with_alt(mods, b"\r"),
        "tab" if mods.shift => b"\x1b[Z".to_vec(),
        "tab" => with_alt(mods, b"\t"),
        "backspace" if mods.ctrl => with_alt(mods, &[0x08]),
        "backspace" => with_alt(mods, &[0x7f]),
        "escape" => with_alt(mods, &[ESC]),
        "space" if mods.ctrl => with_alt(mods, &[0]),
        "space" => with_alt(mods, b" "),
        "up" => cursor_key(b'A', mods, app_cursor),
        "down" => cursor_key(b'B', mods, app_cursor),
        "right" => cursor_key(b'C', mods, app_cursor),
        "left" => cursor_key(b'D', mods, app_cursor),
        "home" => cursor_key(b'H', mods, app_cursor),
        "end" => cursor_key(b'F', mods, app_cursor),
        "insert" => tilde_key(2, mods),
        "delete" => tilde_key(3, mods),
        "pageup" => tilde_key(5, mods),
        "pagedown" => tilde_key(6, mods),
        "f1" => ss3_function_key(b'P', mods),
        "f2" => ss3_function_key(b'Q', mods),
        "f3" => ss3_function_key(b'R', mods),
        "f4" => ss3_function_key(b'S', mods),
        "f5" => tilde_key(15, mods),
        "f6" => tilde_key(17, mods),
        "f7" => tilde_key(18, mods),
        "f8" => tilde_key(19, mods),
        "f9" => tilde_key(20, mods),
        "f10" => tilde_key(21, mods),
        "f11" => tilde_key(23, mods),
        "f12" => tilde_key(24, mods),
        _ => return None,
    };
    Some(seq)
}

/// The C0 control code for `ctrl+ch`, if there is one.
fn control_code(ch: char) -> Option<u8> {
    let code = match ch {
        'a'..='z' => ch as u8 - b'a' + 1,
        'A'..='Z' => ch as u8 - b'A' + 1,
        '@' | '2' | ' ' | '`' => 0,
        '[' | '3' | '{' => 0x1b,
        '\\' | '4' | '|' => 0x1c,
        ']' | '5' | '}' => 0x1d,
        '^' | '6' | '~' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    };
    Some(code)
}

fn encode_char(ch: char, mods: KeyMods) -> Vec<u8> {
    if mods.ctrl
        && let Some(code) = control_code(ch)
    {
        return with_alt(mods, &[code]);
    }
    let ch = if mods.shift && ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    };
    let mut buf = [0u8; 4];
    with_alt(mods, ch.encode_utf8(&mut buf).as_bytes())
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
