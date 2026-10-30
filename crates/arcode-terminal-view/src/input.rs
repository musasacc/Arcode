//! Pure input helpers: deciding how a key press reaches the PTY, wheel
//! accumulation and mouse wheel reports.

use arcode_terminal::{GridPoint, KeyMods, encode_key};
use gpui::Modifiers;

/// What to do with a key press that no action consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInput {
    /// Write these bytes to the PTY and stop propagation.
    Bytes(Vec<u8>),
    /// Plain text: let it propagate so the platform delivers it (with IME
    /// composition and dead keys) through the input handler.
    Text,
    /// Not for the terminal (app shortcut, lone modifier, unknown key).
    Ignore,
}

/// Classifies a key press. `key` and `key_char` follow GPUI's `Keystroke`
/// (`key` is the unmodified key name, `key_char` the typed text).
/// `option_composes` is true on macOS, where Option types characters
/// instead of acting as Meta.
pub fn classify_key(
    key: &str,
    key_char: Option<&str>,
    mods: Modifiers,
    app_cursor: bool,
    option_composes: bool,
) -> KeyInput {
    if mods.platform {
        return KeyInput::Ignore;
    }
    let tmods = KeyMods {
        ctrl: mods.control,
        alt: mods.alt,
        shift: mods.shift,
    };
    let bytes = |key: &str, m: KeyMods| match encode_key(key, m, app_cursor) {
        Some(b) => KeyInput::Bytes(b),
        None => KeyInput::Ignore,
    };
    let is_named = key.chars().nth(1).is_some();
    if is_named {
        if key == "space" && !mods.control && !mods.alt && key_char.is_some() {
            return KeyInput::Text;
        }
        return bytes(key, tmods);
    }
    if mods.control {
        return bytes(key, tmods);
    }
    if mods.alt {
        let typed = key_char.filter(|c| c.chars().count() == 1);
        if option_composes && typed.is_some_and(|c| c != key) {
            return KeyInput::Text;
        }
        // Meta: ESC prefix plus the shifted character.
        let ch = typed.unwrap_or(key);
        return bytes(
            ch,
            KeyMods {
                alt: true,
                ..KeyMods::NONE
            },
        );
    }
    if key_char.is_some() {
        KeyInput::Text
    } else {
        bytes(key, tmods)
    }
}

/// Adds a wheel delta (pixels, positive = towards older content) to
/// `remainder` and returns the whole lines to scroll, keeping the fraction.
pub fn wheel_lines(remainder: &mut f32, delta_px: f32, line_height: f32) -> i32 {
    if !(line_height > 0.) || !delta_px.is_finite() {
        return 0;
    }
    let total = *remainder + delta_px / line_height;
    let lines = total.trunc();
    *remainder = total - lines;
    lines.clamp(-10_000., 10_000.) as i32
}

