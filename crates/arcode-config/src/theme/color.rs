//! RGBA colors written as hex strings (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB color with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel (255 = opaque).
    pub a: u8,
}

/// Error returned when a color string is not valid hex notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError(String);

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid color {:?} (expected #rgb, #rgba, #rrggbb or #rrggbbaa)",
            self.0
        )
    }
}

impl std::error::Error for ColorParseError {}

impl Color {
    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Fully transparent black.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A color with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// From `0xRRGGBBAA`.
    pub const fn from_u32(rgba: u32) -> Self {
        let [r, g, b, a] = rgba.to_be_bytes();
        Self { r, g, b, a }
    }

    /// As `0xRRGGBBAA` (the layout GPUI's `rgba()` expects).
    pub const fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }

    /// As `0xRRGGBB`, dropping alpha (the layout GPUI's `rgb()` expects).
    pub const fn to_rgb_u32(self) -> u32 {
        u32::from_be_bytes([0, self.r, self.g, self.b])
    }

    /// Channels scaled to `0.0..=1.0` as `[r, g, b, a]`.
    pub fn to_f32_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| f32::from(c) / 255.0)
    }

    /// The same color with a different alpha.
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Hex notation: `#rrggbb` when opaque, else `#rrggbbaa`.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:08x}", self.to_u32())
        }
    }

    /// Alpha-composites `self` over an opaque `background`.
    pub fn over(self, background: Color) -> Color {
        let a = u16::from(self.a);
        let mix = |fg: u8, bg: u8| {
            let value = (u16::from(fg) * a + u16::from(bg) * (255 - a) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        Color::rgb(
            mix(self.r, background.r),
            mix(self.g, background.g),
            mix(self.b, background.b),
        )
    }

    /// WCAG relative luminance (alpha ignored).
    pub fn relative_luminance(self) -> f32 {
        let linear = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio between two colors (1.0 ..= 21.0, alpha ignored).
    pub fn contrast_ratio(self, other: Color) -> f32 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ColorParseError(s.to_owned());
        let hex = s.trim().strip_prefix('#').ok_or_else(err)?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err());
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).map_err(|_| err());
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        match hex.len() {
            3 | 4 => {
                let short = |i: usize| digit(i).map(|d| d * 17);
                let a = if hex.len() == 4 { short(3)? } else { 255 };
                Ok(Color::rgba(short(0)?, short(1)?, short(2)?, a))
            }
            6 | 8 => {
                let a = if hex.len() == 8 { pair(6)? } else { 255 };
                Ok(Color::rgba(pair(0)?, pair(2)?, pair(4)?, a))
            }
            _ => Err(err()),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_forms() {
        assert_eq!("#fff".parse(), Ok(Color::WHITE));
        assert_eq!("#f008".parse(), Ok(Color::rgba(255, 0, 0, 0x88)));
        assert_eq!("#1e1f22".parse(), Ok(Color::rgb(0x1e, 0x1f, 0x22)));
        assert_eq!("#5B9CF54D".parse(), Ok(Color::rgba(0x5b, 0x9c, 0xf5, 0x4d)));
        assert_eq!(" #000000 ".parse(), Ok(Color::BLACK));
    }

    #[test]
    fn rejects_invalid() {
        for bad in [
            "",
            "#",
            "fff",
            "#ff",
            "#fffff",
            "#ggg",
            "#12345678a",
            "#+12",
            "#é12",
        ] {
            assert!(bad.parse::<Color>().is_err(), "{bad:?} should fail");
        }
        let msg = "nope".parse::<Color>().unwrap_err().to_string();
        assert!(msg.contains("nope"));
    }

    #[test]
    fn u32_and_hex_conversions() {
        let c = Color::rgba(0x12, 0x34, 0x56, 0x78);
        assert_eq!(c.to_u32(), 0x1234_5678);
        assert_eq!(c.to_rgb_u32(), 0x0012_3456);
        assert_eq!(Color::from_u32(0x1234_5678), c);
        assert_eq!(c.to_hex(), "#12345678");
        assert_eq!(c.with_alpha(255).to_hex(), "#123456");
        assert_eq!(c.to_string(), "#12345678");
        assert_eq!(Color::WHITE.to_f32_array(), [1.0; 4]);
    }

    #[test]
    fn serde_uses_strings() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct W {
            c: Color,
        }
        let w: W = toml::from_str("c = \"#abc\"").unwrap();
        assert_eq!(w.c, Color::rgb(0xaa, 0xbb, 0xcc));
        assert_eq!(toml::to_string(&w).unwrap().trim(), "c = \"#aabbcc\"");
        assert!(toml::from_str::<W>("c = \"red\"").is_err());
    }

    #[test]
    fn contrast_and_compositing() {
        let ratio = Color::BLACK.contrast_ratio(Color::WHITE);
        assert!((ratio - 21.0).abs() < 0.01);
        assert!((Color::WHITE.contrast_ratio(Color::WHITE) - 1.0).abs() < 1e-6);
        let half = Color::rgba(255, 255, 255, 128).over(Color::BLACK);
        assert_eq!(half, Color::rgb(128, 128, 128));
        assert_eq!(Color::TRANSPARENT.over(Color::WHITE), Color::WHITE);
    }
}
