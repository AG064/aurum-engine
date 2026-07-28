# life_evolution — emergent universe simulation

A real-time 3D simulation where complexity emerges from fundamental
physics. Starting from pre-atomic particle soup, the simulation
evolves through atomic formation, chemistry, molecular complexity,
and — given the right conditions — life itself.

Originally developed as a standalone project; now integrated into
the Aurum engine as a demo (`demos/life_evolution/`).

## Run it

The `run/main_scene` in `aurum/godot/project.godot` defaults to the
2D squares demo. To run this one:

1. Open the project in Godot.
2. Open `demos/life_evolution/scenes/main.tscn`.
3. Press **F5** (or click the run button).
4. Click the **Start** button on the loading screen when it appears.

Or set it as the main scene in `project.godot`:

```
run/main_scene="res://demos/life_evolution/scenes/main.tscn"
```

## Build the GDExtension

The simulation core is a Rust GDExtension. The Aurum build script
compiles it alongside the engine:

```pwsh
pwsh scripts/build.ps1
```

This runs `cargo build --release` in the Aurum workspace (for the
engine) and then in `godot/demos/life_evolution/GDExtension/rust/`
(for the simulation). The DLL ends up at
`GDExtension/rust/target/release/life_evolution.dll` and is loaded
by Godot via the GDExtension manifest at
`GDExtension/life_evolution.gdextension`.

To skip the life_evolution build: `pwsh scripts/build.ps1 -NoLife`.

## Controls

| Key | Action |
|---|---|
| `Right-Click + Drag` | Orbit camera |
| `Middle-Click + Drag` | Pan camera |
| `Mouse Wheel` | Zoom in/out |
| `Space` | Pause / Resume |
| `E` / `Q` | Speed up / Slow down time |
| `F` | Focus on the heaviest object |
| `R` | Reset simulation |
| `Tab` | Toggle UI |
| `F1` | Aurum dev console (added) |

## Architecture

The simulation has its own GDExtension Rust crate at
`GDExtension/rust/`. It depends on `glam`, `rand`, `rayon`,
`parking_lot`, `crossbeam-channel`, `serde`, `log` — the user's
original choice, preserved verbatim.

The crate is intentionally NOT a workspace member of the Aurum
workspace (`aurum/Cargo.toml` excludes it under
`workspace.exclude`). Reasons:

- It has its own `Cargo.lock` and dependency set, and pinning it to
  the Aurum workspace would force shared resolution.
- It builds into a separate `target/` so a Rust rebuild doesn't
  clobber engine build artifacts.
- The gdext version is taken from the master branch on GitHub
  (no specific commit pin), same as the engine crate.

The engine integration is otherwise standard:

- `addons/aurum/` provides the engine GDExtension (Rust crate
  `aurum-godot`).
- `demos/life_evolution/GDExtension/` provides the simulation
  GDExtension (Rust crate `life_evolution`).
- Both DLLs live alongside their `.gdextension` manifests and are
  loaded automatically by Godot.

The simulation exposes the same `SimulationWorld` and `ParticleRenderer`
classes it always did — the GDScript in `scripts/main.gd` references
them as `type="SimulationWorld"` in `main.tscn`.

## What the integration changed

The original project at `C:\Game_Development\life_evolution\` had
its own `project.godot`. The integration into Aurum made these
changes:

1. The Rust crate moved from `GDExtension/rust/` (sibling to its
   own `project.godot`) to the same path inside the Aurum Godot
   project (`demos/life_evolution/GDExtension/rust/`). It still
   builds in its own `target/` directory.
2. The inner `project.godot` is gone — the demo is just a folder
   inside the main Aurum project, not a sub-project.
3. The GDExtension manifest now points at the in-project path
   (`res://demos/life_evolution/GDExtension/rust/target/release/...`).
4. Scene `ext_resource` paths were updated from `res://scripts/...`
   to `res://demos/life_evolution/scripts/...`.
5. The shader path in `gpu_simulation.gd` was updated similarly.
6. The input map (`time_accelerate`, `toggle_pause`, etc.) was
   merged into the main `project.godot` so it works in any demo
   loaded by the project.
7. The renderer was switched from GL Compatibility to **Forward
   Plus** because the particle compute shaders require it. The
   2D and VN demos still work under Forward Plus; only the GL
   Compatibility path is no longer the default. If you need GL
   for some reason, change `renderer/rendering_method` in
   `project.godot` back to `"gl_compatibility"` — but the
   life_evolution simulation won't render correctly.

## Files preserved verbatim

The GDScript in `scripts/` and the Rust code in `GDExtension/rust/`
are unchanged from the user's original. The scripts, shaders, and
Rust modules work the same way as they did before the transfer.
