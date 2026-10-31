//! Conversions between theme colors, terminal colors and GPUI colors.

use arcode_config::{Color, TerminalColors};
use arcode_terminal::{Palette, Rgb};
use gpui::{Hsla, Rgba};

/// Converts an opaque terminal color into a GPUI color.
pub fn rgb_to_hsla(c: Rgb) -> Hsla {
    Rgba {
        r: f32::from(c.r) / 255.,
        g: f32::from(c.g) / 255.,
        b: f32::from(c.b) / 255.,
        a: 1.,
    }
    .into()
}

/// Converts a theme color into a terminal color, dropping alpha.
pub fn color_to_rgb(c: Color) -> Rgb {
    Rgb::new(c.r, c.g, c.b)
}

/// Builds the terminal palette from a theme's `[terminal]` colors.
pub fn palette_from_theme(colors: &TerminalColors) -> Palette {
    Palette {
        foreground: color_to_rgb(colors.foreground),
        background: color_to_rgb(colors.background),
        cursor: color_to_rgb(colors.cursor),
        ansi: colors.ansi.map(color_to_rgb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcode_config::{Appearance, ThemeRegistry};

    #[test]
    fn hsla_of_primaries() {
        let white = rgb_to_hsla(Rgb::new(255, 255, 255));
        assert!((white.l - 1.).abs() < 1e-4 && (white.a - 1.).abs() < 1e-6);
        let black = rgb_to_hsla(Rgb::new(0, 0, 0));
        assert!(black.l.abs() < 1e-4);
        let red = rgb_to_hsla(Rgb::new(255, 0, 0));
        assert!(red.h.abs() < 1e-4 && (red.s - 1.).abs() < 1e-4 && (red.l - 0.5).abs() < 1e-4);
        let back: Rgba = rgb_to_hsla(Rgb::from_hex(0x336699)).into();
        assert_eq!(
            (
                (back.r * 255.).round() as u8,
                (back.g * 255.).round() as u8,
                (back.b * 255.).round() as u8
            ),
            (0x33, 0x66, 0x99)
        );
    }

    #[test]
    fn color_drops_alpha() {
        let c = Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        };
        assert_eq!(color_to_rgb(c), Rgb::new(1, 2, 3));
    }

    #[test]
    fn palette_matches_theme() {
        let registry = ThemeRegistry::builtin();
        let theme = registry.fallback(Appearance::Dark);
        let t = &theme.terminal;
        let p = palette_from_theme(t);
        assert_eq!(p.foreground, color_to_rgb(t.foreground));
        assert_eq!(p.background, color_to_rgb(t.background));
        assert_eq!(p.cursor, color_to_rgb(t.cursor));
        for i in 0..16 {
            assert_eq!(p.ansi[i], color_to_rgb(t.ansi[i]));
        }
    }
}
