@tool
extends EditorPlugin

# Aurum editor plugin.
#
# The plugin is intentionally minimal: there are no custom resource editors
# or scene inspectors in v0.1.0. Genre-specific add-ons (aurum-2d, aurum-3d,
# aurum-vn, etc.) can extend this base plugin to add their own tooling.
#
# For now, enabling the plugin makes the `Mavis` class available to GDScript
# (via the GDExtension) and registers the `Aurum` autoload.

const ENGINE_CLASS := "Mavis"


func _enter_tree() -> void:
	# The GDExtension is loaded automatically by the engine when the
	# `aurum.gdextension` manifest is in the add-on's bin/ directory.
	# We just verify the class is reachable and print a friendly message.
	if ClassDB.class_exists(ENGINE_CLASS):
		print("[Aurum] Mavis class loaded; engine ready.")
	else:
		push_warning("[Aurum] Mavis class not found. Check addons/aurum/bin/aurum.gdextension and the DLL location.")


func _exit_tree() -> void:
	pass
