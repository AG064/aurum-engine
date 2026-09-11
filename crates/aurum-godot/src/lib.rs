//! Aurum Godot shim for the GDExtension surface that GDScript sees.
//!
//! This crate exposes a single `AurumNode` Node class to Godot. It owns:
//!
//! - A `World` (typed Rust ECS) for Rust-side systems.
//! - A dynamic, JSON-blob component store for GDScript-authored entities.
//! - A `State` for typed global values with save/load.
//! - A FIFO dynamic event queue that bridges to Godot signals.
//!
//! GDScript uses the dynamic store (string-keyed components). Rust systems
//! can use the typed `World` directly. The two are independent. GDScript
//! doesn't have to know about Rust types, and Rust code doesn't have to
//! know about GDScript-defined components.
//!
//! ## GDScript API (AurumNode node)
//!
//! ```gdscript
//! # Entities
//! var e: int = Aurum.spawn()
//! Aurum.despawn(e)
//!
//! # Components (JSON-blob; GDScript passes Dictionaries)
//! Aurum.set_component(e, "Position2D", {"x": 0, "y": 0})
//! var pos: Dictionary = Aurum.get_component(e, "Position2D")
//! var enemies: Array = Aurum.get_entities_with("Enemy")
//!
//! # Events
//! Aurum.emit_event("PlayerHit", {"damage": 10})
//! Aurum.dispatch_events()  # delivers queued events, fires Godot signal
//!
//! # State
//! Aurum.state_set("score", 100)
//! print(Aurum.state_get("score", 0))
//!
//! # Save/Load
//! var json := Aurum.save_to_json()
//! Aurum.load_from_json(json)
//!
//! # Time
//! Aurum.set_time_scale(0.5)  # slow-mo
//! ```

mod bridge;
mod build_info;

use std::collections::{HashMap, HashSet, VecDeque};

use godot::classes::Node;
use godot::init::{ExtensionLibrary, InitLevel, gdextension};
use godot::prelude::*;

use aurum_core::ecs::World;
use aurum_space::{FlightConfig, FlightInput, SpaceClock, SpaceSimulation, SpaceSnapshot};
use aurum_core::state::{State, StateValue};
use aurum_vn::{Event as StoryEvent, Interpreter, Story, VarValue};

use bridge::{json_to_variant, variant_to_json};

/// Marker type for the GDExtension entry point. Required by `gdext`.
pub struct AurumExtension;

#[gdextension(entry_symbol = gdext_rust_init)]
unsafe impl ExtensionLibrary for AurumExtension {
    fn min_level() -> InitLevel {
        InitLevel::Scene
    }
}

/// The single Godot-facing class. Registered as `AurumNode`.
///
/// Editor lifecycle callbacks must remain side-effect free. Any future
/// runtime-only side effect must explicitly guard against editor execution.
#[derive(GodotClass)]
#[class(base=Node, rename=AurumNode, tool)]
pub struct AurumNode {
    base: Base<Node>,
    /// Typed Rust ECS (for Rust-side systems; optional for GDScript).
    ///
    /// Held for the node's whole lifetime so Rust-side systems have a world to
    /// query once they are registered; no in-crate reader exists yet.
    #[allow(dead_code)]
    pub(crate) world: World,
    /// Dynamic JSON-blob component store: entity -> {type_name -> value}.
    pub(crate) components: HashMap<i64, HashMap<String, serde_json::Value>>,
    /// Reverse index: type_name -> set of entities.
    pub(crate) by_type: HashMap<String, HashSet<i64>>,
    /// Next entity id to assign.
    pub(crate) next_entity_id: i64,
    /// FIFO queue for dynamic events emitted by GDScript.
    pub(crate) event_queue: VecDeque<DynamicEvent>,
    /// Typed space simulation. Games mirror snapshots into presentation nodes.
    pub(crate) space: SpaceSimulation,
    /// Fixed clock for the typed space simulation.
    pub(crate) space_clock: SpaceClock,
    /// Typed global state.
    pub(crate) state: State,
    /// Time scale (1.0 = normal).
    pub(crate) time_scale: f32,
    /// Registered module names.
    pub(crate) modules: Vec<String>,
    /// Story interpreter (set by `story_load`; None until a story is loaded).
    pub(crate) story: Option<Interpreter>,
}

