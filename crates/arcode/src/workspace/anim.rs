//! Slide animations of the sidebar and the bottom panel (100-150 ms, only
//! when `arcode_ui_kit::animation_duration` allows them). No frame is drawn
//! for them once they end.

use arcode_ui_kit::animation_duration;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, Div, Pixels, div, ease_out_quint, prelude::*, px,
};

use super::Workspace;
use crate::prefs::Slide;

/// Which panel slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slider {
    Sidebar,
    Panel,
}

impl Workspace {
    fn slide(&self, which: Slider) -> &Slide {
        match which {
            Slider::Sidebar => &self.prefs.anim.sidebar,
            Slider::Panel => &self.prefs.anim.panel,
        }
    }

    /// Starts sliding `which` open or closed (after its visibility flag
    /// changed). Does nothing when animations are off.
    pub(super) fn start_slide(&mut self, which: Slider, opening: bool, cx: &mut Context<Self>) {
        let Some(duration) = animation_duration(cx) else {
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |ws, cx| {
                let slide = match which {
                    Slider::Sidebar => &mut ws.prefs.anim.sidebar,
                    Slider::Panel => &mut ws.prefs.anim.panel,
                };
                slide.task = None;
                cx.notify();
            })
            .ok();
        });
        let slide = match which {
            Slider::Sidebar => &mut self.prefs.anim.sidebar,
            Slider::Panel => &mut self.prefs.anim.panel,
        };
        slide.generation += 1;
        slide.opening = opening;
        slide.task = Some(task);
    }

    /// Whether `which` is still drawn while it slides closed.
    pub(super) fn sliding_closed(&self, which: Slider) -> bool {
        let slide = self.slide(which);
        slide.running() && !slide.opening
    }

    /// `el` (a panel of size `full` along its sliding axis), wrapped in a
    /// clipping box whose size animates while a slide runs.
    pub(super) fn slide_wrap(
        &self,
        which: Slider,
        el: Div,
        full: Pixels,
        cx: &Context<Self>,
    ) -> AnyElement {
        let slide = self.slide(which);
        let Some(duration) = animation_duration(cx).filter(|_| slide.running()) else {
            return el.into_any_element();
        };
        let opening = slide.opening;
        let full = f32::from(full);
        let wrapper = div().flex().flex_none().overflow_hidden().child(el);
        let (id, wrapper) = match which {
            Slider::Sidebar => ("sidebar-slide", wrapper.h_full().justify_end()),
            Slider::Panel => ("panel-slide", wrapper.w_full().flex_col().justify_end()),
        };
        wrapper
            .with_animation(
                (id, slide.generation),
                Animation::new(duration).with_easing(ease_out_quint()),
                move |w, t| {
                    let size = px(full * if opening { t } else { 1. - t });
                    match which {
                        Slider::Sidebar => w.w(size),
                        Slider::Panel => w.h(size),
                    }
                },
            )
            .into_any_element()
    }
}
