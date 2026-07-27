//! Aurum 2D module.
//!
//! Provides typed 2D components, AABB collision math, and a kinematic
//! step system. GDScript uses the same component names via the JSON-blob
//! store, so entities are visible to both layers.
//!
//! ## Component names (the contract with GDScript)
//!
//! - `"Position2D"`  `{ "x": f32, "y": f32 }`
//! - `"Velocity2D"`  `{ "x": f32, "y": f32 }`
//! - `"AABB"`        `{ "x": f32, "y": f32, "w": f32, "h": f32 }`
//! - `"Sprite"`      `{ "path": String, "modulate": String }`
//! - `"Tag"`         `{ "name": String }`
//!
//! ## Quick example (Rust-side)
//!
//! ```rust
//! use aurum_core::prelude::*;
//! use aurum_2d::{Position2D, Velocity2D, step_kinematics, aabb_overlap};
//!
//! let mut world = World::new();
//! let e = world.spawn();
//! world.insert(e, Position2D { x: 0.0, y: 0.0 });
//! world.insert(e, Velocity2D { x: 10.0, y: 0.0 });
//! step_kinematics(&mut world, 1.0 / 60.0);
//! assert_eq!(world.get::<Position2D>(e).unwrap().x, 10.0 / 60.0);
//! ```

use serde::{Deserialize, Serialize};

/// Position in 2D world space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position2D {
    pub x: f32,
    pub y: f32,
}

impl Position2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// Velocity in 2D world space (units per second).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Velocity2D {
    pub x: f32,
    pub y: f32,
}

impl Velocity2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// Axis-aligned bounding box, used for collision.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AABB {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl AABB {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn min_x(&self) -> f32 {
        self.x
    }
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }
    pub fn min_y(&self) -> f32 {
        self.y
    }
    pub fn max_y(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// AABB overlap test. Touching edges count as overlap (`<` not `<=`).
pub fn aabb_overlap(a: &AABB, b: &AABB) -> bool {
    a.min_x() < b.max_x() && a.max_x() > b.min_x() && a.min_y() < b.max_y() && a.max_y() > b.min_y()
}

/// Step the kinematic system: integrate position from velocity for every
/// entity that has both `Position2D` and `Velocity2D` components.
pub fn step_kinematics(world: &mut aurum_core::ecs::World, dt: f32) {
    // Collect velocity data first to avoid borrow issues.
    let velocities: Vec<(aurum_core::ecs::Entity, f32, f32)> = world
        .iter::<Velocity2D>()
        .map(|(e, v)| (e, v.x, v.y))
        .collect();
    for (entity, vx, vy) in velocities {
        if let Some(pos) = world.get_mut::<Position2D>(entity) {
            pos.x += vx * dt;
            pos.y += vy * dt;
        }
    }
}

/// Wrap a position around the given bounds. Useful for arcade games.
pub fn wrap_position(pos: &mut Position2D, width: f32, height: f32) {
    if pos.x < 0.0 {
        pos.x += width;
    } else if pos.x > width {
        pos.x -= width;
    }
    if pos.y < 0.0 {
        pos.y += height;
    } else if pos.y > height {
        pos.y -= height;
    }
}

/// Sprite component (path + optional color).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprite {
    pub path: String,
    pub modulate: String,
}

impl Sprite {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            modulate: "#ffffff".to_string(),
        }
    }
}

/// Tag component — useful for filtering ("Player", "Enemy", "Coin").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
}

impl Tag {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurum_core::ecs::World;

    #[test]
    fn aabb_overlap_basic() {
        let a = AABB::new(0.0, 0.0, 10.0, 10.0);
        let b = AABB::new(5.0, 5.0, 10.0, 10.0);
        let c = AABB::new(20.0, 20.0, 5.0, 5.0);
        assert!(aabb_overlap(&a, &b));
        assert!(!aabb_overlap(&a, &c));
    }

    #[test]
    fn aabb_touching_is_not_overlap() {
        let a = AABB::new(0.0, 0.0, 10.0, 10.0);
        let b = AABB::new(10.0, 0.0, 10.0, 10.0);
        assert!(!aabb_overlap(&a, &b));
    }

    #[test]
    fn kinematics_step() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, Position2D::new(0.0, 0.0));
        world.insert(e, Velocity2D::new(60.0, 30.0));
        step_kinematics(&mut world, 0.5);
        let pos = world.get::<Position2D>(e).unwrap();
        assert_eq!(pos.x, 30.0);
        assert_eq!(pos.y, 15.0);
    }

    #[test]
    fn wrap_position_works() {
        let mut p = Position2D::new(-1.0, 5.0);
        wrap_position(&mut p, 10.0, 10.0);
        assert_eq!(p.x, 9.0);
        assert_eq!(p.y, 5.0);
    }
}
