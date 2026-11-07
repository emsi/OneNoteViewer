//! Profile target: parses a `.one` file in a loop so a sampling profiler
//! can collect enough samples. Not committed to git history (this file
//! is for local profiling sessions only).
//!
//!     cargo build --release --example profile_parse
//!     samply record ./target/release/examples/profile_parse \
//!         "crates/parser/tests/samples/Large OneDrive.one" 200

use onenote_parser::Parser;
use std::hint::black_box;
use typed_path::TypedPath;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: profile_parse <path> [iterations]");
    let path = TypedPath::derive(&path);
    let iterations: usize = args
        .next()
        .map(|s| s.parse().expect("iterations must be a number"))
        .unwrap_or(100);

    eprintln!("parsing {} × {}", path.to_string_lossy(), iterations);

    // Warm the OS page cache so we measure parse cost, not first-touch I/O.
    let parser = Parser::new();
    let _ = parser.parse_section(path).expect("warmup parse");

    for _ in 0..iterations {
        let parser = Parser::new();
        let section = parser.parse_section(path).expect("parse failed");
        black_box(section);
    }
}
