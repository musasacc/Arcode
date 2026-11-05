//! Telling a tap on Alt from Alt held for something else. GPUI reports a
//! bare `alt` keystroke whenever Alt goes down and up with no key pressed
//! in between, even when the mouse was used meanwhile. Alt+click adds a
//! cursor, Alt+Shift+drag selects a column, and many Linux window managers
//! move or resize the window on Alt+drag: none of these may activate the
//! menu bar, or the next letters typed would pick menu items instead of
//! reaching the editor.

use gpui::{Bounds, Modifiers, Pixels};

/// What happened while Alt was held.
#[derive(Debug, Default)]
pub struct AltTap {
    /// The window's bounds when Alt went down, while it is held.
    down: Option<Bounds<Pixels>>,
    /// A mouse button went down or up while Alt was held.
    used: bool,
}

impl AltTap {
    /// The modifiers changed to `modifiers`; the window is at `bounds`.
    pub fn modifiers(&mut self, modifiers: Modifiers, bounds: Bounds<Pixels>) {
        if !modifiers.alt {
            *self = AltTap::default();
        } else if self.down.is_none() {
            self.down = Some(bounds);
            self.used = false;
        }
    }

    /// A mouse button went down or up with `modifiers` held.
    pub fn mouse(&mut self, modifiers: Modifiers) {
        if modifiers.alt {
            self.used = true;
        }
    }

    /// Alt went up with no key pressed while it was held: whether that was
    /// a tap, with the window now at `bounds` (it moved: the window manager
    /// took the drag). Forgets the press.
    pub fn take_tap(&mut self, bounds: Bounds<Pixels>) -> bool {
        let moved = self.down.is_some_and(|down| down != bounds);
        let tap = !self.used && !moved;
        *self = AltTap::default();
        tap
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, point, px, size};

    use super::*;

    fn at(x: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(0.)), size(px(800.), px(600.)))
    }

    #[test]
    fn a_plain_tap_counts() {
        let mut tap = AltTap::default();
        tap.modifiers(Modifiers::alt(), at(0.));
        assert!(tap.take_tap(at(0.)));
        // Without a press seen first it still counts.
        assert!(tap.take_tap(at(0.)));
    }

    #[test]
    fn the_mouse_spoils_the_tap() {
        let mut tap = AltTap::default();
        tap.modifiers(Modifiers::alt(), at(0.));
        tap.mouse(Modifiers::alt());
        assert!(!tap.take_tap(at(0.)), "Alt+click");
        // The next press starts afresh.
        tap.modifiers(Modifiers::alt(), at(0.));
        assert!(tap.take_tap(at(0.)));
        // Alt+Shift+drag, Shift released first.
        let alt_shift = Modifiers {
            alt: true,
            shift: true,
            ..Modifiers::default()
        };
        tap.modifiers(Modifiers::alt(), at(0.));
        tap.modifiers(alt_shift, at(0.));
        tap.mouse(alt_shift);
        tap.modifiers(Modifiers::alt(), at(0.));
        assert!(!tap.take_tap(at(0.)), "column selection");
        // A click without Alt does not count.
        tap.modifiers(Modifiers::alt(), at(0.));
        tap.mouse(Modifiers::none());
        assert!(tap.take_tap(at(0.)));
    }

    #[test]
    fn a_window_moved_by_the_window_manager_is_not_a_tap() {
        let mut tap = AltTap::default();
        tap.modifiers(Modifiers::alt(), at(0.));
        assert!(!tap.take_tap(at(40.)));
        // Releasing Alt forgets the press.
        tap.modifiers(Modifiers::alt(), at(0.));
        tap.modifiers(Modifiers::none(), at(40.));
        tap.modifiers(Modifiers::alt(), at(40.));
        assert!(tap.take_tap(at(40.)));
    }
}
