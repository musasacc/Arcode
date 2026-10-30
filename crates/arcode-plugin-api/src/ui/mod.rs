//! Declarative panel UI: builders, flatten, diff and [`Panel`].
//!
//! Build a tree with [`vstack`], [`text`], [`table`], … and hand it to
//! [`Panel::rerender`]. The first call sends a full `render`; later calls
//! diff against the previous tree and send only `apply-patches`.

mod builder;
mod diff;
mod flatten;

pub use crate::bindings::arcode::plugin::types::{NodeId, PanelId};
pub use crate::bindings::arcode::plugin::ui::{
    Axis, BadgeProps, ButtonProps, Cell, CheckboxProps, CodeProps, Column, Event, EventKind,
    InputProps, Location, Node, Patch, RowPage, SelectProps, SplitProps, StackProps, TableProps,
    TabsProps, TextProps, Tone, TreeItem, UiError, View, Widget,
};
pub use builder::*;
pub use diff::{apply, diff};
pub use flatten::{FlatView, FlattenError, IdMap, flatten};

use crate::args::{Error, Result};
use crate::sys;

/// One open panel and the last tree sent to it.
#[derive(Debug, Clone)]
pub struct Panel {
    id: PanelId,
    ids: IdMap,
    last: Option<FlatView>,
}

impl Panel {
    /// Opens (or focuses) a panel of the manifest contribution `contribution`.
    pub fn open(contribution: &str, title: &str, location: Option<Location>) -> Result<Self> {
        let id = sys::ui::open_panel(contribution, title, location)?;
        Ok(Self::attach(id))
    }

    /// Wraps a panel the host created (the `panel` of `open-view`).
    pub fn attach(id: PanelId) -> Self {
        Self {
            id,
            ids: IdMap::new(),
            last: None,
        }
    }

    /// The host's id for this panel.
    pub fn id(&self) -> PanelId {
        self.id
    }

    /// Sends `root` to the host: a full `render` the first time (or after an
    /// error), otherwise only the patches against the previous tree. Sends
    /// nothing when the tree is unchanged.
    pub fn rerender(&mut self, root: &Element) -> Result<()> {
        let next = flatten(root, &mut self.ids)?;
        let result = match &self.last {
            None => sys::ui::render(self.id, &next.to_view()),
            Some(prev) => {
                let patches = diff(Some(prev), &next);
                if patches.is_empty() {
                    return Ok(());
                }
                sys::ui::apply_patches(self.id, &patches)
            }
        };
        match result {
            Ok(()) => {
                self.last = Some(next);
                Ok(())
            }
            Err(e) => {
                // The host kept its old tree; resync with a full render next time.
                self.last = None;
                Err(Error::from(e))
            }
        }
    }

    /// The id of the node keyed `key` in the last tree sent, e.g. to tell
    /// which table a `fetch-rows` call is about.
    pub fn node_id(&self, key: &str) -> Option<NodeId> {
        self.ids.find_key(key)
    }

    /// Tells the host a table's cached pages are stale.
    pub fn rows_changed(&self, table: NodeId, row_count: Option<u64>) -> Result<()> {
        sys::ui::apply_patches(self.id, &[Patch::RowsChanged((table, row_count))])?;
        Ok(())
    }

    /// Forgets the last tree, so the next [`Panel::rerender`] is a full render.
    pub fn invalidate(&mut self) {
        self.last = None;
    }

    /// Sets the panel title.
    pub fn set_title(&self, title: &str) {
        sys::ui::set_title(self.id, title);
    }

    /// Closes the panel.
    pub fn close(self) {
        sys::ui::close_panel(self.id);
    }
}
