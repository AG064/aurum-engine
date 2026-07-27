# 2D squares demo

A simple 2D arcade-style demo. The player square (blue) moves
with arrow keys or WASD. Coin squares (yellow) drift across the
screen. Touch a coin to collect it; the score goes up. Press F1
for the dev console. Press R after a game over to reset.

## Run it

1. Set this scene as the main scene in `project.godot`:
   ```
   run/main_scene="res://demos/2d_squares/scenes/main.tscn"
   ```
2. Or just open the scene in the editor and press F5.

## How it works

- `aurum_2d_kinematics.gd` reads `Velocity2D` components, integrates
  position over time, and writes back to the scene nodes.
- `square.gd` is the base entity class — spawns an entity in the
  engine, syncs position.
- `player.gd` and `coin.gd` are the two entity subclasses.
- `main.gd` does collision detection (AABB), handles scoring, and
  respawns coins.

The Rust side (`aurum-2d` crate) has the typed `Position2D`,
`Velocity2D`, and `AABB` components. The GDScript side uses the
same component names with the same field shapes.
