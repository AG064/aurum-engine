extends Node
## GDScript-side bridge that fills in the gap for GDExtension methods that
## the gdext 0.5.4 master has trouble registering.
##
## All methods here are GDScript-only — no GDExtension involvement. They
## compose the working Rust methods (set_paused, is_paused, is_initialized,
## etc.) into the higher-level operations (start_simulation, get_loading_progress,
## is_simulation_started, get_tick_status).
##
## main.gd calls these methods instead of the GDExtension ones directly.

@export var simulation_path: NodePath

var _simulation: Node = null
var _is_started: bool = false
var _loading_tick: float = 0.0

func _ready() -> void:
	if simulation_path != NodePath():
		_simulation = get_node_or_null(simulation_path)
	if _simulation == null:
		# Try to find by name
		_simulation = get_parent().get_node_or_null("SimulationWorld")

func set_simulation_node(simulation_node: Node) -> void:
	_simulation = simulation_node

## Loading progress 0-100.
## Returns 0 until the simulation is initialized, then ramps to 100.
## This avoids depending on the broken Rust `get_loading_progress`.
func get_loading_progress() -> int:
	if _simulation == null:
		return 0
	if not _simulation.has_method("is_initialized"):
		return 0
	if _simulation.is_initialized():
		return 100
	# Animate the bar while loading so user sees activity
	_loading_tick = fmod(_loading_tick + 0.02, 0.95)
	return int(_loading_tick * 100.0)

## Begin the simulation tick loop.
## Since gdext 0.5.4 master has trouble registering `start_simulation`,
## we use the working `set_paused` method as the "start" mechanism.
func start_simulation() -> void:
	if _simulation == null:
		push_error("simulation_bridge: simulation reference is null")
		return
	_is_started = true
	# Use set_paused(false) as the start signal — this method is known to register
	if _simulation.has_method("set_paused"):
		_simulation.set_paused(false)
	print("[LifeEvolution][INFO] simulation started (via GDScript bridge)")

## Whether the simulation has been started.
func is_simulation_started() -> bool:
	return _is_started

## Get the completed tick ID and pending count for diagnostics.
## Returns a JSON string. Since the Rust `get_tick_status` doesn't register,
## we return a simple JSON with basic info.
func get_tick_status() -> String:
	if _simulation == null:
		return "{\"completed_tick\":0,\"pending_ticks\":0}"
	var completed := 0
	var pending := 0
	if _simulation.has_method("get_tick_status"):
		# Try the Rust method first (in case it ever starts working)
		var raw: String = _simulation.get_tick_status()
		return raw
	# Fallback: synthesize a JSON from what we can read
	if _simulation.has_method("get_frame"):
		completed = int(_simulation.get_frame())
	return "{\"completed_tick\":%d,\"pending_ticks\":%d}" % [completed, pending]

## Get the particle positions from the simulation.
func get_particle_positions() -> PackedVector3Array:
	if _simulation == null or not _simulation.has_method("get_particle_positions"):
		return PackedVector3Array()
	return _simulation.get_particle_positions()

## Get the particle colors from the simulation.
func get_particle_colors() -> PackedColorArray:
	if _simulation == null or not _simulation.has_method("get_particle_colors"):
		return PackedColorArray()
	return _simulation.get_particle_colors()

## Get the particle radii from the simulation.
func get_particle_radii() -> PackedFloat32Array:
	if _simulation == null or not _simulation.has_method("get_particle_radii"):
		return PackedFloat32Array()
	return _simulation.get_particle_radii()
