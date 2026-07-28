//! Life Evolution - Emergent Universe Simulation
//!
//! This crate implements the core simulation engine for the Life Evolution project.
//! It uses GDExtension to integrate with Godot 4.7 for rendering and visualization.

use godot::prelude::*;

mod godot_bridge;
mod layers;
mod simulation;

/// Initialize the GDExtension library. This is the entry point called by Godot.
pub struct LifeEvolutionExtension;

#[gdextension]
unsafe impl ExtensionLibrary for LifeEvolutionExtension {
    fn min_level() -> InitLevel {
        InitLevel::Scene
    }
}

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize Rayon thread pool to use all available CPU cores.
/// Called lazily on first simulation tick.
pub fn init_rayon() {
    use rayon::ThreadPoolBuilder;
    use std::cmp::max;
    use std::sync::OnceLock;

    static POOL: OnceLock<()> = OnceLock::new();
    POOL.get_or_init(|| {
        // Use all available cores minus one for Godot's main thread.
        let num_threads = max(1, num_cpus::get().saturating_sub(1));

        ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .thread_name(|i| format!("LifeEvolution-Worker-{}", i))
            .build_global()
            .expect("failed to initialize Rayon thread pool");

        log::info!(
            "LifeEvolution: Rayon thread pool started with {} threads",
            num_threads
        );
    });
}
