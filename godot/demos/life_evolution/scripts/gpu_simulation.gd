extends Node
## Real GPU particle simulation with spatial hashing and merge compaction.

const DEFAULT_COUNT: int = 50000
const WORKGROUP_SIZE: int = 256
const GRID_DIMENSION: int = 32
const GRID_CELL_SIZE: float = 12.5
const GRID_ORIGIN: float = 200.0
const MAX_CELL_PARTICLES: int = 64
const PARTICLE_FLOATS: int = 16
const GLOBAL_SIMULATION_STEP: float = 1.0 / 60.0
const MAX_GLOBAL_STEPS_PER_DISPATCH: int = 128
const MAX_GLOBAL_BACKLOG_STEPS: int = 512

@onready var _merged_core: MeshInstance3D = get_node_or_null("../MergedCore") as MeshInstance3D

var _rd: RenderingDevice
var _shader: RID
var _pipeline: RID
var _particle_a: RID
var _particle_b: RID
var _cell_counts: RID
var _cell_indices: RID
var _merge_counts: RID
var _merge_flags: RID
var _merge_partners: RID
var _count_a: RID
var _count_b: RID
var _render_output: RID
var _render_count: RID
var _diagnostics: RID
var _uniform_a: RID
var _uniform_b: RID
var _global_gpu: bool = false
var _global_initializing: bool = false
var _global_tick_queued: bool = false
var _global_time_accumulator: float = 0.0
var _global_step_count: int = 1
var _dispatch_dt: float = 0.033
var _dispatch_simulation_time: float = 0.0
var _shader_spirv: RDShaderSPIRV
var _initialization_failed: bool = false
var _input_is_a: bool = true
var _particle_capacity: int = 0
var _initial_count: int = 0
var _active_count: int = 0
var _initialized: bool = false
var _paused: bool = true
var _time_scale: float = 1.0
var _simulation_time: float = 0.0
var _completed_tick: int = 0
var _config: Dictionary = {}
var _positions := PackedVector3Array()
var _colors := PackedColorArray()
var _radii := PackedFloat32Array()
var _render_positions := PackedVector3Array()
var _render_colors := PackedColorArray()
var _render_radii := PackedFloat32Array()
var _stats: Dictionary = {}
var _snapshot_tick: int = -1
var _logged_tick: bool = false
var _last_phase: String = ""
var _snapshot_interval_ticks: int = 2
var _gpu_submission_in_flight: bool = false
var _diagnostics_read_queued: bool = false
var _focus_read_requested: bool = false
var _gpu_heaviest_position: Vector3 = Vector3.ZERO
var _gpu_heaviest_mass: float = 0.0
var _big_bang_until: float = -1.0
var _big_bang_pending_count: int = 0

func initialize(config_json: String) -> void:
	if _initialized:
		return
	_config = JSON.parse_string(config_json)
	if not _config is Dictionary:
		_config = {}
	_initial_count = clampi(int(_config.get("particle_count", DEFAULT_COUNT)), 1, 1000000)
	_particle_capacity = clampi(int(_config.get("particle_capacity", _initial_count)), _initial_count, 1000000)
	_active_count = _initial_count
	var shader_file: RDShaderFile = load("res://demos/life_evolution/shaders/particle_merge_compute_v3.glsl")
	if shader_file == null:
		_initialization_failed = true
		push_error("GPU compute unavailable: compute shader failed to load")
		return
	_shader_spirv = shader_file.get_spirv()
	var global_rd: RenderingDevice = RenderingServer.get_rendering_device()
	var mm_instance := get_node_or_null("../ParticleRenderer/MultiMeshInstance3D") as MultiMeshInstance3D
	var mm: MultiMesh = mm_instance.multimesh if mm_instance != null else null
	var mm_buffer: RID = RenderingServer.multimesh_get_buffer_rd_rid(mm.get_rid()) if mm != null and global_rd != null else RID()
	if global_rd != null and mm_buffer.is_valid():
		_rd = global_rd
		_render_output = mm_buffer
		_global_gpu = true
		_global_initializing = true
		RenderingServer.call_on_render_thread(Callable(self, "_initialize_global_on_render_thread"))
		return
	_rd = RenderingServer.create_local_rendering_device()
	if _rd == null:
		_initialization_failed = true
		push_warning("GPU compute unavailable: no local RenderingDevice")
		return
	_shader = _rd.shader_create_from_spirv(_shader_spirv)
	if not _shader.is_valid():
		_initialization_failed = true
		push_error("GPU compute unavailable: compute shader failed to compile")
		return
	_pipeline = _rd.compute_pipeline_create(_shader)
	if not _pipeline.is_valid():
		_initialization_failed = true
		push_error("GPU compute unavailable: compute pipeline failed")
		return
	if not _allocate_gpu_buffers():
		push_error("GPU compute unavailable: uniform buffers failed")
		return
	print("[LifeEvolution][GPU] Spatial merge pipeline initialized for %d particles" % _particle_capacity)

