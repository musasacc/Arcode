use std::sync::Arc;

use arcode_config::{DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, ThemeRegistry};
use gpui::{AssetSource, px};

use super::raster::{device_px, rasterize};
use super::*;
use crate::{Assets, UiTheme};

/// The panel colours of Ember and Chalk as 0..1 RGB.
fn panels() -> Vec<(String, [f32; 3])> {
    let registry = ThemeRegistry::builtin();
    [DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME]
        .into_iter()
        .map(|name| {
            let theme = UiTheme::new(Arc::new(registry.get(name).unwrap().clone()));
            let c = theme.colors.panel().to_rgb();
            (name.to_owned(), [c.r, c.g, c.b])
        })
        .collect()
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    let lin = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Share of a logo's covered pixels that stand out from `bg` (contrast at
/// least 1.5 after blending onto it), drawn `side` pixels square.
fn visible_share(rgba: &[u8], bg: [f32; 3]) -> f32 {
    let (mut covered, mut visible) = (0usize, 0usize);
    for px in rgba.chunks_exact(4) {
        let a = px[3] as f32 / 255.;
        if a < 0.5 {
            continue;
        }
        covered += 1;
        let c = [0, 1, 2].map(|i| px[i] as f32 / 255. * a + bg[i] * (1. - a));
        if contrast(c, bg) >= 1.5 {
            visible += 1;
        }
    }
    visible as f32 / covered.max(1) as f32
}

#[test]
fn every_logo_is_embedded_small_and_draws() {
    for &logo in FileLogo::ALL {
        let bytes = Assets
            .load(logo.path())
            .unwrap()
            .unwrap_or_else(|| panic!("{}", logo.path()));
        assert!(
            bytes.len() < 9 * 1024,
            "{}: {} bytes",
            logo.path(),
            bytes.len()
        );
        let text = std::str::from_utf8(&bytes).expect("utf-8");
        assert!(
            text.starts_with("<svg"),
            "{}: no metadata before <svg>",
            logo.path()
        );
        for junk in ["<metadata", "<!--", "inkscape:", "sodipodi:", "<title"] {
            assert!(!text.contains(junk), "{}: {junk}", logo.path());
        }
        for side in [14, 15, 16, 28, 30, 32] {
            let rgba = rasterize(&bytes, side).unwrap_or_else(|| panic!("{}", logo.path()));
            assert_eq!(rgba.len(), (side * side * 4) as usize);
            let covered = rgba.chunks_exact(4).filter(|p| p[3] > 32).count();
            assert!(
                covered as u32 >= side * side / 16,
                "{} at {side}px covers only {covered} pixels",
                logo.path()
            );
        }
    }
    let listed = Assets.list("file-icons/").unwrap();
    assert_eq!(listed.len(), FileLogo::ALL.len());
}

#[test]
fn logo_names_and_paths_are_unique() {
    for (i, a) in FileLogo::ALL.iter().enumerate() {
        for b in &FileLogo::ALL[i + 1..] {
            assert_ne!(a.path(), b.path());
            assert_ne!(a.name(), b.name());
        }
    }
}

/// Colour logos must stay visible on Ember's dark and Chalk's light panels:
/// a good part of each (at least 15 % of what it covers, such as the dark
/// letters on JavaScript's yellow square) reaches 1.5:1 against the panel.
/// Black and very dark logos fail this on Ember and are drawn tinted.
#[test]
fn colour_logos_stand_out_on_both_themes() {
    let panels = panels();
    for &logo in FileLogo::ALL {
        let bytes = Assets.load(logo.path()).unwrap().unwrap();
        let rgba = rasterize(&bytes, 32).unwrap();
        for (theme, bg) in &panels {
            let share = visible_share(&rgba, *bg);
            match logo.style() {
                LogoStyle::Colour => assert!(
                    share >= 0.15,
                    "{} on {theme}: only {:.0}% of it stands out",
                    logo.name(),
                    share * 100.
                ),
                LogoStyle::Tinted => {}
            }
        }
    }
}

/// A tinted logo loses nothing by being drawn in one colour: it has only
/// one (black or a single grey), and in its own colour it would vanish on
/// Ember or be no more than grey.
#[test]
fn tinted_logos_are_single_colour() {
    for &logo in FileLogo::ALL {
        if logo.style() != LogoStyle::Tinted {
            continue;
        }
        let bytes = Assets.load(logo.path()).unwrap().unwrap();
        let rgba = rasterize(&bytes, 32).unwrap();
        let mut opaque = rgba.chunks_exact(4).filter(|p| p[3] > 200);
        let first = opaque.next().unwrap_or_else(|| panic!("{}", logo.path()));
        for p in opaque {
            for i in 0..3 {
                assert!(
                    first[i].abs_diff(p[i]) <= 8,
                    "{} has more than one colour",
                    logo.name()
                );
            }
        }
        let [r, g, b] = [first[0], first[1], first[2]].map(|c| c as f32 / 255.);
        assert!(luminance([r, g, b]) < 0.1, "{} is not dark", logo.name());
    }
}

#[test]
fn device_pixels_follow_the_scale_factor() {
    assert_eq!(device_px(px(16.), 1.), 16);
    assert_eq!(device_px(px(15.), 2.), 30);
    assert_eq!(device_px(px(14.), 1.5), 21);
    assert_eq!(device_px(px(0.), 1.), 1);
}

#[test]
fn rasterising_keeps_the_aspect_and_centres() {
    // A 2:1 bar fills the width and sits in the vertical middle.
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10"><rect width="20" height="10" fill="#ff0000"/></svg>"##;
    let rgba = rasterize(svg, 16).unwrap();
    let alpha = |x: usize, y: usize| rgba[(y * 16 + x) * 4 + 3];
    assert_eq!(alpha(0, 0), 0);
    assert_eq!(alpha(8, 8), 255);
    assert_eq!(alpha(0, 8), 255);
    assert_eq!(&rgba[(8 * 16 + 8) * 4..][..3], &[255, 0, 0]);
    assert!(rasterize(b"not svg", 16).is_none());
}

#[test]
fn file_icon_carries_its_glyph_and_tint() {
    let el = file_glyph(FileLogo::Rust).size(px(15.));
    assert_eq!(el.glyph(), FileGlyph::Logo(FileLogo::Rust));
    assert_eq!(el.tint(), None);
    let red = gpui::red();
    let el = file_glyph(IconName::File).color(red);
    assert_eq!(el.glyph(), FileGlyph::Icon(IconName::File));
    assert_eq!(el.tint(), Some(red));
}

/// Writes PPM contact sheets of every SVG in `$ARCODE_LOGO_SHEET` on the
/// Ember and Chalk panels, for looking at them; and prints each logo's
/// visible share.
#[test]
#[ignore]
fn contact_sheet() {
    let dir = std::env::var("ARCODE_LOGO_SHEET").expect("ARCODE_LOGO_SHEET");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    files.sort();
    let sizes = [14u32, 16, 32];
    let cell = 40usize;
    let cols = sizes.len();
    for (theme, bg) in panels() {
        let (w, h) = (cell * cols, cell * files.len());
        let mut img = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                for i in 0..3 {
                    img[(y * w + x) * 3 + i] = (bg[i] * 255.) as u8;
                }
            }
        }
        for (row, path) in files.iter().enumerate() {
            let bytes = std::fs::read(path).unwrap();
            for (col, &side) in sizes.iter().enumerate() {
                let rgba = rasterize(&bytes, side).unwrap();
                if side == 32 {
                    println!(
                        "{theme:14} {:28} {:.2}",
                        path.file_name().unwrap().to_string_lossy(),
                        visible_share(&rgba, bg)
                    );
                }
                for y in 0..side as usize {
                    for x in 0..side as usize {
                        let p = &rgba[(y * side as usize + x) * 4..][..4];
                        let a = p[3] as f32 / 255.;
                        let (ox, oy) = (col * cell + 4 + x, row * cell + 4 + y);
                        for i in 0..3 {
                            let o = &mut img[(oy * w + ox) * 3 + i];
                            *o = (p[i] as f32 * a + *o as f32 * (1. - a)) as u8;
                        }
                    }
                }
            }
        }
        let mut out = format!("P6 {w} {h} 255\n").into_bytes();
        out.extend(img);
        let name = theme.replace(' ', "-").to_lowercase();
        std::fs::write(format!("{dir}/sheet-{name}.ppm"), out).unwrap();
    }
}
