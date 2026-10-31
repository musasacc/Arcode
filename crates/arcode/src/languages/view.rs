//! The Languages page (Plugins activity): one card per language pack with
//! its languages, language servers (installed or not, and the command to
//! install one, to copy), snippets, run commands and an enable switch.
//! Nothing is downloaded or installed from here.

use std::collections::{HashMap, HashSet};

use arcode_ui_kit::{IconName, icon, icon_button, settings, switch, theme};
use gpui::{
    App, ClipboardItem, Context, EventEmitter, IntoElement, SharedString, Subscription, Task,
    Window, div, prelude::*, px,
};

use super::bundle::{PackRow, ServerRow, installed_commands, pack_rows};

/// Messages for the workspace (shown as toasts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguagesEvent {
    /// A short status message.
    Message(SharedString),
}

/// The Languages page.
pub struct LanguagesView {
    rows: Vec<PackRow>,
    /// Server command → found on `PATH`. Empty until checked.
    installed: HashMap<String, bool>,
    checking: bool,
    checked: bool,
    expanded: HashSet<String>,
    check_task: Option<Task<()>>,
    write_task: Option<Task<()>>,
    /// Shown as a tab of the Plugins page, which draws the header.
    embedded: bool,
    _settings: Subscription,
}

impl EventEmitter<LanguagesEvent> for LanguagesView {}

impl LanguagesView {
    /// Creates the page; servers are looked up on `PATH` when it is first
    /// shown ([`LanguagesView::shown`]).
    pub fn new(cx: &mut Context<Self>) -> Self {
        let sub = cx.observe_global::<arcode_ui_kit::Settings>(|view: &mut Self, cx| {
            let rows = pack_rows(settings(cx));
            if rows != view.rows {
                view.rows = rows;
                if view.checked {
                    view.check_servers(cx);
                }
                cx.notify();
            }
        });
        Self {
            rows: pack_rows(settings(cx)),
            installed: HashMap::new(),
            checking: false,
            checked: false,
            expanded: HashSet::new(),
            check_task: None,
            write_task: None,
            embedded: false,
            _settings: sub,
        }
    }

    /// Leaves the "Plugins" header to the page this is a tab of.
    pub fn set_embedded(&mut self, embedded: bool, cx: &mut Context<Self>) {
        self.embedded = embedded;
        cx.notify();
    }

    /// The rows shown.
    pub fn rows(&self) -> &[PackRow] {
        &self.rows
    }

    /// Whether `command` was found on `PATH` (`None` before the check).
    pub fn is_installed(&self, command: &str) -> Option<bool> {
        self.installed.get(command).copied()
    }

    /// The page became visible: check the servers once.
    pub fn shown(&mut self, cx: &mut Context<Self>) {
        if !self.checked && !self.checking {
            self.check_servers(cx);
        }
    }