#[godot_api]
impl INode for AurumNode {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            world: World::new(),
            components: HashMap::new(),
            by_type: HashMap::new(),
            next_entity_id: 1,
            event_queue: VecDeque::new(),
            space: SpaceSimulation::default(),
            space_clock: SpaceClock::default(),
            state: State::new(),
            time_scale: 1.0,
            modules: Vec::new(),
            story: None,
        }
    }
}

#[godot_api]
impl AurumNode {
    // ===== Signal =====
    //
    // Fired by `dispatch_events`. The first argument is the event type
    // name (e.g. "PlayerHit"); the second is the data as a Dictionary.

    #[signal]
    fn event_received(type_name: GString, data: Dictionary<GString, Variant>);

    // ===== Entities =====

    /// Create a new entity. Returns its id.
    #[func]
    fn spawn(&mut self) -> i64 {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        self.components.insert(id, HashMap::new());
        id
    }

    /// Destroy an entity and all its components.
    #[func]
    fn despawn(&mut self, entity: i64) -> bool {
        if let Some(comps) = self.components.remove(&entity) {
            for type_name in comps.keys() {
                if let Some(set) = self.by_type.get_mut(type_name) {
                    set.remove(&entity);
                }
            }
            true
        } else {
            false
        }
    }

    /// Check whether an entity exists.
    #[func]
    fn entity_exists(&self, entity: i64) -> bool {
        self.components.contains_key(&entity)
    }

    /// Total number of entities.
    #[func]
    fn entity_count(&self) -> i32 {
        self.components.len() as i32
    }

    // ===== Components (JSON-blob) =====

    /// Attach a component to an entity. The `data` Dictionary is serialized
    /// to JSON for storage. Returns true on success.
    #[func]
    fn set_component(
        &mut self,
        entity: i64,
        type_name: String,
        data: Variant,
    ) -> bool {
        let comps = match self.components.get_mut(&entity) {
            Some(c) => c,
            None => return false,
        };
        let json = variant_to_json(&data);
        comps.insert(type_name.clone(), json);
        self.by_type.entry(type_name).or_default().insert(entity);
        true
    }

    /// Read a component. Returns an empty Dictionary if missing.
    #[func]
    fn get_component(&self, entity: i64, type_name: String) -> Dictionary<GString, Variant> {
        let Some(comps) = self.components.get(&entity) else {
            return Dictionary::new();
        };
        let Some(json) = comps.get(&type_name) else {
            return Dictionary::new();
        };
        let variant = json_to_variant(json);
        match variant.try_to::<Dictionary<GString, Variant>>() {
            Ok(d) => d,
            Err(_) => Dictionary::new(),
        }
    }

    /// Check whether an entity has a component of the given type.
    #[func]
    fn has_component(&self, entity: i64, type_name: String) -> bool {
        self.components
            .get(&entity)
            .is_some_and(|c| c.contains_key(&type_name))
    }

    /// Remove a component. Returns whether it was present.
    #[func]
    fn remove_component(&mut self, entity: i64, type_name: String) -> bool {
        let Some(comps) = self.components.get_mut(&entity) else {
            return false;
        };
        let removed = comps.remove(&type_name).is_some();
        if removed {
            if let Some(set) = self.by_type.get_mut(&type_name) {
                set.remove(&entity);
            }
        }
        removed
    }

