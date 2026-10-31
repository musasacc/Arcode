//! Layout of end-of-line virtual text, inline lenses (diagnostics, the
//! paused line) and code lenses (at the end of a line or on a block row
//! above it), and their click targets. All of it is set in the UI font at
//! the meta size (Inter 12), so it never reads as code.
//!
//! End-of-line text (blame) ends with an ellipsis at the pane edge. A code
//! lens label starting with `▶` (a plugin's "▶ Send request") gets the
//! Lucide `play` icon in `success` instead of the glyph (DESIGN 6.7 Run).

use arcode_ui_kit::{IconName, ThemeColors, settings, theme, type_scale, ui_font_family};
use gpui::{
    Bounds, Context, Font, FontStyle, FontWeight, Hsla, Pixels, ShapedLine, SharedString,
    UnderlineStyle, Window, point, px, size,
};

use crate::Editor;
use crate::diagnostics::lens_diagnostics;
use crate::display::text_run;
use crate::ext::LensTarget;
use crate::ext::{CodeLens, LensPlacement};
use crate::inline_lens::{
    EDGE_ROOM, LENS_GAP, LensShaper, MIN_MESSAGE, diagnostic_lens, paused_lens,
};
use crate::layout::LayoutSnapshot;

/// A shaped piece of extra text; `x` is relative to the text origin.
pub(crate) struct ExtraItem {
    pub x: Pixels,
    pub shaped: ShapedLine,
    pub target: Option<LensTarget>,
    /// An icon before the text: icon, color, size and x (text-origin
    /// relative).
    pub icon: Option<(IconName, Hsla, Pixels, Pixels)>,
}

/// Size of the play icon of a lens label, and the space after it.
const LENS_ICON: Pixels = px(12.);
const LENS_ICON_GAP: Pixels = px(4.);

/// The rest of a lens label that starts with a play glyph (`▶ Send
/// request` gives `Send request`); `None` for other labels.
pub(crate) fn play_label(label: &str) -> Option<&str> {
    let rest = label
        .strip_prefix('▶')
        .or_else(|| label.strip_prefix('▷'))?;
    let rest = rest.strip_prefix('\u{fe0f}').unwrap_or(rest).trim_start();
    (!rest.is_empty()).then_some(rest)
}

/// Room for end-of-line text at `x` before the pane edge at `right`;
/// `None` when too little is left for anything readable.
pub(crate) fn eol_room(x: Pixels, right: Pixels) -> Option<Pixels> {
    let room = right - EDGE_ROOM - x;
    (room >= MIN_MESSAGE).then_some(room)
}

/// A block row drawn above `line`.
pub(crate) struct ExtraRow {
    pub line: usize,
    pub items: Vec<ExtraItem>,
}

/// A clickable lens item in window coordinates.
#[derive(Clone)]
pub(crate) struct LensHit {
    pub bounds: Bounds<Pixels>,
    pub target: LensTarget,
}

struct Shaper<'a> {
    lens: LensShaper,
    colors: &'a ThemeColors,
    hovered: Option<&'a LensTarget>,
    gap: Pixels,
}

impl Shaper<'_> {
    fn shape(
        &self,
        text: SharedString,
        color: Hsla,
        italic: bool,
        underline: bool,
        window: &mut Window,
    ) -> ShapedLine {
        if !underline {
            return self.lens.shape(text, color, italic, window);
        }
        let mut run = text_run(text.len(), &self.lens.font, &Default::default(), color);
        run.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        });
        window
            .text_system()
            .shape_line(text, self.lens.size, &[run], None)
    }

    /// Shapes `lens` starting at `x`; returns the x after it.
    fn lens(
        &self,
        set: &SharedString,
        lens: &CodeLens,
        mut x: Pixels,
        out: &mut Vec<ExtraItem>,
        window: &mut Window,
    ) -> Pixels {
        let muted = self.colors.text_muted;
        for (i, item) in lens.items.iter().enumerate() {
            if i > 0 {
                let sep = self.shape(" | ".into(), muted, false, false, window);
                let w = sep.width;
                out.push(ExtraItem {
                    x,
                    shaped: sep,
                    target: None,
                    icon: None,
                });
                x += w;
            }
            let target: LensTarget = (set.clone(), lens.line, item.id);
            let hovered = self.hovered == Some(&target);
            let color = if hovered { self.colors.accent } else { muted };
            let (label, icon) = match play_label(&item.label) {
                Some(rest) => {
                    let icon = (IconName::Play, self.colors.success, LENS_ICON, x);
                    x += LENS_ICON + LENS_ICON_GAP;
                    (SharedString::from(rest.to_string()), Some(icon))
                }
                None => (item.label.clone(), None),
            };
            let shaped = self.shape(label, color, false, hovered, window);
            let w = shaped.width;
            out.push(ExtraItem {
                x,
                shaped,
                target: Some(target),
                icon,
            });
            x += w;
        }
        x + self.gap
    }
}

