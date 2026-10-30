//! Install plans: the exact steps shown to the user before anything runs.

use serde::{Deserialize, Serialize};

use super::{Component, ComponentKind, ComponentStatus, InstallStep, LanguagePack, Os, ToolState};

/// One step of an install plan, with the component it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    /// Component name.
    pub component: String,
    /// Component role.
    pub kind: ComponentKind,
    /// Programs the component needs first (`npm`, `python`, ...).
    pub requires: Vec<String>,
    /// The step.
    pub step: InstallStep,
}

/// Steps installing every non-optional component of `pack` on `os`, in
/// order. Steps with [`InstallStep::network`] download from the internet;
/// manual steps are display-only. Show the plan and ask before running it.
pub fn install_plan(pack: &LanguagePack, os: Os) -> Vec<PlanStep> {
    plan(pack.components.iter().filter(|c| !c.optional), os)
}

/// Steps installing the named components (optional ones included).
pub fn install_plan_for(pack: &LanguagePack, os: Os, names: &[&str]) -> Vec<PlanStep> {
    plan(
        pack.components
            .iter()
            .filter(|c| names.contains(&c.name.as_str())),
        os,
    )
}

/// Steps installing the non-optional components reported
/// [`ToolState::Missing`] in `statuses` (from [`super::check_installed`]).
pub fn install_plan_missing(
    pack: &LanguagePack,
    os: Os,
    statuses: &[ComponentStatus],
) -> Vec<PlanStep> {
    let missing = |c: &&Component| {
        !c.optional
            && statuses
                .iter()
                .any(|s| s.component == c.name && s.state == ToolState::Missing)
    };
    plan(pack.components.iter().filter(missing), os)
}

fn plan<'a>(components: impl Iterator<Item = &'a Component>, os: Os) -> Vec<PlanStep> {
    components
        .flat_map(|c| {
            c.install_steps(os).map(|step| PlanStep {
                component: c.name.clone(),
                kind: c.kind,
                requires: c.requires.clone(),
                step: step.clone(),
            })
        })
        .collect()
}
