//! Core simulation engine for Life Evolution.
//!
//! This module provides the main simulation world, particle systems,
//! and emergence detection that drives the emergent behavior.

pub mod binding;
pub mod emergence;
pub mod gravity;
pub mod particles;
pub mod runner;
pub mod spatial;

pub use runner::SimulationRunner;

pub use binding::*;
pub use emergence::*;
pub use gravity::*;
pub use particles::*;
pub use spatial::*;

use parking_lot::RwLock;
use std::sync::Arc;

/// Layer identification for the multi-scale simulation architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimulationLayer {
    /// Fundamental particles (quarks, electrons, photons)
    Particle,
    /// Atomic nuclei and atoms
    Atomic,
    /// Molecules and chemistry
    Molecular,
    /// Macromolecules, protocells
    Cellular,
    /// Multicellular organisms
    Organism,
    /// Planets, stars, stellar systems
    Cosmic,
}

/// Top-level simulation world that orchestrates all the layers.
pub struct SimulationWorldCore {
    /// All particles in the simulation
    pub particles: Arc<RwLock<Vec<Particle>>>,
    /// Emergent entities that have formed
    pub emergent_entities: Arc<RwLock<EmergentEntities>>,
    /// Spatial partitioning structure
    pub spatial_index: Arc<RwLock<SpatialHashGrid>>,
    /// Current simulation time
    pub time: f64,
    /// Number of completed simulation steps.
    pub tick_count: u64,
    /// Time scale (1.0 = real-time)
    pub time_scale: f64,
    /// Whether the simulation is paused
    pub paused: bool,
    /// Configuration
    pub config: SimulationConfig,
}

/// Statistics for a particular simulation layer.
#[derive(Debug, Clone, Default)]
pub struct LayerStatistics {
    pub particle_count: u64,
    pub atom_count: u64,
    pub molecule_count: u64,
    pub organism_count: u64,
    pub max_complexity: u32,
    pub temperature: f64,
    pub total_energy: f64,
    // Gravity diagnostics
    pub center_of_mass: (f64, f64, f64),
    pub mean_radius: f64,
    pub avg_speed: f64,
    pub max_speed: f64,
    pub avg_accel: f64,
    pub max_accel: f64,
    pub total_momentum: f64,
    pub active_force_count: u64,
    pub avg_force: f64,
    pub max_force: f64,
}

impl SimulationWorldCore {
    /// Create a new simulation world with default configuration.
    pub fn new() -> Self {
        Self {
            particles: Arc::new(RwLock::new(Vec::new())),
            emergent_entities: Arc::new(RwLock::new(EmergentEntities::new())),
            // Cell size of 10m in a 200m-radius sphere (~40 cells/axis, ~64K cells total).
            // N=50K → ~1 particle/cell average. Each 10m-radius EM query checks ~27 cells.
            spatial_index: Arc::new(RwLock::new(SpatialHashGrid::new(10.0))),
            time: 0.0,
            tick_count: 0,
            time_scale: 1.0,
            paused: false,
            config: SimulationConfig::default(),
        }
    }

    /// Initialize the simulation with a particle soup of `n` particles.
    pub fn initialize(&mut self, particle_count: usize, config: SimulationConfig) {
        self.config = config;
        self.time = 0.0;
        self.tick_count = 0;

        let mut particles = self.particles.write();
        particles.clear();
        particles.reserve(particle_count);

        for i in 0..particle_count {
            particles.push(Particle::random_soup(i as u64, &self.config));
        }
        // Release the particle write lock before rebuilding the index. The
        // rebuild takes a read lock on the same particle collection.
        drop(particles);

        self.emergent_entities.write().clear();
        self.rebuild_spatial_index();

        log::info!("Initialized simulation with {} particles", particle_count);
    }

    /// Advance the simulation by `dt` seconds of real time.
    /// The internal simulation time is scaled by `time_scale`.
    pub fn tick(&mut self, dt: f64) {
        if self.paused {
            return;
        }

        // Cap the simulation dt to prevent huge time jumps at high
        // time-scales, which would explode the integrator.
        let raw_dt = dt * self.time_scale;
        let sim_dt = raw_dt.min(self.config.max_dt);
        self.time += sim_dt;
        self.tick_count += 1;

        // Reset forces at the start of each tick so stats reflect the
        // current tick's forces (not stale forces from a previous tick).
        // integrate() does NOT reset the force — that happens here.
        self.reset_forces();

        // Apply forces
        self.apply_gravity(sim_dt);
        self.apply_electromagnetic(sim_dt);
        self.apply_quantum_forces(sim_dt);

        // Integrate motion (uses accumulated force, but does NOT reset it
        // so HUD stats can read it)
        self.integrate_motion(sim_dt);

        // Emergence scans are much more expensive than force integration.
        // Run them periodically so particle count does not multiply the cost
        // of every physics step.
        if self.config.emergence_enabled && self.tick_count % 10 == 0 {
            self.detect_emergence();
        }

        // Rebuild spatial index for queries
        self.rebuild_spatial_index();

        // Auto-LOD: simplify far particles
        if self.config.auto_lod {
            self.update_lod();
        }
    }

