//! Emergence detection.
//!
//! This module detects when particles have formed stable configurations
//! and promotes them to higher-level entities. This is how complexity
//! emerges from the bottom up — we don't program atoms, molecules, or
//! life directly; instead, we detect when particles have arranged
//! themselves into these patterns naturally.

use glam::Vec3;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::particles::{Particle, ParticleType};
use super::spatial::SpatialHashGrid;
use super::SimulationConfig;
use super::SimulationLayer;

/// Container for all emergent entities across all layers.
#[derive(Debug)]
pub struct EmergentEntities {
    /// Atomic entities (atoms, ions)
    pub atomic: Vec<AtomicEntity>,
    /// Molecular entities
    pub molecular: Vec<MolecularEntity>,
    /// Cellular entities (protocells, simple cells)
    pub cellular: Vec<CellularEntity>,
    /// Organism entities
    pub organism: Vec<OrganismEntity>,
    /// Cosmic entities (stars, planets)
    pub cosmic: Vec<CosmicEntity>,
    /// Statistics counters
    pub counters: EmergenceCounters,
}

impl EmergentEntities {
    pub fn new() -> Self {
        Self {
            atomic: Vec::new(),
            molecular: Vec::new(),
            cellular: Vec::new(),
            organism: Vec::new(),
            cosmic: Vec::new(),
            counters: EmergenceCounters::default(),
        }
    }

    pub fn clear(&mut self) {
        self.atomic.clear();
        self.molecular.clear();
        self.cellular.clear();
        self.organism.clear();
        self.cosmic.clear();
        self.counters = EmergenceCounters::default();
    }

