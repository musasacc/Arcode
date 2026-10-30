//! `spin` fixture: burns CPU without ever returning, so only the host's
//! epoch budget can stop it (§13 step 4).
//!
//! - command `run` with `{"noop": true}` returns at once (warm-up);
//! - command `run` with `{}` spins at once;
//! - command `run` with `{"task": true}` first calls `host.begin-task`;
//! - `provide-code-lenses` spins too (lens budget, strikes).

use arcode_plugin::prelude::*;
use serde_json::Value;

#[derive(Default)]
struct Spin;

/// Never returns; `black_box` keeps the loop from being optimised away.
fn spin() -> ! {
    let mut n: u64 = 0;
    loop {
        n = std::hint::black_box(n.wrapping_mul(6364136223846793005).wrapping_add(1));
    }
}

impl Plugin for Spin {
    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        if args["noop"].as_bool() == Some(true) {
            return Ok(());
        }
        if args["task"].as_bool() == Some(true) {
            let started = host::begin_task("Spinning");
            host::log(host::Level::Info, &format!("begin-task={started}"));
        }
        spin()
    }

    fn code_lenses(&mut self, _doc: &DocInfo) -> Vec<CodeLens> {
        spin()
    }
}

arcode_plugin::export_plugin!(Spin);
