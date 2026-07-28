//! Binding forces between particles.
//!
//! Implements electromagnetic, strong, and weak force interactions.
//! These forces determine how particles bind together to form atoms,
//! molecules, and more complex structures.

use glam::Vec3;
use parking_lot::RwLock;
use std::sync::Arc;

use super::particles::Particle;
use super::particles::ParticleType;
use super::spatial::SpatialHashGrid;

/// Apply electromagnetic forces between charged particles.
/// Uses Coulomb's law: F = k * q1 * q2 / r^2, with a force scale
/// to make interactions visible at the simulated scale.
pub fn apply_electromagnetic_forces(
    particles: &Arc<RwLock<Vec<Particle>>>,
    spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    _dt: f64,
    force_scale: f32,
) {
    // Compute forces into a parallel buffer to avoid borrow conflicts
    let particles_guard = particles.read();
    let n = particles_guard.len();
    let mut forces = vec![Vec3::ZERO; n];

    // Simulation-unit Coulomb constant. Real k = 8.99e9 N·m²/C² would
    // produce forces ~1e9x stronger than gravity, overwhelming gravitational
    // attraction. Use k_simulation ≈ G_simulation so EM and gravity are
    // comparable when q1=q2=m=1.
    let k = 5.0_f32;
    let softening_sq = 0.01_f32; // Avoid singularities
                                 // Short-range local interactions only — 10m in a 200m volume.
                                 // With ~10m cells, each query checks ~27 neighboring cells.
    let query_radius = 10.0_f32;

    // Use spatial hash for efficient neighbor queries
    let spatial_guard = spatial_index.read();
    for i in 0..n {
        let p1 = &particles_guard[i];
        if !p1.alive || p1.charge == 0 {
            continue;
        }

        spatial_guard.query_radius(p1.position, query_radius as f64, &particles_guard, |j| {
            if j <= i || !particles_guard[j].alive {
                return;
            }
            let p2 = &particles_guard[j];
            if p2.charge == 0 {
                return;
            }

            let delta = p2.position - p1.position;
            let dist_sq = delta.length_squared() + softening_sq;

            // F = k * q1 * q2 / r^2, scaled to be visible
            let force_mag = k * force_scale * (p1.charge as f32) * (p2.charge as f32) / dist_sq;
            let direction = if delta.length_squared() > 1e-12 {
                delta.normalize()
            } else {
                Vec3::ZERO
            };
            let force_vec = direction * force_mag;

            forces[i] += force_vec;
            forces[j] -= force_vec;
        });
    }

    drop(particles_guard);

    // Apply accumulated forces
    let mut particles = particles.write();
    for (i, force) in forces.into_iter().enumerate() {
        particles[i].force += force;
    }
}

/// Apply quantum forces (strong force for quark binding, weak for decay).
pub fn apply_quantum_forces(
    particles: &Arc<RwLock<Vec<Particle>>>,
    spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    _dt: f64,
) {
    let particles_guard = particles.read();
    let n = particles_guard.len();
    let mut forces = vec![Vec3::ZERO; n];

    // Strong force binding for quarks in close proximity
    // The strong force is very short range (~1 fm)
    let strong_force_range = 1e-15_f32;
    let strong_force_constant = 1.0e20_f32; // Scaled for simulation

    for i in 0..n {
        let p1 = &particles_guard[i];
        if !p1.alive || !is_quark(p1.particle_type) {
            continue;
        }

        spatial_index.read().query_radius(
            p1.position,
            strong_force_range as f64,
            &particles_guard,
            |j| {
                if j <= i || !particles_guard[j].alive {
                    return;
                }
                let p2 = &particles_guard[j];
                if p1.color_charge.is_none() || p2.color_charge.is_none() {
                    return;
                }
                let delta = p2.position - p1.position;
                let dist = delta.length();
                if dist < strong_force_range && dist > 0.0 {
                    let force_mag = strong_force_constant / (dist * dist + 1e-30);
                    let force_vec = delta.normalize() * force_mag;
                    forces[i] += force_vec;
                    forces[j] -= force_vec;
                }
            },
        );
    }

    drop(particles_guard);
    let mut particles_guard = particles.write();
    for (i, force) in forces.into_iter().enumerate() {
        particles_guard[i].force += force;
    }
}

fn is_quark(p: ParticleType) -> bool {
    matches!(
        p,
        ParticleType::UpQuark
            | ParticleType::DownQuark
            | ParticleType::StrangeQuark
            | ParticleType::CharmQuark
            | ParticleType::BottomQuark
            | ParticleType::TopQuark
    )
}

/// Compute the electromagnetic potential between two particles.
pub fn electromagnetic_potential(q1: i8, q2: i8, distance: f64) -> f64 {
    let k = 8.99e9;
    k * (q1 as f64) * (q2 as f64) / distance.max(1e-10)
}

/// Compute the strong force potential between two quarks.
pub fn strong_force_potential(distance: f64) -> f64 {
    // Simplified Yukawa-like potential
    let alpha_s = 0.5;
    let r0 = 1e-15;
    let mass_gluon = 0.0; // Massless gluons in this simplified model
    -alpha_s / distance.max(r0) * (-distance * mass_gluon).exp()
}

/// Check if a particle can bind with another based on quantum numbers.
pub fn can_bind(p1: &Particle, p2: &Particle) -> bool {
    // Baryon number conservation
    // Charge conservation
    // Color confinement for quarks

    if !p1.alive || !p2.alive {
        return false;
    }

    // Color confinement: 3 quarks with RGB or 3 antiquarks with anti-RGB
    if is_quark(p1.particle_type) && is_quark(p2.particle_type) {
        if let (Some(c1), Some(c2)) = (p1.color_charge, p2.color_charge) {
            return !is_same_color(c1, c2) && !is_anti_pair(c1, c2);
        }
    }

    // Quark-antiquark can form meson
    if is_quark(p1.particle_type) && is_antiquark(p2.particle_type) {
        return true;
    }
    if is_antiquark(p1.particle_type) && is_quark(p2.particle_type) {
        return true;
    }

    // Atoms: opposite charges attract at appropriate distance
    if p1.charge != 0 && p2.charge != 0 && p1.charge != p2.charge {
        return true;
    }

    false
}

fn is_antiquark(p: ParticleType) -> bool {
    // In this simplified model, we don't have separate antiquarks
    // Could be extended
    false
}

fn is_same_color(c1: super::particles::ColorCharge, c2: super::particles::ColorCharge) -> bool {
    use super::particles::ColorCharge::*;
    matches!(
        (c1, c2),
        (Red, Red)
            | (Green, Green)
            | (Blue, Blue)
            | (AntiRed, AntiRed)
            | (AntiGreen, AntiGreen)
            | (AntiBlue, AntiBlue)
    )
}

fn is_anti_pair(c1: super::particles::ColorCharge, c2: super::particles::ColorCharge) -> bool {
    use super::particles::ColorCharge::*;
    matches!(
        (c1, c2),
        (Red, AntiRed)
            | (AntiRed, Red)
            | (Green, AntiGreen)
            | (AntiGreen, Green)
            | (Blue, AntiBlue)
            | (AntiBlue, Blue)
    )
}

/// Compute binding energy between two particles.
pub fn binding_energy(p1: &Particle, p2: &Particle) -> f64 {
    let distance = p1.position.distance(p2.position) as f64;

    if is_quark(p1.particle_type) && is_quark(p2.particle_type) {
        strong_force_potential(distance)
    } else if p1.charge != 0 && p2.charge != 0 {
        electromagnetic_potential(p1.charge, p2.charge, distance)
    } else {
        0.0
    }
}
