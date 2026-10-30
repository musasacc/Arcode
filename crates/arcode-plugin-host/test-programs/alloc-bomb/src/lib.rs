//! `alloc-bomb` fixture: allocates until the store's memory limit.
//!
//! - `{"mode": "try"}` grows with `try_reserve`, sees the failure inside
//!   its own sandbox and reports how far it got, then returns normally;
//! - `{"mode": "abort"}` allocates infallibly, so the guest allocator
//!   aborts (a trap) when the host refuses to grow memory.

use arcode_plugin::prelude::*;
use serde_json::{Value, json};

#[derive(Default)]
struct AllocBomb {
    hoard: Vec<Vec<u8>>,
}

const CHUNK: usize = 1024 * 1024;

impl Plugin for AllocBomb {
    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        if args["mode"].as_str() == Some("try") {
            let mut total = 0usize;
            loop {
                let mut chunk = Vec::new();
                if chunk.try_reserve_exact(CHUNK).is_err() {
                    break;
                }
                chunk.resize(CHUNK, 1);
                total += CHUNK;
                self.hoard.push(chunk);
            }
            let held = self.hoard.len();
            self.hoard.clear();
            let report = json!({"op": "try", "reached-mib": total / CHUNK, "chunks": held});
            host::log(host::Level::Info, &report.to_string());
            return Ok(());
        }
        loop {
            self.hoard.push(vec![1u8; CHUNK]);
        }
    }
}

arcode_plugin::export_plugin!(AllocBomb);
