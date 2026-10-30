//! Performance: viewport highlighting and incremental reparse on a big file.
//!
//! Run with `--nocapture` to see timings. Limits are generous in debug.

mod common;

use std::time::{Duration, Instant};

use arcode_syntax::{Syntax, language_by_id};
use common::apply_edit;
use ropey::Rope;

const LINES: usize = 100_000;

/// Generates at least `LINES` lines of varied Rust (9 lines per function).
fn big_rust_file() -> String {
    let mut out = String::with_capacity(LINES * 40);
    for i in 0..LINES.div_ceil(9) {
        out.push_str(&format!(
            "/// Function number {i}.\n\
             pub fn function_{i}(value: u64, name: &str) -> Option<u64> {{\n\
             \x20   // A comment with ünïcode {i}.\n\
             \x20   let text = \"string {i} with \\n escape\";\n\
             \x20   if value > {i} {{ return Some(value * 2); }}\n\
             \x20   println!(\"{{}} {{}}\", name, text);\n\
             \x20   None\n\
             }}\n\n"
        ));
    }
    out
}

fn limit(release_ms: u64, debug_ms: u64) -> Duration {
    Duration::from_millis(if cfg!(debug_assertions) {
        debug_ms
    } else {
        release_ms
    })
}

#[test]
fn viewport_highlight_is_fast_and_reparse_is_incremental() {
    let src = big_rust_file();
    let rust = language_by_id("rust").expect("rust");
    let mut text = Rope::from_str(&src);
    let lines = text.len_lines();
    assert!(lines >= LINES, "generated {lines} lines");

    // Compile the query up front so it is not part of the timings.
    assert!(rust.highlight_query().is_some());

    let started = Instant::now();
    let mut syntax = Syntax::new(rust, &text).expect("parse");
    let initial_parse = started.elapsed();

    let viewport = lines - 200..lines - 140;
    let started = Instant::now();
    let spans = syntax.highlight_lines(&text, viewport.clone());
    let highlight = started.elapsed();
    assert_eq!(spans.len(), 60);
    assert!(spans.iter().any(|line| !line.is_empty()));

    // Warm call (allocator, caches).
    let started = Instant::now();
    let again = syntax.highlight_lines(&text, viewport.clone());
    let highlight_warm = started.elapsed();
    assert_eq!(again, spans);

    // Small edit near the end, then incremental reparse.
    let at = text.line_to_byte(lines - 170);
    let edit = apply_edit(&mut text, at..at, "let added = 1;\n");
    let started = Instant::now();
    syntax.edit(&edit);
    syntax.reparse(&text);
    let reparse = started.elapsed();

    let started = Instant::now();
    let after = syntax.highlight_lines(&text, viewport);
    let highlight_after = started.elapsed();
    assert_eq!(after.len(), 60);

    println!(
        "{lines} lines: initial parse {initial_parse:?}, viewport highlight {highlight:?} \
         (warm {highlight_warm:?}, after edit {highlight_after:?}), incremental reparse {reparse:?}"
    );

    let max_highlight = limit(20, 250);
    assert!(
        highlight_warm < max_highlight,
        "viewport highlight took {highlight_warm:?} (limit {max_highlight:?})"
    );
    assert!(
        reparse < initial_parse,
        "incremental reparse {reparse:?} not faster than initial parse {initial_parse:?}"
    );
}

#[test]
fn whole_file_outline_and_runnables_are_fast() {
    let src = big_rust_file();
    let rust = language_by_id("rust").expect("rust");
    let text = Rope::from_str(&src);
    let syntax = Syntax::new(rust, &text).expect("parse");
    assert!(rust.outline_query().is_some() && rust.runnables_query().is_some());

    let started = Instant::now();
    let symbols = syntax.symbols(&text);
    let outline = started.elapsed();
    assert_eq!(symbols.len(), LINES.div_ceil(9));

    let started = Instant::now();
    let runnables = syntax.runnables(&text);
    let run = started.elapsed();
    assert!(runnables.is_empty());

    println!("outline {outline:?}, runnables {run:?}");
    let max = limit(500, 5_000);
    assert!(outline < max, "outline took {outline:?} (limit {max:?})");
    assert!(run < max, "runnables took {run:?} (limit {max:?})");
}
