# Visual novel demo (minimal)

A minimal visual novel demo built on `aurum-vn`. Loads a story JSON,
advances the engine, displays dialogue and choices. No localization, no
extras, no menu deterioration, no transitions — just dialogue, choices,
and save/load.

## Run it

Set the main scene in `project.godot`:

```
run/main_scene="res://demos/vn_minimal/scenes/main.tscn"
```

Or open the scene in the editor and press F5.

## Controls

- **Click** / **Space** / **Enter** — advance dialogue
- **1** / **2** / **3** — pick a choice
- **F1** — dev console (shows story state, scene, entry, variables)

## The story

`stories/demo.json` is a small story. It has:

- A `start` scene (title card) that gotos to `morning`
- A `morning` scene with narration, dialogue, and a 3-way choice
- Three choice destinations (`coffee`, `phone`, `sleep`), each with its
  own short scene
- An `ending` scene that shows variable state
- A `checked_phone` and `demo_done` variable

The story uses the same JSON format the original VNEngine used. The
exact same demo.json from `godot/vn/stories/demo.json` (the 527-line
one) will also work — just copy it over `stories/demo.json`.

## What's "left out" of the original VN

The original `godot/vn/` had a lot of code that this minimal port
deliberately drops:

- HUD overlay system
- Typewriter effect (text appears character-by-character)
- Background image display
- Character portrait display
- Menu deterioration (the visual corruption effects)
- Localization (`.csv` files + translation import)
- Achievements, gallery, route tracking
- Custom transition shaders
- Audio buses
- Save/load menu UI (the engine has `story_export_state` /
  `story_import_state`; the menu would be on top)

All of these are achievable in Aurum (the primitives are there) — they
just aren't needed for a minimal demo. Add them as you need them.

## API summary

`Aurum.story_*` calls (autoloaded `Aurum` wraps the native Mavis
class):

| Method | Description |
|---|---|
| `story_load(json, start_scene)` | Load a story from JSON. |
| `story_is_loaded()` | True if a story is loaded. |
| `story_advance()` | Get the next event as a Dictionary. |
| `story_pick_choice(index)` | Apply a choice (visible index). |
| `story_jump_to(target)` | Jump to a scene or label. |
| `story_get_variable(key, default)` | Read a story variable. |
| `story_set_variable(key, value)` | Write a story variable. |
| `story_export_state()` | JSON for save files. |
| `story_import_state(json)` | Restore from JSON. |
| `story_current_scene()` | Current scene name. |
| `story_current_entry_index()` | Current entry index. |

Event shape returned by `story_advance()`:

```gdscript
# dialogue line
{
  "type": "dialogue",
  "speaker": "You" or null,
  "text": "...",
  "presentation": "thought" or "narration" or "dialogue",
  "append": false,
  "character": "", "position": "", "background": "",
  "emotion": "", "text_key": "", "speaker_key": ""
}

# choice block
{
  "type": "choice",
  "entry_index": 5,
  "choices": [
    {"text": "...", "goto": "scene_name", "text_key": ""}
  ]
}

# terminal events
{"type": "scene_ended"}
{"type": "quit"}
{"type": "goto", "target": "scene_name"}
{"type": "command", "command": {...}}
{"type": "error", "message": "..."}
```
