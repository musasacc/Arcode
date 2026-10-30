//! `spin-cancellable` fixture: a long loop that polls `host.cancelled()`
//! and returns early once the user cancels (§5 cooperative cancellation).

use arcode_plugin::prelude::*;

#[derive(Default)]
struct SpinCancellable;

impl Plugin for SpinCancellable {
    fn command(&mut self, _id: &str, _args: Json) -> Result<()> {
        let mut n: u64 = 0;
        let mut rounds: u64 = 0;
        loop {
            for _ in 0..10_000 {
                n = std::hint::black_box(n.wrapping_mul(31).wrapping_add(7));
            }
            rounds += 1;
            if host::cancelled() {
                host::log(
                    host::Level::Info,
                    &format!("cancelled after {rounds} rounds"),
                );
                return Err(Error::msg("cancelled"));
            }
        }
    }
}

arcode_plugin::export_plugin!(SpinCancellable);
