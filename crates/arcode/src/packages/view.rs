//! The Packages view in the sidebar: a section per manifest (manager
//! badge, toolbar), a filter box and the dependency list. Actions are
//! emitted as [`PackagesEvent`]s; the workspace runs them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use arcode_project::deps::{PackageAction, ProjectManifest};
use arcode_ui_kit::{IconName, icon, icon_button, theme};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use super::forms::{AddForm, DepMenu};
use super::rows::{Row, build_rows, target_manifest};
use super::store::PackagesStore;
use super::view_actions::DepKey;
use crate::text_field::{TextChanged, TextField};

/// Height of every list row.
pub(super) const ROW_HEIGHT: f32 = 24.;

/// What the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackagesEvent {
    /// Open `path` at zero-based `line`.
    Open {
        /// The manifest.
        path: PathBuf,
        /// The declaration's line.
        line: usize,
    },
    /// Run `action` for the manifest at `manifest`.
    Run {
        /// The manifest.
        manifest: PathBuf,
        /// What to run.
        action: PackageAction,
    },
    /// Open a registry page in the browser.
    OpenUrl(String),
}

/// A request from the command palette, run once the project is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackagesRequest {
    /// Open the Add form for the target manifest.
    Add,
    /// Update everything in the target manifest.
    UpdateAll,
    /// Check every manifest for updates.
    CheckUpdates,
}

/// The Packages view.
pub struct PackagesView {
    pub(super) store: Entity<PackagesStore>,
    pub(super) filter: Entity<TextField>,
    pub(super) collapsed: HashSet<PathBuf>,
    pub(super) rows: Vec<Row>,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) focus_handle: FocusHandle,
    /// The Add form, while open.
    pub(super) add: Option<AddForm>,
    /// The dependency context menu, while open.
    pub(super) menu: Option<DepMenu>,
    /// The dependency whose removal waits for confirmation.
    pub(super) confirm_remove: Option<DepKey>,
    pending: Vec<(PackagesRequest, Option<PathBuf>)>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<PackagesEvent> for PackagesView {}

