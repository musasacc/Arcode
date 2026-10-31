//! Contrast guard for Arcode Ember and Arcode Chalk (docs/DESIGN.md section
//! 3): text pairs stay at or above 4.5:1, UI elements at or above 3:1, with
//! the 5 % margin the design promises, so a palette edit cannot regress
//! readability unnoticed.

use super::*;

/// Text must reach 4.5:1 (WCAG AA for body text).
const TEXT: f32 = 4.5;
/// Non-text UI elements must reach 3:1.
const UI: f32 = 3.0;
/// No pair may sit within 5 % of its limit (antialiasing headroom).
const MARGIN: f32 = 1.05;

/// Surfaces text is drawn on, by `[ui]` key (`background` is the panel).
const PANEL: &str = "background";
const RAISED: &str = "surface_raised";

/// `(foreground, background, minimum)` pairs of section 3, by `[ui]` key.
const UI_PAIRS: &[(&str, &str, f32)] = &[
    ("text", "canvas", TEXT),
    ("text", PANEL, TEXT),
    ("text", RAISED, TEXT),
    ("text", "pill", TEXT),
    ("text", "hover", TEXT),
    ("text", "line_highlight", TEXT),
    ("text", "selection", TEXT),
    ("text", "paused_line", TEXT),
    ("text_muted", "canvas", TEXT),
    ("text_muted", PANEL, TEXT),
    ("text_muted", RAISED, TEXT),
    ("text_muted", "pill", TEXT),
    ("text_muted", "hover", TEXT),
    ("text_muted", "selection", TEXT),
    ("accent_text", "accent", TEXT),
    ("accent", PANEL, TEXT),
    ("accent", RAISED, TEXT),
    ("accent", "pill", TEXT),
    ("gutter_text", PANEL, TEXT),
    ("gutter_text", "line_highlight", TEXT),
    ("gutter_text_active", "line_highlight", TEXT),
    ("error", "line_highlight", TEXT),
    ("border_strong", PANEL, UI),
    ("border_strong", RAISED, UI),
    ("border_strong", "canvas", UI),
    ("border_focused", "canvas", UI),
    ("border_focused", PANEL, UI),
    ("accent", "canvas", UI),
    ("cursor", "line_highlight", UI),
    ("cursor", "selection", UI),
    ("accent_alt", PANEL, UI),
    ("accent_alt", "pill", UI),
    ("diff_added", PANEL, UI),
    ("diff_modified", PANEL, UI),
    ("diff_deleted", PANEL, UI),
];

/// Status colors, readable as text on the panel, the canvas (footer) and
/// raised surfaces (lens, toast).
const STATUS: [&str; 5] = ["error", "warning", "info", "success", "hint"];

/// Surfaces code is drawn on.
const CODE_SURFACES: [&str; 4] = [PANEL, "line_highlight", "selection", "paused_line"];

/// ANSI colors that are background colors by convention, per theme.
fn ansi_exempt(theme: &str, name: &str) -> bool {
    match theme {
        "Arcode Ember" => name == "black",
        _ => matches!(name, "white" | "bright_white"),
    }
}

fn check(failures: &mut Vec<String>, theme: &str, what: &str, fg: Color, bg: Color, min: f32) {
    let ratio = fg.contrast_ratio(bg);
    if ratio < min * MARGIN {
        failures.push(format!("{theme}: {what} is {ratio:.2}, needs {min} + 5 %"));
    }
}

#[test]
fn ember_and_chalk_keep_their_contrast() {
    let registry = ThemeRegistry::builtin();
    let mut failures = Vec::new();
    let mut checked = 0;
    for name in [DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME] {
        let theme = registry.get(name).unwrap();
        let ui = |key: &str| theme.ui.get(key).unwrap();
        let mut pairs: Vec<(&str, &str, f32)> = UI_PAIRS.to_vec();
        for status in STATUS {
            for surface in [PANEL, "canvas", RAISED] {
                pairs.push((status, surface, TEXT));
            }
        }
        pairs.extend([("error", PANEL, UI), ("warning", PANEL, UI)]);
        for (fg, bg, min) in pairs {
            check(
                &mut failures,
                name,
                &format!("{fg} on {bg}"),
                ui(fg),
                ui(bg),
                min,
            );
            checked += 1;
        }
        for (capture, style) in &theme.syntax {
            let color = style.color.unwrap_or(theme.ui.text);
            for surface in CODE_SURFACES {
                let what = format!("syntax {capture} on {surface}");
                check(&mut failures, name, &what, color, ui(surface), TEXT);
                checked += 1;
            }
        }
        let term = &theme.terminal;
        check(
            &mut failures,
            name,
            "terminal foreground",
            term.foreground,
            term.background,
            TEXT,
        );
        for (color, ansi) in term.ansi.iter().zip(ANSI_NAMES) {
            if !ansi_exempt(name, ansi) {
                let what = format!("terminal {ansi}");
                check(&mut failures, name, &what, *color, term.background, TEXT);
                checked += 1;
            }
        }
    }
    assert!(checked > 250, "only {checked} pairs checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn ember_is_never_status() {
    // The accent must stay apart from error and warning by luminance, so
    // the focus frame never reads as an error frame without color vision.
    let registry = ThemeRegistry::builtin();
    for name in [DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME] {
        let ui = &registry.get(name).unwrap().ui;
        for status in [ui.error, ui.warning] {
            let ratio = ui.accent.contrast_ratio(status);
            assert!(ratio >= 1.4, "{name}: accent vs status {ratio:.2}");
        }
    }
}
