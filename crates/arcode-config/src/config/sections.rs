//! The `[editor]`, `[ui]` and `[terminal]` sections of `config.toml`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::completion::CompletionSettings;
use super::inline_diagnostics::InlineDiagnostics;
use crate::lenient::impl_section;

/// The bundled editor font.
pub const DEFAULT_EDITOR_FONT: &str = "JetBrains Mono";

/// Keybinding scheme for the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keymap {
    /// Conventional (VS Code-like) bindings.
    #[default]
    Default,
    /// Modal Vim emulation.
    Vim,
}

/// `[editor]`: text editing and rendering of code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Primary code font family.
    pub font_family: String,
    /// Fonts tried, in order, for glyphs missing from `font_family`.
    pub font_fallbacks: Vec<String>,
    /// Font size in points.
    pub font_size: f32,
    /// Line height as a multiple of the font size.
    pub line_height: f32,
    /// Enable programming ligatures.
    pub ligatures: bool,
    /// Width of a tab stop in columns.
    pub tab_size: usize,
    /// Insert spaces instead of tab characters.
    pub insert_spaces: bool,
    /// Guess tabs or spaces and the indent size of each opened file from
    /// its content (overriding `tab_size` / `insert_spaces` for that file).
    pub detect_indentation: bool,
    /// Show line numbers in the gutter.
    pub show_line_numbers: bool,
    /// Highlight the line containing the cursor.
    pub highlight_current_line: bool,
    /// Blink the cursor.
    pub cursor_blink: bool,
    /// Soft-wrap long lines at the viewport edge.
    pub word_wrap: bool,
    /// Keybinding scheme.
    pub keymap: Keymap,
    /// Modal Vim editing (also on when `keymap` is `"vim"`). "Editor:
    /// Toggle Vim Mode" switches it for the session.
    pub vim_mode: bool,
    /// Insert the closing bracket/quote when typing an opening one.
    pub auto_close_brackets: bool,
    /// Format the file (via LSP or the language formatter) when saving.
    pub format_on_save: bool,
    /// Longest wait for the formatter before saving unformatted.
    pub format_on_save_timeout_ms: u64,
    /// Files at least this many bytes are "large": no full syntax tree is
    /// kept; only the visible region is parsed for highlighting.
    pub large_file_bytes: u64,
    /// Files with at least this many lines are "large" too.
    pub large_file_lines: usize,
    /// Show documentation (and diagnostics) when the mouse rests on a
    /// symbol.
    pub hover: bool,
    /// Milliseconds the mouse must rest on a symbol before its hover shows.
    pub hover_delay_ms: u64,
    /// Highlight the other occurrences of the symbol under the cursor.
    pub occurrence_highlight: bool,
    /// Show inlay hints (types, parameter names) from language servers.
    pub inlay_hints: bool,
    /// Show a light bulb in the gutter when the cursor line has code
    /// actions.
    pub code_action_lightbulb: bool,
    /// Which diagnostics get an inline lens after their line: `"errors"`,
    /// `"all"` or `"off"`.
    pub inline_diagnostics: InlineDiagnostics,
    /// `[editor.completion]`: completion popup, snippets, signature help.
    pub completion: CompletionSettings,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_EDITOR_FONT.to_owned(),
            font_fallbacks: [
                "Noto Sans CJK SC",
                "Noto Color Emoji",
                "Segoe UI Emoji",
                "Apple Color Emoji",
            ]
            .map(String::from)
            .to_vec(),
            font_size: 14.0,
            line_height: 1.5,
            ligatures: true,
            tab_size: 4,
            insert_spaces: true,
            detect_indentation: true,
            show_line_numbers: true,
            highlight_current_line: true,
            cursor_blink: false,
            word_wrap: false,
            keymap: Keymap::Default,
            vim_mode: false,
            auto_close_brackets: true,
            format_on_save: false,
            format_on_save_timeout_ms: 1500,
            large_file_bytes: 4 * 1024 * 1024,
            large_file_lines: 50_000,
            hover: true,
            hover_delay_ms: 300,
            occurrence_highlight: true,
            inlay_hints: true,
            code_action_lightbulb: true,
            inline_diagnostics: InlineDiagnostics::Errors,
            completion: CompletionSettings::default(),
        }
    }
}

impl_section!(EditorSettings {
    font_family,
    font_fallbacks,
    font_size,
    line_height,
    ligatures,
    tab_size,
    insert_spaces,
    detect_indentation,
    show_line_numbers,
    highlight_current_line,
    cursor_blink,
    word_wrap,
    keymap,
    vim_mode,
    auto_close_brackets,
    format_on_save,
    format_on_save_timeout_ms,
    large_file_bytes,
    large_file_lines,
    hover,
    hover_delay_ms,
    occurrence_highlight,
    inlay_hints,
    code_action_lightbulb,
    inline_diagnostics,
});

impl EditorSettings {
    /// The primary family followed by the fallbacks, without duplicates.
    pub fn font_stack(&self) -> Vec<&str> {
        let mut stack: Vec<&str> = vec![self.font_family.as_str()];
        for font in &self.font_fallbacks {
            if !stack.contains(&font.as_str()) {
                stack.push(font);
            }
        }
        stack
    }

