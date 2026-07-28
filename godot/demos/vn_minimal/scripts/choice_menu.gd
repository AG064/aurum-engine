extends VBoxContainer

# Choice menu — displays a list of buttons, one per choice. Click
# to pick. Emits `choice_picked(index)`.

signal choice_picked(index: int)

var _buttons: Array[Button] = []


func _ready() -> void:
	hide()


func set_choices(choices: Array) -> void:
	# Clear existing buttons.
	for child in get_children():
		child.queue_free()
	_buttons.clear()
	add_theme_constant_override("separation", 6)
	for i in range(choices.size()):
		var c: Dictionary = choices[i]
		var btn := Button.new()
		btn.text = "%d. %s" % [i + 1, c.get("text", "")]
		btn.focus_mode = Control.FOCUS_ALL
		btn.pressed.connect(_on_pressed.bind(i))
		add_child(btn)
		_buttons.append(btn)
	show()
	# Focus the first button so keyboard works.
	if _buttons.size() > 0:
		_buttons[0].grab_focus()


func _on_pressed(index: int) -> void:
	choice_picked.emit(index)


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventKey and event.pressed and not event.echo:
		var n: int = event.keycode - KEY_1
		if n >= 0 and n < _buttons.size():
			_buttons[n].pressed.emit()
			get_viewport().set_input_as_handled()
