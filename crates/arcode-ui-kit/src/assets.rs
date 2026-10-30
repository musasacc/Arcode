//! Assets compiled into the binary: icons (Lucide, ISC) and the default
//! editor font (JetBrains Mono, OFL). Nothing is ever fetched at runtime.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

macro_rules! embedded {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/", $path)))),*]
    };
}

static FILES: &[(&str, &[u8])] = embedded![
    "icons/arrow-down.svg",
    "icons/arrow-right.svg",
    "icons/arrow-up.svg",
    "icons/bug.svg",
    "icons/case-sensitive.svg",
    "icons/check.svg",
    "icons/chevron-down.svg",
    "icons/chevron-right.svg",
    "icons/chevrons-down-up.svg",
    "icons/circle-alert.svg",
    "icons/circle-check.svg",
    "icons/circle-dot.svg",
    "icons/circle-play.svg",
    "icons/circle-x.svg",
    "icons/columns-2.svg",
    "icons/command.svg",
    "icons/copy.svg",
    "icons/ellipsis.svg",
    "icons/external-link.svg",
    "icons/file-text.svg",
    "icons/file.svg",
    "icons/files.svg",
    "icons/folder-open.svg",
    "icons/folder.svg",
    "icons/git-branch.svg",
    "icons/git-commit-horizontal.svg",
    "icons/git-compare.svg",
    "icons/git-merge.svg",
    "icons/history.svg",
    "icons/info.svg",
    "icons/keyboard.svg",
    "icons/lightbulb.svg",
    "icons/list-todo.svg",
    "icons/loader.svg",
    "icons/minus.svg",
    "icons/monitor.svg",
    "icons/moon.svg",
    "icons/package.svg",
    "icons/palette.svg",
    "icons/panel-bottom.svg",
    "icons/panel-left.svg",
    "icons/play.svg",
    "icons/plus.svg",
    "icons/puzzle.svg",
    "icons/refresh-cw.svg",
    "icons/regex.svg",
    "icons/replace-all.svg",
    "icons/replace.svg",
    "icons/rows-2.svg",
    "icons/save.svg",
    "icons/search.svg",
    "icons/settings.svg",
    "icons/square.svg",
    "icons/sun.svg",
    "icons/terminal.svg",
    "icons/trash-2.svg",
    "icons/triangle-alert.svg",
    "icons/type.svg",
    "icons/undo-2.svg",
    "icons/whole-word.svg",
    "icons/x.svg",
];

static FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
];

/// Family name of the bundled monospace font.
pub const BUNDLED_MONO_FONT: &str = "JetBrains Mono";

/// The application's [`AssetSource`]: serves embedded files by relative path.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

/// Registers the bundled fonts with GPUI's text system.
pub fn load_fonts(cx: &App) {
    let fonts = FONTS.iter().map(|f| Cow::Borrowed(*f)).collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::error!("failed to load bundled fonts: {err:#}");
    }
}
