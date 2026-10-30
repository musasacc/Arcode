//! Line operators: `>` `<` (shift), `=` (basic re-indent), `gc` (comment).

use arcode_buffer::ops::IndentUnit;

use crate::normal::status;
use crate::ops::Region;
use crate::text::{indent_of, is_blank, line_blank};
use crate::types::VimAction;
use crate::vim::{Ed, Vim};

/// Number of leading chars of `indent` making up `width` display columns
/// (at least one char if `indent` is non-empty and `width > 0`).
fn chars_for_width(indent: &str, width: usize, tab: usize) -> usize {
    let mut col = 0;
    let mut n = 0;
    for c in indent.chars() {
        if col >= width {
            break;
        }
        col = if c == '\t' {
            (col / tab + 1) * tab
        } else {
            col + 1
        };
        n += 1;
    }
    n
}

/// `indent` with one unit removed from its end (or its start for tabs).
fn dedent(indent: &str, unit: IndentUnit, tab: usize) -> String {
    let n = chars_for_width(indent, unit.width(tab), tab);
    indent.chars().skip(n).collect()
}

impl Vim {
    /// `>` / `<`: shifts every non-empty line by `levels` indent units.
    pub(crate) fn op_shift(
        &mut self,
        regions: &[(Region, usize)],
        right: bool,
        levels: usize,
        ed: &mut Ed,
    ) {
        let tab = ed.ctx.tab_size.max(1);
        let unit = ed.ctx.indent;
        let mut edits = Vec::new();
        for line in Self::region_lines(regions, ed) {
            if ed.b.line_len(line) == 0 {
                continue;
            }
            let start = ed.b.line_start(line);
            if right {
                edits.push((start..start, unit.text().repeat(levels)));
            } else {
                let indent = indent_of(ed.b, line);
                let n = chars_for_width(&indent, unit.width(tab) * levels, tab);
                edits.push((start..start + n, String::new()));
            }
        }
        self.apply_edits(ed, edits);
        self.cursor_to_first_line(regions, ed);
    }

    /// `=`: re-indents each line from the nearest non-blank line above:
    /// one unit deeper after a line ending in `{ ( [`, one shallower for a
    /// line starting with `} ) ]`. Blank lines lose their whitespace.
    pub(crate) fn op_reindent(&mut self, regions: &[(Region, usize)], ed: &mut Ed) {
        let tab = ed.ctx.tab_size.max(1);
        let unit = ed.ctx.indent;
        let lines = Self::region_lines(regions, ed);
        let Some(&first) = lines.first() else { return };
        let mut prev: Option<(String, String)> = (0..first)
            .rev()
            .find(|&l| !line_blank(ed.b, l))
            .map(|l| (indent_of(ed.b, l), ed.b.line_text(l)));
        let mut edits = Vec::new();
        for line in lines {
            let text = ed.b.line_text(line);
            let old_indent = indent_of(ed.b, line);
            let start = ed.b.line_start(line);
            let body = text.trim_start_matches(is_blank);
            if body.is_empty() {
                edits.push((start..start + old_indent.chars().count(), String::new()));
                continue;
            }
            let mut indent = match &prev {
                Some((ind, t)) => {
                    let mut ind = ind.clone();
                    if t.trim_end().ends_with(['{', '(', '[']) {
                        ind.push_str(&unit.text());
                    }
                    ind
                }
                None => String::new(),
            };
            if body.starts_with(['}', ')', ']']) {
                indent = dedent(&indent, unit, tab);
            }
            edits.push((start..start + old_indent.chars().count(), indent.clone()));
            prev = Some((indent.clone(), format!("{indent}{body}")));
        }
        self.apply_edits(ed, edits);
        self.cursor_to_first_line(regions, ed);
    }

    /// `gc`: toggles the language's line comment on the lines (adds it at
    /// the smallest indentation unless every non-blank line has it).
    pub(crate) fn op_comment(&mut self, regions: &[(Region, usize)], ed: &mut Ed) -> VimAction {
        let Some(prefix) = ed.ctx.line_comment.clone().filter(|p| !p.trim().is_empty()) else {
            return status("No line comment defined for this language");
        };
        let prefix = prefix.trim().to_string();
        let lines: Vec<usize> = Self::region_lines(regions, ed)
            .into_iter()
            .filter(|&l| !line_blank(ed.b, l))
            .collect();
        let plen = prefix.chars().count();
        let body_start =
            |ed: &Ed, l: usize| ed.b.line_start(l) + indent_of(ed.b, l).chars().count();
        let commented = lines.iter().all(|&l| {
            let s = body_start(ed, l);
            ed.b.slice_to_string(s..s + plen) == prefix
        });
        let mut edits = Vec::new();
        if commented {
            for &l in &lines {
                let s = body_start(ed, l);
                let mut e = s + plen;
                if ed.b.char_at(e) == Some(' ') {
                    e += 1;
                }
                edits.push((s..e, String::new()));
            }
        } else {
            let min = lines
                .iter()
                .map(|&l| indent_of(ed.b, l).chars().count())
                .min()
                .unwrap_or(0);
            for &l in &lines {
                let at = ed.b.line_start(l) + min;
                edits.push((at..at, format!("{prefix} ")));
            }
        }
        self.apply_edits(ed, edits);
        self.cursor_to_first_line(regions, ed);
        VimAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(chars_for_width("        x", 4, 4), 4);
        assert_eq!(chars_for_width("\t  ", 4, 4), 1);
        assert_eq!(chars_for_width("  ", 4, 4), 2);
        assert_eq!(dedent("        ", IndentUnit::Spaces(4), 4), "    ");
        assert_eq!(dedent("\t\t", IndentUnit::Tab, 4), "\t");
    }
}
