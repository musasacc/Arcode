//! Opens a window with one integrated terminal.
//!
//! Usage: `cargo run -p arcode-terminal-view --example terminal [CWD]`
//! The window closes when the shell exits.

use std::{path::PathBuf, sync::Arc};

use arcode_config::{Appearance, Config, ThemeRegistry};
use arcode_terminal_view::{TerminalView, TerminalViewEvent};
use arcode_ui_kit::{Assets, load_fonts, set_settings, set_theme};
use gpui::{App, Application, Bounds, Focusable, WindowBounds, WindowOptions, px, size};

fn main() {
    env_logger::init();
    let cwd = std::env::args().nth(1).map(PathBuf::from);
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            load_fonts(cx);
            set_settings(Config::default(), cx);
            let theme = ThemeRegistry::builtin().fallback(Appearance::Dark).clone();
            set_theme(Arc::new(theme), cx);
            arcode_terminal_view::init(cx);

            let bounds = Bounds::centered(None, size(px(900.), px(560.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };
            let opened = cx.open_window(options, |window, cx| {
                let view = match TerminalView::spawn(cwd, window, cx) {
                    Ok(view) => view,
                    Err(err) => {
                        eprintln!("failed to start the terminal: {err:#}");
                        std::process::exit(1);
                    }
                };
                window.focus(&view.focus_handle(cx));
                window.set_window_title(&view.read(cx).title());
                let handle = window.window_handle();
                cx.subscribe(&view, move |view, event, cx| match event {
                    TerminalViewEvent::Exited => cx.quit(),
                    TerminalViewEvent::TitleChanged => {
                        let title = view.read(cx).title();
                        handle
                            .update(cx, |_, window, _| window.set_window_title(&title))
                            .ok();
                    }
                    TerminalViewEvent::Bell => log::info!("bell"),
                })
                .detach();
                view
            });
            if let Err(err) = opened {
                eprintln!("failed to open a window: {err:#}");
                cx.quit();
            }
        });
}
