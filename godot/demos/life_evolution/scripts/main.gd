extends Node3D
## Main controller for Life Evolution.
##
## This script ties together the simulation, rendering, camera, and UI.
## It manages the overall game loop, loading screen, and user input.

const USE_GPU_SIMULATION: bool = true
const USE_GPU_PARTICLES: bool = false

@onready var simulation: Node = $GPUSimulation if USE_GPU_SIMULATION else $SimulationWorld
@onready var simulation_bridge: Node = $SimulationBridge
@onready var camera_rig: Node3D = $CameraRig
@onready var camera: Camera3D = $CameraRig/Camera3D
@onready var hud: Control = $UI/HUD
@onready var loading_screen: Control = $LoadingLayer/LoadingScreen
@onready var particle_renderer: Node3D = $ParticleRenderer
@onready var multimesh_instance: MultiMeshInstance3D = $ParticleRenderer/MultiMeshInstance3D

## Time scale presets (1x to 1e9x)
const TIME_SCALES: Array[float] = [
	1.0,        # Real time
	10.0,       # 10x
	100.0,      # 100x
	1000.0,     # 1,000x
	10000.0,    # 10,000x
	100000.0,   # 100,000x
	1e6,        # Million
	1e7,        # 10 million
	1e8,        # 100 million
	1e9,        # Billion
	1e10,       # 10 billion
	1e12,       # Trillion (for stellar/cosmic scales)
]

var current_time_scale_index: int = 0
var time_scale: float = 1.0
var camera_target: Vector3 = Vector3.ZERO
var camera_distance: float = 1000.0
var is_dragging: bool = false
var last_mouse_pos: Vector2 = Vector2.ZERO
var camera_rotation: Vector2 = Vector2.ZERO

## Loading state
var is_loaded: bool = false
var loading_poll_interval: float = 0.05  # Poll every 50ms
var loading_poll_timer: float = 0.0
var simulation_started: bool = false
var _warned_no_loading_progress: bool = false
var _auto_complete_timer: float = 0.0
var _simulation_config: Dictionary = {}
var _gpu_fallback_started: bool = false
var _perf_window_seconds: float = 0.0
var _perf_frame_count: int = 0
var _perf_max_frame_ms: float = 0.0
var _perf_max_render_ms: float = 0.0
var _perf_max_tick_ms: float = 0.0
var _time_slider: HSlider

## Auto-LOD settings
const LOD_NEAR: float = 100.0
const LOD_FAR: float = 10000.0

func _ready() -> void:
	_log_info("Startup began")
	if not _validate_runtime_nodes():
		return
	_configure_particle_renderer()
	_connect_time_slider()
	_connect_loading_signals()
	_initialize_simulation_async()
	_set_time_scale_index(0)
	_log_info("Startup completed — loading screen shown")

func _connect_loading_signals() -> void:
	loading_screen.user_clicked_start.connect(_on_user_clicked_start)

func _on_user_clicked_start() -> void:
	_log_info("User clicked Start — launching simulation")
	# Use the GDScript bridge (gdext 0.5.4 master has trouble registering
	# `start_simulation` directly on the Rust class)
	if simulation_bridge and simulation_bridge.has_method("start_simulation"):
		simulation_bridge.start_simulation()
	simulation_started = true
	_set_time_scale_index(0)  # Reset to 1x on start

func _initialize_simulation_async() -> void:
	# Show loading screen immediately — simulation initializes on the Rust
	# background thread, so Godot remains responsive
	loading_screen.show_loading()
	_is_loading = true
	
	var config: Dictionary = {
		"particle_count": 50000,
		"particle_capacity": 100000,
		"temperature": 1e6,
		"radius": 35.0,
		"gravity_enabled": true,
		"electromagnetic_enabled": true,
		"quantum_forces_enabled": false,
		"emergence_enabled": true,
		"auto_lod": true,
		"particle_mass": 1.0,
		"gravity_constant": 0.03,
		"force_scale": 1.0,
		"expansion_strength": 4.0,
		"expansion_duration": 12.0,
		"initial_velocity": 1.5,
		"merge_radius": 0.75,
		"merge_speed": 3.0,
		"gravity_softening": 0.5,
		"collapse_strength": 0.0,
		"merge_start": 18.0,
		"merge_duration": 90.0,
		"velocity_scale": 0.5,
		"periodic_boundaries": true,
		"boundary_radius": 200.0,
		"max_dt": 1.0,
	}
	_simulation_config = config
	# This call starts the background thread in Rust and returns immediately.
	# Godot remains responsive during the ~100ms initialization.
	simulation.initialize(JSON.stringify(config))
	if USE_GPU_SIMULATION and simulation.has_method("has_initialization_failed") and simulation.has_initialization_failed():
		_switch_to_cpu_backend()
	_log_info("Simulation background thread started")

