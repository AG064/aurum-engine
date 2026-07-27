//! Aurum CLI module — stub.
//!
//! The CLI module provides:
//!
//! - Argument parsing helpers
//! - Subcommand dispatch via the engine's event bus
//! - Exit codes and structured output (JSON lines, table)
//!
//! The shape will be:
//!
//! - `Command` event: a parsed subcommand invocation
//! - `Output` resource: stdout/stderr sink with format options

#![allow(dead_code)]

/// A parsed command-line argument.
#[derive(Debug, Clone)]
pub struct Arg {
    pub key: String,
    pub value: Option<String>,
}

/// A subcommand: name + args.
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub args: Vec<Arg>,
}
