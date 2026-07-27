//! Aurum text module — stub.
//!
//! The text module runs the engine core (ECS, state, save/load) without
//! any rendering. Useful for:
//!
//! - Text adventures
//! - AI training environments
//! - Server-side simulations
//!
//! The shape will be:
//!
//! - `Printer` resource that the engine writes lines into
//! - `InputLine` event for player commands
//! - Per-genre script interpretation (Ink, Twine, custom DSL)

#![allow(dead_code)]

/// Event: the player typed a line of text.
#[derive(Debug, Clone)]
pub struct InputLine(pub String);

/// Resource: where to print. By default writes to stdout.
#[derive(Debug, Default)]
pub struct Printer;

impl Printer {
    pub fn println(&self, line: &str) {
        println!("{}", line);
    }
}