    fn apply_gravity(&self, dt: f64) {
        if !self.config.gravity_enabled {
            return;
        }
        gravity::apply_gravity_barnes_hut(
            &self.particles,
            &self.spatial_index,
            dt,
            self.config.gravity_constant,
            self.config.force_scale,
        );
    }

    /// Reset all particle forces to zero.
    /// Called at the start of each tick so force accumulation is clean
    /// and HUD stats can read the current tick's forces.
    fn reset_forces(&self) {
        let mut particles = self.particles.write();
        for p in particles.iter_mut() {
            p.reset_force();
        }
    }

    fn apply_electromagnetic(&self, dt: f64) {
        if !self.config.electromagnetic_enabled {
            return;
        }
        binding::apply_electromagnetic_forces(
            &self.particles,
            &self.spatial_index,
            dt,
            self.config.force_scale,
        );
    }

    fn apply_quantum_forces(&self, dt: f64) {
        if !self.config.quantum_forces_enabled {
            return;
        }
        // Strong force binding for quarks, weak force for particle decay
        binding::apply_quantum_forces(&self.particles, &self.spatial_index, dt);
    }

    fn integrate_motion(&self, dt: f64) {
        let mut particles = self.particles.write();
        let boundary = self.config.boundary_radius;
        let use_periodic = self.config.periodic_boundaries;
        for particle in particles.iter_mut() {
            particle.integrate(dt);

            if use_periodic {
                // Wrap particles back into the simulation volume.
                let p = &mut particle.position;
                let range = boundary as f32;
                if p.x > range {
                    p.x -= 2.0 * range;
                } else if p.x < -range {
                    p.x += 2.0 * range;
                }
                if p.y > range {
                    p.y -= 2.0 * range;
                } else if p.y < -range {
                    p.y += 2.0 * range;
                }
                if p.z > range {
                    p.z -= 2.0 * range;
                } else if p.z < -range {
                    p.z += 2.0 * range;
                }
            } else {
                // Soft boundary: reflect particles back in.
                let limit = boundary as f32;
                let p = &mut particle.position;
                if p.x.abs() > limit {
                    p.x = limit * p.x.signum();
                    particle.velocity.x *= -0.5;
                }
                if p.y.abs() > limit {
                    p.y = limit * p.y.signum();
                    particle.velocity.y *= -0.5;
                }
                if p.z.abs() > limit {
                    p.z = limit * p.z.signum();
                    particle.velocity.z *= -0.5;
                }
            }
        }
        particles.retain(|p| p.alive);
    }

    fn detect_emergence(&self) {
        emergence::detect_all_emergence(
            &self.particles,
            &self.emergent_entities,
            &self.spatial_index,
            &self.config,
        );
    }

    fn rebuild_spatial_index(&self) {
        let particles = self.particles.read();
        let mut index = self.spatial_index.write();
        index.rebuild(&particles);
    }

    fn update_lod(&self) {
        // TODO: Implement LOD - when many particles exist, simplify far ones
        // For now, just mark distant particles for simplified simulation
    }

