//! Several windows, one project each: opening windows (with the saved
//! placement), finding the window of a project, quitting with every
//! window's session saved, and the bookkeeping of which projects to
//! reopen at the next start.
//!
//! Each window has its own [`Workspace`], so its own language servers,
//! terminals, git store and session.

use std::path::{Path, PathBuf};

use arcode_config::WindowState;
use gpui::{
    App, AppContext, Bounds, Focusable, PromptLevel, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, point, px, size,
};

use crate::app_state;
use crate::workspace::Workspace;
use crate::workspace::close_guard;

mod x11;

/// Offset of a new window from another one at the same place.
const CASCADE: f32 = 32.;

/// Window options for a window placed at `saved` (or centered).
pub fn window_options(saved: Option<WindowState>, cx: &App) -> WindowOptions {
    let saved = saved.filter(|w| w.width >= 400. && w.height >= 300.);
    let window_bounds = match saved {
        Some(w) => {
            let bounds = Bounds::new(point(px(w.x), px(w.y)), size(px(w.width), px(w.height)));
            if w.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }
        }
        None => {
            let screen = cx
                .primary_display()
                .map(|d| d.bounds().size)
                .unwrap_or(size(px(1440.), px(900.)));
            let width = px(1280.).min(screen.width * 0.9);
            let height = px(820.).min(screen.height * 0.85);
            WindowBounds::Windowed(Bounds::centered(None, size(width, height), cx))
        }
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

/// Every open workspace window.
pub fn workspace_windows(cx: &App) -> Vec<WindowHandle<Workspace>> {
    cx.windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        .collect()
}

/// The project roots open in windows other than the one being updated
/// (a window cannot be read while it is updated), with their windows.
pub fn open_projects(cx: &App) -> Vec<(PathBuf, WindowHandle<Workspace>)> {
    workspace_windows(cx)
        .into_iter()
        .filter_map(|handle| {
            let root = handle.read(cx).ok()?.project_root()?.to_path_buf();
            Some((root, handle))
        })
        .collect()
}

/// The window showing `root`, if another window has it open.
pub fn window_for_project(root: &Path, cx: &App) -> Option<WindowHandle<Workspace>> {
    open_projects(cx)
        .into_iter()
        .find(|(r, _)| r == root)
        .map(|(_, handle)| handle)
}

/// Moves `saved` down and right while another window sits at exactly the
/// same place, so a new window never hides an old one.
fn cascade(mut saved: WindowState, taken: &[WindowState]) -> WindowState {
    for _ in 0..20 {
        let clash = taken
            .iter()
            .any(|t| (t.x - saved.x).abs() < 4. && (t.y - saved.y).abs() < 4.);
        if !clash {
            break;
        }
        saved.x += CASCADE;
        saved.y += CASCADE;
    }
    saved
}

/// Moves `s` so it fits on a display at `(dx, dy)` sized `dw` x `dh`
/// (shrinking it if it is larger), e.g. after cascading or when the saved
/// place was on a display that is gone.
fn keep_on_screen(mut s: WindowState, dx: f32, dy: f32, dw: f32, dh: f32) -> WindowState {
    s.width = s.width.min(dw);
    s.height = s.height.min(dh);
    s.x = s.x.clamp(dx, dx + dw - s.width);
    s.y = s.y.clamp(dy, dy + dh - s.height);
    s
}

/// The display (`[x, y, width, height]`) that holds the top-left corner
/// of `s`, so a window saved on a secondary monitor stays there.
fn display_for(s: &WindowState, displays: &[[f32; 4]]) -> Option<[f32; 4]> {
    displays
        .iter()
        .find(|[x, y, w, h]| s.x >= *x && s.x < x + w && s.y >= *y && s.y < y + h)
        .copied()
}

/// Opens a new window, optionally with `project` (its session restored)
/// and `file`. The window goes where `project`'s window was last, else
/// where the last window was, shifted when another window is there.
pub fn open_window(
    project: Option<PathBuf>,
    file: Option<PathBuf>,
    cx: &mut App,
) -> Option<WindowHandle<Workspace>> {
    let saved = project
        .as_deref()
        .and_then(|p| app_state::session(p, cx))
        .and_then(|s| s.window)
        .or(app_state::state(cx).window);
    let taken: Vec<WindowState> = cx
        .windows()
        .into_iter()
        .filter_map(|w| {
            w.update(cx, |_, window, _| app_state::window_state(window))
                .ok()
        })
        .collect();
    let displays: Vec<[f32; 4]> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            [
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ]
        })
        .collect();
    let primary = cx.primary_display().map(|d| d.bounds()).map(|b| {
        [
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        ]
    });
    let saved = saved.map(|s| {
        if s.maximized {
            return s;
        }
        let s = cascade(s, &taken);
        match display_for(&s, &displays).or(primary) {
            Some([dx, dy, dw, dh]) => keep_on_screen(s, dx, dy, dw, dh),
            None => s,
        }
    });
    let options = window_options(saved, cx);
    let opened = cx.open_window(options, |window, cx| {
        let ws = cx.new(|cx| Workspace::new(window, cx));
        ws.update(cx, |ws, cx| {
            if let Some(project) = project {
                ws.open_project(project, window, cx);
            }
            if let Some(file) = &file {
                ws.open_file(file, window, cx);
            }
        });
        if file.is_none() && ws.read(cx).active_editor(cx).is_none() {
            window.focus(&ws.focus_handle(cx));
        }
        ws
    });
    match opened {
        Ok(handle) => {
            let titles: Vec<String> = workspace_windows(cx)
                .iter()
                .filter_map(|h| h.read(cx).ok().map(|ws| ws.window_title()))
                .collect();
            handle
                .update(cx, |ws, window, _| {
                    if let Some(s) = saved.filter(|s| !s.maximized) {
                        let title = ws.window_title();
                        let same = titles.iter().filter(|t| **t == title).count();
                        x11::place(window, title, same, s.x, s.y);
                    }
                    window.activate_window();
                })
                .ok();
            Some(handle)
        }
        Err(err) => {
            log::error!("failed to open a window: {err:#}");
            None
        }
    }
}

