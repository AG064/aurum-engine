extends Node2D
class_name AurumEntity

# AurumEntity — a 2D entity that mirrors a Rust-side engine entity.
#
# When the scene is created, `_ready` calls `Aurum.spawn()` and stores the
# resulting entity id. Every change to position/velocity/components is
# written back to the engine. The engine is the source of truth for
# state, but the scene tree handles rendering and input.
#
# This is the GDScript-friendly side of the 2D module. The Rust side
# (aurum-2d crate) defines the same component names with the same
# field names, so entities created in Rust are visible to GDScript and
# vice versa.

var entity_id: int = 0
var _type_name: String = "Position2D"


func _ready() -> void:
	entity_id = Aurum.spawn()
	# Default components. Override or extend in subclasses.
	_sync_to_engine()


func _exit_tree() -> void:
	if entity_id != 0 and Aurum.entity_exists(entity_id):
		Aurum.despawn(entity_id)


func _process(_delta: float) -> void:
	_sync_to_engine()


# Subclasses override this. The default just stores the node's position.
func _sync_to_engine() -> void:
	Aurum.set_component(entity_id, _type_name, {
		"x": position.x,
		"y": position.y,
	})
