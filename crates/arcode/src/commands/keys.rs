//! Human-readable key binding labels (`ctrl-shift-p` → `Ctrl+Shift+P`).

use gpui::KeyBinding;

/// Modifier prefixes as GPUI writes them, with their display names.
fn modifier_name(prefix: &str) -> Option<&'static str> {
    Some(match prefix {
        "ctrl" => "Ctrl",
        "alt" if cfg!(target_os = "macos") => "Option",
        "alt" => "Alt",
        "shift" => "Shift",
        "cmd" => "Cmd",
        "super" => "Super",
        "win" => "Win",
        "fn" => "Fn",
        _ => return None,
    })
}

/// Display name of a key (`f12` → `F12`, `left` → `Left`, `\` stays).
fn key_name(key: &str) -> String {
    match key {
        "" => String::new(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "escape" => "Esc".into(),
        "backspace" => "Backspace".into(),
        key => {
            let mut chars = key.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
            first.unwrap_or_default() + chars.as_str()
        }
    }
}

/// Formats one keystroke as GPUI unparses it (`ctrl-shift-p`, `ctrl--`).
pub fn keystroke_label(unparsed: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = unparsed;
    while let Some((prefix, tail)) = rest.split_once('-') {
        let Some(name) = modifier_name(prefix).filter(|_| !tail.is_empty()) else {
            break;
        };
        parts.push(name.to_owned());
        rest = tail;
    }
    parts.push(key_name(rest));
    parts.join("+")
}

/// Formats a binding: its keystrokes separated by spaces (chords).
pub fn binding_label(binding: &KeyBinding) -> String {
    binding
        .keystrokes()
        .iter()
        .map(|k| keystroke_label(&k.unparse()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_modifiers_and_keys() {
        assert_eq!(keystroke_label("ctrl-shift-p"), "Ctrl+Shift+P");
        assert_eq!(keystroke_label("f12"), "F12");
        assert_eq!(keystroke_label("ctrl--"), "Ctrl+-");
        assert_eq!(keystroke_label("-"), "-");
        assert_eq!(keystroke_label("cmd-\\"), "Cmd+\\");
        assert_eq!(keystroke_label("shift-escape"), "Shift+Esc");
        assert!(keystroke_label("alt-]").ends_with("+]"));
    }

    #[test]
    fn chords_are_joined_with_spaces() {
        let binding = KeyBinding::new("ctrl-k ctrl-i", crate::actions::ToggleFinder, None);
        assert_eq!(binding_label(&binding), "Ctrl+K Ctrl+I");
    }
}
