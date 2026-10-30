//! Unit tests for `keys`.

use super::*;

const CTRL_ALT: KeyMods = KeyMods {
    ctrl: true,
    alt: true,
    shift: false,
};
const CTRL_SHIFT: KeyMods = KeyMods {
    ctrl: true,
    alt: false,
    shift: true,
};
const ALL: KeyMods = KeyMods {
    ctrl: true,
    alt: true,
    shift: true,
};

fn enc(key: &str, mods: KeyMods) -> Option<Vec<u8>> {
    encode_key(key, mods, false)
}

fn s(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(bytes.to_vec())
}

#[test]
fn basic_named_keys() {
    let table: &[(&str, &[u8])] = &[
        ("enter", b"\r"),
        ("tab", b"\t"),
        ("backspace", b"\x7f"),
        ("escape", b"\x1b"),
        ("space", b" "),
        ("up", b"\x1b[A"),
        ("down", b"\x1b[B"),
        ("right", b"\x1b[C"),
        ("left", b"\x1b[D"),
        ("home", b"\x1b[H"),
        ("end", b"\x1b[F"),
        ("insert", b"\x1b[2~"),
        ("delete", b"\x1b[3~"),
        ("pageup", b"\x1b[5~"),
        ("pagedown", b"\x1b[6~"),
        ("f1", b"\x1bOP"),
        ("f2", b"\x1bOQ"),
        ("f3", b"\x1bOR"),
        ("f4", b"\x1bOS"),
        ("f5", b"\x1b[15~"),
        ("f6", b"\x1b[17~"),
        ("f7", b"\x1b[18~"),
        ("f8", b"\x1b[19~"),
        ("f9", b"\x1b[20~"),
        ("f10", b"\x1b[21~"),
        ("f11", b"\x1b[23~"),
        ("f12", b"\x1b[24~"),
    ];
    for (key, expected) in table {
        assert_eq!(enc(key, KeyMods::NONE), s(expected), "key {key}");
    }
}

#[test]
fn app_cursor_mode() {
    let table: &[(&str, &[u8])] = &[
        ("up", b"\x1bOA"),
        ("down", b"\x1bOB"),
        ("right", b"\x1bOC"),
        ("left", b"\x1bOD"),
        ("home", b"\x1bOH"),
        ("end", b"\x1bOF"),
        ("pageup", b"\x1b[5~"),
        ("enter", b"\r"),
    ];
    for (key, expected) in table {
        assert_eq!(
            encode_key(key, KeyMods::NONE, true),
            s(expected),
            "key {key}"
        );
    }
    // Modifiers override app cursor mode.
    assert_eq!(encode_key("up", KeyMods::CTRL, true), s(b"\x1b[1;5A"));
}

#[test]
fn modified_cursor_keys() {
    let table: &[(&str, KeyMods, &[u8])] = &[
        ("left", KeyMods::CTRL, b"\x1b[1;5D"),
        ("right", KeyMods::CTRL, b"\x1b[1;5C"),
        ("up", KeyMods::SHIFT, b"\x1b[1;2A"),
        ("down", KeyMods::ALT, b"\x1b[1;3B"),
        ("left", CTRL_SHIFT, b"\x1b[1;6D"),
        ("right", CTRL_ALT, b"\x1b[1;7C"),
        ("up", ALL, b"\x1b[1;8A"),
        ("home", KeyMods::SHIFT, b"\x1b[1;2H"),
        ("end", KeyMods::CTRL, b"\x1b[1;5F"),
    ];
    for (key, mods, expected) in table {
        assert_eq!(enc(key, *mods), s(expected), "key {key} {mods:?}");
    }
}

#[test]
fn modified_tilde_and_function_keys() {
    let table: &[(&str, KeyMods, &[u8])] = &[
        ("delete", KeyMods::CTRL, b"\x1b[3;5~"),
        ("pageup", KeyMods::SHIFT, b"\x1b[5;2~"),
        ("pagedown", KeyMods::ALT, b"\x1b[6;3~"),
        ("insert", KeyMods::SHIFT, b"\x1b[2;2~"),
        ("f1", KeyMods::SHIFT, b"\x1b[1;2P"),
        ("f4", KeyMods::CTRL, b"\x1b[1;5S"),
        ("f5", KeyMods::CTRL, b"\x1b[15;5~"),
        ("f12", KeyMods::SHIFT, b"\x1b[24;2~"),
    ];
    for (key, mods, expected) in table {
        assert_eq!(enc(key, *mods), s(expected), "key {key} {mods:?}");
    }
}

