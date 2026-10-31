//! The "This project has packages that are not installed" banner at the
//! top of the editor area. The check is cheap and offline
//! (`arcode_project::missing_packages`) and runs in the background when a
//! project or file opens and when manifests or install folders change.
//! Nothing is installed until "Install packages" is clicked; the install
//! commands then run in terminal tabs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use arcode_terminal_view::TerminalView;
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, settings, theme};
use gpui::{AnyElement, App, Context, Entity, Window, div, prelude::*, px};

use super::Workspace;
use crate::app_state;
use crate::project::relative_display;

/// Wait after the last trigger before checking.
const CHECK_DELAY: Duration = Duration::from_millis(300);

/// Wait after an install folder appeared before checking once more: the
/// folder comes first, what makes it usable (`.venv/bin/python`) a moment
/// later, and nothing inside ignored folders is reported.
const FOLLOW_UP_DELAY: Duration = Duration::from_secs(4);

/// Install folders and files whose appearance (or removal) is reported by
/// the watcher although they are usually gitignored
/// (`git::watcher::INSTALL_MARKERS`).
fn is_install_marker(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| crate::git::watcher::INSTALL_MARKERS.contains(&n))
}

/// Changed files and folders that can change the answer.
fn is_relevant(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    is_install_marker(path)
        || matches!(
            name,
            "package.json"
                | "package-lock.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "bun.lock"
                | "bun.lockb"
                | "pyproject.toml"
                | "Pipfile"
                | ".venv"
                | "venv"
                | "env"
                | "uv.lock"
                | "poetry.lock"
                | "go.mod"
                | "go.sum"
                | "vendor"
                | "Gemfile"
                | "Gemfile.lock"
                | "composer.json"
                | "composer.lock"
                | "Pipfile.lock"
                | "pdm.lock"
                | "npm-shrinkwrap.json"
        )
        || (name.starts_with("requirements") && name.ends_with(".txt"))
}

impl Workspace {
    /// Checks the project for missing packages after a short delay
    /// (replacing a pending check). Clears the banner when there is no
    /// project, the banner is turned off, or the project said "don't ask".
    pub(super) fn schedule_package_check(&mut self, cx: &mut Context<Self>) {
        self.schedule_package_check_with(false, cx);
    }

    /// Like [`Self::schedule_package_check`]; with `follow_up`, a banner
    /// still shown after the check is checked once more a few seconds
    /// later (an install folder just appeared and may not be complete).
    fn schedule_package_check_with(&mut self, follow_up: bool, cx: &mut Context<Self>) {
        let root = self.project_root().map(Path::to_path_buf);
        let off = !settings(cx).workspace.package_banner;
        let Some(root) = root.filter(|r| !off && !app_state::state(cx).package_banner_hidden(r))
        else {
            let banner = &mut self.langs.banner;
            banner.task = None;
            if !banner.found.is_empty() {
                banner.found.clear();
                cx.notify();
            }
            return;
        };
        let opened: Vec<PathBuf> = self
            .editors(cx)
            .iter()
            .filter_map(|e| e.read(cx).path().map(Path::to_path_buf))
            .collect();
        self.langs.banner.task = Some(cx.spawn(async move |this, cx| {
            let delays: &[Duration] = if follow_up {
                &[CHECK_DELAY, FOLLOW_UP_DELAY]
            } else {
                &[CHECK_DELAY]
            };
            for delay in delays {
                cx.background_executor().timer(*delay).await;
                let (scan_root, opened) = (root.clone(), opened.clone());
                let found = cx
                    .background_spawn(async move {
                        arcode_project::missing_packages(&scan_root, &opened)
                    })
                    .await;
                let still_missing = this
                    .update(cx, |ws, cx| {
                        if ws.project_root() != Some(root.as_path()) {
                            return false;
                        }
                        if ws.langs.banner.found != found {
                            ws.langs.banner.found = found;
                            cx.notify();
                        }
                        !ws.langs.banner.found.is_empty()
                    })
                    .unwrap_or(false);
                if !still_missing {
                    break;
                }
            }
        }));
    }

