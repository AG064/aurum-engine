//! Dynamic, string-keyed world: entities with JSON-blob components.
//!
//! [`crate::ecs::World`] is generically typed. Components there are Rust types
//! keyed by [`std::any::TypeId`], which is fast and type-safe but cannot be
//! driven by anything that only knows component names as strings — a save
//! file, a script, or an external tool such as an MCP client.
//!
//! `DynamicWorld` is that scriptable counterpart. Entities are `i64` handles
//! and components are `serde_json::Value` blobs stored under a `String` type
//! name. It is the same model the Godot bridge exposes as
//! `AurumNode.set_component` / `get_component`, so a world saved on one
//! surface loads on the other.
//!
//! # When to use which
//!
//! - Typed systems, per-tick simulation, anything performance-sensitive:
//!   [`crate::ecs::World`].
//! - Persistence, tooling, editors, AI agents, cross-surface interchange:
//!   `DynamicWorld`.
//!
//! The two are complementary rather than competing. A genre module can keep
//! its hot loop on typed components and mirror the durable parts here.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

/// Errors produced when loading a dynamic world from JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicWorldError {
    /// The payload was not a JSON object.
    NotAnObject,
    /// The `components` field was present but not shaped like
    /// `{ "<entity id>": { "<type name>": <value> } }`.
    BadComponents,
    /// An entity key was not a valid integer id.
    BadEntityId(String),
    /// The `next_entity_id` field was present but not a positive integer.
    BadNextEntityId,
}

impl std::fmt::Display for DynamicWorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "dynamic world payload is not a JSON object"),
            Self::BadComponents => {
                write!(
                    f,
                    "'components' must be an object of entity id to component map"
                )
            }
            Self::BadEntityId(id) => write!(f, "'{id}' is not a valid entity id"),
            Self::BadNextEntityId => write!(f, "'next_entity_id' must be a positive integer"),
        }
    }
}

impl std::error::Error for DynamicWorldError {}

/// A world of entities carrying JSON-blob components, addressed by name.
///
/// Entity ids start at 1 and only ever move forward, matching
/// [`crate::ecs::World`]. Handles stay valid across component add and remove;
/// only [`DynamicWorld::despawn`] invalidates one.
#[derive(Debug, Clone, Default)]
pub struct DynamicWorld {
    next_entity_id: i64,
    /// `entity id -> (type name -> component blob)`.
    components: HashMap<i64, HashMap<String, Value>>,
    /// Reverse index so "which entities have X" is not a full scan.
    by_type: HashMap<String, BTreeSet<i64>>,
}

impl DynamicWorld {
    /// An empty world. The first spawned entity gets id 1.
    pub fn new() -> Self {
        Self {
            next_entity_id: 1,
            components: HashMap::new(),
            by_type: HashMap::new(),
        }
    }

    // ----- entities -----

    /// Create an entity and return its id.
    pub fn spawn(&mut self) -> i64 {
        let id = self.next_entity_id;
        self.next_entity_id = self
            .next_entity_id
            .checked_add(1)
            .expect("dynamic world entity id overflow");
        self.components.insert(id, HashMap::new());
        id
    }

    /// Destroy an entity and all of its components.
    ///
    /// Returns whether the entity existed.
    pub fn despawn(&mut self, entity: i64) -> bool {
        let Some(comps) = self.components.remove(&entity) else {
            return false;
        };
        for type_name in comps.keys() {
            if let Some(set) = self.by_type.get_mut(type_name) {
                set.remove(&entity);
                if set.is_empty() {
                    self.by_type.remove(type_name);
                }
            }
        }
        true
    }

    /// Whether an entity with this id currently exists.
    pub fn exists(&self, entity: i64) -> bool {
        self.components.contains_key(&entity)
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        self.components.len()
    }

    /// Every live entity id, ascending.
    pub fn entity_ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.components.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    // ----- components -----

    /// Attach or replace a component blob.
    ///
    /// Returns `false` when the entity does not exist, in which case nothing
    /// is written. Callers that want implicit creation should call
    /// [`DynamicWorld::spawn`] first.
    pub fn set_component(&mut self, entity: i64, type_name: &str, data: Value) -> bool {
        let Some(comps) = self.components.get_mut(&entity) else {
            return false;
        };
        comps.insert(type_name.to_string(), data);
        self.by_type
            .entry(type_name.to_string())
            .or_default()
            .insert(entity);
        true
    }

    /// Borrow a component blob.
    pub fn get_component(&self, entity: i64, type_name: &str) -> Option<&Value> {
        self.components.get(&entity)?.get(type_name)
    }

    /// Whether the entity carries this component type.
    pub fn has_component(&self, entity: i64, type_name: &str) -> bool {
        self.components
            .get(&entity)
            .is_some_and(|c| c.contains_key(type_name))
    }