func _initialize_global_on_render_thread() -> void:
	_shader = _rd.shader_create_from_spirv(_shader_spirv)
	if not _shader.is_valid():
		call_deferred("_mark_initialization_failed", "global compute shader failed")
		return
	_pipeline = _rd.compute_pipeline_create(_shader)
	if not _pipeline.is_valid():
		call_deferred("_mark_initialization_failed", "global compute pipeline failed")
		return
	if not _allocate_gpu_buffers():
		call_deferred("_mark_initialization_failed", "global compute buffers failed")
		return
	_initialized = true
	_global_initializing = false
	print("[LifeEvolution][GPU] Global RenderingDevice pipeline initialized for %d particles" % _particle_capacity)

func _allocate_gpu_buffers() -> bool:
	_particle_a = _rd.storage_buffer_create(_particle_capacity * PARTICLE_FLOATS * 4, _initial_particle_bytes())
	_particle_b = _rd.storage_buffer_create(_particle_capacity * PARTICLE_FLOATS * 4)
	_cell_counts = _rd.storage_buffer_create(GRID_DIMENSION * GRID_DIMENSION * GRID_DIMENSION * 4)
	_cell_indices = _rd.storage_buffer_create(GRID_DIMENSION * GRID_DIMENSION * GRID_DIMENSION * MAX_CELL_PARTICLES * 4)
	_merge_counts = _rd.storage_buffer_create(_particle_capacity * 4)
	_merge_flags = _rd.storage_buffer_create(_particle_capacity * 4)
	_merge_partners = _rd.storage_buffer_create(_particle_capacity * 4)
	if not _global_gpu:
		_render_output = _rd.storage_buffer_create(_particle_capacity * 16 * 4)
		_render_count = _rd.storage_buffer_create(4)
	else:
		_render_count = _rd.storage_buffer_create(4)
	_diagnostics = _rd.storage_buffer_create(52)
	_count_a = _rd.storage_buffer_create(8, PackedInt32Array([_active_count, 0]).to_byte_array())
	_count_b = _rd.storage_buffer_create(8, PackedInt32Array([0, 0]).to_byte_array())
	_uniform_a = _create_uniform_set(_particle_a, _particle_b, _count_a, _count_b)
	_uniform_b = _create_uniform_set(_particle_b, _particle_a, _count_b, _count_a)
	if not _uniform_a.is_valid() or not _uniform_b.is_valid():
		return false
	_positions.resize(_particle_capacity)
	_colors.resize(_particle_capacity)
	_radii.resize(_particle_capacity)
	_stats = _make_stats()
	return true

func _mark_initialization_failed(reason: String) -> void:
	_initialization_failed = true
	_global_initializing = false
	push_error("GPU compute unavailable: %s" % reason)

