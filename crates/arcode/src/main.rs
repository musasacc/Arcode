//! Arcode: a fast, native IDE.
//!
//! ```text
//! arcode [PATH]
//! ```
//!
//! PATH may be a folder (opened as the project) or a file (its project root
//! is opened and the file shown in an editor).

mod actions;
mod activity_bar;
mod app_state;
mod bottom_panel;
mod file_icons;
mod finder;
mod fuzzy;
mod pane;
mod project;
mod sidebar;
mod status_bar;
mod text_field;
mod theme_sync;
mod title_bar;
mod welcome;
mod workspace;

use std::path::PathBuf;
use std::time::Instant;

use arcode_config::Config;
use gpui::{
    App, Application, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, point,
    prelude::*, px, size,
};

use crate::status_bar::Startup;
use crate::workspace::Workspace;

/// What to open at startup, from the command line.
#[derive(Debug, Default, PartialEq)]
struct LaunchTarget {
    project: Option<PathBuf>,
    file: Option<PathBuf>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<LaunchTarget, String> {
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

fn window_options(cx: &App) -> WindowOptions {
    let saved = app_state::state(cx)
        .window
        .filter(|w| w.width >= 400. && w.height >= 300.);
    let window_bounds = match saved {
        Some(w) => {
            let bounds = Bounds::new(point(px(w.x), px(w.y)), size(px(w.width), px(w.height)));
            if w.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }
        }
        None => WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx)),
    };
    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("Arcode".into()),
            ..Default::default()
        }),
        app_id: Some("dev.arcode.Arcode".into()),
        window_min_size: Some(size(px(480.), px(320.))),
        ..Default::default()
    }
}

fn main() {
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

    Application::new()
        .with_assets(arcode_ui_kit::Assets)
        .run(move |cx: &mut App| {
            cx.set_global(Startup {
                started,
                ready: None,
            });
            arcode_ui_kit::load_fonts(cx);
            arcode_ui_kit::set_settings(config, cx);
            theme_sync::init(cx);
            theme_sync::apply(cx.window_appearance(), cx);
            app_state::init(cx);
            arcode_editor::init(cx);
            arcode_terminal_view::init(cx);
            actions::bind_keys(cx);
            text_field::bind_keys(cx);
            finder::bind_keys(cx);
            sidebar::file_tree::bind_keys(cx);

            cx.on_action(|_: &actions::Quit, cx| quit(cx));
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.on_app_quit(|cx| {
                app_state::save(cx);
                async {}
            })
            .detach();

            let options = window_options(cx);
            let opened = cx.open_window(options, |window, cx| {
                let ws = cx.new(|cx| Workspace::new(window, cx));
                ws.update(cx, |ws, cx| {
                    if let Some(project) = target.project.clone() {
                        ws.open_project(project, window, cx);
                    }
                    if let Some(file) = target.file.clone() {
                        ws.open_file(&file, window, cx);
                    }
                });
                if target.file.is_none() {
                    window.focus(&ws.focus_handle(cx));
                }
                ws
            });
            if let Err(err) = opened {
                log::error!("failed to open the main window: {err:#}");
                cx.quit();
            }
            cx.activate(true);
        });
}

/// Remembers every window's placement, then quits.
fn quit(cx: &mut App) {
    for handle in cx.windows() {
        handle
            .update(cx, |_, window, cx| app_state::record_window(window, cx))
            .ok();
    }
    app_state::save(cx);
    cx.quit();
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
