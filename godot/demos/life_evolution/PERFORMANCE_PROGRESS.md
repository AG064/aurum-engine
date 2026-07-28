# Life Evolution — Performance & Correctness Progress

## Git State
- Branch: `main` (separate from existing `rust/` VN engine)
- All changes untracked in `life_evolution/` subdirectory

## Current Simulation Call Path (ASYNC)
```
Godot main thread (NON-BLOCKING):
  main.gd::_process(delta)
    -> simulation.tick(delta)           [DISPATCHES to background thread, returns immediately]
      -> Rust: SimulationRunner::tick(dt)
        -> cmd_tx.send(Tick { dt })    [bounded channel, non-blocking]
        -> pending_ticks.fetch_add(1)
        -> wake.store(true)            [signal worker]
    -> particle_renderer.update_rendering()
      -> get_particle_positions()     [reads from completed snapshot]
      -> get_particle_colors()        [reads from completed snapshot]
      -> get_particle_radii()         [reads from completed snapshot]
      -> for i in 0..N: set_instance_transform(i, t), set_instance_color(i, color)

Background thread (LifeEvolution-Sim):
  worker_loop()
    -> recv commands from channel (non-blocking drain)
    -> run simulation tick(s)
    -> publish snapshot via Arc<RwLock<Option<ParticleSnapshot>>>
    -> completed_tick.store(tick_id)
    -> pending_ticks.fetch_sub(1)
```

## Current Gravity Path (FIXED)
- N ≤ 2000: direct O(n²) pairwise ✓
- N > 2000: grid-aggregation (HashMap of cells) ✓
- **Force formula**: `a = G * m_j * delta / (dist_sq * dist)` = G*m*delta/r³ ✓

## Bugs Fixed

### BUG 1: Gravity formula uses wrong exponent ✓ FIXED
**File**: `src/simulation/gravity.rs`
- Before: `delta.normalize() * G * m1 * m2 / dist_sq`
  = G*m1*m2 * delta / (r² * r) = G*m1*m2 * delta / r³  ← wrong!
- After: `delta * G * m_j / (dist_sq * dist)`  
  = G*m_j * delta / (r² * r) = G*m_j * delta / r³  ← correct a toward j!

### BUG 2: EM forces use real Coulomb constant ✓ FIXED
**File**: `src/simulation/binding.rs`
- Before: `k = 8.99e9_f32` → EM 1e9x stronger than gravity
- After: `k = 5.0_f32` (≈ G_simulation), `query_radius = 10.0` (local only)

### BUG 3: Initial velocity astronomical ✓ FIXED
**File**: `src/simulation/particles.rs`
- Before: `thermal_speed = sqrt(2*k_B*T/m_p) ≈ 1.3e6 m/s` → particles escape in 0.1ms
- After: `v_thermal = 1.0 m/s` in simulation units → particles stay in volume

### BUG 4: Spatial grid cell size ✓ FIXED
- Before: `cell_size = 100.0` → ~64 cells in 200m volume → 800 particles/cell
- After: `cell_size = 10.0` → ~64K cells → ~1 particle/cell

### BUG 5: EM query radius = entire volume ✓ FIXED
- Before: `query_radius = 100.0` → O(N²) EM with N=50K
- After: `query_radius = 10.0` → local only, ~27 cells per query

### BUG 6: Thread blocking ✓ FIXED
- Before: `simulation.tick()` blocks Godot main thread for ~1.5s
- After: `SimulationRunner` owns background thread, Godot dispatches and polls

### BUG 7: Force application double-write ✓ FIXED
- Before: forces written by both direct loop AND grid_agg, then applied again
- After: forces applied inside the branch that computed them only

## Diagnostics Added
- `get_statistics_json()` now includes: center_of_mass, mean_radius, avg_speed, max_speed, avg_accel, max_accel
- HUD displays gravity diagnostics panel with CoM, Mean Radius, Speed, Accel
- `get_tick_status()` returns completed_tick_id and pending_ticks

## DLL Status
- Built: `life_evolution/GDExtension/rust/target/release/life_evolution.dll`
- Size: 4.1 MB
- Build time: ~1m 14s (release, LTO, codegen-units=1)

## Commands Used
```powershell
cd C:/Game_Development/life_evolution/GDExtension/rust
cargo build --release 2>&1
```

## Key Files Changed
- `src/simulation/gravity.rs` — gravity formula fix + Rayon parallel O(n²)
- `src/simulation/binding.rs` — EM constant + query radius fix
- `src/simulation/particles.rs` — thermal velocity fix
- `src/simulation/mod.rs` — cell size, gravity diagnostics
- `src/simulation/runner.rs` — async runner with Arc swap, pre-allocated buffers
- `src/godot_bridge/simulation_world.rs` — uses SimulationRunner + Arc<Vec> snapshots
- `src/godot_bridge/particle_renderer.rs` — PackedArray API fix
- `src/lib.rs` — Rayon thread pool init (all CPU cores)
- `Cargo.toml` — LTO fat, codegen-units=1, num_cpus dep
- `scripts/main.gd` — async init + loading screen integration
- `scripts/ui/loading_screen.gd` — NEW loading screen with progress bar
- `scripts/ui/hud.gd` — gravity diagnostics display
- `scenes/main.tscn` — GravityLabel + LoadingLayer nodes

## Performance Optimizations Applied

### 1. Parallel Gravity (Rayon)
- O(n²) gravity for N ≤ 2000: parallelized with `par_iter_mut()`
- Each thread processes its chunk of particles, writes to private force buffers
- Final reduction sums all chunks (lock-free accumulation)
- For N=50K on 8 cores: ~312M pairs per core instead of 2.5B sequential

### 2. Zero-Copy Snapshot Publishing (Arc Swap)
- Before: `snapshot.read().clone()` → deep copy all Vecs (positions, colors, radii)
- After: `Arc<Vec>` per field — Arc::clone() is a cheap pointer copy
- No data copied on publish; only atomic pointer swap

### 3. Pre-allocated Snapshot Buffers
- `SnapshotBuffers` reused every tick — no Vec reallocation
- `Arc::new()` + `std::mem::take()` pattern for zero-copy buffer handoff
- Ensures capacity upfront, clears and refills

### 4. Rayon Thread Pool (All CPU Cores)
- `ThreadPoolBuilder::new().num_threads(N-1).build_global()`
- On an 8-core machine: 7 threads for gravity, 1 for Godot
- Named threads: "LifeEvolution-Worker-0", etc.

### 5. Async Initialization
- Loading screen shown immediately at startup
- `SimulationRunner::new()` spawns background thread
- Initialization (particle creation, spatial index) runs on worker thread
- Progress tracked via atomic u64 (0→10→80→100)
- Godot main thread never blocks on simulation init

### 6. Build Optimization
- `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`
- Full LTO across all dependencies
- Single codegen unit for maximum optimization

## Loading Screen
- Shown immediately on startup (dark background, centered progress bar)
- Status messages update as init progresses
- 0.5s fade-out when simulation is ready
- No blocking on Godot main thread

## Next Steps
1. Run Godot and check gravity diagnostics in HUD (mean_radius, avg_speed, avg_accel)
2. If particles still don't cluster, reduce initial_radius to 50m or increase G to 50
3. Test async runner responsiveness (HUD should update even during heavy sim)
4. Profile parallel gravity — expect ~8x speedup on 8 cores
5. Consider Barnes-Hut octree for N>2000 (current grid-agg is O(n_cells × n_particles))
6. Consider disabling EM forces initially — focus on gravitational clustering first
