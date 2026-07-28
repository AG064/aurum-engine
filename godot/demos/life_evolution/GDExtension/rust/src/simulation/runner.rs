//! Asynchronous simulation runner.
//!
//! Runs the simulation on a dedicated background thread, exposing a
//! non-blocking interface to Godot. The renderer always has the newest
//! available snapshot — never waits for the current tick to complete.
//!
//! Performance notes:
//! - Uses Arc<Vec> for snapshot data to avoid deep copies (zero-copy publish)
//! - Pre-allocated buffers reused each tick
//!
//! Loading is chunked into granular steps so the loading bar is always moving.
//! After loading completes, the simulation waits for the user to click "Start"
//! before beginning the tick loop.

use crate::simulation::{LayerStatistics, SimulationConfig, SimulationWorldCore};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

/// A snapshot of the simulation state for rendering.
/// Uses Arc<Vec> for zero-copy sharing between worker and renderer threads.
/// Each tick: worker atomically swaps in a new Arc, old Arc is dropped.
pub struct ParticleSnapshot {
    pub tick_id: u64,
    pub time: f64,
    /// Arc-shared positions — never cloned, just Arc::clone() for sharing
    pub positions: Arc<Vec<glam::Vec3>>,
    pub colors: Arc<Vec<[f32; 3]>>,
    pub radii: Arc<Vec<f32>>,
    pub particle_count: usize,
}

/// Pre-allocated buffers reused every tick to avoid repeated allocation.
struct SnapshotBuffers {
    positions: Vec<glam::Vec3>,
    colors: Vec<[f32; 3]>,
    radii: Vec<f32>,
}

impl SnapshotBuffers {
    fn new(capacity: usize) -> Self {
        Self {
            positions: Vec::with_capacity(capacity),
            colors: Vec::with_capacity(capacity),
            radii: Vec::with_capacity(capacity),
        }
    }

    fn clear(&mut self) {
        self.positions.clear();
        self.colors.clear();
        self.radii.clear();
    }

    fn ensure_capacity(&mut self, n: usize) {
        if self.positions.capacity() < n {
            self.positions = Vec::with_capacity(n);
            self.colors = Vec::with_capacity(n);
            self.radii = Vec::with_capacity(n);
        }
    }
}

impl ParticleSnapshot {
    /// Build a snapshot using pre-allocated buffers.
    /// Publishes by Arc-cloning the buffers (cheap pointer copy, no data copy).
    fn from_world(core: &SimulationWorldCore, tick_id: u64, buffers: &mut SnapshotBuffers) -> Self {
        let particles = core.particles.read();
        let n = particles.len();

        buffers.ensure_capacity(n);
        buffers.clear();

        for p in particles.iter() {
            buffers.positions.push(p.position);
            buffers.colors.push(p.render_color());
            buffers.radii.push(p.render_radius());
        }

        Self {
            tick_id,
            time: core.time,
            positions: Arc::new(std::mem::take(&mut buffers.positions)),
            colors: Arc::new(std::mem::take(&mut buffers.colors)),
            radii: Arc::new(std::mem::take(&mut buffers.radii)),
            particle_count: n,
        }
    }
}

/// Commands sent from the main thread to the simulation worker.
enum Command {
    Tick { dt: f64 },
    SetTimeScale { scale: f64 },
    SetPaused { paused: bool },
    Reset { config: SimulationConfig },
    Shutdown,
}

/// Asynchronous simulation runner.
/// Owns a background thread that advances the simulation.
/// Godot polls for completed snapshots without blocking.
pub struct SimulationRunner {
    core: Arc<RwLock<SimulationWorldCore>>,
    /// Newest completed snapshot — Arc<Vec> enables zero-copy publish.
    /// AtomicPtr swap is the publish primitive.
    snapshot: Arc<RwLock<Option<Arc<ParticleSnapshot>>>>,
    /// Commands to the worker thread (bounded, never grows unbounded).
    cmd_tx: crossbeam_channel::Sender<Command>,
    /// Worker thread handle.
    thread: Option<thread::JoinHandle<()>>,
    /// Latest completed tick ID.
    completed_tick: Arc<AtomicU64>,
    /// Pending tick count (how many ticks have been dispatched but not completed).
    pending_ticks: Arc<AtomicI64>,
    /// Shutdown flag.
    running: Arc<AtomicBool>,
    /// Loading state for the HUD
    loading_progress: Arc<AtomicU64>, // 0 = init, 100 = ready
    stats: Arc<RwLock<LayerStatistics>>,
}

