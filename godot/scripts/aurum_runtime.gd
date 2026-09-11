extends Node

# Aurum runtime, autoloaded as `Aurum`.
#
# The single GDScript-facing entry point. Wraps the `AurumNode` GDExtension
# node (which is the actual engine) and exposes a friendly API:
#
#   Aurum.spawn()
#   Aurum.set_component(entity, "Position2D", {"x": 0, "y": 0})
#   Aurum.emit_event("PlayerHit", {"damage": 10})
#   Aurum.state_set("score", 100)
#   Aurum.save_to_json()
#
# The wrapper exists so:
# - GDScript can treat the engine as a singleton (`Aurum.foo()`).
# - The dev console can hook into one place.
# - Module registration has a clear call site.
#
# The native AurumNode class is loaded dynamically via ClassDB so the
# autoload can fail gracefully if the GDExtension DLL is missing.

const ENGINE_CLASS := "AurumNode"

var _engine: Node = null


func _ready() -> void:
	_engine = _instantiate_engine()
	if _engine == null:
		push_error("[Aurum] Could not instantiate AurumNode. Build the GDExtension with: pwsh build.ps1")
		return
	if _engine is Node:
		add_child(_engine as Node)
	_engine.event_received.connect(_on_event_received)
	print("[Aurum] Runtime ready. Modules: ", _engine.list_modules())


func _instantiate_engine() -> Node:
	# Try ClassDB first (more reliable than singleton lookup in editor).
	if ClassDB.can_instantiate(ENGINE_CLASS):
		return ClassDB.instantiate(ENGINE_CLASS)
	# Fallback: search for a singleton named AurumNode.
	if Engine.has_singleton(ENGINE_CLASS):
		return Engine.get_singleton(ENGINE_CLASS)
	return null


# ===== Build diagnostics =====

func runtime_fingerprint() -> String:
	if _engine == null:
		return ""
	return _engine.runtime_fingerprint()


# ===== Module registration =====

# Modules call this from their `_init` or in a scene's `_ready`. Idempotent.
func register_module(name: String) -> void:
	if _engine == null:
		push_warning("[Aurum] Cannot register module; engine not loaded.")
		return
	_engine.register_module(name)


func has_module(name: String) -> bool:
	if _engine == null:
		return false
	return _engine.has_module(name)


func list_modules() -> Array:
	if _engine == null:
		return []
	return _engine.list_modules()


# ===== Story (visual novel) =====

# The story engine lives in the Rust `aurum-vn` crate. GDScript code
# interacts with it through these methods. Story events are returned
# as Dictionaries; see the docs for the shape of each event.

func story_load(json: String, start_scene: String = "start") -> String:
	if _engine == null:
		return "Engine not loaded"
	Aurum.register_module("vn")
	return _engine.story_load(json, start_scene)


func story_is_loaded() -> bool:
	if _engine == null:
		return false
	return _engine.story_is_loaded()


func story_advance() -> Dictionary:
	if _engine == null:
		return {"type": "error", "message": "Engine not loaded"}
	return _engine.story_advance()


func story_pick_choice(index: int) -> String:
	if _engine == null:
		return "Engine not loaded"
	return _engine.story_pick_choice(index)


func story_jump_to(target: String) -> String:
	if _engine == null:
		return "Engine not loaded"
	return _engine.story_jump_to(target)


func story_get_variable(key: String, default: Variant = null) -> Variant:
	if _engine == null:
		return default
	return _engine.story_get_variable(key, default)


func story_set_variable(key: String, value: Variant) -> bool:
	if _engine == null:
		return false
	return _engine.story_set_variable(key, value)


func story_export_state() -> String:
	if _engine == null:
		return ""
	return _engine.story_export_state()


func story_import_state(json: String) -> bool:
	if _engine == null:
		return false
	return _engine.story_import_state(json)


func story_current_scene() -> String:
	if _engine == null:
		return ""
	return _engine.story_current_scene()


func story_current_entry_index() -> int:
	if _engine == null:
		return -1
	return _engine.story_current_entry_index()


# ===== Entity API =====

func spawn() -> int:
	return _engine.spawn()


func despawn(entity: int) -> bool:
	return _engine.despawn(entity)


func entity_exists(entity: int) -> bool:
	return _engine.entity_exists(entity)


func entity_count() -> int:
	return _engine.entity_count()


# ===== Component API =====

func set_component(entity: int, type_name: String, data: Dictionary) -> bool:
	return _engine.set_component(entity, type_name, data)


