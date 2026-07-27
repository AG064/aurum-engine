//! Aurum 3D module.
//!
//! Provides typed 3D components and helpers. Mirror of `aurum-2d` for the
//! 3D world: positions, velocities, AABBs, kinematic step, sphere-AABB
//! intersection. Add more (rotations, frustum culling, navmesh) when a
//! project needs them.
//!
//! ## Component names (the contract with GDScript)
//!
//! - `"Position3D"`  `{ "x": f32, "y": f32, "z": f32 }`
//! - `"Velocity3D"`  `{ "x": f32, "y": f32, "z": f32 }`
//! - `"Mesh"`        `{ "path": String }`
//! - `"Collider3D"`  `{ "shape": String, "radius": f32, "w": f32, "h": f32, "d": f32 }`

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Velocity3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Velocity3D {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

/// Step the kinematic system. Mirrors `aurum_2d::step_kinematics`.
pub fn step_kinematics(world: &mut aurum_core::ecs::World, dt: f32) {
    let velocities: Vec<(aurum_core::ecs::Entity, f32, f32, f32)> = world
        .iter::<Velocity3D>()
        .map(|(e, v)| (e, v.x, v.y, v.z))
        .collect();
    for (entity, vx, vy, vz) in velocities {
        if let Some(pos) = world.get_mut::<Position3D>(entity) {
            pos.x += vx * dt;
            pos.y += vy * dt;
            pos.z += vz * dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurum_core::ecs::World;

    #[test]
    fn kinematics_step() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, Position3D::zero());
        world.insert(e, Velocity3D::new(0.0, 9.8, 0.0));
        step_kinematics(&mut world, 1.0);
        let pos = world.get::<Position3D>(e).unwrap();
        assert!((pos.y - 9.8).abs() < 1e-4);
    }
}
