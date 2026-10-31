//! Inline lenses after the end of a line: the diagnostic lens
//! (`editor.inline_diagnostics`) and the debugger's paused lens with its
//! inline values. Both are UI-font pills, so they never read as code.

use arcode_ui_kit::{IconName, ThemeColors};
use gpui::{Font, FontStyle, Hsla, Pixels, ShapedLine, SharedString, Window, px};

use crate::diagnostics::EditorDiagnostic;
use crate::display::text_run;
use crate::ext::PausedLine;
use crate::layout_extras::ExtraItem;

/// Space between the end of the text and a lens.
pub(crate) const LENS_GAP: Pixels = px(24.);
/// Lens pill: height, corner radius, horizontal padding, gap between the
/// icon and the text parts.
pub(crate) const PILL_H: Pixels = px(18.);
pub(crate) const PILL_RADIUS: Pixels = px(5.);
const PILL_PAD: Pixels = px(7.);
const PILL_GAP: Pixels = px(6.);
/// Space between the paused lens and the inline values.
const VALUES_GAP: Pixels = px(12.);
/// Space kept free at the right edge of the text area.
const EDGE_ROOM: Pixels = px(8.);
/// Least room for the message of a diagnostic lens; with less the lens is
/// left out (the gutter dot marks the line instead) rather than clipped.
const MIN_MESSAGE: Pixels = px(40.);

/// A lens pill behind text: x and width relative to the text origin.
pub(crate) struct LensPill {
    pub x: Pixels,
    pub width: Pixels,
    pub fill: Hsla,
    pub border: Hsla,
    /// Icon, its color and size, and its x relative to the text origin.
    pub icon: Option<(IconName, Hsla, Pixels, Pixels)>,
}

/// Horizontal layout of a pill starting at `x`: the x of each text part and
/// the pill width, for an icon of `icon` px (zero for none) and parts of
/// `widths`.
pub(crate) fn pill_layout(x: Pixels, icon: Pixels, widths: &[Pixels]) -> (Vec<Pixels>, Pixels) {
    let mut cursor = x + PILL_PAD;
    if icon > Pixels::ZERO {
        cursor += icon + PILL_GAP;
    }
    let mut xs = Vec::with_capacity(widths.len());
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            cursor += PILL_GAP;
        }
        xs.push(cursor);
        cursor += *w;
    }
    (xs, cursor + PILL_PAD - x)
}

/// Shapes lens text in the UI font.
pub(crate) struct LensShaper {
    pub font: Font,
    pub size: Pixels,
}

impl LensShaper {
    pub fn shape(
        &self,
        text: SharedString,
        color: Hsla,
        italic: bool,
        window: &Window,
    ) -> ShapedLine {
        let mut font = self.font.clone();
        if italic {
            font.style = FontStyle::Italic;
        }
        let run = text_run(text.len(), &font, &Default::default(), color);
        window
            .text_system()
            .shape_line(text, self.size, &[run], None)
    }

    /// Shapes `text`, cut with an ellipsis to fit `max` (when it can).
    fn shape_fit(
        &self,
        text: &str,
        color: Hsla,
        italic: bool,
        max: Pixels,
        window: &Window,
    ) -> ShapedLine {
        let shaped = self.shape(SharedString::from(text.to_string()), color, italic, window);
        if shaped.width <= max {
            return shaped;
        }
        let ellipsis = self.shape("…".into(), color, italic, window).width;
        let cut = shaped
            .index_for_x((max - ellipsis).max(Pixels::ZERO))
            .unwrap_or(0);
        let mut short = text[..cut].trim_end().to_string();
        short.push('…');
        self.shape(short.into(), color, italic, window)
    }
}

/// Room for the text parts of a diagnostic lens at `x` with an icon of
/// `icon` px, so the pill ends [`EDGE_ROOM`] before `right`; `None` when
/// that is too little for a readable message.
fn lens_room(x: Pixels, right: Pixels, icon: Pixels) -> Option<Pixels> {
    let room = right - EDGE_ROOM - x - (PILL_PAD * 2. + icon + PILL_GAP);
    (room >= MIN_MESSAGE).then_some(room)
}