    /// Remove a component. Returns whether it was present.
    pub fn remove_component(&mut self, entity: i64, type_name: &str) -> bool {
        let Some(comps) = self.components.get_mut(&entity) else {
            return false;
        };
        let removed = comps.remove(type_name).is_some();
        if removed {
            if let Some(set) = self.by_type.get_mut(type_name) {
                set.remove(&entity);
                if set.is_empty() {
                    self.by_type.remove(type_name);
                }
            }
        }
        removed
    }

    /// Every entity carrying `type_name`, ascending.
    pub fn entities_with(&self, type_name: &str) -> Vec<i64> {
        self.by_type
            .get(type_name)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Every entity carrying **all** of `type_names`, ascending.
    ///
    /// An empty `type_names` matches nothing, which is the useful reading for
    /// an agent asking "what has all of these" with an empty list.
    pub fn entities_with_all(&self, type_names: &[String]) -> Vec<i64> {
        let mut iter = type_names.iter();
        let Some(first) = iter.next() else {
            return Vec::new();
        };
        let Some(acc) = self.by_type.get(first) else {
            return Vec::new();
        };
        let mut acc = acc.clone();
        for name in iter {
            let Some(next) = self.by_type.get(name) else {
                return Vec::new();
            };
            acc = acc.intersection(next).copied().collect();
            if acc.is_empty() {
                return Vec::new();
            }
        }
        acc.into_iter().collect()
    }

    /// Every component type name currently in use, sorted.
    pub fn component_types(&self) -> Vec<String> {
        let mut names: Vec<String> = self.by_type.keys().cloned().collect();
        names.sort();
        names
    }

    /// The component map for one entity, or `None` if it does not exist.
    pub fn components_of(&self, entity: i64) -> Option<&HashMap<String, Value>> {
        self.components.get(&entity)
    }

    /// The id that the next [`DynamicWorld::spawn`] will return.
    pub fn next_entity_id(&self) -> i64 {
        self.next_entity_id
    }

    /// Raise the next-entity counter so future spawns cannot reuse an id that
    /// a save file already handed out.
    ///
    /// Never lowers the counter, so this is safe to call with a stale value.
    pub fn reserve_entity_ids_up_to(&mut self, next_id: i64) {
        if next_id > self.next_entity_id {
            self.next_entity_id = next_id;
        }
    }

    /// Remove every entity and component, resetting the id counter.
    pub fn clear(&mut self) {
        self.components.clear();
        self.by_type.clear();
        self.next_entity_id = 1;
    }

    // ----- persistence -----

    /// The whole world as a JSON object.
    ///
    /// Entity ids become JSON object keys, so they are strings on the wire.
    /// This is the shape the Godot bridge writes and reads.
    pub fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (entity, types) in &self.components {
            let mut inner = serde_json::Map::new();
            for (name, blob) in types {
                inner.insert(name.clone(), blob.clone());
            }
            out.insert(entity.to_string(), Value::Object(inner));
        }
        Value::Object(out)
    }

    /// Replace this world's contents from a [`DynamicWorld::to_json`] payload.
    ///
    /// The reverse index is rebuilt, so a loaded world is immediately
    /// queryable. `next_entity_id` is raised to one past the highest loaded
    /// id so subsequently spawned entities cannot collide with loaded ones.
    pub fn from_json(&mut self, payload: &Value) -> Result<(), DynamicWorldError> {
        let obj = payload.as_object().ok_or(DynamicWorldError::NotAnObject)?;

        let mut components: HashMap<i64, HashMap<String, Value>> = HashMap::new();
        let mut highest: i64 = 0;

        for (key, value) in obj {
            let entity: i64 = key
                .parse()
                .map_err(|_| DynamicWorldError::BadEntityId(key.clone()))?;
            let types = value.as_object().ok_or(DynamicWorldError::BadComponents)?;
            let mut inner = HashMap::new();
            for (name, blob) in types {
                inner.insert(name.clone(), blob.clone());
            }
            highest = highest.max(entity);
            components.insert(entity, inner);
        }

        self.components = components;
        self.by_type.clear();
        for (entity, types) in &self.components {
            for name in types.keys() {
                self.by_type
                    .entry(name.clone())
                    .or_default()
                    .insert(*entity);
            }
        }
        self.next_entity_id = highest.saturating_add(1).max(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spawn_assigns_ascending_ids_from_one() {
        let mut world = DynamicWorld::new();
        assert_eq!(world.spawn(), 1);
        assert_eq!(world.spawn(), 2);
        assert_eq!(world.entity_count(), 2);
        assert_eq!(world.entity_ids(), vec![1, 2]);
    }

    #[test]
    fn components_round_trip_by_name() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        assert!(world.set_component(e, "Position2D", json!({"x": 1.5, "y": -2.0})));

        assert_eq!(
            world.get_component(e, "Position2D"),
            Some(&json!({"x": 1.5, "y": -2.0}))
        );
        assert!(world.has_component(e, "Position2D"));
        assert!(!world.has_component(e, "Velocity2D"));
        assert_eq!(world.get_component(e, "Velocity2D"), None);
    }

    #[test]
    fn set_component_on_missing_entity_is_refused() {
        let mut world = DynamicWorld::new();
        assert!(!world.set_component(99, "Position2D", json!({"x": 0.0})));
        assert_eq!(world.entity_count(), 0);
    }

    #[test]
    fn despawn_drops_components_and_reverse_index() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        world.set_component(e, "Position2D", json!({"x": 0.0}));
        assert_eq!(world.entities_with("Position2D"), vec![e]);

        assert!(world.despawn(e));
        assert!(!world.despawn(e));
        assert_eq!(world.entity_count(), 0);
        assert!(world.entities_with("Position2D").is_empty());
        assert!(world.component_types().is_empty());
    }

