//! The interactive tour: its steps, the element bounds it points at, and
//! where the callout goes. The overlay itself is drawn by the workspace
//! (`workspace/tour.rs`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, Bounds, IntoElement, KeyBinding, Pixels, Point, Size, Styled, actions, canvas, point, px,
};

use crate::actions::shortcut_label;

actions!(
    tour,
    [
        /// Goes to the next tour step (or ends the tour after the last).
        NextStep,
        /// Goes back one tour step.
        PreviousStep,
        /// Ends the tour.
        EndTour,
    ]
);

/// Key context of the tour overlay.
pub const CONTEXT: &str = "Tour";

/// Keys of the tour overlay.
pub fn bind_keys(cx: &mut App) {
    let ctx = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("right", NextStep, ctx),
        KeyBinding::new("enter", NextStep, ctx),
        KeyBinding::new("left", PreviousStep, ctx),
        KeyBinding::new("escape", EndTour, ctx),
    ]);
}

/// An element of the workspace a step points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TourTarget {
    /// The title bar's project dropdown.
    ProjectDropdown,
    /// The activity bar on the left.
    ActivityBar,
    /// The title bar's search box.
    SearchBox,
    /// The title bar's Run button.
    RunButton,
    /// The bottom panel (terminal).
    TerminalPanel,
    /// The status bar.
    StatusBar,
    /// The settings gear in the title bar.
    SettingsButton,
}

/// One step of the tour.
#[derive(Clone, Debug, PartialEq)]
pub struct TourStep {
    /// What is highlighted.
    pub target: TourTarget,
    /// Heading of the callout.
    pub title: &'static str,
    /// Text of the callout.
    pub body: String,
    /// Whether the element only exists with a folder open.
    pub needs_project: bool,
}

/// The steps, in order.
pub fn steps() -> Vec<TourStep> {
    let step = |target, title, body: String, needs_project| TourStep {
        target,
        title,
        body,
        needs_project,
    };
    vec![
        step(
            TourTarget::ProjectDropdown,
            "Projects",
            "Switch between recent projects, open a folder (in this or a new window) or start a new project here.".into(),
            false,
        ),
        step(
            TourTarget::ActivityBar,
            "Activity bar",
            "Files, Search, Source Control, Run and Debug, Languages and TODOs. Click the active icon again to hide the sidebar.".into(),
            true,
        ),
        step(
            TourTarget::SearchBox,
            "Find anything",
            format!(
                "{} finds files by name. Type > for commands, or press {} for the command palette.",
                shortcut_label("secondary-p"),
                shortcut_label("secondary-shift-p")
            ),
            false,
        ),
        step(
            TourTarget::RunButton,
            "Run",
            format!(
                "{} runs the detected command (cargo run, npm start, …) in a terminal. The arrow picks another one.",
                shortcut_label("secondary-f5")
            ),
            true,
        ),
        step(
            TourTarget::TerminalPanel,
            "Terminal and panels",
            format!(
                "A real shell in the project folder, plus Problems, TODOs and the debugger. Toggle the panel with {}.",
                shortcut_label("secondary-j")
            ),
            true,
        ),
        step(
            TourTarget::StatusBar,
            "Status bar",
            "Branch, errors and warnings, cursor position, indentation, line endings, language and language servers. Most entries can be clicked.".into(),
            false,
        ),
        step(
            TourTarget::SettingsButton,
            "Settings",
            format!(
                "{} opens themes, fonts, editor options and low-end mode. Run \"Help: Start Tour\" from the palette to see this tour again.",
                shortcut_label("secondary-,")
            ),
            false,
        ),
    ]
}

/// A running tour.
#[derive(Clone, Debug)]
pub struct Tour {
    /// Index of the shown step.
    pub step: usize,
    /// All steps.
    pub steps: Vec<TourStep>,
    /// Whether the tour showed the bottom panel (hidden again at the end).
    pub opened_panel: bool,
}

impl Default for Tour {
    fn default() -> Self {
        Self {
            step: 0,
            steps: steps(),
            opened_panel: false,
        }
    }
}

impl Tour {
    /// The shown step.
    pub fn current(&self) -> &TourStep {
        &self.steps[self.step.min(self.steps.len() - 1)]
    }

    /// Moves forward; `false` when the tour is over.
    pub fn advance(&mut self) -> bool {
        if self.step + 1 < self.steps.len() {
            self.step += 1;
            true
        } else {
            false
        }
    }

    /// Moves back (stays on the first step).
    pub fn back(&mut self) {
        self.step = self.step.saturating_sub(1);
    }

    /// Whether the shown step is the last.
    pub fn is_last(&self) -> bool {
        self.step + 1 >= self.steps.len()
    }
}

