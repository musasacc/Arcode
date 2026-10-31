//! The tour overlay: a dimmed backdrop with a cut-out over the highlighted
//! element and a callout bubble with Back / Next / Skip.

use arcode_ui_kit::{ButtonStyle, Elevated, button, theme};
use gpui::{Bounds, Context, Div, Pixels, Stateful, Window, div, point, prelude::*, px, size};

use super::Workspace;
use super::overlays::fade_in;
use crate::bottom_panel::BottomTab;
use crate::prefs::tour::{
    CALLOUT_WIDTH, CONTEXT, EndTour, NextStep, PreviousStep, Tour, TourTarget, callout_origin,
};

/// Room left around the highlighted element.
const PAD: f32 = 4.;

impl Workspace {
    /// Starts the tour at its first step.
    pub fn start_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu_open = false;
        // Undo a theme or font preview: the tour shows the saved settings.
        self.close_prefs_picker(true, window, cx);
        self.close_palette(window, cx);
        self.close_finder(window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.prefs.tour_focus = Some(focus);
        self.prefs.tour = Some(Tour::default());
        self.prefs.anchors.set_active(true);
        self.enter_tour_step(window, cx);
    }

    /// The running tour.
    pub fn tour(&self) -> Option<&Tour> {
        self.prefs.tour.as_ref()
    }

    /// The bounds the tour reads for its targets.
    pub fn tour_anchors(&self) -> &crate::prefs::tour::Anchors {
        &self.prefs.anchors
    }

