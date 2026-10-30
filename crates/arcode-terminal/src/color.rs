//! Colors: [`Rgb`], [`Palette`] and full resolution of cell colors
//! (named, indexed, truecolor, bold-as-bright, dim, inverse, hidden).

use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{self, Color, NamedColor};

/// Factor applied to a color's channels for the DIM attribute.
pub const DIM_FACTOR: f32 = 0.66;

/// An 8-bit-per-channel RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Rgb {
    /// Creates a color from its channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Creates a color from a `0xRRGGBB` value.
    pub const fn from_hex(hex: u32) -> Self {
        Self::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Returns the color packed as `0xRRGGBB`.
    pub const fn to_hex(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// Scales every channel by `factor` (clamped to `0.0..=1.0`).
    pub fn scale(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        let ch = |c: u8| (f32::from(c) * f).round() as u8;
        Self::new(ch(self.r), ch(self.g), ch(self.b))
    }

    /// The color with the DIM attribute applied.
    pub fn dim(self) -> Self {
        self.scale(DIM_FACTOR)
    }
}

impl From<ansi::Rgb> for Rgb {
    fn from(c: ansi::Rgb) -> Self {
        Self::new(c.r, c.g, c.b)
    }
}

impl From<Rgb> for ansi::Rgb {
    fn from(c: Rgb) -> Self {
        ansi::Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

/// The colors a terminal renders with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Palette {
    /// Default text color.
    pub foreground: Rgb,
    /// Default background color.
    pub background: Rgb,
    /// Cursor color.
    pub cursor: Rgb,
    /// The 16 ANSI colors: 0..8 normal, 8..16 bright.
    pub ansi: [Rgb; 16],
}

impl Default for Palette {
    /// A muted dark palette (One Dark inspired).
    fn default() -> Self {
        Self {
            foreground: Rgb::from_hex(0xabb2bf),
            background: Rgb::from_hex(0x1e2127),
            cursor: Rgb::from_hex(0x528bff),
            ansi: [
                Rgb::from_hex(0x282c34),
                Rgb::from_hex(0xe06c75),
                Rgb::from_hex(0x98c379),
                Rgb::from_hex(0xe5c07b),
                Rgb::from_hex(0x61afef),
                Rgb::from_hex(0xc678dd),
                Rgb::from_hex(0x56b6c2),
                Rgb::from_hex(0xabb2bf),
                Rgb::from_hex(0x5c6370),
                Rgb::from_hex(0xef8189),
                Rgb::from_hex(0xb5e090),
                Rgb::from_hex(0xf0d197),
                Rgb::from_hex(0x82c4f8),
                Rgb::from_hex(0xd79ce8),
                Rgb::from_hex(0x7bd0db),
                Rgb::from_hex(0xffffff),
            ],
        }
    }
}

impl Palette {
    /// Resolves an xterm 256-color index: 0..16 from the palette, 16..232 the
    /// 6x6x6 color cube, 232..256 the grayscale ramp.
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..16 => self.ansi[usize::from(index)],
            16..232 => {
                let i = index - 16;
                let level = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
                Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6))
            }
            _ => {
                let v = 8 + (index - 232) * 10;
                Rgb::new(v, v, v)
            }
        }
    }

    /// Resolves an alacritty color-table index (0..256 indexed colors, then
    /// foreground, background, cursor, dim colors, bright/dim foreground).
    pub(crate) fn table(&self, index: usize) -> Rgb {
        if let Ok(i) = u8::try_from(index) {
            return self.indexed(i);
        }
        let named = NamedColor::Foreground as usize;
        match index.checked_sub(named) {
            Some(0) | Some(11) => self.foreground,
            Some(1) => self.background,
            Some(2) => self.cursor,
            Some(n @ 3..=10) => self.ansi[n - 3].dim(),
            Some(12) => self.foreground.dim(),
            _ => self.foreground,
        }
    }
}

/// Resolves terminal colors against a [`Palette`], honoring colors the
/// running program overrode via OSC 4/10/11/12.
pub(crate) struct ColorResolver<'a> {
    palette: &'a Palette,
    overrides: Option<&'a Colors>,
}

impl<'a> ColorResolver<'a> {
    /// A resolver; `overrides` are the terminal's dynamic colors, if any.
    pub(crate) fn new(palette: &'a Palette, overrides: Option<&'a Colors>) -> Self {
        Self { palette, overrides }
    }

    fn lookup(&self, index: usize) -> Rgb {
        self.overrides
            .and_then(|c| c[index])
            .map(Rgb::from)
            .unwrap_or_else(|| self.palette.table(index))
    }

    /// Resolves a named color.
    pub(crate) fn named(&self, color: NamedColor) -> Rgb {
        self.lookup(color as usize)
    }

    /// Resolves an indexed color.
    pub(crate) fn indexed(&self, index: u8) -> Rgb {
        self.lookup(usize::from(index))
    }

    /// Resolves the cursor color.
    pub(crate) fn cursor(&self) -> Rgb {
        self.named(NamedColor::Cursor)
    }

    /// Resolves a cell's final `(foreground, background)`.
    pub(crate) fn cell_colors(&self, cell: &TermCell) -> (Rgb, Rgb) {
        let mut fg = self.foreground(cell.fg, cell.flags);
        let mut bg = self.background(cell.bg);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        (fg, bg)
    }

    fn foreground(&self, color: Color, flags: Flags) -> Rgb {
        let bold = flags.contains(Flags::BOLD);
        let dim = flags.contains(Flags::DIM);
        match color {
            Color::Spec(rgb) if dim => Rgb::from(rgb).dim(),
            Color::Spec(rgb) => rgb.into(),
            Color::Named(named) => {
                let named = if dim {
                    named.to_dim()
                } else if bold && is_bright_candidate(named) {
                    named.to_bright()
                } else {
                    named
                };
                self.named(named)
            }
            Color::Indexed(idx) if idx < 8 && dim => self.named(named_from_index(idx).to_dim()),
            Color::Indexed(idx) if idx < 8 && bold => self.indexed(idx + 8),
            Color::Indexed(idx) if dim => self.indexed(idx).dim(),
            Color::Indexed(idx) => self.indexed(idx),
        }
    }

    fn background(&self, color: Color) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb.into(),
            Color::Named(named) => self.named(named),
            Color::Indexed(idx) => self.indexed(idx),
        }
    }
}

/// Whether bold turns this named color into its bright variant.
fn is_bright_candidate(named: NamedColor) -> bool {
    (named as usize) < 8 || named == NamedColor::Foreground
}

fn named_from_index(idx: u8) -> NamedColor {
    match idx {
        0 => NamedColor::Black,
        1 => NamedColor::Red,
        2 => NamedColor::Green,
        3 => NamedColor::Yellow,
        4 => NamedColor::Blue,
        5 => NamedColor::Magenta,
        6 => NamedColor::Cyan,
        _ => NamedColor::White,
    }
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
