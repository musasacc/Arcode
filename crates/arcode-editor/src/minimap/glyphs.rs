//! Minimap glyphs: every char is drawn as a tiny pattern of device pixels
//! that follows its outline (ascenders and capitals reach the top of the
//! cell, lower-case letters only the lower part, a dot sits at the bottom,
//! a quote at the top), so lines read as scaled-down code rather than bars.
//!
//! A cell is `col` device pixels wide and `pitch` tall. At 1x it is 1 x 2
//! and the outline survives as two intensities; from 1.5x on, the last
//! pixel row stays empty (only descenders reach into it), so lines are
//! separated, and narrow chars (`i`, `l`, `.`) cover only part of the
//! width.

/// Rough outline of a char.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Capitals, digits, ascenders, brackets: the full height.
    Tall,
    /// Lower-case letters without ascenders: the x-height.
    Short,
    /// `g j p q y`: the x-height and below it.
    Descender,
    /// `. , _ : ;`: at the bottom.
    Low,
    /// `- = + * < > ~`: in the middle.
    Mid,
    /// Quotes and `^`: at the top.
    High,
}

const SHAPES: usize = 6;

/// The outline of `c` and whether it is narrow (`None` for blanks).
pub(crate) fn shape_of(c: char) -> Option<(Shape, bool)> {
    if c.is_whitespace() || c.is_control() {
        return None;
    }
    let shape = match c {
        'a' | 'c' | 'e' | 'm' | 'n' | 'o' | 'r' | 's' | 'u' | 'v' | 'w' | 'x' | 'z' => Shape::Short,
        'g' | 'j' | 'p' | 'q' | 'y' => Shape::Descender,
        '.' | ',' | '_' | ':' | ';' => Shape::Low,
        '-' | '=' | '+' | '*' | '<' | '>' | '~' => Shape::Mid,
        '\'' | '"' | '`' | '^' | '\u{b0}' => Shape::High,
        _ => Shape::Tall,
    };
    let narrow = matches!(
        c,
        'i' | 'l' | 'j' | 't' | 'f' | 'r' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '`'
    );
    Some((shape, narrow))
}

/// Coverage (0-255) of each pixel of a cell, per shape and width.
pub(crate) struct Patterns {
    pub col: usize,
    pub pitch: usize,
    /// `SHAPES * 2` patterns of `col * pitch` values, row by row.
    cells: Vec<u8>,
}

impl Patterns {
    /// Patterns for cells `col` x `pitch` device pixels.
    pub fn new(col: usize, pitch: usize) -> Self {
        let (col, pitch) = (col.max(1), pitch.max(2));
        let mut cells = Vec::with_capacity(SHAPES * 2 * col * pitch);
        for shape in [
            Shape::Tall,
            Shape::Short,
            Shape::Descender,
            Shape::Low,
            Shape::Mid,
            Shape::High,
        ] {
            let rows = row_coverage(shape, pitch);
            for narrow in [false, true] {
                let cols = col_coverage(narrow, col);
                for r in &rows {
                    for c in &cols {
                        cells.push((r * c * 255.).round() as u8);
                    }
                }
            }
        }
        Patterns { col, pitch, cells }
    }

    /// The pattern of a char of `shape`, row by row.
    pub fn get(&self, shape: Shape, narrow: bool) -> &[u8] {
        let n = self.col * self.pitch;
        let ix = (shape as usize * 2 + usize::from(narrow)) * n;
        &self.cells[ix..ix + n]
    }
}

/// Coverage of each pixel row of a cell `pitch` tall.
fn row_coverage(shape: Shape, pitch: usize) -> Vec<f32> {
    if pitch == 2 {
        // 1x: an upper and a lower half; the lower half carries the line.
        let [top, bottom] = match shape {
            Shape::Tall => [0.8, 1.],
            Shape::Short | Shape::Descender => [0.3, 1.],
            Shape::Low => [0., 0.55],
            Shape::Mid => [0.2, 0.75],
            Shape::High => [0.6, 0.1],
        };
        return vec![top, bottom];
    }
    // The last row is the gap between lines; the x-height starts a third
    // down the rest.
    let body = pitch - 1;
    let x_top = ((body as f32) / 3.).round().max(1.) as usize;
    let mut rows = vec![0.; pitch];
    let fill = |rows: &mut Vec<f32>, range: std::ops::Range<usize>, v: f32| {
        for r in range {
            rows[r] = v;
        }
    };
    match shape {
        Shape::Tall => fill(&mut rows, 0..body, 1.),
        Shape::Short => fill(&mut rows, x_top..body, 1.),
        Shape::Descender => {
            fill(&mut rows, x_top..body, 1.);
            rows[body] = 0.7;
        }
        Shape::Low => rows[body - 1] = 0.75,
        Shape::Mid => rows[(x_top + body - 1) / 2] = 0.85,
        Shape::High => {
            rows[0] = 0.85;
            if body >= 4 {
                rows[1] = 0.4;
            }
        }
    }
    rows
}

