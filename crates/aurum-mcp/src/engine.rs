//! The headless engine session an MCP client drives.
//!
//! [`Engine`] composes the pieces a running Aurum game has — a world, typed
//! state, an event queue, a time scale, registered modules, and the space
//! simulation — without a Godot process anywhere.
//!
//! Its serialized form is deliberately **the same shape** the Godot bridge
//! writes in `AurumNode.save_to_json`: `next_entity_id`, `time_scale`,
//! `state`, `components`, and `space`. A world saved from a running editor
//! loads here, and vice versa.

use std::collections::VecDeque;

use aurum_core::dynamic::{DynamicWorld, DynamicWorldError};
use aurum_core::prelude::{FixedTimestep, State, StateValue, TimeScale};
use aurum_space::SpaceSimulation;
use serde_json::{json, Value};

/// A pending event: a type name plus a JSON object payload.
///
/// This mirrors the Godot bridge's dynamic event, where the payload is always
/// an object so it can be handed to `emit_signal` as a Dictionary.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicEvent {
    pub type_name: String,
    pub data: Value,
}

/// Errors from loading a saved engine payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The payload was not a JSON object.
    NotAnObject,
    /// The `components` section was malformed.
    Components(DynamicWorldError),
    /// The `state` section was malformed.
    State(String),
    /// The `space` section was malformed.
    Space(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "save payload is not a JSON object"),
            Self::Components(e) => write!(f, "invalid components section: {e}"),
            Self::State(e) => write!(f, "invalid state section: {e}"),
            Self::Space(e) => write!(f, "invalid space section: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// The compile-time identifier of this headless runtime.
///
/// Mirrors `aurum-godot`'s `runtime_fingerprint`, so an agent can tell which
/// surface — and which build — answered a call.
pub fn runtime_fingerprint() -> &'static str {
    option_env!("AURUM_RUNTIME_FINGERPRINT")
        .filter(|value| !value.is_empty())
        .unwrap_or("aurum-mcp-unmanaged")
}

/// A headless Aurum engine session.
#[derive(Debug)]
pub struct Engine {
    world: DynamicWorld,
    state: State,
    events: VecDeque<DynamicEvent>,
    time_scale: TimeScale,
    fixed: FixedTimestep,
    modules: Vec<String>,
    space: SpaceSimulation,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An empty session at 60 Hz with a default space configuration.
    pub fn new() -> Self {
        Self {
            world: DynamicWorld::new(),
            state: State::new(),
            events: VecDeque::new(),
            time_scale: TimeScale::default(),
            fixed: FixedTimestep::default(),
            modules: Vec::new(),
            space: SpaceSimulation::default(),
        }
    }

    // ----- accessors -----

