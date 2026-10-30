//! Security review: kv quota accounting. Its own test binary because it
//! installs a counting global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use arcode_plugin_host::bindings::kv::{KvError, Scope};
use arcode_plugin_host::host_impl::kv::{KV_QUOTA_BYTES, KvFiles, KvStore};

/// Live heap bytes of the whole process.
struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards to the system allocator and only counts sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Finding: the 1 MiB kv quota counts key and value bytes only. With
/// 3-byte keys and empty values a plugin stores ~350k entries per scope,
/// and each costs a `String`, a `Vec` and B-tree node space on the host:
/// tens of MB of host memory per scope (two scopes per plugin), kept for
/// the whole session and outside the plugin's memory limit.
#[test]
fn the_kv_quota_bounds_host_memory() {
    let before = LIVE.load(Ordering::Relaxed);
    let mut store = KvStore::new(KvFiles::none());
    let alphabet: Vec<char> = (0x21u8..0x7f).map(char::from).collect();
    let mut entries = 0usize;
    'fill: for a in &alphabet {
        for b in &alphabet {
            for c in &alphabet {
                let key: String = [*a, *b, *c].iter().collect();
                match store.set(Scope::Global, &key, Vec::new()) {
                    Ok(()) => entries += 1,
                    Err(KvError::QuotaExceeded) => break 'fill,
                    Err(e) => panic!("{e:?}"),
                }
            }
        }
    }
    let used = LIVE.load(Ordering::Relaxed) - before;
    let used = usize::try_from(used).unwrap_or(0);
    assert!(
        used <= 4 * KV_QUOTA_BYTES,
        "{entries} entries within the 1 MiB quota use {used} bytes of host memory"
    );
    drop(store);
}