func tick(delta: float) -> void:
	if not _initialized or _paused:
		return
	if _global_gpu:
		if _global_tick_queued:
			return
		_global_time_accumulator = minf(
			_global_time_accumulator + maxf(delta * _time_scale, 0.0),
			GLOBAL_SIMULATION_STEP * float(MAX_GLOBAL_BACKLOG_STEPS)
		)
		_global_step_count = mini(
			int(floor(_global_time_accumulator / GLOBAL_SIMULATION_STEP)),
			MAX_GLOBAL_STEPS_PER_DISPATCH
		)
		if _global_step_count < 1:
			return
		_global_time_accumulator -= float(_global_step_count) * GLOBAL_SIMULATION_STEP
		_simulation_time += float(_global_step_count) * GLOBAL_SIMULATION_STEP
		_dispatch_dt = GLOBAL_SIMULATION_STEP
		_dispatch_simulation_time = _simulation_time
		_update_global_stats()
		if Engine.get_process_frames() % 30 == 0 and not _diagnostics_read_queued:
			_diagnostics_read_queued = true
			RenderingServer.call_on_render_thread(Callable(self, "_read_global_diagnostics_on_render_thread"))
		_global_tick_queued = true
		RenderingServer.call_on_render_thread(Callable(self, "_submit_global_tick"))
		return
	# A local RenderingDevice permits only one submitted command buffer at a time.
	# Retire the previous submission on the next frame, then expose its snapshot
	# to the renderer before queuing another simulation tick.
	if _gpu_submission_in_flight:
		_rd.sync()
		_gpu_submission_in_flight = false
		_read_snapshot()
		return
	var dt: float = minf(maxf(delta * _time_scale, 0.0), 0.033)
	_simulation_time += dt
	_dispatch_dt = dt
	_dispatch_simulation_time = _simulation_time
	var list := _rd.compute_list_begin()
	var start_time: float = _simulation_time - float(_global_step_count) * GLOBAL_SIMULATION_STEP
	for step_index in _global_step_count:
		_dispatch_simulation_time = start_time + float(step_index + 1) * GLOBAL_SIMULATION_STEP
		var step_emission_count: int = _big_bang_pending_count if step_index == 0 else 0
		_record_pass(list, 0, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 1, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 2, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 3, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 4, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 5, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 6, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 7, maxi(step_emission_count, 1), step_emission_count)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 8, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 9, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 10, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_input_is_a = not _input_is_a
		_completed_tick += 1
	_big_bang_pending_count = 0
	_rd.compute_list_end()
	_rd.submit()
	_gpu_submission_in_flight = true
	_input_is_a = not _input_is_a
	_completed_tick += 1
	if not _logged_tick:
		_logged_tick = true
		print("[LifeEvolution][GPU] First merge-capable tick submitted")

func _submit_global_tick() -> void:
	if not _initialized:
		_global_tick_queued = false
		return
	var list := _rd.compute_list_begin()
	var start_time: float = _simulation_time - float(_global_step_count) * GLOBAL_SIMULATION_STEP
	for step_index in _global_step_count:
		_dispatch_simulation_time = start_time + float(step_index + 1) * GLOBAL_SIMULATION_STEP
		var step_emission_count: int = _big_bang_pending_count if step_index == 0 else 0
		_record_pass(list, 0, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 1, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 2, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 3, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 4, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 5, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 6, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 7, maxi(step_emission_count, 1), step_emission_count)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 8, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 9, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_record_pass(list, 10, _particle_capacity)
		_rd.compute_list_add_barrier(list)
		_input_is_a = not _input_is_a
		_completed_tick += 1
	_big_bang_pending_count = 0
	_rd.compute_list_end()
	_global_tick_queued = false
	if _completed_tick % 120 == 0:
		print("[LifeEvolution][GPU] Global chemistry ticks advancing: tick=%d substeps=%d input=%s sim_time=%.2f" % [_completed_tick, _global_step_count, "A" if _input_is_a else "B", _simulation_time])
	if not _logged_tick:
		_logged_tick = true
		print("[LifeEvolution][GPU] First global render tick submitted")

func _apply_global_count(value: int) -> void:
	_active_count = value
	var merge_ratio: float = 1.0 - float(_active_count) / maxf(float(_particle_capacity), 1.0)
	_stats["particle_count"] = _active_count
	_stats["merged_count"] = maxi(_initial_count - _active_count, 0)
	_stats["merge_progress"] = clampf(1.0 - float(_active_count) / maxf(float(_initial_count), 1.0), 0.0, 1.0)
	_stats["phase"] = _get_phase()

func _update_global_stats() -> void:
	_stats["backend"] = "GPU resident"
	_stats["particle_count"] = _active_count
	_stats["active_force_count"] = _active_count
	_stats["phase"] = _get_phase()

