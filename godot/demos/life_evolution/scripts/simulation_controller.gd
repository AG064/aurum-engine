extends Node
## Singleton that manages the simulation state from GDScript.
##
## This provides a convenient way to access the simulation from anywhere
## in the Godot project.

signal simulation_initialized
signal simulation_reset
signal stats_updated(stats: Dictionary)

var simulation: Node = null
var config: Dictionary = {}
var stats: Dictionary = {}
var is_initialized: bool = false

## Initialize the simulation.
func initialize_simulation(custom_config: Dictionary = {}) -> void:
	if simulation == null:
		push_error("No simulation node assigned")
		return
	
	var default_config: Dictionary = {
		"particle_count": 50000,
		"temperature": 1e9,
		"radius": 1000.0,
		"gravity_enabled": true,
		"electromagnetic_enabled": true,
		"quantum_forces_enabled": true,
		"emergence_enabled": true,
		"auto_lod": true,
	}
	
	# Merge with custom config
	for key in custom_config:
		default_config[key] = custom_config[key]
	
	config = default_config
	if not simulation.has_method("initialize"):
		push_error("Assigned simulation node does not expose initialize")
		return
	simulation.initialize(JSON.stringify(config))
	is_initialized = true
	simulation_initialized.emit()
	
	print("[SimulationController] Initialized with %d particles" % config.get("particle_count", 0))

## Reset the simulation.
func reset_simulation() -> void:
	if simulation:
		simulation.reset()
		simulation_reset.emit()

## Get current statistics.
func get_current_stats() -> Dictionary:
	if simulation:
		if simulation.has_method("get_statistics_json"):
			var parsed = JSON.parse_string(simulation.get_statistics_json())
			stats = parsed if parsed is Dictionary else {}
		stats_updated.emit(stats)
	return stats

## Get a human-readable complexity description.
func get_complexity_description(complexity: int) -> String:
	match complexity:
		0: return "Pre-particle"
		1: return "Particle soup"
		2: return "Atomic"
		3: return "Molecular"
		4: return "Cellular"
		5: return "Multicellular"
		6: return "Cosmic"
		_: return "Unknown (%d)" % complexity

## Set the simulation node.
func set_simulation(sim: Node) -> void:
	simulation = sim

## Set time scale.
func set_time_scale(time_scale_value: float) -> void:
	if simulation:
		simulation.set_time_scale(time_scale_value)

## Toggle pause.
func toggle_pause() -> void:
	if simulation:
		simulation.set_paused(not simulation.is_paused())