    /// Looks every server command up on `PATH` in the background.
    pub fn check_servers(&mut self, cx: &mut Context<Self>) {
        let rows = self.rows.clone();
        self.checking = true;
        let work = cx.background_spawn(async move { installed_commands(&rows) });
        self.check_task = Some(cx.spawn(async move |this, cx| {
            let found = work.await;
            this.update(cx, |view, cx| {
                view.installed = found.into_iter().collect();
                view.checking = false;
                view.checked = true;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Turns a pack on or off by writing `[language.<id>].enabled` for
    /// each of its languages to `config.toml`, then reloads the settings.
    pub fn set_enabled(&mut self, pack: &str, enabled: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|r| r.id == pack) else {
            return;
        };
        let ids = row.language_ids.clone();
        let name = row.name.clone();
        // Queued with the Settings page's writes; the result is applied.
        let write = crate::prefs::store::edit_config(
            move |source| {
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                arcode_config::config::edit::set_languages_enabled(source, &ids, enabled)
            },
            cx,
        );
        self.write_task = Some(cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |_, cx| {
                let message = match result {
                    Ok(()) => {
                        let state = if enabled { "enabled" } else { "disabled" };
                        format!("{name} {state}")
                    }
                    Err(err) => err,
                };
                cx.emit(LanguagesEvent::Message(message.into()));
            })
            .ok();
        }));
    }

    fn toggle_expanded(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    fn render_server(&self, pack: &str, ix: usize, server: &ServerRow, cx: &App) -> gpui::Div {
        let c = theme(cx).colors.clone();
        let state = self.is_installed(&server.command);
        let (glyph, color, label) = match state {
            Some(true) => (IconName::CircleCheck, c.success, "installed"),
            Some(false) => (IconName::CircleX, c.warning, "not installed"),
            None => (IconName::CircleDot, c.text_muted, "checking…"),
        };
        let hint = server
            .install_command
            .clone()
            .or_else(|| server.install_note.clone());
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_1()
                    .child(icon(glyph).size(px(12.)).color(color))
                    .child(div().text_color(c.text).child(server.name.clone()))
                    .child(div().text_color(c.text_muted).child(label)),
            )
            .when(state == Some(false), |el| {
                el.when_some(hint, |el, hint| {
                    let copy = SharedString::from(hint.clone());
                    let mono = settings(cx).editor.font_family.clone();
                    el.child(
                        div()
                            .flex()
                            .items_start()
                            .gap_1()
                            .ml(px(16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(c.background)
                                    .text_color(c.text_muted)
                                    .when(server.install_command.is_some(), |el| {
                                        el.font_family(mono)
                                    })
                                    .child(hint),
                            )
                            .child(
                                icon_button(
                                    SharedString::from(format!("copy-{pack}-{ix}")),
                                    IconName::Copy,
                                )
                                .size(px(20.))
                                .tooltip("Copy")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy.to_string(),
                                    ))
                                }),
                            ),
                    )
                })
            })
    }

    fn render_card(&self, row: &PackRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let id = row.id.clone();
        let expanded = self.expanded.contains(&row.id);
        let names: Vec<&str> = row.languages.iter().map(|l| l.name).collect();
        let files: Vec<String> = row
            .languages
            .iter()
            .flat_map(|l| l.files.iter().take(3).cloned())
            .take(5)
            .collect();
        let mut summary = vec![files.join(" ")];
        if row.snippets > 0 {
            summary.push(format!("{} snippets", row.snippets));
        }
        let toggle_id = id.clone();
        let enabled = row.enabled;
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .id(SharedString::from(format!("pack-head-{id}")))
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.))
                        .color(c.text_muted),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if enabled { c.text } else { c.text_muted })
                            .child(row.name.clone()),
                    )
                    .on_click(cx.listener(move |v, _, _, cx| v.toggle_expanded(&id, cx))),
            )
            .child(
                switch(
                    SharedString::from(format!("pack-switch-{toggle_id}")),
                    enabled,
                )
                .tooltip(if enabled {
                    "Disable: no language servers or run buttons"
                } else {
                    "Enable"
                })
                .on_click(cx.listener(move |v, _, _, cx| v.set_enabled(&toggle_id, !enabled, cx))),
            );
        let servers: Vec<_> = row
            .servers
            .iter()
            .enumerate()
            .map(|(ix, s)| self.render_server(&row.id, ix, s, cx))
            .collect();
        let details = expanded.then(|| render_details(row, cx));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .mx_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(c.border)
            .text_xs()
            .child(header)
            .when(names.len() > 1, |el| {
                el.child(div().text_color(c.text_muted).child(names.join(", ")))
            })
            .child(div().text_color(c.text_muted).child(summary.join(" · ")))
            .when(enabled, |el| el.children(servers))
            .when(!enabled, |el| {
                el.child(
                    div()
                        .text_color(c.text_disabled)
                        .child("Disabled: highlighting only"),
                )
            })
            .children(details)
    }
}

fn render_details(row: &PackRow, cx: &App) -> gpui::Div {
    let c = theme(cx).colors.clone();
    let line = |label: &str, value: String| {
        div()
            .flex()
            .gap_1()
            .child(
                div()
                    .flex_none()
                    .text_color(c.text_muted)
                    .child(label.to_string()),
            )
            .child(div().min_w_0().child(value))
    };
    let mut el = div().flex().flex_col().gap_1().pt_1();
    for language in &row.languages {
        let mut parts = vec![language.files.join(" ")];
        if !language.comments.is_empty() {
            parts.push(format!("comments {}", language.comments.join("  ")));
        }
        if !language.grammar {
            parts.push("no grammar".into());
        }
        el = el.child(line(language.name, parts.join(" · ")));
    }
    if !row.run_commands.is_empty() {
        el = el.child(line("Run", row.run_commands.join(", ")));
    }
    if !row.associations.is_empty() {
        el = el.child(line("Also", row.associations.join(" ")));
    }
    el
}

impl Render for LanguagesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let rows = self.rows.clone();
        let cards: Vec<_> = rows
            .iter()
            .map(|row| self.render_card(row, cx).into_any_element())
            .collect();
        let missing = rows
            .iter()
            .filter(|r| r.enabled)
            .flat_map(|r| &r.servers)
            .filter(|s| self.is_installed(&s.command) == Some(false))
            .count();
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(!self.embedded, |el| {
                el.child(crate::sidebar::header("Plugins", cx))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(if self.checking {
                        "LANGUAGES · checking…".to_string()
                    } else if missing > 0 {
                        format!("LANGUAGES · {missing} servers missing")
                    } else {
                        "LANGUAGES".to_string()
                    })
                    .child(
                        icon_button("languages-refresh", IconName::Refresh)
                            .size(px(22.))
                            .tooltip("Check servers again")
                            .on_click(cx.listener(|v, _, _, cx| v.check_servers(cx))),
                    ),
            )
            .child(
                div()
                    .id("languages-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pb_3()
                    .children(cards),
            )
    }
}
