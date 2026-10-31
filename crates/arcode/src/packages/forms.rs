//! The Add form (name, optional version, dev toggle) shown above the list,
//! and the context menu of a dependency row.

use std::path::{Path, PathBuf};

use arcode_project::deps::{
    Manager, PackageAction, is_cmd_shim, validate_package_name, validate_version,
};
use arcode_ui_kit::{ButtonStyle, Elevated, IconName, button, icon, switch, theme};
use gpui::{
    AnyElement, App, AppContext, Context, Entity, Focusable, KeyBinding, Pixels, Point, Window,
    actions, anchored, deferred, div, prelude::*, px,
};

use super::rows::relative_dir;
use super::store::PackagesStoreEvent;
use super::view::PackagesView;
use super::view_actions::{DepCommand, DepKey, can_add, removable};
use crate::text_field::TextField;

actions!(packages_form, [SubmitAdd, CancelAdd]);

/// Key context of the Add form.
const FORM_CONTEXT: &str = "PackagesAddForm";

/// Enter adds, Escape closes the form.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", SubmitAdd, Some(FORM_CONTEXT)),
        KeyBinding::new("escape", CancelAdd, Some(FORM_CONTEXT)),
    ]);
}

/// The open Add form.
pub struct AddForm {
    /// The manifest the package is added to.
    pub manifest: PathBuf,
    pub(super) name: Entity<TextField>,
    pub(super) version: Entity<TextField>,
    pub(super) dev: bool,
    pub(super) error: Option<String>,
}

/// The open context menu of a dependency.
pub struct DepMenu {
    pub(super) section: usize,
    pub(super) dep: usize,
    /// The dependency it was opened for (a rescan closes the menu when
    /// the row shows another one).
    pub(super) key: DepKey,
    pub(super) position: Point<Pixels>,
}

impl PackagesView {
    /// Opens the Add form for the manifest at `manifest` and focuses its
    /// name field.
    pub fn open_add(&mut self, manifest: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .store
            .read(cx)
            .manifest(manifest)
            .is_some_and(|m| !can_add(m))
        {
            self.store.update(cx, |_, cx| {
                cx.emit(PackagesStoreEvent::Message(
                    "This Cargo.toml is a virtual workspace: add packages to one of its members"
                        .to_string(),
                ))
            });
            return;
        }
        let name = cx.new(|cx| TextField::new("Package name", cx));
        let version = cx.new(|cx| TextField::new("Version (optional)", cx));
        window.focus(&name.read(cx).focus_handle(cx));
        self.add = Some(AddForm {
            manifest: manifest.to_path_buf(),
            name,
            version,
            dev: false,
            error: None,
        });
        cx.notify();
    }

    /// The open Add form.
    pub fn add_form(&self) -> Option<&AddForm> {
        self.add.as_ref()
    }

    /// Fills the open Add form (tests and the palette).
    pub fn fill_add(&mut self, name: &str, version: &str, dev: bool, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.add {
            form.name.update(cx, |f, cx| f.set_text(name, cx));
            form.version.update(cx, |f, cx| f.set_text(version, cx));
            form.dev = dev;
            cx.notify();
        }
    }

    /// Validates the form and asks to run the add command; keeps the form
    /// open with an error when the input is not valid.
    pub fn submit_add(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.add else {
            return;
        };
        let name = form.name.read(cx).text().trim().to_string();
        let version = form.version.read(cx).text().trim().to_string();
        let manager = self
            .store
            .read(cx)
            .manifest(&form.manifest)
            .map(|m| m.manager);
        let checked = check_add_input(manager, &name, &version, cfg!(windows));
        if let Err(err) = checked {
            form.error = Some(err);
            cx.notify();
            return;
        }
        let manifest = form.manifest.clone();
        let action = PackageAction::Add {
            name,
            version: (!version.is_empty()).then_some(version),
            dev: form.dev,
        };
        self.add = None;
        self.run(&manifest, action, cx);
        cx.notify();
    }