    /// Return all entities that have a component of the given type.
    #[func]
    fn get_entities_with(&self, type_name: String) -> Array<i64> {
        let mut out = Array::<i64>::new();
        if let Some(set) = self.by_type.get(&type_name) {
            for id in set {
                out.push(*id);
            }
        }
        out
    }

    /// Return all entities that have ALL the given component types.
    #[func]
    fn get_entities_with_all(&self, type_names: Array<GString>) -> Array<i64> {
        if type_names.is_empty() {
            return Array::<i64>::new();
        }
        let mut iter = type_names.iter_shared();
        let first = match iter.next() {
            Some(s) => s,
            None => return Array::<i64>::new(),
        };
        let mut acc: Option<HashSet<i64>> = self.by_type.get(&first.to_string()).cloned();
        for next in iter {
            let set = self.by_type.get(&next.to_string()).cloned();
            acc = match (acc, set) {
                (Some(a), Some(b)) => Some(a.intersection(&b).copied().collect()),
                _ => None,
            };
            if acc.is_none() {
                return Array::<i64>::new();
            }
        }
        let mut out = Array::<i64>::new();
        if let Some(set) = acc {
            for id in set {
                out.push(id);
            }
        }
        out
    }

    // ===== Events =====

    /// Queue an event. It will be delivered on the next `dispatch_events` call.
    #[func]
    fn emit_event(&mut self, type_name: String, data: Variant) {
        let json = variant_to_json(&data);
        let object = match json {
            serde_json::Value::Object(_) => json,
            _ => serde_json::Value::Object(serde_json::Map::new()),
        };
        self.event_queue.push_back(DynamicEvent {
            type_name,
            data: object,
        });
    }

    /// Drain the event queue. For each event, fire the `event_received`
    /// Godot signal with `(type_name, data)`.
    #[func]
    fn dispatch_events(&mut self) {
        while let Some(event) = self.event_queue.pop_front() {
            let dict = json_to_variant(&event.data)
                .try_to::<Dictionary<GString, Variant>>()
                .unwrap_or_default();
            self.base_mut().emit_signal(
                "event_received",
                &[event.type_name.to_variant(), dict.to_variant()],
            );
        }
    }

    // ===== State =====

    /// Read a state value. Returns `default` if not set.
    #[func]
    fn state_get(&self, key: String, default: Variant) -> Variant {
        match self.state.get(&key) {
            Some(v) => state_value_to_variant(v),
            None => default,
        }
    }

    /// Set a state value. The value must be bool, int, float, or String.
    /// Returns true on success.
    #[func]
    fn state_set(&mut self, key: String, value: Variant) -> bool {
        let sv = match variant_to_state_value(&value) {
            Some(v) => v,
            None => return false,
        };
        let key_static: &'static str = Box::leak(key.into_boxed_str());
        self.state.set(key_static, sv);
        true
    }

    /// Check whether a state key exists.
    #[func]
    fn state_has(&self, key: String) -> bool {
        self.state.get(&key).is_some()
    }

    /// Remove a state key. Returns whether it was present.
    #[func]
    fn state_remove(&mut self, key: String) -> bool {
        self.state.remove(&key).is_some()
    }

    /// Clear all state.
    #[func]
    fn state_clear(&mut self) {
        self.state.clear();
    }

    // ===== Time =====

    /// Set the time scale (1.0 = normal, 0.0 = paused, max 100.0).
    #[func]
    fn set_time_scale(&mut self, scale: f32) {
        self.time_scale = scale.clamp(0.0, 100.0);
    }

    /// Get the time scale.
    #[func]
    fn get_time_scale(&self) -> f32 {
        self.time_scale
    }

    // ===== Space simulation =====

