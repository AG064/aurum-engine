@tool
extends EditorPlugin
## Aurum editor plugin: the thin GDScript half of the editor bridge.
##
## Everything that manipulates the scene lives in Rust (`AurumEditor`). This
## script exists only to supply the three things that genuinely need the
## editor, which gdext does not expose without its experimental feature:
##
##   1. the edited scene root, so Rust has something to operate on;
##   2. the plugin lifecycle, so the bridge runs only in the editor;
##   3. a per-frame pump for the request directory.
##
## Keeping this file small is deliberate. The native class is the stable
## surface (see `crates/aurum-editor/src/lib.rs`), and every line added here is
## a line that can drift from it.

const BRIDGE_ROOT := "user://aurum_editor"

var _editor: AurumEditor = null
var _request_dir := ""
var _response_dir := ""


func _enter_tree() -> void:
	_editor = AurumEditor.new()
	_editor.name = "AurumEditor"
	add_child(_editor)

	# The bridge rendezvous is a directory pair under user://, so nothing
	# outside the project's own storage is touched.
	var root := ProjectSettings.globalize_path(BRIDGE_ROOT)
	_request_dir = root.path_join("requests")
	_response_dir = root.path_join("responses")
	DirAccess.make_dir_recursive_absolute(_request_dir)
	DirAccess.make_dir_recursive_absolute(_response_dir)

	scene_changed.connect(_on_scene_changed)
	_on_scene_changed(get_editor_interface().get_edited_scene_root())

	print("[aurum-editor] ready; requests in ", _request_dir)


func _exit_tree() -> void:
	if scene_changed.is_connected(_on_scene_changed):
		scene_changed.disconnect(_on_scene_changed)
	if _editor != null:
		_editor.clear_scene_root()
		_editor.queue_free()
		_editor = null


func _process(_delta: float) -> void:
	if _editor != null:
		_editor.pump_requests(_request_dir, _response_dir)


func _on_scene_changed(root: Node) -> void:
	if _editor == null:
		return
	if root == null:
		_editor.clear_scene_root()
	else:
		_editor.set_scene_root(root)
