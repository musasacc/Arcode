//! Linear-space Myers diff ("An O(ND) Difference Algorithm", divide and
//! conquer on the middle snake) over interned line ids.
//!
//! The search depth per sub-problem is capped so pathological inputs (two
//! large, completely different files) degrade into one coarse replacement
//! instead of quadratic run time.

use std::ops::Range;

/// One edit in the script, in increasing position order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// `old` lines are removed; they sat just before new line `new_pos`.
    Delete { old: Range<usize>, new_pos: usize },
    /// `new` lines are inserted just before old line `old_pos`.
    Insert { old_pos: usize, new: Range<usize> },
}

/// Lower bound on the number of edit steps searched per sub-problem.
const MIN_D_LIMIT: usize = 256;
/// Rough work budget per sub-problem, divided by its size to get the limit.
const WORK_BUDGET: usize = 8_000_000;

/// Computes an edit script turning `a` into `b`.
pub(crate) fn diff(a: &[u32], b: &[u32]) -> Vec<Op> {
    let max_d = (a.len() + b.len()).div_ceil(2) + 1;
    let mut ctx = Ctx {
        a,
        b,
        vf: V::new(max_d),
        vb: V::new(max_d),
        out: Vec::new(),
    };
    ctx.conquer(0..a.len(), 0..b.len());
    ctx.out
}

/// Furthest-reaching x per diagonal `k`, indexed by signed `k`.
struct V {
    offset: isize,
    v: Vec<isize>,
}

impl V {
    fn new(max_d: usize) -> Self {
        let len = 2 * max_d + 3;
        Self {
            offset: (max_d + 1) as isize,
            v: vec![0; len],
        }
    }

    fn get(&self, k: isize) -> isize {
        self.v[(k + self.offset) as usize]
    }

    fn set(&mut self, k: isize, x: isize) {
        self.v[(k + self.offset) as usize] = x;
    }
}

struct Ctx<'a> {
    a: &'a [u32],
    b: &'a [u32],
    vf: V,
    vb: V,
    out: Vec<Op>,
}

impl Ctx<'_> {
    fn conquer(&mut self, mut ar: Range<usize>, mut br: Range<usize>) {
        let prefix = self.a[ar.clone()]
            .iter()
            .zip(&self.b[br.clone()])
            .take_while(|(x, y)| x == y)
            .count();
        ar.start += prefix;
        br.start += prefix;
        let suffix = self.a[ar.clone()]
            .iter()
            .rev()
            .zip(self.b[br.clone()].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        ar.end -= suffix;
        br.end -= suffix;

        if ar.is_empty() && br.is_empty() {
            return;
        }
        if ar.is_empty() {
            self.out.push(Op::Insert {
                old_pos: ar.start,
                new: br,
            });
            return;
        }
        if br.is_empty() {
            self.out.push(Op::Delete {
                old: ar,
                new_pos: br.start,
            });
            return;
        }
        match self.middle_snake(ar.clone(), br.clone()) {
            // Splitting at either corner would not shrink the problem.
            Some((x, y)) if (x, y) != (ar.start, br.start) && (x, y) != (ar.end, br.end) => {
                self.conquer(ar.start..x, br.start..y);
                self.conquer(x..ar.end, y..br.end);
            }
            _ => {
                self.out.push(Op::Delete {
                    old: ar.clone(),
                    new_pos: br.start,
                });
                self.out.push(Op::Insert {
                    old_pos: ar.end,
                    new: br,
                });
            }
        }
    }

    /// Finds the start of the middle snake of an optimal path through
    /// `a[ar] -> b[br]`, in absolute coordinates. Both ranges are non-empty
    /// and have no common prefix or suffix. Returns `None` when the search
    /// budget is exhausted.
    fn middle_snake(&mut self, ar: Range<usize>, br: Range<usize>) -> Option<(usize, usize)> {
        let (a, b) = (&self.a[ar.clone()], &self.b[br.clone()]);
        let n = a.len() as isize;
        let m = b.len() as isize;
        let delta = n - m;
        let odd = delta & 1 != 0;
        let max_d = (a.len() + b.len()).div_ceil(2) + 1;
        let limit = max_d.min(MIN_D_LIMIT.max(WORK_BUDGET / (a.len() + b.len())));
        let (vf, vb) = (&mut self.vf, &mut self.vb);
        vf.set(1, 0);
        vb.set(1, 0);

        for d in 0..limit as isize {
            for k in (-d..=d).rev().step_by(2) {
                let mut x = if k == -d || (k != d && vf.get(k - 1) < vf.get(k + 1)) {
                    vf.get(k + 1)
                } else {
                    vf.get(k - 1) + 1
                };
                let mut y = x - k;
                let (x0, y0) = (x, y);
                while x >= 0 && y >= 0 && x < n && y < m && a[x as usize] == b[y as usize] {
                    x += 1;
                    y += 1;
                }
                vf.set(k, x);
                if odd && (k - delta).abs() < d && x + vb.get(delta - k) >= n {
                    return to_abs(&ar, &br, x0, y0);
                }
            }
            for k in (-d..=d).rev().step_by(2) {
                let mut x = if k == -d || (k != d && vb.get(k - 1) < vb.get(k + 1)) {
                    vb.get(k + 1)
                } else {
                    vb.get(k - 1) + 1
                };
                let mut y = x - k;
                while x >= 0
                    && y >= 0
                    && x < n
                    && y < m
                    && a[(n - x - 1) as usize] == b[(m - y - 1) as usize]
                {
                    x += 1;
                    y += 1;
                }
                vb.set(k, x);
                if !odd && (k - delta).abs() <= d && x + vf.get(delta - k) >= n {
                    return to_abs(&ar, &br, n - x, m - y);
                }
            }
        }
        None
    }
}

/// Converts sub-problem coordinates to absolute ones, rejecting points
/// outside the sub-problem.
fn to_abs(ar: &Range<usize>, br: &Range<usize>, x: isize, y: isize) -> Option<(usize, usize)> {
    let x = usize::try_from(x).ok()?;
    let y = usize::try_from(y).ok()?;
    (x <= ar.len() && y <= br.len()).then(|| (ar.start + x, br.start + y))
}