    /// Next step, or the end of the tour after the last one.
    pub fn tour_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let more = self.prefs.tour.as_mut().is_some_and(Tour::advance);
        if more {
            self.enter_tour_step(window, cx);
        } else {
            self.end_tour(window, cx);
        }
    }

    /// Previous step.
    pub fn tour_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tour) = &mut self.prefs.tour {
            tour.back();
            self.enter_tour_step(window, cx);
        }
    }

    /// Ends the tour; a bottom panel it opened is hidden again.
    pub fn end_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tour) = self.prefs.tour.take() else {
            return;
        };
        self.prefs.anchors.set_active(false);
        self.prefs.tour_focus = None;
        if tour.opened_panel {
            self.bottom_panel_visible = false;
        }
        self.active_pane().read(cx).focus(window, cx);
        cx.notify();
    }

    /// Makes the step's element visible (the bottom panel for the
    /// terminal step).
    fn enter_tour_step(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(tour) = &mut self.prefs.tour else {
            return;
        };
        let wants_panel = tour.current().target == TourTarget::TerminalPanel;
        if wants_panel && self.project.is_some() && !self.bottom_panel_visible {
            tour.opened_panel = true;
            self.bottom_panel_visible = true;
            self.bottom_panel.update(cx, |panel, cx| {
                panel.set_active_tab(BottomTab::Terminal, cx)
            });
        }
        cx.notify();
    }

    /// Bounds of the step's target in the last frame, when it is drawn.
    fn tour_target_bounds(&self, target: TourTarget, cx: &gpui::App) -> Option<Bounds<Pixels>> {
        let workbench = !self.shows_welcome(cx);
        let drawn = match target {
            TourTarget::ActivityBar => workbench,
            TourTarget::RunButton => self.project.is_some(),
            TourTarget::TerminalPanel => self.bottom_panel_visible && workbench,
            _ => true,
        };
        drawn.then(|| self.prefs.anchors.get(target)).flatten()
    }

    pub(super) fn render_tour_overlay(
        &self,
        root: Stateful<Div>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (Some(tour), Some(focus)) = (&self.prefs.tour, &self.prefs.tour_focus) else {
            return root;
        };
        let t = theme(cx);
        let c = t.colors.clone();
        let dim = if t.is_dark {
            c.background.opacity(0.72)
        } else {
            c.text.opacity(0.4)
        };
        let viewport = window.viewport_size();
        let step = tour.current().clone();
        let target = self.tour_target_bounds(step.target, cx).map(|b| {
            let origin = point(
                (b.origin.x - px(PAD)).max(px(0.)),
                (b.origin.y - px(PAD)).max(px(0.)),
            );
            let end = point(
                (b.origin.x + b.size.width + px(PAD)).min(viewport.width),
                (b.origin.y + b.size.height + px(PAD)).min(viewport.height),
            );
            Bounds::from_corners(origin, end)
        });
        let rect = |x: Pixels, y: Pixels, w: Pixels, h: Pixels| {
            div()
                .absolute()
                .left(x)
                .top(y)
                .w(w.max(px(0.)))
                .h(h.max(px(0.)))
                .bg(dim)
        };
        let mut overlay = div()
            .id("tour-overlay")
            .key_context(CONTEXT)
            .track_focus(focus)
            .on_action(cx.listener(|ws, _: &NextStep, window, cx| ws.tour_next(window, cx)))
            .on_action(cx.listener(|ws, _: &PreviousStep, window, cx| ws.tour_back(window, cx)))
            .on_action(cx.listener(|ws, _: &EndTour, window, cx| ws.end_tour(window, cx)))
            .absolute()
            .inset_0()
            .occlude();
        overlay = match target {
            Some(b) => {
                let (x0, y0) = (b.origin.x, b.origin.y);
                let (x1, y1) = (x0 + b.size.width, y0 + b.size.height);
                overlay
                    .child(rect(px(0.), px(0.), viewport.width, y0))
                    .child(rect(px(0.), y1, viewport.width, viewport.height - y1))
                    .child(rect(px(0.), y0, x0, b.size.height))
                    .child(rect(x1, y0, viewport.width - x1, b.size.height))
                    .child(
                        div()
                            .absolute()
                            .left(x0)
                            .top(y0)
                            .w(b.size.width)
                            .h(b.size.height)
                            .rounded_md()
                            .border_2()
                            .border_color(c.accent),
                    )
            }
            None => overlay.child(rect(px(0.), px(0.), viewport.width, viewport.height)),
        };
        let origin = callout_origin(target, size(viewport.width, viewport.height));
        let hidden = target.is_none() && step.needs_project;
        root.child(
            overlay.child(div().absolute().left(origin.x).top(origin.y).child(fade_in(
                ("tour-callout", tour.step),
                self.render_callout(tour, hidden, cx),
                cx,
            ))),
        )
    }

    fn render_callout(&self, tour: &Tour, hidden: bool, cx: &mut Context<Self>) -> Div {
        let c = theme(cx).colors.clone();
        let step = tour.current();
        let count = format!("{} of {}", tour.step + 1, tour.steps.len());
        let next = if tour.is_last() { "Finish" } else { "Next" };
        div()
            .w(px(CALLOUT_WIDTH))
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(c.accent)
            .bg(c.surface_raised)
            .elevated_lg(cx)
            .text_color(c.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(step.title),
                    )
                    .child(div().text_xs().text_color(c.text_muted).child(count)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(c.text_muted)
                    .child(step.body.clone()),
            )
            .when(hidden, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(c.info)
                        .child("Open a folder to see it."),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(
                        button("tour-skip", "Skip tour")
                            .style(ButtonStyle::Ghost)
                            .on_click(cx.listener(|ws, _, window, cx| ws.end_tour(window, cx))),
                    )
                    .child(div().flex_1())
                    .when(tour.step > 0, |el| {
                        el.child(
                            button("tour-back", "Back")
                                .style(ButtonStyle::Secondary)
                                .on_click(
                                    cx.listener(|ws, _, window, cx| ws.tour_back(window, cx)),
                                ),
                        )
                    })
                    .child(
                        button("tour-next", next)
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|ws, _, window, cx| ws.tour_next(window, cx))),
                    ),
            )
    }
}