    #[test]
    fn remove_component_prunes_empty_type() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        world.set_component(e, "Health", json!(100));
        assert!(world.remove_component(e, "Health"));
        assert!(!world.remove_component(e, "Health"));
        assert!(world.component_types().is_empty());
    }

    #[test]
    fn entities_with_all_is_an_intersection() {
        let mut world = DynamicWorld::new();
        let a = world.spawn();
        let b = world.spawn();
        let c = world.spawn();
        world.set_component(a, "Position2D", json!({}));
        world.set_component(b, "Position2D", json!({}));
        world.set_component(c, "Position2D", json!({}));
        world.set_component(a, "Player", json!({}));
        world.set_component(b, "Enemy", json!({}));

        assert_eq!(
            world.entities_with_all(&["Position2D".into(), "Player".into()]),
            vec![a]
        );
        assert_eq!(
            world.entities_with_all(&["Player".into(), "Enemy".into()]),
            Vec::<i64>::new()
        );
        assert_eq!(
            world.entities_with_all(&["Position2D".into()]),
            vec![a, b, c]
        );
    }

    #[test]
    fn entities_with_all_with_empty_list_matches_nothing() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        world.set_component(e, "Position2D", json!({}));
        assert!(world.entities_with_all(&[]).is_empty());
    }

    #[test]
    fn json_round_trip_preserves_entities_and_components() {
        let mut world = DynamicWorld::new();
        let a = world.spawn();
        let b = world.spawn();
        world.set_component(a, "Position2D", json!({"x": 3.0, "y": 4.0}));
        world.set_component(b, "Tag", json!("hero"));

        let payload = world.to_json();
        let mut restored = DynamicWorld::new();
        restored.from_json(&payload).unwrap();

        assert_eq!(restored.entity_count(), 2);
        assert_eq!(
            restored.get_component(a, "Position2D"),
            Some(&json!({"x": 3.0, "y": 4.0}))
        );
        assert_eq!(restored.get_component(b, "Tag"), Some(&json!("hero")));
        // The reverse index must be rebuilt, not merely the raw map.
        assert_eq!(restored.entities_with("Position2D"), vec![a]);
    }

    #[test]
    fn load_advances_next_id_past_loaded_entities() {
        let mut world = DynamicWorld::new();
        world
            .from_json(&json!({"7": {"Tag": true}, "3": {"Tag": true}}))
            .unwrap();
        assert_eq!(world.next_entity_id(), 8);
        assert_eq!(world.spawn(), 8, "a spawn after load must not collide");
    }

    #[test]
    fn load_rejects_malformed_payloads() {
        let mut world = DynamicWorld::new();
        assert_eq!(
            world.from_json(&json!([1, 2, 3])),
            Err(DynamicWorldError::NotAnObject)
        );
        assert_eq!(
            world.from_json(&json!({"not-an-id": {}})),
            Err(DynamicWorldError::BadEntityId("not-an-id".into()))
        );
        assert_eq!(
            world.from_json(&json!({"1": 5})),
            Err(DynamicWorldError::BadComponents)
        );
    }

    #[test]
    fn failed_load_leaves_world_untouched() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        world.set_component(e, "Tag", json!(true));
        assert!(world.from_json(&json!({"bad": {}})).is_err());
        assert_eq!(
            world.entity_count(),
            1,
            "a rejected load must not clear the world"
        );
    }

    #[test]
    fn clear_resets_ids() {
        let mut world = DynamicWorld::new();
        let e = world.spawn();
        world.set_component(e, "Tag", json!(1));
        world.clear();
        assert_eq!(world.entity_count(), 0);
        assert_eq!(world.spawn(), 1);
    }
}
