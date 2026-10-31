//! Application startup: command line, globals, key bindings and the main
//! window.

use std::path::PathBuf;
use std::time::Instant;

use arcode_config::Config;
use gpui::{App, Application};

use crate::status_bar::Startup;
use crate::{
    actions, app_state, commands, find_bar, finder, new_project, path_prompt, project, prompt,
    search_view, sidebar, text_field, theme_sync, windows,
};

/// What to open at startup, from the command line.
#[derive(Debug, Default, PartialEq)]
pub struct LaunchTarget {
    /// Folder to open as the project.
    pub project: Option<PathBuf>,
    /// File to open in an editor.
    pub file: Option<PathBuf>,
}

/// Parses `arcode [PATH]`.
pub fn parse_args(args: impl Iterator<Item = String>) -> Result<LaunchTarget, String> {
    let mut target = LaunchTarget::default();
    for arg in args {
        if arg == "-h" || arg == "--help" {
            return Err("usage: arcode [PATH]".to_string());
        }
        if target.project.is_some() {
            return Err(format!("unexpected argument `{arg}`; usage: arcode [PATH]"));
        }
        let path = PathBuf::from(&arg);
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if path.is_dir() {
            target.project = Some(path);
        } else if path.is_file() {
            target.project = Some(project::root_for_file(&path));
            target.file = Some(path);
        } else {
            return Err(format!("{arg}: no such file or folder"));
        }
    }
    Ok(target)
}

/// Runs Arcode with the process arguments; returns when the app quits.
pub fn run() {
    let started = Instant::now();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let target = match parse_args(std::env::args().skip(1)) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let (config, warnings) = Config::load();
    for warning in &warnings {
        log::warn!("config: {warning}");
    }
    // Without a configuration folder there is nothing the first-run dialog
    // could write, so it is not shown.
    let config_exists =
        arcode_config::paths::config_file().is_none_or(|path| path.try_exists().unwrap_or(true));
    let first_run = crate::prefs::first_run::wanted(
        config_exists,
        std::env::var("ARCODE_SKIP_FIRST_RUN").ok().as_deref(),
    );
    crate::prefs::frames::init_logging();

    Application::new()
        .with_assets(arcode_ui_kit::Assets)
        .run(move |cx: &mut App| {
            cx.set_global(Startup {
                started,
                ready: None,
            });
            if first_run {
                cx.set_global(crate::prefs::first_run::FirstRunPending);
            }
            arcode_ui_kit::load_fonts(cx);
            arcode_ui_kit::set_settings(config, cx);
            theme_sync::init(cx);
            theme_sync::apply(cx.window_appearance(), cx);
            app_state::init(cx);
            arcode_editor::init(cx);
            crate::vim_glue::init(cx);
            arcode_terminal_view::init(cx);
            actions::bind_keys(cx);
            prompt::init(cx);
            text_field::bind_keys(cx);
            text_field::bind_multiline_keys(cx);
            finder::bind_keys(cx);
            find_bar::bind_keys(cx);
            search_view::bind_keys(cx);
            commands::init(cx);
            crate::lsp_bridge::init(cx);
            path_prompt::bind_keys(cx);
            sidebar::file_tree::bind_keys(cx);

            new_project::bind_keys(cx);

            cx.on_action(|_: &actions::Quit, cx| windows::quit(cx));
            cx.on_action(|_: &actions::NewWindow, cx| {
                cx.defer(|cx| {
                    windows::open_window(None, None, cx);
                })
            });
            cx.on_window_closed(|cx| {
                if windows::workspace_windows(cx).is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.on_app_quit(|cx| {
                // Quitting through the app records sessions first; this
                // covers a quit requested by the OS.
                if !windows::workspace_windows(cx).is_empty() {
                    windows::record_all(cx);
                }
                app_state::save(cx);
                async {}
            })
            .detach();

            let opened = open_startup_windows(target, cx);
            if opened == 0 {
                log::error!("failed to open the main window");
                cx.quit();
            }
            cx.activate(true);
        });
}

/// Re-reads the configuration file and applies it (theme, fonts, …) to
/// every window. The file is read on the background executor, after the
/// settings writes queued before (see [`crate::prefs::store`]).
pub fn reload_settings(cx: &mut App) {
    crate::prefs::store::reload(cx);
}

/// Makes a freshly loaded configuration active (theme, fonts, …) in every
/// window. `warnings` are the loader's.
pub fn apply_settings(mut config: Config, warnings: &[String], cx: &mut App) {
    for warning in warnings {
        log::warn!("config: {warning}");
    }
    if warnings.iter().any(|w| Config::is_syntax_warning(w)) {
        // A typo mid-edit must not restart every language server with the
        // built-in list; keep the running LSP setup until the file parses.
        let current = arcode_ui_kit::settings(cx);
        config.languages = current.languages.clone();
        config.lsp = current.lsp.clone();
    }
    arcode_ui_kit::set_settings(config, cx);
    theme_sync::apply(cx.window_appearance(), cx);
    log::info!("settings reloaded");
}

/// Opens the windows Arcode starts with: the command-line target, else the
/// projects open at the last quit (when `workspace.reopen_projects` is on),
/// else an empty window with the welcome screen. Returns how many opened.
fn open_startup_windows(target: LaunchTarget, cx: &mut App) -> usize {
    // Every project opened from here on records itself again, so after a
    // crash only the projects open in this run are reopened.
    let last_open = app_state::open_projects(cx);
    app_state::set_open_projects(Vec::new(), cx);
    if target.project.is_some() || target.file.is_some() {
        return usize::from(windows::open_window(target.project, target.file, cx).is_some());
    }
    let reopen = arcode_ui_kit::settings(cx).workspace.reopen_projects;
    let projects: Vec<_> = if reopen {
        last_open.into_iter().filter(|p| p.is_dir()).collect()
    } else {
        Vec::new()
    };
    let mut opened = 0;
    for project in projects {
        opened += usize::from(windows::open_window(Some(project), None, cx).is_some());
    }
    if opened == 0 {
        opened += usize::from(windows::open_window(None, None, cx).is_some());
    }
    opened
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_folder_file_and_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(dir.path()).expect("canonical");
        std::fs::write(root.join("Cargo.toml"), "").expect("write");
        std::fs::create_dir(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), "").expect("write");

        let none = parse_args(std::iter::empty()).expect("ok");
        assert_eq!(none, LaunchTarget::default());

        let folder = parse_args([root.display().to_string()].into_iter()).expect("ok");
        assert_eq!(folder.project.as_deref(), Some(root.as_path()));
        assert_eq!(folder.file, None);

        let file = root.join("src/main.rs");
        let target = parse_args([file.display().to_string()].into_iter()).expect("ok");
        assert_eq!(target.project.as_deref(), Some(root.as_path()));
        assert_eq!(target.file.as_deref(), Some(file.as_path()));

        assert!(parse_args(["/definitely/missing".to_string()].into_iter()).is_err());
        let two = [root.display().to_string(), root.display().to_string()];
        assert!(parse_args(two.into_iter()).is_err());
    }
}
