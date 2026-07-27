extends Node3D

# Main 3D scene controller.
#
# Spawns a player cube, a target cube, and the 3D kinematics system.
# Detects AABB collisions between them. Hides the cursor on game
# start. F1 toggles the dev console.

@onready var kinematics: Node = $Kinematics
@onready var player_spawn: Marker3D = $PlayerSpawn
@onready var target_spawn: Marker3D = $TargetSpawn
@onready var hud: CanvasLayer = $HUD
@onready var hud_label: Label = $HUD/Label
@onready var dev_console: CanvasLayer = $DevConsole

const Player3DScene = preload("res://demos/3d_bounce/scenes/player.tscn")
const Target3DScene = preload("res://demos/3d_bounce/scenes/target.tscn")

const COLLISION_DIST := 1.2

var _player: Node3D = null
var _target: Node3D = null


func _ready() -> void:
	Aurum.register_module("3d")
	Aurum.register_module("demo_3d")
	Aurum.event_received.connect(_on_event_received)
	Aurum.state_set("score", 0)
	_spawn_player()
	_spawn_target()
	_refresh_hud()


func _process(_delta: float) -> void:
	_check_collision()
	_refresh_hud()


func _spawn_player() -> void:
	_player = Player3DScene.instantiate()
	_player.position = player_spawn.position
	add_child(_player)


func _spawn_target() -> void:
	_target = Target3DScene.instantiate()
	_target.position = target_spawn.position
	add_child(_target)


func _check_collision() -> void:
	if _player == null or _target == null:
		return
	var ppos: Dictionary = Aurum.get_component(_player.entity_id, "Position3D")
	var tpos: Dictionary = Aurum.get_component(_target.entity_id, "Position3D")
	if ppos.is_empty() or tpos.is_empty():
		return
	var d := Vector3(
		tpos.get("x", 0.0) - ppos.get("x", 0.0),
		tpos.get("y", 0.0) - ppos.get("y", 0.0),
		tpos.get("z", 0.0) - ppos.get("z", 0.0),
	)
	if d.length() < COLLISION_DIST:
		_on_target_hit()


func _on_target_hit() -> void:
	# Award score, respawn target at a new random location.
	Aurum.state_set("score", Aurum.state_get("score", 0) + 1)
	Aurum.emit_event("TargetHit", {})
	# Respawn target at a random offset.
	var new_pos := Vector3(
		randf_range(-4.0, 4.0),
		2.0 + randf() * 1.0,
		randf_range(-4.0, 4.0),
	)
	_target.position = new_pos
	Aurum.set_component(_target.entity_id, "Position3D", {
		"x": new_pos.x,
		"y": new_pos.y,
		"z": new_pos.z,
	})


func _on_event_received(type_name: String, _data: Dictionary) -> void:
	if type_name == "TargetHit":
		pass  # already handled


func _refresh_hud() -> void:
	hud_label.text = "Score: %d\n\nWASD to move, Space to jump.\nF1 for dev console." % Aurum.state_get("score", 0)
