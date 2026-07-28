# Getting started with Aurum

This walks you through building the engine, running the demo, and
starting your own project on top of Aurum.

## Prerequisites

- Rust 1.75 or later (`rustup default stable`).
- Godot 4.7 (download from [godotengine.org](https://godotengine.org/download/)).
- PowerShell (Windows).
- `cargo-watch` for the dev script: `cargo install cargo-watch`.

## Clone and build

```pwsh
git clone https://github.com/AG064/aurum-engine.git
cd aurum-engine

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
pwsh scripts/build.ps1 -Run -Editor
```

The editor will open with the project at `godot/`. The `Mavis` class
is available — type `Mavis` in the search box of the Add Node dialog
to see it.

To play the demo, press F5 (or click the play button in the top-right).

## Run the test suite

```pwsh
cargo test --workspace
```

This runs all Rust tests across all crates. As of v0.1.0, that's
~26 tests covering the ECS, event bus, state, time, 2D module, 3D
module, and VN story parser.

## Develop with hot-reload

In one terminal, run the dev script:

```pwsh
pwsh scripts/dev.ps1
```

This watches `crates/` and rebuilds on any change, copying the new
DLL into the Godot project.

In another terminal (or the editor itself), run the demo. GDScript
and scene changes hot-reload in <100ms (built into Godot). Rust
changes require a 5–15s rebuild and the next launch picks them up.

## Create your own game

1. Copy `godot/templates/2d/` (or whichever template fits your
   genre) to a new folder.
2. Open it in the Godot editor (use "Import" from the Project Manager).
3. Add your own scenes, components, and game logic.
4. If you need new components, add them in `aurum-2d` (or whichever
   module) in Rust, and mirror the names in GDScript.

## Common pitfalls

- **"Mavis class not found"** — the GDExtension DLL is not at
  `godot/addons/aurum/bin/aurum_godot.dll`. Re-run
  `pwsh scripts/build.ps1`.
- **"DLL changed on disk"** — Godot locks the DLL while the editor is
  open. Close the editor, run the build, then re-open.
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
