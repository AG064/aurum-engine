//! Main GDExtension class for the Life Evolution simulation.
//!
//! Uses an async SimulationRunner to advance the simulation on a
//! background thread. The Godot main thread polls for completed
//! snapshots without blocking.

use godot::prelude::*;
use godot::register::{godot_api, GodotClass};
use parking_lot::RwLock;
use std::sync::Arc;

use crate::simulation::{SimulationConfig, SimulationRunner};

/// GDExtension class that wraps the simulation world via an async runner.
#[derive(GodotClass)]
#[class(base=Node)]
pub struct SimulationWorld {
    runner: Arc<RwLock<Option<SimulationRunner>>>,
    frame: u64,
    initialized: bool,
    /// Cached loading progress (0-100) — mirror of runner state to avoid
    /// RwLock access from gdext 0.5.4 master which has a method registration bug
    /// for methods that acquire RwLock in a specific pattern.
    loading_progress_cached: u32,
    /// Whether the user has clicked "Start" to begin simulation ticking.
    simulation_started: bool,
    #[base]
    base: Base<Node>,
}

#[godot_api]
impl INode for SimulationWorld {
    fn init(base: Base<Node>) -> Self {
        Self {
            runner: Arc::new(RwLock::new(None)),
            frame: 0,
            initialized: false,
            loading_progress_cached: 0,
            simulation_started: false,
            base,
        }
    }
}

#[godot_api]
impl SimulationWorld {
    /// Initialize the simulation.
    #[func]
    fn initialize(&mut self, config_json: String) {
        let start = std::time::Instant::now();
        godot_print!("LifeEvolution: initialization started");
        let mut sim_config = SimulationConfig::default();

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&config_json) {
            if let Some(n) = value.get("particle_count").and_then(|v| v.as_i64()) {
                sim_config.particle_count = n.max(0) as usize;
            }
            if let Some(t) = value.get("temperature").and_then(|v| v.as_f64()) {
                sim_config.initial_temperature = t;
            }
            if let Some(r) = value.get("radius").and_then(|v| v.as_f64()) {
                sim_config.initial_radius = r;
            }
            if let Some(b) = value.get("gravity_enabled").and_then(|v| v.as_bool()) {
                sim_config.gravity_enabled = b;
            }
            if let Some(b) = value
                .get("electromagnetic_enabled")
                .and_then(|v| v.as_bool())
            {
                sim_config.electromagnetic_enabled = b;
            }
            if let Some(b) = value
                .get("quantum_forces_enabled")
                .and_then(|v| v.as_bool())
            {
                sim_config.quantum_forces_enabled = b;
            }
            if let Some(b) = value.get("emergence_enabled").and_then(|v| v.as_bool()) {
                sim_config.emergence_enabled = b;
            }
            if let Some(b) = value.get("auto_lod").and_then(|v| v.as_bool()) {
                sim_config.auto_lod = b;
            }
            if let Some(f) = value.get("force_scale").and_then(|v| v.as_f64()) {
                sim_config.force_scale = f as f32;
            }
            if let Some(v) = value.get("velocity_scale").and_then(|v| v.as_f64()) {
                sim_config.velocity_scale = v as f32;
            }
            if let Some(m) = value.get("particle_mass").and_then(|v| v.as_f64()) {
                sim_config.particle_mass = m as f32;
            }
            if let Some(g) = value.get("gravity_constant").and_then(|v| v.as_f64()) {
                sim_config.gravity_constant = g as f32;
            }
            if let Some(b) = value.get("periodic_boundaries").and_then(|v| v.as_bool()) {
                sim_config.periodic_boundaries = b;
            }
            if let Some(b) = value.get("boundary_radius").and_then(|v| v.as_f64()) {
                sim_config.boundary_radius = b;
            }
            if let Some(m) = value.get("max_dt").and_then(|v| v.as_f64()) {
                sim_config.max_dt = m;
            }
        }

        let count = sim_config.particle_count;
        let runner = SimulationRunner::new(sim_config);
        *self.runner.write() = Some(runner);
        self.initialized = true;
        self.frame = 0;

