@tool
extends EditorPlugin

# Aurum editor plugin.
#
# The plugin is intentionally minimal: there are no custom resource editors
# or scene inspectors in v0.1.0. Genre-specific add-ons (aurum-2d, aurum-3d,
# aurum-vn, etc.) can extend this base plugin to add their own tooling.
#
# For now, enabling the plugin makes the `AurumNode` class available to GDScript
# (via the GDExtension) and registers the `Aurum` autoload.

const ENGINE_CLASS := "AurumNode"
const DEBUG_RELOAD_MARKER_PATH := "res://.godot/aurum/aurum_godot.debug.reload"
const AURUM_MANIFEST_SUFFIX := "/addons/aurum/bin/aurum.gdextension"
const RELOAD_POLL_SECONDS := 0.25
const RELOAD_DEBOUNCE_MILLISECONDS := 350

var _poll_elapsed_seconds := 0.0
var _pending_marker_hash := ""
var _pending_since_msec := 0
var _last_applied_marker_hash := ""
var _failed_marker_hash := ""
var _reload_in_progress := false


func _enter_tree() -> void:
	# The GDExtension is loaded automatically by the engine when the
	# `aurum.gdextension` manifest is in the add-on's bin/ directory.
	# We just verify the class is reachable and print a friendly message.
	if ClassDB.class_exists(ENGINE_CLASS):
		print("[Aurum] AurumNode class loaded; engine ready.")
	else:
		push_warning("[Aurum] AurumNode class not found. Check addons/aurum/bin/aurum.gdextension and the DLL location.")
	_last_applied_marker_hash = _read_reload_marker_hash()
	set_process(true)


func _process(delta: float) -> void:
	_poll_elapsed_seconds += delta
	if _poll_elapsed_seconds < RELOAD_POLL_SECONDS:
		return
	_poll_elapsed_seconds = 0.0
	_poll_debug_reload_marker()


func _poll_debug_reload_marker() -> void:
	if _reload_in_progress:
		return
	var marker_hash := _read_reload_marker_hash()
	if marker_hash.is_empty():
		_pending_marker_hash = ""
		_pending_since_msec = 0
		return
	if marker_hash == _last_applied_marker_hash or marker_hash == _failed_marker_hash:
		return

	var now_msec := Time.get_ticks_msec()
	if marker_hash != _pending_marker_hash:
		_pending_marker_hash = marker_hash
		_pending_since_msec = now_msec
		return
	if now_msec - _pending_since_msec < RELOAD_DEBOUNCE_MILLISECONDS:
		return

	_pending_marker_hash = ""
	_pending_since_msec = 0
	_reload_aurum_extension(marker_hash)


func _read_reload_marker_hash() -> String:
	if not FileAccess.file_exists(DEBUG_RELOAD_MARKER_PATH):
		return ""
	var marker_hash := FileAccess.get_file_as_string(DEBUG_RELOAD_MARKER_PATH).strip_edges().to_upper()
	if marker_hash.length() != 64:
		return ""
	for character in marker_hash:
		if "0123456789ABCDEF".find(character) == -1:
			return ""
	return marker_hash


func _reload_aurum_extension(marker_hash: String) -> void:
	_reload_in_progress = true
	var loaded_extensions: Array[String] = []
	var aurum_extensions: Array[String] = []
	for extension in GDExtensionManager.get_loaded_extensions():
		var extension_path := str(extension)
		loaded_extensions.append(extension_path)
		if extension_path.ends_with(AURUM_MANIFEST_SUFFIX):
			aurum_extensions.append(extension_path)

	if aurum_extensions.size() != 1:
		_record_reload_failure(
			marker_hash,
			"expected exactly one loaded Aurum manifest; loaded=%s" % [loaded_extensions])
		_reload_in_progress = false
		return

	var status := GDExtensionManager.reload_extension(aurum_extensions[0])
	if status == OK:
		_last_applied_marker_hash = marker_hash
		_failed_marker_hash = ""
		print("[Aurum] Native debug extension reloaded for DLL %s." % marker_hash)
	else:
		_record_reload_failure(
			marker_hash,
			"GDExtensionManager.reload_extension returned status %s" % status)
	_reload_in_progress = false


func _record_reload_failure(marker_hash: String, details: String) -> void:
	_failed_marker_hash = marker_hash
	push_warning(
		"[Aurum] Native debug reload failed: %s. " % details
		+ "Save editor work and use a controlled editor restart before continuing.")


func _exit_tree() -> void:
	set_process(false)
	_reload_in_progress = false
