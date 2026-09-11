# Changelog

All notable changes to Aurum are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **`aurum-mcp`** — headless Model Context Protocol server. Drives the engine
  over newline-delimited JSON-RPC 2.0 on stdio with no Godot process, exposing
  31 `aurum_*` tools (12 read-only, 19 mutating) across entities, components,
  events, state, time, the space simulation, visual-novel stories, and
  save/load. Adds **no external dependencies**: only `aurum-core`,
  `aurum-space`, `aurum-vn`, and the already-locked `serde` stack.
  - `--read-only` omits and refuses mutating tools.
  - `--root` confines `aurum_save` / `aurum_load` and story files, refusing
    `..` escapes and re-checking existing paths through symlinks.
  - Saves in the same JSON shape as `AurumNode.save_to_json`, so a session
    round-trips between the headless and Godot surfaces. A loaded story travels
    with its own definition, so a save file is self-contained.
  - 90 tests, including full protocol sessions driven in memory.

- **`aurum-core::dynamic`** — `DynamicWorld`, a scriptable string-keyed world
  with JSON-blob components. The generically typed `ecs::World` cannot be
  driven from a save file or an external tool; this is its counterpart, and
  the model the Godot bridge already exposes.

- **`aurum-core::state`** — `State::set_owned` accepts a runtime-owned key.
  Callers receiving keys from outside the binary no longer have to
  `Box::leak` them to satisfy `set`'s `&'static str` bound.

- **`aurum-core::time`** — `FixedTimestep` derives `Debug` and `Clone`.

- **`aurum-vn`** — `ChoiceData` is now re-exported from the crate root.

- **`aurum-cli`** — the `aurum` binary, with `aurum mcp` delegating to
  `aurum-mcp` so the two entry points cannot drift.

### Documentation

- `docs/superpowers/specs/2026-09-11-aurum-mcp-design.md` — the three-layer MCP
  architecture, per-layer dependency accounting, and the phasing.
- `crates/aurum-mcp/README.md` — usage, client configuration, and the tool
  surface.

## [0.1.0] — 2026-07-28

The first public release. Foundation only — no API stability promises yet.

### Added

- **`aurum-core`** — pure Rust engine core.
  - `ecs` module: entities, components, systems, resources.
  - `events` module: typed event bus with subscribe/emit/dispatch.
  - `state` module: typed key-value state with JSON save/load.
  - `time` module: time scale and fixed timestep with anti-spiral cap.
  - `assets` module: stable resource IDs.
  - 15 unit tests, all green.

- **`aurum-godot`** — GDExtension shim exposing a `AurumNode` Node class to
  GDScript. The single Rust surface Godot sees. Includes:
  - Entity spawn / despawn.
  - JSON-blob component store with type-keyed reverse index.
  - Dynamic event bus that fires Godot signals.
  - Typed state (bool / int / float / string) with save/load.
  - Time scale control.
  - Module registration.

- **`aurum-2d`** — 2D game module.
  - `Position2D`, `Velocity2D`, `AABB`, `Sprite`, `Tag` components.
  - `step_kinematics`, `aabb_overlap`, `wrap_position` helpers.
  - 4 unit tests + 1 doc test.

- **`aurum-3d`** — 3D game module.
  - `Position3D`, `Velocity3D` components.
  - `step_kinematics` helper.
  - 1 unit test.

- **`aurum-vn`** — visual novel module.
  - `Story` parser.
  - `Interpreter` with `Event` output (Dialogue / Choice / Quit / Goto /
    Command / Error).
  - Full save/load state.
  - 5 unit tests.
  - GDScript shim exposed via the `Aurum` autoload:
    `Aurum.story_load`, `Aurum.story_advance`, `Aurum.story_pick_choice`,
    `Aurum.story_jump_to`, `Aurum.story_get_variable`,
    `Aurum.story_set_variable`, `Aurum.story_export_state`,
    `Aurum.story_import_state`, `Aurum.story_current_scene`,
    `Aurum.story_current_entry_index`. Events come back as Dictionaries.

- **`aurum-vr`**, **`aurum-text`**, **`aurum-cli`** — stub crates
  reserving the module names and a minimal surface so the workspace
  builds and the module surface is fixed.

- **Godot project** at `godot/`.
  - Add-on `addons/aurum/` with the GDExtension, plugin, and runtime
    autoload (`Aurum`).
  - `aurum_dev_console` (F1 in debug builds).
  - `aurum_2d_kinematics` system.

- **Tutorial demos**:
  - `godot/demos/2d_squares/` — player + coins, score, dev console.
  - `godot/demos/3d_bounce/` — gravity + jumping.

- **Templates** for 2D (full), 3D (stub README), VN (stub README).

- **Build pipeline**:
  - `scripts/build.ps1` (build + copy DLL + optional run).
  - `scripts/dev.ps1` (cargo-watch + auto-rebuild).
  - VS Code tasks in `.vscode/tasks.json`.

- **Documentation**:
  - `README.md`.
  - `docs/ARCHITECTURE.md`.
  - `docs/MODULES.md`.
  - `docs/GETTING_STARTED.md`.
  - `CONTRIBUTING.md`.

- **CI** on GitHub Actions: cargo fmt + clippy + test + release build
  on Linux, Windows, macOS.

### Notes

- Game projects that build on Aurum live in their own repositories:
  - `AG064/the-regular-novel` — visual novel game, consumes `aurum-vn`.
  - `AG064/life_evolution` — GPU life simulation, consumes the core
    runtime + its own GDExtension crate.
  Each game copies the engine add-on (`addons/aurum/`) from this
  repo via its own `build.ps1` and depends on the compiled
  `aurum_godot.dll` produced by `scripts/build.ps1` here.
- Cross-platform builds are configured but only the Windows x86_64
  binary is in the add-on bin/. Linux / macOS binaries are produced
  by the CI on tagged releases.
- The 2D and 3D demos are the only complete demos in this release.
  The VN module has full tests and a GDScript shim but no demo
  inside this repo (the live example is the game project above).
