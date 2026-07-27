# 2D template

A minimal starting point for a 2D game using `aurum-2d`.

## What's here

- `scenes/main.tscn` — root scene with a player and one coin, using `aurum-2d` components
- `scripts/player.gd` — Player entity (arrow keys / WASD)
- `scripts/coin.gd` — Coin entity (moves and wraps)
- `scripts/main.gd` — Game logic, collision detection, score

## Component contract

This template uses these components on the GDScript side; the same names
exist in the `aurum-2d` Rust crate so entities are visible to both layers.

| Component   | Fields                              |
|-------------|-------------------------------------|
| Position2D  | `{x: f32, y: f32}`                  |
| Velocity2D  | `{x: f32, y: f32}`                  |
| AABB        | `{x: f32, y: f32, w: f32, h: f32}`  |
| Sprite      | `{path: String, modulate: String}`  |
| Tag         | `{name: String}`                    |

## Run it

```pwsh
pwsh ../../aurum/scripts/build.ps1 -Run
```

## Extend it

1. Add new components in `aurum-2d` (Rust) and mirror their names in GDScript.
2. Add new entity scenes (e.g. `enemy.tscn`) that extend the same pattern.
3. Add new systems in `aurum_2d_kinematics.gd` or as new nodes in the scene tree.
4. Save/load via `Aurum.save_to_json()` / `Aurum.load_from_json()`.
