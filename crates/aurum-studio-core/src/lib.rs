//! # aurum-studio-core
//!
//! Orchestration for Aurum Studio, with no UI and no Godot dependency.
//!
//! Godot is the workhorse — it stays the editor, renderer, asset pipeline, and
//! runtime. This crate is the part that decides *what to run, when, and whether
//! it worked*: finding the project, checking the toolchain, supervising child
//! processes, building the extension, installing it safely, and classifying a
//! change as a reload, a gameplay restart, or an editor restart.
//!
//! Everything here is testable without a GUI and without Godot installed, which
//! is what makes the CLI and a future window two adapters over one
//! implementation rather than two implementations.
//!
//! ## Modules
//!
//! - [`toml`] — a minimal reader for the `aurum.toml` subset
//! - [`project`] — configuration and read-only project discovery
//! - [`toolchain`] — finding Cargo, rustc, and Godot
//! - [`doctor`] — healthy, warning, and blocked findings, with evidence
//! - [`process`] — running children with capture and a bounded wait
//!
//! ## Dependency policy
//!
//! This crate adds **no new packages** to the workspace. It uses `serde` and
//! `serde_json`, which were already locked, and nothing else.
//!
//! The TOML reader is written here rather than taken from a crate because a
//! TOML implementation *would* add packages, and the subset `aurum.toml` needs
//! is small enough that a focused reader is both smaller and more explicit
//! about what it refuses.

pub mod doctor;
pub mod process;
pub mod project;
pub mod registry;
pub mod toml;
pub mod toolchain;

pub use doctor::{Finding, Health, Report};
pub use process::{Command, Outcome};
pub use project::{clean_path, ConfigError, Layout, Project, ProjectConfig};
pub use registry::{Entry, Registry, RegistryError};
pub use toolchain::{Tool, Toolchain};
