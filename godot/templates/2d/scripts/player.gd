extends Square
class_name Player

# Player template — arrow keys / WASD to move.
# Copy this file to your project and customize.

@export var speed: float = 320.0


func _ready() -> void:
	tag = "Player"
	color = Color(0.4, 0.6, 0.9)
	direction = Vector2.ZERO
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