var _is_loading: bool = true

func _process(delta: float) -> void:
	_perf_window_seconds += delta
	_perf_frame_count += 1
	_perf_max_frame_ms = maxf(_perf_max_frame_ms, delta * 1000.0)
	# Poll loading progress via the GDScript bridge (works around gdext
	# 0.5.4 master bug that prevents the Rust method from registering)
	if _is_loading:
		loading_poll_timer += delta
		if loading_poll_timer >= loading_poll_interval:
			loading_poll_timer = 0.0
			var progress: int = 0
			if simulation_bridge and simulation_bridge.has_method("get_loading_progress"):
				progress = simulation_bridge.get_loading_progress()
			loading_screen.update_progress(progress)
			if progress >= 100:
				_on_loading_complete()
		# Fallback: auto-complete if loading takes too long
		if not is_loaded:
			_auto_complete_timer += delta
			if USE_GPU_SIMULATION and _auto_complete_timer > 5.0 and not simulation.is_initialized() and not _gpu_fallback_started and simulation.has_method("has_initialization_failed") and simulation.has_initialization_failed():
				_switch_to_cpu_backend()
			if _auto_complete_timer > 10.0 and not simulation.is_initialized() and not _gpu_fallback_started:
				_log_error("GPU initialization timed out; switching to the CPU fallback")
				_switch_to_cpu_backend()
	
	if not is_loaded:
		return
	
	if not simulation.has_method("is_initialized") or not simulation.is_initialized():
		return
	
	# Advance simulation (non-blocking dispatch to background thread)
	# Only tick once the user has clicked "Start"
	if simulation_started and not simulation.is_paused():
		var tick_started_usec: int = Time.get_ticks_usec()
		simulation.tick(delta)
		_perf_max_tick_ms = maxf(_perf_max_tick_ms, float(Time.get_ticks_usec() - tick_started_usec) / 1000.0)
	
	# GPU particles render directly from their simulation buffer. The Rust
	# renderer is used only by the CPU fallback backend.
	var gpu_resident: bool = simulation.has_method("is_gpu_resident") and simulation.is_gpu_resident()
	if (not USE_GPU_PARTICLES or _gpu_fallback_started) and not gpu_resident:
		var render_started_usec: int = Time.get_ticks_usec()
		particle_renderer.update_rendering()
		_perf_max_render_ms = maxf(_perf_max_render_ms, float(Time.get_ticks_usec() - render_started_usec) / 1000.0)
	
	# Update camera
	_update_camera(delta)
	
	# Update HUD periodically
	if Engine.get_frames_drawn() % 30 == 0:
		_update_hud()
	if _perf_window_seconds >= 2.0:
		var measured_fps: float = float(_perf_frame_count) / _perf_window_seconds
		_log_info("Performance window: fps=%.1f max_frame_ms=%.2f max_tick_ms=%.2f max_render_ms=%.2f" % [measured_fps, _perf_max_frame_ms, _perf_max_tick_ms, _perf_max_render_ms])
		_perf_window_seconds = 0.0
		_perf_frame_count = 0
		_perf_max_frame_ms = 0.0
		_perf_max_render_ms = 0.0
		_perf_max_tick_ms = 0.0

func _on_loading_complete() -> void:
	_is_loading = false
	loading_screen.hide_loading()
	is_loaded = true
	_log_info("Loading complete — simulation ready")
	_update_hud()

func _connect_time_slider() -> void:
	var slider := hud.get_node_or_null("VBox/TimeSlider") as HSlider
	if slider and slider.has_signal("time_scale_changed"):
		_time_slider = slider
		slider.time_scale_changed.connect(_on_slider_time_scale_changed)
		_log_info("Time slider signal connected")

func _on_slider_time_scale_changed(scale: float) -> void:
	var selected_index: int = _find_time_scale_index(scale)
	_set_time_scale_index(selected_index)

func _input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_RIGHT:
			is_dragging = mb.pressed
			last_mouse_pos = mb.position
		elif mb.button_index == MOUSE_BUTTON_WHEEL_UP:
			camera_distance *= 0.9
		elif mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			camera_distance *= 1.1
	
	elif event is InputEventMouseMotion and is_dragging:
		var mm := event as InputEventMouseMotion
		var delta_pos := mm.position - last_mouse_pos
		last_mouse_pos = mm.position
		
		camera_rotation.y -= delta_pos.x * 0.005
		camera_rotation.x -= delta_pos.y * 0.005
		camera_rotation.x = clamp(camera_rotation.x, -PI/2 + 0.1, PI/2 - 0.1)
	
	elif event is InputEventKey:
		_handle_keyboard(event as InputEventKey)

