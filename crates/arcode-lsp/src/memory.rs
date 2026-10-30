//! Keeping memory from very large messages out of the resident set.
//!
//! Opening a multi-megabyte document makes several short-lived copies of
//! its text (the `didOpen` parameters, the JSON value, the JSON body), on
//! the runtime's worker threads. glibc raises its mmap threshold after the
//! first such block is freed, so later copies come from the thread heaps,
//! whose free top is not given back (not even by `malloc_trim`). The
//! process then stays tens of megabytes larger than needed (measured:
//! +28 MB for a 9 MB file). Pinning the threshold keeps blocks of 1 MiB or
//! more in their own mappings, which are returned to the OS when freed.
//! Elsewhere this is a no-op.

/// Allocations at least this large get their own mapping.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MMAP_THRESHOLD: libc::c_int = 1 << 20;

/// Pins glibc's mmap threshold (once per process).
pub(crate) fn tune_allocator() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            // SAFETY: `mallopt` has no memory-safety preconditions; it only
            // changes an allocator tuning parameter and is thread-safe.
            unsafe {
                libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD);
            }
        });
    }
}
