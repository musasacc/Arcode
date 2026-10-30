//! Per-glyph font fallback for text runs.
//!
//! GPUI 0.2.2's Linux text system ignores `Font::fallbacks` and leaves
//! fallback to cosmic-text, which only considers faces with the run's exact
//! style. Italic comments in JetBrains Mono therefore show CJK as boxes,
//! because no CJK font ships an italic face. [`apply_glyph_fallbacks`]
//! splits runs so chars the run's font lacks are drawn with the first font
//! from the user's fallback list that has them, or else with an upright
//! style so the system fallback can find a face. ASCII-only text is
//! returned untouched without any lookups.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use gpui::{Font, FontFeatures, FontId, FontStyle, SharedString, TextRun, TextSystem, px};

#[derive(Default)]
struct Cache {
    /// Family names known to the text system (filled on first need).
    families: Option<HashSet<String>>,
    /// Resolved fonts, `None` when the family is not installed.
    fonts: HashMap<Font, Option<FontId>>,
    /// Whether a font has a glyph for a char.
    glyphs: HashMap<(FontId, char), bool>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
}

/// Families that must not be resolved through GPUI: its Linux backend
/// drops any face without an `m` glyph from the font database, which would
/// break cosmic-text's own emoji fallback. Emoji keep using that path.
fn unsafe_to_resolve(family: &str) -> bool {
    family.to_ascii_lowercase().contains("emoji")
}

impl Cache {
    fn font_id(&mut self, font: &Font, ts: &TextSystem) -> Option<FontId> {
        if let Some(id) = self.fonts.get(font) {
            return *id;
        }
        let families = self
            .families
            .get_or_insert_with(|| ts.all_font_names().into_iter().collect());
        let id = (families.contains(font.family.as_ref()) && !unsafe_to_resolve(&font.family))
            .then(|| ts.resolve_font(font));
        self.fonts.insert(font.clone(), id);
        id
    }

    fn has_glyph(&mut self, id: FontId, c: char, ts: &TextSystem) -> bool {
        *self
            .glyphs
            .entry((id, c))
            .or_insert_with(|| ts.advance(id, px(16.), c).is_ok())
    }

    /// The font to draw `c` with when `font` lacks it, or `None` to keep it.
    fn replacement(
        &mut self,
        font: &Font,
        c: char,
        fallbacks: &[SharedString],
        ts: &TextSystem,
    ) -> Option<Font> {
        let primary = self.font_id(font, ts)?;
        if self.has_glyph(primary, c, ts) {
            return None;
        }
        for family in fallbacks {
            let candidate = Font {
                family: family.clone(),
                features: FontFeatures::default(),
                fallbacks: None,
                weight: font.weight,
                style: FontStyle::Normal,
            };
            if let Some(id) = self.font_id(&candidate, ts)
                && self.has_glyph(id, c, ts)
            {
                return Some(candidate);
            }
        }
        // No configured fallback has it: an upright style lets the
        // platform's own fallback consider every installed face.
        (font.style != FontStyle::Normal).then(|| Font {
            style: FontStyle::Normal,
            ..font.clone()
        })
    }
}

/// Splits `runs` (covering `text` exactly) so that chars missing from their
/// run's font use a fallback font. Returns `None` when nothing changes, which
/// is always the case for ASCII text.
pub fn apply_glyph_fallbacks(
    text: &str,
    runs: &[TextRun],
    fallbacks: &[SharedString],
    ts: &TextSystem,
) -> Option<Vec<TextRun>> {
    if text.is_ascii() {
        return None;
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let mut out: Vec<TextRun> = Vec::with_capacity(runs.len() + 2);
        let mut changed = false;
        let mut offset = 0;
        for run in runs {
            let end = (offset + run.len).min(text.len());
            let slice = text.get(offset..end).unwrap_or_default();
            if slice.is_ascii() {
                push_run(&mut out, run.clone());
                offset = end;
                continue;
            }
            for c in slice.chars() {
                let font = if c.is_ascii() || c.is_whitespace() || is_emoji(c) {
                    None
                } else {
                    cache.replacement(&run.font, c, fallbacks, ts)
                };
                changed |= font.is_some();
                let piece = TextRun {
                    len: c.len_utf8(),
                    font: font.unwrap_or_else(|| run.font.clone()),
                    ..run.clone()
                };
                push_run(&mut out, piece);
            }
            offset = end;
        }
        if !changed {
            return None;
        }
        keep_font_runs_distinct(&mut out);
        Some(out)
    })
}

/// Emoji are left to cosmic-text, which always considers color emoji
/// fonts regardless of the run's style.
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE00..=0xFE0F | 0x200D | 0xE0020..=0xE007F
    )
}

/// GPUI 0.2.2's `layout_line` merges a run into the previous one when its
/// color and decorations are equal *without comparing fonts*, so a fallback
/// run with the same color as its neighbour would be drawn in the
/// neighbour's font. Nudging the alpha by an invisible amount makes GPUI
/// resolve each run's own font.
pub fn keep_font_runs_distinct(runs: &mut [TextRun]) {
    for i in 1..runs.len() {
        let (before, after) = runs.split_at_mut(i);
        let (prev, run) = (&before[i - 1], &mut after[0]);
        if prev.font != run.font
            && prev.color == run.color
            && prev.underline == run.underline
            && prev.strikethrough == run.strikethrough
        {
            let step = 1.0 / 512.0;
            run.color.a = if run.color.a > step {
                run.color.a - step
            } else {
                run.color.a + step
            };
        }
    }
}

/// Appends `run`, merging it into the previous run when they look the same.
fn push_run(out: &mut Vec<TextRun>, run: TextRun) {
    if let Some(last) = out.last_mut()
        && last.font == run.font
        && last.color == run.color
        && last.background_color == run.background_color
        && last.underline == run.underline
        && last.strikethrough == run.strikethrough
    {
        last.len += run.len;
        return;
    }
    out.push(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: usize, family: &'static str) -> TextRun {
        TextRun {
            len,
            font: gpui::font(family),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn merges_equal_runs() {
        let mut out = Vec::new();
        push_run(&mut out, run(2, "A"));
        push_run(&mut out, run(3, "A"));
        push_run(&mut out, run(1, "B"));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len, 5);
    }

    #[test]
    fn runs_with_different_fonts_get_distinct_colors() {
        let mut runs = vec![run(2, "A"), run(3, "B"), run(1, "B")];
        keep_font_runs_distinct(&mut runs);
        assert_ne!(runs[0].color, runs[1].color);
        assert!((runs[0].color.a - runs[1].color.a).abs() < 0.01);
        // Same font as its neighbour: left as is (colors already differ).
        assert_eq!(runs[2].color, runs[0].color);
    }

    #[test]
    fn emoji_families_are_never_resolved() {
        assert!(unsafe_to_resolve("Noto Color Emoji"));
        assert!(!unsafe_to_resolve("Noto Sans CJK SC"));
        assert!(is_emoji('😀') && is_emoji('🚀') && !is_emoji('漢'));
    }
}
