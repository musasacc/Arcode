//! `stdout-flood` fixture: writes `{"mib": n}` MiB of numbered lines to
//! stdout plus a little stderr, then logs "done". The host keeps only the
//! newest 1 MiB (§4 WASI context).

use std::io::Write;

use arcode_plugin::prelude::*;
use serde_json::Value;

#[derive(Default)]
struct Flood;

impl Plugin for Flood {
    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        let mib = args["mib"].as_u64().unwrap_or(4);
        let line = "x".repeat(1000);
        let mut out = std::io::stdout().lock();
        for i in 0..mib * 1024 {
            // Errors are ignored on purpose: a flood must not fail the call.
            let _ = writeln!(out, "{i:08} {line}");
        }
        let _ = out.flush();
        eprintln!("stderr line");
        host::log(host::Level::Info, "done");
        Ok(())
    }
}

arcode_plugin::export_plugin!(Flood);