/// Lays out the diagnostic lens of `diag` at `x` (text-origin relative);
/// `right` is where the visible text area ends. Returns the x after it, or
/// `None` (and lays out nothing) when the line leaves no room for it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn diagnostic_lens(
    diag: &EditorDiagnostic,
    x: Pixels,
    right: Pixels,
    shaper: &LensShaper,
    colors: &ThemeColors,
    window: &Window,
    pills: &mut Vec<LensPill>,
    items: &mut Vec<ExtraItem>,
) -> Option<Pixels> {
    let color = diag.severity.color(colors);
    let icon = px(12.);
    let (message, meta) = diag.lens_text();
    let room = lens_room(x, right, icon)?;
    let message = shaper.shape_fit(message, color, false, room, window);
    // The source and code go first when space is short, then the message
    // is cut with an ellipsis.
    let meta = (!meta.is_empty())
        .then(|| shaper.shape(meta.into(), colors.text_muted, false, window))
        .filter(|m| message.width + PILL_GAP + m.width <= room);
    let mut shaped = vec![message];
    shaped.extend(meta);
    let widths: Vec<Pixels> = shaped.iter().map(|s| s.width).collect();
    let (xs, width) = pill_layout(x, icon, &widths);
    pills.push(LensPill {
        x,
        width,
        fill: colors.panel_raised(),
        border: colors.border,
        icon: Some((diag.severity.icon(), color, icon, x + PILL_PAD)),
    });
    for (shaped, x) in shaped.into_iter().zip(xs) {
        items.push(ExtraItem {
            x,
            shaped,
            target: None,
        });
    }
    Some(x + width)
}

/// Lays out the paused lens and the inline values at `x`; `right` is where
/// the visible text area ends, and the values are cut with an ellipsis
/// there. Returns the x after them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paused_lens(
    paused: &PausedLine,
    x: Pixels,
    right: Pixels,
    shaper: &LensShaper,
    colors: &ThemeColors,
    window: &Window,
    pills: &mut Vec<LensPill>,
    items: &mut Vec<ExtraItem>,
) -> Pixels {
    let icon = px(11.);
    let label = shaper.shape(paused.label.clone(), colors.text, false, window);
    let (xs, width) = pill_layout(x, icon, &[label.width]);
    pills.push(LensPill {
        x,
        width,
        fill: colors.panel(),
        border: colors.border_strong,
        icon: Some((IconName::Pause, colors.accent, icon, x + PILL_PAD)),
    });
    items.push(ExtraItem {
        x: xs[0],
        shaped: label,
        target: None,
    });
    let mut end = x + width;
    if !paused.values.is_empty() {
        let text = paused
            .values
            .iter()
            .map(|v| v.as_ref())
            .collect::<Vec<_>>()
            .join("  ·  ");
        let vx = end + VALUES_GAP;
        let room = right - EDGE_ROOM - vx;
        if room >= MIN_MESSAGE {
            let shaped = shaper.shape_fit(&text, colors.text_muted, true, room, window);
            end = vx + shaped.width;
            items.push(ExtraItem {
                x: vx,
                shaped,
                target: None,
            });
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pills_pad_and_space_their_parts() {
        // Icon 12, message 100, meta 40: 7 + 12 + 6 | 100 | 6 | 40 | 7.
        let (xs, width) = pill_layout(px(10.), px(12.), &[px(100.), px(40.)]);
        assert_eq!(xs, vec![px(35.), px(141.)]);
        assert_eq!(width, px(178.));
        // No icon: just the padding.
        let (xs, width) = pill_layout(px(0.), Pixels::ZERO, &[px(50.)]);
        assert_eq!(xs, vec![px(7.)]);
        assert_eq!(width, px(64.));
    }

    #[test]
    fn lens_needs_room_for_a_message_before_the_edge() {
        // 8 edge room + 7 + 12 + 6 + 7 pill chrome = 40 px around the text.
        assert_eq!(lens_room(px(100.), px(300.), px(12.)), Some(px(160.)));
        assert_eq!(lens_room(px(100.), px(180.), px(12.)), Some(px(40.)));
        // Less than a readable message: no lens, never a clipped sliver.
        assert_eq!(lens_room(px(100.), px(179.), px(12.)), None);
        // A line that runs past the edge.
        assert_eq!(lens_room(px(400.), px(300.), px(12.)), None);
    }
}
