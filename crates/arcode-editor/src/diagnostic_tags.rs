//! Unused and deprecated code, as language servers tag it on diagnostics.
//!
//! Code a server marks unnecessary (unused imports, variables and
//! functions, unreachable code) is drawn faded in its own syntax colour;
//! deprecated code is struck through. Both are styles on the text runs of
//! the visible lines only: a line is shaped once with them and is then
//! served from GPUI's line cache like any other.

use std::ops::Range;

use gpui::{Hsla, StrikethroughStyle, TextRun, px};

use crate::diagnostics::{DiagnosticSeverity, EditorDiagnostic};
use crate::display::DisplayLine;

/// Opacity of unnecessary (unused) code. No theme key describes it, so it
/// is the same in every theme; the syntax colour stays recognisable.
pub const UNNECESSARY_OPACITY: f32 = 0.55;

/// Tags a language server put on a diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DiagnosticTags {
    /// Unused or unreachable code: drawn faded (`editor.fade_unused`).
    pub unnecessary: bool,
    /// Deprecated code: drawn struck through.
    pub deprecated: bool,
}

impl DiagnosticTags {
    /// Whether any tag is set.
    pub fn any(self) -> bool {
        self.unnecessary || self.deprecated
    }
}

impl EditorDiagnostic {
    /// Whether the fade is all this diagnostic shows: a hint that only
    /// marks unused code gets no gutter dot and no lens, just the faded
    /// text (and its message on hover). With fading off it is a plain hint.
    pub(crate) fn fade_only(&self, fade_unused: bool) -> bool {
        fade_unused && self.tags.unnecessary && self.severity == DiagnosticSeverity::Hint
    }
}

/// How a column range of one line is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagSpan {
    /// Char columns of the line.
    pub cols: Range<usize>,
    /// Draw the text faded.
    pub fade: bool,
    /// Strike the text through.
    pub strike: bool,
}

/// The tagged diagnostics of the lines one layout pass shapes, with what
/// they draw under `editor.fade_unused`.
pub(crate) struct LineTags<'a> {
    tagged: Vec<&'a EditorDiagnostic>,
    fade_unused: bool,
}

impl<'a> LineTags<'a> {
    /// Collects the diagnostics of `diagnostics` that style any of `lines`.
    pub(crate) fn new(
        diagnostics: &'a [EditorDiagnostic],
        lines: Range<usize>,
        fade_unused: bool,
    ) -> Self {
        let tagged = tagged_in(diagnostics, lines, fade_unused);
        Self {
            tagged,
            fade_unused,
        }
    }

    /// Fades and strikes the tagged parts of `line` (`line_len` chars),
    /// shown as `display`.
    pub(crate) fn restyle(
        &self,
        display: &mut DisplayLine,
        line: usize,
        line_len: usize,
        strike_color: Hsla,
    ) {
        if self.tagged.is_empty() {
            return;
        }
        let spans = line_spans(&self.tagged, line, line_len, self.fade_unused);
        restyle(display, &spans, strike_color);
    }
}

/// The tagged diagnostics that touch `lines` and draw something with
/// `fade_unused`; computed once per layout, so each line only looks at
/// these few.
pub(crate) fn tagged_in(
    diagnostics: &[EditorDiagnostic],
    lines: Range<usize>,
    fade_unused: bool,
) -> Vec<&EditorDiagnostic> {
    diagnostics
        .iter()
        .filter(|d| (fade_unused && d.tags.unnecessary) || d.tags.deprecated)
        .filter(|d| d.range.start.line < lines.end && d.range.end.line >= lines.start)
        .filter(|d| !d.range.is_empty())
        .collect()
}

/// The faded and struck column ranges of `line` (`line_len` chars).
pub(crate) fn line_spans(
    tagged: &[&EditorDiagnostic],
    line: usize,
    line_len: usize,
    fade_unused: bool,
) -> Vec<TagSpan> {
    tagged
        .iter()
        .filter(|d| d.range.start.line <= line && line <= d.range.end.line)
        .filter_map(|d| {
            let (s, e) = (d.range.start, d.range.end);
            let a = if line == s.line {
                s.column.min(line_len)
            } else {
                0
            };
            let b = if line == e.line {
                e.column.min(line_len)
            } else {
                line_len
            };
            (a < b).then_some(TagSpan {
                cols: a..b,
                fade: fade_unused && d.tags.unnecessary,
                strike: d.tags.deprecated,
            })
        })
        .collect()
}

