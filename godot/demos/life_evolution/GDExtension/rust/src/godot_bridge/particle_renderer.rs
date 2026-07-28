//! Godot-side particle renderer.
//!
//! Packed MultiMesh rendering via Rust. Updates are throttled to completed
//! snapshots and uploaded in one contiguous buffer.

use godot::classes::{MultiMesh, MultiMeshInstance3D};
use godot::prelude::*;
use godot::register::{godot_api, GodotClass};

/// Node that updates MultiMesh rendering from the simulation snapshot.
#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct ParticleRenderer {
    /// Path to the simulation world node (relative from this node).
    simulation_path: String,

    /// Cached reference to the MultiMeshInstance3D.
    mm_instance: Option<Gd<MultiMeshInstance3D>>,

    /// Cached MultiMesh (resolved lazily on first render call).
    multi_mesh: Option<Gd<MultiMesh>>,
    last_rendered_tick: u64,
    render_stride: u64,

    #[base]
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for ParticleRenderer {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            simulation_path: "..".to_string(),
            mm_instance: None,
            multi_mesh: None,
            last_rendered_tick: 0,
            render_stride: 2,
            base,
        }
    }

    fn ready(&mut self) {
        godot_print!("ParticleRenderer: ready");
    }
}

#[godot_api]
impl ParticleRenderer {
    /// Set the path to the SimulationWorld node.
    #[func]
    fn set_simulation_path(&mut self, path: String) {
        self.simulation_path = path;
    }

    /// Set the MultiMeshInstance3D node that holds the MultiMesh to render into.
    #[func]
    fn set_multimesh_instance(&mut self, instance: Gd<MultiMeshInstance3D>) {
        self.mm_instance = Some(instance);
    }

    /// Main render entry point, called once per Godot frame.
    /// Uploads one packed MultiMesh buffer instead of issuing one API call per
    /// particle. This keeps the render-side overhead bounded for large clouds.
    #[func]
    fn update_rendering(&mut self) {
        // Lazy-resolve the MultiMesh on first use
        if self.multi_mesh.is_none() {
            if let Some(ref instance) = self.mm_instance {
                if let Some(mm) = instance.get_multimesh() {
                    self.multi_mesh = Some(mm);
                }
            }
        }

        // Fetch the completed tick first. The simulation can finish several
        // worker ticks between frames, but rendering the newest snapshot is
        // enough. This avoids repeated 50K-element uploads.
        let sim_path = NodePath::from(self.simulation_path.as_str());
        let mut sim = match self.base().get_node_or_null(&sim_path) {
            Some(node) => node,
            None => return,
        };
        if sim.has_method("get_completed_tick") {
            let completed_tick: i64 = sim.call("get_completed_tick", &[]).to();
            if completed_tick <= 0 {
                return;
            }
            let completed_tick = completed_tick as u64;
            if completed_tick == self.last_rendered_tick || completed_tick % self.render_stride != 0
            {
                return;
            }
            self.last_rendered_tick = completed_tick;
        }

        // Fetch data from simulation node before mutably borrowing MultiMesh.
        let (positions, colors, radii, n) = match self.fetch_particle_data() {
            Some(v) => v,
            None => return,
        };
        if n == 0 {
            return;
        }

        // Now we can mutably borrow mm
        let Some(ref mut mm) = self.multi_mesh else {
            return;
        };

        // Sync instance count
        let current_count = mm.get_instance_count() as usize;
        if current_count != n {
            mm.set_instance_count(n as i32);
        }

        let colors_len = colors.len() as usize;
        let radii_len = radii.len() as usize;
        let base_scale = 1.0f32;
        let mut buffer_values = Vec::with_capacity(n * 16);

        for i in 0..n {
            let pos = positions.get(i).unwrap_or(Vector3::ZERO);
            let scale = base_scale
                * if i < radii_len {
                    radii.get(i).unwrap_or(1.0)
                } else {
                    1.0
                };
            let color = if i < colors_len {
                colors.get(i).unwrap_or(Color::WHITE)
            } else {
                Color::WHITE
            };
            buffer_values.extend_from_slice(&[
                scale, 0.0, 0.0, pos.x,
                0.0, scale, 0.0, pos.y,
                0.0, 0.0, scale, pos.z,
                color.r, color.g, color.b, color.a,
            ]);
        }
        let buffer: PackedFloat32Array = buffer_values.into_iter().collect();
        mm.set_buffer(&buffer);
    }
}

impl ParticleRenderer {
    /// Fetch particle data from the simulation node.
    /// Returns (positions, colors, radii, count) or None on error.
    fn fetch_particle_data(
        &self,
    ) -> Option<(
        PackedVector3Array,
        PackedColorArray,
        PackedFloat32Array,
        usize,
    )> {
        let sim_path = NodePath::from(self.simulation_path.as_str());
        let mut sim = self.base().get_node_or_null(&sim_path)?;
        if !sim.has_method("get_particle_positions") {
            return None;
        }
        let positions: PackedVector3Array = sim.call("get_particle_positions", &[]).to();
        let colors: PackedColorArray = sim.call("get_particle_colors", &[]).to();
        let radii: PackedFloat32Array = sim.call("get_particle_radii", &[]).to();
        let n = positions.len() as usize;
        Some((positions, colors, radii, n))
    }
}
