//! Per-type component storage.
//!
//! Components live in `HashMap<Entity, T>`. The trait is sealed by the blanket
//! `ComponentBound` impl in `super::mod.rs` so that any `Send + Sync + 'static +
//! Debug` type is automatically a valid component.

use std::collections::HashMap;

use super::world::Entity;
use super::ComponentBound;

/// Trait object for a single component type's storage.
///
/// We erase the concrete type so `World` can keep one map of storages
/// (`HashMap<TypeId, Box<dyn ComponentStorage>>`) and dispatch lookups by
/// `TypeId`. This is enough for hundreds of components; for tens of thousands
/// of entities with the same component, swap to a real archetype ECS.
pub trait ComponentStorage: std::any::Any + Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn remove(&mut self, entity: Entity) -> bool;
    fn entity_count(&self) -> usize;
}

pub struct ComponentMap<T: ComponentBound> {
    data: HashMap<Entity, T>,
}

impl<T: ComponentBound> ComponentMap<T> {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn insert(&mut self, entity: Entity, value: T) {
        self.data.insert(entity, value);
    }

    pub fn get(&self, entity: Entity) -> Option<&T> {
        self.data.get(&entity)
    }

    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        self.data.get_mut(&entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.data.contains_key(&entity)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.data.iter().map(|(e, c)| (*e, c))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.data.iter_mut().map(|(e, c)| (*e, c))
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<T: ComponentBound> Default for ComponentMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ComponentBound> ComponentStorage for ComponentMap<T> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn remove(&mut self, entity: Entity) -> bool {
        self.data.remove(&entity).is_some()
    }
    fn entity_count(&self) -> usize {
        self.data.len()
    }
}

/// Type alias so downstream code can write `Component<T>` instead of the longer
/// `ComponentBound` when constraining generic bounds. Functionally identical.
pub trait Component: ComponentBound {}
impl<T: ComponentBound> Component for T {}