func _handle_keyboard(event: InputEventKey) -> void:
	if event.pressed:
		match event.keycode:
			KEY_SPACE:
				simulation.set_paused(not simulation.is_paused())
			KEY_E:
				_set_time_scale_index(min(current_time_scale_index + 1, TIME_SCALES.size() - 1))
			KEY_Q:
				_set_time_scale_index(max(current_time_scale_index - 1, 0))
			KEY_F:
				_focus_heaviest_object()
			KEY_B:
				_trigger_big_bang()
			KEY_R:
				simulation.reset()
			KEY_TAB:
				hud.visible = not hud.visible

func _set_time_scale_index(index: int) -> void:
	current_time_scale_index = clampi(index, 0, TIME_SCALES.size() - 1)
	time_scale = TIME_SCALES[current_time_scale_index]
	simulation.set_time_scale(time_scale)
	if _time_slider != null:
		_time_slider.sync_index(current_time_scale_index)
	_log_info("Time scale changed: index=%d requested=%.0fx" % [current_time_scale_index, time_scale])
	_update_hud()

func _find_time_scale_index(scale: float) -> int:
	var closest_index: int = 0
	var closest_error: float = INF
	for index in TIME_SCALES.size():
		var error: float = absf(log(maxf(scale, 0.000001)) - log(TIME_SCALES[index]))
		if error < closest_error:
			closest_error = error
			closest_index = index
	return closest_index

func _update_camera(_delta: float) -> void:
	_update_camera_transform()

func _update_camera_transform() -> void:
	var rot_x := camera_rotation.x
	var rot_y := camera_rotation.y
	
	var offset := Vector3(
		cos(rot_x) * sin(rot_y),
		sin(rot_x),
		cos(rot_x) * cos(rot_y)
	) * camera_distance
	
	camera.global_position = camera_target + offset
	camera.look_at(camera_target, Vector3.UP)

func _validate_runtime_nodes() -> bool:
	if not is_instance_valid(simulation) or not simulation.has_method("initialize"):
		_log_error("SimulationWorld is unavailable. Check the GDExtension binary and platform path.")
		return false
	if not is_instance_valid(particle_renderer) or not particle_renderer.has_method("update_rendering"):
		_log_error("ParticleRenderer is unavailable.")
		return false
	if not is_instance_valid(multimesh_instance):
		_log_error("Particle MultiMeshInstance3D is unavailable.")
		return false
	return true

func _switch_to_cpu_backend() -> void:
	_gpu_fallback_started = true
	_log_info("GPU compute backend unavailable, falling back to Rust CPU backend")
	simulation = $SimulationWorld
	simulation_bridge.set_simulation_node(simulation)
	particle_renderer.set_simulation_path("../SimulationWorld")
	particle_renderer.visible = true
	multimesh_instance.visible = true
	simulation.initialize(JSON.stringify(_simulation_config))

