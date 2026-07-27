extends Square

# Player — controlled by arrow keys / WASD.
#
# On _ready, the player square is set up with the right color, size, and tag.
# Each _process, we read input and write the desired velocity to the engine.
# The kinematics system then moves us, and the engine broadcasts the new
# position back to the scene node.

@export var speed: float = 320.0
@export var square_size: float = 40.0
@export var square_color: Color = Color(0.4, 0.6, 0.9)


func _ready() -> void:
	custom_minimum_size = Vector2(square_size, square_size)
	size = Vector2(square_size, square_size)
	color = square_color
	# Store the tag in self before calling super so Square._ready picks it up.
	tag = "Player"
	super._ready()


func _process(_delta: float) -> void:
	var v := Vector2.ZERO
	if Input.is_key_pressed(KEY_LEFT) or Input.is_key_pressed(KEY_A):
		v.x -= 1.0
	if Input.is_key_pressed(KEY_RIGHT) or Input.is_key_pressed(KEY_D):
		v.x += 1.0
	if Input.is_key_pressed(KEY_UP) or Input.is_key_pressed(KEY_W):
		v.y -= 1.0
	if Input.is_key_pressed(KEY_DOWN) or Input.is_key_pressed(KEY_S):
		v.y += 1.0
	if v.length() > 0.0:
		v = v.normalized()
	Aurum.set_component(entity_id, "Velocity2D", {
		"x": v.x * speed,
		"y": v.y * speed,
	})
