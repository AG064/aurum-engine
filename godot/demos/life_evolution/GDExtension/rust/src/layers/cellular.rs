//! Cellular layer simulation.
//!
//! Detects and simulates protocells, lipid bilayer formation,
//! and early metabolic systems.

use glam::Vec3;
use std::collections::HashMap;

use crate::simulation::emergence::MolecularEntity;

/// A simple protocell representation.
#[derive(Debug, Clone)]
pub struct Protocell {
    pub id: u64,
    pub center: Vec3,
    pub radius: f64,
    pub mass: f64,
    pub molecules: Vec<u64>, // IDs of constituent molecules
    pub energy: f64,
    pub has_membrane: bool,
    pub age: f64,
    pub is_alive: bool,
}

/// Detect protocell formation from molecular patterns.
/// A protocell forms when:
/// 1. There's a lipid-like molecule (long carbon chain with polar head)
/// 2. There's a hydrophilic environment (water molecules)
/// 3. There's some encapsulated cargo
pub fn detect_protocells(molecules: &[MolecularEntity]) -> Vec<Protocell> {
    let mut protocells = Vec::new();

    // Find lipid-like molecules (long carbon chains)
    let lipids: Vec<&MolecularEntity> = molecules
        .iter()
        .filter(|m| m.is_organic && m.atom_count > 10)
        .collect();

    // Find water
    let water: Vec<&MolecularEntity> = molecules.iter().filter(|m| m.formula == "H2O").collect();

    // Group nearby lipids together - they tend to form membranes
    let cluster_radius = 1e-8; // 10nm
    let mut clusters: HashMap<(i32, i32, i32), Vec<&MolecularEntity>> = HashMap::new();

    for lipid in &lipids {
        let cell = (
            (lipid.center.x / cluster_radius as f32) as i32,
            (lipid.center.y / cluster_radius as f32) as i32,
            (lipid.center.z / cluster_radius as f32) as i32,
        );
        clusters.entry(cell).or_default().push(lipid);
    }

    for (_, cluster) in clusters {
        if cluster.len() < 5 {
            continue; // Need at least 5 lipids for a membrane
        }

        let center: Vec3 = cluster.iter().map(|m| m.center).sum::<Vec3>() / cluster.len() as f32;

        // Check if there's water nearby
        let has_water = water
            .iter()
            .any(|w| w.center.distance(center) < cluster_radius as f32);

        if !has_water {
            continue;
        }

        let radius = cluster_radius as f64;
        let mass: f64 = cluster.iter().map(|_| 1e-21).sum(); // Approximate mass

        protocells.push(Protocell {
            id: protocells.len() as u64,
            center,
            radius,
            mass,
            molecules: cluster.iter().map(|m| m.id).collect(),
            energy: 0.0,
            has_membrane: true,
            age: 0.0,
            is_alive: true,
        });
    }

    protocells
}
