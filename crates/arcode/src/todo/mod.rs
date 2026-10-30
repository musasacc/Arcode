//! The TODO list: TODO, FIXME, HACK, XXX, BUG and NOTE comments (the tags
//! are `todo.tags` in the settings) across the project, grouped by file
//! with counts and tag filter chips. One [`TodoStore`] feeds two
//! [`TodoView`]s: the TODO activity in the sidebar and the Todo tab of the
//! bottom panel.

pub mod rows;
mod store;
#[cfg(test)]
mod tests;
mod view;

pub use store::TodoStore;
pub use view::{TodoEvent, TodoPlacement, TodoView, tag_color};
