//! Aurum VN module — story interpreter.
//!
//! Lightweight port of the original `VNEngine` (in `godot/vn/addons/regular_vn`).
//! The goal is the same: parse a story JSON, advance scene entries, expose
//! typed state for save/load. The shape is a clean break from the original —
//! the original is preserved in `godot/vn/`.
//!
//! ## What this crate provides
//!
//! - [`Story`] — parsed story with scenes and variables.
//! - [`Interpreter`] — advances a cursor through entries, emitting events.
//! - [`Event`] — the events an interpreter produces (Dialogue, Choice, ...).
//!
//! ## What this crate does NOT do
//!
//! - Render. UI/HUD is in the GDScript shim / per-game presentation layer.
//! - Save to disk. The caller serializes `Story` state to JSON.
//! - Localization. Keys are exposed; the caller resolves them.

mod story;
mod interpreter;

pub use story::{Choice, ChoiceEntry, DialogueEntry, Entry, Scene, Story, StoryError};
pub use interpreter::{Event, Interpreter, InterpreterState};