func _record_pass(list, pass_id: int, dispatch_count: int, emission_count: int = 0) -> void:
	_rd.compute_list_bind_compute_pipeline(list, _pipeline)
	_rd.compute_list_bind_uniform_set(list, _uniform_a if _input_is_a else _uniform_b, 0)
	var params := PackedFloat32Array([
		_dispatch_dt,
		float(_config.get("gravity_constant", 5.0)),
		float(_config.get("force_scale", 1.0)),
		float(_config.get("boundary_radius", 200.0)),
		float(GRID_DIMENSION), GRID_CELL_SIZE, GRID_ORIGIN, float(MAX_CELL_PARTICLES),
		float(_config.get("merge_radius", 0.0)), float(_config.get("merge_speed", 2.0)), float(_config.get("gravity_softening", 0.5)), float(_config.get("merge_start", 18.0)),
		float(_config.get("expansion_strength", 15.0)) * (8.0 if _big_bang_until >= _dispatch_simulation_time else 1.0), float(_config.get("expansion_duration", 12.0)), _dispatch_simulation_time, float(pass_id),
		1.0 if _big_bang_until >= _dispatch_simulation_time else 0.0, float(emission_count), float(_particle_capacity), float(pass_id)
	])
	_rd.compute_list_set_push_constant(list, params.to_byte_array(), 80)
	_rd.compute_list_dispatch(list, ceili(float(dispatch_count) / WORKGROUP_SIZE), 1, 1)

func get_particle_positions() -> PackedVector3Array:
	if _global_gpu:
		return PackedVector3Array()
	_read_snapshot()
	return _render_positions

func get_particle_colors() -> PackedColorArray:
	if _global_gpu:
		return PackedColorArray()
	_read_snapshot()
	return _render_colors

func get_particle_radii() -> PackedFloat32Array:
	if _global_gpu:
		return PackedFloat32Array()
	_read_snapshot()
	return _render_radii

func _read_snapshot() -> void:
	if _global_gpu:
		return
	if not _initialized or _snapshot_tick == _completed_tick:
		return
	if _gpu_submission_in_flight:
		return
	# Snapshots are read only after tick() retires the previous submission. This
	# keeps GPU synchronization out of the renderer update path.
	if _snapshot_tick >= 0 and _completed_tick % _snapshot_interval_ticks != 0:
		return
	var previous_active_count: int = _active_count
	var count_bytes: PackedByteArray = _rd.buffer_get_data(_count_a if _input_is_a else _count_b)
	if count_bytes.size() >= 4:
		_active_count = count_bytes.decode_u32(0)
	_active_count = clampi(_active_count, 0, _particle_capacity)
	if _active_count != previous_active_count:
		print("[LifeEvolution][GPU] Merge compaction at %.2fs: %d -> %d active particles" % [_simulation_time, previous_active_count, _active_count])
	var bytes: PackedByteArray = _rd.buffer_get_data(_particle_a if _input_is_a else _particle_b, 0, _active_count * PARTICLE_FLOATS * 4)
	var values: PackedFloat32Array = bytes.to_float32_array()
	var total_speed: float = 0.0
	var max_speed: float = 0.0
	var total_mass: float = 0.0
	var total_radius: float = 0.0
	var weighted_position := Vector3.ZERO
	for i in _active_count:
		var base: int = i * PARTICLE_FLOATS
		var position := Vector3(values[base], values[base + 1], values[base + 2])
		var velocity := Vector3(values[base + 4], values[base + 5], values[base + 6])
		_positions[i] = position
		_colors[i] = Color(values[base + 8], values[base + 9], values[base + 10], 1.0)
		_radii[i] = values[base + 11]
		var speed: float = velocity.length()
		total_radius += position.length()
		weighted_position += position * values[base + 3]
		total_speed += speed
		max_speed = maxf(max_speed, speed)
		total_mass += values[base + 3]
	var merge_ratio: float = 1.0 - float(_active_count) / maxf(float(_particle_capacity), 1.0)
	_stats["particle_count"] = _active_count
	_stats["atom_count"] = int(_active_count * 0.02)
	_stats["molecule_count"] = int(merge_ratio * _particle_capacity * 0.004)
	_stats["max_complexity"] = 2 if merge_ratio > 0.05 else 1
	_stats["avg_speed"] = total_speed / maxf(float(_active_count), 1.0)
	_stats["max_speed"] = max_speed
	_stats["active_force_count"] = _active_count
	_stats["avg_force"] = total_mass / maxf(float(_active_count), 1.0) * float(_config.get("gravity_constant", 5.0))
	_stats["max_force"] = _stats["avg_force"]
	_stats["avg_accel"] = float(_config.get("gravity_constant", 5.0))
	_stats["max_accel"] = _stats["avg_accel"]
	_stats["mean_radius"] = total_radius / maxf(float(_active_count), 1.0)
	var center_of_mass := weighted_position / maxf(total_mass, 0.0001)
	_stats["center_of_mass"] = [center_of_mass.x, center_of_mass.y, center_of_mass.z]
	var phase: String = _get_phase()
	_stats["phase"] = phase
	_stats["merged_count"] = maxi(_initial_count - _active_count, 0)
	_stats["merge_progress"] = clampf(1.0 - float(_active_count) / maxf(float(_initial_count), 1.0), 0.0, 1.0)
	if phase != _last_phase:
		_last_phase = phase
		print("[LifeEvolution][GPU] Lifecycle phase: %s at %.2fs" % [phase, _simulation_time])
	if _completed_tick % 120 == 0:
		print("[LifeEvolution][GPU] Snapshot t=%.2fs active=%d avg_speed=%.4f" % [_simulation_time, _active_count, _stats["avg_speed"]])
	_update_merged_core(merge_ratio)
	_render_positions = _positions.slice(0, _active_count)
	_render_colors = _colors.slice(0, _active_count)
	_render_radii = _radii.slice(0, _active_count)
	_snapshot_tick = _completed_tick

