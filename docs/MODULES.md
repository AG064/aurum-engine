# Modules

Each module is a Cargo crate and (when needed) a Godot add-on piece.
The Rust crate is the source of truth for component types and math.
The GDScript side mirrors the component names so the same entity is
visible from both layers.

## `aurum-core` — engine core

Pure Rust, no Godot. ECS, events, state, save/load, time. Always
compiled in.

```rust
use aurum_core::prelude::*;

let mut world = World::new();
let e = world.spawn();
world.insert(e, MyComponent { ... });
world.add_system(|w, dt| { /* ... */ });
world.tick(1.0 / 60.0);
```

## `aurum-godot` — GDExtension shim

The only Rust code Godot sees. Exposes a single `Mavis` Node class.
Always compiled in (it's the bridge to the engine).

GDScript API is documented at the top of `crates/aurum-godot/src/lib.rs`.
The wrapper `scripts/aurum_runtime.gd` (autoloaded as `Aurum`) is what
GDScript code should call.

## `aurum-2d` — 2D games

Components: `Position2D`, `Velocity2D`, `AABB`, `Sprite`, `Tag`.

Helpers:

- `step_kinematics(world, dt)` — integrate position from velocity
- `aabb_overlap(a, b)` — AABB collision test
- `wrap_position(pos, w, h)` — arcade-style wrap

GDScript shim: `scripts/aurum_2d_kinematics.gd` runs the kinematics
system on every frame and mirrors engine positions to scene nodes.

## `aurum-3d` — 3D games

Components: `Position3D`, `Velocity3D`, `Mesh`, `Collider3D`.

Helpers:

- `step_kinematics(world, dt)` — same as 2D, but for 3D

GDScript shim: not yet written. Plan: a `Node3D` analogue of the 2D
entity, a 3D kinematics node, and a camera controller.

## `aurum-vn` — visual novels

The full VN module: Rust interpreter + GDScript shim + a working demo.

Rust side (in `aurum-vn`):

- `Story` — parsed story with scenes, variables, entries.
- `Interpreter` — advances a cursor, emitting `Event`s
  (Dialogue, Choice, Quit, Goto, Command, Error).
- `Event` and `ChoiceData` — typed payloads.

GDScript shim (exposed via the `Mavis` class, wrapped by `Aurum`):

- `Aurum.story_load(json, start_scene)` — load and start.
- `Aurum.story_advance()` — get the next event as a Dictionary.
- `Aurum.story_pick_choice(i)` — apply a choice (visible index).
- `Aurum.story_jump_to(target)` — jump to a scene or label.
- `Aurum.story_get_variable(key, default)` / `set_variable(key, value)`.
- `Aurum.story_export_state()` / `import_state(json)` — save/load.
- `Aurum.story_current_scene()` / `current_entry_index()` — cursor.

A working minimal demo is at `godot/aurum/demos/vn_minimal/`. It shows
the API surface, supports a 3-way choice, and demonstrates variable
state. The demo's `stories/demo.json` is the story format the original
`godot/vn/` engine used — the same format is supported, so existing
stories can be copied over.

## `aurum-vr` — XR/VR (stub)

The shape will be:

- `XrRig` resource: head, two hands, playspace origin.
- `Comfort` resource: vignette, snap turn, teleport.
- `Hand` / `Controller` components.
- An `OpenXR` shim that wires into Godot's `XRInterface`.

Nothing implemented yet. The Godot 4.7 `XRInterface` system already
gives a lot; the module is mostly about ergonomics.

## `aurum-text` — text-only (stub)

No rendering. The engine core (ECS, state, save/load) drives a
`Printer` resource and an `InputLine` event. Useful for:

- Text adventures.
- AI training environments.
- Headless server-side simulations.

## `aurum-cli` — CLI tools (stub)

Command-line argument parsing, subcommand dispatch via the engine's
event bus, structured output (JSON lines, table). Useful for tools,
asset processors, and server-side use cases.

## Adding a new module

1. Add a crate at `crates/aurum-<name>/`.
2. Add it to the workspace `Cargo.toml`.
3. Define your components (with `Serialize`/`Deserialize` if they
   should survive save/load).
4. Add a GDScript shim in `addons/aurum/scripts/aurum_<name>.gd` if
   you want GDScript access.
5. Register the module on startup: `Aurum.register_module("<name>")`.
6. Add a demo under `demos/<name>/`.
7. Add a template under `templates/<name>/`.

The contract is: a component called `"Foo"` in Rust has the same name
in GDScript, with the same field names, in the same order (or at least
in the same shape).
