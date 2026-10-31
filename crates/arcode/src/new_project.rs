//! The New Project dialog: a name, a parent folder and a template. Create
//! makes the folder; a template's setup command (`cargo init`, ...) is run
//! in the new project's terminal only because the user picked it and
//! clicked Create. Nothing is downloaded or installed here.

use arcode_ui_kit::Elevated;
use std::path::PathBuf;

use arcode_project::templates::{ProjectTemplate, create_project, templates};
use arcode_ui_kit::{ButtonStyle, IconName, button, icon, kbd, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, PathPromptOptions,
    SharedString, Subscription, Window, actions, div, prelude::*, px,
};

use crate::path_prompt::expand_home;
use crate::text_field::{TextChanged, TextField};

actions!(new_project, [Create, Dismiss]);

const CONTEXT: &str = "NewProjectDialog";

/// Registers Enter / Escape for the dialog.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Create, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
    ]);
}

/// Events emitted by the dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum NewProjectEvent {
    /// The folder was created.
    Created {
        /// The new project folder.
        path: PathBuf,
        /// The template's command line to run in it, if any.
        command: Option<String>,
        /// Open it in a new window instead of this one.
        new_window: bool,
    },
    /// The dialog was closed.
    Dismissed,
}

/// The dialog view.
pub struct NewProjectDialog {
    name: Entity<TextField>,
    location: Entity<TextField>,
    template: &'static ProjectTemplate,
    new_window: bool,
    error: Option<SharedString>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<NewProjectEvent> for NewProjectDialog {}

impl Focusable for NewProjectDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

/// A folder name in `parent` that does not exist yet.
pub fn unused_name(parent: &std::path::Path) -> String {
    (1..1000)
        .map(|n| {
            if n == 1 {
                "untitled-project".to_owned()
            } else {
                format!("untitled-project-{n}")
            }
        })
        .find(|name| !parent.join(name).exists())
        .unwrap_or_else(|| "new-project".to_owned())
}

impl NewProjectDialog {
    /// Creates the dialog proposing a new folder in `parent`.
    pub fn new(parent: PathBuf, cx: &mut Context<Self>) -> Self {
        let name_text = unused_name(&parent);
        let name = cx.new(|cx| {
            let mut field = TextField::new("Project name", cx);
            field.set_text(&name_text, cx);
            field.select_all_text(cx);
            field
        });
        let location = cx.new(|cx| {
            let mut field = TextField::new("Parent folder", cx);
            field.set_text(&parent.display().to_string(), cx);
            field
        });
        let clear =
            |dialog: &mut Self, _: Entity<TextField>, _: &TextChanged, cx: &mut Context<Self>| {
                if dialog.error.take().is_some() {
                    cx.notify();
                }
            };
        let subs = vec![cx.subscribe(&name, clear), cx.subscribe(&location, clear)];
        Self {
            name,
            location,
            template: &templates()[0],
            new_window: false,
            error: None,
            _subs: subs,
        }
    }

    /// The selected template.
    pub fn template(&self) -> &'static ProjectTemplate {
        self.template
    }

    /// Selects the template `id` (unknown ids are ignored).
    pub fn select_template(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(t) = arcode_project::templates::template(id) {
            self.template = t;
            cx.notify();
        }
    }

    /// Sets the name field (tests, scripting).
    pub fn set_name(&mut self, name: &str, cx: &mut Context<Self>) {
        self.name.update(cx, |f, cx| f.set_text(name, cx));
    }

    /// Sets the location field (tests, scripting).
    pub fn set_location(&mut self, location: &str, cx: &mut Context<Self>) {
        self.location.update(cx, |f, cx| f.set_text(location, cx));
    }