    pub fn max_complexity(&self) -> u32 {
        if !self.organism.is_empty() {
            5
        } else if !self.cellular.is_empty() {
            4
        } else if !self.molecular.is_empty() {
            3
        } else if !self.atomic.is_empty() {
            2
        } else {
            1
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct EmergenceCounters {
    pub total_atoms_formed: u64,
    pub total_molecules_formed: u64,
    pub total_cells_formed: u64,
    pub total_organisms_formed: u64,
    pub total_stars_formed: u64,
    pub total_planets_formed: u64,
}

/// An emergent atomic entity (atom or ion).
#[derive(Debug, Clone)]
pub struct AtomicEntity {
    pub id: u64,
    pub center: Vec3,
    pub element: Element,
    pub ionization: i32, // Net charge (negative = gained electrons)
    pub energy_level: f64,
    pub stability: f64, // 0.0 to 1.0
    pub particle_ids: Vec<u64>,
    pub formation_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Hydrogen,
    Helium,
    Lithium,
    Beryllium,
    Boron,
    Carbon,
    Nitrogen,
    Oxygen,
    Neon,
    Sodium,
    Magnesium,
    Silicon,
    Iron,
    Other(u32), // Atomic number
}

impl Element {
    pub fn symbol(&self) -> &'static str {
        match self {
            Element::Hydrogen => "H",
            Element::Helium => "He",
            Element::Lithium => "Li",
            Element::Beryllium => "Be",
            Element::Boron => "B",
            Element::Carbon => "C",
            Element::Nitrogen => "N",
            Element::Oxygen => "O",
            Element::Neon => "Ne",
            Element::Sodium => "Na",
            Element::Magnesium => "Mg",
            Element::Silicon => "Si",
            Element::Iron => "Fe",
            Element::Other(_) => "?",
        }
    }

    pub fn atomic_number(&self) -> u32 {
        match self {
            Element::Hydrogen => 1,
            Element::Helium => 2,
            Element::Lithium => 3,
            Element::Beryllium => 4,
            Element::Boron => 5,
            Element::Carbon => 6,
            Element::Nitrogen => 7,
            Element::Oxygen => 8,
            Element::Neon => 10,
            Element::Sodium => 11,
            Element::Magnesium => 12,
            Element::Silicon => 14,
            Element::Iron => 26,
            Element::Other(z) => *z,
        }
    }
}

/// An emergent molecular entity.
#[derive(Debug, Clone)]
pub struct MolecularEntity {
    pub id: u64,
    pub center: Vec3,
    pub formula: String,
    pub atom_count: usize,
    pub atomic_ids: Vec<u64>,
    pub stability: f64,
    pub formation_time: f64,
    pub is_organic: bool,
}

/// An emergent cellular entity (protocell).
#[derive(Debug, Clone)]
pub struct CellularEntity {
    pub id: u64,
    pub center: Vec3,
    pub radius: f64,
    pub mass: f64,
    pub energy: f64,
    pub molecular_ids: Vec<u64>,
    pub stability: f64,
    pub formation_time: f64,
    pub has_membrane: bool,
    pub has_genetic_material: bool,
    pub has_metabolism: bool,
}

/// An emergent organism.
#[derive(Debug, Clone)]
pub struct OrganismEntity {
    pub id: u64,
    pub center: Vec3,
    pub radius: f64,
    pub mass: f64,
    pub cell_ids: Vec<u64>,
    pub complexity: u32,
    pub stability: f64,
    pub formation_time: f64,
    pub generation: u32,
    pub lineage_id: u64,
}

/// An emergent cosmic entity (star, planet, etc.).
#[derive(Debug, Clone)]
pub struct CosmicEntity {
    pub id: u64,
    pub center: Vec3,
    pub velocity: Vec3,
    pub mass: f64,
    pub radius: f64,
    pub kind: CosmicKind,
    pub temperature: f64,
    pub stability: f64,
    pub formation_time: f64,
    pub particle_ids: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CosmicKind {
    Star,
    Planet,
    Moon,
    Asteroid,
    Nebula,
    GasCloud,
}

/// Detect all emergent structures in the current particle configuration.
pub fn detect_all_emergence(
    particles: &Arc<RwLock<Vec<Particle>>>,
    emergent: &Arc<RwLock<EmergentEntities>>,
    spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    config: &SimulationConfig,
) {
    if !config.emergence_enabled {
        return;
    }

    // Detect in order from lowest to highest level
    detect_atoms(particles, emergent, spatial_index, config);
    detect_molecules(particles, emergent, spatial_index, config);
    detect_cosmic_structures(particles, emergent, spatial_index, config);
    // detect_cellular() and detect_organisms() will be implemented in later phases
}

/// Detect atomic formations (proton + electron = hydrogen, etc.)
fn detect_atoms(
    particles: &Arc<RwLock<Vec<Particle>>>,
    emergent: &Arc<RwLock<EmergentEntities>>,
    _spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    config: &SimulationConfig,
) {
    // First, gather all atom data while holding only the read lock on particles
    let new_atoms: Vec<AtomicEntity> = {
        let particles_guard = particles.read();
        let mut atoms = Vec::new();
        let mut counter = 0u64;

        // Cluster size: scaled to the simulation. Real nuclei are picometers,
        // but our sim volume is hundreds of meters, so a few percent of
        // initial_radius is the relevant scale for "nuclei".
        let cluster_size: f32 = (config.initial_radius * 0.01) as f32;
        // Capture radius for electrons: a bit larger than the cluster size.
        let electron_capture_radius: f32 = cluster_size * 10.0;

        // Group protons and neutrons by proximity to find nuclei
        let mut nucleon_clusters: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();

        for (i, p) in particles_guard.iter().enumerate() {
            if !p.alive {
                continue;
            }
            if matches!(
                p.particle_type,
                ParticleType::Proton | ParticleType::Neutron
            ) {
                let cell = (
                    (p.position.x / cluster_size) as i32,
                    (p.position.y / cluster_size) as i32,
                    (p.position.z / cluster_size) as i32,
                );
                nucleon_clusters.entry(cell).or_default().push(i);
            }
        }

        // For each cluster, count protons and neutrons
        for (_, indices) in nucleon_clusters {
            if indices.is_empty() {
                continue;
            }

            let protons = indices
                .iter()
                .filter(|&&i| particles_guard[i].particle_type == ParticleType::Proton)
                .count();
            let _neutrons = indices
                .iter()
                .filter(|&&i| particles_guard[i].particle_type == ParticleType::Neutron)
                .count();

            if protons == 0 {
                continue; // Need at least one proton to be an atom
            }

            // Find bound electrons (close to the nucleus)
            let nucleus_center: Vec3 = indices
                .iter()
                .map(|&i| particles_guard[i].position)
                .sum::<Vec3>()
                / indices.len() as f32;

            let mut bound_electrons = 0;
            let mut electron_ids = Vec::new();

            for p in particles_guard.iter() {
                if p.alive && p.particle_type == ParticleType::Electron {
                    if p.position.distance(nucleus_center) < electron_capture_radius {
                        bound_electrons += 1;
                        electron_ids.push(p.id);
                    }
                }
            }

            // Determine element based on proton count
            let element = match protons {
                1 => Element::Hydrogen,
                2 => Element::Helium,
                3 => Element::Lithium,
                4 => Element::Beryllium,
                5 => Element::Boron,
                6 => Element::Carbon,
                7 => Element::Nitrogen,
                8 => Element::Oxygen,
                10 => Element::Neon,
                11 => Element::Sodium,
                12 => Element::Magnesium,
                14 => Element::Silicon,
                26 => Element::Iron,
                z => Element::Other(z as u32),
            };

            // Atom is stable if electrons match protons
            let is_neutral = bound_electrons as i32 == protons as i32;
            let stability = if is_neutral { 0.9 } else { 0.5 };

            let mut particle_ids: Vec<u64> =
                indices.iter().map(|&i| particles_guard[i].id).collect();
            particle_ids.extend(electron_ids);

            atoms.push(AtomicEntity {
                id: counter,
                center: nucleus_center,
                element,
                ionization: protons as i32 - bound_electrons as i32,
                energy_level: 0.0,
                stability,
                particle_ids,
                formation_time: 0.0,
            });

            counter += 1;
        }

        atoms
    };

    // Now acquire the write lock to update emergent entities
    let mut emergent_guard = emergent.write();
    emergent_guard.atomic.clear();
    let count = new_atoms.len() as u64;
    emergent_guard.atomic = new_atoms;
    emergent_guard.counters.total_atoms_formed = count;
}

/// Detect molecular formations (atoms bound together).
/// Uses a simple grid-based cluster search for O(n) instead of O(n²).
fn detect_molecules(
    _particles: &Arc<RwLock<Vec<Particle>>>,
    emergent: &Arc<RwLock<EmergentEntities>>,
    _spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    config: &SimulationConfig,
) {
    let mut emergent_guard = emergent.write();

    // Clear old molecules
    emergent_guard.molecular.clear();

    // Snapshot the atoms so we can release the borrow on emergent.
    let atoms = emergent_guard.atomic.clone();
    drop(emergent_guard);

    if atoms.len() < 2 {
        return;
    }

    // Use a coarse grid keyed by cell (sized to the bond distance) so
    // candidate pairs are only checked among neighbors.
    // Coordinates are simulation meters, not SI atomic meters. Scale the
    // molecular radius with the configured simulation volume so grid keys
    // stay finite and molecules can actually form in this world.
    let bond_distance: f32 = (config.initial_radius as f32 * 0.02).max(0.1);
    let cell_size: f32 = bond_distance * 1.5;
    let bond_distance_sq = bond_distance * bond_distance;

    // Build a grid of atom indices.
    let mut grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    for (i, atom) in atoms.iter().enumerate() {
        let cell = (
            (atom.center.x / cell_size) as i32,
            (atom.center.y / cell_size) as i32,
            (atom.center.z / cell_size) as i32,
        );
        grid.entry(cell).or_default().push(i);
    }

    let mut visited = vec![false; atoms.len()];
    let mut molecules = Vec::new();
    let mut counter: u64 = 0;

    for i in 0..atoms.len() {
        if visited[i] {
            continue;
        }
        let atom = &atoms[i];
        let cell = (
            (atom.center.x / cell_size) as i32,
            (atom.center.y / cell_size) as i32,
            (atom.center.z / cell_size) as i32,
        );

        // BFS over the 3x3x3 neighborhood to find all bonded atoms.
        let mut cluster = vec![i];
        let mut to_visit = vec![i];
        visited[i] = true;

        while let Some(idx) = to_visit.pop() {
            let a = &atoms[idx];
            let acell = (
                (a.center.x / cell_size) as i32,
                (a.center.y / cell_size) as i32,
                (a.center.z / cell_size) as i32,
            );
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let nc = (acell.0 + dx, acell.1 + dy, acell.2 + dz);
                        if let Some(indices) = grid.get(&nc) {
                            for &j in indices {
                                if visited[j] {
                                    continue;
                                }
                                let b = &atoms[j];
                                if a.center.distance_squared(b.center) <= bond_distance_sq {
                                    visited[j] = true;
                                    cluster.push(j);
                                    to_visit.push(j);
                                }
                            }
                        }
                    }
                }
            }
        }

        if cluster.len() >= 2 {
            // Build formula
            let mut element_counts: HashMap<Element, usize> = HashMap::new();
            for &idx in &cluster {
                *element_counts.entry(atoms[idx].element).or_insert(0) += 1;
            }

            let mut formula = String::new();
            let mut elements: Vec<_> = element_counts.keys().collect();
            elements.sort_by_key(|e| e.atomic_number());
            for element in elements {
                let count = element_counts[element];
                formula.push_str(element.symbol());
                if count > 1 {
                    formula.push_str(&count.to_string());
                }
            }

            let center: Vec3 =
                cluster.iter().map(|&idx| atoms[idx].center).sum::<Vec3>() / cluster.len() as f32;

            let is_organic = element_counts.contains_key(&Element::Carbon)
                && (element_counts.contains_key(&Element::Hydrogen)
                    || element_counts.contains_key(&Element::Oxygen)
                    || element_counts.contains_key(&Element::Nitrogen));

            molecules.push(MolecularEntity {
                id: counter,
                center,
                formula: formula.clone(),
                atom_count: cluster.len(),
                atomic_ids: cluster.iter().map(|&idx| atoms[idx].id).collect(),
                stability: 0.7,
                formation_time: 0.0,
                is_organic,
            });
            counter += 1;
        }
    }

