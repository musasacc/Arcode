//! Hue, saturation and value, the coordinates of the theme editor's colour
//! picker (a saturation/value square and a hue strip).

use super::Color;

/// A colour as hue (degrees, `0.0..360.0`), saturation and value
/// (`0.0..=1.0`), plus straight alpha (`0.0..=1.0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    /// Hue in degrees.
    pub h: f32,
    /// Saturation.
    pub s: f32,
    /// Value (brightness).
    pub v: f32,
    /// Alpha.
    pub a: f32,
}

impl Hsv {
    /// The HSV coordinates of `color`. Greys get hue 0.
    pub fn from_color(color: Color) -> Self {
        let [r, g, b, a] = color.to_f32_array();
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let h = if delta <= f32::EPSILON {
            0.0
        } else if max == r {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        let s = if max <= f32::EPSILON {
            0.0
        } else {
            delta / max
        };
        Self {
            h: h.rem_euclid(360.0),
            s,
            v: max,
            a,
        }
    }

    /// The colour, rounded to 8 bits per channel. Out-of-range inputs are
    /// clamped (the hue wraps).
    pub fn to_color(self) -> Color {
        let h = self.h.rem_euclid(360.0) / 60.0;
        let s = self.s.clamp(0.0, 1.0);
        let v = self.v.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        let byte = |f: f32| ((f + m).clamp(0.0, 1.0) * 255.0).round() as u8;
        let alpha = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color::rgba(byte(r), byte(g), byte(b), alpha)
    }

    /// The fully saturated, full-value colour of this hue (the corner of
    /// the picker's square).
    pub fn hue_color(self) -> Color {
        Hsv {
            h: self.h,
            s: 1.0,
            v: 1.0,
            a: 1.0,
        }
        .to_color()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primaries_and_greys() {
        let cases = [
            (Color::rgb(255, 0, 0), 0.0, 1.0, 1.0),
            (Color::rgb(0, 255, 0), 120.0, 1.0, 1.0),
            (Color::rgb(0, 0, 255), 240.0, 1.0, 1.0),
            (Color::rgb(255, 255, 0), 60.0, 1.0, 1.0),
            (Color::rgb(255, 0, 255), 300.0, 1.0, 1.0),
            (Color::WHITE, 0.0, 0.0, 1.0),
            (Color::BLACK, 0.0, 0.0, 0.0),
        ];
        for (color, h, s, v) in cases {
            let hsv = Hsv::from_color(color);
            assert!((hsv.h - h).abs() < 1e-3, "{color}: {hsv:?}");
            assert!((hsv.s - s).abs() < 1e-3, "{color}: {hsv:?}");
            assert!((hsv.v - v).abs() < 1e-3, "{color}: {hsv:?}");
            assert_eq!(hsv.to_color(), color);
        }
    }

    #[test]
    fn every_8_bit_colour_round_trips() {
        // Every channel step of 5 (52^3 colours) and a few alphas.
        for r in (0..=255).step_by(5) {
            for g in (0..=255).step_by(5) {
                for b in (0..=255).step_by(5) {
                    for a in [0, 77, 255] {
                        let color = Color::rgba(r as u8, g as u8, b as u8, a);
                        assert_eq!(Hsv::from_color(color).to_color(), color);
                    }
                }
            }
        }
        // The Ember accent and a translucent selection.
        for hex in ["#f0a24a", "#5b9cf54d", "#0b0a09", "#fbf6ee"] {
            let color: Color = hex.parse().unwrap();
            assert_eq!(Hsv::from_color(color).to_color(), color, "{hex}");
        }
    }

    #[test]
    fn out_of_range_values_clamp_and_hue_wraps() {
        let hsv = Hsv {
            h: 480.0,
            s: 2.0,
            v: -1.0,
            a: 3.0,
        };
        assert_eq!(hsv.to_color(), Color::BLACK);
        let wrapped = Hsv {
            h: -240.0,
            s: 1.0,
            v: 1.0,
            a: 1.0,
        };
        assert_eq!(wrapped.to_color(), Color::rgb(0, 255, 0));
        assert_eq!(
            Hsv::from_color(Color::rgb(0, 128, 0)).hue_color(),
            Color::rgb(0, 255, 0)
        );
    }
}
