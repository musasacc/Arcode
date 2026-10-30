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
