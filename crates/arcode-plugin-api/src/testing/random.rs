//! Seeded random view trees for property tests (§13 step 8: render → diff
//! → apply equals a fresh render). The host's tests use the same generator
//! to check that `PanelTree` accepts every patch set the SDK emits.
//!
//! Trees are drawn from a small space (few keys, few texts, shallow) so
//! consecutive trees share many key paths and the diffs are interesting.
//! Every tree obeys the widget schema (split: 2 children, tabs: one per
//! title, scroll: 1, leaves: 0) and sibling keys are unique.

use crate::ui::{self, Axis, Element, Tone, TreeItem};

/// A small deterministic PRNG (xorshift64*), so failures reproduce from
/// the seed alone.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator for `seed` (any value, including 0).
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    /// True with probability `percent` / 100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

const KEYS: &[&str] = &["a", "b", "c", "status", "body"];
const TEXTS: &[&str] = &["", "ok", "200", "404", "héllo", "SELECT 1"];

/// A random tree at most `depth` levels deep.
pub fn random_tree(rng: &mut Rng, depth: u32) -> Element {
    let element = if depth <= 1 {
        leaf(rng)
    } else {
        match rng.below(10) {
            0..=3 => {
                let n = rng.below(5);
                let children = (0..n)
                    .map(|_| random_tree(rng, depth - 1))
                    .collect::<Vec<_>>();
                let stack = if rng.chance(50) {
                    ui::vstack(children)
                } else {
                    ui::hstack(children)
                };
                with_unique_keys(rng, stack)
            }
            4 => ui::split(
                if rng.chance(50) {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                },
                rng.below(100) as f32 / 100.0,
                random_tree(rng, depth - 1),
                random_tree(rng, depth - 1),
            ),
            5 => {
                let n = 1 + rng.below(3);
                let pages = (0..n)
                    .map(|i| (format!("Tab {i}"), random_tree(rng, depth - 1)))
                    .collect::<Vec<_>>();
                ui::tabs(rng.below(n) as u32, pages)
            }
            6 => ui::scroll(random_tree(rng, depth - 1)),
            _ => leaf(rng),
        }
    };
    if rng.chance(20) {
        element.grow()
    } else {
        element
    }
}

/// Gives some children explicit keys, never two equal ones.
fn with_unique_keys(rng: &mut Rng, mut stack: Element) -> Element {
    let mut free: Vec<&str> = KEYS.to_vec();
    for child in &mut stack.children {
        if !free.is_empty() && rng.chance(50) {
            let key = free.remove(rng.below(free.len() as u64) as usize);
            child.key = Some(key.to_owned());
        }
    }
    stack
}

fn leaf(rng: &mut Rng) -> Element {
    let text = rng.pick(TEXTS);
    match rng.below(12) {
        0 => ui::text(text).tone(Tone::Accent),
        1 => ui::badge(text, Tone::Success),
        2 => ui::button(text).primary(),
        3 => ui::input(text, "type here").language("sql"),
        4 => ui::table(
            vec![ui::column("n"), ui::column(text)],
            Some(rng.below(3) * 1000),
            rng.below(2) as u32,
        ),
        5 => ui::tree(vec![TreeItem {
            key: text.into(),
            label: text.into(),
            detail: None,
            icon: None,
            depth: 0,
            expandable: rng.chance(50),
            expanded: false,
        }]),
        6 => ui::code(text, Some("rust".into())),
        7 => ui::checkbox(text, rng.chance(50)),
        8 => ui::divider(),
        9 => ui::spacer(),
        10 => ui::key_values([(text.to_owned(), "v".to_owned())]),
        _ => ui::text(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{FlatView, IdMap, apply, diff, flatten};

    /// The SDK property: whatever the sequence of trees, applying each diff
    /// to the previous state gives exactly the fresh flattening.
    #[test]
    fn render_diff_apply_equals_a_fresh_render() {
        let mut patches_seen = 0usize;
        for seed in 0..300 {
            let mut rng = Rng::new(seed);
            let mut ids = IdMap::new();
            let mut host_side = FlatView::default();
            let mut prev: Option<FlatView> = None;
            for step in 0..25 {
                let depth = 1 + rng.below(5) as u32;
                let tree = random_tree(&mut rng, depth);
                let next = flatten(&tree, &mut ids).expect("generated keys are unique");
                let patches = diff(prev.as_ref(), &next);
                patches_seen += patches.len();
                apply(&mut host_side, &patches);
                assert_eq!(host_side, next, "seed {seed}, step {step}");
                // The same tree again diffs to nothing.
                let again = flatten(&tree, &mut ids).expect("same tree");
                assert!(
                    diff(Some(&next), &again).is_empty(),
                    "seed {seed}, step {step}"
                );
                prev = Some(next);
            }
        }
        assert!(patches_seen > 10_000, "the generator exercises the diff");
    }

    #[test]
    fn the_generator_is_deterministic() {
        let a = random_tree(&mut Rng::new(7), 4);
        let b = random_tree(&mut Rng::new(7), 4);
        assert_eq!(a, b);
    }
}
