//! Backwards-compatible `reinject-parser` binary.
//!
//! All logic lives in `reinject-core::parser`. This thin wrapper preserves
//! the original CLI interface:
//!
//! ```text
//! reinject-parser <transcript_path> <byte_offset>
//! ```
//!
//! Stdout: `"nt_bytes th_bytes"` (space-separated integers).

use std::path::Path;
use std::process;

use reinject_core::parse_transcript_delta;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: reinject-parser <transcript_path> <byte_offset>");
        process::exit(1);
    }

    let path = Path::new(&args[1]);
    let offset: u64 = args[2].parse().unwrap_or(0);

    if !path.exists() {
        println!("0 0");
        return;
    }

    match parse_transcript_delta(path, offset) {
        Ok((nt, th)) => println!("{nt} {th}"),
        Err(e) => {
            eprintln!("reinject-parser: {e:#}");
            println!("0 0");
        }
    }
}
