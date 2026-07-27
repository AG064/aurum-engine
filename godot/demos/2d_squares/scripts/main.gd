extends Node2D

# Main scene controller for the 2D squares demo.
#
# Responsibilities:
# - Spawn coins over time
# - Detect collisions between Player and Coins (AABB in GDScript)
# - Forward game events to the HUD
# - Reset the game on R press
# - Show the dev console
#
# This is a thin "controller" scene — most logic is in the engine (via
# Aurum) and the individual entity scripts.

@onready var kinematics: Node = $Kinematics
@onready var player_spawn: Marker2D = $PlayerSpawn
@onready var hud: CanvasLayer = $HUD
@onready var world: Node2D = $World
@onready var dev_console: CanvasLayer = $DevConsole
@onready var game_over_panel: Panel = $GameOverPanel
@onready var game_over_label: Label = $GameOverPanel/MarginContainer/VBoxContainer/MessageLabel

const Player = preload("res://demos/2d_squares/scripts/player.gd")
const Coin = preload("res://demos/2d_squares/scripts/coin.gd")

const ARENA_WIDTH := 1280.0
const ARENA_HEIGHT := 720.0
const PLAYER_SIZE := 40.0
const COIN_SIZE := 24.0
const COIN_SPAWN_INTERVAL := 0.9
const MAX_COINS := 12

var _player: Node = null
var _coin_spawn_timer: float = 0.0
var _coin_scene: PackedScene = null


func _ready() -> void:
	# Configure the kinematics system to wrap coins around the arena.
	kinematics.wrap_bounds = Vector2(ARENA_WIDTH, ARENA_HEIGHT)
	# Register 2D module availability (in case main runs without the autoload).
	Aurum.register_module("2d")
	Aurum.register_module("demo")

	Aurum.event_received.connect(_on_event_received)
	game_over_panel.hide()

	_spawn_player()
	_coin_scene = preload("res://demos/2d_squares/scenes/coin.tscn")
	# Spawn a few coins up front so the player has something to chase.
	for i in range(5):
		_spawn_coin()
	Aurum.state_set("coins_total", Aurum.get_entities_with("Coin").size())


func _process(delta: float) -> void:
	_coin_spawn_timer += delta
	if _coin_spawn_timer >= COIN_SPAWN_INTERVAL:
		_coin_spawn_timer = 0.0
		if Aurum.get_entities_with("Coin").size() < MAX_COINS:
			_spawn_coin()
			Aurum.state_set("coins_total", Aurum.state_get("coins_total", 0) + 1)
	_check_collisions()
	_handle_reset_input()


func _spawn_player() -> void:
	_player = Player.new()
	_player.square_size = PLAYER_SIZE
	_player.position = player_spawn.position
	world.add_child(_player)


func _spawn_coin() -> void:
	var coin = Coin.new()
	coin.square_size = COIN_SIZE
	coin.position = Vector2(
		randf_range(COIN_SIZE, ARENA_WIDTH - COIN_SIZE),
		randf_range(COIN_SIZE, ARENA_HEIGHT - COIN_SIZE),
	)
	world.add_child(coin)


func _check_collisions() -> void:
	if _player == null or not is_instance_valid(_player):
		return
	var player_pos_dict: Dictionary = Aurum.get_component(_player.entity_id, "Position2D")
	if player_pos_dict.is_empty():
		return
	var ppos := Vector2(player_pos_dict.get("x", 0.0), player_dict_y(player_pos_dict))
	var player_rect := Rect2(
		ppos - Vector2(PLAYER_SIZE * 0.5, PLAYER_SIZE * 0.5),
		Vector2(PLAYER_SIZE, PLAYER_SIZE),
	)
	var coins := Aurum.get_entities_with("Coin")
	for entity in coins:
		var coin_pos: Dictionary = Aurum.get_component(entity, "Position2D")
		if coin_pos.is_empty():
			continue
		var cpos := Vector2(coin_pos.get("x", 0.0), coin_pos.get("y", 0.0))
		var coin_rect := Rect2(
			cpos - Vector2(COIN_SIZE * 0.5, COIN_SIZE * 0.5),
			Vector2(COIN_SIZE, COIN_SIZE),
		)
		if player_rect.intersects(coin_rect):
			_collect_coin(entity)


func player_dict_y(d: Dictionary) -> float:
	return d.get("y", 0.0)


func _collect_coin(entity: int) -> void:
	# Find the scene node and remove it (the entity is despawned in its
	# `_exit_tree`).
	for node in world.get_children():
		if "entity_id" in node and node.entity_id == entity:
			node.queue_free()
			break
	Aurum.emit_event("CoinCollected", {})
	if hud:
		hud.on_coin_collected()


func _on_event_received(type_name: String, _data: Dictionary) -> void:
	match type_name:
		"GameOver":
			_show_game_over()
		"CoinCollected":
			pass


func _show_game_over() -> void:
	var collected: int = Aurum.state_get("coins_collected", 0)
	var total: int = Aurum.state_get("coins_total", 0)
	game_over_label.text = "Game Over\n\nCoins collected: %d / %d\n\nPress R to play again." % [collected, total]
	game_over_panel.show()


func _handle_reset_input() -> void:
	if Input.is_key_pressed(KEY_R) and game_over_panel.visible:
		_reset_game()
	# Also allow reset at any time
	if Input.is_action_just_pressed("ui_cancel"):
		pass


func _reset_game() -> void:
	# Despawn all entities.
	for type_name in ["Player", "Coin"]:
		for entity in Aurum.get_entities_with(type_name):
			Aurum.despawn(entity)
	for child in world.get_children():
		child.queue_free()
	Aurum.state_set("score", 0)
	Aurum.state_set("lives", 3)
	Aurum.state_set("coins_collected", 0)
	Aurum.state_set("coins_total", 0)
	game_over_panel.hide()
	_spawn_player()
	for i in range(5):
		_spawn_coin()
	Aurum.state_set("coins_total", Aurum.get_entities_with("Coin").size())
