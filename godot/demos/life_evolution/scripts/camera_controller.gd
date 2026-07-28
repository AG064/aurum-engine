extends Camera3D
## Smooth camera controller for Life Evolution.
##
## Supports orbit, pan, zoom, and auto-focus on interesting objects.

## Target point to orbit around
@export var target: Vector3 = Vector3.ZERO
## Current distance from target
@export var distance: float = 1000.0
## Rotation in radians (x = pitch, y = yaw)
@export var rotation_angles: Vector2 = Vector2.ZERO
## Smoothing speed for camera movement
@export var smoothing: float = 5.0
## Auto-rotation enabled
@export var auto_rotate: bool = false
## Auto-rotation speed
@export var auto_rotate_speed: float = 0.1

var _target_position: Vector3 = Vector3.ZERO
var _target_distance: float = 1000.0
var _target_rotation: Vector2 = Vector2.ZERO
var _is_dragging: bool = false
var _last_mouse_pos: Vector2 = Vector2.ZERO

func _process(delta: float) -> void:
	if auto_rotate:
		_target_rotation.y += auto_rotate_speed * delta
	
	# Smooth interpolation
	target = target.lerp(_target_position, smoothing * delta)
	distance = lerp(distance, _target_distance, smoothing * delta)
	rotation_angles = rotation_angles.lerp(_target_rotation, smoothing * delta)
	
	# Apply camera transform
	var pitch := rotation_angles.x
	var yaw := rotation_angles.y
	
	var offset := Vector3(
		cos(pitch) * sin(yaw),
		sin(pitch),
		cos(pitch) * cos(yaw)
	) * distance
	
	global_position = target + offset
	look_at(target, Vector3.UP)

func _input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		_handle_mouse_button(event as InputEventMouseButton)
	elif event is InputEventMouseMotion:
		_handle_mouse_motion(event as InputEventMouseMotion)
	elif event is InputEventKey:
		_handle_key(event as InputEventKey)

func _handle_mouse_button(event: InputEventMouseButton) -> void:
	if event.pressed:
		match event.button_index:
			MOUSE_BUTTON_RIGHT:
				_is_dragging = true
				_last_mouse_pos = event.position
			MOUSE_BUTTON_MIDDLE:
				_is_dragging = true
				_last_mouse_pos = event.position
				Input.set_default_cursor_shape(Input.CURSOR_DRAG)
			MOUSE_BUTTON_WHEEL_UP:
				_target_distance *= 0.9
			MOUSE_BUTTON_WHEEL_DOWN:
				_target_distance *= 1.1
	else:
		_is_dragging = false
		Input.set_default_cursor_shape(Input.CURSOR_ARROW)

func _handle_mouse_motion(event: InputEventMouseMotion) -> void:
	if not _is_dragging:
		return
	
	var delta_pos := event.position - _last_mouse_pos
	_last_mouse_pos = event.position
	
	if event.button_mask & MOUSE_BUTTON_MASK_MIDDLE:
		# Pan
		var right := global_transform.basis.x
		var up := global_transform.basis.y
		var pan_speed := _target_distance * 0.001
		_target_position -= right * delta_pos.x * pan_speed
		_target_position += up * delta_pos.y * pan_speed
	else:
		# Orbit
		_target_rotation.y -= delta_pos.x * 0.005
		_target_rotation.x -= delta_pos.y * 0.005
		_target_rotation.x = clamp(_target_rotation.x, -PI/2 + 0.1, PI/2 - 0.1)

func _handle_key(event: InputEventKey) -> void:
	if not event.pressed:
		return
	
	match event.keycode:
		KEY_W, KEY_UP:
			_target_position -= global_transform.basis.z * _target_distance * 0.05
		KEY_S, KEY_DOWN:
			_target_position += global_transform.basis.z * _target_distance * 0.05
		KEY_A, KEY_LEFT:
			_target_position -= global_transform.basis.x * _target_distance * 0.05
		KEY_D, KEY_RIGHT:
			_target_position += global_transform.basis.x * _target_distance * 0.05
		KEY_R:
			_target_position = Vector3.ZERO
			_target_distance = 1000.0
			_target_rotation = Vector2.ZERO
		KEY_SPACE:
			auto_rotate = not auto_rotate

## Focus on a specific world position.
func focus_on(world_position: Vector3, focus_distance: float = 0.0) -> void:
	_target_position = world_position
	if focus_distance > 0:
		_target_distance = focus_distance

## Get the current view distance.
func get_view_distance() -> float:
	return _target_distance
