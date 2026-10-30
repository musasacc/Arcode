//! Snapshot tests driven by real escape sequences through the VT parser.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

use crate::color::{Palette, Rgb};
use crate::selection::{self, GridPoint, SelectionKind};
use crate::size::TermSize;
use crate::snapshot::{CursorShape, build, text};

fn term(cols: usize, rows: usize, input: &str) -> Term<VoidListener> {
    let mut term = Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener);
    feed(&mut term, input);
    term
}

fn feed(term: &mut Term<VoidListener>, input: &str) {
    let mut parser: Processor = Processor::new();
    parser.advance(term, input.as_bytes());
}

#[test]
fn dimensions_and_text() {
    let t = term(10, 3, "hi  \r\nthere");
    let p = Palette::default();
    let s = build(&t, &p);
    assert_eq!((s.cols, s.rows, s.cells.len()), (10, 3, 30));
    assert_eq!(s.text(), "hi\nthere\n");
    assert_eq!(text(&t), "hi\nthere\n");
    assert_eq!(s.row_text(1), "there");
    assert_eq!(s.cell(0, 1).map(|c| c.ch), Some('i'));
    assert!(s.cell(3, 0).is_none() && s.cell(0, 10).is_none());
    assert_eq!(s.row_iter().count(), 3);
}

#[test]
fn sgr_colors_resolve() {
    let p = Palette::default();
    // 31 red, 1;32 bold green -> bright green, 38;5;196 cube red,
    // 48;5;244 gray bg, 38;2 truecolor, 7 inverse.
    let input = "\x1b[31mA\x1b[0m\x1b[1;32mB\x1b[0m\x1b[38;5;196mC\x1b[0m\
                 \x1b[48;5;244mD\x1b[0m\x1b[38;2;10;20;30mE\x1b[0m\x1b[7mF\x1b[0m\x1b[2;34mG";
    let t = term(20, 2, input);
    let s = build(&t, &p);
    let c = |col| s.cell(0, col).cloned().unwrap();
    assert_eq!(c(0).fg, p.ansi[1]);
    assert_eq!((c(1).fg, c(1).bold), (p.ansi[10], true));
    assert_eq!(c(2).fg, Rgb::new(255, 0, 0));
    assert_eq!(c(3).bg, Rgb::new(128, 128, 128));
    assert_eq!(c(4).fg, Rgb::new(10, 20, 30));
    assert_eq!((c(5).fg, c(5).bg), (p.background, p.foreground));
    assert_eq!((c(6).fg, c(6).dim), (p.ansi[4].dim(), true));
    // Untouched cells use the defaults.
    assert_eq!((c(10).fg, c(10).bg), (p.foreground, p.background));
}

#[test]
fn bold_bright_indexed_and_attributes() {
    let p = Palette::default();
    let t = term(10, 1, "\x1b[1;38;5;3mA\x1b[0m\x1b[3;4;9mB");
    let s = build(&t, &p);
    let a = s.cell(0, 0).unwrap();
    assert_eq!(a.fg, p.ansi[11]);
    let b = s.cell(0, 1).unwrap();
    assert!(b.italic && b.underline && b.strikeout && !b.bold);
}

#[test]
fn osc_color_override_is_honored() {
    let p = Palette::default();
    // OSC 4 sets palette entry 1 (red); OSC 11 sets the background.
    let t = term(
        10,
        1,
        "\x1b]4;1;rgb:11/22/33\x07\x1b]11;rgb:01/02/03\x07\x1b[31mR",
    );
    let s = build(&t, &p);
    let r = s.cell(0, 0).unwrap();
    assert_eq!(r.fg, Rgb::new(0x11, 0x22, 0x33));
    assert_eq!(r.bg, Rgb::new(1, 2, 3));
}

#[test]
fn wide_chars() {
    let t = term(10, 1, "a\u{4e2d}b");
    let s = build(&t, &Palette::default());
    let lead = s.cell(0, 1).unwrap();
    assert!(lead.wide && !lead.wide_spacer);
    assert_eq!(lead.ch, '\u{4e2d}');
    let spacer = s.cell(0, 2).unwrap();
    assert!(spacer.wide_spacer);
    assert_eq!(s.cell(0, 3).unwrap().ch, 'b');
    assert_eq!(s.text(), "a\u{4e2d}b");
    assert_eq!(text(&t), "a\u{4e2d}b");
}

