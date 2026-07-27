extends Square

# Coin — moves in a random direction, wraps around the screen.
# When the player collides with it, the main scene removes the coin and
# increments the score.

@export var speed: float = 140.0
@export var square_size: float = 24.0
@export var square_color: Color = Color(0.95, 0.78, 0.2)


func _ready() -> void:
	custom_minimum_size = Vector2(square_size, square_size)
	size = Vector2(square_size, square_size)
	color = square_color
	tag = "Coin"
	super._ready()
	# Set a random initial velocity.
	var angle := randf() * TAU
	Aurum.set_component(entity_id, "Velocity2D", {
		"x": cos(angle) * speed,
		"y": sin(angle) * speed,
	})