/// Fades and strikes the runs of `display` covered by `spans`. Inlay text
/// (type and parameter hints) inside a span keeps its own look.
/// `strike_color` is the colour of the strikethrough on unfaded text.
pub(crate) fn restyle(display: &mut DisplayLine, spans: &[TagSpan], strike_color: Hsla) {
    if spans.is_empty() {
        return;
    }
    // Display byte ranges of each span, without the inlay text inside it.
    let mut ranges: Vec<(Range<usize>, bool, bool)> = Vec::new();
    for span in spans {
        let start = display.char_start(span.cols.start);
        let end = display.byte_for_col(span.cols.end);
        let mut at = start;
        let inlays = display
            .inlays
            .iter()
            .filter(|(col, _)| span.cols.start < *col && *col < span.cols.end);
        for (_, r) in inlays {
            ranges.push((at..r.start, span.fade, span.strike));
            at = r.end;
        }
        ranges.push((at..end, span.fade, span.strike));
    }
    ranges.retain(|(r, _, _)| r.start < r.end);
    let mut cuts: Vec<usize> = ranges
        .iter()
        .flat_map(|(r, _, _)| [r.start, r.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let runs = split_runs(std::mem::take(&mut display.runs), &cuts);
    let mut pos = 0;
    display.runs = runs
        .into_iter()
        .map(|mut run| {
            let covering = ranges.iter().filter(|(r, _, _)| r.contains(&pos));
            let (fade, strike) = covering.fold((false, false), |(f, s), (_, fade, strike)| {
                (f || *fade, s || *strike)
            });
            pos += run.len;
            if fade {
                run.color.a *= UNNECESSARY_OPACITY;
            }
            if strike {
                let mut color = strike_color;
                if fade {
                    color.a *= UNNECESSARY_OPACITY;
                }
                run.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(color),
                });
            }
            run
        })
        .collect();
}

