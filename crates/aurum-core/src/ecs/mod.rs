//! Tiny ECS: entities, components, systems.
//!
//! Design goals:
//! - Zero unsafe, simple to reason about
//! - Component storage is per-type, `HashMap<Entity, T>`-based
//! - Systems are functions that borrow the world
//! - Easy to extend (a `Resource` is just a non-Entity global; see `World::resource`)
//!
//! Not a goal: maximum performance. This is a foundation. If a project needs
//! archetype storage or a query cache, drop in `bevy_ecs` or `hecs` and the
//! `World` API stays similar.

pub mod storage;
pub mod world;

pub use storage::Component;
pub use world::{Entity, System, World};

use std::any::Any;
use std::fmt::Debug;

/// Marker trait for components: `Send + Sync + 'static + Debug` plus serde-friendly defaults.
///
/// We don't require `Serialize`/`Deserialize` because not every component is saveable
/// (e.g. handles into a non-saveable resource). For saveable components, implement
/// them in the genre module that needs persistence.
pub trait ComponentBound: Any + Send + Sync + Debug + 'static {}

/// Blanket impl so any `Send + Sync + 'static + Debug` type is a valid component.
impl<T> ComponentBound for T where T: Any + Send + Sync + Debug + 'static {}
