extends ColorRect
class_name Square

# Square — base class for a 2D entity in the squares demo.
#
# Subclasses set the visual properties (color, size) and behavior.
# This base class handles the engine integration: on _ready it spawns
# a new entity in the Aurum ECS, attaches a Position2D and Velocity2D
# component, and on _process it syncs the engine's position back to
# the scene node.
#
# Subclasses:
#   - player.gd  — controlled by the player
#   - coin.gd    — moves in a random direction, wraps around

var entity_id: int = 0
var tag: String = ""


func _ready() -> void:
	entity_id = Aurum.spawn()
	Aurum.set_component(entity_id, "Position2D", {
		"x": position.x,
		"y": position.y,
	})
	Aurum.set_component(entity_id, "Velocity2D", {
		"x": 0.0,
		"y": 0.0,
	})
	if "tag" in self and not tag.is_empty():
		Aurum.set_component(entity_id, "Tag", {"name": tag})
	add_to_group("aurum_entities")
	# Register with the kinematics system so engine positions sync to us.
	var kin := _find_kinematics()
	if kin:
		kin.register_node(self)


func _exit_tree() -> void:
	if entity_id != 0 and Aurum.entity_exists(entity_id):
		Aurum.despawn(entity_id)


func _process(_delta: float) -> void:
	if entity_id == 0:
		return
	var pos_dict: Dictionary = Aurum.get_component(entity_id, "Position2D")
	if pos_dict.is_empty():
		return
	position = Vector2(pos_dict.get("x", position.x), pos_dict.get("y", position.y))


func _find_kinematics() -> Node:
	for node in get_tree().get_nodes_in_group("aurum_kinematics"):
		return node
	return null
