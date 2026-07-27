# 3D template

A minimal starting point for a 3D game using `aurum-3d`.

## Status

Stub. The `aurum-3d` Rust crate is in place with `Position3D`, `Velocity3D`,
and a `step_kinematics` system. The GDScript side has the 2D kinematics
node; you'll want a parallel 3D version that translates positions to
Godot `Node3D` transforms.

To build a 3D starter:

1. Create a `Node3D` scene root.
2. Add a child `AurumEntity3D` (similar to the 2D template, but for Node3D).
3. Add a `Kinematics3D` node that reads `Position3D` and writes back to
   `node.position` for each registered entity.
4. Use the camera, lighting, and physics as normal Godot.

## Component contract

| Component   | Fields                                  |
|-------------|-----------------------------------------|
| Position3D  | `{x: f32, y: f32, z: f32}`              |
| Velocity3D  | `{x: f32, y: f32, z: f32}`              |
| Mesh        | `{path: String}`                        |
| Collider3D  | `{shape: String, radius, w, h, d: f32}` |