/// Bounds of the elements the tour can point at, as drawn in the last
/// frame. Recording is cheap and happens whether or not a tour runs; while
/// one runs, a change of bounds (a resize) asks for one more frame so the
/// callout follows.
#[derive(Clone, Default)]
pub struct Anchors {
    bounds: Rc<RefCell<HashMap<TourTarget, Bounds<Pixels>>>>,
    active: Rc<Cell<bool>>,
}

impl Anchors {
    /// The last bounds of `target`.
    pub fn get(&self, target: TourTarget) -> Option<Bounds<Pixels>> {
        self.bounds.borrow().get(&target).copied()
    }

    /// Forgets every bound (elements not drawn any more, e.g. the bottom
    /// panel after it was hidden, record themselves again when drawn).
    pub fn clear(&self) {
        self.bounds.borrow_mut().clear();
    }

    /// Marks a tour as running (or not).
    pub fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    /// An invisible child, filling its (relatively positioned) parent, that
    /// records the parent's bounds for `target`.
    pub fn recorder(&self, target: TourTarget) -> impl IntoElement + use<> {
        let bounds = self.bounds.clone();
        let active = self.active.clone();
        canvas(
            move |b, window, _| {
                let old = bounds.borrow_mut().insert(target, b);
                if active.get() && old != Some(b) {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// Width of the callout bubble.
pub const CALLOUT_WIDTH: f32 = 320.;
/// Height assumed for placing the callout.
pub const CALLOUT_HEIGHT: f32 = 170.;
const GAP: f32 = 12.;
const MARGIN: f32 = 12.;

/// Top-left corner of the callout for a highlighted `target` (centered
/// when there is none): below the target when it fits, else above, else
/// to its right; always inside the window.
pub fn callout_origin(target: Option<Bounds<Pixels>>, viewport: Size<Pixels>) -> Point<Pixels> {
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    let (w, h) = (CALLOUT_WIDTH, CALLOUT_HEIGHT);
    let clamp_x = |x: f32| x.min(vw - w - MARGIN).max(MARGIN);
    let clamp_y = |y: f32| y.min(vh - h - MARGIN).max(MARGIN);
    let Some(t) = target else {
        return point(px(clamp_x((vw - w) / 2.)), px(clamp_y((vh - h) / 2.)));
    };
    let (left, top) = (f32::from(t.origin.x), f32::from(t.origin.y));
    let (tw, th) = (f32::from(t.size.width), f32::from(t.size.height));
    let below = top + th + GAP;
    let above = top - GAP - h;
    let (x, y) = if below + h <= vh - MARGIN {
        (left + tw / 2. - w / 2., below)
    } else if above >= MARGIN {
        (left + tw / 2. - w / 2., above)
    } else {
        (left + tw + GAP, top + th / 2. - h / 2.)
    };
    point(px(clamp_x(x)), px(clamp_y(y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn tour_has_the_spec_steps() {
        let tour = Tour::default();
        assert!((5..=8).contains(&tour.steps.len()));
        let targets: Vec<_> = tour.steps.iter().map(|s| s.target).collect();
        for t in [
            TourTarget::ProjectDropdown,
            TourTarget::ActivityBar,
            TourTarget::SearchBox,
            TourTarget::RunButton,
            TourTarget::TerminalPanel,
            TourTarget::StatusBar,
        ] {
            assert!(targets.contains(&t), "{t:?}");
        }
    }

    #[test]
    fn next_and_back_stop_at_the_ends() {
        let mut tour = Tour::default();
        tour.back();
        assert_eq!(tour.step, 0);
        let n = tour.steps.len();
        for _ in 1..n {
            assert!(tour.advance());
        }
        assert!(tour.is_last());
        assert!(!tour.advance());
        assert_eq!(tour.step, n - 1);
    }

    #[test]
    fn callout_goes_below_above_or_beside() {
        let viewport = size(px(1280.), px(800.));
        // Title bar element: below it, centered on it.
        let p = callout_origin(Some(bounds(600., 5., 80., 26.)), viewport);
        assert_eq!((f32::from(p.x), f32::from(p.y)), (480., 43.));
        // Status bar: above it.
        let p = callout_origin(Some(bounds(0., 776., 1280., 24.)), viewport);
        assert_eq!(f32::from(p.y), 776. - GAP - CALLOUT_HEIGHT);
        // Full-height activity bar: to its right, kept on screen.
        let p = callout_origin(Some(bounds(0., 36., 44., 740.)), viewport);
        assert_eq!(f32::from(p.x), 44. + GAP);
        // Near the left edge: clamped into the window.
        let p = callout_origin(Some(bounds(0., 5., 20., 20.)), viewport);
        assert_eq!(f32::from(p.x), MARGIN);
        // No target: centered.
        let p = callout_origin(None, viewport);
        assert_eq!(f32::from(p.x), (1280. - CALLOUT_WIDTH) / 2.);
    }
}
