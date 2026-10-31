//! The scroll state of the changed-files list. Group headers are taller
//! than file rows (docs/DESIGN.md 6.4: section headers are 30, rows 24), so
//! the list is a variable-height GPUI `list`, which caches each row's
//! height. The cache only has to be dropped when the header/file pattern
//! changes; the scroll position survives that.

use gpui::{ListAlignment, ListState, px};

use super::rows::Row;

/// A `ListState` kept in step with the rows it shows.
pub(super) struct RowsList {
    state: ListState,
    /// Which rows are headers, as last synced.
    shape: Vec<bool>,
}

impl RowsList {
    pub(super) fn new() -> Self {
        Self {
            state: ListState::new(0, ListAlignment::Top, px(200.)),
            shape: Vec::new(),
        }
    }

    /// The state to hand to `gpui::list`, after syncing it with `rows`.
    /// Returns whether the cached heights had to be dropped.
    pub(super) fn sync(&mut self, rows: &[Row]) -> bool {
        let shape: Vec<bool> = rows.iter().map(|r| matches!(r, Row::Header(..))).collect();
        if shape == self.shape {
            return false;
        }
        let top = self.state.logical_scroll_top();
        self.state.reset(shape.len());
        self.state.scroll_to(top);
        self.shape = shape;
        true
    }

    pub(super) fn state(&self) -> &ListState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arcode_git::{FileStatus, StatusCode};
    use gpui::ListOffset;

    use super::super::rows::Group;
    use super::*;

    fn file(name: &str) -> Row {
        Row::File(
            Group::Changes,
            FileStatus {
                path: PathBuf::from(name),
                orig_path: None,
                index: StatusCode::Unmodified,
                worktree: StatusCode::Modified,
                conflict: None,
            },
        )
    }

    #[test]
    fn only_a_new_header_pattern_drops_the_heights() {
        let mut list = RowsList::new();
        let rows = vec![Row::Header(Group::Changes, 2), file("a.rs"), file("b.rs")];
        assert!(list.sync(&rows));
        assert_eq!(list.state().item_count(), 3);
        // Same pattern, other files: the cached heights still hold.
        let renamed = vec![Row::Header(Group::Changes, 2), file("c.rs"), file("d.rs")];
        assert!(!list.sync(&renamed));
        let mut more = renamed.clone();
        more.push(Row::Header(Group::Untracked, 0));
        assert!(list.sync(&more));
        assert_eq!(list.state().item_count(), 4);
    }

    #[test]
    fn a_resync_keeps_the_scroll_position() {
        let mut list = RowsList::new();
        let mut rows = vec![Row::Header(Group::Changes, 9)];
        rows.extend((0..9).map(|i| file(&format!("{i}.rs"))));
        list.sync(&rows);
        let top = ListOffset {
            item_ix: 4,
            offset_in_item: px(3.),
        };
        list.state().scroll_to(top);
        rows.push(Row::Header(Group::Untracked, 1));
        list.sync(&rows);
        let kept = list.state().logical_scroll_top();
        assert_eq!((kept.item_ix, kept.offset_in_item), (4, px(3.)));
    }
}