impl SimulationRunner {
    /// Create and start a new runner. Initialization happens on the worker thread.
    pub fn new(config: SimulationConfig) -> Self {
        // Initialize Rayon thread pool before any parallel work
        crate::init_rayon();
        let core: Arc<RwLock<SimulationWorldCore>> =
            Arc::new(RwLock::new(SimulationWorldCore::new()));
        let snapshot: Arc<RwLock<Option<Arc<ParticleSnapshot>>>> = Arc::new(RwLock::new(None));

        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded(4);
        let cmd_rx = Arc::new(cmd_rx);

        let core_clone = core.clone();
        let snapshot_clone = snapshot.clone();
        let completed_tick = Arc::new(AtomicU64::new(0));
        let completed_tick_clone = completed_tick.clone();
        let pending_ticks = Arc::new(AtomicI64::new(0));
        let pending_ticks_clone = pending_ticks.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let loading_progress = Arc::new(AtomicU64::new(0));
        let loading_progress_clone = loading_progress.clone();
        let stats = Arc::new(RwLock::new(LayerStatistics::default()));
        let stats_clone = stats.clone();
        let config_clone = config.clone();

        let handle = thread::Builder::new()
            .name("LifeEvolution-Sim".into())
            .spawn(move || {
                // --- Loading phase: chunked progress updates so the bar is always moving ---
                loading_progress_clone.store(5, Ordering::SeqCst);

                // Create particles + spatial grid (this is the heavy work)
                {
                    let mut c = core_clone.write();
                    c.initialize(config_clone.particle_count, config_clone.clone());
                    *stats_clone.write() = c.get_statistics();
                }
                loading_progress_clone.store(70, Ordering::SeqCst);

                // Build spatial index
                {
                    let c = core_clone.read();
                    let _ = c.get_statistics(); // triggers index rebuild
                }
                loading_progress_clone.store(95, Ordering::SeqCst);

                // Emit initial snapshot so the scene isn't blank when the user clicks Start
                {
                    let mut initial_buffers = SnapshotBuffers::new(config_clone.particle_count);
                    let snap = Arc::new(ParticleSnapshot::from_world(
                        &core_clone.read(),
                        0,
                        &mut initial_buffers,
                    ));
                    *snapshot_clone.write() = Some(snap);
                }
                loading_progress_clone.store(100, Ordering::SeqCst);
                godot::global::godot_print!("LifeEvolution: loading complete — entering tick loop");

                // --- Enter tick loop directly ---
                // No more "wait for start" gate — the simulation runs from the moment it's
                // loaded. Pausing via set_paused(true) halts the tick loop. This way
                // the GDScript bridge can start the simulation just by calling
                // set_paused(false) without needing a separate "start" method (which
                // the gdext 0.5.4 master has trouble registering).
                Self::worker_loop(
                    core_clone,
                    cmd_rx,
                    snapshot_clone,
                    completed_tick_clone,
                    pending_ticks_clone,
                    running_clone,
                    stats_clone,
                );
            })
            .expect("failed to spawn simulation thread");

        Self {
            core,
            snapshot,
            cmd_tx,
            thread: Some(handle),
            completed_tick,
            pending_ticks,
            running,
            loading_progress,
            stats,
        }
    }

