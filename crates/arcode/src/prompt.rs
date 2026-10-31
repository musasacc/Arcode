//! A themed in-window dialog used for prompts on platforms without native
//! dialogs (Linux). Enter picks the first button, Escape the cancel one.

use std::collections::HashMap;

use arcode_ui_kit::metrics::WEIGHT_SEMIBOLD;
use arcode_ui_kit::{
    ButtonStyle, IconName, button, icon, overlay, theme, type_scale, ui_font_family,
};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, PromptButton,
    PromptHandle, PromptLevel, PromptResponse, RenderablePromptHandle, SharedString, WeakEntity,
    Window, WindowId, div, prelude::*, px,
};

/// Marks that [`init`] chose the app's prompts (see [`restore`]).
struct AppPrompts;

impl gpui::Global for AppPrompts {}

/// The app's prompt shown in each window (see [`open_in`]).
#[derive(Default)]
struct OpenPrompts(HashMap<WindowId, WeakEntity<PromptDialog>>);

impl gpui::Global for OpenPrompts {}

/// The app's prompt open in `window`, if any. The plugin consent dialog
/// waits for it to go (GPUI shows one prompt per window; a new one would
/// replace it).
pub fn open_in(window: &Window, cx: &App) -> Option<Entity<PromptDialog>> {
    let id = window.window_handle().window_id();
    cx.try_global::<OpenPrompts>()?.0.get(&id)?.upgrade()
}

/// Installs the themed prompt (Linux and other non-macOS/Windows Unixes;
/// macOS and Windows keep their native dialogs).
pub fn init(cx: &mut App) {
    cx.set_global(AppPrompts);
    if cfg!(not(any(target_os = "macos", target_os = "windows"))) {
        cx.set_prompt_builder(build);
    }
}

/// Puts back the prompts [`init`] chose (or GPUI's default without it)
/// after a one-off custom prompt such as the plugin consent dialog.
pub fn restore(cx: &mut App) {
    cx.reset_prompt_builder();
    if cx.has_global::<AppPrompts>() {
        init(cx);
    }
}

fn build(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let view = cx.new(|cx| PromptDialog {
        level,
        message: message.to_string().into(),
        detail: detail.map(|d| SharedString::from(d.to_string())),
        buttons: buttons.iter().map(|b| b.label().clone()).collect(),
        cancel: buttons
            .iter()
            .position(|b| matches!(b, PromptButton::Cancel(_)))
            .or_else(|| buttons.iter().position(|b| b.label().as_ref() == "Cancel"))
            .unwrap_or(buttons.len().saturating_sub(1)),
        focus: cx.focus_handle(),
    });
    let id = window.window_handle().window_id();
    let open = &mut cx.default_global::<OpenPrompts>().0;
    open.retain(|_, p| p.upgrade().is_some());
    open.insert(id, view.downgrade());
    handle.with_view(view, window, cx)
}

/// The dialog view.
pub struct PromptDialog {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    buttons: Vec<SharedString>,
    cancel: usize,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for PromptDialog {}

impl Focusable for PromptDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PromptDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors.clone();
        let scale = type_scale(cx);
        let (icon_name, icon_color) = match self.level {
            PromptLevel::Critical => (IconName::CircleX, c.error),
            PromptLevel::Warning => (IconName::TriangleAlert, c.warning),
            PromptLevel::Info => (IconName::Info, c.info),
        };
        // The first button is the default (Enter): the one Primary, drawn
        // last so it sits at the right; the rest are Secondary.
        let buttons: Vec<_> = self
            .buttons
            .iter()
            .enumerate()
            .rev()
            .map(|(ix, label)| {
                let style = if ix == 0 {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                };
                button(ix, label.clone())
                    .style(style)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(PromptResponse(ix))))
                    .into_any_element()
            })
            .collect();
        let dialog = overlay(cx)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => cx.emit(PromptResponse(0)),
                    "escape" => cx.emit(PromptResponse(this.cancel)),
                    _ => {}
                }
            }))
            .w(px(440.))
            .font_family(ui_font_family(cx))
            .text_size(scale.body)
            .text_color(c.text)
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .px(px(16.))
                    .pt(px(16.))
                    .child(icon(icon_name).size(px(20.)).color(icon_color))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(scale.title)
                                    .font_weight(WEIGHT_SEMIBOLD)
                                    .child(self.message.clone()),
                            )
                            .when_some(self.detail.clone(), |el, detail| {
                                el.child(div().text_color(c.text_muted).child(detail))
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(16.))
                    .pt(px(16.))
                    .pb(px(14.))
                    .children(buttons),
            );
        // No dim behind dialogs (docs/DESIGN.md 5.2): the layer only blocks
        // the window below.
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(dialog)
    }
}
