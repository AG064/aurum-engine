# VN template

A minimal starting point for a visual novel using `aurum-vn`.

## Status

Stub. The `aurum-vn` Rust crate has the full story interpreter (parser,
condition, branches, choices, save state). The GDScript shim is not yet
written.

To build a VN starter:

1. Wait for the `aurum-vn` GDScript shim (planned). It will expose:
   ```gdscript
   Aurum.story.load("res://stories/main.json")
   Aurum.story.start("intro")
   Aurum.story.advance()  # returns a Dialogue / Choice / Goto event
   Aurum.story.pick_choice(0)
   Aurum.story.save()
   Aurum.story.load(json)
   ```
2. Build a HUD that displays dialogue and a choices panel.
3. Write your story as JSON (see `stories/example.json` in the existing
   `godot/vn/` project for the format).

## The original VN engine

`godot/vn/` (in the workspace root) is the original visual novel engine
that this template will eventually replace. It works today and is the
source of truth for the story format. The new template will be a drop-in
replacement once the GDScript shim lands.