#[test]
fn combining_marks() {
    let t = term(10, 1, "e\u{301}x");
    let s = build(&t, &Palette::default());
    assert_eq!(s.cell(0, 0).unwrap().zerowidth, vec!['\u{301}']);
    assert_eq!(s.text(), "e\u{301}x");
}

#[test]
fn cursor_position_shape_and_visibility() {
    let p = Palette::default();
    let mut t = term(10, 3, "ab\r\ncd");
    let cursor = build(&t, &p).cursor.unwrap();
    assert_eq!(
        (cursor.row, cursor.col, cursor.shape),
        (1, 2, CursorShape::Block)
    );
    assert_eq!(cursor.color, p.cursor);
    feed(&mut t, "\x1b[6 q");
    assert_eq!(build(&t, &p).cursor.unwrap().shape, CursorShape::Beam);
    feed(&mut t, "\x1b[4 q");
    assert_eq!(build(&t, &p).cursor.unwrap().shape, CursorShape::Underline);
    feed(&mut t, "\x1b[?25l");
    assert_eq!(build(&t, &p).cursor, None);
    feed(&mut t, "\x1b[?25h");
    assert!(build(&t, &p).cursor.is_some());
}

#[test]
fn scrolling_hides_cursor_and_shifts_view() {
    let p = Palette::default();
    let mut lines = String::new();
    for i in 0..10 {
        lines.push_str(&format!("line{i}\r\n"));
    }
    lines.push_str("last");
    let mut t = term(10, 3, &lines);
    let s = build(&t, &p);
    assert_eq!(s.text(), "line8\nline9\nlast");
    assert_eq!(s.history_size, 8);
    t.scroll_display(Scroll::Delta(2));
    let s = build(&t, &p);
    assert_eq!(s.display_offset, 2);
    assert_eq!(s.text(), "line6\nline7\nline8");
    assert_eq!(text(&t), "line6\nline7\nline8");
    assert_eq!(s.cursor, None);
    t.scroll_display(Scroll::Bottom);
    assert!(build(&t, &p).cursor.is_some());
}

#[test]
fn modes() {
    let p = Palette::default();
    let mut t = term(10, 2, "");
    let m = build(&t, &p).mode;
    assert!(!m.app_cursor && !m.bracketed_paste && !m.alt_screen && !m.mouse_reporting);
    feed(
        &mut t,
        "\x1b[?1h\x1b[?2004h\x1b[?1049h\x1b[?1000h\x1b[?1006h",
    );
    let m = build(&t, &p).mode;
    assert!(m.app_cursor && m.bracketed_paste && m.alt_screen && m.mouse_reporting && m.sgr_mouse);
}

#[test]
fn selected_cells_are_marked() {
    let p = Palette::default();
    let mut t = term(10, 2, "hello");
    let a = selection::start(&mut t, GridPoint::new(0, 1), SelectionKind::Simple);
    selection::update(&mut t, a, GridPoint::new(0, 3));
    let s = build(&t, &p);
    let selected: Vec<bool> = (0..5).map(|c| s.cell(0, c).unwrap().selected).collect();
    assert_eq!(selected, vec![false, true, true, true, false]);
}

#[test]
fn selection_follows_scrolled_view() {
    let mut lines = String::new();
    for i in 0..6 {
        lines.push_str(&format!("row{i}\r\n"));
    }
    let mut t = term(10, 3, &lines);
    // Screen shows row4, row5 and an empty line; one line up shows row3 first.
    t.scroll_display(Scroll::Delta(1));
    let a = selection::start(&mut t, GridPoint::new(0, 0), SelectionKind::Simple);
    selection::update(&mut t, a, GridPoint::new(0, 3));
    assert_eq!(selection::text(&t).as_deref(), Some("row3"));
}
