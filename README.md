# Aurum

> A modular game engine foundation built on Godot 4.7 + Rust.

Aurum is one engine for many game genres. You write game logic in GDScript
and the engine layer in Rust. Hot-reload stays fast because GDScript
and scenes are unchanged — only the Rust crate boundary is slower.

[![CI](https://github.com/AG064/aurum-engine/actions/workflows/ci.yml/badge.svg)](https://github.com/AG064/aurum-engine/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Godot](https://img.shields.io/badge/godot-4.7-blue.svg)](https://godotengine.org)

## What you get

- **`aurum-core`** — pure Rust ECS, event bus, typed state with save/load,
  fixed timestep. No Godot dependency; fully tested with `cargo test`.
- **`aurum-godot`** — GDExtension shim. Exposes a single `AurumNode` Node
  class to GDScript with a clean entity/component/event/state API.
- **`aurum-2d`** — 2D components (Position2D, Velocity2D, AABB) +
  AABB collision math + kinematic step. **Working 2D demo.**
- **`aurum-3d`** — 3D components and kinematic step. **Working 3D demo.**
- **`aurum-space`** — fixed-step 6DOF flight, ship resources, and bounded
  universe coordinates. Pure Rust and reusable by space games.
- **`aurum-vn`** — story interpreter with full GDScript shim
  (`Aurum.story_*`). Includes a minimal visual novel demo that ports
  the original `godot/vn/` story format onto the new engine.
- **`aurum-mcp`** — headless MCP server. Lets an AI model drive the engine
  with no Godot process: entities, components, events, state, the space
  simulation, and save/load. Adds no external dependencies.
- **`aurum-vr`** / **`aurum-text`** — stubs for VR and text-only genres.
- **`aurum-cli`** — the `aurum` command line, providing `aurum mcp`.
- **`godot/`** — Godot project with the add-on, a dev console,
  and two tutorial demos:
  - **2D squares** — movement + collision + score
  - **3D bounce** — gravity + jumping

## Quick start

```pwsh
# 1. Build the engine + copy the DLL to the Godot project
pwsh scripts/build.ps1

# 2. Run the 2D demo
pwsh scripts/build.ps1 -Run

# 3. Open the Godot editor
pwsh scripts/build.ps1 -DebugBuild -RunEditor
```

The build script defaults to the Godot project at `./godot/`. To use a
project at a different path, pass `-GodotProject <path>`.

## Repository layout

```
aurum-engine/                  # Cargo workspace root
├── Cargo.toml                 # workspace manifest
├── crates/
│   ├── aurum-core/            # pure Rust engine (tested)
│   ├── aurum-godot/           # GDExtension shim
│   ├── aurum-2d/              # 2D game module
│   ├── aurum-3d/              # 3D game module
│   ├── aurum-space/           # space flight and universe coordinates
│   ├── aurum-vn/              # VN story interpreter
│   ├── aurum-vr/              # VR (stub)
│   ├── aurum-text/            # text-only (stub)
│   └── aurum-cli/             # CLI tools (stub)
├── scripts/
│   ├── build.ps1              # build + copy DLL + (optional) run
│   └── dev.ps1                # self-contained debug watcher
├── .vscode/tasks.json         # VS Code task definitions
├── .github/workflows/ci.yml   # GitHub Actions CI
├── docs/
│   ├── ARCHITECTURE.md
│   ├── MODULES.md
│   └── GETTING_STARTED.md
├── godot/                     # the Godot project (one folder per repo)
│   ├── project.godot
│   ├── addons/aurum/          # the engine add-on (bin/ is built, source is here)
│   ├── scripts/               # shared GDScript (runtime, dev console)
│   ├── templates/             # starter projects per genre
│   └── demos/
│       ├── 2d_squares/        # the 2D tutorial
│       └── 3d_bounce/         # the 3D tutorial
├── CHANGELOG.md
├── CONTRIBUTING.md
└── LICENSE
```

> **Note** — game projects that build on Aurum live in their own
> repositories. The `aurum-vn` module is consumed by
> [`the-regular-novel`](https://github.com/AG064/the-regular-novel);
> the `aurum-2d` / `aurum-3d` / core runtime can be used by any
> 2D or 3D game. See `docs/GETTING_STARTED.md` for the recommended
> project layout.

## How it fits together

```
┌──────────────────────────────────────────────────────────────┐
│ Your game (in a separate repo, sibling to aurum-engine)     │
│ - Scenes, UI, art, audio                                     │
│ - GDScript game logic (hot-reloads in <100ms)                │
└──────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────┐
│ Engine add-on (addons/aurum/, copied into the game repo)    │
│ - The `AurumNode` Node (the only Rust surface to GDScript)      │
│ - The `Aurum` autoload (ergonomic shim around AurumNode)        │
│ - The compiled GDExtension DLL (aurum_godot.dll)            │
└──────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────┐
│ Genre module (aurum-2d, aurum-3d, aurum-vn, ...)             │
│ - Genre-specific components, systems, helpers                │
│ - Optional: a GDScript shim that exposes the module         │
└──────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────┐
│ Engine shim (aurum-godot)                                    │
│ - AurumNode Node class (the only Rust surface to GDScript)      │
│ - JSON-blob component store (GDScript-friendly)             │
│ - Bridges the typed event bus to Godot signals              │
└──────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────┐
│ Engine core (aurum-core)                                     │
│ - ECS, events, state, save/load, time                        │
│ - Pure Rust, no Godot, fully tested                          │
└──────────────────────────────────────────────────────────────┘
```

## Hot-reload story

| Layer                              | Normal feedback | How                                      |
|------------------------------------|-----------------|------------------------------------------|
| GDScript                           | Immediate       | Godot reloads scripts                    |
| `.tscn` scenes and resources       | Immediate       | Godot reloads editor resources           |
| Safe Rust implementation changes   | Debug build     | Reloadable GDExtension, same editor PID  |
| Native Godot API structure changes | Controlled      | Exceptional editor restart               |

Run `pwsh scripts/dev.ps1` for the self-contained debug watcher. It does not
require `cargo-watch`. After a verified debug DLL install, the watcher publishes
its hash and the enabled Aurum editor plugin performs the checked native reload.
A failed build keeps the last working DLL. A rejected native reload warns that a
controlled editor restart may be required. See `docs/HOT_RELOAD.md` for the
tested boundary and live product-path evidence.

## Known issues

**Godot 4.7 exits with an access violation at the end of the first headless
import of any project that loads a GDExtension.** The import itself finishes —
the editor settings are saved before it dies — so the only symptom is a
non-zero exit code from `godot --headless --path . --import`.

It is not this engine's code. It reproduces with a fifteen-line GDExtension
that registers a single empty `Node` subclass, and it does not happen on a
second import, on a windowed import, or in a project with no extension at all.
Running the import twice leaves the second run a clean no-op, which is what
`aurum build` and `scripts/setup.ps1` in a downstream project do.

Reproduction, in a project with one `.gdextension`:

```powershell
Remove-Item -Recurse -Force .godot
godot --headless --path . --import   # import completes; exit code is -1073741819
godot --headless --path . --import   # nothing to do; exit code is 0
```

## Naming

- **Aurum** is Latin for "gold".
- The engine core is "the gold" — the precious, stable thing.
- Genre modules are like alloys — they share the same metal base but
  take different forms for different uses.

## License

MIT.
