extends Square
class_name Coin

# Coin template — moves in a random direction, wraps around the arena.
# A real game would handle pickup logic in the main scene.

@export var speed: float = 140.0


func _ready() -> void:
	tag = "Coin"
	color = Color(0.95, 0.78, 0.2)
	var angle := randf() * TAU
	direction = Vector2(cos(angle), sin(angle))
	super._ready()
