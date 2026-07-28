//! Gravitational force calculations.
//!
//! Uses Barnes-Hut approximation for O(n log n) gravity when possible.
//! Uses a parallel direct solver for small systems and a Barnes-Hut octree
//! for large systems.

use crate::simulation::particles::Particle;
use crate::simulation::spatial::SpatialHashGrid;
use glam::Vec3;
use parking_lot::RwLock;
use rayon::prelude::*;
use std::sync::Arc;

/// Apply gravity to all particles using parallel Rayon computation.
///
/// For n ≤ 2000: direct O(n²) with parallel outer loop
/// For n > 1024: Barnes-Hut O(n log n) force evaluation.
pub fn apply_gravity_barnes_hut(
    particles: &Arc<RwLock<Vec<Particle>>>,
    _spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    _dt: f64,
    gravity_constant: f32,
    force_scale: f32,
) {
    // Snapshot positions/masses so we can release the read lock
    let snapshot: Vec<(usize, Vec3, f32, bool)> = {
        let guard = particles.read();
        guard
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.position, p.mass, p.alive))
            .collect()
    };

    let n = snapshot.len();
    if n == 0 {
        return;
    }

    // Use all available cores. Rayon auto-sizes its pool.
    if n <= 1024 {
        // Parallel direct O(n²):
        // Each thread computes into its own Vec, then we sum all chunks.
        // This avoids the borrow conflict of writing to the same Vec from multiple threads.
        let num_threads = rayon::current_num_threads();
        let chunk_size = (n + num_threads - 1) / num_threads;

        // Each chunk writes to its own private force buffer
        let mut chunk_forces: Vec<Vec<Vec3>> =
            (0..num_threads).map(|_| vec![Vec3::ZERO; n]).collect();

        // Each thread handles its chunk of particles (sequential inner loop)
        chunk_forces
            .par_iter_mut()
            .enumerate()
            .for_each(|(ci, forces_ci)| {
                let softening_sq = 1.0_f32;
                let start = ci * chunk_size;
                let end = (start + chunk_size).min(n);
                for i in start..end {
                    if !snapshot[i].3 {
                        continue;
                    }
                    let pos_i = snapshot[i].1;
                    for j in (i + 1)..n {
                        if !snapshot[j].3 {
                            continue;
                        }
                        let pos_j = snapshot[j].1;
                        let mass_j = snapshot[j].2;

                        let delta = pos_j - pos_i;
                        let dist_sq = delta.length_squared() + softening_sq;
                        let dist = dist_sq.sqrt();

                        // Newton's law: a_i += G * m_j * delta / dist³
                        // a_j -= G * m_j * delta / dist³
                        let accel_ij =
                            gravity_constant * force_scale * mass_j * delta / (dist_sq * dist);
                        forces_ci[i] += accel_ij;
                        forces_ci[j] -= accel_ij;
                    }
                }
            });

        // Sum all chunk forces into the final forces buffer
        let mut forces = vec![Vec3::ZERO; n];
        for chunk in chunk_forces {
            for (i, f) in chunk.into_iter().enumerate() {
                forces[i] += f;
            }
        }

        // Apply forces (single write lock, O(n) writes)
        let mut particles_guard = particles.write();
        for (i, force) in forces.into_iter().enumerate() {
            if i < particles_guard.len() {
                particles_guard[i].force += force;
            }
        }
    } else {
        let forces = apply_barnes_hut(&snapshot, gravity_constant, force_scale);
        let mut particles_guard = particles.write();
        for (i, force) in forces.into_iter().enumerate() {
            if i < particles_guard.len() {
                particles_guard[i].force += force;
            }
        }
    }
}

struct BarnesHutNode {
    center: Vec3,
    half_size: f32,
    mass: f32,
    center_of_mass: Vec3,
    children: [Option<usize>; 8],
    particles: Vec<usize>,
}

fn apply_barnes_hut(
    snapshot: &[(usize, Vec3, f32, bool)],
    gravity_constant: f32,
    force_scale: f32,
) -> Vec<Vec3> {
    let n = snapshot.len();
    if n == 0 {
        return vec![];
    }

    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for s in snapshot.iter().filter(|s| s.3) {
        min = min.min(s.1);
        max = max.max(s.1);
    }

    let center = (min + max) * 0.5;
    let half_size = ((max - min).max_element() * 0.5).max(1.0) + 1.0;
    let indices: Vec<usize> = snapshot
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.3.then_some(i))
        .collect();
    let mut nodes = Vec::with_capacity(n * 2);
    build_node(&indices, center, half_size, snapshot, &mut nodes, 0);

    let root = 0;
    let theta = 0.65_f32;
    let softening_sq = 1.0_f32;
    let mut forces = vec![Vec3::ZERO; n];
    forces.par_iter_mut().enumerate().for_each(|(i, force_i)| {
        if !snapshot[i].3 {
            return;
        }
        accumulate_force(
            root,
            i,
            snapshot,
            &nodes,
            theta,
            softening_sq,
            gravity_constant * force_scale,
            force_i,
        );
    });

    forces
}

