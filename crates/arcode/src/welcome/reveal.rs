//! Keeping the row picked with Up / Down on screen: the selected row
//! carries an invisible marker that, once drawn, scrolls the welcome
//! screen just enough to show the row. It acts once per key press, so it
//! never fights the mouse wheel.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Bounds, Pixels, ScrollHandle, canvas, point, prelude::*, px};

/// Room left between a revealed row and the edge of the screen.
const MARGIN: f32 = 12.;

/// The scroll offset (`offset_y`, negative when scrolled down, at least
/// `-max_y`) that brings `row` into `view`, or `None` when it is already
/// fully visible. Both are window bounds with the current offset applied.
pub fn offset_to_show(
    view: Bounds<Pixels>,
    row: Bounds<Pixels>,
    offset_y: Pixels,
    max_y: Pixels,
) -> Option<Pixels> {
    let margin = px(MARGIN);
    let top = view.top() + margin;
    let bottom = view.bottom() - margin;
    let shift = if row.top() < top {
        top - row.top()
    } else if row.bottom() > bottom {
        // Never past the top: a row taller than the view shows its top.
        (bottom - row.bottom()).max(top - row.top())
    } else {
        return None;
    };
    let target = (offset_y + shift).clamp(-max_y, px(0.));
    (target != offset_y).then_some(target)
}

/// An invisible element covering the selected row. When `pending` is set
/// it scrolls `scroll` to show the row and asks for one more frame.
pub fn marker(scroll: ScrollHandle, pending: Rc<Cell<bool>>) -> impl IntoElement {
    canvas(
        move |row, window, _| {
            if !pending.replace(false) {
                return;
            }
            let offset = scroll.offset();
            let max = scroll.max_offset().height;
            if let Some(y) = offset_to_show(scroll.bounds(), row, offset.y, max) {
                scroll.set_offset(point(offset.x, y));
                window.on_next_frame(|window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::offset_to_show;

    fn rect(top: f32, height: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(100.), px(height)))
    }

    #[test]
    fn rows_are_scrolled_into_view_by_the_least_amount() {
        let view = rect(0., 500.);
        let max = px(1000.);
        // Fully visible: nothing to do.
        assert_eq!(offset_to_show(view, rect(100., 40.), px(0.), max), None);
        // Below the view: scroll down so it sits at the bottom edge.
        assert_eq!(
            offset_to_show(view, rect(700., 40.), px(0.), max),
            Some(px(-252.))
        );
        // Above the view: scroll up so it sits at the top edge.
        assert_eq!(
            offset_to_show(view, rect(-100., 40.), px(-300.), max),
            Some(px(-188.))
        );
        // Never past the ends.
        assert_eq!(
            offset_to_show(view, rect(-100., 40.), px(-50.), max),
            Some(px(0.))
        );
        assert_eq!(
            offset_to_show(view, rect(2000., 40.), px(0.), max),
            Some(px(-1000.))
        );
        // Already as far as it goes.
        assert_eq!(offset_to_show(view, rect(5., 40.), px(0.), max), None);
    }
}
