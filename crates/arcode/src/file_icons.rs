//! Icon and tint for a file name, used by the file tree, tabs and finder.

use arcode_ui_kit::{IconName, ThemeColors};
use gpui::Hsla;

/// The icon and its color for a file called `name`.
pub fn file_icon(name: &str, colors: &ThemeColors) -> (IconName, Hsla) {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cargo.toml" | "cargo.lock" | "package.json" | "package-lock.json" | "go.mod"
        | "go.sum" | "pyproject.toml" => return (IconName::Package, colors.accent_alt),
        ".gitignore" | ".gitattributes" | ".gitmodules" => {
            return (IconName::GitBranch, colors.text_muted);
        }
        _ => {}
    }
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "rs" => (IconName::FileText, colors.accent_alt),
        "md" | "markdown" | "txt" | "rst" => (IconName::FileText, colors.info),
        "toml" | "yaml" | "yml" | "json" | "ini" | "cfg" | "conf" => {
            (IconName::Settings, colors.text_muted)
        }
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => (IconName::FileText, colors.warning),
        "py" | "go" | "c" | "h" | "cpp" | "hpp" | "java" | "kt" | "rb" | "sh" | "zig" => {
            (IconName::FileText, colors.accent)
        }
        "html" | "css" | "scss" | "svg" => (IconName::FileText, colors.success),
        "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "ttf" | "otf" | "woff" | "woff2" => {
            (IconName::File, colors.text_disabled)
        }
        _ => (IconName::File, colors.text_muted),
    }
}

/// What a file is, for the tint of its icon in the file tree
/// (docs/DESIGN.md 6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// Rust and other source files.
    Source,
    /// Package manifests and lock files.
    Manifest,
    /// Documentation and plain text.
    Doc,
    /// Everything else (configuration, media, unknown).
    Other,
}

/// The kind of a file called `name`.
pub fn file_kind(name: &str) -> FileKind {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "go.mod"
            | "go.sum"
            | "pyproject.toml"
    ) {
        return FileKind::Manifest;
    }
    match lower.rsplit_once('.').map_or("", |(_, e)| e) {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "go" | "c" | "h" | "cpp"
        | "hpp" | "java" | "kt" | "rb" | "sh" | "zig" | "html" | "css" | "scss" => FileKind::Source,
        "md" | "markdown" | "txt" | "rst" => FileKind::Doc,
        _ => FileKind::Other,
    }
}

/// The icon of [`file_icon`] tinted by kind from the theme's syntax
/// colors: source files `type`, manifests `function.macro`, docs
/// `number`, everything else muted. Ties the tree to the editor's palette
/// instead of a rainbow of language colors.
pub fn file_icon_tinted(name: &str, theme: &arcode_ui_kit::UiTheme) -> (IconName, Hsla) {
    let c = &theme.colors;
    let (icon, _) = file_icon(name, c);
    let syntax = |capture: &str| theme.syntax(capture).color.unwrap_or(c.text_muted);
    let color = match file_kind(name) {
        FileKind::Source => syntax("type"),
        FileKind::Manifest => syntax("function.macro"),
        FileKind::Doc => syntax("number"),
        FileKind::Other => c.text_muted,
    };
    (icon, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_of_common_files() {
        assert_eq!(file_kind("main.rs"), FileKind::Source);
        assert_eq!(file_kind("App.TSX"), FileKind::Source);
        assert_eq!(file_kind("Cargo.toml"), FileKind::Manifest);
        assert_eq!(file_kind("Cargo.lock"), FileKind::Manifest);
        assert_eq!(file_kind("README.md"), FileKind::Doc);
        assert_eq!(file_kind("notes.txt"), FileKind::Doc);
        assert_eq!(file_kind("rustfmt.toml"), FileKind::Other);
        assert_eq!(file_kind(".gitignore"), FileKind::Other);
        assert_eq!(file_kind("Makefile"), FileKind::Other);
    }
}