fn build_node(
    indices: &[usize],
    center: Vec3,
    half_size: f32,
    snapshot: &[(usize, Vec3, f32, bool)],
    nodes: &mut Vec<BarnesHutNode>,
    depth: u32,
) -> usize {
    let node_index = nodes.len();
    nodes.push(BarnesHutNode {
        center,
        half_size,
        mass: 0.0,
        center_of_mass: Vec3::ZERO,
        children: [None; 8],
        particles: Vec::new(),
    });

    let mut mass = 0.0;
    let mut weighted_position = Vec3::ZERO;
    for &i in indices {
        mass += snapshot[i].2;
        weighted_position += snapshot[i].1 * snapshot[i].2;
    }
    nodes[node_index].mass = mass;
    nodes[node_index].center_of_mass = if mass > 0.0 {
        weighted_position / mass
    } else {
        center
    };

    if indices.len() <= 16 || depth >= 24 || half_size <= 0.01 {
        nodes[node_index].particles.extend_from_slice(indices);
        return node_index;
    }

    let mut buckets: [Vec<usize>; 8] = std::array::from_fn(|_| Vec::new());
    for &i in indices {
        let p = snapshot[i].1;
        let octant = (if p.x >= center.x { 1 } else { 0 })
            | (if p.y >= center.y { 2 } else { 0 })
            | (if p.z >= center.z { 4 } else { 0 });
        buckets[octant].push(i);
    }

    for (octant, bucket) in buckets.iter().enumerate() {
        if bucket.is_empty() {
            continue;
        }
        let child_center = center
            + Vec3::new(
                if octant & 1 != 0 {
                    half_size * 0.5
                } else {
                    -half_size * 0.5
                },
                if octant & 2 != 0 {
                    half_size * 0.5
                } else {
                    -half_size * 0.5
                },
                if octant & 4 != 0 {
                    half_size * 0.5
                } else {
                    -half_size * 0.5
                },
            );
        let child = build_node(
            bucket,
            child_center,
            half_size * 0.5,
            snapshot,
            nodes,
            depth + 1,
        );
        nodes[node_index].children[octant] = Some(child);
    }
    node_index
}

fn accumulate_force(
    node_index: usize,
    target_index: usize,
    snapshot: &[(usize, Vec3, f32, bool)],
    nodes: &[BarnesHutNode],
    theta: f32,
    softening_sq: f32,
    gravity_scale: f32,
    output: &mut Vec3,
) {
    let node = &nodes[node_index];
    if node.mass <= 0.0 {
        return;
    }
    let target_position = snapshot[target_index].1;
    let delta = node.center_of_mass - target_position;
    let distance_sq = delta.length_squared() + softening_sq;
    let distance = distance_sq.sqrt();
    let contains_target = (target_position.x - node.center.x).abs() <= node.half_size
        && (target_position.y - node.center.y).abs() <= node.half_size
        && (target_position.z - node.center.z).abs() <= node.half_size;
    let is_leaf = node.children.iter().all(Option::is_none);

    if is_leaf || (!contains_target && (node.half_size * 2.0) / distance < theta) {
        if is_leaf {
            for &other_index in &node.particles {
                if other_index == target_index || !snapshot[other_index].3 {
                    continue;
                }
                let pair_delta = snapshot[other_index].1 - target_position;
                let pair_distance_sq = pair_delta.length_squared() + softening_sq;
                let pair_distance = pair_distance_sq.sqrt();
                *output += gravity_scale * snapshot[other_index].2 * pair_delta
                    / (pair_distance_sq * pair_distance);
            }
        } else {
            *output += gravity_scale * node.mass * delta / (distance_sq * distance);
        }
        return;
    }

    for child in node.children.iter().flatten() {
        accumulate_force(
            *child,
            target_index,
            snapshot,
            nodes,
            theta,
            softening_sq,
            gravity_scale,
            output,
        );
    }
}

/// Compute the gravitational potential energy of the system.
pub fn gravitational_potential_energy(particles: &[Particle], gravity_constant: f64) -> f32 {
    let mut total = 0.0;
    for i in 0..particles.len() {
        for j in (i + 1)..particles.len() {
            let delta = particles[j].position - particles[i].position;
            let dist = delta.length().max(1.0);
            total -= (gravity_constant as f32) * particles[i].mass * particles[j].mass / dist;
        }
    }
    total
}

/// Compute the center of mass of a particle collection.
pub fn center_of_mass(particles: &[Particle]) -> Vec3 {
    if particles.is_empty() {
        return Vec3::ZERO;
    }
    let total_mass: f32 = particles.iter().map(|p| p.mass).sum();
    if total_mass <= 0.0 {
        return Vec3::ZERO;
    }
    let com: Vec3 = particles.iter().map(|p| p.position * p.mass).sum::<Vec3>() / total_mass;
    com
}

/// Compute the total mass.
pub fn total_mass(particles: &[Particle]) -> f32 {
    particles.iter().map(|p| p.mass).sum()
}

/// Find the most massive cluster center (potential future star/planet).
pub fn find_dense_regions(
    particles: &[Particle],
    threshold_radius: f64,
) -> Vec<(Vec3, f32, usize)> {
    use std::collections::HashMap;
    let mut grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    let cell_size = threshold_radius as f32;

    for (i, p) in particles.iter().enumerate() {
        let cell = (
            (p.position.x / cell_size) as i32,
            (p.position.y / cell_size) as i32,
            (p.position.z / cell_size) as i32,
        );
        grid.entry(cell).or_default().push(i);
    }

    grid.values()
        .filter(|indices| indices.len() >= 10)
        .map(|indices| {
            let mass: f32 = indices.iter().map(|&i| particles[i].mass).sum();
            let com: Vec3 = indices
                .iter()
                .map(|&i| particles[i].position * particles[i].mass)
                .sum::<Vec3>()
                / mass;
            (com, mass, indices.len())
        })
        .collect()
}