    /// The error shown, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.as_ref())
    }

    /// Creates the folder and emits [`NewProjectEvent::Created`], or
    /// shows why it cannot.
    pub fn create(&mut self, _: &Create, _: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim().to_owned();
        let parent = expand_home(self.location.read(cx).text().trim());
        match create_project(&parent, &name) {
            Ok(path) => cx.emit(NewProjectEvent::Created {
                command: self.template.command_line(&name),
                path,
                new_window: self.new_window,
            }),
            Err(err) => {
                self.error = Some(err.to_string().into());
                cx.notify();
            }
        }
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(NewProjectEvent::Dismissed);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |dialog, cx| {
                    dialog.set_location(&path.display().to_string(), cx)
                })
                .ok();
            }
        })
        .detach();
    }

    fn render_template(
        &self,
        ix: usize,
        t: &'static ProjectTemplate,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let selected = std::ptr::eq(t, self.template);
        div()
            .id(("template", ix))
            .flex()
            .items_center()
            .gap_2()
            .h(px(28.))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .border_1()
            .border_color(if selected { c.accent } else { c.border })
            .when(selected, |el| el.bg(c.selection))
            .hover(|s| s.bg(c.hover))
            .child(
                div()
                    .size(px(12.))
                    .flex_none()
                    .rounded_full()
                    .border_1()
                    .border_color(if selected { c.accent } else { c.text_muted })
                    .when(selected, |el| el.bg(c.accent)),
            )
            .child(div().flex_none().text_sm().text_color(c.text).child(t.name))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(c.text_muted)
                    .child(arcode_ui_kit::truncated(t.description)),
            )
            .on_click(cx.listener(move |dialog, _, _, cx| dialog.select_template(t.id, cx)))
    }
}

impl Render for NewProjectDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let field = |label: &'static str, input: Entity<TextField>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(c.text_muted).child(label))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .min_w_0()
                        .overflow_hidden()
                        .px_2()
                        .rounded_md()
                        .border_1()
                        .border_color(c.border)
                        .bg(c.background)
                        .child(input),
                )
        };
        let note: SharedString = match self.template.command_line("x") {
            Some(_) => {
                let name = self.name.read(cx).text().trim().to_owned();
                let line = self.template.command_line(&name).unwrap_or_default();
                format!("After creating the folder, Create runs `{line}` in its terminal.").into()
            }
            None => "Only the folder is created; nothing is run.".into(),
        };
        let templates_list = div().flex().flex_col().gap_1().children(
            templates()
                .iter()
                .enumerate()
                .map(|(ix, t)| self.render_template(ix, t, cx)),
        );
        let new_window = self.new_window;
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::create))
            .on_action(cx.listener(Self::dismiss))
            .w(px(520.))
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(c.text)
                    .child(icon(IconName::Plus).size(px(16.)).color(c.accent))
                    .child("New Project"),
            )
            .child(field("Name", self.name.clone()))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field("Location", self.location.clone())),
                    )
                    .child(
                        button("np-browse", "Browse…")
                            .on_click(cx.listener(|d, _, window, cx| d.browse(window, cx))),
                    ),
            )
            .child(div().text_xs().text_color(c.text_muted).child("Template"))
            .child(templates_list)
            .child(div().text_xs().text_color(c.text_muted).child(note))
            .children(
                self.error
                    .clone()
                    .map(|err| div().text_xs().text_color(c.error).child(err)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("np-new-window")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(c.text_muted)
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(c.border)
                                    .when(new_window, |el| {
                                        el.bg(c.accent).child(
                                            icon(IconName::Check)
                                                .size(px(12.))
                                                .color(c.accent_text),
                                        )
                                    }),
                            )
                            .child("Open in a new window")
                            .on_click(cx.listener(|d, _, _, cx| {
                                d.new_window = !d.new_window;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(kbd("Esc"))
                            .child(
                                button("np-cancel", "Cancel")
                                    .style(ButtonStyle::Ghost)
                                    .on_click(cx.listener(|d, _, window, cx| {
                                        d.dismiss(&Dismiss, window, cx)
                                    })),
                            )
                            .child(
                                button("np-create", "Create")
                                    .style(ButtonStyle::Primary)
                                    .on_click(cx.listener(|d, _, window, cx| {
                                        d.create(&Create, window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_names_skip_existing_folders() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unused_name(dir.path()), "untitled-project");
        std::fs::create_dir(dir.path().join("untitled-project")).unwrap();
        assert_eq!(unused_name(dir.path()), "untitled-project-2");
    }
}
