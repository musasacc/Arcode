//! Icon and tint for a file name, used by the file tree, tabs and finder.

use arcode_ui_kit::{IconName, ThemeColors};
use gpui::Hsla;

/// The icon for a file called `name` and its tint by kind (docs/DESIGN.md
/// 6.5): source files `syntax.type`, manifests `syntax.macro`, docs
/// `syntax.number`, everything else muted. The same in the file tree, the
/// tabs and every list, and never ember or a status colour (2.4).
pub fn file_icon(name: &str, colors: &ThemeColors) -> (IconName, Hsla) {
    let tint = match file_kind(name) {
        FileKind::Source => colors.file_source,
        FileKind::Manifest => colors.file_manifest,
        FileKind::Doc => colors.file_doc,
        FileKind::Other => colors.text_muted,
    };
    (file_glyph(name), tint)
}

/// The icon for a file called `name`.
fn file_glyph(name: &str) -> IconName {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cargo.toml" | "cargo.lock" | "package.json" | "package-lock.json" | "go.mod"
        | "go.sum" | "pyproject.toml" => return IconName::Package,
        ".gitignore" | ".gitattributes" | ".gitmodules" => return IconName::GitBranch,
        _ => {}
    }
    match lower.rsplit_once('.').map_or("", |(_, e)| e) {
        "toml" | "yaml" | "yml" | "json" | "ini" | "cfg" | "conf" => IconName::Settings,
        "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "ttf" | "otf" | "woff" | "woff2" => {
            IconName::File
        }
        _ if file_kind(name) == FileKind::Other => IconName::File,
        _ => IconName::FileText,
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

/// The icon of a file in the file tree: [`file_icon`] (kept for the tree,
/// which has the whole theme at hand).
pub fn file_icon_tinted(name: &str, theme: &arcode_ui_kit::UiTheme) -> (IconName, Hsla) {
    file_icon(name, &theme.colors)
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
