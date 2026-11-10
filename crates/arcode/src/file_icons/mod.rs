//! The icon of a file wherever one is shown: the file tree, tabs and the
//! tab list, the breadcrumb, quick open and the other file pickers,
//! project search, Problems, TODO and the Source Control list. Every one of
//! them goes through [`file_icon`].
//!
//! With `ui.file_icons = "logos"` (the default) a file whose language has a
//! logo shows it ([`table`]); every other file, and every file with
//! `"plain"`, shows a plain icon tinted by kind (docs/DESIGN.md 6.5).

pub mod table;

use std::path::Path;

use arcode_config::{Config, FileIcons};
use arcode_ui_kit::{FileGlyph, FileIcon, IconName, ThemeColors, file_glyph, settings, theme};
use gpui::{App, Hsla};

pub use table::{logo_for, logo_for_language};

/// The icon for the file at `path` (a bare file name works too) under the
/// current settings and theme. Callers set its size, and may override the
/// tint (`accent` for the selected row); colour logos keep their colours.
pub fn file_icon(path: impl AsRef<Path>, cx: &App) -> FileIcon {
    let (glyph, tint) = file_icon_parts(path.as_ref(), settings(cx), &theme(cx).colors);
    file_glyph(glyph).color(tint)
}

/// What [`file_icon`] shows and its tint: the language logo (tinted logos
/// in `text`) or the plain icon tinted by kind.
pub fn file_icon_parts(path: &Path, config: &Config, colors: &ThemeColors) -> (FileGlyph, Hsla) {
    if config.ui.file_icons == FileIcons::Logos
        && let Some(logo) = logo_for(path, config)
    {
        return (FileGlyph::Logo(logo), colors.text);
    }
    let name = path.file_name().map(|n| n.to_string_lossy());
    let (glyph, tint) = plain_icon(name.as_deref().unwrap_or(""), colors);
    (FileGlyph::Icon(glyph), tint)
}

/// The plain icon for a file called `name` and its tint by kind: source
/// files `syntax.type`, manifests `syntax.macro`, docs `syntax.number`,
/// everything else muted. Never ember or a status colour (2.4).
pub fn plain_icon(name: &str, colors: &ThemeColors) -> (IconName, Hsla) {
    let tint = match file_kind(name) {
        FileKind::Source => colors.file_source,
        FileKind::Manifest => colors.file_manifest,
        FileKind::Doc => colors.file_doc,
        FileKind::Other => colors.text_muted,
    };
    (plain_glyph(name), tint)
}

/// The plain icon for a file called `name`.
fn plain_glyph(name: &str) -> IconName {
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

/// What a file is, for the tint of its plain icon (docs/DESIGN.md 6.5).
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

#[cfg(test)]
mod tests;
