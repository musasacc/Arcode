//! A themed in-window dialog used for prompts on platforms without native
//! dialogs (Linux). Enter picks the first button, Escape the cancel one.

use std::collections::HashMap;

use arcode_ui_kit::Elevated;
use arcode_ui_kit::{IconName, icon, theme};
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
        let (icon_name, icon_color) = match self.level {
            PromptLevel::Critical => (IconName::CircleX, c.error),
            PromptLevel::Warning => (IconName::TriangleAlert, c.warning),
            PromptLevel::Info => (IconName::Info, c.info),
        };
        let buttons = self.buttons.iter().enumerate().map(|(ix, label)| {
            let primary = ix == 0;
            div()
                .id(ix)
                .flex()
                .items_center()
                .h(px(28.))
                .px_3()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .when(primary, |el| {
                    el.bg(c.accent)
                        .text_color(c.accent_text)
                        .hover(|s| s.bg(c.accent.opacity(0.85)))
                })
                .when(!primary, |el| {
                    el.border_1()
                        .border_color(c.border)
                        .bg(c.surface)
                        .text_color(c.text)
                        .hover(|s| s.bg(c.hover))
                })
                .child(label.clone())
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(PromptResponse(ix))))
        });
        let dialog = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => cx.emit(PromptResponse(0)),
                    "escape" => cx.emit(PromptResponse(this.cancel)),
                    _ => {}
                }
            }))
            .w(px(420.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(c.border)
            .bg(c.surface_raised)
            .text_color(c.text)
            .elevated(cx)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(icon(icon_name).size(px(20.)).color(icon_color))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(self.message.clone()),
                            )
                            .when_some(self.detail.clone(), |el, detail| {
                                el.child(div().text_sm().text_color(c.text_muted).child(detail))
                            }),
                    ),
            )
            .child(div().flex().justify_end().gap_2().children(buttons.rev()));
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.3))
            .occlude()
            .child(dialog)
    }
}
