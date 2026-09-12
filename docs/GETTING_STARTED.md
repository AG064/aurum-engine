# Getting started with Aurum

This walks you through building the engine, running the demo, and
starting your own project on top of Aurum.

## Prerequisites

- Rust 1.75 or later (`rustup default stable`).
- Godot 4.7 (download from [godotengine.org](https://godotengine.org/download/)).
- PowerShell (Windows).

## Clone and build

```pwsh
git clone https://github.com/AG064/aurum-studio.git
cd aurum-studio

# Build the engine and copy the DLL into the Godot project
pwsh scripts/build.ps1

# Run the 2D squares demo
pwsh scripts/build.ps1 -Run
```

You should see a player square (blue) that you can move with arrow
keys or WASD. Coin squares (yellow) drift across the screen. Touch a
coin to collect it; your score goes up. Press F1 for the dev console.
Press R after a game over to reset.

The build script defaults to the Godot project at `./godot/`. To use a
Godot binary at a non-default path, pass `-GodotBinary <path>`.

## Open the project in the Godot editor

```pwsh
pwsh scripts/build.ps1 -DebugBuild -RunEditor
```

The editor will open with the project at `godot/`. The `AurumNode` class
is available — type `AurumNode` in the search box of the Add Node dialog
to see it.

To play the demo, press F5 (or click the play button in the top-right).

## Run the test suite

```pwsh
cargo test --workspace
```

This runs all Rust tests across all crates. As of v0.1.0, that's
~26 tests covering the ECS, event bus, state, time, 2D module, 3D
module, and VN story parser.

## Develop without routine editor restarts

Run the self-contained watcher:

```pwsh
pwsh scripts/dev.ps1 -RunEditor
```

The watcher uses debug builds and installs `aurum_godot.debug.dll`. GDScript,
scene, resource, shader, and safe Rust implementation changes keep the editor
process alive. A running game may restart independently.

Each verified debug install publishes its DLL hash under `.godot/aurum/`. The
enabled Aurum editor plugin observes that marker, requires one loaded Aurum
manifest, and accepts the native reload only when Godot returns `OK`.

Native class registration, inheritance, exported method or signal signatures,
entry symbols, and Godot API-version changes may require a controlled editor
restart. See `docs/HOT_RELOAD.md` for the exact tested boundary.

## Create your own game

1. Copy `godot/templates/2d/` (or whichever template fits your
   genre) to a new folder.
2. Open it in the Godot editor (use "Import" from the Project Manager).
3. Add your own scenes, components, and game logic.
4. If you need new components, add them in `aurum-2d` (or whichever
   module) in Rust, and mirror the names in GDScript.

## Common pitfalls

- **"AurumNode class not found"**: run `pwsh scripts/dev.ps1 -Once` and verify
  that `addons/aurum/bin/aurum_godot.debug.dll` exists.
- **DLL installation failed before commit**: the last working DLL remains
  installed. Close programs that independently locked the staged file, then let
  the watcher retry.
- **Native reload warning**: save editor work and use the controlled restart
  path. The plugin suppresses repeated attempts for the same failed DLL hash.
- **Native structure changed**: save editor work and use the controlled restart
  path. Do not treat this exceptional case as the normal development loop.
- **Component shape mismatches** — if Rust expects `{x, y, z}` and
  GDScript passes `{x, y}`, the JSON conversion silently drops fields.
  Always match field names exactly.

## Where to go from here

- `ARCHITECTURE.md` — the design, the data model, the seams.
- `MODULES.md` — what's in each module, how to add a new one.
- `crates/aurum-2d/src/lib.rs` — read the doc comments for the 2D
  component contract.
- `crates/aurum-godot/src/lib.rs` — read the GDScript-facing API.
- `godot/demos/2d_squares/scripts/main.gd` — read the demo for
  a working example.
