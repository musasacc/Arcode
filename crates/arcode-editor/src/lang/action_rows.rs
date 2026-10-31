//! Grouping code actions by kind for the menu: quick fixes first, then the
//! refactorings (extract, inline, rewrite, other) and source actions.

use super::CodeAction;
use crate::menu::{MenuItem, MenuRow};

/// A menu group of code actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionGroup {
    /// `quickfix`
    QuickFix,
    /// `refactor.extract`
    Extract,
    /// `refactor.inline`
    Inline,
    /// `refactor.rewrite`
    Rewrite,
    /// `refactor` and other `refactor.*` kinds
    Refactor,
    /// `source.*` (organize imports, fix all)
    Source,
    /// No kind or an unknown one
    Other,
}

impl ActionGroup {
    /// The group's title in the menu.
    pub fn title(self) -> &'static str {
        match self {
            ActionGroup::QuickFix => "Quick Fix",
            ActionGroup::Extract => "Extract",
            ActionGroup::Inline => "Inline",
            ActionGroup::Rewrite => "Rewrite",
            ActionGroup::Refactor => "Refactor",
            ActionGroup::Source => "Source Action",
            ActionGroup::Other => "More Actions",
        }
    }

    /// Whether the group holds refactorings (shown by "Refactor...").
    pub fn is_refactor(self) -> bool {
        matches!(
            self,
            ActionGroup::Extract
                | ActionGroup::Inline
                | ActionGroup::Rewrite
                | ActionGroup::Refactor
        )
    }
}

/// The group of a hierarchical kind (`refactor.extract.function` is in
/// `Extract`).
pub fn action_group(kind: Option<&str>) -> ActionGroup {
    let Some(kind) = kind else {
        return ActionGroup::Other;
    };
    let is = |prefix: &str| kind == prefix || kind.starts_with(&format!("{prefix}."));
    if is("quickfix") {
        ActionGroup::QuickFix
    } else if is("refactor.extract") {
        ActionGroup::Extract
    } else if is("refactor.inline") {
        ActionGroup::Inline
    } else if is("refactor.rewrite") {
        ActionGroup::Rewrite
    } else if is("refactor") {
        ActionGroup::Refactor
    } else if is("source") {
        ActionGroup::Source
    } else {
        ActionGroup::Other
    }
}

/// Menu rows for `actions` (only refactorings when `refactor_only`), and
/// for each row the index of its action.
pub(crate) fn action_rows(
    actions: &[CodeAction],
    refactor_only: bool,
) -> (Vec<MenuRow>, Vec<Option<usize>>) {
    let mut order: Vec<(ActionGroup, bool, usize)> = actions
        .iter()
        .enumerate()
        .map(|(ix, a)| (action_group(a.kind.as_deref()), !a.preferred, ix))
        .filter(|(group, _, _)| !refactor_only || group.is_refactor())
        .collect();
    order.sort();
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let mut current = None;
    for (group, _, ix) in order {
        if current != Some(group) {
            current = Some(group);
            rows.push(MenuRow::Header(group.title().into()));
            targets.push(None);
        }
        let action = &actions[ix];
        let item = MenuItem::new(action.title.clone())
            .enabled(action.disabled.is_none())
            .detail(action.disabled.clone());
        rows.push(MenuRow::Item(item));
        targets.push(Some(ix));
    }
    (rows, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_map_to_groups() {
        assert_eq!(action_group(Some("quickfix")), ActionGroup::QuickFix);
        assert_eq!(
            action_group(Some("refactor.extract.function")),
            ActionGroup::Extract
        );
        assert_eq!(action_group(Some("refactor.inline")), ActionGroup::Inline);
        assert_eq!(action_group(Some("refactor.rewrite")), ActionGroup::Rewrite);
        assert_eq!(action_group(Some("refactor")), ActionGroup::Refactor);
        assert_eq!(action_group(Some("refactor.move")), ActionGroup::Refactor);
        assert_eq!(
            action_group(Some("source.organizeImports")),
            ActionGroup::Source
        );
        assert_eq!(action_group(Some("quickfixer")), ActionGroup::Other);
        assert_eq!(action_group(None), ActionGroup::Other);
    }

    #[test]
    fn rows_are_grouped_with_preferred_fixes_first() {
        let mut preferred = CodeAction::new("Import HashMap", Some("quickfix"));
        preferred.preferred = true;
        let mut disabled = CodeAction::new("Inline call", Some("refactor.inline"));
        disabled.disabled = Some("not a call".into());
        let actions = vec![
            CodeAction::new("Extract into variable", Some("refactor.extract")),
            CodeAction::new("Remove unused", Some("quickfix")),
            preferred,
            CodeAction::new("Organize imports", Some("source.organizeImports")),
            disabled,
        ];
        let (rows, targets) = action_rows(&actions, false);
        let labels: Vec<String> = rows
            .iter()
            .map(|r| match r {
                MenuRow::Header(h) => format!("[{h}]"),
                MenuRow::Item(i) if i.enabled => i.label.to_string(),
                MenuRow::Item(i) => format!("({})", i.label),
                MenuRow::Separator => "-".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "[Quick Fix]",
                "Import HashMap",
                "Remove unused",
                "[Extract]",
                "Extract into variable",
                "[Inline]",
                "(Inline call)",
                "[Source Action]",
                "Organize imports",
            ]
        );
        assert_eq!(targets[1], Some(2));
        assert_eq!(targets[0], None);

        let (rows, _) = action_rows(&actions, true);
        assert_eq!(rows.len(), 4, "only extract and inline groups");
    }
}