func get_component(entity: int, type_name: String) -> Dictionary:
	return _engine.get_component(entity, type_name)


func has_component(entity: int, type_name: String) -> bool:
	return _engine.has_component(entity, type_name)


func remove_component(entity: int, type_name: String) -> bool:
	return _engine.remove_component(entity, type_name)


func get_entities_with(type_name: String) -> Array:
	return _engine.get_entities_with(type_name)


func get_entities_with_all(type_names: Array) -> Array:
	return _engine.get_entities_with_all(type_names)


# ===== Event API =====

func emit_event(type_name: String, data: Dictionary = {}) -> void:
	if _engine == null:
		return
	var typed: Dictionary[String, Variant] = {}
	for key in data.keys():
		typed[String(key)] = data[key]
	_engine.emit_event(type_name, typed)


func dispatch_events() -> void:
	_engine.dispatch_events()


# ===== State API =====

func state_get(key: String, default: Variant = null) -> Variant:
	return _engine.state_get(key, default)


func state_set(key: String, value: Variant) -> bool:
	return _engine.state_set(key, value)


func state_has(key: String) -> bool:
	return _engine.state_has(key)


func state_remove(key: String) -> bool:
	return _engine.state_remove(key)


func state_clear() -> void:
	_engine.state_clear()


# ===== Time API =====

func set_time_scale(scale: float) -> void:
	_engine.set_time_scale(scale)


func get_time_scale() -> float:
	return _engine.get_time_scale()


# ===== Space simulation =====

func space_configure(
	mass_kg: float,
	thrust_n: float,
	rotation_accel_rad_s2: float,
	max_rotation_rate_rad_s: float,
	max_speed_mps: float,
	boost_multiplier: float,
	boost_fuel_per_s: float,
	boost_heat_per_s: float,
	fuel_capacity: float,
	heat_capacity: float,
	heat_dissipation_per_s: float,
	shield_capacity: float,
	hull_capacity: float,
	flight_assist_damping: float,
) -> bool:
	if _engine == null:
		return false
	return _engine.space_configure(
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
	)


func space_reset() -> void:
	if _engine != null:
		_engine.space_reset()


func space_set_docked(docked: bool) -> void:
	if _engine != null:
		_engine.space_set_docked(docked)


func space_set_transform(
	position_x: float,
	position_y: float,
	position_z: float,
	orientation_x: float,
	orientation_y: float,
	orientation_z: float,
	orientation_w: float,
) -> void:
	if _engine != null:
		_engine.space_set_transform(
			position_x,
			position_y,
			position_z,
			orientation_x,
			orientation_y,
			orientation_z,
			orientation_w,
		)


func space_set_status(fuel: float, heat: float, shield: float, hull: float) -> void:
	if _engine != null:
		_engine.space_set_status(fuel, heat, shield, hull)


func space_consume_fuel(amount: float) -> bool:
	if _engine == null:
		return false
	return _engine.space_consume_fuel(amount)


func space_stop_motion() -> void:
	if _engine != null:
		_engine.space_stop_motion()


func space_set_input(
	pitch: float,
	yaw: float,
	roll: float,
	thrust_forward: float,
	thrust_lateral: float,
	thrust_vertical: float,
	boost: bool,
	dampen: bool,
	flight_assist: bool,
) -> void:
	if _engine != null:
		_engine.space_set_input(
			pitch,
			yaw,
			roll,
			thrust_forward,
			thrust_lateral,
			thrust_vertical,
			boost,
			dampen,
			flight_assist,
		)


func space_step(real_delta: float) -> Dictionary:
	if _engine == null:
		return {}
	return _engine.space_step(real_delta)


func space_snapshot() -> Dictionary:
	if _engine == null:
		return {}
	return _engine.space_snapshot()


func space_apply_damage(amount: float) -> void:
	if _engine != null:
		_engine.space_apply_damage(amount)


func space_repair_full() -> void:
	if _engine != null:
		_engine.space_repair_full()


func space_refuel_full() -> void:
	if _engine != null:
		_engine.space_refuel_full()


# ===== Save / Load =====

func save_to_json() -> String:
	return _engine.save_to_json()


func load_from_json(json: String) -> bool:
	return _engine.load_from_json(json)


# ===== Internal: forward engine events =====

func _on_event_received(type_name: String, data: Dictionary) -> void:
	event_received.emit(type_name, data)


signal event_received(type_name: String, data: Dictionary)
