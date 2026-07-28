//! Godot-side emergence inspector.

use godot::prelude::*;
use godot::register::{godot_api, GodotClass};

/// Debug node that displays the current state of the simulation.
#[derive(GodotClass)]
#[class(base=Node)]
pub struct EmergenceInspector {
    /// Update interval in seconds.
    #[export]
    update_interval: f64,

    /// Simulation path as a String.
    simulation_path_str: String,

    last_update: f64,

    #[base]
    base: Base<Node>,
}

#[godot_api]
impl INode for EmergenceInspector {
    fn init(base: Base<Node>) -> Self {
        Self {
            simulation_path_str: "../SimulationWorld".to_string(),
            update_interval: 0.5,
            last_update: 0.0,
            base,
        }
    }

    fn process(&mut self, delta: f64) {
        self.last_update += delta;
        if self.last_update >= self.update_interval {
            self.last_update = 0.0;
            let _ = self.refresh();
        }
    }
}

#[godot_api]
impl EmergenceInspector {
    /// Set the simulation path.
    #[func]
    fn set_simulation_path(&mut self, path: String) {
        self.simulation_path_str = path;
    }

    /// Force a refresh. Logs the current emergence stats.
    #[func]
    fn refresh(&mut self) -> bool {
        let path_str = self.simulation_path_str.clone();
        let path = NodePath::from(path_str.as_str());
        let Some(simulation) = self.base().get_node_or_null(&path) else {
            return false;
        };
        let mut sim = simulation.cast::<Node>();
        if !sim.has_method("get_statistics_json") {
            return false;
        }
        let stats: String = sim.call("get_statistics_json", &[]).to();
        godot_print!("LifeEvolution stats: {}", stats);
        true
    }

    /// Get a human-readable report of the current simulation state.
    #[func]
    fn get_report(&self) -> String {
        let path_str = self.simulation_path_str.clone();
        let path = NodePath::from(path_str.as_str());
        let Some(simulation) = self.base().get_node_or_null(&path) else {
            return "No simulation found".to_string();
        };
        let mut sim = simulation.cast::<Node>();
        if !sim.has_method("get_statistics_json") {
            return "Simulation has no statistics method".to_string();
        }

        let stats_json: String = sim.call("get_statistics_json", &[]).to();
        let value: serde_json::Value =
            serde_json::from_str(&stats_json).unwrap_or_else(|_| serde_json::json!({}));

        let particles = value
            .get("particle_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let atoms = value
            .get("atom_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let molecules = value
            .get("molecule_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let organisms = value
            .get("organism_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let complexity = value
            .get("max_complexity")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let complexity_str = match complexity {
            0 => "Pre-particle",
            1 => "Particle soup",
            2 => "Atomic",
            3 => "Molecular",
            4 => "Cellular",
            5 => "Multicellular",
            _ => "Unknown",
        };

        format!(
            "=== Life Evolution ===\n\
             Complexity: {} ({})\n\
             Particles: {}\n\
             Atoms: {}\n\
             Molecules: {}\n\
             Organisms: {}\n\
             Temperature: {:.2e} K",
            complexity, complexity_str, particles, atoms, molecules, organisms, temperature
        )
    }
}
