//! Particle system for the simulation.
//!
//! This is the foundation of everything else. Particles have properties
//! like position, velocity, mass, charge, and quantum numbers that determine
//! how they interact and what emergent structures they can form.

use glam::Vec3;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use super::SimulationConfig;

/// A fundamental particle in the simulation.
#[derive(Debug, Clone)]
pub struct Particle {
    /// Unique ID
    pub id: u64,
    /// Position in 3D space (meters)
    pub position: Vec3,
    /// Velocity in 3D space (m/s)
    pub velocity: Vec3,
    /// Mass (kg)
    pub mass: f32,
    /// Electric charge (in units of elementary charge)
    pub charge: i8,
    /// Spin quantum number (multiplied by 2, so 1/2 -> 1, 1 -> 2)
    pub spin: i8,
    /// Baryon number (1 for baryons, -1 for antibaryons, 0 for leptons)
    pub baryon_number: i32,
    /// Lepton number (1 for leptons, -1 for antileptons, 0 for quarks)
    pub lepton_number: i32,
    /// Type of particle
    pub particle_type: ParticleType,
    /// Whether this particle is still active
    pub alive: bool,
    /// Accumulated potential energy
    pub potential_energy: f32,
    /// Color charge for quarks (RGB triplet or anti-color)
    pub color_charge: Option<ColorCharge>,
    /// Current force on this particle
    pub force: Vec3,
    /// Layer of detail (0 = full sim, higher = simplified)
    pub lod_level: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleType {
    /// Up quark
    UpQuark,
    /// Down quark
    DownQuark,
    /// Strange quark
    StrangeQuark,
    /// Charm quark
    CharmQuark,
    /// Bottom quark
    BottomQuark,
    /// Top quark
    TopQuark,
    /// Electron
    Electron,
    /// Muon
    Muon,
    /// Tau lepton
    Tau,
    /// Electron neutrino
    ElectronNeutrino,
    /// Muon neutrino
    MuonNeutrino,
    /// Tau neutrino
    TauNeutrino,
    /// Photon
    Photon,
    /// Proton (emergent)
    Proton,
    /// Neutron (emergent)
    Neutron,
    /// Custom particle
    Custom(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorCharge {
    Red,
    Green,
    Blue,
    AntiRed,
    AntiGreen,
    AntiBlue,
}

impl Particle {
    /// Create a new particle with the given properties.
    pub fn new(id: u64, particle_type: ParticleType, position: Vec3, velocity: Vec3) -> Self {
        let (mass, charge, spin, baryon, lepton) = particle_properties(particle_type);
        let color_charge = if matches!(
            particle_type,
            ParticleType::UpQuark
                | ParticleType::DownQuark
                | ParticleType::StrangeQuark
                | ParticleType::CharmQuark
                | ParticleType::BottomQuark
                | ParticleType::TopQuark
        ) {
            Some(ColorCharge::Red)
        } else {
            None
        };

        Self {
            id,
            position,
            velocity,
            mass,
            charge,
            spin,
            baryon_number: baryon,
            lepton_number: lepton,
            particle_type,
            alive: true,
            potential_energy: 0.0,
            color_charge,
            force: Vec3::ZERO,
            lod_level: 0,
        }
    }

    /// Create a random particle from the initial soup configuration.
    pub fn random_soup(id: u64, config: &SimulationConfig) -> Self {
        let mut rng = SmallRng::seed_from_u64(id);
        let r = (rng.gen::<f64>().cbrt()) * config.initial_radius;
        let theta = rng.gen::<f64>() * std::f64::consts::TAU;
        let phi = (rng.gen::<f64>() * 2.0 - 1.0).acos();

        // Convert from spherical to Cartesian
        let position = Vec3::new(
            (r * theta.sin() * phi.sin()) as f32,
            (r * theta.cos() * phi.sin()) as f32,
            (r * phi.cos()) as f32,
        );

        // Velocity: random direction in a uniform sphere.
        // Use simulation-unit thermal speed so particles have modest velocities
        // that let gravity act over many seconds, not milliseconds.
        // "Thermal speed" here means: v_thermal = sqrt(G * M_cloud / R_cloud)
        // For a rough estimate, use ~1 m/s as a base, scaled by velocity_scale.
        // This gives particles time to interact gravitationally before scattering.
        let v_thermal = 1.0_f64; // m/s in simulation units — much slower than real plasma
        let speed = v_thermal * (config.velocity_scale as f64) * (-rng.gen::<f64>().ln()).sqrt();

        let v_theta = rng.gen::<f64>() * std::f64::consts::TAU;
        let v_phi = (rng.gen::<f64>() * 2.0 - 1.0).acos();
        let velocity = Vec3::new(
            (speed * v_theta.sin() * v_phi.sin()) as f32,
            (speed * v_theta.cos() * v_phi.sin()) as f32,
            (speed * v_phi.cos()) as f32,
        );

        // Choose particle type based on fractions
        let roll: f64 = rng.gen();
        let particle_type = if roll < config.proton_fraction {
            ParticleType::Proton
        } else if roll < config.proton_fraction + config.neutron_fraction {
            ParticleType::Neutron
        } else if roll < config.proton_fraction + config.neutron_fraction + config.electron_fraction
        {
            ParticleType::Electron
        } else if roll
            < config.proton_fraction
                + config.neutron_fraction
                + config.electron_fraction
                + config.photon_fraction
        {
            ParticleType::Photon
        } else {
            match rng.gen_range(0..3) {
                0 => ParticleType::UpQuark,
                1 => ParticleType::DownQuark,
                _ => ParticleType::Electron,
            }
        };

        // Override mass with the simulation's effective particle mass.
        // Real particle masses produce negligible forces at macroscopic
        // scales; the simulation uses a single mass value for all
        // particles so gravity is visible.
        let mut p = Self::new(id, particle_type, position, velocity);
        p.mass = config.particle_mass;
        p
    }

    /// Integrate motion using semi-implicit Euler.
    /// Note: does NOT reset the force — that's done at the start of
    /// the next tick so HUD stats can read the forces after integration.
    pub fn integrate(&mut self, dt: f64) {
        if !self.alive || self.mass <= 0.0 {
            return;
        }

        // F = ma => a = F/m
        let acceleration = self.force / self.mass;
        self.velocity += acceleration * (dt as f32);
        self.position += self.velocity * (dt as f32);
        // Force NOT reset here — see reset_forces() at start of next tick
    }

    /// Zero out the force on this particle.
    /// Called at the start of each simulation tick to ensure force
    /// accumulation is clean.
    pub fn reset_force(&mut self) {
        self.force = Vec3::ZERO;
    }

    /// Apply an instantaneous force impulse.
    pub fn apply_force(&mut self, force: Vec3) {
        self.force += force;
    }

    /// Get the kinetic energy of this particle.
    pub fn kinetic_energy(&self) -> f32 {
        0.5 * self.mass * self.velocity.length_squared()
    }

    /// Get the potential energy of this particle.
    pub fn potential_energy(&self) -> f32 {
        self.potential_energy
    }

    /// Get the total energy.
    pub fn total_energy(&self) -> f32 {
        self.kinetic_energy() + self.potential_energy
    }

    /// Get a representative color for this particle (for rendering).
    pub fn render_color(&self) -> [f32; 3] {
        match self.particle_type {
            ParticleType::Proton => [0.9, 0.4, 0.4],
            ParticleType::Neutron => [0.5, 0.5, 0.5],
            ParticleType::Electron => [0.3, 0.6, 1.0],
            ParticleType::Photon => [1.0, 1.0, 0.6],
            ParticleType::UpQuark => [1.0, 0.6, 0.8],
            ParticleType::DownQuark => [0.6, 1.0, 0.6],
            ParticleType::StrangeQuark => [0.8, 0.4, 1.0],
            ParticleType::CharmQuark => [1.0, 0.8, 0.4],
            ParticleType::BottomQuark => [0.5, 0.3, 0.2],
            ParticleType::TopQuark => [1.0, 0.3, 0.3],
            ParticleType::Muon => [0.4, 0.8, 1.0],
            ParticleType::Tau => [0.2, 0.4, 0.8],
            ParticleType::ElectronNeutrino => [0.7, 0.7, 1.0],
            ParticleType::MuonNeutrino => [0.5, 0.7, 1.0],
            ParticleType::TauNeutrino => [0.3, 0.5, 1.0],
            ParticleType::Custom(_) => [0.8, 0.8, 0.2],
        }
    }

    /// Get the radius for rendering this particle.
    pub fn render_radius(&self) -> f32 {
        match self.particle_type {
            ParticleType::Proton | ParticleType::Neutron => 0.5,
            ParticleType::Electron => 0.3,
            ParticleType::Photon => 0.4,
            _ => 0.2,
        }
    }
}

/// Get the physical properties of a particle type.
pub fn particle_properties(p: ParticleType) -> (f32, i8, i8, i32, i32) {
    // Returns (mass, charge, spin*2, baryon_number, lepton_number)
    match p {
        ParticleType::UpQuark => (2.2e-30, 2, 1, 1, 0),
        ParticleType::DownQuark => (4.7e-30, -1, 1, 1, 0),
        ParticleType::StrangeQuark => (9.5e-29, -1, 1, 1, 0),
        ParticleType::CharmQuark => (1.27e-27, 2, 1, 1, 0),
        ParticleType::BottomQuark => (4.18e-27, -1, 1, 1, 0),
        ParticleType::TopQuark => (1.73e-25, 2, 1, 1, 0),
        ParticleType::Electron => (9.11e-31, -1, 1, 0, 1),
        ParticleType::Muon => (1.88e-28, -1, 1, 0, 1),
        ParticleType::Tau => (3.17e-27, -1, 1, 0, 1),
        ParticleType::ElectronNeutrino => (1.0e-36, 0, 1, 0, 1),
        ParticleType::MuonNeutrino => (1.0e-36, 0, 1, 0, 1),
        ParticleType::TauNeutrino => (1.0e-36, 0, 1, 0, 1),
        ParticleType::Photon => (0.0, 0, 2, 0, 0),
        ParticleType::Proton => (1.673e-27, 1, 1, 1, 0),
        ParticleType::Neutron => (1.675e-27, 0, 1, 1, 0),
        ParticleType::Custom(_) => (1.0e-27, 0, 0, 0, 0),
    }
}

/// Get the mass of a particle in kg.
pub fn particle_mass(p: ParticleType) -> f32 {
    particle_properties(p).0
}

/// Get the electric charge of a particle in units of e.
pub fn particle_charge(p: ParticleType) -> i8 {
    particle_properties(p).1
}
