//! `aurum` — the Aurum command-line entry point.
//!
//! A thin dispatcher. Each subcommand's behaviour lives in the crate that owns
//! it, so the CLI stays an adapter rather than a second implementation.

use std::process::ExitCode;

const USAGE: &str = "\
aurum — the Aurum engine command-line interface

USAGE:
    aurum <COMMAND> [OPTIONS]

COMMANDS:
    mcp     Run the headless MCP server over stdio, so an AI agent can drive
            the engine. See `aurum mcp --help`.

OPTIONS:
    -h, --help      Print this help.
    -V, --version   Print the version.

EXAMPLES:
    aurum mcp
    aurum mcp --root ./saves --read-only
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("mcp") => ExitCode::from(aurum_mcp::cli::run(&args[1..]) as u8),
        Some("-h") | Some("--help") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V") | Some("--version") => {
            println!("aurum {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("aurum: unknown command '{other}'\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
