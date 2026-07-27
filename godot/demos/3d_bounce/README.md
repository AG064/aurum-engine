# 3D bounce demo

A minimal 3D demo using `aurum-3d`. The player is a cube that moves
on the XZ plane, the target is a cube that floats in the air and
bobs up and down. When the player gets close to the target, score
goes up and the target respawns at a new random location.

## Run it

1. Set this scene as the main scene in `project.godot`:
   ```
   run/main_scene="res://demos/3d_bounce/scenes/main.tscn"
   ```
2. Or just open the scene in the editor and press F5.

## Controls

- **WASD** or **arrow keys** — move
- **Space** — jump
- **F1** — dev console

## How it works

- `aurum_3d_kinematics.gd` reads `Velocity3D` components, integrates
  position over time (with gravity), and writes back to the scene
  nodes.
- `aurum_entity_3d.gd` is the 3D base class — spawns an entity in
  the engine and syncs position.
- `main_3d.gd` does collision detection (sphere-sphere) and handles
  scoring.

The Rust side (`aurum-3d` crate) has the typed `Position3D` and
`Velocity3D` components. The GDScript side uses the same component
names with the same field shapes, so entities are visible to both
layers.