    /// "Packages: Check for Missing Packages": shows the banner again
    /// (also after Dismiss) when something is missing.
    pub(super) fn check_packages_now(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.show_toast("Open a folder first", cx);
            return;
        };
        self.langs.banner.dismissed = false;
        let opened: Vec<PathBuf> = self
            .editors(cx)
            .iter()
            .filter_map(|e| e.read(cx).path().map(Path::to_path_buf))
            .collect();
        let work =
            cx.background_spawn(async move { arcode_project::missing_packages(&root, &opened) });
        self.langs.banner.task = Some(cx.spawn(async move |this, cx| {
            let found = work.await;
            this.update(cx, |ws, cx| {
                if found.is_empty() {
                    ws.show_toast("All packages look installed", cx);
                }
                ws.langs.banner.found = found;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Re-checks when a manifest, lockfile or install folder changed.
    pub(super) fn packages_paths_changed(
        &mut self,
        paths: &[PathBuf],
        overflow: bool,
        cx: &mut Context<Self>,
    ) {
        if overflow || paths.iter().any(|p| is_relevant(p)) {
            let follow_up = paths.iter().any(|p| is_install_marker(p));
            self.schedule_package_check_with(follow_up, cx);
        }
    }

    /// Runs the install commands of every missing manifest, each in its
    /// own terminal tab, and closes the banner for this session.
    ///
    /// Without a banner (dismissed for good, turned off, or the check has
    /// not run yet) the project is checked first, in the background.
    pub(super) fn install_packages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let found = self.langs.banner.found.clone();
        if !found.is_empty() {
            self.run_installs(found, window, cx);
            return;
        }
        let Some(root) = self.project_root().map(Path::to_path_buf) else {
            self.show_toast("Open a folder first", cx);
            return;
        };
        let opened: Vec<PathBuf> = self
            .editors(cx)
            .iter()
            .filter_map(|e| e.read(cx).path().map(Path::to_path_buf))
            .collect();
        let work =
            cx.background_spawn(async move { arcode_project::missing_packages(&root, &opened) });
        cx.spawn_in(window, async move |this, cx| {
            let found = work.await;
            this.update_in(cx, |ws, window, cx| {
                if found.is_empty() {
                    ws.show_toast("All packages look installed", cx);
                } else {
                    ws.run_installs(found, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn run_installs(
        &mut self,
        found: Vec<arcode_project::MissingPackages>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for missing in found {
            let mut steps = missing.steps.into_iter();
            let Some(first) = steps.next() else {
                continue;
            };
            let rest: Vec<String> = steps
                .map(|s| arcode_project::command_line(&s.program, &s.args))
                .collect();
            let title = format!("Install: {}", first.display);
            let line = arcode_project::command_line(&first.program, &first.args);
            let on_started: crate::terminal_panel::OnStarted = Box::new(
                move |terminal: Option<&Entity<TerminalView>>, cx: &mut App| {
                    if let Some(terminal) = terminal {
                        for command in &rest {
                            terminal.read(cx).run_command(command);
                        }
                    }
                },
            );
            self.bottom_panel.update(cx, |panel, cx| {
                panel.run_named(
                    &title,
                    &line,
                    Some(first.cwd.clone()),
                    Some(on_started),
                    window,
                    cx,
                )
            });
        }
        self.bottom_panel_visible = true;
        self.langs.banner.dismissed = true;
        cx.notify();
    }

    pub(super) fn dismiss_package_banner(&mut self, cx: &mut Context<Self>) {
        self.langs.banner.dismissed = true;
        cx.notify();
    }

    pub(super) fn never_ask_packages(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.project_root().map(Path::to_path_buf) {
            app_state::hide_package_banner(&root, cx);
        }
        self.langs.banner.found.clear();
        self.langs.banner.task = None;
        self.show_toast(
            "The package banner stays off for this project (\"Packages: Check for Missing Packages\" checks anyway)",
            cx,
        );
        cx.notify();
    }

    /// The banner, when something is missing and it was not dismissed.
    pub(super) fn render_package_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let banner = &self.langs.banner;
        if !banner.visible() {
            return None;
        }
        let c = theme(cx).colors.clone();
        let root = self.project_root()?;
        let first = banner.found.first()?;
        let mut detail = format!(
            "{} · {}",
            relative_display(root, &first.file.manifest),
            first.display()
        );
        if banner.found.len() > 1 {
            detail.push_str(&format!(" (+{} more)", banner.found.len() - 1));
        }
        let this = cx.entity().downgrade();
        let (install, dismiss, never) = (this.clone(), this.clone(), this);
        Some(
            div()
                .id("package-banner")
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .px_3()
                .py(px(6.))
                .bg(c.surface)
                .border_b_1()
                .border_color(c.border)
                .text_sm()
                .child(icon(IconName::Package).size(px(16.)).color(c.info))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child("This project has packages that are not installed.")
                        .child(
                            div()
                                .text_xs()
                                .text_color(c.text_muted)
                                .truncate()
                                .child(detail),
                        ),
                )
                .child(
                    button("package-install", "Install packages")
                        .style(ButtonStyle::Primary)
                        .tooltip("Runs the command above in a terminal")
                        .on_click(move |_, window, cx| {
                            install
                                .update(cx, |ws, cx| ws.install_packages(window, cx))
                                .ok();
                        }),
                )
                .child(
                    button("package-dismiss", "Dismiss")
                        .style(ButtonStyle::Secondary)
                        .on_click(move |_, _, cx| {
                            dismiss
                                .update(cx, |ws, cx| ws.dismiss_package_banner(cx))
                                .ok();
                        }),
                )
                .child(
                    button("package-never", "Don't ask again for this project")
                        .style(ButtonStyle::Ghost)
                        .on_click(move |_, _, cx| {
                            never.update(cx, |ws, cx| ws.never_ask_packages(cx)).ok();
                        }),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevant_paths() {
        assert!(is_relevant(Path::new("/p/node_modules")));
        assert!(is_relevant(Path::new("/p/requirements-dev.txt")));
        assert!(is_relevant(Path::new("/p/.venv")));
        assert!(is_relevant(Path::new("/p/composer.lock")));
        assert!(is_relevant(Path::new("/p/Pipfile.lock")));
        assert!(is_relevant(Path::new("/p/.pnp.js")));
        assert!(is_install_marker(Path::new("/p/vendor")));
        assert!(!is_install_marker(Path::new("/p/package.json")));
        assert!(!is_relevant(Path::new("/p/src/main.rs")));
    }
}
