//! Single-line text that shrinks to the space it gets and ends in `…`.
//!
//! GPUI 0.2.2's text element caches its first unconstrained measurement,
//! so `text_ellipsis()` never kicks in once a label was measured wide. This
//! element measures the full text once per layout, reports a tiny
//! min-content width so flex parents can shrink it, and truncates to the
//! width it is actually given when painting.
//!
//! The text style is read once, while laying out, and kept for painting:
//! GPUI 0.2.2 paints `deferred` elements (popovers, menus) without the
//! text style of their ancestors, so reading it again in `paint` gave a
//! label in a popover GPUI's default black system font.

use std::ops::Range;

use gpui::{
    App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, HighlightStyle,
    InspectorElementId, IntoElement, LayoutId, Pixels, SharedString, Style, TextRun, TextStyle,
    Window, px, size,
};

/// Where the ellipsis goes when text does not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TruncateMode {
    /// `long_file_na…`
    #[default]
    End,
    /// `…/deep/path/file.rs` (keeps the end, good for paths).
    Start,
    /// `~/projects/…/file.rs`
    Middle,
}

/// The ellipsis character.
pub const ELLIPSIS: &str = "…";

/// Truncates `text` so `measure(result) <= max_width`, placing the
/// ellipsis by `mode`. Returns `text` unchanged when it fits. Uses binary
/// search, so `measure` is called O(log n) times.
pub fn truncate_to_width(
    text: &str,
    max_width: Pixels,
    mode: TruncateMode,
    mut measure: impl FnMut(&str) -> Pixels,
) -> String {
    if measure(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let build = |keep: usize| -> String {
        match mode {
            TruncateMode::End => chars[..keep]
                .iter()
                .copied()
                .chain(ELLIPSIS.chars())
                .collect(),
            TruncateMode::Start => ELLIPSIS
                .chars()
                .chain(chars[chars.len() - keep..].iter().copied())
                .collect(),
            TruncateMode::Middle => {
                let head = keep.div_ceil(2);
                let tail = keep - head;
                chars[..head]
                    .iter()
                    .copied()
                    .chain(ELLIPSIS.chars())
                    .chain(chars[chars.len() - tail..].iter().copied())
                    .collect()
            }
        }
    };
    // Largest `keep` whose result fits; 0 always "fits" (just the ellipsis).
    let (mut lo, mut hi) = (0usize, chars.len().saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if measure(&build(mid)) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    build(lo)
}

/// A one-line label that truncates with an ellipsis to its laid-out width.
/// Uses the inherited text style (font, size, color) as it is at layout.
pub struct Truncated {
    text: SharedString,
    mode: TruncateMode,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

/// A label truncated at the end (`name…`).
pub fn truncated(text: impl Into<SharedString>) -> Truncated {
    Truncated {
        text: text.into(),
        mode: TruncateMode::End,
        highlights: Vec::new(),
    }
}

impl Truncated {
    /// Sets where the ellipsis goes.
    pub fn mode(mut self, mode: TruncateMode) -> Self {
        self.mode = mode;
        self
    }

    /// Keeps the end of the text (`…/path/file.rs`).
    pub fn start(self) -> Self {
        self.mode(TruncateMode::Start)
    }

    /// Puts the ellipsis in the middle.
    pub fn middle(self) -> Self {
        self.mode(TruncateMode::Middle)
    }

    /// Styles byte ranges of the text (sorted, not overlapping), like
    /// `StyledText::with_highlights`: syntax-coloured values. Kept on the
    /// part left before an end ellipsis; a start or middle ellipsis drops
    /// them when the text is cut.
    pub fn highlights(mut self, highlights: Vec<(Range<usize>, HighlightStyle)>) -> Self {
        self.highlights = highlights;
        self
    }
}

/// The text runs for `text` in `style` with `highlights` applied to the
/// first `kept` bytes (the rest, e.g. an ellipsis, in the plain style).
fn runs_for(
    text: &str,
    kept: usize,
    style: &TextStyle,
    highlights: &[(Range<usize>, HighlightStyle)],
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut ix = 0;
    for (range, highlight) in highlights {
        let (start, end) = (range.start.max(ix), range.end.min(kept));
        if start >= end {
            continue;
        }
        if ix < start {
            runs.push(style.to_run(start - ix));
        }
        runs.push(style.clone().highlight(*highlight).to_run(end - start));
        ix = end;
    }
    if ix < text.len() {
        runs.push(style.to_run(text.len() - ix));
    }
    runs
}

fn measure_text(text: &str, style: &TextStyle, window: &mut Window) -> Pixels {
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = style.to_run(text.len());
    window
        .text_system()
        .shape_line(
            SharedString::from(text.to_string()),
            font_size,
            &[run],
            None,
        )
        .width
}

impl IntoElement for Truncated {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The text style a label had at layout, kept for painting.
pub struct LabelStyle {
    style: TextStyle,
    line_height: Pixels,
}

impl Element for Truncated {
    type RequestLayoutState = LabelStyle;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, LabelStyle) {
        let style = window.text_style();
        let full = measure_text(&self.text, &style, window);
        let ellipsis = measure_text(ELLIPSIS, &style, window);
        let line_height = window.line_height();
        let mut layout_style = Style {
            flex_shrink: 1.,
            ..Default::default()
        };
        layout_style.min_size.width = px(0.).into();
        let layout = window.request_measured_layout(layout_style, move |known, available, _, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(w) => full.min(w),
                AvailableSpace::MinContent => ellipsis,
                AvailableSpace::MaxContent => full,
            });
            size(width, known.height.unwrap_or(line_height))
        });
        (layout, LabelStyle { style, line_height })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut LabelStyle,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        label: &mut LabelStyle,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // Layout rounds both edges to device pixels, which can make the
        // bounds up to one device pixel narrower than the measured text:
        // that much slack keeps a label that was given its full width from
        // being cut (`topic` drawn as `to…`).
        let max = bounds.size.width + px(1.0 / window.scale_factor().max(1.0)) + px(0.01);
        let style = &label.style;
        let text = truncate_to_width(&self.text, max, self.mode, |t| {
            measure_text(t, style, window)
        });
        let font_size = style.font_size.to_pixels(window.rem_size());
        // Highlights hold on the untouched text, or on the head an end
        // ellipsis keeps.
        let kept = if text.len() == self.text.len() {
            text.len()
        } else if self.mode == TruncateMode::End {
            text.len() - ELLIPSIS.len()
        } else {
            0
        };
        let runs = runs_for(&text, kept, style, &self.highlights);
        #[cfg(any(debug_assertions, feature = "test-support"))]
        paint_log::record(&text, &runs, bounds);
        let line =
            window
                .text_system()
                .shape_line(SharedString::from(text), font_size, &runs, None);
        let line_height = label.line_height;
        let y = bounds.top() + (bounds.size.height - line_height) * 0.5;
        if let Err(err) = line.paint(gpui::point(bounds.left(), y), line_height, window, cx) {
            log::debug!("failed to paint truncated label: {err}");
        }
    }
}

