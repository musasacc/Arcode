//! Window position on X11. GPUI 0.2.2 creates X11 windows without a
//! position hint, so window managers place them themselves and a saved
//! position is lost; it also offers no X window id. Right after a window
//! is created this finds it (our process id plus its title, searched on a
//! background thread over a connection of our own), sets `USPosition`
//! with static gravity in `WM_NORMAL_HINTS` (keeping GPUI's minimum size)
//! and moves it, so its client area lands where it was. Wayland does not
//! let clients place windows; nothing is done there, nor on macOS and
//! Windows, where GPUI places windows from their bounds itself.

use gpui::Window;

/// Moves the window titled `title` to `(x, y)` (logical pixels, client
/// area) on X11. `same_title` is how many of our windows (this one
/// included) carry that title: the search waits until it sees them all and
/// takes the newest (X ids of one connection only grow), so two windows of
/// the same title never swap places.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn place(window: &Window, title: String, same_title: usize, x: f32, y: f32) {
    // Only X11 sessions; on Wayland GPUI uses its Wayland backend.
    if std::env::var_os("DISPLAY").is_none() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return;
    }
    let scale = window.scale_factor();
    let (x, y) = ((x * scale).round() as i32, (y * scale).round() as i32);
    std::thread::spawn(move || {
        if let Err(err) = imp::place(&title, same_title.max(1), x, y) {
            log::info!("cannot place the window: {err}");
        }
    });
}

/// Elsewhere the platform places windows from their saved bounds.
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub fn place(_: &Window, _: String, _: usize, _: f32, _: f32) {}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod imp {
    use std::time::Duration;

    use x11rb::connection::Connection as _;
    use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
    use x11rb::protocol::xproto::{
        AtomEnum, ConfigureWindowAux, ConnectionExt as _, Gravity, Window,
    };
    use x11rb::rust_connection::RustConnection;

    type Error = Box<dyn std::error::Error>;

    /// Finds our window titled `title` (it may still be waiting for the
    /// window manager, so a few tries) and moves it.
    pub fn place(title: &str, same_title: usize, x: i32, y: i32) -> Result<(), Error> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms {
            pid: conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom,
            net_name: conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom,
            pid_value: std::process::id(),
        };
        let mut found = Vec::new();
        for _ in 0..20 {
            found = find(&conn, root, &atoms, title)?;
            if found.len() >= same_title {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        match found.into_iter().max() {
            Some(id) => move_to(&conn, id, x, y),
            None => Err("window not found".into()),
        }
    }

    struct Atoms {
        pid: u32,
        net_name: u32,
        pid_value: u32,
    }

    /// The title of `id`: `_NET_WM_NAME` (set by GPUI when the title
    /// changes), else `WM_NAME` (the title it was created with).
    fn title_of(conn: &RustConnection, id: Window, atoms: &Atoms) -> Result<Vec<u8>, Error> {
        for atom in [atoms.net_name, u32::from(AtomEnum::WM_NAME)] {
            let name = conn
                .get_property(false, id, atom, AtomEnum::ANY, 0, 256)?
                .reply()?;
            if !name.value.is_empty() {
                return Ok(name.value);
            }
        }
        Ok(Vec::new())
    }

    /// Searches the window tree (top-level windows and the clients the
    /// window manager reparented into frames) for our windows titled
    /// `title`.
    fn find(
        conn: &RustConnection,
        root: Window,
        atoms: &Atoms,
        title: &str,
    ) -> Result<Vec<Window>, Error> {
        let mut found = Vec::new();
        let mut level = vec![root];
        for _ in 0..3 {
            let mut next = Vec::new();
            for parent in level {
                for child in conn.query_tree(parent)?.reply()?.children {
                    let owner = conn
                        .get_property(false, child, atoms.pid, AtomEnum::CARDINAL, 0, 1)?
                        .reply()?;
                    if owner.value32().and_then(|mut v| v.next()) == Some(atoms.pid_value) {
                        if title_of(conn, child, atoms)? == title.as_bytes() {
                            found.push(child);
                        }
                        continue;
                    }
                    next.push(child);
                }
            }
            level = next;
        }
        Ok(found)
    }

    fn move_to(conn: &RustConnection, id: Window, x: i32, y: i32) -> Result<(), Error> {
        let mut hints = WmSizeHints::get_normal_hints(conn, id)?
            .reply()
            .ok()
            .flatten()
            .unwrap_or_default();
        hints.position = Some((WmSizeHintsSpecification::UserSpecified, x, y));
        hints.win_gravity = Some(Gravity::STATIC);
        hints.set_normal_hints(conn, id)?;
        conn.configure_window(id, &ConfigureWindowAux::new().x(x).y(y))?;
        // Wait until the server processed it before disconnecting.
        conn.get_input_focus()?.reply()?;
        Ok(())
    }
}
