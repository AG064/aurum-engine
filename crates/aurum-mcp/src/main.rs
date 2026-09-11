//! `aurum-mcp` — run the Aurum MCP server over stdio.
//!
//! All behaviour lives in [`aurum_mcp::cli`] so that `aurum mcp` and this
//! binary stay identical. stdout carries only the JSON-RPC protocol stream.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match aurum_mcp::cli::run(&args) {
        0 => ExitCode::SUCCESS,
        code => ExitCode::from(code as u8),
    }
}
