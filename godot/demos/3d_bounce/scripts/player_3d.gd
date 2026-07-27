extends AurumEntity3D

# Player3D — WASD to move on the XZ plane, Space to jump.
#
# This is the simplest possible 3D demo. The player is a cube that
# moves around a plane. A target cube floats in the air; the player
# has to jump to reach it.

@export var move_speed: float = 6.0
@export var jump_impulse: float = 8.0


func _ready() -> void:
	tag = "Player3D"
	super._ready()


func _process(delta: float) -> void:
	var v := Vector3.ZERO
	if Input.is_key_pressed(KEY_W) or Input.is_key_pressed(KEY_UP):
		v.z -= 1.0
	if Input.is_key_pressed(KEY_S) or Input.is_key_pressed(KEY_DOWN):
		v.z += 1.0
	if Input.is_key_pressed(KEY_A) or Input.is_key_pressed(KEY_LEFT):
		v.x -= 1.0
	if Input.is_key_pressed(KEY_D) or Input.is_key_pressed(KEY_RIGHT):
		v.x += 1.0
	if v.length() > 0.0:
		v = v.normalized()
	# Read the current velocity to preserve y (gravity) and only change x/z.
	var vel: Dictionary = Aurum.get_component(entity_id, "Velocity3D")
	var vy: float = vel.get("y", 0.0) if not vel.is_empty() else 0.0
	# Jump if grounded and Space is pressed.
	if Input.is_key_pressed(KEY_SPACE) and abs(vy) < 0.1:
		vy = jump_impulse
	Aurum.set_component(entity_id, "Velocity3D", {
		"x": v.x * move_speed,
		"y": vy,
		"z": v.z * move_speed,
	})