/// An xterm mouse wheel report for the program, at a grid cell.
pub fn wheel_report(up: bool, at: GridPoint, sgr: bool) -> Vec<u8> {
    let button: u32 = if up { 64 } else { 65 };
    let (col, row) = (at.col + 1, at.row + 1);
    if sgr {
        format!("\x1b[<{button};{col};{row}M").into_bytes()
    } else {
        // Legacy X10 encoding: one byte per value, offset by 32.
        let enc = |v: usize| (32 + v.min(223)) as u8;
        vec![0x1b, b'[', b'M', (32 + button) as u8, enc(col), enc(row)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(control: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            control,
            alt,
            shift,
            ..Modifiers::default()
        }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    #[test]
    fn plain_text_goes_through_input_handler() {
        assert_eq!(
            classify_key("a", Some("a"), none(), false, false),
            KeyInput::Text
        );
        assert_eq!(
            classify_key("a", Some("A"), m(false, false, true), false, false),
            KeyInput::Text
        );
        assert_eq!(
            classify_key("space", Some(" "), none(), false, false),
            KeyInput::Text
        );
    }

    #[test]
    fn named_keys_are_encoded() {
        let k = |key, mods, app| classify_key(key, None, mods, app, false);
        assert_eq!(k("enter", none(), false), KeyInput::Bytes(b"\r".to_vec()));
        assert_eq!(k("backspace", none(), false), KeyInput::Bytes(vec![0x7f]));
        assert_eq!(k("up", none(), false), KeyInput::Bytes(b"\x1b[A".to_vec()));
        assert_eq!(k("up", none(), true), KeyInput::Bytes(b"\x1bOA".to_vec()));
        assert_eq!(
            k("tab", m(false, false, true), false),
            KeyInput::Bytes(b"\x1b[Z".to_vec())
        );
        assert_eq!(k("escape", none(), false), KeyInput::Bytes(vec![0x1b]));
        // Enter reports a key_char on some platforms; it must still be encoded.
        assert_eq!(
            classify_key("enter", Some("\r"), none(), false, false),
            KeyInput::Bytes(b"\r".to_vec())
        );
        assert_eq!(k("shift", m(false, false, true), false), KeyInput::Ignore);
    }

    #[test]
    fn control_chords() {
        assert_eq!(
            classify_key("c", Some("c"), m(true, false, false), false, false),
            KeyInput::Bytes(vec![3])
        );
        assert_eq!(
            classify_key("space", None, m(true, false, false), false, false),
            KeyInput::Bytes(vec![0])
        );
        assert_eq!(
            classify_key("d", None, m(true, true, false), false, false),
            KeyInput::Bytes(vec![0x1b, 4])
        );
    }

    #[test]
    fn alt_is_meta_except_composing_option() {
        assert_eq!(
            classify_key("b", Some("b"), m(false, true, false), false, false),
            KeyInput::Bytes(b"\x1bb".to_vec())
        );
        assert_eq!(
            classify_key("1", Some("!"), m(false, true, true), false, false),
            KeyInput::Bytes(b"\x1b!".to_vec())
        );
        // macOS: Option+L types "@" on a German layout.
        assert_eq!(
            classify_key("l", Some("@"), m(false, true, false), false, true),
            KeyInput::Text
        );
        // ...but Option+key without a composed char is still Meta.
        assert_eq!(
            classify_key("b", Some("b"), m(false, true, false), false, true),
            KeyInput::Bytes(b"\x1bb".to_vec())
        );
    }

    #[test]
    fn platform_shortcuts_are_ignored() {
        let cmd = Modifiers {
            platform: true,
            ..Modifiers::default()
        };
        assert_eq!(
            classify_key("c", Some("c"), cmd, false, true),
            KeyInput::Ignore
        );
    }

    #[test]
    fn wheel_accumulates_fractions() {
        let mut rem = 0.;
        assert_eq!(wheel_lines(&mut rem, 10., 20.), 0);
        assert_eq!(wheel_lines(&mut rem, 10., 20.), 1);
        assert!(rem.abs() < 1e-6);
        assert_eq!(wheel_lines(&mut rem, -65., 20.), -3);
        assert!((rem + 0.25).abs() < 1e-6);
        assert_eq!(wheel_lines(&mut rem, 5., 0.), 0);
        assert_eq!(wheel_lines(&mut rem, f32::NAN, 20.), 0);
    }

    #[test]
    fn wheel_reports() {
        let p = GridPoint::new(4, 9);
        assert_eq!(wheel_report(true, p, true), b"\x1b[<64;10;5M".to_vec());
        assert_eq!(wheel_report(false, p, true), b"\x1b[<65;10;5M".to_vec());
        assert_eq!(
            wheel_report(true, p, false),
            vec![0x1b, b'[', b'M', 96, 42, 37]
        );
        let far = GridPoint::new(500, 500);
        assert_eq!(wheel_report(false, far, false)[4..], [255, 255]);
    }
}
