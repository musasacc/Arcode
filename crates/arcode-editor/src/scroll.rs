//! Scroll math for a layout pass: autoscroll targets and scrollbar
//! geometry.

use gpui::{Bounds, Pixels, point, px, size};

use crate::api::ScrollAnchor;
use crate::layout::{SCROLLBAR_WIDTH, ScrollbarLayout};

/// New vertical scroll offset that shows `rows` display rows starting at
/// `row`, placed by `mode`.
pub(crate) fn vertical_autoscroll(
    mode: ScrollAnchor,
    scroll_y: Pixels,
    row: usize,
    rows: usize,
    lh: Pixels,
    viewport_h: Pixels,
) -> Pixels {
    let top = lh * row as f32;
    let bottom = top + lh * rows as f32;
    let visible = top >= scroll_y && bottom <= scroll_y + viewport_h;
    match mode {
        ScrollAnchor::Top => top.max(Pixels::ZERO),
        ScrollAnchor::Center if !visible => (top - (viewport_h - lh) * 0.5).max(Pixels::ZERO),
        ScrollAnchor::Center => scroll_y,
        ScrollAnchor::Fit => {
            let margin = (lh * 2.).min((viewport_h - lh).max(Pixels::ZERO) * 0.5);
            if top - margin < scroll_y {
                (top - margin).max(Pixels::ZERO)
            } else if bottom + margin > scroll_y + viewport_h {
                bottom + margin - viewport_h
            } else {
                scroll_y
            }
        }
    }
}

/// Width of the scrollbar thumb.
const THUMB_WIDTH: Pixels = px(6.);
/// Space between the thumb and the track's edges.
pub(crate) const THUMB_INSET: Pixels = px(3.);

pub(crate) fn scrollbar_layout(
    bounds: Bounds<Pixels>,
    scroll_y: Pixels,
    max_scroll_y: Pixels,
    viewport_h: Pixels,
) -> Option<ScrollbarLayout> {
    if max_scroll_y <= Pixels::ZERO {
        return None;
    }
    let track = Bounds::new(
        point(bounds.right() - SCROLLBAR_WIDTH, bounds.top()),
        size(SCROLLBAR_WIDTH, bounds.size.height),
    );
    // A 6 px thumb inset 3 px from the track's edges.
    let inset = THUMB_INSET;
    let room = (track.size.height - inset * 2.).max(px(1.));
    let content = max_scroll_y + viewport_h;
    let thumb_h = (room * (viewport_h / content)).max(px(24.)).min(room);
    let frac = scroll_y / max_scroll_y;
    let thumb_y = track.top() + inset + (room - thumb_h) * frac.clamp(0., 1.);
    Some(ScrollbarLayout {
        track,
        thumb: Bounds::new(
            point(track.right() - inset - THUMB_WIDTH, thumb_y),
            size(THUMB_WIDTH, thumb_h),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoscroll_modes() {
        let (lh, vh) = (px(20.), px(200.));
        // Line 50 at the top / centered / fitted from far above.
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Top, px(0.), 50, 1, lh, vh),
            px(1000.)
        );
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Center, px(0.), 50, 1, lh, vh),
            px(910.)
        );
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Fit, px(0.), 50, 1, lh, vh),
            px(860.)
        );
        // Already visible: center and fit keep the offset.
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Center, px(900.), 50, 1, lh, vh),
            px(900.)
        );
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Fit, px(900.), 50, 1, lh, vh),
            px(900.)
        );
        // Two rows (a lens row above the line) are kept in view together.
        assert_eq!(
            vertical_autoscroll(ScrollAnchor::Fit, px(0.), 49, 2, lh, vh),
            px(860.)
        );
    }

    #[test]
    fn scrollbar_only_when_scrollable() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(200.)));
        assert!(scrollbar_layout(bounds, px(0.), px(0.), px(200.)).is_none());
        let bar = scrollbar_layout(bounds, px(100.), px(200.), px(200.)).expect("bar");
        assert_eq!(bar.track.left(), px(400.) - SCROLLBAR_WIDTH);
        assert!(bar.thumb.top() > px(0.) && bar.thumb.bottom() < px(200.));
        // 6 px wide, 3 px from the right edge and from the top at scroll 0.
        assert_eq!(bar.thumb.size.width, px(6.));
        assert_eq!(bar.thumb.right(), px(397.));
        let top = scrollbar_layout(bounds, px(0.), px(200.), px(200.)).expect("bar");
        assert_eq!(top.thumb.top(), px(3.));
    }
}