#[test]
fn special_modified_named_keys() {
    assert_eq!(enc("tab", KeyMods::SHIFT), s(b"\x1b[Z"));
    assert_eq!(enc("tab", KeyMods::ALT), s(b"\x1b\t"));
    assert_eq!(enc("enter", KeyMods::ALT), s(b"\x1b\r"));
    assert_eq!(enc("enter", KeyMods::SHIFT), s(b"\r"));
    assert_eq!(enc("backspace", KeyMods::CTRL), s(b"\x08"));
    assert_eq!(enc("backspace", KeyMods::ALT), s(b"\x1b\x7f"));
    assert_eq!(enc("escape", KeyMods::ALT), s(b"\x1b\x1b"));
    assert_eq!(enc("space", KeyMods::CTRL), s(b"\x00"));
    assert_eq!(enc("space", KeyMods::ALT), s(b"\x1b "));
    assert_eq!(enc("space", CTRL_ALT), s(b"\x1b\x00"));
}

#[test]
fn ctrl_letters() {
    assert_eq!(enc("c", KeyMods::CTRL), s(b"\x03"));
    assert_eq!(enc("a", KeyMods::CTRL), s(b"\x01"));
    assert_eq!(enc("z", KeyMods::CTRL), s(b"\x1a"));
    assert_eq!(enc("C", KeyMods::CTRL), s(b"\x03"));
    assert_eq!(enc("d", CTRL_SHIFT), s(b"\x04"));
    assert_eq!(enc("c", CTRL_ALT), s(b"\x1b\x03"));
    for (i, ch) in ('a'..='z').enumerate() {
        let expected = vec![i as u8 + 1];
        assert_eq!(enc(&ch.to_string(), KeyMods::CTRL), Some(expected));
    }
}

#[test]
fn ctrl_symbols() {
    let table: &[(&str, u8)] = &[
        ("[", 0x1b),
        ("\\", 0x1c),
        ("]", 0x1d),
        ("^", 0x1e),
        ("_", 0x1f),
        ("/", 0x1f),
        ("@", 0x00),
        ("2", 0x00),
        ("3", 0x1b),
        ("8", 0x7f),
        ("?", 0x7f),
    ];
    for (key, code) in table {
        assert_eq!(enc(key, KeyMods::CTRL), Some(vec![*code]), "ctrl+{key}");
    }
    // No control code: sent as typed.
    assert_eq!(enc("1", KeyMods::CTRL), s(b"1"));
    assert_eq!(enc(".", KeyMods::CTRL), s(b"."));
}

#[test]
fn printable_chars() {
    assert_eq!(enc("a", KeyMods::NONE), s(b"a"));
    assert_eq!(enc("A", KeyMods::NONE), s(b"A"));
    assert_eq!(enc("a", KeyMods::SHIFT), s(b"A"));
    assert_eq!(enc("1", KeyMods::NONE), s(b"1"));
    assert_eq!(enc("/", KeyMods::NONE), s(b"/"));
    assert_eq!(enc("é", KeyMods::NONE), Some("é".as_bytes().to_vec()));
    assert_eq!(enc("€", KeyMods::NONE), Some("€".as_bytes().to_vec()));
}

#[test]
fn alt_prefixes_escape() {
    assert_eq!(enc("b", KeyMods::ALT), s(b"\x1bb"));
    assert_eq!(
        enc(
            "b",
            KeyMods {
                alt: true,
                shift: true,
                ctrl: false
            }
        ),
        s(b"\x1bB")
    );
    assert_eq!(enc(".", KeyMods::ALT), s(b"\x1b."));
}

#[test]
fn unsendable_keys() {
    for key in [
        "shift", "control", "alt", "cmd", "platform", "function", "capslock", "",
    ] {
        assert_eq!(enc(key, KeyMods::NONE), None, "key {key:?}");
    }
    assert_eq!(enc("f13", KeyMods::NONE), None);
    assert_eq!(enc("ab", KeyMods::CTRL), None);
}

#[test]
fn xterm_param_values() {
    assert_eq!(KeyMods::NONE.xterm_param(), 1);
    assert_eq!(KeyMods::SHIFT.xterm_param(), 2);
    assert_eq!(KeyMods::ALT.xterm_param(), 3);
    assert_eq!(KeyMods::CTRL.xterm_param(), 5);
    assert_eq!(ALL.xterm_param(), 8);
    assert!(!KeyMods::NONE.any());
    assert!(KeyMods::CTRL.any());
}