    pub fn world(&self) -> &DynamicWorld {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut DynamicWorld {
        &mut self.world
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    pub fn space(&self) -> &SpaceSimulation {
        &self.space
    }

    pub fn space_mut(&mut self) -> &mut SpaceSimulation {
        &mut self.space
    }

    pub fn time_scale(&self) -> f32 {
        self.time_scale.get()
    }

    pub fn set_time_scale(&mut self, scale: f32) {
        self.time_scale.set(scale);
    }

    pub fn modules(&self) -> &[String] {
        &self.modules
    }

    /// Register a module name. Idempotent, matching `AurumNode.register_module`.
    pub fn register_module(&mut self, name: &str) {
        if !self.modules.iter().any(|m| m == name) {
            self.modules.push(name.to_string());
        }
    }

    /// The fixed timestep in seconds (1/60 by default).
    pub fn fixed_step_seconds(&self) -> f32 {
        self.fixed.step()
    }

    // ----- events -----

    /// Queue an event for delivery on the next drain.
    ///
    /// A non-object payload is replaced with an empty object, matching the
    /// Godot bridge — the payload must be a Dictionary on that side.
    pub fn emit_event(&mut self, type_name: &str, data: Value) {
        let data = match data {
            Value::Object(_) => data,
            _ => Value::Object(serde_json::Map::new()),
        };
        self.events.push_back(DynamicEvent {
            type_name: type_name.to_string(),
            data,
        });
    }

    /// Number of queued, not-yet-drained events.
    pub fn pending_event_count(&self) -> usize {
        self.events.len()
    }

    /// Remove and return every queued event, oldest first.
    pub fn drain_events(&mut self) -> Vec<DynamicEvent> {
        self.events.drain(..).collect()
    }

    // ----- simulation -----

    /// Advance the space simulation by `real_seconds` of wall time.
    ///
    /// Uses the fixed timestep accumulator, scaled by the current time scale,
    /// so the result is frame-rate independent and reproducible. Returns the
    /// number of fixed ticks that ran.
    pub fn step_space(&mut self, real_seconds: f32) -> u32 {
        if real_seconds <= 0.0 {
            return 0;
        }
        let scaled = real_seconds * self.time_scale.get();
        let space = &mut self.space;
        self.fixed.advance(scaled, |dt| space.step(dt))
    }

    // ----- persistence -----

    /// The session as a save payload, in the Godot bridge's format.
    pub fn to_save_json(&self) -> Value {
        json!({
            "next_entity_id": self.world.next_entity_id(),
            "time_scale": self.time_scale.get(),
            "state": serde_json::to_value(&self.state).unwrap_or(Value::Null),
            "components": self.world.to_json(),
            "space": serde_json::to_value(&self.space).unwrap_or(Value::Null),
        })
    }

    /// Replace this session from a save payload.
    ///
    /// Every section is validated before anything is committed, so a rejected
    /// payload leaves the live session untouched.
    pub fn load_save_json(&mut self, payload: &Value) -> Result<(), EngineError> {
        let obj = payload.as_object().ok_or(EngineError::NotAnObject)?;

        // Parse everything fallibly first; mutate only once all of it is good.
        let mut world = DynamicWorld::new();
        if let Some(components) = obj.get("components") {
            world
                .from_json(components)
                .map_err(EngineError::Components)?;
        }
        if let Some(next) = obj.get("next_entity_id").and_then(Value::as_i64) {
            world.reserve_entity_ids_up_to(next);
        }

        let state = match obj.get("state") {
            Some(value) => serde_json::from_value::<State>(value.clone())
                .map_err(|e| EngineError::State(e.to_string()))?,
            None => State::new(),
        };

        let space = match obj.get("space") {
            Some(Value::Null) | None => SpaceSimulation::default(),
            Some(value) => serde_json::from_value::<SpaceSimulation>(value.clone())
                .map_err(|e| EngineError::Space(e.to_string()))?,
        };

        self.world = world;
        self.state = state;
        self.space = space;
        if let Some(scale) = obj.get("time_scale").and_then(Value::as_f64) {
            self.time_scale.set(scale as f32);
        }
        // Events are transient by design and are not part of a save.
        self.events.clear();
        Ok(())
    }

    /// Clear the world, state, events, and space simulation.
    ///
    /// Registered modules and the time scale survive, since they describe the
    /// session rather than its contents.
    pub fn reset(&mut self) {
        self.world.clear();
        self.state.clear();
        self.events.clear();
        self.space = SpaceSimulation::default();
        self.fixed = FixedTimestep::default();
    }

    // ----- diagnostics -----

    /// A single-call digest of the whole session.
    ///
    /// This is the grounding tool: an agent that calls this once knows the
    /// world, the state, the time scale, and the modules without an
    /// exploration loop. Entity payloads are bounded by `entity_limit` so a
    /// large world cannot silently blow up a model's context.
    pub fn snapshot(&self, include_entities: bool, entity_limit: usize) -> Value {
        let ids = self.world.entity_ids();
        let truncated = include_entities && ids.len() > entity_limit;

        let entities: Vec<Value> = if include_entities {
            ids.iter()
                .take(entity_limit)
                .map(|id| {
                    let components = self
                        .world
                        .components_of(*id)
                        .map(|c| {
                            let mut map = serde_json::Map::new();
                            for (k, v) in c {
                                map.insert(k.clone(), v.clone());
                            }
                            Value::Object(map)
                        })
                        .unwrap_or(Value::Null);
                    json!({ "id": id, "components": components })
                })
                .collect()
        } else {
            Vec::new()
        };

        let state = serde_json::to_value(&self.state).unwrap_or(Value::Null);

        let mut out = json!({
            "fingerprint": runtime_fingerprint(),
            "entity_count": self.world.entity_count(),
            "component_types": self.world.component_types(),
            "state": state,
            "time_scale": self.time_scale.get(),
            "paused": self.time_scale.paused(),
            "fixed_step_seconds": self.fixed.step(),
            "modules": self.modules,
            "pending_events": self.events.len(),
            "space": self.space.snapshot(),
        });

        if include_entities {
            out["entities"] = Value::Array(entities);
            if truncated {
                out["entities_truncated"] = json!(true);
                out["entities_omitted"] = json!(ids.len() - entity_limit);
            }
        }
        out
    }

    /// All state entries as a JSON object, sorted by key.
    pub fn state_entries(&self) -> Value {
        let mut map = serde_json::Map::new();
        let mut keys: Vec<&str> = self.state.keys().collect();
        keys.sort_unstable();
        for key in keys {
            let value = match self.state.get(key) {
                Some(StateValue::Bool(b)) => json!(b),
                Some(StateValue::Int(i)) => json!(i),
                Some(StateValue::Float(f)) => json!(f),
                Some(StateValue::String(s)) => json!(s),
                Some(StateValue::Json(raw)) => {
                    serde_json::from_str::<Value>(raw).unwrap_or_else(|_| json!(raw))
                }
                None => Value::Null,
            };
            map.insert(key.to_string(), value);
        }
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_never_empty() {
        assert!(!runtime_fingerprint().is_empty());
    }

    #[test]
    fn events_are_queued_then_drained_in_order() {
        let mut engine = Engine::new();
        engine.emit_event("PlayerHit", json!({"damage": 5}));
        engine.emit_event("Scored", json!({"points": 10}));
        assert_eq!(engine.pending_event_count(), 2);

        let drained = engine.drain_events();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].type_name, "PlayerHit");
        assert_eq!(drained[1].type_name, "Scored");
        assert_eq!(engine.pending_event_count(), 0);
    }

    #[test]
    fn non_object_event_payload_becomes_empty_object() {
        let mut engine = Engine::new();
        engine.emit_event("Ping", json!("not an object"));
        let drained = engine.drain_events();
        assert_eq!(drained[0].data, json!({}));
    }

    #[test]
    fn time_scale_clamps_like_the_core() {
        let mut engine = Engine::new();
        engine.set_time_scale(999.0);
        assert_eq!(engine.time_scale(), 100.0);
        engine.set_time_scale(-5.0);
        assert_eq!(engine.time_scale(), 0.0);
        assert!(engine.snapshot(false, 0)["paused"].as_bool().unwrap());
    }

    #[test]
    fn paused_time_scale_runs_no_simulation() {
        let mut engine = Engine::new();
        engine.set_time_scale(0.0);
        assert_eq!(engine.step_space(1.0), 0);
    }

    #[test]
    fn step_space_advances_at_the_fixed_rate() {
        let mut engine = Engine::new();
        // 1/60 s should produce exactly one fixed tick.
        assert_eq!(engine.step_space(1.0 / 60.0), 1);
        // A full second is capped by the spiral guard.
        assert_eq!(engine.step_space(1.0), 5);
    }

    #[test]
    fn save_and_load_round_trip_the_session() {
        let mut engine = Engine::new();
        let a = engine.world_mut().spawn();
        engine
            .world_mut()
            .set_component(a, "Position2D", json!({"x": 1.0, "y": 2.0}));
        engine.state_mut().set("score", 42i32);
        engine.set_time_scale(2.0);
        engine.register_module("aurum-space");

        let saved = engine.to_save_json();

        let mut restored = Engine::new();
        restored.load_save_json(&saved).unwrap();

        assert_eq!(restored.world().entity_count(), 1);
        assert_eq!(
            restored.world().get_component(a, "Position2D"),
            Some(&json!({"x": 1.0, "y": 2.0}))
        );
        assert_eq!(restored.state().get_int("score").unwrap(), 42);
        assert_eq!(restored.time_scale(), 2.0);
    }

    #[test]
    fn save_payload_uses_the_bridge_field_names() {
        let engine = Engine::new();
        let saved = engine.to_save_json();
        for field in [
            "next_entity_id",
            "time_scale",
            "state",
            "components",
            "space",
        ] {
            assert!(saved.get(field).is_some(), "missing bridge field {field}");
        }
    }

    #[test]
    fn load_accepts_a_bridge_shaped_payload() {
        // Exactly the shape AurumNode.save_to_json emits.
        let payload = json!({
            "next_entity_id": 4,
            "time_scale": 1.0,
            "state": { "values": { "score": { "Int": 7 } } },
            "components": { "1": { "Tag": "hero" } },
            "space": Value::Null,
        });
        let mut engine = Engine::new();
        engine.load_save_json(&payload).unwrap();
        assert_eq!(engine.world().entity_count(), 1);
        assert_eq!(engine.state().get_int("score").unwrap(), 7);
        assert_eq!(
            engine.world().next_entity_id(),
            4,
            "id counter must be honored"
        );
    }

    #[test]
    fn rejected_load_leaves_the_session_untouched() {
        let mut engine = Engine::new();
        let e = engine.world_mut().spawn();
        engine.world_mut().set_component(e, "Tag", json!(true));

        let bad = json!({ "components": { "not-an-id": {} } });
        assert!(engine.load_save_json(&bad).is_err());
        assert_eq!(engine.world().entity_count(), 1);

        assert!(engine.load_save_json(&json!([1, 2])).is_err());
        assert_eq!(engine.world().entity_count(), 1);
    }

    #[test]
    fn load_clears_transient_events() {
        let mut engine = Engine::new();
        engine.emit_event("Pending", json!({}));
        engine.load_save_json(&json!({})).unwrap();
        assert_eq!(engine.pending_event_count(), 0, "events are transient");
    }

    #[test]
    fn reset_clears_contents_but_keeps_session_settings() {
        let mut engine = Engine::new();
        let e = engine.world_mut().spawn();
        engine.world_mut().set_component(e, "Tag", json!(1));
        engine.state_mut().set("k", 1i32);
        engine.emit_event("X", json!({}));
        engine.register_module("aurum-2d");
        engine.set_time_scale(3.0);

        engine.reset();
        assert_eq!(engine.world().entity_count(), 0);
        assert_eq!(engine.state().len(), 0);
        assert_eq!(engine.pending_event_count(), 0);
        assert_eq!(engine.modules(), &["aurum-2d".to_string()]);
        assert_eq!(engine.time_scale(), 3.0);
    }

    #[test]
    fn snapshot_bounds_entity_payloads() {
        let mut engine = Engine::new();
        for _ in 0..5 {
            let e = engine.world_mut().spawn();
            engine.world_mut().set_component(e, "Tag", json!(1));
        }
        let snap = engine.snapshot(true, 2);
        assert_eq!(snap["entity_count"], 5);
        assert_eq!(snap["entities"].as_array().unwrap().len(), 2);
        assert_eq!(snap["entities_truncated"], true);
        assert_eq!(snap["entities_omitted"], 3);

        let full = engine.snapshot(false, 0);
        assert!(full.get("entities").is_none());
        assert_eq!(full["entity_count"], 5);
    }

    #[test]
    fn state_entries_sorts_keys_and_unwraps_variants() {
        let mut engine = Engine::new();
        engine.state_mut().set("zebra", 1i32);
        engine.state_mut().set("alpha", true);
        engine.state_mut().set("mid", "text");
        let entries = engine.state_entries();
        let keys: Vec<&String> = entries.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["alpha", "mid", "zebra"]);
        assert_eq!(entries["alpha"], true);
        assert_eq!(entries["mid"], "text");
        assert_eq!(entries["zebra"], 1);
    }
}