        godot_print!(
            "LifeEvolution: initialized with {} particles in {} ms",
            count,
            start.elapsed().as_millis()
        );
    }

    /// Advance the simulation. Non-blocking — dispatches the tick to the
    /// background thread and returns immediately. The renderer should call
    /// get_particle_positions() which returns the newest available snapshot.
    #[func]
    fn tick(&mut self, delta: f64) {
        if !self.initialized {
            godot_warn!("LifeEvolution: tick ignored before initialization");
            return;
        }
        if let Some(runner) = self.runner.read().as_ref() {
            runner.tick(delta);
        }
        self.frame += 1;
    }

    #[func]
    fn set_time_scale(&mut self, scale: f64) {
        if let Some(runner) = self.runner.read().as_ref() {
            runner.set_time_scale(scale);
        }
    }

    #[func]
    fn get_time_scale(&self) -> f64 {
        1.0 // Time scale is managed internally; this is a stub
    }

    #[func]
    fn set_paused(&mut self, paused: bool) {
        if let Some(runner) = self.runner.read().as_ref() {
            runner.set_paused(paused);
        }
    }

    #[func]
    fn is_paused(&self) -> bool {
        self.runner
            .read()
            .as_ref()
            .map(|r| r.is_paused())
            .unwrap_or(false)
    }

    #[func]
    fn get_simulation_time(&self) -> f64 {
        self.runner
            .read()
            .as_ref()
            .map(|r| r.get_time())
            .unwrap_or(0.0)
    }

    #[func]
    fn get_particle_count(&self) -> i64 {
        self.runner
            .read()
            .as_ref()
            .map(|r| r.get_particle_count() as i64)
            .unwrap_or(0)
    }

    /// ID of the newest completed worker snapshot.
    #[func]
    fn get_completed_tick(&self) -> i64 {
        self.runner
            .read()
            .as_ref()
            .map(|r| r.completed_tick_id() as i64)
            .unwrap_or(0)
    }

    /// Get the completed tick ID and pending count for diagnostics.
    /// (Moved to secondary impl block to work around gdext 0.5.4 master
    /// method registration bug — see bottom of file.)

    /// Statistics as a JSON string.
    #[func]
    fn get_statistics_json(&self) -> String {
        let stats = self
            .runner
            .read()
            .as_ref()
            .map(|r| r.get_statistics())
            .unwrap_or_default();
        let mut obj = serde_json::Map::new();
        obj.insert(
            "particle_count".to_string(),
            serde_json::json!(stats.particle_count),
        );
        obj.insert(
            "atom_count".to_string(),
            serde_json::json!(stats.atom_count),
        );
        obj.insert(
            "molecule_count".to_string(),
            serde_json::json!(stats.molecule_count),
        );
        obj.insert(
            "organism_count".to_string(),
            serde_json::json!(stats.organism_count),
        );
        obj.insert(
            "max_complexity".to_string(),
            serde_json::json!(stats.max_complexity),
        );
        obj.insert(
            "temperature".to_string(),
            serde_json::json!(stats.temperature),
        );
        obj.insert(
            "total_energy".to_string(),
            serde_json::json!(stats.total_energy),
        );
        // Gravity diagnostics
        obj.insert(
            "center_of_mass".to_string(),
            serde_json::json!([
                stats.center_of_mass.0,
                stats.center_of_mass.1,
                stats.center_of_mass.2
            ]),
        );
        obj.insert(
            "mean_radius".to_string(),
            serde_json::json!(stats.mean_radius),
        );
        obj.insert("avg_speed".to_string(), serde_json::json!(stats.avg_speed));
        obj.insert("max_speed".to_string(), serde_json::json!(stats.max_speed));
        obj.insert("avg_accel".to_string(), serde_json::json!(stats.avg_accel));
        obj.insert("max_accel".to_string(), serde_json::json!(stats.max_accel));
        obj.insert(
            "active_force_count".to_string(),
            serde_json::json!(stats.active_force_count),
        );
        obj.insert("avg_force".to_string(), serde_json::json!(stats.avg_force));
        obj.insert("max_force".to_string(), serde_json::json!(stats.max_force));
        serde_json::Value::Object(obj).to_string()
    }

    /// Particle positions as a PackedVector3Array.
    /// Returns data from the newest completed snapshot (never blocks).
    #[func]
    fn get_particle_positions(&self) -> PackedVector3Array {
        let runner_guard = self.runner.read();
        if let Some(runner) = runner_guard.as_ref() {
            if let Some(snap) = runner.get_snapshot() {
                // Arc<Vec> — cheap clone, no data copy
                let positions = &snap.positions;
                let mut array = PackedVector3Array::new();
                for p in positions.iter() {
                    array.push(Vector3::new(p.x, p.y, p.z));
                }
                return array;
            }
        }
        PackedVector3Array::new()
    }

    /// Particle colors as a PackedColorArray.
    #[func]
    fn get_particle_colors(&self) -> PackedColorArray {
        let runner_guard = self.runner.read();
        if let Some(runner) = runner_guard.as_ref() {
            if let Some(snap) = runner.get_snapshot() {
                let colors = &snap.colors;
                let mut array = PackedColorArray::new();
                for rgb in colors.iter() {
                    array.push(Color::from_rgb(rgb[0], rgb[1], rgb[2]));
                }
                return array;
            }
        }
        PackedColorArray::new()
    }

    /// Particle render radii as a PackedFloat32Array.
    #[func]
    fn get_particle_radii(&self) -> PackedFloat32Array {
        let runner_guard = self.runner.read();
        if let Some(runner) = runner_guard.as_ref() {
            if let Some(snap) = runner.get_snapshot() {
                let radii = &snap.radii;
                let mut array = PackedFloat32Array::new();
                for r in radii.iter() {
                    array.push(*r);
                }
                return array;
            }
        }
        PackedFloat32Array::new()
    }

    /// Atoms as a JSON string.
    #[func]
    fn get_atoms_json(&self) -> String {
        // For now, return empty — emergence detection on the runner
        // would need a separate query path
        "[]".to_string()
    }

    /// Molecules as a JSON string.
    #[func]
    fn get_molecules_json(&self) -> String {
        "[]".to_string()
    }

    /// Cosmic entities as a JSON string.
    #[func]
    fn get_cosmic_entities_json(&self) -> String {
        "[]".to_string()
    }

    /// Reset the simulation.
    #[func]
    fn reset(&mut self) {
        let runner_guard = self.runner.read();
        if let Some(runner) = runner_guard.as_ref() {
            let config = SimulationConfig::default();
            runner.reset(config);
        }
        self.frame = 0;
    }

    #[func]
    fn get_max_complexity(&self) -> i64 {
        self.runner
            .read()
            .as_ref()
            .map(|r| r.get_max_complexity() as i64)
            .unwrap_or(0)
    }

    #[func]
    fn get_frame(&self) -> i64 {
        self.frame as i64
    }

    #[func]
    fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// Secondary impl block — methods that the gdext 0.5.4 master has
