//! Review tests (Settings unit): config write ordering and preset edits.
//! Each test states the expected behaviour; failing ones reproduce a bug.

use arcode_config::{Config, FirstRunChoices, SettingEdit, apply_edits, low_end_edits};
use gpui::TestAppContext;

use super::store::{ConfigPathOverride, write_settings};

/// Two quick writes of the same key (two stepper clicks): the file and the
/// active settings must both end with the second value. The background
/// jobs take the lock in whatever order the executor runs them, so the
/// first write can land last in the file while the UI shows the second.
#[gpui::test(iterations = 40)]
async fn review_last_write_wins_in_file_and_settings(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[editor]\nfont_size = 14.0\n").unwrap();
    crate::test_support::init(cx);
    cx.update(|cx| cx.set_global(ConfigPathOverride(path.clone())));
    let first =
        cx.update(|cx| write_settings(vec![SettingEdit::set("editor.font_size", 15.0f32)], cx));
    let second =
        cx.update(|cx| write_settings(vec![SettingEdit::set("editor.font_size", 16.0f32)], cx));
    first.await.unwrap();
    second.await.unwrap();
    cx.run_until_parked();
    let (on_disk, _) = Config::load_from(&path);
    let shown = cx.update(|cx| arcode_ui_kit::settings(cx).editor.font_size);
    assert_eq!(
        on_disk.editor.font_size, 16.0,
        "the last click must win on disk"
    );
    assert_eq!(shown, on_disk.editor.font_size, "the UI must show the file");
}

/// "Show Welcome Setup" on a low-end config: the dialog shows "Normal",
/// the user saves, low-end mode must be off afterwards.
#[test]
fn review_first_run_normal_turns_low_end_off() {
    let source = "[ui]\nlow_end_mode = true\n";
    let choices = FirstRunChoices {
        low_end: false,
        ..FirstRunChoices::default()
    };
    let out = apply_edits(source, &choices.edits()).unwrap();
    let (config, _) = Config::from_toml_str(&out);
    assert!(
        !config.ui.low_end_mode,
        "the dialog said Normal, but low-end mode stays on:\n{out}"
    );
}

/// Turning low-end mode on and off again must not turn on inlay hints the
/// user had switched off before.
#[test]
fn review_low_end_round_trip_keeps_user_choice() {
    let source = "[editor]\ninlay_hints = false\n[git]\ninline_blame = false\n";
    let on = apply_edits(source, &low_end_edits(true)).unwrap();
    let off = apply_edits(&on, &low_end_edits(false)).unwrap();
    let (config, _) = Config::from_toml_str(&off);
    assert!(
        !config.editor.inlay_hints && !config.git.inline_blame,
        "toggling low-end mode off re-enabled hints/blame the user had off:\n{off}"
    );
}

/// A `config.toml` that is a symlink into a dotfiles repository: changing
/// a setting from the Settings page must update the linked file, not
/// replace the link with a regular file (the atomic write renames a temp
/// file over the link).
#[cfg(unix)]
#[gpui::test]
async fn review_symlinked_config_stays_a_link(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let dotfiles = dir.path().join("dotfiles");
    std::fs::create_dir_all(&dotfiles).unwrap();
    let real = dotfiles.join("arcode.toml");
    std::fs::write(&real, "[editor]\nfont_size = 14.0\n").unwrap();
    let link = dir.path().join("config.toml");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    crate::test_support::init(cx);
    cx.update(|cx| cx.set_global(ConfigPathOverride(link.clone())));
    let task =
        cx.update(|cx| write_settings(vec![SettingEdit::set("editor.font_size", 15.0f32)], cx));
    task.await.unwrap();
    cx.run_until_parked();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "config.toml was replaced by a regular file"
    );
    let text = std::fs::read_to_string(&real).unwrap();
    assert!(
        text.contains("font_size = 15.0"),
        "the dotfile was not updated: {text}"
    );
}
