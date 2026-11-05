//! Placing a submenu beside the row it opens from. The submenus are drawn
//! after their parent menu in the same overlay layer, not inside the row,
//! so they paint over the parent's later rows and take the mouse wherever
//! the two overlap. The row's bounds are only known once the parent is
//! laid out, so a [`BesideRow`] element reads them while it prepaints.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Display, Element, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, Position, Size, Style, Window, point, px,
};

/// Where the row a submenu opens from was drawn this frame.
pub type RowAnchor = Rc<Cell<Option<Bounds<Pixels>>>>;

/// The popover's padding plus its edge: a submenu at `row.top - INSET`
/// has its first row level with `row`.
const INSET: f32 = 5.;

/// Where a submenu of `size` opening from `row` goes: right of the row,
/// its first row level with it; left of the parent menu when the window
/// has no room on the right; over the parent, inside the window, when
/// neither side has room. Never closer than `margin` to a window edge.
pub fn submenu_origin(
    row: Bounds<Pixels>,
    size: Size<Pixels>,
    viewport: Size<Pixels>,
    margin: Pixels,
) -> Point<Pixels> {
    let max_x = viewport.width - margin - size.width;
    let right = row.right();
    let left = row.left() - size.width;
    let x = if right <= max_x {
        right
    } else if left >= margin {
        left
    } else {
        // Neither side fits: the side with more room, kept inside.
        let room_right = viewport.width - row.right();
        let room_left = row.left();
        let x = if room_right >= room_left { right } else { left };
        x.min(max_x).max(margin)
    };
    let max_y = viewport.height - margin - size.height;
    let y = (row.top() - px(INSET)).min(max_y).max(margin);
    point(x.round(), y.round())
}

/// Draws its child beside the row recorded in `anchor` (see
/// [`submenu_origin`]).
pub struct BesideRow {
    anchor: RowAnchor,
    margin: Pixels,
    child: AnyElement,
}

/// A submenu placed beside the row in `anchor`, `margin` from the window
/// edges.
pub fn beside_row(anchor: RowAnchor, margin: Pixels, child: impl IntoElement) -> BesideRow {
    BesideRow {
        anchor,
        margin,
        child: child.into_any_element(),
    }
}

impl IntoElement for BesideRow {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BesideRow {
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, LayoutId) {
        let child = self.child.request_layout(window, cx);
        let style = Style {
            position: Position::Absolute,
            display: Display::Flex,
            ..Style::default()
        };
        (window.request_layout(style, [child], cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut LayoutId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid_out = window.layout_bounds(*child);
        let offset = match self.anchor.get() {
            Some(row) => {
                let origin =
                    submenu_origin(row, laid_out.size, window.viewport_size(), self.margin);
                origin - laid_out.origin
            }
            None => Point::default(),
        };
        window.with_element_offset(offset, |window| self.child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut LayoutId,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::submenu_origin;

    fn row(x: f32, y: f32, w: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(28.)))
    }

    #[test]
    fn submenus_open_right_then_left_then_inside() {
        let viewport = size(px(1000.), px(600.));
        let sub = size(px(240.), px(200.));
        let m = px(6.);
        // Room on the right: level with the row.
        assert_eq!(
            submenu_origin(row(100., 100., 250.), sub, viewport, m),
            point(px(350.), px(95.))
        );
        // No room on the right: left of the parent.
        assert_eq!(
            submenu_origin(row(600., 100., 250.), sub, viewport, m),
            point(px(360.), px(95.))
        );
        // Neither: inside the window on the roomier side.
        let narrow = size(px(480.), px(600.));
        let at = submenu_origin(row(200., 100., 250.), sub, narrow, m);
        assert_eq!(at, point(px(6.), px(95.)));
        let at = submenu_origin(row(10., 100., 250.), sub, narrow, m);
        assert_eq!(at, point(px(234.), px(95.)));
        // Near the bottom it moves up, never above the margin.
        assert_eq!(
            submenu_origin(row(100., 500., 250.), sub, viewport, m).y,
            px(394.)
        );
        let tall = size(px(240.), px(900.));
        assert_eq!(
            submenu_origin(row(100., 500., 250.), tall, viewport, m).y,
            px(6.)
        );
    }
}