/// Opens `root` in a new window, or brings forward the window that
/// already shows it.
pub fn open_project_in_new_window(root: PathBuf, cx: &mut App) {
    // Deferred so the calling window can be read (and cascaded from).
    cx.defer(move |cx| open_project_in_new_window_now(root, cx));
}

fn open_project_in_new_window_now(root: PathBuf, cx: &mut App) {
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    if let Some(handle) = window_for_project(&root, cx) {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        return;
    }
    open_window(Some(root), None, cx);
}

/// Records every window's session and placement and which projects are
/// open (reopened at the next start), then saves the state.
pub fn record_all(cx: &mut App) {
    let mut open = Vec::new();
    for handle in workspace_windows(cx) {
        handle
            .update(cx, |ws, window, cx| {
                ws.save_session(Some(window), cx);
                app_state::record_window(window, cx);
                if let Some(root) = ws.project_root() {
                    open.push(root.to_path_buf());
                }
            })
            .ok();
    }
    app_state::set_open_projects(open, cx);
    app_state::save(cx);
}

/// Bookkeeping for a window about to close: its session and placement are
/// saved; its project is reopened at the next start only if it was the
/// last window.
pub fn window_closing(ws: &gpui::Entity<Workspace>, window: &mut gpui::Window, cx: &mut App) {
    let (root, session) = {
        let ws = ws.read(cx);
        (
            ws.project_root().map(Path::to_path_buf),
            ws.capture_session(Some(window), cx),
        )
    };
    if let (Some(root), Some(session)) = (&root, session) {
        app_state::set_session(root, session, cx);
    }
    app_state::record_window(window, cx);
    let others = workspace_windows(cx).len().saturating_sub(1);
    let mut open = app_state::open_projects(cx);
    if let Some(root) = &root {
        open.retain(|p| p != root);
        if others == 0 {
            open = vec![root.clone()];
        }
    } else if others == 0 {
        open.clear();
    }
    app_state::set_open_projects(open, cx);
    app_state::save(cx);
}

