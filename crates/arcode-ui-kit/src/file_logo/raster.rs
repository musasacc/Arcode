//! Full-colour logos rasterised with resvg, once per logo and device size.
//!
//! GPUI's `img()` draws an SVG at twice its own size and lets the GPU scale
//! it, which blurs a 15 px icon at 1x. Here every logo is drawn at exactly
//! the device pixels it covers and kept: a frame only looks the image up,
//! and GPUI's sprite atlas uploads it once.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, AssetSource, Global, Pixels, RenderImage};
use resvg::{tiny_skia, usvg};

use super::FileLogo;

/// The rasterised logos of an app by logo and side length in device
/// pixels; `None` for one that failed to load (logged once, drawn as the
/// plain icon).
#[derive(Default)]
struct LogoImages {
    images: HashMap<(FileLogo, u32), Option<Arc<RenderImage>>>,
    rasterized: usize,
}

impl Global for LogoImages {}

/// How many logo images this app rasterised so far (each logo once per
/// device size). A frame that draws only logos drawn before leaves it
/// unchanged.
pub fn logo_rasterizations(cx: &App) -> usize {
    cx.try_global::<LogoImages>().map_or(0, |l| l.rasterized)
}

/// Side length in device pixels of an icon `size` wide at `scale_factor`.
pub(crate) fn device_px(size: Pixels, scale_factor: f32) -> u32 {
    (f32::from(size) * scale_factor).round().clamp(1., 512.) as u32
}

/// The image of `logo`, `side` device pixels square.
pub(crate) fn logo_image(logo: FileLogo, side: u32, cx: &mut App) -> Option<Arc<RenderImage>> {
    let cache = cx.default_global::<LogoImages>();
    let rasterized = &mut cache.rasterized;
    cache
        .images
        .entry((logo, side))
        .or_insert_with(|| {
            let image = crate::Assets
                .load(logo.path())
                .ok()
                .flatten()
                .and_then(|bytes| rasterize(&bytes, side))
                .map(|rgba| Arc::new(to_render_image(rgba, side)));
            if image.is_none() {
                log::warn!("file icon {} could not be drawn", logo.path());
            }
            *rasterized += 1;
            image
        })
        .clone()
}

/// RGBA pixels (straight alpha, rows top to bottom) of the SVG `bytes`
/// drawn into a `side` x `side` square, scaled to fit and centred.
pub(crate) fn rasterize(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let size = tree.size();
    let side_f = side as f32;
    let scale = side_f / size.width().max(size.height());
    let dx = (side_f - size.width() * scale) / 2.;
    let dy = (side_f - size.height() * scale) / 2.;
    let transform = tiny_skia::Transform::from_row(scale, 0., 0., scale, dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(
        pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect(),
    )
}

/// A GPUI image (which stores BGRA) of the RGBA `pixels`.
fn to_render_image(mut pixels: Vec<u8>, side: u32) -> RenderImage {
    for px in pixels.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(side, side, pixels).expect("buffer matches its size");
    RenderImage::new(vec![image::Frame::new(buffer)])
}