/// trouble registering in the primary block are placed here.
#[godot_api(secondary)]
impl SimulationWorld {
    /// Loading progress 0–100 — secondary impl block registration.
    #[func]
    fn get_loading_progress(&self) -> i64 {
        self.loading_progress_cached as i64
    }

    /// Tell the simulation to begin ticking.
    #[func]
    fn start_simulation(&mut self) {
        self.simulation_started = true;
        if let Some(runner) = self.runner.read().as_ref() {
            let _ = runner;
        }
        godot_print!("LifeEvolution: simulation started");
    }

    /// Whether the simulation tick loop is running.
    #[func]
    fn is_simulation_started(&self) -> bool {
        self.simulation_started
    }

    /// Tick status as JSON string (Dictionary<Variant, Variant> would fail).
    #[func]
    fn get_tick_status(&self) -> String {
        let runner_guard = self.runner.read();
        if let Some(runner) = runner_guard.as_ref() {
            let mut s = String::with_capacity(64);
            s.push_str("{\"completed_tick\":");
            s.push_str(&runner.completed_tick_id().to_string());
            s.push_str(",\"pending_ticks\":");
            s.push_str(&runner.pending_ticks().to_string());
            s.push('}');
            s
        } else {
            String::from("{\"completed_tick\":0,\"pending_ticks\":0}")
        }
    }
}
