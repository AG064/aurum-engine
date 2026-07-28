//! Organism-level simulation.
//!
//! Detects and simulates multicellular organisms, evolutionary
//! dynamics, and ecological interactions.

use glam::Vec3;
use std::collections::HashMap;

use crate::layers::cellular::Protocell;

/// A simple multicellular organism.
#[derive(Debug, Clone)]
pub struct Organism {
    pub id: u64,
    pub center: Vec3,
    pub radius: f64,
    pub mass: f64,
    pub cells: Vec<u64>, // IDs of constituent cells
    pub complexity: u32,
    pub generation: u32,
    pub lineage_id: u64,
    pub age: f64,
    pub energy: f64,
    pub is_alive: bool,
}

/// Detect organism formation from protocell clusters.
pub fn detect_organisms(protocells: &[Protocell]) -> Vec<Organism> {
    let mut organisms = Vec::new();

    // Group nearby protocells
    let cluster_radius = 1e-6; // 1 micrometer
    let mut visited: Vec<bool> = vec![false; protocells.len()];

    for (i, pc) in protocells.iter().enumerate() {
        if visited[i] || !pc.is_alive {
            continue;
        }

        let mut cluster = vec![i];
        let mut to_visit = vec![i];

        while let Some(idx) = to_visit.pop() {
            for (j, other) in protocells.iter().enumerate() {
                if j == idx || visited[j] || !other.is_alive {
                    continue;
                }
                if pc.center.distance(other.center) < cluster_radius as f32 {
                    cluster.push(j);
                    visited[j] = true;
                    to_visit.push(j);
                }
            }
        }

        if cluster.len() >= 3 {
            // Multicellular organism
            let center: Vec3 =
                cluster.iter().map(|&i| protocells[i].center).sum::<Vec3>() / cluster.len() as f32;

            let mass: f64 = cluster.iter().map(|&i| protocells[i].mass).sum();
            let complexity = (cluster.len() as u32).min(10);

            organisms.push(Organism {
                id: organisms.len() as u64,
                center,
                radius: cluster_radius * cluster.len() as f64,
                mass,
                cells: cluster.iter().map(|&i| protocells[i].id).collect(),
                complexity,
                generation: 1,
                lineage_id: 0,
                age: 0.0,
                energy: 0.0,
                is_alive: true,
            });
        }
    }

    organisms
}

/// Simple evolutionary dynamics - apply selection pressure.
pub fn evolve_organisms(organisms: &mut Vec<Organism>, dt: f64) {
    // Simple energy-based selection
    for org in organisms.iter_mut() {
        org.age += dt;

        // Organisms lose energy over time
        org.energy -= 0.001 * dt;

        // Death from old age or starvation
        if org.energy < 0.0 || org.age > 1000.0 {
            org.is_alive = false;
        }
    }

    // Remove dead organisms
    organisms.retain(|o| o.is_alive);
}