    /// Configure the reusable Aurum 6DOF flight model for the active ship.
    ///
    /// The parameter list is the GDScript-facing API; it mirrors `FlightConfig`
    /// field by field and must not be restructured.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn space_configure(
        &mut self,
        mass_kg: f32,
        thrust_n: f32,
        rotation_accel_rad_s2: f32,
        max_rotation_rate_rad_s: f32,
        max_speed_mps: f32,
        boost_multiplier: f32,
        boost_fuel_per_s: f32,
        boost_heat_per_s: f32,
        fuel_capacity: f32,
        heat_capacity: f32,
        heat_dissipation_per_s: f32,
        shield_capacity: f32,
        hull_capacity: f32,
        flight_assist_damping: f32,
    ) -> bool {
        self.space.configure(FlightConfig {
            mass_kg,
            thrust_n,
            rotation_accel_rad_s2,
            max_rotation_rate_rad_s,
            max_speed_mps,
            boost_multiplier,
            boost_fuel_per_s,
            boost_heat_per_s,
            fuel_capacity,
            heat_capacity,
            heat_dissipation_per_s,
            shield_capacity,
            hull_capacity,
            flight_assist_damping,
            dampen_strength: (flight_assist_damping * 6.0).max(4.0),
        });
        true
    }

    /// Reset the active space simulation to its configured ship state.
    #[func]
    fn space_reset(&mut self) {
        self.space.reset();
        self.space_clock.reset();
    }

    /// Set whether the ship is docked. Docked ships do not integrate flight.
    #[func]
    fn space_set_docked(&mut self, docked: bool) {
        self.space.set_docked(docked);
    }

    /// Place the authoritative ship transform from a presentation or load
    /// boundary. Normal flight then owns subsequent transform changes.
    ///
    /// The parameter list is the GDScript-facing API: seven loose floats,
    /// because GDScript has no value type for a position/quaternion pair.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn space_set_transform(
        &mut self,
        position_x: f32,
        position_y: f32,
        position_z: f32,
        orientation_x: f32,
        orientation_y: f32,
        orientation_z: f32,
        orientation_w: f32,
    ) {
        self.space.set_transform(
            aurum_space::Vec3::new(position_x, position_y, position_z),
            aurum_space::Quat {
                x: orientation_x,
                y: orientation_y,
                z: orientation_z,
                w: orientation_w,
            },
        );
    }

    /// Set persistent ship status at a game load or service boundary.
    #[func]
    fn space_set_status(&mut self, fuel: f32, heat: f32, shield: f32, hull: f32) {
        self.space.set_status(fuel, heat, shield, hull);
    }

    /// Consume fuel through the authoritative space state.
    #[func]
    fn space_consume_fuel(&mut self, amount: f32) -> bool {
        self.space.consume_fuel(amount)
    }

    /// Stop linear and angular motion at a travel or respawn boundary.
    #[func]
    fn space_stop_motion(&mut self) {
        self.space.stop_motion();
    }

    /// Submit normalized control input. The values are consumed by the next
    /// fixed simulation ticks.
    ///
    /// The parameter list is the GDScript-facing API; it mirrors `FlightInput`
    /// field by field and must not be restructured.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn space_set_input(
        &mut self,
        pitch: f32,
        yaw: f32,
        roll: f32,
        thrust_forward: f32,
        thrust_lateral: f32,
        thrust_vertical: f32,
        boost: bool,
        dampen: bool,
        flight_assist: bool,
    ) {
        self.space.set_input(FlightInput {
            pitch,
            yaw,
            roll,
            thrust_forward,
            thrust_lateral,
            thrust_vertical,
            boost,
            dampen,
            flight_assist,
        });
    }

    /// Advance the space simulation using a real frame delta. Internally the
    /// simulation runs at Aurum's fixed physics rate.
    #[func]
    fn space_step(&mut self, real_delta: f32) -> Dictionary<GString, Variant> {
        self.space_clock.advance(real_delta.max(0.0), &mut self.space);
        space_snapshot_dict(self.space.snapshot())
    }

    /// Return the latest typed space snapshot for presentation and telemetry.
    #[func]
    fn space_snapshot(&self) -> Dictionary<GString, Variant> {
        space_snapshot_dict(self.space.snapshot())
    }

    /// Apply damage through the authoritative space state.
    #[func]
    fn space_apply_damage(&mut self, amount: f32) {
        self.space.apply_damage(amount);
    }

    /// Restore heat, shields, and hull to configured values.
    #[func]
    fn space_repair_full(&mut self) {
        self.space.repair_full();
    }

    /// Restore fuel to the configured capacity.
    #[func]
    fn space_refuel_full(&mut self) {
        self.space.refuel_full();
    }

    // ===== Save / Load =====

    /// Serialize the engine state to JSON. Includes state, components,
    /// entity id counter, and time scale. Does not include events (those
    /// are transient).
    #[func]
    fn save_to_json(&self) -> GString {
        let payload = serde_json::json!({
            "next_entity_id": self.next_entity_id,
            "time_scale": self.time_scale,
            "state": state_to_json(&self.state),
            "components": components_to_json(&self.components),
            "space": serde_json::to_value(&self.space).unwrap_or(serde_json::Value::Null),
        });
        let s: String = serde_json::to_string(&payload).unwrap_or_default();
        GString::from(s.as_str())
    }

    /// Load engine state from JSON. Replaces current state and components.
    /// Returns true on success.
    #[func]
    fn load_from_json(&mut self, json: GString) -> bool {
        let value: serde_json::Value = match serde_json::from_str(&json.to_string()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return false,
        };
        if let Some(id) = obj.get("next_entity_id").and_then(|v| v.as_i64()) {
            self.next_entity_id = id;
        }
        if let Some(ts) = obj.get("time_scale").and_then(|v| v.as_f64()) {
            self.time_scale = ts as f32;
        }
        if let Some(s) = obj.get("state") {
            self.state = match serde_json::from_value::<State>(s.clone()) {
                Ok(st) => st,
                Err(_) => return false,
            };
        }
        if let Some(c) = obj.get("components") {
            self.components = match serde_json::from_value(c.clone()) {
                Ok(m) => m,
                Err(_) => return false,
            };
            self.by_type.clear();
            for (entity_id, comps) in &self.components {
                for type_name in comps.keys() {
                    self.by_type
                        .entry(type_name.clone())
                        .or_default()
                        .insert(*entity_id);
                }
            }
        }
        if let Some(space) = obj.get("space") {
            self.space = match serde_json::from_value(space.clone()) {
                Ok(simulation) => simulation,
                Err(_) => return false,
            };
            self.space_clock.reset();
        }
        true
    }

    // ===== Build diagnostics =====

    /// Return the compile-time identifier of the loaded development runtime.
    #[func]
    fn runtime_fingerprint(&self) -> GString {
        GString::from(build_info::runtime_fingerprint())
    }

    // ===== Modules =====

    /// Register a module by name. Idempotent.
    #[func]
    fn register_module(&mut self, name: String) {
        if !self.modules.contains(&name) {
            self.modules.push(name);
        }
    }

    /// List registered module names.
    #[func]
    fn list_modules(&self) -> Array<GString> {
        let mut out = Array::<GString>::new();
        for m in &self.modules {
            let gs = GString::from(m.as_str());
            out.push(&gs);
        }
        out
    }

    /// Check whether a module is registered.
    #[func]
    fn has_module(&self, name: String) -> bool {
        self.modules.contains(&name)
    }

    // ===== Story (visual novel) =====

    /// Load a story from JSON. The story becomes the active story;
    /// subsequent `story_advance` calls return entries from it.
    /// Returns "" on success, an error message on failure.
    #[func]
    fn story_load(&mut self, json: GString, start_scene: GString) -> GString {
        let story = match Story::from_json(&json.to_string()) {
            Ok(s) => s,
            Err(e) => return GString::from(format!("{:?}", e).as_str()),
        };
        match Interpreter::new(story, &start_scene.to_string()) {
            Ok(interp) => {
                self.story = Some(interp);
                GString::new()
            }
            Err(e) => GString::from(format!("{:?}", e).as_str()),
        }
    }

    /// Check whether a story is currently loaded.
    #[func]
    fn story_is_loaded(&self) -> bool {
        self.story.is_some()
    }

    /// Advance the story and return the next event as a Dictionary.
    ///
    /// Shapes:
    /// - `{"type": "dialogue", "speaker": ..., "text": ..., "presentation": ..., ...}`
    /// - `{"type": "choice", "entry_index": ..., "choices": [{"text": ..., "goto": ...}]}`
    /// - `{"type": "scene_ended"}` / `"quit"` / `"goto"` / `"command"` / `"error"`
    #[func]
    fn story_advance(&mut self) -> Dictionary<GString, Variant> {
        let Some(interp) = self.story.as_mut() else {
            let msg = GString::from("No story loaded");
            return story_event_dict("error", &[("message", msg.to_variant())]);
        };
        match interp.advance() {
            StoryEvent::Dialogue {
                speaker,
                text,
                character,
                position,
                background,
                emotion,
                presentation,
                text_key,
                speaker_key,
                append,
            } => {
                let mut pairs: Vec<(&str, Variant)> = vec![
                    ("type", "dialogue".to_variant()),
                    ("text", text.to_variant()),
                    ("presentation", presentation.to_variant()),
                    ("append", append.to_variant()),
                    ("character", character.to_variant()),
                    ("position", position.to_variant()),
                    ("background", background.to_variant()),
                    ("emotion", emotion.to_variant()),
                    ("text_key", text_key.to_variant()),
                    ("speaker_key", speaker_key.to_variant()),
                ];
                if let Some(s) = speaker {
                    pairs.push(("speaker", s.to_variant()));
                } else {
                    pairs.push(("speaker", Variant::nil()));
                }
                story_event_dict_from_pairs(&pairs)
            }
            StoryEvent::Choice { entry_index, choices } => {
                let mut arr = VarArray::new();
                for c in choices {
                    let mut d = Dictionary::<GString, Variant>::new();
                    d.set("text", c.text);
                    d.set("goto", c.goto);
                    d.set("text_key", c.text_key);
                    arr.push(&d.to_variant());
                }
                let pairs: Vec<(&str, Variant)> = vec![
                    ("type", "choice".to_variant()),
                    ("entry_index", (entry_index as i64).to_variant()),
                    ("choices", arr.to_variant()),
                ];
                story_event_dict_from_pairs(&pairs)
            }
            StoryEvent::SceneEnded => story_event_dict("scene_ended", &[]),
            StoryEvent::Quit => story_event_dict("quit", &[]),
            StoryEvent::Goto(target) => {
                let t = GString::from(target.as_str());
                story_event_dict("goto", &[("target", t.to_variant())])
            }
            StoryEvent::Command(cmd) => {
                let v = json_to_variant(&cmd);
                let d = match v.try_to::<Dictionary<GString, Variant>>() {
                    Ok(d) => d,
                    Err(_) => Dictionary::new(),
                };
                let pairs: Vec<(&str, Variant)> = vec![
                    ("type", "command".to_variant()),
                    ("command", d.to_variant()),
                ];
                story_event_dict_from_pairs(&pairs)
            }
            StoryEvent::Error(msg) => {
                let m = GString::from(msg.as_str());
                story_event_dict("error", &[("message", m.to_variant())])
            }
        }
    }

    /// Apply a choice (by visible index from the most recent Choice event).
    /// Returns "" on success, an error message on failure.
    #[func]
    fn story_pick_choice(&mut self, index: i64) -> GString {
        let Some(interp) = self.story.as_mut() else {
            return GString::from("No story loaded");
        };
        let idx = match usize::try_from(index) {
            Ok(i) => i,
            Err(_) => return GString::from("Choice index cannot be negative"),
        };
        match interp.pick_choice(idx) {
            Ok(()) => GString::new(),
            Err(e) => GString::from(format!("{:?}", e).as_str()),
        }
    }

    /// Jump to a target. The target can be a scene name, a scene name with
    /// a label (`scene:label`), or a label in the current scene (`.label`).
    /// Returns "" on success, an error message on failure.
    #[func]
    fn story_jump_to(&mut self, target: GString) -> GString {
        let Some(interp) = self.story.as_mut() else {
            return GString::from("No story loaded");
        };
        match interp.jump_to(&target.to_string()) {
            Ok(()) => GString::new(),
            Err(e) => GString::from(format!("{:?}", e).as_str()),
        }
    }

    /// Get a story variable. Returns the default if not set or the type
    /// doesn't match.
    #[func]
    fn story_get_variable(&self, key: GString, default: Variant) -> Variant {
        let Some(interp) = self.story.as_ref() else {
            return default;
        };
        match interp.state().variables.get(&key.to_string()) {
            Some(VarValue::Bool(b)) => b.to_variant(),
            Some(VarValue::Number(n)) => n.to_variant(),
            Some(VarValue::String(s)) => s.to_variant(),
            None => default,
        }
    }

    /// Set a story variable. The value must be bool, int, float, or string.
    /// Returns true on success.
    #[func]
    fn story_set_variable(&mut self, key: GString, value: Variant) -> bool {
        let Some(interp) = self.story.as_mut() else {
            return false;
        };
        let sv = if let Ok(b) = value.try_to::<bool>() {
            VarValue::Bool(b)
        } else if let Ok(i) = value.try_to::<i64>() {
            VarValue::Number(i as f64)
        } else if let Ok(f) = value.try_to::<f64>() {
            VarValue::Number(f)
        } else if let Ok(s) = value.try_to::<GString>() {
            VarValue::String(s.to_string())
        } else {
            return false;
        };
        interp.state_mut().variables.insert(key.to_string(), sv);
        true
    }

    /// Export the story state as JSON for save files.
    #[func]
    fn story_export_state(&self) -> GString {
        match self.story.as_ref() {
            Some(interp) => GString::from(interp.export_state().as_str()),
            None => GString::new(),
        }
    }

    /// Import story state from JSON. Returns true on success.
    #[func]
    fn story_import_state(&mut self, json: GString) -> bool {
        match self.story.as_mut() {
            Some(interp) => interp.import_state(&json.to_string()),
            None => false,
        }
    }

    /// Current scene name, or "" if no story is loaded.
    #[func]
    fn story_current_scene(&self) -> GString {
        match self.story.as_ref() {
            Some(interp) => {
                let s = interp.state().current_scene.clone();
                GString::from(s.as_str())
            }
            None => GString::new(),
        }
    }

    /// Current entry index (0-based), or -1 if no story is loaded.
    #[func]
    fn story_current_entry_index(&self) -> i32 {
        match self.story.as_ref() {
            Some(interp) => interp.state().current_entry_index,
            None => -1,
        }
    }
}