func _update_merged_core(merge_ratio: float) -> void:
	# Merged bodies are rendered as surviving GPU particles. The legacy center
	# mesh was only a progress indicator at the origin and could not move.
	if _merged_core != null:
		_merged_core.visible = false

func get_statistics_json() -> String:
	return JSON.stringify(_stats)

func _make_stats() -> Dictionary:
	return {"particle_count": _initial_count, "atom_count": 0, "molecule_count": 0, "organism_count": 0, "max_complexity": 1, "temperature": float(_config.get("temperature", 1000000.0)), "total_energy": 0.0, "center_of_mass": [0.0, 0.0, 0.0], "mean_radius": 0.0, "avg_speed": 0.0, "max_speed": 0.0, "avg_accel": 0.0, "max_accel": 0.0, "active_force_count": _initial_count, "avg_force": 0.0, "max_force": 0.0, "phase": "Expansion", "merged_count": 0, "merge_progress": 0.0}

func _get_phase() -> String:
	var merge_start: float = float(_config.get("merge_start", 18.0))
	var merge_duration: float = maxf(float(_config.get("merge_duration", 90.0)), 0.001)
	var expansion_duration: float = float(_config.get("expansion_duration", 12.0))
	if _simulation_time < expansion_duration:
		return "Expansion"
	if _simulation_time < merge_start:
		return "Collapse"
	if _simulation_time < merge_start + merge_duration and _active_count > 1:
		return "Coalescence"
	return "Stable Core"

func _create_uniform_set(input_buffer: RID, output_buffer: RID, input_counter: RID, output_counter: RID) -> RID:
	var uniforms: Array[RDUniform] = []
	for binding in 12:
		var uniform := RDUniform.new()
		uniform.uniform_type = RenderingDevice.UNIFORM_TYPE_STORAGE_BUFFER
		uniform.binding = binding
		uniforms.append(uniform)
	uniforms[0].add_id(input_buffer)
	uniforms[1].add_id(output_buffer)
	uniforms[2].add_id(_cell_counts)
	uniforms[3].add_id(_cell_indices)
	uniforms[4].add_id(_merge_counts)
	uniforms[5].add_id(_merge_flags)
	uniforms[6].add_id(_merge_partners)
	uniforms[7].add_id(input_counter)
	uniforms[8].add_id(output_counter)
	uniforms[9].add_id(_render_output)
	uniforms[10].add_id(_render_count)
	uniforms[11].add_id(_diagnostics)
	return _rd.uniform_set_create(uniforms, _shader, 0)