    fn close_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(super) fn render_add_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.add.as_ref()?;
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let m = store.manifest(&form.manifest)?;
        let rel = relative_dir(store.root(), &m.path);
        let target = if rel.is_empty() {
            m.file_name()
        } else {
            format!("{rel}/{}", m.file_name())
        };
        let supports_dev = m.manager.supports_dev();
        let field = |field: &Entity<TextField>| {
            div()
                .flex()
                .items_center()
                .h(px(26.))
                .px_1p5()
                .rounded_md()
                .border_1()
                .border_color(c.border)
                .bg(c.background)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .overflow_hidden()
                        .child(field.clone()),
                )
        };
        Some(
            div()
                .key_context(FORM_CONTEXT)
                .on_action(cx.listener(|view, _: &SubmitAdd, _, cx| view.submit_add(cx)))
                .on_action(
                    cx.listener(|view, _: &CancelAdd, window, cx| view.close_add(window, cx)),
                )
                .mx_2()
                .mb_2()
                .p_2()
                .flex()
                .flex_col()
                .gap_1p5()
                .rounded_md()
                .border_1()
                .border_color(c.border_focused)
                .bg(c.surface_raised)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(c.text_muted)
                        .child(icon(IconName::Plus).size(px(12.)).color(c.accent))
                        .child(
                            div()
                                .truncate()
                                .child(format!("Add to {target} ({})", m.manager.label())),
                        ),
                )
                .child(field(&form.name))
                .child(field(&form.version))
                .when(supports_dev, |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(switch("pkg-add-dev", form.dev).on_click(cx.listener(
                                |view, _, _, cx| {
                                    if let Some(form) = &mut view.add {
                                        form.dev = !form.dev;
                                        cx.notify();
                                    }
                                },
                            )))
                            .child("Development dependency"),
                    )
                })
                .when_some(form.error.clone(), |el, err| {
                    el.child(div().text_xs().text_color(c.error).child(err))
                })
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_1()
                        .child(
                            button("pkg-add-cancel", "Cancel")
                                .style(ButtonStyle::Ghost)
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.close_add(window, cx)),
                                ),
                        )
                        .child(
                            button("pkg-add-submit", "Add")
                                .style(ButtonStyle::Primary)
                                .tooltip("Runs the package manager in a terminal")
                                .on_click(cx.listener(|view, _, _, cx| view.submit_add(cx))),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Labels of the open context menu's entries.
    pub fn menu_labels(&self, cx: &App) -> Vec<&'static str> {
        let Some(menu) = &self.menu else {
            return Vec::new();
        };
        menu_entries(self, menu, cx)
            .into_iter()
            .map(|(label, _, _)| label)
            .collect()
    }

    pub(super) fn render_menu(&self, _: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let c = theme(cx).colors.clone();
        let (section, dep) = (menu.section, menu.dep);
        let mut list = div()
            .id("packages-menu")
            .min_w(px(200.))
            .py_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .elevated(cx)
            .text_sm()
            .occlude()
            .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                view.menu = None;
                cx.notify();
            }));
        for (ix, (label, glyph, kind)) in menu_entries(self, menu, cx).into_iter().enumerate() {
            list = list.child(
                div()
                    .id(("packages-menu-item", ix))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(26.))
                    .px_2()
                    .mx_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .child(icon(glyph).size(px(14.)).color(c.text_muted))
                    .child(div().flex_1().whitespace_nowrap().child(label))
                    .on_click(
                        cx.listener(move |view, _, _, cx| view.dep_action(section, dep, kind, cx)),
                    ),
            );
        }
        Some(
            deferred(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// Checks the Add form's name and optional version for `manager`. Only
/// Windows batch shims (npm, pnpm, Yarn, Composer, Bundler) refuse
/// `< > ^ !` in versions; cargo, uv, pip... are real executables there.
pub(super) fn check_add_input(
    manager: Option<Manager>,
    name: &str,
    version: &str,
    windows: bool,
) -> Result<(), String> {
    validate_package_name(name)?;
    if version.is_empty() {
        return Ok(());
    }
    let shim = manager.is_some_and(|m| is_cmd_shim(m, windows));
    validate_version(version, shim)
}

/// The menu entries for the menu's dependency.
fn menu_entries(
    view: &PackagesView,
    menu: &DepMenu,
    cx: &App,
) -> Vec<(&'static str, IconName, DepCommand)> {
    let mut entries = vec![
        ("Go to Declaration", IconName::FileText, DepCommand::Open),
        ("Update", IconName::ArrowUp, DepCommand::Update),
    ];
    let (can_remove, has_docs) = view
        .store
        .read(cx)
        .manifests()
        .get(menu.section)
        .and_then(|m| {
            m.deps
                .get(menu.dep)
                .map(|d| (removable(m, d), m.manager.docs_url(&d.package).is_some()))
        })
        .unwrap_or((false, false));
    if can_remove {
        entries.push(("Remove…", IconName::Trash, DepCommand::Remove));
    }
    if has_docs {
        entries.push((
            "Open Documentation",
            IconName::ExternalLink,
            DepCommand::Docs,
        ));
    }
    entries
}
