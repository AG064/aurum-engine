extends CanvasLayer

# Aurum dev console.
#
# Toggle with F1 (debug builds only). Shows entity count, registered
# modules, and a way to dump engine state to console output.
#
# Designed to be added to any scene for in-game debugging.

@export var toggle_key: Key = KEY_F1


var _visible_panel: Control = null
var _label: Label = null
var _refresh_timer: float = 0.0


func _ready() -> void:
	layer = 100
	_build_ui()
	hide()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == toggle_key and OS.is_debug_build():
			_toggle()


func _process(delta: float) -> void:
	if not visible:
		return
	_refresh_timer += delta
	if _refresh_timer > 0.25:
		_refresh_timer = 0.0
		_refresh_label()


func _toggle() -> void:
	if visible:
		hide()
	else:
		show()
		_refresh_label()


func _build_ui() -> void:
	var root := Control.new()
	root.name = "Root"
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)

	_visible_panel = Panel.new()
	_visible_panel.name = "Panel"
	_visible_panel.position = Vector2(20, 20)
	_visible_panel.size = Vector2(420, 280)
	_visible_panel.mouse_filter = Control.MOUSE_FILTER_STOP
	root.add_child(_visible_panel)

	_label = Label.new()
	_label.name = "Info"
	_label.position = Vector2(12, 12)
	_label.size = Vector2(400, 256)
	_label.add_theme_color_override("font_color", Color(0.9, 0.9, 0.95))
	_label.add_theme_font_size_override("font_size", 13)
	_label.text = "Aurum dev console (F1 to hide)"
	_visible_panel.add_child(_label)


func _refresh_label() -> void:
	var lines: Array[String] = []
	lines.append("[b]Aurum dev console[/b]  (F1 to hide)")
	lines.append("")
	lines.append("Entities:        %d" % Aurum.entity_count())
	lines.append("Time scale:      %.2f" % Aurum.get_time_scale())
	lines.append("Modules:         %s" % str(Aurum.list_modules()))
	lines.append("")
	lines.append("Sample components:")
	var shown := 0
	for type_name in ["Position2D", "Velocity2D", "Sprite", "Tag"]:
		var ids: Array = Aurum.get_entities_with(type_name)
		if ids.size() > 0:
			lines.append("  %s: %d entities" % [type_name, ids.size()])
			shown += 1
		if shown > 6:
			break
	lines.append("")
	lines.append("State keys: %d" % Aurum.state_get("__key_count", 0) if false else "")
	var state_lines: Array = []
	# (We don't have a key iterator yet, so show a few common keys.)
	for k in ["score", "lives", "paused"]:
		if Aurum.state_has(k):
			state_lines.append("%s = %s" % [k, str(Aurum.state_get(k))])
	if state_lines.is_empty():
		lines.append("State: (none set)")
	else:
		lines.append("State:")
		for sl in state_lines:
			lines.append("  " + sl)
	lines.append("")
	lines.append("Dump: press D to print JSON to console")

	_label.text = "\n".join(lines)


func _input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_D:
				var json := Aurum.save_to_json()
				print("[Aurum] Engine state:")
				print(json)
