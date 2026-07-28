extends Control

# Main controller for the minimal VN demo. Loads the story, drives
# the dialogue box and choice menu, and follows engine events.

@onready var scene_label: Label = $HUD/SceneLabel
@onready var var_label: Label = $HUD/VarLabel
@onready var dialogue_box: Control = $DialogueBox
@onready var choice_menu: VBoxContainer = $ChoiceMenu
@onready var end_label: Label = $EndLabel


func _ready() -> void:
	Aurum.register_module("vn")
	dialogue_box.advanced.connect(_on_advance_clicked)
	choice_menu.choice_picked.connect(_on_choice_picked)
	end_label.hide()
	_load_and_start()


func _process(_delta: float) -> void:
	# Reflect story state in the HUD.
	scene_label.text = "Scene: %s  /  Entry: %d" % [
		Aurum.story_current_scene(),
		Aurum.story_current_entry_index(),
	]
	var checked: Variant = Aurum.story_get_variable("checked_phone", false)
	var done: Variant = Aurum.story_get_variable("demo_done", false)
	var_label.text = "checked_phone = %s   demo_done = %s" % [
		str(checked), str(done)
	]


func _load_and_start() -> void:
	var story_path := "res://demos/vn_minimal/stories/demo.json"
	if not FileAccess.file_exists(story_path):
		push_error("Story not found: " + story_path)
		return
	var file: FileAccess = FileAccess.open(story_path, FileAccess.READ)
	var json_text := file.get_as_text()
	file.close()
	var err := Aurum.story_load(json_text, "start")
	if not err.is_empty():
		push_error("Story load failed: " + err)
		return
	# Start the engine by advancing once.
	_step()


# --- engine step driver ---

func _step() -> void:
	if not Aurum.story_is_loaded():
		return
	var event: Dictionary = Aurum.story_advance()
	_dispatch(event)


func _on_advance_clicked() -> void:
	_step()


func _on_choice_picked(index: int) -> void:
	var err := Aurum.story_pick_choice(index)
	if not err.is_empty():
		push_warning("Choice failed: " + err)
	# The engine will return a goto event next; consume it.
	_step()


func _dispatch(event: Dictionary) -> void:
	var t: String = event.get("type", "")
	match t:
		"dialogue":
			choice_menu.hide()
			dialogue_box.set_line(event)
		"choice":
			dialogue_box.set_line({})
			dialogue_box.set_blank()
			choice_menu.set_choices(event.get("choices", []))
		"goto":
			# Follow the goto by advancing again.
			_step()
		"scene_ended":
			end_label.show()
			end_label.text = "— end of story —\n\nThe story ended. Press ESC to quit."
			dialogue_box.hide()
			choice_menu.hide()
		"quit":
			get_tree().quit()
		"command":
			# Generic story command. Ignore for this minimal demo.
			_step()
		"error":
			push_error("Story error: " + str(event.get("message", "?")))
		_:
			# Unknown event — try advancing.
			_step()