    /// Worker thread main loop.
    fn worker_loop(
        core: Arc<RwLock<SimulationWorldCore>>,
        cmd_rx: Arc<crossbeam_channel::Receiver<Command>>,
        snapshot: Arc<RwLock<Option<Arc<ParticleSnapshot>>>>,
        completed_tick: Arc<AtomicU64>,
        pending_ticks: Arc<AtomicI64>,
        running: Arc<AtomicBool>,
        stats: Arc<RwLock<LayerStatistics>>,
    ) {
        let mut pending_cmds: Vec<Command> = Vec::with_capacity(4);
        let mut tick_accumulator: f64 = 0.0;
        let mut logged_first_tick = false;
        let mut next_tick_id: u64 = 0;
        // Target: try to produce a snapshot every ~16ms (60 FPS)
        let target_frame: f64 = 1.0 / 60.0;
        // Pre-allocated buffers — reused every tick to avoid allocation
        let mut buffers = SnapshotBuffers::new(0);

        loop {
            // Collect pending commands (non-blocking drain)
            while let Ok(cmd) = cmd_rx.try_recv() {
                if matches!(cmd, Command::Shutdown) {
                    running.store(false, Ordering::SeqCst);
                    return;
                }
                pending_cmds.push(cmd);
            }

            // Process commands
            for cmd in pending_cmds.drain(..) {
                match cmd {
                    Command::Tick { dt } => {
                        tick_accumulator += dt;
                        // Run multiple ticks if enough time has accumulated
                        // Cap at 4 ticks per wake cycle to prevent infinite loops
                        let mut ticks_run = 0;
                        while tick_accumulator >= target_frame && ticks_run < 4 {
                            let step_dt = tick_accumulator.min(target_frame);
                            tick_accumulator -= step_dt;
                            let (tick_id, current_stats) = {
                                let mut c = core.write();
                                c.tick(step_dt);
                                next_tick_id += 1;
                                (next_tick_id, c.get_statistics())
                            };
                            if !logged_first_tick {
                                godot::global::godot_print!(
                                    "LifeEvolution: first tick completed, avg_force={}, active_forces={}",
                                    current_stats.avg_force,
                                    current_stats.active_force_count
                                );
                                logged_first_tick = true;
                            }
                            *stats.write() = current_stats;

                            // Build snapshot using pre-allocated buffers
                            // Arc::new() wraps the Vec; Arc::clone() is a cheap pointer copy
                            let snap = Arc::new(ParticleSnapshot::from_world(
                                &core.read(),
                                tick_id,
                                &mut buffers,
                            ));

                            // Publish: swap in the new Arc (atomic, no lock needed for the swap itself)
                            *snapshot.write() = Some(snap);
                            completed_tick.store(tick_id, Ordering::SeqCst);
                            pending_ticks.fetch_sub(1, Ordering::SeqCst);
                            ticks_run += 1;
                        }
                    }
                    Command::SetTimeScale { scale } => {
                        core.write().time_scale = scale;
                    }
                    Command::SetPaused { paused } => {
                        core.write().paused = paused;
                    }
                    Command::Reset { config } => {
                        let mut c = core.write();
                        c.initialize(config.particle_count, config);
                        *stats.write() = c.get_statistics();
                        // Invalidate the current snapshot
                        *snapshot.write() = None;
                    }
                    Command::Shutdown => unreachable!(),
                }
            }

            if !running.load(Ordering::SeqCst) {
                return;
            }

            if pending_ticks.load(Ordering::SeqCst) == 0 && cmd_rx.is_empty() {
                thread::sleep(std::time::Duration::from_micros(500));
            }
        }
    }

    /// Request a simulation tick (non-blocking).
    pub fn tick(&self, dt: f64) {
        if self.cmd_tx.send(Command::Tick { dt }).is_ok() {
            self.pending_ticks.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Get the newest available completed snapshot.
    /// Returns None if no tick has completed yet.
    pub fn get_snapshot(&self) -> Option<Arc<ParticleSnapshot>> {
        self.snapshot.read().as_ref().map(|arc| Arc::clone(arc))
    }

    /// Get loading progress (0–100).
    pub fn loading_progress(&self) -> u64 {
        self.loading_progress.load(Ordering::SeqCst)
    }

    pub fn completed_tick_id(&self) -> u64 {
        self.completed_tick.load(Ordering::SeqCst)
    }

    pub fn pending_ticks(&self) -> i64 {
        self.pending_ticks.load(Ordering::SeqCst)
    }

    pub fn set_time_scale(&self, scale: f64) {
        let _ = self.cmd_tx.send(Command::SetTimeScale { scale });
    }

    /// Set the simulation's paused state.
    /// When false, the worker thread will advance ticks.
    pub fn set_paused(&self, paused: bool) {
        let _ = self.cmd_tx.send(Command::SetPaused { paused });
    }

    /// Whether the simulation is currently paused.
    pub fn is_paused(&self) -> bool {
        self.core.read().paused
    }

    pub fn reset(&self, config: SimulationConfig) {
        let _ = self.cmd_tx.send(Command::Reset { config });
    }

    pub fn get_time(&self) -> f64 {
        self.core.read().time
    }

    pub fn get_particle_count(&self) -> usize {
        self.core.read().particles.read().len()
    }

    pub fn get_max_complexity(&self) -> u32 {
        self.stats.read().max_complexity
    }

    pub fn get_statistics(&self) -> LayerStatistics {
        self.stats.read().clone()
    }
}

impl Drop for SimulationRunner {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