func refresh_gpu_statistics() -> void:
	if _global_gpu and not _diagnostics_read_queued:
		_diagnostics_read_queued = true
		RenderingServer.call_on_render_thread(Callable(self, "_read_global_diagnostics_on_render_thread"))

func _read_global_diagnostics_on_render_thread() -> void:
	if not _global_gpu or not _initialized:
		_diagnostics_read_queued = false
		return
	var bytes: PackedByteArray = _rd.buffer_get_data(_diagnostics)
	if bytes.size() >= 52:
		var particle_count: int = int(bytes.decode_u32(0))
		var atom_count: int = int(bytes.decode_u32(4))
		var molecule_count: int = int(bytes.decode_u32(8))
		var organism_count: int = int(bytes.decode_u32(12))
		var speed_sum: int = int(bytes.decode_u32(16))
		var max_speed: int = int(bytes.decode_u32(20))
		var radius_sum: int = int(bytes.decode_u32(24))
		var mass_sum: int = int(bytes.decode_u32(28))
		var accel_sum: int = int(bytes.decode_u32(32))
		var max_accel: int = int(bytes.decode_u32(36))
		var force_sum: int = int(bytes.decode_u32(40))
		var max_force: int = int(bytes.decode_u32(44))
		var active_force_count: int = int(bytes.decode_u32(48))
		call_deferred("_apply_gpu_diagnostics", particle_count, atom_count, molecule_count, organism_count, speed_sum, max_speed, radius_sum, mass_sum, accel_sum, max_accel, force_sum, max_force, active_force_count)
	if _focus_read_requested:
		_focus_read_requested = false
		var output_buffer: RID = _particle_b if _input_is_a else _particle_a
		var particle_bytes: PackedByteArray = _rd.buffer_get_data(output_buffer)
		var values: PackedFloat32Array = particle_bytes.to_float32_array()
		var best_mass: float = 0.0
		var best_position := Vector3.ZERO
		var limit: int = min(_active_count, values.size() / PARTICLE_FLOATS)
		for i in limit:
			var base: int = i * PARTICLE_FLOATS
			var mass: float = values[base + 3]
			if mass > best_mass:
				best_mass = mass
				best_position = Vector3(values[base], values[base + 1], values[base + 2])
		call_deferred("_apply_gpu_heaviest", best_position, best_mass)
	_diagnostics_read_queued = false

func _apply_gpu_diagnostics(particle_count: int, atom_count: int, molecule_count: int, organism_count: int, speed_sum: int, max_speed: int, radius_sum: int, mass_sum: int, accel_sum: int, max_accel: int, force_sum: int, max_force: int, active_force_count: int) -> void:
	_active_count = clampi(particle_count, 0, _particle_capacity)
	var body_count: float = maxf(float(_active_count), 1.0)
	_stats["particle_count"] = _active_count
	_stats["atom_count"] = atom_count
	_stats["molecule_count"] = molecule_count
	_stats["organism_count"] = organism_count
	_stats["merged_count"] = maxi(_initial_count - _active_count, 0)
	_stats["merge_progress"] = clampf(1.0 - float(_active_count) / maxf(float(_initial_count), 1.0), 0.0, 1.0)
	_stats["max_complexity"] = 4 if organism_count > 0 else (3 if molecule_count > 0 else (2 if atom_count > 0 else 1))
	_stats["avg_speed"] = float(speed_sum) / 1000.0 / body_count
	_stats["max_speed"] = float(max_speed) / 1000.0
	_stats["mean_radius"] = float(radius_sum) / 1000.0 / body_count
	_stats["avg_accel"] = float(accel_sum) / 1000.0 / body_count
	_stats["max_accel"] = float(max_accel) / 1000.0
	_stats["avg_force"] = float(force_sum) / 1000.0 / body_count
	_stats["max_force"] = float(max_force) / 1000.0
	_stats["active_force_count"] = active_force_count
	_stats["total_energy"] = 0.5 * float(mass_sum) / 1000.0 * _stats["avg_speed"] * _stats["avg_speed"]

func _apply_gpu_heaviest(position: Vector3, mass: float) -> void:
	_gpu_heaviest_position = position
	_gpu_heaviest_mass = mass