/// Coverage of each pixel column of a cell `col` wide.
fn col_coverage(narrow: bool, col: usize) -> Vec<f32> {
    if col == 1 {
        return vec![if narrow { 0.65 } else { 1. }];
    }
    (0..col)
        .map(|c| {
            if narrow {
                // The left half.
                if c < col.div_ceil(2) { 1. } else { 0. }
            } else if col >= 3 && c == col - 1 {
                // A little space between wide chars.
                0.35
            } else {
                1.
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chars_get_their_outline() {
        assert_eq!(shape_of('H'), Some((Shape::Tall, false)));
        assert_eq!(shape_of('d'), Some((Shape::Tall, false)));
        assert_eq!(shape_of('l'), Some((Shape::Tall, true)));
        assert_eq!(shape_of('m'), Some((Shape::Short, false)));
        assert_eq!(shape_of('y'), Some((Shape::Descender, false)));
        assert_eq!(shape_of('.'), Some((Shape::Low, true)));
        assert_eq!(shape_of('='), Some((Shape::Mid, false)));
        assert_eq!(shape_of('"'), Some((Shape::High, false)));
        assert_eq!(shape_of('{'), Some((Shape::Tall, false)));
        assert_eq!(shape_of('\u{4e2d}'), Some((Shape::Tall, false)));
        assert_eq!(shape_of(' '), None);
        assert_eq!(shape_of('\t'), None);
    }

    #[test]
    fn one_x_cells_keep_the_outline_as_intensity() {
        let p = Patterns::new(1, 2);
        assert_eq!(p.get(Shape::Tall, false), &[204, 255]);
        assert_eq!(p.get(Shape::Short, false), &[77, 255]);
        assert_eq!(p.get(Shape::Low, false), &[0, 140]);
        assert_eq!(p.get(Shape::High, false)[1], 26);
        // Narrow chars are lighter.
        assert!(p.get(Shape::Tall, true)[1] < p.get(Shape::Tall, false)[1]);
    }

    #[test]
    fn two_x_cells_have_a_gap_row_and_narrow_chars() {
        let p = Patterns::new(2, 4);
        // Row by row, two pixels each.
        assert_eq!(
            p.get(Shape::Tall, false),
            &[255, 255, 255, 255, 255, 255, 0, 0]
        );
        assert_eq!(
            p.get(Shape::Short, false),
            &[0, 0, 255, 255, 255, 255, 0, 0]
        );
        assert_eq!(p.get(Shape::Descender, false)[6], 179, "below the line");
        assert_eq!(p.get(Shape::Tall, true), &[255, 0, 255, 0, 255, 0, 0, 0]);
        assert_eq!(p.get(Shape::Low, true), &[0, 0, 0, 0, 191, 0, 0, 0]);
        assert_eq!(p.get(Shape::High, false), &[217, 217, 0, 0, 0, 0, 0, 0]);
        // Every shape leaves the last row to descenders.
        for shape in [Shape::Tall, Shape::Short, Shape::Low, Shape::Mid] {
            assert_eq!(&p.get(shape, false)[6..], &[0, 0], "{shape:?}");
        }
    }

    #[test]
    fn odd_scales_still_separate_lines() {
        // 1.5x: cells 2 x 3.
        let p = Patterns::new(2, 3);
        assert_eq!(p.get(Shape::Tall, false), &[255, 255, 255, 255, 0, 0]);
        assert_eq!(p.get(Shape::Short, false), &[0, 0, 255, 255, 0, 0]);
        // 3x: cells 3 x 6, a lighter column between chars.
        let p = Patterns::new(3, 6);
        let tall = p.get(Shape::Tall, false);
        assert_eq!(&tall[..3], &[255, 255, 89]);
        assert_eq!(&tall[15..], &[0, 0, 0]);
    }
}
