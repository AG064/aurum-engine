//! Spatial partitioning for efficient neighbor queries.
//!
//! Uses a spatial hash grid for O(1) cell lookups, enabling efficient
//! collision detection and force calculation between nearby particles.

use glam::Vec3;
use std::collections::HashMap;

use super::particles::Particle;

/// A spatial hash grid for efficient spatial queries.
pub struct SpatialHashGrid {
    /// Cell size
    pub cell_size: f64,
    /// Map from cell coordinates to particle indices
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
}

impl SpatialHashGrid {
    /// Create a new spatial hash grid with the given cell size.
    pub fn new(cell_size: f64) -> Self {
        Self {
            cell_size,
            cells: HashMap::new(),
        }
    }

    /// Rebuild the spatial index from the given particles.
    pub fn rebuild(&mut self, particles: &[Particle]) {
        self.cells.clear();

        for (i, p) in particles.iter().enumerate() {
            let cell = self.cell_coords(p.position);
            self.cells.entry(cell).or_default().push(i);
        }
    }

    /// Get the cell coordinates for a position.
    pub fn cell_coords(&self, pos: Vec3) -> (i32, i32, i32) {
        let cs = self.cell_size as f32;
        (
            (pos.x / cs).floor() as i32,
            (pos.y / cs).floor() as i32,
            (pos.z / cs).floor() as i32,
        )
    }

    /// Get all particle indices in the same cell as the given position.
    pub fn get_cell(&self, pos: Vec3) -> Option<&Vec<usize>> {
        self.cells.get(&self.cell_coords(pos))
    }

    /// Get all particle indices within a radius of the given position.
    pub fn get_neighbors(&self, pos: Vec3, radius: f64) -> Vec<(i32, i32, i32)> {
        let cells = (radius / self.cell_size).ceil() as i32;
        let base = self.cell_coords(pos);
        let mut result = Vec::new();

        for dx in -cells..=cells {
            for dy in -cells..=cells {
                for dz in -cells..=cells {
                    let cell = (base.0 + dx, base.1 + dy, base.2 + dz);
                    if self.cells.contains_key(&cell) {
                        result.push(cell);
                    }
                }
            }
        }

        result
    }

    /// Iterate over all particle indices within a radius of the given position.
    pub fn query_radius<F>(&self, pos: Vec3, radius: f64, particles: &[Particle], mut callback: F)
    where
        F: FnMut(usize),
    {
        let radius_sq = (radius * radius) as f32;
        let cells = self.get_neighbors(pos, radius);

        for cell in cells {
            if let Some(indices) = self.cells.get(&cell) {
                for &i in indices {
                    let p = &particles[i];
                    if p.position.distance_squared(pos) <= radius_sq {
                        callback(i);
                    }
                }
            }
        }
    }

    /// Get the number of populated cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}
