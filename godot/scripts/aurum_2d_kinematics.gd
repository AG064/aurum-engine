extends Node

# Aurum2DKinematics — 2D movement system.
#
# Reads `Velocity2D` components, integrates position over time, and writes
# back to `Position2D` components. Then mirrors the change to scene nodes
# via their `entity_id` so Godot's renderer sees the new position.
#
# This is the "2D module" on the GDScript side. The same components and
# the same field names exist in `aurum_2d` (Rust), so the engine and
# scene tree stay in sync.
#
# Add this node as a child of your main scene to enable 2D movement.

@export var step_rate: float = 60.0
@export var wrap_bounds: Vector2 = Vector2.ZERO  # (0, 0) = no wrap

var _accumulator: float = 0.0
var _node_for_entity: Dictionary = {}


func _ready() -> void:
	# Register the 2D module with the engine so other code can ask
	# `Aurum.has_module("2d")` to know it's available.
	Aurum.register_module("2d")
	_rebuild_node_index()


func _process(delta: float) -> void:
	# Use a fixed timestep for stable physics, with a cap to avoid spirals.
	var rate: float = max(step_rate, 1.0)
	var step: float = 1.0 / rate
	_accumulator += delta
	var ticks: int = 0
	while _accumulator >= step and ticks < 5:
		_step(step)
		_accumulator -= step
		ticks += 1


func register_node(node: Node) -> void:
	# Called by AurumEntity (or similar) on _ready to map entity id → node.
	if node and "entity_id" in node and node.entity_id != 0:
		_node_for_entity[node.entity_id] = node


func _step(dt: float) -> void:
	var moved_any := false
	for entity in Aurum.get_entities_with("Velocity2D"):
		var vel: Dictionary = Aurum.get_component(entity, "Velocity2D")
		var pos_dict: Dictionary = Aurum.get_component(entity, "Position2D")
		if pos_dict.is_empty():
			continue
		var vx: float = vel.get("x", 0.0)
		var vy: float = vel.get("y", 0.0)
		if vx == 0.0 and vy == 0.0:
			continue
		var x: float = pos_dict.get("x", 0.0) + vx * dt
		var y: float = pos_dict.get("y", 0.0) + vy * dt
		if wrap_bounds.x > 0.0:
			if x < 0.0:
				x += wrap_bounds.x
			elif x > wrap_bounds.x:
				x -= wrap_bounds.x
		if wrap_bounds.y > 0.0:
			if y < 0.0:
				y += wrap_bounds.y
			elif y > wrap_bounds.y:
				y -= wrap_bounds.y
		Aurum.set_component(entity, "Position2D", {"x": x, "y": y})
		moved_any = true
	if moved_any:
		_sync_nodes_to_engine()


func _sync_nodes_to_engine() -> void:
	for entity in Aurum.get_entities_with("Position2D"):
		var node: Node = _node_for_entity.get(entity)
		if node == null:
			continue
		var pos: Dictionary = Aurum.get_component(entity, "Position2D")
		node.position = Vector2(pos.get("x", 0.0), pos.get("y", 0.0))


func _rebuild_node_index() -> void:
	_node_for_entity.clear()
	for child in get_tree().get_nodes_in_group("aurum_entities"):
		if "entity_id" in child and child.entity_id != 0:
			_node_for_entity[child.entity_id] = child
