//! Assets compiled into the binary: icons (Lucide, ISC, plus the Arcode
//! logo), the UI font (Inter, OFL) and the default editor font (JetBrains
//! Mono, OFL). Nothing is ever fetched at runtime.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

use crate::Settings;

macro_rules! embedded {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/", $path)))),*]
    };
}

static FILES: &[(&str, &[u8])] = embedded![
    "icons/arrow-down-to-dot.svg",
    "icons/arrow-down.svg",
    "icons/arrow-right.svg",
    "icons/arrow-up-from-dot.svg",
    "icons/arrow-up.svg",
    "icons/box.svg",
    "icons/braces.svg",
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
    "icons/code-xml.svg",
    "icons/columns-2.svg",
    "icons/command.svg",
    "icons/copy.svg",
    "icons/ellipsis.svg",
    "icons/ember.svg",
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
    "icons/logo-arc.svg",
    "icons/minus.svg",
    "icons/monitor.svg",
    "icons/moon.svg",
    "icons/package.svg",
    "icons/palette.svg",
    "icons/panel-bottom.svg",
    "icons/panel-left.svg",
    "icons/pause.svg",
    "icons/play.svg",
    "icons/plus.svg",
    "icons/puzzle.svg",
    "icons/redo-dot.svg",
    "icons/refresh-cw.svg",
    "icons/regex.svg",
    "icons/replace-all.svg",
    "icons/replace.svg",
    "icons/rotate-ccw.svg",
    "icons/rows-2.svg",
    "icons/save.svg",
    "icons/search.svg",
    "icons/settings.svg",
    "icons/square-function.svg",
    "icons/square.svg",
    "icons/sun.svg",
    "icons/terminal.svg",
    "icons/trash-2.svg",
    "icons/triangle-alert.svg",
    "icons/type.svg",
    "icons/undo-2.svg",
    "icons/variable.svg",
    "icons/whole-word.svg",
    "icons/x.svg",
];

static MONO_FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../../assets/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
];

/// Inter 4.1 static TTFs: Regular (400), Medium (500), SemiBold (600).
static UI_FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/inter/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/inter/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/inter/Inter-SemiBold.ttf"),
];

/// Family name of the bundled monospace font.
pub const BUNDLED_MONO_FONT: &str = "JetBrains Mono";

/// Family name of the bundled UI font.
pub const BUNDLED_UI_FONT: &str = "Inter";

/// GPUI's name for the platform UI font, used when Inter cannot be loaded.
pub const SYSTEM_UI_FONT: &str = ".SystemUIFont";

/// Whether [`load_fonts`] registered Inter.
static UI_FONT_LOADED: AtomicBool = AtomicBool::new(false);

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

/// Registers the bundled fonts with GPUI's text system. Inter is added on
/// its own, so a failure there only falls back to the system UI font.
pub fn load_fonts(cx: &App) {
    let add = |fonts: &[&'static [u8]]| {
        let fonts = fonts.iter().map(|f| Cow::Borrowed(*f)).collect();
        cx.text_system().add_fonts(fonts)
    };
    if let Err(err) = add(MONO_FONTS) {
        log::error!("failed to load the bundled editor font: {err:#}");
    }
    match add(UI_FONTS) {
        Ok(()) => UI_FONT_LOADED.store(true, Ordering::Relaxed),
        Err(err) => log::error!("failed to load Inter, using the system UI font: {err:#}"),
    }
}

/// The UI font family for the workbench root: `ui.font_family` when set,
/// else Inter, or the system UI font when Inter could not be loaded.
pub fn ui_font_family(cx: &App) -> SharedString {
    let configured = cx
        .try_global::<Settings>()
        .and_then(|s| s.0.ui.font_family.clone());
    resolve_ui_font(configured, UI_FONT_LOADED.load(Ordering::Relaxed))
}

/// [`ui_font_family`] without the globals.
fn resolve_ui_font(configured: Option<String>, inter_loaded: bool) -> SharedString {
    match configured {
        Some(family) => family.into(),
        None if inter_loaded => BUNDLED_UI_FONT.into(),
        None => SYSTEM_UI_FONT.into(),
    }
}

/// The font family of readouts (Ln/Col, counts, durations, line numbers in
/// lists): always the bundled JetBrains Mono, so numbers never jitter.
pub fn mono_font_family() -> SharedString {
    BUNDLED_MONO_FONT.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_file_is_served() {
        for (path, bytes) in FILES {
            let loaded = Assets.load(path).unwrap().unwrap();
            assert_eq!(loaded.len(), bytes.len(), "{path}");
        }
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        let icons = Assets.list("icons/").unwrap();
        assert!(icons.iter().any(|p| p.as_ref() == "icons/ember.svg"));
    }

    #[test]
    fn fonts_are_static_truetype() {
        for font in MONO_FONTS.iter().chain(UI_FONTS) {
            assert_eq!(font[..4], [0, 1, 0, 0], "not a TrueType font");
        }
        assert_eq!(UI_FONTS.len(), 3);
    }

    #[test]
    fn ui_font_resolution() {
        assert_eq!(resolve_ui_font(None, true).as_ref(), "Inter");
        assert_eq!(resolve_ui_font(None, false).as_ref(), SYSTEM_UI_FONT);
        let own = resolve_ui_font(Some("Noto Sans".into()), true);
        assert_eq!(own.as_ref(), "Noto Sans");
        assert_eq!(mono_font_family().as_ref(), BUNDLED_MONO_FONT);
    }
}
