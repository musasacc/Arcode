use super::*;
use arcode_terminal::Palette;

fn row(text: &str) -> Vec<Cell> {
    let palette = Palette::default();
    text.chars()
        .map(|ch| Cell {
            ch,
            ..Cell::blank(&palette)
        })
        .collect()
}

fn fg() -> Rgb {
    Palette::default().foreground
}

#[test]
fn grid_size_fits_whole_cells() {
    assert_eq!(grid_size(800., 480., 8., 16.), (100, 30));
    assert_eq!(grid_size(807.9, 495.9, 8., 16.), (100, 30));
    // Clamped to the minimum and robust against degenerate input.
    assert_eq!(grid_size(3., 3., 8., 16.), (MIN_COLS, MIN_ROWS));
    assert_eq!(grid_size(800., 480., 0., 16.), (MIN_COLS, 30));
    assert_eq!(grid_size(f32::NAN, -5., 8., 16.), (MIN_COLS, MIN_ROWS));
    assert_eq!(grid_size(1e9, 1e9, 1., 1.), (u16::MAX, u16::MAX));
}

#[test]
fn grid_point_clamps() {
    assert_eq!(grid_point(0., 0., 8., 16., 80, 24), GridPoint::new(0, 0));
    assert_eq!(grid_point(17., 33., 8., 16., 80, 24), GridPoint::new(2, 2));
    assert_eq!(grid_point(-5., -5., 8., 16., 80, 24), GridPoint::new(0, 0));
    assert_eq!(
        grid_point(1e6, 1e6, 8., 16., 80, 24),
        GridPoint::new(23, 79)
    );
    assert_eq!(grid_point(10., 10., 8., 16., 0, 0), GridPoint::new(0, 0));
}

#[test]
fn background_runs_merge_equal_colors() {
    let default = Palette::default().background;
    let red = Rgb::new(255, 0, 0);
    let blue = Rgb::new(0, 0, 255);
    let mut cells = row("abcdefg");
    for (i, c) in [red, red, default, blue, blue, blue, red]
        .into_iter()
        .enumerate()
    {
        cells[i].bg = c;
    }
    assert_eq!(
        background_runs(&cells, default),
        vec![(0..2, red), (3..6, blue), (6..7, red)]
    );
    assert!(background_runs(&row("plain"), default).is_empty());
}

#[test]
fn selection_runs_group_selected_cells() {
    let mut cells = row("abcdef");
    for i in [1, 2, 4] {
        cells[i].selected = true;
    }
    assert_eq!(selection_runs(&cells), vec![1..3, 4..5]);
}

#[test]
fn segments_trim_and_batch_styles() {
    let mut cells = row("  ab cd   ");
    let red = Rgb::new(255, 0, 0);
    cells[5].fg = red;
    cells[6].fg = red;
    let segs = text_segments(&cells, None);
    assert_eq!(segs.len(), 1);
    let s = &segs[0];
    assert_eq!((s.col, s.text.as_str(), s.single), (2, "ab cd", false));
    // The blank between "ab" and "cd" joins the first run.
    let lens: Vec<_> = s.runs.iter().map(|r| (r.len, r.style.fg)).collect();
    assert_eq!(lens, vec![(3, fg()), (2, red)]);
    assert!(text_segments(&row("     "), None).is_empty());
}

#[test]
fn font_changes_split_segments() {
    let mut cells = row("ab cd ef");
    cells[1].bold = true;
    cells[3].bold = true;
    cells[4].bold = true;
    cells[6].italic = true;
    cells[7].italic = true;
    let segs = text_segments(&cells, None);
    let summary: Vec<_> = segs
        .iter()
        .map(|s| (s.col, s.text.as_str(), s.font_key()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (0, "a", Some((false, false))),
            (1, "b cd", Some((true, false))),
            (6, "ef", Some((false, true))),
        ]
    );
}

#[test]
fn decorations_split_runs_but_not_segments() {
    let mut cells = row("abc");
    cells[1].underline = true;
    cells[2].strikeout = true;
    let segs = text_segments(&cells, None);
    assert_eq!(segs.len(), 1);
    let styles: Vec<_> = segs[0]
        .runs
        .iter()
        .map(|r| (r.len, r.style.underline, r.style.strikeout))
        .collect();
    assert_eq!(
        styles,
        vec![(1, false, false), (1, true, false), (1, false, true)]
    );
}

#[test]
fn blank_after_decoration_is_plain() {
    let mut cells = row("ab c");
    cells[1].underline = true;
    let segs = text_segments(&cells, None);
    let styles: Vec<_> = segs[0]
        .runs
        .iter()
        .map(|r| (r.len, r.style.underline))
        .collect();
    // "a" plain, "b" underlined, " c" plain: the space is not underlined.
    assert_eq!(styles, vec![(1, false), (1, true), (2, false)]);
}

#[test]
fn underlined_spaces_are_kept() {
    let mut cells = row("a   ");
    cells[1].underline = true;
    let segs = text_segments(&cells, None);
    assert_eq!(segs[0].text, "a ");
    assert_eq!(segs[0].runs.iter().map(|r| r.len).sum::<usize>(), 2);
}

#[test]
fn wide_chars_get_own_segment() {
    let palette = Palette::default();
    let mut cells = row("a    b");
    cells[1] = Cell {
        ch: '漢',
        wide: true,
        ..Cell::blank(&palette)
    };
    cells[2] = Cell {
        wide_spacer: true,
        ..Cell::blank(&palette)
    };
    cells[3].ch = 'x';
    let segs = text_segments(&cells, None);
    let summary: Vec<_> = segs
        .iter()
        .map(|s| (s.col, s.text.as_str(), s.single))
        .collect();
    assert_eq!(
        summary,
        vec![(0, "a", false), (1, "漢", true), (3, "x b", false)]
    );
}

#[test]
fn combining_marks_stay_on_their_cell() {
    let mut cells = row("ae");
    cells[0].zerowidth = vec!['\u{301}'];
    let segs = text_segments(&cells, None);
    assert_eq!(segs.len(), 2);
    assert_eq!((segs[0].col, segs[0].text.as_str()), (0, "a\u{301}"));
    assert!(segs[0].single);
    assert_eq!(segs[0].runs[0].len, "a\u{301}".len());
    assert_eq!((segs[1].col, segs[1].text.as_str()), (1, "e"));
}

#[test]
fn fg_override_recolors_one_cell() {
    let cells = row("abc");
    let bg = Rgb::new(1, 2, 3);
    let segs = text_segments(&cells, Some((1, bg)));
    let runs: Vec<_> = segs[0].runs.iter().map(|r| (r.len, r.style.fg)).collect();
    assert_eq!(runs, vec![(1, fg()), (1, bg), (1, fg())]);
}

#[test]
fn multibyte_runs_count_bytes() {
    let mut cells = row("é─x");
    cells[2].fg = Rgb::new(9, 9, 9);
    let segs = text_segments(&cells, None);
    assert_eq!(segs[0].runs[0].len, "é─".len());
    assert_eq!(segs[0].runs[1].len, 1);
}
