//! The plugin's panels: one [`DbView`] per opened database file, and the
//! [`Sidebar`].

pub mod db;
mod db_events;
mod db_render;
pub mod sidebar;

pub use db::{DbView, Intent, QueryState, SNAPSHOT_NOTE};
pub use db_render::{RUNNING, group};
pub use sidebar::{OpenRequest, Sidebar};

/// One open panel.
#[derive(Debug)]
pub enum View {
    /// A database file (the `sqlite` opener).
    Db(Box<DbView>),
    /// The `databases` sidebar.
    Sidebar(Sidebar),
}