// --- Internal: dynamic event, flush, conversions ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DynamicEvent {
    pub(crate) type_name: String,
    pub(crate) data: serde_json::Value,
}

fn state_value_to_variant(v: &StateValue) -> Variant {
    match v {
        StateValue::Bool(b) => b.to_variant(),
        StateValue::Int(i) => i.to_variant(),
        StateValue::Float(f) => f.to_variant(),
        StateValue::String(s) => s.to_variant(),
        StateValue::Json(s) => s.to_variant(),
    }
}

fn variant_to_state_value(v: &Variant) -> Option<StateValue> {
    if let Ok(b) = v.try_to::<bool>() {
        return Some(StateValue::Bool(b));
    }
    if let Ok(i) = v.try_to::<i64>() {
        return Some(StateValue::Int(i));
    }
    if let Ok(f) = v.try_to::<f64>() {
        return Some(StateValue::Float(f));
    }
    if let Ok(s) = v.try_to::<GString>() {
        return Some(StateValue::String(s.to_string()));
    }
    None
}

fn state_to_json(state: &State) -> serde_json::Value {
    serde_json::to_value(state).unwrap_or(serde_json::Value::Null)
}

fn components_to_json(
    comps: &HashMap<i64, HashMap<String, serde_json::Value>>,
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (entity, types) in comps {
        let mut inner = serde_json::Map::new();
        for (k, v) in types {
            inner.insert(k.clone(), v.clone());
        }
        out.insert(entity.to_string(), serde_json::Value::Object(inner));
    }
    serde_json::Value::Object(out)
}