func request_focus_heaviest() -> void:
	if _global_gpu:
		_focus_read_requested = true
		refresh_gpu_statistics()

func get_heaviest_position() -> Vector3:
	return _gpu_heaviest_position

func get_heaviest_mass() -> float:
	return _gpu_heaviest_mass

func _initial_particle_bytes() -> PackedByteArray:
	var values := PackedFloat32Array()
	values.resize(_particle_capacity * PARTICLE_FLOATS)
	var radius: float = float(_config.get("radius", 5.0))
	for i in _particle_capacity:
		# Hash each index into independent, deterministic samples. The previous
		# low-discrepancy sequence made the initial cloud visibly spiral.
		var index_value: float = float(i)
		var u: float = fmod(absf(sin(index_value * 12.9898 + 78.233) * 43758.5453), 1.0)
		var v: float = fmod(absf(sin(index_value * 39.3467 + 11.135) * 24634.6345), 1.0)
		var radial: float = fmod(absf(sin(index_value * 73.1569 + 19.417) * 15731.7431), 1.0)
		var z: float = 1.0 - 2.0 * u
		var ring: float = sqrt(maxf(1.0 - z * z, 0.0))
		var angle: float = v * TAU
		var distance: float = radius * pow(maxf(radial, 0.0001), 0.3333333)
		var initial_velocity: float = float(_config.get("initial_velocity", 1.5))
		var base: int = i * PARTICLE_FLOATS
		values[base] = cos(angle) * ring * distance
		values[base + 1] = z * distance
		values[base + 2] = sin(angle) * ring * distance
		values[base + 3] = 1.0
		values[base + 4] = cos(angle) * ring * initial_velocity
		values[base + 5] = z * initial_velocity
		values[base + 6] = sin(angle) * ring * initial_velocity
		values[base + 7] = 0.0
		values[base + 8] = 0.35
		values[base + 9] = 0.65
		values[base + 10] = 1.0
		values[base + 11] = 0.35
		var species_roll: int = i % 10
		# Explicit seed proportions: 40 percent protons, 40 percent
		# electrons, and 20 percent neutrons. This makes the first reactions
		# reproducible and keeps the initial colors chemically meaningful.
		if species_roll < 4:
			values[base + 3] = 1.0
			values[base + 7] = 1.0
			values[base + 8] = 0.95
			values[base + 9] = 0.18
			values[base + 10] = 0.08
			values[base + 12] = 1.0
			values[base + 14] = 1.0
		elif species_roll < 8:
			values[base + 3] = 0.1
			values[base + 7] = -1.0
			values[base + 8] = 0.12
			values[base + 9] = 0.72
			values[base + 10] = 1.0
			values[base + 11] = 0.22
			values[base + 12] = 3.0
			values[base + 14] = -1.0
		else:
			values[base + 3] = 1.0
			values[base + 8] = 0.72
			values[base + 9] = 0.72
			values[base + 10] = 0.78
			values[base + 12] = 2.0
			values[base + 14] = 0.0
		values[base + 13] = 0.0
		values[base + 15] = 1.0
	return values.to_byte_array()

func get_particle_count() -> int:
	return _active_count

func get_completed_tick() -> int:
	return _completed_tick

func get_frame() -> int:
	return _completed_tick

func get_simulation_time() -> float:
	return _simulation_time

func get_max_complexity() -> int:
	return int(_stats.get("max_complexity", 1))

func is_initialized() -> bool:
	return _initialized

func is_gpu_resident() -> bool:
	return _global_gpu

func has_initialization_failed() -> bool:
	return _initialization_failed

func is_paused() -> bool:
	return _paused

func set_paused(value: bool) -> void:
	_paused = value

func set_time_scale(value: float) -> void:
	_time_scale = maxf(value, 0.0)

func get_time_scale() -> float:
	return _time_scale

