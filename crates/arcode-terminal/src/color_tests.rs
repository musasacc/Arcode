//! Unit tests for `color`.

use super::*;

fn cell(fg: Color, bg: Color, flags: Flags) -> TermCell {
    TermCell {
        fg,
        bg,
        flags,
        ..TermCell::default()
    }
}

#[test]
fn hex_roundtrip() {
    let c = Rgb::from_hex(0x123456);
    assert_eq!(c, Rgb::new(0x12, 0x34, 0x56));
    assert_eq!(c.to_hex(), 0x123456);
}

#[test]
fn color_cube() {
    let p = Palette::default();
    assert_eq!(p.indexed(16), Rgb::new(0, 0, 0));
    assert_eq!(p.indexed(21), Rgb::new(0, 0, 255));
    assert_eq!(p.indexed(196), Rgb::new(255, 0, 0));
    assert_eq!(p.indexed(46), Rgb::new(0, 255, 0));
    assert_eq!(p.indexed(231), Rgb::new(255, 255, 255));
    // 16 + 36*1 + 6*2 + 3 = 67 -> (95, 135, 175)
    assert_eq!(p.indexed(67), Rgb::new(95, 135, 175));
}

#[test]
fn grayscale_ramp() {
    let p = Palette::default();
    assert_eq!(p.indexed(232), Rgb::new(8, 8, 8));
    assert_eq!(p.indexed(244), Rgb::new(128, 128, 128));
    assert_eq!(p.indexed(255), Rgb::new(238, 238, 238));
}

#[test]
fn first_sixteen_come_from_palette() {
    let p = Palette::default();
    for i in 0..16u8 {
        assert_eq!(p.indexed(i), p.ansi[usize::from(i)]);
    }
}

#[test]
fn default_colors() {
    let p = Palette::default();
    let r = ColorResolver::new(&p, None);
    let c = TermCell::default();
    assert_eq!(r.cell_colors(&c), (p.foreground, p.background));
    assert_eq!(r.cursor(), p.cursor);
}

#[test]
fn bold_as_bright_for_named_and_indexed() {
    let p = Palette::default();
    let r = ColorResolver::new(&p, None);
    let bg = Color::Named(NamedColor::Background);
    let named = cell(Color::Named(NamedColor::Red), bg, Flags::BOLD);
    assert_eq!(r.cell_colors(&named).0, p.ansi[9]);
    let indexed = cell(Color::Indexed(2), bg, Flags::BOLD);
    assert_eq!(r.cell_colors(&indexed).0, p.ansi[10]);
    // Bright colors and colors above 8 stay as they are.
    let high = cell(Color::Indexed(100), bg, Flags::BOLD);
    assert_eq!(r.cell_colors(&high).0, p.indexed(100));
    let bright = cell(Color::Named(NamedColor::BrightBlue), bg, Flags::BOLD);
    assert_eq!(r.cell_colors(&bright).0, p.ansi[12]);
    // Not bold: normal color.
    let plain = cell(Color::Named(NamedColor::Red), bg, Flags::empty());
    assert_eq!(r.cell_colors(&plain).0, p.ansi[1]);
}

#[test]
fn inverse_swaps() {
    let p = Palette::default();
    let r = ColorResolver::new(&p, None);
    let c = cell(
        Color::Named(NamedColor::Green),
        Color::Named(NamedColor::Blue),
        Flags::INVERSE,
    );
    assert_eq!(r.cell_colors(&c), (p.ansi[4], p.ansi[2]));
    let d = cell(
        Color::Named(NamedColor::Foreground),
        Color::Named(NamedColor::Background),
        Flags::INVERSE,
    );
    assert_eq!(r.cell_colors(&d), (p.background, p.foreground));
}

#[test]
fn dim_darkens() {
    let p = Palette::default();
    let r = ColorResolver::new(&p, None);
    let bg = Color::Named(NamedColor::Background);
    let named = cell(Color::Named(NamedColor::Red), bg, Flags::DIM);
    assert_eq!(r.cell_colors(&named).0, p.ansi[1].dim());
    let fg = cell(Color::Named(NamedColor::Foreground), bg, Flags::DIM);
    assert_eq!(r.cell_colors(&fg).0, p.foreground.dim());
    let spec = cell(
        Color::Spec(ansi::Rgb {
            r: 200,
            g: 100,
            b: 50,
        }),
        bg,
        Flags::DIM,
    );
    assert_eq!(r.cell_colors(&spec).0, Rgb::new(132, 66, 33));
    let idx = cell(Color::Indexed(3), bg, Flags::DIM);
    assert_eq!(r.cell_colors(&idx).0, p.ansi[3].dim());
    let cube = cell(Color::Indexed(196), bg, Flags::DIM);
    assert_eq!(r.cell_colors(&cube).0, Rgb::new(255, 0, 0).dim());
}

#[test]
fn truecolor_and_hidden() {
    let p = Palette::default();
    let r = ColorResolver::new(&p, None);
    let fg = ansi::Rgb { r: 1, g: 2, b: 3 };
    let bg = ansi::Rgb { r: 4, g: 5, b: 6 };
    let c = cell(Color::Spec(fg), Color::Spec(bg), Flags::empty());
    assert_eq!(r.cell_colors(&c), (Rgb::new(1, 2, 3), Rgb::new(4, 5, 6)));
    let h = cell(Color::Spec(fg), Color::Spec(bg), Flags::HIDDEN);
    assert_eq!(r.cell_colors(&h), (Rgb::new(4, 5, 6), Rgb::new(4, 5, 6)));
}

#[test]
fn overrides_win() {
    let p = Palette::default();
    let mut colors = Colors::default();
    colors[NamedColor::Red] = Some(ansi::Rgb { r: 9, g: 9, b: 9 });
    colors[NamedColor::Background] = Some(ansi::Rgb { r: 7, g: 7, b: 7 });
    let r = ColorResolver::new(&p, Some(&colors));
    let c = cell(
        Color::Named(NamedColor::Red),
        Color::Named(NamedColor::Background),
        Flags::empty(),
    );
    assert_eq!(r.cell_colors(&c), (Rgb::new(9, 9, 9), Rgb::new(7, 7, 7)));
}

#[test]
fn table_special_entries() {
    let p = Palette::default();
    assert_eq!(p.table(256), p.foreground);
    assert_eq!(p.table(257), p.background);
    assert_eq!(p.table(258), p.cursor);
    assert_eq!(p.table(259), p.ansi[0].dim());
    assert_eq!(p.table(266), p.ansi[7].dim());
    assert_eq!(p.table(267), p.foreground);
    assert_eq!(p.table(268), p.foreground.dim());
    assert_eq!(p.table(9999), p.foreground);
}