/// Splits `runs` so that every byte offset in `cuts` (sorted) starts a run.
fn split_runs(runs: Vec<TextRun>, cuts: &[usize]) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + cuts.len());
    let mut cuts = cuts.iter().copied().peekable();
    let mut pos = 0;
    for run in runs {
        let end = pos + run.len;
        let mut at = pos;
        while let Some(cut) = cuts.next_if(|&c| c < end) {
            if cut > at {
                out.push(TextRun {
                    len: cut - at,
                    ..run.clone()
                });
                at = cut;
            }
        }
        out.push(TextRun {
            len: end - at,
            ..run
        });
        pos = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_buffer::Point;
    use arcode_syntax::HighlightSpan;
    use arcode_ui_kit::UiTheme;

    use crate::display::{InlayText, build_display_line};

    fn theme() -> UiTheme {
        let registry = arcode_config::ThemeRegistry::builtin();
        let theme = registry.get("Arcode Chalk").expect("Chalk").clone();
        UiTheme::new(std::sync::Arc::new(theme))
    }

    fn diag(
        a: (usize, usize),
        b: (usize, usize),
        severity: DiagnosticSeverity,
        tags: DiagnosticTags,
    ) -> EditorDiagnostic {
        EditorDiagnostic {
            range: Point::new(a.0, a.1)..Point::new(b.0, b.1),
            severity,
            message: "m".into(),
            source: "s".into(),
            code: None,
            tags,
        }
    }

    const UNUSED: DiagnosticTags = DiagnosticTags {
        unnecessary: true,
        deprecated: false,
    };
    const DEPRECATED: DiagnosticTags = DiagnosticTags {
        unnecessary: false,
        deprecated: true,
    };

    #[test]
    fn spans_cover_each_line_of_a_range() {
        let ds = [
            diag((1, 4), (3, 2), DiagnosticSeverity::Hint, UNUSED),
            diag((2, 0), (2, 3), DiagnosticSeverity::Warning, DEPRECATED),
            diag((9, 0), (9, 3), DiagnosticSeverity::Hint, UNUSED),
            diag(
                (2, 1),
                (2, 2),
                DiagnosticSeverity::Error,
                DiagnosticTags::default(),
            ),
            diag((2, 5), (2, 5), DiagnosticSeverity::Hint, UNUSED),
        ];
        let tagged = tagged_in(&ds, 0..4, true);
        assert_eq!(tagged.len(), 2, "untagged, empty and off-screen ones drop");
        assert_eq!(
            line_spans(&tagged, 1, 10, true),
            vec![TagSpan {
                cols: 4..10,
                fade: true,
                strike: false
            }]
        );
        let line2 = line_spans(&tagged, 2, 6, true);
        assert_eq!(line2.len(), 2);
        assert_eq!((line2[0].cols.clone(), line2[0].fade), (0..6, true));
        assert_eq!((line2[1].cols.clone(), line2[1].strike), (0..3, true));
        assert_eq!(line_spans(&tagged, 3, 1, true)[0].cols, 0..1);
        // Fading off: only the strikethrough is left.
        let tagged = tagged_in(&ds, 0..4, false);
        assert_eq!(tagged.len(), 1);
        assert!(line_spans(&tagged, 1, 10, false).is_empty());
        assert!(line_spans(&tagged, 2, 6, false)[0].strike);
    }

    #[test]
    fn hints_that_only_mark_unused_code_are_fade_only() {
        let hint = diag((0, 0), (0, 1), DiagnosticSeverity::Hint, UNUSED);
        assert!(hint.fade_only(true));
        assert!(!hint.fade_only(false), "a plain hint with fading off");
        let warning = diag((0, 0), (0, 1), DiagnosticSeverity::Warning, UNUSED);
        assert!(!warning.fade_only(true), "warnings keep their underline");
        let plain = diag((0, 0), (0, 1), DiagnosticSeverity::Hint, DEPRECATED);
        assert!(!plain.fade_only(true));
    }

    #[test]
    fn restyle_fades_and_strikes_only_the_spans() {
        let t = theme();
        let spans = [HighlightSpan {
            range: 0..3,
            name: "keyword",
        }];
        // "use os; old()" with "use os;" unused and "old" deprecated.
        let mut line = build_display_line("use os; old()", &spans, &[], 4, &gpui::font("Mono"), &t);
        let keyword = line.runs[0].color;
        let text = t.colors.text;
        restyle(
            &mut line,
            &[
                TagSpan {
                    cols: 0..7,
                    fade: true,
                    strike: false,
                },
                TagSpan {
                    cols: 8..11,
                    fade: false,
                    strike: true,
                },
            ],
            text,
        );
        let lens: Vec<usize> = line.runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![3, 4, 1, 3, 2]);
        assert_eq!(line.runs[0].color.a, keyword.a * UNNECESSARY_OPACITY);
        assert_eq!(line.runs[0].color.h, keyword.h, "keeps the syntax colour");
        assert_eq!(line.runs[1].color.a, text.a * UNNECESSARY_OPACITY);
        assert_eq!(line.runs[2].color, text);
        let strike = line.runs[3].strikethrough.expect("struck");
        assert_eq!(strike.color, Some(text));
        assert_eq!(line.runs[3].color, text);
        assert!(
            line.runs
                .iter()
                .enumerate()
                .all(|(i, r)| (i == 3) == r.strikethrough.is_some())
        );
        assert_eq!(line.runs[4].color, text);
    }

    #[test]
    fn restyle_leaves_inlay_text_alone() {
        let t = theme();
        let muted = t.colors.text_muted;
        let inlay = InlayText {
            col: 2,
            text: ": i32",
            color: muted,
            background: None,
            italic: false,
        };
        let mut line = build_display_line("abcd", &[], &[inlay], 4, &gpui::font("Mono"), &t);
        let both = TagSpan {
            cols: 0..4,
            fade: true,
            strike: true,
        };
        restyle(&mut line, &[both], t.colors.text);
        let total: usize = line.runs.iter().map(|r| r.len).sum();
        assert_eq!(total, line.text.len());
        let faded = t.colors.text.a * UNNECESSARY_OPACITY;
        let inlay_run = line.runs.iter().find(|r| r.color == muted).expect("inlay");
        assert!(inlay_run.strikethrough.is_none());
        let code: Vec<_> = line.runs.iter().filter(|r| r.color != muted).collect();
        assert_eq!(code.len(), 2);
        for run in code {
            assert_eq!(run.color.a, faded);
            let strike = run.strikethrough.expect("struck");
            assert_eq!(
                strike.color.map(|c| c.a),
                Some(faded),
                "faded with the text"
            );
        }
    }
}
