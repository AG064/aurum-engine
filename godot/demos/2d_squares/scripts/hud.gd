extends CanvasLayer

# HUD — shows the score, lives, and instructions.
# Lives and score are stored in Aurum state so they survive save/load.

@onready var label: Label = $Label


func _ready() -> void:
	Aurum.state_set("score", 0)
	Aurum.state_set("lives", 3)
	Aurum.state_set("coins_collected", 0)
	Aurum.state_set("coins_total", 0)
	_refresh()


func _process(_delta: float) -> void:
	_refresh()


func _refresh() -> void:
	var score: int = Aurum.state_get("score", 0)
	var lives: int = Aurum.state_get("lives", 3)
	var collected: int = Aurum.state_get("coins_collected", 0)
	var total: int = Aurum.state_get("coins_total", 0)
	label.text = "Score: %d   Lives: %d   Coins: %d / %d\n\nArrow keys / WASD to move. F1 for dev console. R to reset." % [score, lives, collected, total]


func on_coin_collected() -> void:
	Aurum.state_set("score", Aurum.state_get("score", 0) + 10)
	Aurum.state_set("coins_collected", Aurum.state_get("coins_collected", 0) + 1)


func on_player_hit() -> void:
	Aurum.state_set("lives", Aurum.state_get("lives", 0) - 1)
	if Aurum.state_get("lives", 0) <= 0:
		Aurum.emit_event("GameOver", {})