    /// Get statistics for all layers.
    pub fn get_statistics(&self) -> LayerStatistics {
        let particles = self.particles.read();
        let emergent = self.emergent_entities.read();

        let total_energy: f64 = particles
            .iter()
            .map(|p| (p.kinetic_energy() + p.potential_energy()) as f64)
            .sum();

        // Compute gravity diagnostics
        let total_mass: f64 = particles.iter().map(|p| p.mass as f64).sum::<f64>();
        let com = if total_mass > 0.0 {
            let com_x: f64 = particles
                .iter()
                .map(|p| (p.position.x as f64) * (p.mass as f64))
                .sum::<f64>()
                / total_mass;
            let com_y: f64 = particles
                .iter()
                .map(|p| (p.position.y as f64) * (p.mass as f64))
                .sum::<f64>()
                / total_mass;
            let com_z: f64 = particles
                .iter()
                .map(|p| (p.position.z as f64) * (p.mass as f64))
                .sum::<f64>()
                / total_mass;
            (com_x, com_y, com_z)
        } else {
            (0.0, 0.0, 0.0)
        };

        let n = particles.len() as f64;
        let mean_radius: f64 = if n > 0.0 {
            particles
                .iter()
                .map(|p| {
                    let dx = p.position.x as f64 - com.0;
                    let dy = p.position.y as f64 - com.1;
                    let dz = p.position.z as f64 - com.2;
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .sum::<f64>()
                / n
        } else {
            0.0
        };

        let mut total_speed = 0.0_f64;
        let mut max_speed = 0.0_f64;
        let mut total_force = 0.0_f64;
        let mut max_force = 0.0_f64;
        let mut active_force_count = 0_u64;
        for p in particles.iter() {
            let speed = p.velocity.length() as f64;
            total_speed += speed;
            max_speed = max_speed.max(speed);
            let force = p.force.length() as f64;
            total_force += force;
            max_force = max_force.max(force);
            if force > 1e-6 {
                active_force_count += 1;
            }
        }
        let avg_speed: f64 = if n > 0.0 { total_speed / n } else { 0.0 };
        let avg_force: f64 = if n > 0.0 { total_force / n } else { 0.0 };
        let avg_accel: f64 = if n > 0.0 {
            particles
                .iter()
                .map(|p| (p.force.length() / p.mass.max(1e-6)) as f64)
                .sum::<f64>()
                / n
        } else {
            0.0
        };
        let max_accel: f64 = particles
            .iter()
            .map(|p| (p.force.length() / p.mass.max(1e-6)) as f64)
            .fold(0.0_f64, f64::max);

        let total_momentum: f64 = particles
            .iter()
            .map(|p| {
                let vx = p.velocity.x as f64;
                let vy = p.velocity.y as f64;
                let vz = p.velocity.z as f64;
                (vx * vx + vy * vy + vz * vz).sqrt() * p.mass as f64
            })
            .sum::<f64>();

        LayerStatistics {
            particle_count: particles.len() as u64,
            atom_count: emergent.atomic.len() as u64,
            molecule_count: emergent.molecular.len() as u64,
            organism_count: emergent.organism.len() as u64,
            max_complexity: emergent.max_complexity(),
            temperature: self.estimate_temperature(&particles),
            total_energy,
            center_of_mass: com,
            mean_radius,
            avg_speed,
            max_speed,
            avg_accel,
            max_accel,
            total_momentum,
            active_force_count,
            avg_force,
            max_force,
        }
    }

    fn estimate_temperature(&self, particles: &[Particle]) -> f64 {
        // Average kinetic energy -> temperature
        if particles.is_empty() {
            return 0.0;
        }
        let avg_ke: f32 =
            particles.iter().map(|p| p.kinetic_energy()).sum::<f32>() / particles.len() as f32;
        // Use scaled units for visualization
        (avg_ke as f64) * 1e10
    }
}

impl Default for SimulationWorldCore {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for the simulation.
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Number of initial particles
    pub particle_count: usize,
    /// Initial temperature in Kelvin
    pub initial_temperature: f64,
    /// Initial spatial radius
    pub initial_radius: f64,
    /// Whether gravity is enabled
    pub gravity_enabled: bool,
    /// Gravitational constant (in simulation units).
    /// Real G = 6.674e-11; for visible dynamics at meter scale with
    /// unit-mass particles, the simulation uses a much larger G.
    pub gravity_constant: f32,
    /// Whether electromagnetic forces are enabled
    pub electromagnetic_enabled: bool,
    /// Whether quantum forces are enabled
    pub quantum_forces_enabled: bool,
    /// Whether emergence detection is enabled
    pub emergence_enabled: bool,
    /// Whether to automatically apply LOD
    pub auto_lod: bool,
    /// Boundary radius - particles wrap or reflect beyond this
    pub boundary_radius: f64,
    /// Proton fraction (0.0 to 1.0)
    pub proton_fraction: f64,
    /// Neutron fraction (0.0 to 1.0)
    pub neutron_fraction: f64,
    /// Electron fraction (0.0 to 1.0)
    pub electron_fraction: f64,
    /// Photon fraction (0.0 to 1.0)
    pub photon_fraction: f64,
    /// Force scale factor (multiplies all forces).
    /// Provides a global tuning knob on top of G.
    pub force_scale: f32,
    /// Effective particle mass in simulation units.
    /// All particles get this mass (overrides real physics masses).
    pub particle_mass: f32,
    /// Initial velocity scale (multiplies the thermal velocity).
    /// Lower values keep particles in the simulation volume.
    pub velocity_scale: f32,
    /// Enable periodic boundary conditions (particles wrap around).
    pub periodic_boundaries: bool,
    /// Time step cap (max sim seconds per tick). Limits how much
    /// simulation time can advance in one frame at high time-scales.
    pub max_dt: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            particle_count: 100_000,
            initial_temperature: 1e6, // K
            initial_radius: 100.0,    // m — small enough to see dynamics
            gravity_enabled: true,
            // G is in simulation units. For unit-mass particles at
            // ~100m initial radius, this produces visible collapse
            // on a timescale of ~100 sim seconds.
            gravity_constant: 5.0,
            electromagnetic_enabled: true,
            quantum_forces_enabled: true,
            emergence_enabled: true,
            auto_lod: true,
            boundary_radius: 200.0, // m
            proton_fraction: 0.45,
            neutron_fraction: 0.45,
            electron_fraction: 0.10,
            photon_fraction: 0.0,
            force_scale: 1.0,
            particle_mass: 1.0,
            velocity_scale: 0.5,
            periodic_boundaries: true,
            // Cap tick time at 1 sim sec to prevent huge time jumps
            // at high time scales (which would explode the integrator).
            max_dt: 1.0,
        }
    }
}