impl Focusable for PackagesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PackagesView {
    /// A view over `store`.
    pub fn new(store: Entity<PackagesStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextField::new("Filter packages", cx));
        let subs = vec![
            cx.observe_in(&store, window, |view, _, window, cx| {
                view.forget_stale_targets(cx);
                view.rebuild(cx);
                view.run_pending(window, cx);
            }),
            cx.subscribe(&filter, |view, _, _: &TextChanged, cx| view.rebuild(cx)),
        ];
        let mut view = Self {
            store,
            filter,
            collapsed: HashSet::new(),
            rows: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            add: None,
            menu: None,
            confirm_remove: None,
            pending: Vec::new(),
            _subs: subs,
        };
        view.rebuild(cx);
        view
    }

    /// The rows as displayed.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The store.
    pub fn store(&self) -> &Entity<PackagesStore> {
        &self.store
    }

    /// Sets the filter text.
    pub fn set_filter(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.update(cx, |f, cx| f.set_text(text, cx));
        self.rebuild(cx);
    }

    pub(super) fn rebuild(&mut self, cx: &mut Context<Self>) {
        let text = self.filter.read(cx).text().to_string();
        let rows = build_rows(self.store.read(cx).manifests(), &text, &self.collapsed);
        if rows != self.rows {
            self.rows = rows;
        }
        cx.notify();
    }

    /// Runs `request` now, or once the project is read. `hint` (the active
    /// file) picks the manifest of Add and Update All.
    pub fn request(
        &mut self,
        request: PackagesRequest,
        hint: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending.push((request, hint));
        self.store.update(cx, |s, cx| s.ensure_loaded(cx));
        self.run_pending(window, cx);
    }

    fn run_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.store.read(cx).is_loaded() || self.pending.is_empty() {
            return;
        }
        for (request, hint) in std::mem::take(&mut self.pending) {
            let manifests = self.store.read(cx).manifests().to_vec();
            if manifests.is_empty() {
                continue;
            }
            match request {
                PackagesRequest::CheckUpdates => {
                    for m in &manifests {
                        self.store.update(cx, |s, cx| s.check_updates(&m.path, cx));
                    }
                }
                PackagesRequest::Add | PackagesRequest::UpdateAll => {
                    let Some(target) = target_manifest(&manifests, hint.as_deref()) else {
                        continue;
                    };
                    let path = target.path.clone();
                    if request == PackagesRequest::Add {
                        self.open_add(&path, window, cx);
                    } else {
                        self.run(&path, PackageAction::UpdateAll, cx);
                    }
                }
            }
        }
    }

    /// The manifest of section `ix`.
    pub(super) fn manifest<'a>(&self, ix: usize, cx: &'a App) -> Option<&'a ProjectManifest> {
        self.store.read(cx).manifests().get(ix)
    }

    /// Asks the workspace to run `action` for `manifest`.
    pub fn run(&mut self, manifest: &Path, action: PackageAction, cx: &mut Context<Self>) {
        cx.emit(PackagesEvent::Run {
            manifest: manifest.to_path_buf(),
            action,
        });
    }

    /// Collapses or expands section `ix`.
    pub fn toggle_section(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(path) = self.manifest(ix, cx).map(|m| m.path.clone()) else {
            return;
        };
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        self.rebuild(cx);
    }

    /// Opens the manifest at the declaration of dependency `dep` of
    /// section `section`.
    pub fn open_dep(&mut self, section: usize, dep: usize, cx: &mut Context<Self>) {
        let Some(m) = self.manifest(section, cx) else {
            return;
        };
        let line = m.deps.get(dep).and_then(|d| d.line).unwrap_or(0);
        let path = m.path.clone();
        cx.emit(PackagesEvent::Open { path, line });
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = theme(cx).colors.clone();
        let store = self.store.read(cx);
        let text = if store.root().is_none() {
            "Open a folder to see its packages."
        } else if !store.is_loaded() {
            "Reading manifests…"
        } else if store.manifests().is_empty() {
            "No package manifests found (Cargo.toml, package.json, pyproject.toml, requirements.txt, Pipfile, go.mod, composer.json, Gemfile)."
        } else {
            "No packages match the filter."
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .text_sm()
            .text_center()
            .text_color(c.text_muted)
            .child(icon(IconName::Package).size(px(24.)).color(c.text_disabled))
            .child(text)
    }
}

impl Render for PackagesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Shown for the first time: read the project now (never before).
        let store = self.store.clone();
        if store.read(cx).root().is_some()
            && !store.read(cx).is_loaded()
            && !store.read(cx).is_loading()
        {
            cx.defer(move |cx| store.update(cx, |s, cx| s.ensure_loaded(cx)));
        }
        let c = theme(cx).colors.clone();
        let count: usize = self
            .store
            .read(cx)
            .manifests()
            .iter()
            .map(|m| m.deps.len())
            .sum();
        let loading = self.store.read(cx).is_loading();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .pr_1()
            .child(crate::sidebar::header(&format!("Packages ({count})"), cx))
            .child(
                icon_button(
                    "packages-refresh-all",
                    if loading {
                        IconName::Loader
                    } else {
                        IconName::Refresh
                    },
                )
                .size(px(22.))
                .tooltip(if loading {
                    "Reading manifests…"
                } else {
                    "Refresh"
                })
                .on_click(cx.listener(|view, _, _, cx| {
                    view.store.update(cx, |s, cx| s.refresh(cx));
                })),
            );
        let filter = div().px_2().pb_1().child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .h(px(26.))
                .px_1p5()
                .rounded_md()
                .border_1()
                .border_color(c.border)
                .bg(c.background)
                .child(icon(IconName::Search).size(px(12.)).color(c.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .overflow_hidden()
                        .child(self.filter.clone()),
                ),
        );
        let add_form = self.render_add_form(cx);
        let menu = self.render_menu(window, cx);
        let list = (!self.rows.is_empty()).then(|| {
            uniform_list(
                "packages-list",
                self.rows.len(),
                cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                    range.map(|ix| view.render_row(ix, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1()
        });
        let empty = self.rows.is_empty().then(|| self.render_empty(cx));
        div()
            .id("packages-view")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .text_sm()
            .child(header)
            .child(filter)
            .children(add_form)
            .children(list)
            .children(empty)
            .children(menu)
    }
}
