extends AurumEntity3D

# Target3D — a floating cube that gently bobs up and down.
# Score when the player collides with it (handled by main.gd).

@export var bob_speed: float = 2.0
@export var bob_height: float = 1.0
@export var rotation_speed: float = 1.0

var _start_y: float = 0.0
var _time: float = 0.0


func _ready() -> void:
	tag = "Target3D"
	super._ready()
	_start_y = position.y


func _process(delta: float) -> void:
	_time += delta
	var pos_dict: Dictionary = Aurum.get_component(entity_id, "Position3D")
	if not pos_dict.is_empty():
		# Override the y to bob.
		var y: float = _start_y + sin(_time * bob_speed) * bob_height
		Aurum.set_component(entity_id, "Position3D", {
			"x": pos_dict.get("x", position.x),
			"y": y,
			"z": pos_dict.get("z", position.z),
		})
	# Rotate visually (the engine doesn't care about rotation, so this is
	# just for the player's enjoyment).
	rotation.y += rotation_speed * delta
