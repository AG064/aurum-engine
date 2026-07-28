# Aurum

> A modular game engine foundation built on Godot 4.7 + Rust.

Aurum is one engine for many game genres. You write game logic in GDScript
and the engine layer in Rust. Hot-reload stays fast because GDScript
and scenes are unchanged — only the Rust crate boundary is slower.

[![CI](https://github.com/yourname/aurum/actions/workflows/ci.yml/badge.svg)](https://github.com/yourname/aurum/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Godot](https://img.shields.io/badge/godot-4.7-blue.svg)](https://godotengine.org)

## What you get

- **`aurum-core`** — pure Rust ECS, event bus, typed state with save/load,
  fixed timestep. No Godot dependency; fully tested with `cargo test`.
- **`aurum-godot`** — GDExtension shim. Exposes a single `Mavis` Node
  class to GDScript with a clean entity/component/event/state API.
- **`aurum-2d`** — 2D components (Position2D, Velocity2D, AABB) +
  AABB collision math + kinematic step. **Working 2D demo.**
- **`aurum-3d`** — 3D components and kinematic step. **Working 3D demo.**
- **`aurum-vn`** — story interpreter with full GDScript shim
  (`Aurum.story_*`). Includes a minimal visual novel demo that ports
  the original `godot/vn/` story format onto the new engine.
- **`aurum-vr`** / **`aurum-text`** / **`aurum-cli`** — stubs for
  VR, text-only, and CLI tool genres.
- **`godot/aurum/`** — Godot project with the add-on, a dev console,
  and four demos:
  - **2D squares** — movement + collision + score
  - **3D bounce** — gravity + jumping
  - **VN minimal** — dialogue + choices, full `aurum-vn` shim
  - **life_evolution** — emergent-universe GPU simulation
    (full GDExtension integration, 50k+ particles, compute shaders)

## Quick start

```pwsh
# 1. Build the engine + copy the DLL to the Godot project
pwsh scripts/build.ps1

# 2. Run the 2D demo
pwsh scripts/build.ps1 -Run

# 3. Open the Godot editor
pwsh scripts/build.ps1 -Run -Editor
```

The build script defaults to the Godot project at `./godot/`. To use a
project at a different path, pass `-GodotProject <path>`.

## Repository layout

```
aurum/                         # Cargo workspace root
├── Cargo.toml                 # workspace manifest
├── crates/
│   ├── aurum-core/            # pure Rust engine (tested)
│   ├── aurum-godot/           # GDExtension shim
│   ├── aurum-2d/              # 2D game module
│   ├── aurum-3d/              # 3D game module
│   ├── aurum-vn/              # VN story interpreter
│   ├── aurum-vr/              # VR (stub)
│   ├── aurum-text/            # text-only (stub)
│   └── aurum-cli/             # CLI tools (stub)
├── scripts/
│   ├── build.ps1              # build + copy DLL + (optional) run
│   └── dev.ps1                # cargo-watch + auto-rebuild
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
│       ├── 2d_squares/        # the 2D demo
│       ├── 3d_bounce/         # the 3D demo
│       ├── vn_minimal/        # the visual novel demo
│       └── life_evolution/    # the emergent-universe GPU simulation
│                               # (own GDExtension Rust crate; built by
│                               # the same `scripts/build.ps1`)
├── CHANGELOG.md
├── CONTRIBUTING.md
└── LICENSE
```

## How it fits together

```
┌──────────────────────────────────────────────────────────────┐
│ Your game (in godot/aurum/demos/<your_game>)                 │
│ - Scenes, UI, art, audio                                     │
│ - GDScript game logic (hot-reloads in <100ms)                │
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
│ - Mavis Node class (the only Rust surface to GDScript)      │
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

| Layer                              | Reload time | How                         |
|------------------------------------|-------------|-----------------------------|
| GDScript                          | <100ms      | Godot already does this     |
| `.tscn` scenes                    | <100ms      | Godot already does this     |
| Rust engine (`aurum-godot`)       | 5–15s       | `cargo build`, next launch  |
| Rust modules (optional in editor) | 5–15s       | Same                        |

For 95% of iteration (gameplay tweaks, UI changes, scene layout) you
stay in the Godot editor with sub-100ms feedback. Rust rebuilds are
rare because engine code stabilizes after the first version.

## Naming

- **Aurum** is Latin for "gold".
- The engine core is "the gold" — the precious, stable thing.
- Genre modules are like alloys — they share the same metal base but
  take different forms for different uses.

## License

MIT.
