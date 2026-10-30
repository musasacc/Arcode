//! Subscriptions of the workspace to short-lived entities (overlays such as
//! the finder or the palette, editors, panes), kept so they do not pile up
//! over a long session: an overlay's subscription is replaced when the
//! overlay opens again, and one tied to an entity is dropped once that
//! entity is released.

use std::collections::HashMap;

use gpui::{AnyWeakEntity, Subscription};

/// Bounded subscription storage (see the module docs).
#[derive(Default)]
pub struct Subs {
    keyed: HashMap<&'static str, Subscription>,
    tied: Vec<(AnyWeakEntity, Subscription)>,
}

impl Subs {
    /// Keeps `sub` under `key`, dropping the one stored there before (the
    /// previous instance of the same overlay).
    pub fn set(&mut self, key: &'static str, sub: Subscription) {
        self.keyed.insert(key, sub);
    }

    /// Keeps `sub` while `entity` is alive; subscriptions of released
    /// entities are dropped first.
    pub fn tie(&mut self, entity: AnyWeakEntity, sub: Subscription) {
        self.tied.retain(|(e, _)| e.upgrade().is_some());
        self.tied.push((entity, sub));
    }

    /// How many subscriptions are kept.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.keyed.len() + self.tied.len()
    }
}
