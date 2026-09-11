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
## Beside the reload marker the build publishes, rather than under `user://`.
## The marker records that a DLL was written; this records that the new code is
## the code running, and the two belong together for anyone comparing them.
const LIVE_FINGERPRINT_PATH := "res://.godot/aurum/live-fingerprint.txt"

var _editor: AurumEditor = null
var _request_dir := ""
var _response_dir := ""
var _probe: Node = null
var _published := ""


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
	_publish_live_fingerprint()


## Record the fingerprint the loaded extension actually reports.
##
## A rebuild that installs a new DLL proves a file was written; only this
## proves the new code is the code running. Written to a file rather than
## served over a socket so that verifying a live reload needs nothing but a
## filesystem — no client library, and nothing to install.
##
## The probe is re-created whenever it goes away, because a reloaded
## extension can invalidate an instance created by the previous one.
func _publish_live_fingerprint() -> void:
	if _probe == null or not is_instance_valid(_probe):
		if not ClassDB.class_exists("AurumNode"):
			return
		_probe = ClassDB.instantiate("AurumNode")
		if _probe == null:
			return
		add_child(_probe)

	if not _probe.has_method("runtime_fingerprint"):
		_probe = null
		return

	var current := str(_probe.runtime_fingerprint())
	if current == _published:
		return
	_published = current

	DirAccess.make_dir_recursive_absolute(
		ProjectSettings.globalize_path("res://.godot/aurum"))
	var file := FileAccess.open(LIVE_FINGERPRINT_PATH, FileAccess.WRITE)
	if file != null:
		file.store_string(current)
		file.close()


func _on_scene_changed(root: Node) -> void:
	if _editor == null:
		return
	if root == null:
		_editor.clear_scene_root()
	else:
		_editor.set_scene_root(root)