func reset() -> void:
	if not _initialized:
		return
	if _global_gpu:
		_input_is_a = true
		_active_count = _initial_count
		_simulation_time = 0.0
		_completed_tick = 0
		_global_time_accumulator = 0.0
		_global_tick_queued = false
		_snapshot_tick = -1
		_last_phase = ""
		_big_bang_until = -1.0
		_big_bang_pending_count = 0
		_gpu_heaviest_position = Vector3.ZERO
		_gpu_heaviest_mass = 0.0
		RenderingServer.call_on_render_thread(Callable(self, "_reset_global_buffers"))
		return
	# Reset can be called after several compute submissions. Retire the local
	# device before replacing resources so no old command buffer can reference
	# buffers that are about to be freed.
	if _gpu_submission_in_flight:
		_rd.sync()
		_gpu_submission_in_flight = false
	_rd.free_rid(_particle_a)
	_rd.free_rid(_particle_b)
	_rd.free_rid(_count_a)
	_rd.free_rid(_count_b)
	_particle_a = _rd.storage_buffer_create(_particle_capacity * PARTICLE_FLOATS * 4, _initial_particle_bytes())
	_particle_b = _rd.storage_buffer_create(_particle_capacity * PARTICLE_FLOATS * 4)
	_active_count = _initial_count
	_count_a = _rd.storage_buffer_create(8, PackedInt32Array([_active_count, 0]).to_byte_array())
	_count_b = _rd.storage_buffer_create(8, PackedInt32Array([0, 0]).to_byte_array())
	_uniform_a = _create_uniform_set(_particle_a, _particle_b, _count_a, _count_b)
	_uniform_b = _create_uniform_set(_particle_b, _particle_a, _count_b, _count_a)
	_input_is_a = true
	_simulation_time = 0.0
	_completed_tick = 0
	_snapshot_tick = -1
	_last_phase = ""
	_big_bang_until = -1.0
	_big_bang_pending_count = 0
	_render_positions = PackedVector3Array()
	_render_colors = PackedColorArray()
	_render_radii = PackedFloat32Array()

func _exit_tree() -> void:
	if _rd == null:
		return
	if _global_gpu:
		# Global RenderingDevice RIDs are owned by the render thread. The engine
		# will release them with the scene; do not free them from the main thread.
		return
	if _gpu_submission_in_flight:
		_rd.sync()
		_gpu_submission_in_flight = false
	_free_gpu_rid(_uniform_a)
	_free_gpu_rid(_uniform_b)
	_free_gpu_rid(_particle_a)
	_free_gpu_rid(_particle_b)
	_free_gpu_rid(_cell_counts)
	_free_gpu_rid(_cell_indices)
	_free_gpu_rid(_merge_counts)
	_free_gpu_rid(_merge_flags)
	_free_gpu_rid(_merge_partners)
	_free_gpu_rid(_render_output)
	_free_gpu_rid(_render_count)
	_free_gpu_rid(_diagnostics)
	_free_gpu_rid(_count_a)
	_free_gpu_rid(_count_b)
	_free_gpu_rid(_pipeline)
	_free_gpu_rid(_shader)

func _free_gpu_rid(rid: RID) -> void:
	if rid.is_valid():
		_rd.free_rid(rid)

func _reset_global_buffers() -> void:
	if not _global_gpu or not _initialized:
		return
	_rd.buffer_update(_particle_a, 0, _particle_capacity * PARTICLE_FLOATS * 4, _initial_particle_bytes())
	_rd.buffer_update(_count_a, 0, 8, PackedInt32Array([_initial_count, 0]).to_byte_array())
	_rd.buffer_update(_count_b, 0, 8, PackedInt32Array([0, 0]).to_byte_array())
	_rd.buffer_update(_render_count, 0, 4, PackedInt32Array([0]).to_byte_array())
	var diagnostics_zeroes := PackedByteArray()
	diagnostics_zeroes.resize(52)
	_rd.buffer_update(_diagnostics, 0, 52, diagnostics_zeroes)

func trigger_big_bang() -> void:
	if not _initialized:
		return
	var available: int = maxi(_particle_capacity - _active_count, 0)
	if available <= 0:
		push_warning("Big Bang emission skipped: particle capacity is full. Let existing bodies merge first.")
		return
	_big_bang_until = _simulation_time + 8.0
	_big_bang_pending_count = mini(available, maxi(_initial_count / 2, 10000))
	print("[LifeEvolution][GPU] Big Bang emission queued: +%d particles, existing state preserved" % _big_bang_pending_count)
