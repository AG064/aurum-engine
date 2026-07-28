extends Control
## Loading screen with animated progress bar and click-to-start.
##
## Simple implementation: animates the progress bar over MIN_DISPLAY_TIME
## and shows "Click anywhere to start" when complete.

@onready var progress_bar: ProgressBar = $"../LoadingBG/VBox/ProgressBar"
@onready var status_label: Label = $"../LoadingBG/VBox/StatusLabel"
@onready var title_label: Label = $"../LoadingBG/VBox/TitleLabel"
@onready var loading_bg: ColorRect = $"../LoadingBG"

const FADE_DURATION: float = 0.4
const MIN_DISPLAY_TIME: float = 1.5

var loading_done: bool = false
var _dismissed: bool = false
var _start_time_msec: int = 0
var _display_tween: Tween = null

signal user_clicked_start()

func _ready() -> void:
	progress_bar.value = 0
	_set_status("Initializing simulation engine...")
	_set_title("LIFE EVOLUTION")

func show_loading() -> void:
	modulate = Color(1, 1, 1, 1)
	visible = true
	loading_bg.visible = true
	loading_bg.modulate = Color(1, 1, 1, 1)
	progress_bar.visible = true
	progress_bar.value = 0
	loading_done = false
	_dismissed = false
	_start_time_msec = Time.get_ticks_msec()
	_set_status("Initializing simulation engine...")

	# Animate the progress bar smoothly over MIN_DISPLAY_TIME.
	# Using a single value_tween that finishes after MIN_DISPLAY_TIME.
	if _display_tween and _display_tween.is_valid():
		_display_tween.kill()
	_display_tween = create_tween()
	_display_tween.tween_property(progress_bar, "value", 100.0, MIN_DISPLAY_TIME)
	_display_tween.finished.connect(_on_display_tween_finished)

func _on_display_tween_finished() -> void:
	if _dismissed:
		return
	# Mark as ready for click — don't block on anything
	loading_done = true
	progress_bar.value = 100.0
	_set_status("[ Click anywhere to start ]")
	_pulse_prompt()

## Called from main.gd to push the real progress value (0-100).
## We use this to update the status text but the bar is already animating.
func update_progress(progress: int) -> void:
	if loading_done:
		return
	match progress:
		5:
			_set_status("Allocating spatial grid...")
		30:
			_set_status("Creating particle soup...")
		60:
			_set_status("Building spatial index...")
		80:
			_set_status("Warming up rendering buffers...")
		95:
			_set_status("Finalizing setup...")

func _pulse_prompt() -> void:
	if _dismissed:
		return
	var pulse := create_tween()
	pulse.set_loops()
	pulse.tween_property(status_label, "modulate:a", 0.3, 0.8)
	pulse.tween_property(status_label, "modulate:a", 1.0, 0.8)

## Catches all input on the LoadingLayer so clicks anywhere dismiss the screen.
func _input(event: InputEvent) -> void:
	if not loading_done or _dismissed:
		return
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.pressed and mb.button_index == MOUSE_BUTTON_LEFT:
			_dismiss()
	elif event is InputEventKey:
		var k := event as InputEventKey
		if k.pressed:
			_dismiss()

func _dismiss() -> void:
	if _dismissed:
		return
	_dismissed = true
	loading_done = false
	user_clicked_start.emit()

	# Fade out the loading UI
	var tween := create_tween()
	tween.set_parallel(true)
	tween.tween_property(loading_bg, "modulate:a", 0.0, FADE_DURATION)
	tween.tween_property(self, "modulate:a", 0.0, FADE_DURATION)
	# Don't await — let it run in background
	tween.finished.connect(_on_dismiss_finished)

func _on_dismiss_finished() -> void:
	visible = false
	loading_bg.visible = false

## Public method to programmatically hide the loading screen.
func hide_loading() -> void:
	_dismiss()

func _set_status(text: String) -> void:
	if status_label:
		status_label.text = text

func _set_title(text: String) -> void:
	if title_label:
		title_label.text = text
