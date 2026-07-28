extends Control

# Dialogue box — minimal. Displays the current line of dialogue or
# narration. Call `set_line(event: Dictionary)` to update. The
# `append` field lets consecutive lines glue together visually.

signal advanced

@onready var speaker_label: Label = $VBox/Speaker
@onready var text_label: RichTextLabel = $VBox/Text
@onready var indicator: Label = $VBox/Indicator


func _ready() -> void:
	hide()


func set_line(event: Dictionary) -> void:
	show()
	# event has: type, text, speaker (or null), presentation, append, ...
	if event.get("append", false):
		var current := text_label.text
		text_label.text = current + " " + str(event.get("text", ""))
	else:
		text_label.text = str(event.get("text", ""))
	var speaker = event.get("speaker", null)
	if speaker == null or (speaker is String and (speaker as String).is_empty()):
		# Narration: italic, no speaker.
		speaker_label.text = ""
		text_label.bbcode_enabled = true
		text_label.text = "[i]" + text_label.text + "[/i]"
	else:
		# Spoken line.
		speaker_label.text = str(speaker) + ":"
		text_label.bbcode_enabled = false
	indicator.text = "▼  (click to continue)"


func set_blank() -> void:
	text_label.text = ""
	speaker_label.text = ""


func _unhandled_input(event: InputEvent) -> void:
	if not visible:
		return
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		advanced.emit()
		get_viewport().set_input_as_handled()
	elif event is InputEventKey and event.pressed and not event.echo:
		if event.keycode in [KEY_SPACE, KEY_ENTER, KEY_KP_ENTER]:
			advanced.emit()
			get_viewport().set_input_as_handled()
