//! `nostos`: command line for Nostos.
//!
//! - `nostos scan <folder>`: reads a folder of photos and prints `GeoJSON`.
//! - `nostos import <folder> --trip <slug>`: sends a folder to a Nostos API.

mod import;
mod scan;

use std::process::ExitCode;

const USAGE: &str = "usage: nostos <command> [options]

commands:
  scan <folder> [-o out.geojson]
      Reads GPS, capture time and film simulation of every .jpg/.raf under
      <folder> and prints GeoJSON. Works offline.

  import <folder> --trip <slug> [--title <title>] [--jobs <n>] [--dry-run] [--wait]
      Sends the photos of <folder> to a Nostos API (NOSTOS_API_URL) with the
      owner token (NOSTOS_OWNER_TOKEN). Files already sent are skipped.

run `nostos <command> --help` for details.";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "scan" => scan::run(args),
        "import" => import::run(args),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        "-V" | "--version" => {
            println!("nostos {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