/// Quits after saving every session; asks first when any window has
/// unsaved changes.
pub fn quit(cx: &mut App) {
    // A Quit dispatched from a window runs while that window is updated,
    // and an updated window cannot be read: look at the windows after.
    cx.defer(quit_now);
}

fn quit_now(cx: &mut App) {
    let mut dirty = 0;
    let mut pids = Vec::new();
    let mut ask_in = None;
    for handle in workspace_windows(cx) {
        let Ok(ws) = handle.read(cx) else { continue };
        let count = ws.dirty_count(cx);
        let shells = ws.shell_pids(cx);
        if (count > 0 || !shells.is_empty()) && ask_in.is_none() {
            ask_in = Some(handle);
        }
        dirty += count;
        pids.extend(shells);
    }
    if dirty == 0 && pids.is_empty() {
        record_all(cx);
        cx.quit();
        return;
    }
    let busy = close_guard::count_busy_shells(pids, cx);
    cx.spawn(async move |cx| {
        let busy = busy.await;
        cx.update(|cx| ask_before_quitting(dirty, busy, ask_in, cx))
            .ok();
    })
    .detach();
}

/// Quits, first asking in `ask_in` when something would be lost.
fn ask_before_quitting(
    dirty: usize,
    busy: usize,
    ask_in: Option<WindowHandle<Workspace>>,
    cx: &mut App,
) {
    let warning = close_guard::close_warning(dirty, busy, "Quit anyway?");
    let (Some(warning), Some(handle)) = (warning, ask_in) else {
        record_all(cx);
        cx.quit();
        return;
    };
    handle
        .update(cx, |_, window, cx| {
            window.activate_window();
            let answer = window.prompt(
                PromptLevel::Warning,
                &warning.message,
                Some(warning.detail),
                &["Quit Anyway", "Cancel"],
                cx,
            );
            cx.spawn(async move |_, cx| {
                if answer.await.ok() == Some(0) {
                    cx.update(|cx| {
                        record_all(cx);
                        cx.quit();
                    })
                    .ok();
                }
            })
            .detach();
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> WindowState {
        WindowState {
            x,
            y,
            width: 800.,
            height: 600.,
            maximized: false,
        }
    }

    #[test]
    fn windows_are_kept_on_screen() {
        let s = keep_on_screen(at(900., 500.), 0., 0., 1280., 800.);
        assert_eq!((s.x, s.y, s.width, s.height), (480., 200., 800., 600.));
        let s = keep_on_screen(at(-50., 10.), 0., 0., 640., 480.);
        assert_eq!((s.x, s.y, s.width, s.height), (0., 0., 640., 480.));
    }

    #[test]
    fn a_window_stays_on_the_display_it_was_on() {
        let displays = [[0., 0., 1280., 800.], [1280., -200., 1920., 1080.]];
        let s = at(2000., 100.);
        let d = display_for(&s, &displays).expect("second display");
        assert_eq!(d, displays[1]);
        let [dx, dy, dw, dh] = d;
        let placed = keep_on_screen(s, dx, dy, dw, dh);
        assert_eq!((placed.x, placed.y), (2000., 100.));
        // Left of every display: no match (the caller uses the primary).
        assert_eq!(display_for(&at(-500., 10.), &displays), None);
    }

    #[test]
    fn new_windows_cascade_away_from_taken_places() {
        let taken = [at(10., 10.), at(42., 42.)];
        let placed = cascade(at(10., 10.), &taken);
        assert_eq!((placed.x, placed.y), (74., 74.));
        let free = cascade(at(300., 10.), &taken);
        assert_eq!((free.x, free.y), (300., 10.));
    }
}
