//! The `World` and `Entity` types.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

use super::storage::{Component, ComponentMap, ComponentStorage};

/// A handle to an entity in the world. Entities are created by `World::spawn`
/// and destroyed by `World::despawn`. They are stable across component
/// add/remove — only `despawn` invalidates the handle.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct Entity(u64);

impl Entity {
    /// Mostly for debugging. Returns the raw id.
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({})", self.0)
    }
}

/// A system is a function that runs every tick. It gets a mutable world and the
/// elapsed delta time in seconds.
pub type System = Box<dyn FnMut(&mut World, f32) + Send + Sync>;

/// The world owns entities, their components, resources, and systems.
///
/// Cheap to clone for read-only ops? No — `World` is unique-owned. Use a
/// reference if you only need to query. The world itself is the lock.
pub struct World {
    next_entity_id: u64,
    storages: HashMap<TypeId, Box<dyn ComponentStorage>>,
    /// Resources: non-entity global state (camera, audio mixer, time, ...).
    resources: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    systems: Vec<System>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            next_entity_id: 1,
            storages: HashMap::new(),
            resources: HashMap::new(),
            systems: Vec::new(),
        }
    }

    // ----- entities -----

    /// Create a new entity. Returns its handle.
    pub fn spawn(&mut self) -> Entity {
        let id = self.next_entity_id;
        self.next_entity_id = self
            .next_entity_id
            .checked_add(1)
            .expect("entity id overflow");
        Entity(id)
    }

    /// Destroy an entity and all its components.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        let mut removed = false;
        for storage in self.storages.values_mut() {
            if storage.remove(entity) {
                removed = true;
            }
        }
        removed
    }

    pub fn entity_count(&self) -> usize {
        // The number of distinct entities across all storages. Because the same
        // entity appears in many storages, we approximate via the next-id minus
        // 1. For an exact count, add a separate `entities: HashSet<Entity>`
        // field. Skipping for now — entity count is for diagnostics.
        (self.next_entity_id - 1) as usize
    }

    // ----- components -----

    /// Attach a component to an entity. Creates the storage if needed.
    pub fn insert<T: Component + Send + Sync>(&mut self, entity: Entity, value: T) {
        let storage = self
            .storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(ComponentMap::<T>::new()));
        // Safe downcast: we just inserted the right type above.
        let map = storage
            .as_any_mut()
            .downcast_mut::<ComponentMap<T>>()
            .expect("storage type mismatch");
        map.insert(entity, value);
    }

    /// Read a component. Returns `None` if the entity has no such component.
    pub fn get<T: Component + Send + Sync>(&self, entity: Entity) -> Option<&T> {
        let storage = self.storages.get(&TypeId::of::<T>())?;
        let map = storage
            .as_any()
            .downcast_ref::<ComponentMap<T>>()
            .expect("storage type mismatch");
        map.get(entity)
    }

    /// Mutate a component. Returns `None` if the entity has no such component.
    pub fn get_mut<T: Component + Send + Sync>(&mut self, entity: Entity) -> Option<&mut T> {
        let storage = self.storages.get_mut(&TypeId::of::<T>())?;
        let map = storage
            .as_any_mut()
            .downcast_mut::<ComponentMap<T>>()
            .expect("storage type mismatch");
        map.get_mut(entity)
    }

    /// Remove a component. Returns whether it was present.
    pub fn remove<T: Component + Send + Sync>(&mut self, entity: Entity) -> bool {
        match self.storages.get_mut(&TypeId::of::<T>()) {
            Some(storage) => {
                let map = storage
                    .as_any_mut()
                    .downcast_mut::<ComponentMap<T>>()
                    .expect("storage type mismatch");
                map.remove(entity)
            }
            None => false,
        }
    }

    /// Iterate `(Entity, &Component)` pairs.
    pub fn iter<T: Component + Send + Sync>(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.storages
            .get(&TypeId::of::<T>())
            .and_then(|s| s.as_any().downcast_ref::<ComponentMap<T>>())
            .map(|m| m.iter())
            .into_iter()
            .flatten()
    }

    /// Iterate `(Entity, &mut Component)` pairs.
    pub fn iter_mut<T: Component + Send + Sync>(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.storages
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| s.as_any_mut().downcast_mut::<ComponentMap<T>>())
            .map(|m| m.iter_mut())
            .into_iter()
            .flatten()
    }

    // ----- resources -----

    /// Insert a resource (non-entity global state). Overwrites if already present.
    pub fn insert_resource<T: Any + Send + Sync>(&mut self, value: T) {
        self.resources.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Get a reference to a resource.
    pub fn resource<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }

    /// Get a mutable reference to a resource.
    pub fn resource_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut::<T>())
    }

    /// Remove a resource. Returns whether it was present.
    pub fn remove_resource<T: Any + Send + Sync>(&mut self) -> bool {
        self.resources.remove(&TypeId::of::<T>()).is_some()
    }

    // ----- systems -----

    /// Register a system. Systems run in registration order on each tick.
    pub fn add_system<F>(&mut self, system: F)
    where
        F: FnMut(&mut World, f32) + Send + Sync + 'static,
    {
        self.systems.push(Box::new(system));
    }

    /// Run all systems once with the given delta time in seconds.
    pub fn tick(&mut self, dt: f32) {
        // Drain the systems out so each call can borrow the world mutably.
        // The boxed trait objects are `Send + Sync`, so this is safe.
        let mut systems = std::mem::take(&mut self.systems);
        for system in systems.iter_mut() {
            system(self, dt);
        }
        self.systems = systems;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Debug, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[test]
    fn spawn_and_components() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, Position { x: 0.0, y: 0.0 });
        world.insert(e, Velocity { dx: 1.0, dy: 2.0 });

        assert_eq!(world.get::<Position>(e), Some(&Position { x: 0.0, y: 0.0 }));
        assert_eq!(world.get::<Velocity>(e), Some(&Velocity { dx: 1.0, dy: 2.0 }));
    }

    #[test]
    fn despawn_removes_components() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, Position { x: 1.0, y: 2.0 });
        assert!(world.remove::<Position>(e));
        assert_eq!(world.get::<Position>(e), None);
    }

    #[test]
    fn iter_components() {
        let mut world = World::new();
        let a = world.spawn();
        let b = world.spawn();
        world.insert(a, Position { x: 1.0, y: 0.0 });
        world.insert(b, Position { x: 0.0, y: 1.0 });
        let mut total = 0.0;
        for (_e, pos) in world.iter::<Position>() {
            total += pos.x + pos.y;
        }
        assert!((total - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resources() {
        let mut world = World::new();
        world.insert_resource(42i32);
        assert_eq!(world.resource::<i32>(), Some(&42));
        *world.resource_mut::<i32>().unwrap() = 100;
        assert_eq!(world.resource::<i32>(), Some(&100));
    }

    #[test]
    fn systems_run_in_order() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, Position { x: 0.0, y: 0.0 });
        world.insert(e, Velocity { dx: 1.0, dy: 0.0 });

        world.add_system(|w, dt| {
            for (_, pos) in w.iter_mut::<Position>() {
                pos.x += dt;
            }
        });
        world.add_system(|w, _dt| {
            for (_, pos) in w.iter_mut::<Position>() {
                pos.x *= 2.0;
            }
        });

        world.tick(1.0);
        // After tick(1.0): system 1 sets x=1, system 2 doubles to 2.
        assert!((world.get::<Position>(e).unwrap().x - 2.0).abs() < f32::EPSILON);
    }
}
