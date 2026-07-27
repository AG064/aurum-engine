# Aurum architecture

This document describes the runtime architecture, the data model, and the
seams between layers. If you want to add a new module or change the
engine, start here.

## The four layers

### 1. Godot 4.7 (binary, untouched)

We use the public Godot binary at `godot/Godot_v4.7-stable_win64.exe`.
We do not modify Godot's C++ source. Everything we build is loaded into
Godot via the GDExtension mechanism.

### 2. Engine core (`aurum-core`)

Pure Rust, no Godot dependency. The whole thing is `cargo test`-able
without launching an editor.

| Module                | What it does                                  |
|-----------------------|-----------------------------------------------|
| `ecs`                | Entities, components, systems, resources.     |
| `events`             | Typed event bus with subscribe/emit/dispatch. |
| `state`              | Key-value state with save/load (JSON).        |
| `time`               | Time scale, fixed timestep.                   |
| `assets`             | Stable resource IDs (path-based).             |

#### ECS

Small hand-rolled ECS:

- Entities are `u64` ids.
- Components are stored in `HashMap<Entity, T>` per type.
- Resources are global state (`HashMap<TypeId, Box<dyn Any>>`).
- Systems are `Box<dyn FnMut(&mut World, f32)>`.

Designed for clarity, not maximum performance. If a project hits the
limits, swap the storage layer for `bevy_ecs` or `hecs` and the rest of
the API stays similar.

#### Event bus

Typed events. Subscribe to a specific type, emit events, dispatch them
in a `dispatch()` call. No reentrancy — events emitted during dispatch
are queued for the next call.

The Rust event bus is bridged to Godot signals in `aurum-godot` so
GDScript can `connect` to it like any other signal.

#### State

A small typed key-value store. Keys are `&'static str`; values are one
of `bool`, `i64`, `f64`, or `String`. The `gdext` shim `Box::leak`s the
GDScript-supplied keys to satisfy the `&'static str` requirement. The
leak is bounded by the number of distinct keys a game uses (small).

The state round-trips through JSON. `state.to_json()` and
`State::from_json()` are the save/load boundary for plain key-value
state. The shim wraps this with `Aurum.save_to_json()` /
`Aurum.load_from_json()` which also includes components and time scale.

#### Time

Two notions: real time (wall clock) and game time (scaled real time).
The GDScript shim exposes a `set_time_scale(scale)` so games can pause
or slow time without freezing the editor.

Fixed timestep is `FixedTimestep`, with a default of 60 Hz and a
per-frame cap of 5 ticks to avoid the "spiral of death" on slow frames.

### 3. GDExtension shim (`aurum-godot`)

The single Rust surface that GDScript sees: a `Mavis` Node class.

#### Why a single class

A single class is easier to use from GDScript (`Aurum.spawn()` reads
better than `Engine.spawn()`). The wrapper `scripts/aurum_runtime.gd`
exposes the API as a singleton autoload, so the call site is always
`Aurum.<method>()`.

#### Dynamic component store

Components are stored as `serde_json::Value` (a JSON blob), keyed by
`type_name: String`. GDScript passes a `Dictionary`; the shim converts
it to JSON for storage and back to a `Dictionary` on read.

This is intentionally simple:

- No registration step — just `Aurum.set_component(e, "Foo", {...})`.
- No type safety — the GDScript side is responsible for shape.
- Trade-off: every read/write parses JSON. Fine for hundreds of
  components; rewrite with typed components if a project needs more.

The Rust side of a module (e.g. `aurum-2d::Position2D`) defines a
strongly-typed version of the same component. Rust systems can iterate
the typed `World` and the JSON-blob `Mavis` independently.

#### Event bridge

`Aurum.emit_event(type_name, data)` queues a `DynamicEvent` in the
typed `EventBus`. `Aurum.dispatch_events()` drains the queue and fires
the `event_received(type_name, data)` Godot signal for each one.

The dance of subscribe-dispatch-unsubscribe is so that the GDScript
side can react to events one at a time, mutating the world between
events. Without it, all events fire in a tight loop before the GDScript
side can react.

#### Save/load

`Aurum.save_to_json()` returns a JSON string with:

- `next_entity_id` (so deserialization doesn't reuse ids)
- `time_scale`
- `state` (the typed state)
- `components` (the dynamic component store, per-entity)

`Aurum.load_from_json(json)` replaces everything. It does not merge.
Games that want to merge should read the JSON, manipulate it, and write
it back.

### 4. Genre modules (the demo has `aurum-2d`)

Each module follows the same pattern:

- Pure Rust crate (`aurum-2d` etc.) with typed components, math, and
  Rust-side systems.
- Optional GDScript shim in `addons/<module>/scripts/` (e.g.
  `aurum_2d_kinematics.gd`) that bridges the typed Rust world to the
  JSON-blob `Mavis` store and to Godot's scene tree.
- A demo and a template under `demos/<module>/` and
  `templates/<module>/`.

Cross-module composition: a 3D game with VN dialogue enables both
`aurum-3d` and `aurum-vn`, calls `Aurum.register_module("3d")` and
`Aurum.register_module("vn")` on startup, and uses both APIs.

## The contract between layers

The key thing to get right is the component name contract: a
component called `"Position2D"` in the Rust crate has the same name in
GDScript, with the same field names, so entities are visible to both
layers.

```rust
// Rust (aurum-2d)
#[derive(Serialize, Deserialize)]
pub struct Position2D {
    pub x: f32,
    pub y: f32,
}
```

```gdscript
# GDScript
Aurum.set_component(entity, "Position2D", {"x": 0, "y": 0})
```

The same name, the same shape. That's the contract.

## What's intentionally not here

- **No serialization of Rust-side `World` components.** The JSON-blob
  store is the save format. If a Rust-side component needs to persist,
  add a JSON `to_json` / `from_json` impl and write a GDScript-facing
  helper.
- **No networking.** Add `aurum-net` as a separate module when needed.
- **No asset pipeline.** Godot's import system handles art/audio. The
  `assets` module in `aurum-core` is a placeholder for resource
  metadata (sizes, hashes) for the dev console.
- **No scripting language (Rhai/Lua).** GDScript is enough for 95% of
  iteration. Add a hot-reloadable scripting layer if a project needs
  sub-50ms reload on game-logic changes.