fn space_snapshot_dict(snapshot: SpaceSnapshot) -> Dictionary<GString, Variant> {
    let mut out = Dictionary::<GString, Variant>::new();
    set_space_value(&mut out, "sector_x", snapshot.sector.x.to_variant());
    set_space_value(&mut out, "sector_y", snapshot.sector.y.to_variant());
    set_space_value(&mut out, "sector_z", snapshot.sector.z.to_variant());
    set_space_value(&mut out, "position_x", snapshot.local_position.x.to_variant());
    set_space_value(&mut out, "position_y", snapshot.local_position.y.to_variant());
    set_space_value(&mut out, "position_z", snapshot.local_position.z.to_variant());
    set_space_value(&mut out, "orientation_x", snapshot.orientation.x.to_variant());
    set_space_value(&mut out, "orientation_y", snapshot.orientation.y.to_variant());
    set_space_value(&mut out, "orientation_z", snapshot.orientation.z.to_variant());
    set_space_value(&mut out, "orientation_w", snapshot.orientation.w.to_variant());
    set_space_value(&mut out, "velocity_x", snapshot.velocity.x.to_variant());
    set_space_value(&mut out, "velocity_y", snapshot.velocity.y.to_variant());
    set_space_value(&mut out, "velocity_z", snapshot.velocity.z.to_variant());
    set_space_value(&mut out, "angular_velocity_x", snapshot.angular_velocity.x.to_variant());
    set_space_value(&mut out, "angular_velocity_y", snapshot.angular_velocity.y.to_variant());
    set_space_value(&mut out, "angular_velocity_z", snapshot.angular_velocity.z.to_variant());
    set_space_value(&mut out, "fuel", snapshot.fuel.to_variant());
    set_space_value(&mut out, "heat", snapshot.heat.to_variant());
    set_space_value(&mut out, "shield", snapshot.shield.to_variant());
    set_space_value(&mut out, "hull", snapshot.hull.to_variant());
    set_space_value(&mut out, "boost_active", snapshot.boost_active.to_variant());
    set_space_value(&mut out, "docked", snapshot.docked.to_variant());
    set_space_value(&mut out, "tick", (snapshot.tick as i64).to_variant());
    out
}

fn set_space_value(
    dictionary: &mut Dictionary<GString, Variant>,
    key: &str,
    value: Variant,
) {
    let key = GString::from(key);
    dictionary.set(&key, &value);
}

// Story event helpers build Dictionary payloads for `story_advance`.

fn story_event_dict(type_name: &str, extras: &[(&str, Variant)]) -> Dictionary<GString, Variant> {
    let mut pairs: Vec<(&str, Variant)> =
        vec![("type", GString::from(type_name).to_variant())];
    pairs.extend_from_slice(extras);
    story_event_dict_from_pairs(&pairs)
}

fn story_event_dict_from_pairs(
    pairs: &[(&str, Variant)],
) -> Dictionary<GString, Variant> {
    let mut d = Dictionary::<GString, Variant>::new();
    for (k, v) in pairs {
        let key_gs = GString::from(*k);
        d.set(&key_gs, v);
    }
    d
}