    // Re-acquire the write lock to write results.
    let mut emergent_guard = emergent.write();
    emergent_guard.molecular = molecules;
    emergent_guard.counters.total_molecules_formed = counter;
}

/// Detect cosmic structures (stars, planets, gas clouds).
fn detect_cosmic_structures(
    particles: &Arc<RwLock<Vec<Particle>>>,
    emergent: &Arc<RwLock<EmergentEntities>>,
    _spatial_index: &Arc<RwLock<SpatialHashGrid>>,
    config: &SimulationConfig,
) {
    // First, gather all cosmic data while only holding the read lock
    let (new_cosmic, star_count, planet_count) = {
        let particles_guard = particles.read();
        let mut cosmic = Vec::new();
        let mut stars = 0u64;
        let mut planets = 0u64;

        // Cell size scaled to the simulation. 1/10 of the initial
        // radius works well for finding "proto-stars" and clusters.
        let cosmic_cell_size: f32 = (config.initial_radius * 0.1) as f32;
        // Mass thresholds for "star"/"planet" classification,
        // also scaled. With 50K unit-mass particles in 100m radius,
        // a cell of mass 50+ is a small cluster, 500+ is significant.
        let total_mass: f32 = particles_guard.iter().map(|p| p.mass).sum();
        let cell_star_threshold = total_mass * 0.01;
        let cell_planet_threshold = total_mass * 0.001;
        let cell_moon_threshold = total_mass * 0.0001;
        let cell_asteroid_threshold = total_mass * 0.00001;
        let cell_min_mass = total_mass * 0.000001;

        let mut cell_mass: HashMap<(i32, i32, i32), f64> = HashMap::new();
        let mut cell_particles: HashMap<(i32, i32, i32), Vec<u64>> = HashMap::new();
        let mut cell_center: HashMap<(i32, i32, i32), Vec3> = HashMap::new();

        for p in particles_guard.iter() {
            if !p.alive {
                continue;
            }
            let cell = (
                (p.position.x / cosmic_cell_size) as i32,
                (p.position.y / cosmic_cell_size) as i32,
                (p.position.z / cosmic_cell_size) as i32,
            );
            *cell_mass.entry(cell).or_insert(0.0) += p.mass as f64;
            cell_particles.entry(cell).or_default().push(p.id);
            *cell_center.entry(cell).or_insert(Vec3::ZERO) += p.position * p.mass;
        }

        for (cell, mass) in cell_mass {
            if mass < cell_min_mass as f64 {
                continue; // Too small to be a cosmic body
            }

            let com = cell_center[&cell] / mass as f32;
            let kind = if mass > cell_star_threshold as f64 {
                CosmicKind::Star
            } else if mass > cell_planet_threshold as f64 {
                CosmicKind::Planet
            } else if mass > cell_moon_threshold as f64 {
                CosmicKind::Moon
            } else if mass > cell_asteroid_threshold as f64 {
                CosmicKind::Asteroid
            } else {
                CosmicKind::GasCloud
            };

            // Rough temperature estimate based on mass.
            let radius = (mass / 1000.0).cbrt();
            let temperature = if matches!(kind, CosmicKind::Star) {
                1e6
            } else if matches!(kind, CosmicKind::Planet) {
                1000.0
            } else {
                100.0
            };

            cosmic.push(CosmicEntity {
                id: cosmic.len() as u64,
                center: com,
                velocity: Vec3::ZERO,
                mass,
                radius: radius as f64,
                kind,
                temperature,
                stability: 0.8,
                formation_time: 0.0,
                particle_ids: cell_particles.remove(&cell).unwrap_or_default(),
            });

            match kind {
                CosmicKind::Star => stars += 1,
                CosmicKind::Planet => planets += 1,
                _ => {}
            }
        }

        (cosmic, stars, planets)
    };

    // Now write the results
    let mut emergent_guard = emergent.write();
    emergent_guard.cosmic.clear();
    emergent_guard.cosmic = new_cosmic;
    emergent_guard.counters.total_stars_formed = star_count;
    emergent_guard.counters.total_planets_formed = planet_count;
}

/// Update LOD based on distance from camera.
pub fn update_lod_for_camera(
    particles: &Arc<RwLock<Vec<Particle>>>,
    camera_pos: Vec3,
    near_distance: f64,
    far_distance: f64,
) {
    let mut particles_guard = particles.write();
    for p in particles_guard.iter_mut() {
        let dist = p.position.distance(camera_pos) as f64;
        p.lod_level = if dist < near_distance {
            0 // Full simulation
        } else if dist < far_distance {
            1 // Simplified
        } else {
            2 // Highly simplified / aggregated
        };
    }
}