func _configure_particle_renderer() -> void:
	if USE_GPU_PARTICLES:
		particle_renderer.visible = false
		multimesh_instance.visible = false
	# ====== Diagnostic: dump class state BEFORE any method calls ======
	_log_info("=== ParticleRenderer diagnostics ===")
	_log_info("  node type: %s" % str(particle_renderer.get_class()))
	_log_info("  script: %s" % str(particle_renderer.get_script()))
	var pr_methods: Array = particle_renderer.get_method_list()
	var pr_method_names: Array = []
	for m in pr_methods:
		pr_method_names.append(m.get("name", ""))
	_log_info("  method count: %d" % pr_method_names.size())
	_log_info("  has 'ping_test': %s" % str(particle_renderer.has_method("ping_test")))
	_log_info("  has 'set_simulation_path': %s" % str(particle_renderer.has_method("set_simulation_path")))
	_log_info("  has 'set_multimesh_instance': %s" % str(particle_renderer.has_method("set_multimesh_instance")))
	_log_info("  has 'update_rendering': %s" % str(particle_renderer.has_method("update_rendering")))

	_log_info("=== SimulationWorld diagnostics ===")
	_log_info("  node type: %s" % str(simulation.get_class()))
	var sw_methods: Array = simulation.get_method_list()
	var sw_method_names: Array = []
	for m in sw_methods:
		sw_method_names.append(m.get("name", ""))
	_log_info("  method count: %d" % sw_method_names.size())
	# Check ALL user-defined methods
	var checks: Array = [
		"initialize", "tick", "set_time_scale", "get_time_scale", "set_paused",
		"is_paused", "get_simulation_time", "get_particle_count", "get_tick_status",
		"get_statistics_json", "get_atoms_json", "get_molecules_json", "get_cosmic_entities_json",
		"reset", "get_max_complexity", "get_frame", "is_initialized",
		"get_loading_progress", "start_simulation", "is_simulation_started"
	]
	for m in checks:
		_log_info("  has '%s': %s" % [m, str(simulation.has_method(m))])
	# Also print the user methods we have (filtering out node/object builtins)
	_log_info("  User method names (non-inherited):")
	for name in sw_method_names:
		# Only show methods starting with our prefixes
		if name in checks:
			_log_info("    ✓ %s" % name)
	_log_info("=== End diagnostics ===")

	# Now actually call methods
	particle_renderer.set_simulation_path("../GPUSimulation" if USE_GPU_SIMULATION else "../SimulationWorld")
	if particle_renderer.has_method("ping_test"):
		particle_renderer.ping_test()
		_log_info("ping_test called successfully")
	if particle_renderer.has_method("set_multimesh_instance"):
		particle_renderer.set_multimesh_instance(multimesh_instance)
		_log_info("Particle renderer configured for %d GPU instances" % multimesh_instance.multimesh.instance_count)
	else:
		_log_error("set_multimesh_instance NOT available — check GDExtension class registration")

func _log_info(message: String) -> void:
	print("[LifeEvolution][INFO] %s" % message)

func _log_error(message: String) -> void:
	push_error("[LifeEvolution][ERROR] %s" % message)

func _focus_heaviest_object() -> void:
	if simulation.has_method("request_focus_heaviest"):
		simulation.request_focus_heaviest()
		get_tree().create_timer(0.25).timeout.connect(_apply_gpu_focus_result, CONNECT_ONE_SHOT)
		_log_info("GPU heaviest-body focus requested")
		return
	var cosmic: Array = _get_json_array("get_cosmic_entities_json")
	var max_mass := 0.0
	var heaviest: Vector3 = Vector3.ZERO
	
	for entity in cosmic:
		var mass: float = entity.get("mass", 0.0)
		if mass > max_mass:
			max_mass = mass
			var pos: Vector3 = entity.get("position", Vector3.ZERO)
			heaviest = pos
	
	if max_mass > 0:
		camera_target = heaviest
		var boundary: float = 200.0
		camera_distance = max(50.0, boundary * 0.5)
		_log_info("Focusing on heaviest object at %s (mass %.2f)" % [heaviest, max_mass])
	else:
		_log_info("No cosmic entities yet — keeping current camera target")
		camera_distance = 200.0

func _trigger_big_bang() -> void:
	if simulation.has_method("trigger_big_bang"):
		_set_time_scale_index(0)
		simulation.trigger_big_bang()
		_log_info("Big Bang event requested at 1x simulation speed")
	else:
		_log_info("Big Bang event is unavailable on the active simulation backend")

func _apply_gpu_focus_result() -> void:
	if not simulation.has_method("get_heaviest_mass") or simulation.get_heaviest_mass() <= 0.0:
		_log_info("GPU heaviest-body data is not ready yet")
		return
	camera_target = simulation.get_heaviest_position()
	camera_distance = 100.0
	_log_info("Focused GPU heaviest body at %s (mass %.2f)" % [camera_target, simulation.get_heaviest_mass()])

func _update_hud() -> void:
	if hud and hud.has_method("update_stats"):
		var stats: Dictionary = _get_json_dictionary("get_statistics_json")
		var sim_time: float = simulation.get_simulation_time()
		var complexity: int = simulation.get_max_complexity()
		var particle_count: int = simulation.get_particle_count()
		var paused: bool = simulation.is_paused()
		hud.update_stats(stats, sim_time, time_scale, complexity, particle_count, paused)

func _get_json_dictionary(method_name: String) -> Dictionary:
	if not simulation.has_method(method_name):
		return {}
	var parsed = JSON.parse_string(simulation.call(method_name))
	return parsed if parsed is Dictionary else {}

func _get_json_array(method_name: String) -> Array:
	if not simulation.has_method(method_name):
		return []
	var parsed = JSON.parse_string(simulation.call(method_name))
	return parsed if parsed is Array else []
