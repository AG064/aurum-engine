extends HSlider
## Time scale slider for the simulation.
##
## Maps slider positions to the time-scale table used by main.gd.

signal time_scale_changed(scale: float)

const WARNING_INDEX: int = 3
const CRITICAL_INDEX: int = 5
@onready var warning_label: Label = get_node_or_null("../TimeWarning") as Label

## Time scales corresponding to slider positions
const TIME_SCALES: Array[float] = [
	1.0,      # Real time
	10.0,     # 10x
	100.0,    # 100x
	1000.0,   # 1,000x
	10000.0,  # 10,000x
	100000.0, # 100,000x
	1e6,      # Million
	1e7,      # 10 million
	1e8,      # 100 million
	1e9,      # Billion
	1e10,     # 10 billion
	1e12,     # Trillion
]

func _ready() -> void:
	min_value = 0
	max_value = TIME_SCALES.size() - 1
	step = 1
	value = 0  # Start at 1x (real time)
	value_changed.connect(_on_value_changed)
	_update_warning_state(0)

func _on_value_changed(new_value: float) -> void:
	var index := int(new_value)
	if index >= 0 and index < TIME_SCALES.size():
		var selected_time_scale: float = TIME_SCALES[index]
		_update_warning_state(index)
		time_scale_changed.emit(selected_time_scale)

func sync_index(index: int) -> void:
	var clamped_index: int = clampi(index, 0, TIME_SCALES.size() - 1)
	set_value_no_signal(clamped_index)
	_update_warning_state(clamped_index)

func _update_warning_state(index: int) -> void:
	var high_speed: bool = index >= WARNING_INDEX
	var critical_speed: bool = index >= CRITICAL_INDEX
	if warning_label != null:
		warning_label.visible = high_speed
		warning_label.text = "WARNING: high speed may reduce physics accuracy" if not critical_speed else "WARNING: extreme speed may skip interactions"
		warning_label.modulate = Color(1.0, 0.35, 0.12, 1.0) if critical_speed else Color(1.0, 0.72, 0.2, 1.0)
	tooltip_text = "High speed can reduce collision and merge accuracy" if high_speed else "Simulation speed"
	queue_redraw()

func _draw() -> void:
	var index: int = int(value)
	if index < WARNING_INDEX:
		return
	var outline_color: Color = Color(1.0, 0.28, 0.08, 0.95) if index >= CRITICAL_INDEX else Color(1.0, 0.65, 0.12, 0.95)
	draw_rect(Rect2(1.0, 1.0, size.x - 2.0, size.y - 2.0), outline_color, false, 2.0)

func get_time_scale() -> float:
	var index := int(value)
	if index >= 0 and index < TIME_SCALES.size():
		return TIME_SCALES[index]
	return 1.0
