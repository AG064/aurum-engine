# Changelog

All notable changes to Aurum are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project adheres to [Semantic Versioning](https://semver.org/).

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

- **`aurum-godot`** — GDExtension shim exposing a `Mavis` Node class to
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

- **`aurum-vr`**, **`aurum-text`**, **`aurum-cli`** — stub crates
  reserving the module names and a minimal surface so the workspace
  builds and the module surface is fixed.

- **Godot project** at `godot/aurum/`.
  - Add-on `addons/aurum/` with the GDExtension, plugin, and runtime
    autoload (`Aurum`).
  - `aurum_dev_console` (F1 in debug builds).
  - `aurum_2d_kinematics` system.
  - 2D squares demo (player + coins, score, dev console).

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

- The original `godot/vn/` project (VNEngine) is unchanged. The new
  `aurum-vn` module is a clean re-implementation. Migration of the
  existing VN is a follow-up.
- The 2D squares demo is the only complete demo in this release. The
  3D and VN demos are planned for 0.2.0.
- Cross-platform builds are configured but only the Windows x86_64
  binary is in the add-on bin/. Linux / macOS binaries will be
  produced by the CI on tagged releases.
