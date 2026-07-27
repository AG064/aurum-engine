//! Aurum core: the engine foundation shared by every genre module.
//!
//! - [`ecs`] — tiny ECS: entities, components, systems
//! - [`events`] — typed event bus
//! - [`state`] — typed global state with save/load
//! - [`time`] — fixed timestep, time scale
//! - [`assets`] — resource handles + hot-reload hooks
//!
//! This crate has no dependency on Godot. It is fully testable with `cargo test`.

pub mod assets;
pub mod ecs;
pub mod events;
pub mod state;
pub mod time;

/// Convenience re-exports for the most common types.
pub mod prelude {
    pub use crate::ecs::{Component, Entity, System, World};
    pub use crate::events::{Event, EventBus};
    pub use crate::state::{State, StateError, StateValue};
    pub use crate::time::{FixedTimestep, TimeScale};
}
