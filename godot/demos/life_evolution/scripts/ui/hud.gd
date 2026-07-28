extends Control
## Heads-up display for Life Evolution.
##
## Shows simulation statistics, time scale, complexity level, gravity diagnostics, and controls.

@onready var stats_label: Label = $VBox/StatsLabel
@onready var time_label: Label = $VBox/TimeLabel
@onready var complexity_label: Label = $VBox/ComplexityLabel
@onready var gravity_label: Label = $VBox/GravityLabel
@onready var controls_label: Label = $VBox/ControlsLabel
@onready var progress_bar: ProgressBar = $VBox/ProgressBar

## Update the HUD with current statistics.
func update_stats(stats: Dictionary, sim_time: float, time_scale: float, complexity: int, particle_count: int, paused: bool) -> void:
	_update_stats_label(stats, particle_count)
	_update_time_label(sim_time, time_scale, paused)
	_update_complexity_label(complexity)
	_update_gravity_diagnostics(stats)
	_update_progress_bar(complexity)

func _update_stats_label(stats: Dictionary, particle_count: int) -> void:
	var atoms: int = stats.get("atom_count", 0)
	var molecules: int = stats.get("molecule_count", 0)
	var organisms: int = stats.get("organism_count", 0)
	var temperature: float = stats.get("temperature", 0.0)
	var energy: float = stats.get("total_energy", 0.0)
	var backend: String = stats.get("backend", "")
	
	var particle_text: String = str(particle_count)
	var molecule_text: String = str(molecules)
	if backend == "GPU resident":
		particle_text += " (GPU bodies)"
	var text := """=== SIMULATION STATS ===
Particles: %s
Atoms: %d  Molecules: %s  Organisms: %d
Temperature: %s K
Total Energy: %s J""" % [particle_text, atoms, molecule_text, organisms, String.num(temperature, 3), String.num(energy, 3)]
	if not backend.is_empty():
		text += "\nBackend: %s" % backend
	stats_label.text = text

func _update_time_label(sim_time: float, time_scale: float, paused: bool) -> void:
	var state := "⏸ PAUSED" if paused else "▶ RUNNING"
	var scale_str: String = _format_scale(time_scale)
	time_label.text = """=== TIME ===
%s
Simulation: %s s
Speed: %s""" % [state, String.num(sim_time, 2), scale_str]

func _format_scale(time_scale_value: float) -> String:
	if time_scale_value >= 1e9:
		return "%s x (Billion)" % String.num(time_scale_value / 1e9, 0)
	elif time_scale_value >= 1e6:
		return "%s x (Million)" % String.num(time_scale_value / 1e6, 0)
	elif time_scale_value >= 1e3:
		return "%s x (Thousand)" % String.num(time_scale_value / 1e3, 0)
	else:
		return "%s x" % String.num(time_scale_value, 1)

func _update_gravity_diagnostics(stats: Dictionary) -> void:
	# Show gravity diagnostics so we can verify physics is working
	var com: Array = stats.get("center_of_mass", [0.0, 0.0, 0.0])
	var mean_radius: float = stats.get("mean_radius", 0.0)
	var avg_speed: float = stats.get("avg_speed", 0.0)
	var max_speed: float = stats.get("max_speed", 0.0)
	var avg_accel: float = stats.get("avg_accel", 0.0)
	var max_accel: float = stats.get("max_accel", 0.0)
	var phase: String = stats.get("phase", "Unknown")
	var merged_count: int = stats.get("merged_count", 0)
	var active_force_count: int = stats.get("active_force_count", 0)
	var avg_force: float = stats.get("avg_force", 0.0)
	var max_force: float = stats.get("max_force", 0.0)
	var merged_text: String = str(merged_count)
	
	# Warn if particles are escaping (mean radius near boundary)
	var boundary: float = 200.0
	var radius_status: String
	if mean_radius > boundary * 0.9:
		radius_status = "⚠️ NEAR BOUNDARY"
	elif mean_radius > boundary * 0.7:
		radius_status = "⚡ SPREADING"
	else:
		radius_status = "✓ NORMAL"
	
	gravity_label.text = """=== GRAVITY DIAGNOSTICS ===
CoM: (%.1f, %.1f, %.1f)
Mean Radius: %.1f m  %s
Avg Speed: %.3f m/s
Max Speed: %.3f m/s
Active Forces: %d / %d
Avg Force: %.4f
Max Force: %.4f
Avg Accel: %.4f m/s²
Max Accel: %.4f m/s²
Phase: %s
Merged: %s""" % [com[0], com[1], com[2], mean_radius, radius_status, avg_speed, max_speed, active_force_count, stats.get("particle_count", 0), avg_force, max_force, avg_accel, max_accel, phase, merged_text]

func _update_complexity_label(complexity: int) -> void:
	var description: String = _get_complexity_description(complexity)
	var emoji: String = _get_complexity_emoji(complexity)
	complexity_label.text = """=== COMPLEXITY ===
Level %d/6
%s %s""" % [complexity, emoji, description]

func _get_complexity_description(complexity: int) -> String:
	match complexity:
		0: return "Pre-particle"
		1: return "Particle Soup"
		2: return "Atoms"
		3: return "Molecules"
		4: return "Cells"
		5: return "Organisms"
		6: return "Cosmic"
		_: return "Unknown"

func _get_complexity_emoji(complexity: int) -> String:
	match complexity:
		0: return "💫"
		1: return "✨"
		2: return "⚛️"
		3: return "🧪"
		4: return "🦠"
		5: return "🐟"
		6: return "🌌"
		_: return "❓"

func _update_progress_bar(complexity: int) -> void:
	progress_bar.value = (complexity as float / 6.0) * 100.0