impl Editor {
    /// Fills end-of-line items, lens pills, block rows and lens hit boxes.
    pub(crate) fn layout_extras(
        &self,
        snap: &mut LayoutSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = settings(cx).editor.inline_diagnostics;
        let lensed = lens_diagnostics(&self.diagnostics, snap.visible_range(), mode);
        let paused = self.ext.paused.as_ref();
        if !self.ext.has_line_extras() && paused.is_none() && lensed.iter().all(Option::is_none) {
            return;
        }
        let colors = theme(cx).colors.clone();
        let font = Font {
            family: ui_font_family(cx),
            features: Default::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        let shaper = Shaper {
            lens: LensShaper {
                font,
                size: type_scale(cx).meta,
            },
            colors: &colors,
            hovered: self.ext.hovered_lens.as_ref(),
            gap: px(16.),
        };
        // Right edge of the visible text area, relative to the text origin.
        let right = snap.scroll_x + snap.text_bounds.size.width;
        let mut eols = Vec::with_capacity(snap.lines.len());
        let mut rows = Vec::new();
        for (v, diag) in snap.lines.iter().zip(lensed) {
            let mut items = Vec::new();
            let mut pills = Vec::new();
            let mut x = v.shaped.width + LENS_GAP;
            let lens = &shaper.lens;
            // The paused lens takes the place of a diagnostic lens; the
            // gutter dot then marks the diagnostic.
            let mut diag_lens = false;
            if let Some(p) = paused.filter(|p| p.line == v.line) {
                x = paused_lens(p, x, right, lens, &colors, window, &mut pills, &mut items)
                    + shaper.gap;
            } else if let Some(diag) = diag
                && let Some(end) = diagnostic_lens(
                    diag, x, right, lens, &colors, window, &mut pills, &mut items,
                )
            {
                x = end + shaper.gap;
                diag_lens = true;
            }
            // Blame and other end-of-line text: cut with an ellipsis at
            // the pane edge, left out where nothing readable fits.
            for text in self.ext.eol_text(v.line) {
                let Some(room) = eol_room(x, right) else {
                    break;
                };
                let color = text.color.resolve(&colors);
                let shaped = lens.shape_fit(&text.text, color, text.italic, room, window);
                let w = shaped.width;
                items.push(ExtraItem {
                    x,
                    shaped,
                    target: None,
                    icon: None,
                });
                x += w + shaper.gap;
            }
            for (set, lens) in self.ext.lenses_on(v.line, LensPlacement::EndOfLine) {
                x = shaper.lens(set, lens, x, &mut items, window);
            }
            eols.push((items, pills, diag_lens));
            if snap.blocks.has_block(v.line) {
                let indent = self
                    .buffer
                    .line(v.line)
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .count();
                let mut x = v.shaped.x_for_index(v.display.byte_for_col(indent));
                let mut items = Vec::new();
                for (set, lens) in self.ext.lenses_on(v.line, LensPlacement::Above) {
                    x = shaper.lens(set, lens, x, &mut items, window);
                }
                rows.push(ExtraRow {
                    line: v.line,
                    items,
                });
            }
        }
        for (v, (items, pills, diag_lens)) in snap.lines.iter_mut().zip(eols) {
            v.eol = items;
            v.pills = pills;
            v.diag_lens = diag_lens;
        }
        snap.block_rows = rows;
        snap.lens_hits = lens_hits(snap);
    }
}

fn lens_hits(snap: &LayoutSnapshot) -> Vec<LensHit> {
    let lh = snap.line_height;
    let left = snap.text_left();
    let clip = snap.text_bounds;
    let mut hits = Vec::new();
    // Lens text is painted under the text area's content mask, so a hit
    // box only covers the part of an item that is actually visible: an
    // item scrolled under the gutter or past the scrollbar must not take
    // clicks meant for them.
    let mut push = |items: &[ExtraItem], top: Pixels| {
        for item in items {
            if let Some(target) = &item.target {
                // The icon before a label takes clicks too.
                let start = item.icon.map_or(item.x, |(_, _, _, x)| x);
                let width = item.x + item.shaped.width - start;
                let full = Bounds::new(point(left + start, top), size(width, lh));
                let bounds = full.intersect(&clip);
                if bounds.size.width > Pixels::ZERO && bounds.size.height > Pixels::ZERO {
                    hits.push(LensHit {
                        bounds,
                        target: target.clone(),
                    });
                }
            }
        }
    };
    for v in &snap.lines {
        push(&v.eol, snap.line_top(v.line));
    }
    for row in &snap.block_rows {
        push(&row.items, snap.line_top(row.line) - lh);
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_glyphs_become_an_icon() {
        assert_eq!(play_label("▶ Send request"), Some("Send request"));
        assert_eq!(play_label("▷Run"), Some("Run"));
        assert_eq!(play_label("▶\u{fe0f} Run"), Some("Run"));
        assert_eq!(play_label("Send request"), None);
        assert_eq!(play_label("Accept ▶"), None);
        // A lone glyph keeps its text (there would be no label left).
        assert_eq!(play_label("▶ "), None);
    }

    #[test]
    fn end_of_line_text_needs_room_before_the_edge() {
        assert_eq!(eol_room(px(100.), px(300.)), Some(px(192.)));
        assert_eq!(eol_room(px(100.), px(148.)), Some(px(40.)));
        assert_eq!(eol_room(px(100.), px(147.)), None);
        assert_eq!(eol_room(px(500.), px(300.)), None);
    }
}
