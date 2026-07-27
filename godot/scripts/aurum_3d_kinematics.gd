extends Node

# Aurum3DKinematics — 3D movement system.
#
# Mirror of `aurum_2d_kinematics.gd` for Node3D. Reads `Velocity3D`
# components, integrates position, and writes back to scene nodes.
#
# Add this node as a child of your main 3D scene. Each entity that
# wants to participate should extend `aurum_entity_3d.gd` (or call
# `register_node` manually after spawning).

@export var step_rate: float = 60.0
@export var gravity: float = 9.8
@export var use_gravity: bool = true
@export var wrap_bounds: Vector3 = Vector3.ZERO  # (0, 0, 0) = no wrap

var _accumulator: float = 0.0
var _node_for_entity: Dictionary = {}


func _ready() -> void:
	Aurum.register_module("3d")
	_rebuild_node_index()


func _process(delta: float) -> void:
	var rate: float = max(step_rate, 1.0)
	var step: float = 1.0 / rate
	_accumulator += delta
	var ticks: int = 0
	while _accumulator >= step and ticks < 5:
		_step(step)
		_accumulator -= step
		ticks += 1


func register_node(node: Node) -> void:
	if node and "entity_id" in node and node.entity_id != 0:
		_node_for_entity[node.entity_id] = node


func _step(dt: float) -> void:
	var moved_any := false
	for entity in Aurum.get_entities_with("Velocity3D"):
		var vel: Dictionary = Aurum.get_component(entity, "Velocity3D")
		var pos_dict: Dictionary = Aurum.get_component(entity, "Position3D")
		if pos_dict.is_empty():
			continue
		var vx: float = vel.get("x", 0.0)
		var vy: float = vel.get("y", 0.0)
		var vz: float = vel.get("z", 0.0)
		# Apply gravity to the y-axis if enabled.
		if use_gravity:
			vy -= gravity * dt
			Aurum.set_component(entity, "Velocity3D", {"x": vx, "y": vy, "z": vz})
		if vx == 0.0 and vy == 0.0 and vz == 0.0 and not use_gravity:
			continue
		var x: float = pos_dict.get("x", 0.0) + vx * dt
		var y: float = pos_dict.get("y", 0.0) + vy * dt
		var z: float = pos_dict.get("z", 0.0) + vz * dt
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
		if wrap_bounds.z > 0.0:
			if z < 0.0:
				z += wrap_bounds.z
			elif z > wrap_bounds.z:
				z -= wrap_bounds.z
		Aurum.set_component(entity, "Position3D", {"x": x, "y": y, "z": z})
		moved_any = true
	if moved_any:
		_sync_nodes_to_engine()


func _sync_nodes_to_engine() -> void:
	for entity in Aurum.get_entities_with("Position3D"):
		var node: Node = _node_for_entity.get(entity)
		if node == null:
			continue
		var pos: Dictionary = Aurum.get_component(entity, "Position3D")
		node.position = Vector3(
			pos.get("x", 0.0),
			pos.get("y", 0.0),
			pos.get("z", 0.0),
		)


func _rebuild_node_index() -> void:
	_node_for_entity.clear()
	for child in get_tree().get_nodes_in_group("aurum_entities_3d"):
		if "entity_id" in child and child.entity_id != 0:
			_node_for_entity[child.entity_id] = child
