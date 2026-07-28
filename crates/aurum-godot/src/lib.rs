//! Aurum Godot shim — the GDExtension surface that GDScript sees.
//!
//! This crate exposes a single `Mavis` Node class to Godot. It owns:
//!
//! - A `World` (typed Rust ECS) for Rust-side systems.
//! - A dynamic, JSON-blob component store for GDScript-authored entities.
//! - A `State` for typed global values with save/load.
//! - A typed `EventBus` that bridges to Godot signals.
//!
//! GDScript uses the dynamic store (string-keyed components). Rust systems
//! can use the typed `World` directly. The two are independent — GDScript
//! doesn't have to know about Rust types, and Rust code doesn't have to
//! know about GDScript-defined components.
//!
//! ## GDScript API (Mavis node)
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

use std::collections::{HashMap, HashSet};

use godot::classes::Node;
use godot::init::{ExtensionLibrary, InitLevel, gdextension};
use godot::prelude::*;

use aurum_core::ecs::World;
use aurum_core::events::EventBus;
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

/// The single Godot-facing class. Registered as `Mavis`.
#[derive(GodotClass)]
#[class(base=Node, rename=Mavis)]
pub struct Mavis {
    base: Base<Node>,
    /// Typed Rust ECS (for Rust-side systems; optional for GDScript).
    pub(crate) world: World,
    /// Dynamic JSON-blob component store: entity -> {type_name -> value}.
    pub(crate) components: HashMap<i64, HashMap<String, serde_json::Value>>,
    /// Reverse index: type_name -> set of entities.
    pub(crate) by_type: HashMap<String, HashSet<i64>>,
    /// Next entity id to assign.
    pub(crate) next_entity_id: i64,
    /// Typed event bus.
    pub(crate) events: EventBus,
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
impl INode for Mavis {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            world: World::new(),
            components: HashMap::new(),
            by_type: HashMap::new(),
            next_entity_id: 1,
            events: EventBus::new(),
            state: State::new(),
            time_scale: 1.0,
            modules: Vec::new(),
            story: None,
        }
    }
}

#[godot_api]
impl Mavis {
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
        while let Some(next) = iter.next() {
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
    fn emit_event(&mut self, type_name: String, data: Dictionary<GString, Variant>) {
        let json = variant_to_json(&data.to_variant());
        self.events.emit(DynamicEvent {
            type_name,
            data: json,
        });
    }

    /// Drain the event queue. For each event, fire the `event_received`
    /// Godot signal with `(type_name, data)`.
    #[func]
    fn dispatch_events(&mut self) {
        while self.events.pending() > 0 {
            self.flush_one_event();
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
        true
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
                    let _ = arr.push(&d.to_variant());
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

impl Mavis {
    /// Internal: drain one event from the bus, fire the Godot signal.
    fn flush_one_event(&mut self) -> bool {
        use std::sync::{Arc, Mutex};
        let captured: Arc<Mutex<Option<DynamicEvent>>> = Arc::new(Mutex::new(None));
        let captured_clone = captured.clone();
        let _sub_id = self.events.subscribe::<DynamicEvent, _>(move |e| {
            *captured_clone.lock().unwrap() = Some(DynamicEvent {
                type_name: e.type_name.clone(),
                data: e.data.clone(),
            });
        });
        self.events.dispatch();
        let event = match captured.lock().unwrap().take() {
            Some(e) => e,
            None => return false,
        };
        let dict = json_to_variant(&event.data)
            .try_to::<Dictionary<GString, Variant>>()
            .unwrap_or_default();
        self.base_mut()
            .emit_signal("event_received", &[event.type_name.to_variant(), dict.to_variant()]);
        true
    }
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

// Story event helpers — build Dictionary payloads for `story_advance`.

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