/// What [`Truncated`] labels painted, for tests that check the colours
/// people actually see. Debug builds and the `test-support` feature only
/// (which the `arcode` crate's tests turn on, so they also run in release
/// builds), and off until a test calls
/// [`paint_log::start`], so drawing pays one thread-local read per label.
#[cfg(any(debug_assertions, feature = "test-support"))]
pub mod paint_log {
    use std::cell::{Cell, RefCell};

    use gpui::{Bounds, Hsla, Pixels, TextRun};

    /// One painted label.
    #[derive(Debug, Clone)]
    pub struct PaintedLabel {
        /// The text drawn (with an ellipsis when cut).
        pub text: String,
        /// The colour of its first run.
        pub color: Hsla,
        /// Where it was drawn, in window coordinates.
        pub bounds: Bounds<Pixels>,
    }

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static LOG: RefCell<Vec<PaintedLabel>> = const { RefCell::new(Vec::new()) };
    }

    /// Starts recording on this thread (GPUI tests draw on it), dropping
    /// anything recorded before.
    pub fn start() {
        ON.with(|on| on.set(true));
        take();
    }

    pub(super) fn record(text: &str, runs: &[TextRun], bounds: Bounds<Pixels>) {
        if !ON.with(Cell::get) {
            return;
        }
        let Some(run) = runs.first() else { return };
        LOG.with(|log| {
            log.borrow_mut().push(PaintedLabel {
                text: text.to_string(),
                color: run.color,
                bounds,
            })
        });
    }

    /// The labels painted on this thread since the last call.
    pub fn take() -> Vec<PaintedLabel> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every char is 10px wide.
    fn w(s: &str) -> Pixels {
        px(10. * s.chars().count() as f32)
    }

    #[test]
    fn fits_unchanged() {
        assert_eq!(
            truncate_to_width("abc", px(30.), TruncateMode::End, w),
            "abc"
        );
    }

    #[test]
    fn end_start_middle() {
        let t = "abcdefghij"; // 100px
        assert_eq!(truncate_to_width(t, px(50.), TruncateMode::End, w), "abcd…");
        assert_eq!(
            truncate_to_width(t, px(50.), TruncateMode::Start, w),
            "…ghij"
        );
        assert_eq!(
            truncate_to_width(t, px(50.), TruncateMode::Middle, w),
            "ab…ij"
        );
        assert_eq!(
            truncate_to_width(t, px(60.), TruncateMode::Middle, w),
            "abc…ij"
        );
    }

    #[test]
    fn tiny_width_keeps_ellipsis_only() {
        assert_eq!(
            truncate_to_width("abcdef", px(5.), TruncateMode::End, w),
            "…"
        );
        assert_eq!(truncate_to_width("", px(0.), TruncateMode::End, w), "");
    }

    #[test]
    fn highlights_stop_at_the_ellipsis() {
        let style = TextStyle::default();
        let bold = HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..Default::default()
        };
        let lens = |runs: Vec<TextRun>| runs.iter().map(|r| r.len).collect::<Vec<_>>();
        // `ab` plain, `cdef` bold, `gh` plain; uncut.
        let marks = vec![(2..6, bold)];
        assert_eq!(lens(runs_for("abcdefgh", 8, &style, &marks)), [2, 4, 2]);
        // Cut to `abcd…`: the bold run ends at the cut, the ellipsis is plain.
        let cut = format!("abcd{ELLIPSIS}");
        let runs = runs_for(&cut, 4, &style, &marks);
        assert_eq!(lens(runs.clone()), [2, 2, ELLIPSIS.len()]);
        assert_eq!(runs[1].font.weight, gpui::FontWeight::BOLD);
        assert_eq!(runs[2].font.weight, style.font_weight);
        // Nothing kept: one plain run.
        assert_eq!(lens(runs_for(&cut, 0, &style, &marks)), [cut.len()]);
    }

    #[test]
    fn multibyte_text() {
        let t = "漢字漢字漢字";
        assert_eq!(truncate_to_width(t, px(30.), TruncateMode::End, w), "漢字…");
    }
}
