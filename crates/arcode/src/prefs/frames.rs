//! A count of drawn frames, to check that an idle window draws nothing.
//!
//! The workspace is the root view of every window, so each drawn frame
//! renders it exactly once. With `ARCODE_LOG_FRAMES=1` a background thread
//! prints the count to stderr whenever it changed during the last second;
//! an idle window prints nothing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static FRAMES: AtomicU64 = AtomicU64::new(0);

/// Records one drawn frame.
pub fn tick() {
    FRAMES.fetch_add(1, Ordering::Relaxed);
}

/// Frames drawn so far (all windows).
pub fn count() -> u64 {
    FRAMES.load(Ordering::Relaxed)
}

/// Starts the logger when `ARCODE_LOG_FRAMES` is set to anything but `0`.
pub fn init_logging() {
    let wanted = std::env::var("ARCODE_LOG_FRAMES").is_ok_and(|v| !v.is_empty() && v != "0");
    if !wanted {
        return;
    }
    let started = Instant::now();
    let spawned = std::thread::Builder::new()
        .name("frame-log".into())
        .spawn(move || {
            let mut last = count();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let now = count();
                if now != last {
                    eprintln!(
                        "frames: {now} (+{}) at {:.1}s",
                        now - last,
                        started.elapsed().as_secs_f32()
                    );
                    last = now;
                }
            }
        });
    if let Err(err) = spawned {
        log::warn!("cannot start the frame logger: {err}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ticks_count() {
        let before = super::count();
        super::tick();
        assert!(super::count() > before);
    }
}
