//! Layout of end-of-line virtual text and code lenses (at the end of a line
//! or on a block row above it), and their click targets.

use arcode_ui_kit::{ThemeColors, theme};
use gpui::{
    Bounds, Context, FontStyle, Hsla, Pixels, ShapedLine, SharedString, UnderlineStyle, Window,
    point, px, size,
};

use crate::Editor;
use crate::display::text_run;
use crate::ext::LensTarget;
use crate::ext::{CodeLens, LensPlacement};
use crate::layout::LayoutSnapshot;

/// A shaped piece of extra text; `x` is relative to the text origin.
pub(crate) struct ExtraItem {
    pub x: Pixels,
    pub shaped: ShapedLine,
    pub target: Option<LensTarget>,
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
    style: &'a crate::EditorStyle,
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
        let mut font = self.style.font.clone();
        if italic {
            font.style = FontStyle::Italic;
        }
        let mut run = text_run(text.len(), &font, &Default::default(), color);
        if underline {
            run.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            });
        }
        let size = (self.style.font_size * 0.9).round();
        window.text_system().shape_line(text, size, &[run], None)
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
                });
                x += w;
            }
            let target: LensTarget = (set.clone(), lens.line, item.id);
            let hovered = self.hovered == Some(&target);
            let color = if hovered { self.colors.accent } else { muted };
            let shaped = self.shape(item.label.clone(), color, false, hovered, window);
            let w = shaped.width;
            out.push(ExtraItem {
                x,
                shaped,
                target: Some(target),
            });
            x += w;
        }
        x + self.gap
    }
}

impl Editor {
    /// Fills end-of-line items, block rows and lens hit boxes.
    pub(crate) fn layout_extras(
        &self,
        snap: &mut LayoutSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ext.has_line_extras() {
            return;
        }
        let colors = theme(cx).colors.clone();
        let shaper = Shaper {
            style: &snap.style,
            colors: &colors,
            hovered: self.ext.hovered_lens.as_ref(),
            gap: snap.char_width * 2.,
        };
        let mut eols = Vec::with_capacity(snap.lines.len());
        let mut rows = Vec::new();
        for v in &snap.lines {
            let mut items = Vec::new();
            let mut x = v.shaped.width + shaper.gap;
            for text in self.ext.eol_text(v.line) {
                let color = text.color.resolve(&colors);
                let shaped = shaper.shape(text.text.clone(), color, text.italic, false, window);
                let w = shaped.width;
                items.push(ExtraItem {
                    x,
                    shaped,
                    target: None,
                });
                x += w + shaper.gap;
            }
            for (set, lens) in self.ext.lenses_on(v.line, LensPlacement::EndOfLine) {
                x = shaper.lens(set, lens, x, &mut items, window);
            }
            eols.push(items);
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
        for (v, items) in snap.lines.iter_mut().zip(eols) {
            v.eol = items;
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
                let full = Bounds::new(point(left + item.x, top), size(item.shaped.width, lh));
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
