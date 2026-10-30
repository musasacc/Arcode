//! `crash` fixture: `run` with `{"crash": true}` panics (a trap in wasm),
//! otherwise it answers normally, so tests can watch restarts.

use arcode_plugin::prelude::*;
use serde_json::Value;

#[derive(Default)]
struct Crash;

impl Plugin for Crash {
    fn activate(&mut self, reason: Activation) -> Result<()> {
        host::log(host::Level::Info, &format!("activate {reason:?}"));
        Ok(())
    }

    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        if args["crash"].as_bool() == Some(true) {
            panic!("crash fixture asked to crash");
        }
        host::log(host::Level::Info, "ok");
        Ok(())
    }
}

arcode_plugin::export_plugin!(Crash);