    /// Resets out-of-range values to their defaults, recording warnings.
    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        let defaults = Self::default();
        if self.font_family.trim().is_empty() {
            warnings.push("`editor.font_family` is empty; using default".into());
            self.font_family = defaults.font_family;
        }
        check_range(
            &mut self.font_size,
            4.0,
            200.0,
            14.0,
            "editor.font_size",
            warnings,
        );
        check_range(
            &mut self.line_height,
            0.5,
            4.0,
            1.5,
            "editor.line_height",
            warnings,
        );
        if !(1..=32).contains(&self.tab_size) {
            warnings.push(format!(
                "`editor.tab_size` must be between 1 and 32, got {}; using default",
                self.tab_size
            ));
            self.tab_size = defaults.tab_size;
        }
        if self.hover_delay_ms > Self::MAX_HOVER_DELAY_MS {
            warnings.push(format!(
                "`editor.hover_delay_ms` capped at {}",
                Self::MAX_HOVER_DELAY_MS
            ));
            self.hover_delay_ms = Self::MAX_HOVER_DELAY_MS;
        }
        if self.format_on_save_timeout_ms > Self::MAX_FORMAT_TIMEOUT_MS {
            warnings.push(format!(
                "`editor.format_on_save_timeout_ms` capped at {}",
                Self::MAX_FORMAT_TIMEOUT_MS
            ));
            self.format_on_save_timeout_ms = Self::MAX_FORMAT_TIMEOUT_MS;
        }
        self.completion.validate(warnings);
    }

    /// Longest accepted hover delay.
    pub const MAX_HOVER_DELAY_MS: u64 = 5000;
    /// Longest accepted format-on-save wait.
    pub const MAX_FORMAT_TIMEOUT_MS: u64 = 10_000;
}

/// Which theme to use. Written in TOML as a string: `"system"`, `"dark"`,
/// `"light"` or the name of a theme.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow the OS appearance, using `ui.dark_theme` / `ui.light_theme`.
    #[default]
    System,
    /// Always use `ui.dark_theme`.
    Dark,
    /// Always use `ui.light_theme`.
    Light,
    /// A specific theme by name.
    Named(String),
}

impl ThemeChoice {
    /// Parses the TOML string form (keywords are case-insensitive; an empty
    /// string means `System`).
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "" | "system" => Self::System,
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::Named(trimmed.to_owned()),
        }
    }

    /// The TOML string form.
    pub fn as_str(&self) -> &str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Named(name) => name,
        }
    }
}

impl fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ThemeChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThemeChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw))
    }
}

/// `[ui]`: the workbench around the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// UI font family; `None` uses the bundled Inter (the system UI font
    /// if Inter cannot be loaded), `".SystemUIFont"` the platform UI font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// UI font size in points.
    pub font_size: f32,
    /// Theme selection.
    pub theme: ThemeChoice,
    /// Theme used for `theme = "dark"` and dark system appearance.
    pub dark_theme: String,
    /// Theme used for `theme = "light"` and light system appearance.
    pub light_theme: String,
    /// Reduce visual effects for slow machines (implies no animations).
    pub low_end_mode: bool,
    /// Enable UI animations.
    pub animations: bool,
    /// Duration of UI animations in milliseconds.
    pub animation_ms: u32,
    /// Show frame-time / memory statistics in the status bar.
    pub show_perf_stats: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 13.0,
            theme: ThemeChoice::System,
            dark_theme: crate::theme::DEFAULT_DARK_THEME.to_owned(),
            light_theme: crate::theme::DEFAULT_LIGHT_THEME.to_owned(),
            low_end_mode: false,
            animations: true,
            animation_ms: 120,
            show_perf_stats: true,
        }
    }
}

impl_section!(UiSettings {
    font_family,
    font_size,
    theme,
    dark_theme,
    light_theme,
    low_end_mode,
    animations,
    animation_ms,
    show_perf_stats,
});

impl UiSettings {
    /// Effective animation duration: 0 when animations are off or in
    /// low-end mode.
    pub fn effective_animation_ms(&self) -> u32 {
        if self.animations && !self.low_end_mode {
            self.animation_ms
        } else {
            0
        }
    }

    /// A debounce delay for background refreshes (diagnostics, git):
    /// `base_ms`, or three times it in low-end mode.
    pub fn debounce_ms(&self, base_ms: u64) -> u64 {
        if self.low_end_mode {
            base_ms.saturating_mul(3)
        } else {
            base_ms
        }
    }

    pub(crate) fn validate(&mut self, warnings: &mut Vec<String>) {
        let defaults = Self::default();
        check_range(
            &mut self.font_size,
            4.0,
            200.0,
            13.0,
            "ui.font_size",
            warnings,
        );
        if self.animation_ms > 5000 {
            warnings.push(format!(
                "`ui.animation_ms` must be at most 5000, got {}; using default",
                self.animation_ms
            ));
            self.animation_ms = defaults.animation_ms;
        }
        if self
            .font_family
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            self.font_family = None;
        }
        for (name, slot, default) in [
            ("ui.dark_theme", &mut self.dark_theme, defaults.dark_theme),
            (
                "ui.light_theme",
                &mut self.light_theme,
                defaults.light_theme,
            ),
        ] {
            if slot.trim().is_empty() {
                warnings.push(format!("`{name}` is empty; using default"));
                *slot = default;
            }
        }
    }
}

fn check_range(
    slot: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    name: &str,
    warnings: &mut Vec<String>,
) {
    if !(slot.is_finite() && (min..=max).contains(slot)) {
        warnings.push(format!(
            "`{name}` must be between {min} and {max}, got {slot}; using default"
        ));
        *slot = default;
    }
}
