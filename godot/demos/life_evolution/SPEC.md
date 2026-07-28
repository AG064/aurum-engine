# Life Evolution — Emergent Universe Simulation

## Overview

**Life Evolution** is a real-time 3D simulation where complexity emerges from fundamental physics. Starting from pre-atomic particle soup, the simulation evolves through atomic formation, chemistry, molecular complexity, and — given the right conditions — life itself.

The project is built on **Godot 4.7** for rendering/visualization and **Rust (GDExtension)** for the simulation core, combining stunning visuals with high-performance computation.

## Design Philosophy

### Emergence Over Programming

We don't code "how a star forms" or "how life begins." Instead, we define:
- Fundamental particle behaviors
- Conservation laws (energy, momentum, charge, baryon number)
- Environmental conditions (gravity, temperature, radiation)
- Binding rules based on quantum numbers

Complexity *must* emerge. Stars form because gravity pulls matter together. Chemistry emerges because particles have binding affinities. Life emerges because self-replicating patterns are thermodynamically favorable under certain conditions.

### Multi-Scale Architecture

```
Layer 0: Fundamental Particles (quarks, electrons, photons, neutrinos)
    ↓ Emergence
Layer 1: Atomic Nuclei, Atoms, Ions
    ↓ Emergence
Layer 2: Molecules, Chemical Bonds
    ↓ Emergence
Layer 3: Macromolecules, Protocells
    ↓ Emergence
Layer 4: Cellular Life
    ↓ Emergence
Layer 5: Multicellular Organisms
    ↓ Emergence
Layer 6: Ecosystems, Planets, Stars
```

Each layer is defined by a separate simulation module. Higher layers don't simulate lower layers in detail — they inherit aggregated properties.

## Technical Architecture

### Stack

| Component | Technology | Purpose |
|-----------|-------------|---------|
| Engine | Godot 4.7 | Rendering, UI, game loop |
| Simulation Core | Rust (GDExtension) | Physics, particle systems, emergent behaviors |
| Physics | Custom Rust implementation + Rapier3D | Gravity, collisions, constraints |
| Rendering | Godot 3D + Custom Shaders | Particle visualization, LOD rendering |
| Interop | GDScript ↔ Rust via GDExtension | Game logic calls simulation |

### Core Simulation Components

#### 1. Particle System (Layer 0-1)
```rust
struct Particle {
    position: Vec3,
    velocity: Vec3,
    mass: f64,
    charge: i8,           // -2 to +2 (electron, quarks)
    spin: i8,             // fermion/boson classification
    baryon_number: i32,   // conserved
    particle_type: ParticleType,
}

enum ParticleType {
    Quark { flavor: QuarkFlavor, color: Color },
    Electron,
    Photon,
    Neutrino,
    Proton,      // emergent (3 quarks bound)
    Neutron,    // emergent (3 quarks bound)
}
```

#### 2. Emergence Engine
The emergence engine detects stable configurations and promotes them to higher-layer entities:
- 3 quarks with color neutrality → Proton/Neutron
- Proton + Electron → Hydrogen atom
- Multiple atoms → Molecule
- Self-replicating molecule pattern → Life

#### 3. Gravity & Spatial Partitioning
- Barnes-Hut algorithm for O(n log n) gravity
- Spatial hashing for collision/binding detection
- LOD: distant particles use aggregated mass points

#### 4. Binding System
Particles have quantum numbers that determine binding:
- Color charge (quarks) → Strong force binding
- Electric charge → Electromagnetic binding
- Electron shells → Chemical bonding rules

### Performance Strategy

#### LOD (Level of Detail)
| Zoom Level | Visible Entities | Simulation Detail |
|------------|------------------|-------------------|
| Galactic | Galaxy clusters | Aggregated mass, simplified gravity |
| Stellar | Star systems | Protostars, gas clouds as particles |
| Planetary | Planets/moons | Simplified molecular structures |
| Molecular | Surface chemistry | Full atomic simulation |
| Cellular | Cell structures | Molecular dynamics |
| Atomic | Quantum effects | Full particle simulation |

#### Parallelization
- Rayon for CPU parallelism (independent regions)
- Compute shaders for GPU particle rendering
- Spatial partitioning for cache efficiency

### Visualization

#### Rendering Pipeline
1. **Particle Cloud Shader**: GPU instanced rendering for millions of particles
2. **Adaptive LOD**: Automatic detail reduction based on camera distance
3. **Bloom/Glow**: Particle emission based on energy state
4. **Color Mapping**: 
   - Temperature → color (blackbody radiation)
   - Particle type → hue
   - Energy state → brightness

#### Post-Processing
- Bloom for stellar bodies
- Volumetric fog for gas clouds
- Subsurface scattering for organic structures

## File Structure

```
life_evolution/
├── SPEC.md
├── project.godot
├── GDExtension/
│   ├── life_evolution.gdextension
│   └── rust/
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs              # GDExtension entry point
│       │   ├── simulation/
│       │   │   ├── mod.rs
│       │   │   ├── particles.rs     # Particle system
│       │   │   ├── emergence.rs    # Emergence detection
│       │   │   ├── gravity.rs      # Gravity simulation
│       │   │   ├── binding.rs      # Particle binding logic
│       │   │   └── spatial.rs       # Spatial partitioning
│       │   ├── layers/
│       │   │   ├── mod.rs
│       │   │   ├── atomic.rs        # Atomic formation
│       │   │   ├── molecular.rs     # Chemistry
│       │   │   └── life.rs          # Life emergence
│       │   └── godot/
│       │       ├── mod.rs
│       │       ├── simulation_world.rs
│       │       └── particle_renderer.rs
├── scenes/
│   ├── main.tscn
│   ├── simulation_world.tscn
│   └── ui/
│       ├── hud.tscn
│       └── time_control.tscn
├── scripts/
│   ├── main.gd
│   ├── simulation_controller.gd
│   ├── camera_controller.gd
│   └── ui/
│       ├── hud.gd
│       └── time_slider.gd
└── shaders/
    ├── particle_cloud.gdshader
    ├── stellar_body.gdshader
    └── emission_glow.gdshader
```

## Simulation Parameters

### Default Starting Conditions
```gdscript
# Initial particle soup
const INITIAL_PARTICLE_COUNT = 100_000
const INITIAL_TEMPERATURE = 1e9  # Kelvin (early universe)
const INITIAL_DENSITY = 1e-6     # particles per cubic meter
const VOLUME_RADIUS = 1000.0     # meters

# Physics constants (scaled for simulation)
const GRAVITY_CONSTANT = 6.674e-11
const BOLTZMANN_CONSTANT = 1.380e-23
const PLANCK_CONSTANT = 6.626e-34

# Time control
const MIN_TIME_SCALE = 0.1x
const MAX_TIME_SCALE = 1e12x
const DEFAULT_TIME_SCALE = 1.0x
```

## Godot Integration

### Exposed Rust Functions
```rust
#[godot_api]
impl SimulationWorld {
    #[func]
    pub fn initialize(&mut self, config: Dictionary) -> void;
    
    #[func]
    pub fn set_time_scale(&mut self, scale: f64) -> void;
    
    #[func]
    pub fn get_particle_count(&self) -> i64;
    
    #[func]
    pub fn get_layer_statistics(&self, layer: i32) -> Dictionary;
    
    #[func]
    pub fn get_camera_bounds(&self) -> AABB;
    
    #[func]
    pub fn request_lod_update(&mut self, camera_position: Vector3, fov: f64) -> void;
}
```

### GDScript Interface
```gdscript
# simulation_controller.gd
extends Node3D

@onready var simulation: GDExtensionSimulation = $SimulationWorld

func _ready():
    simulation.initialize({
        "particle_count": 100000,
        "temperature": 1e9,
        "gravity_enabled": true,
        "emergence_enabled": true
    })

func _process(delta: float):
    simulation.tick(delta * time_scale)
    update_lod()
```

## Emergence Rules

### Atomic Formation
1. Quarks bind in groups of 3 (color-neutral) → Hadrons (protons, neutrons)
2. Quark-antiquark pairs → Mesons
3. Protons + Electrons → Hydrogen atoms (if temperature < 13.6 eV)
4. Neutron capture → Deuterium, Tritium, Helium
5. Nuclear fusion (at sufficient temperature/pressure) → Heavier elements

### Molecular Emergence
1. Electron sharing/transfer → Covalent/Ionic bonds
2. Hydrogen bonding → Water, organic molecules
3. Carbon chains → Organic chemistry backbone
4. Self-catalyzing reactions → Early metabolism

### Life Emergence
1. Lipid bilayer formation → Protocell membranes
2. RNA/DNA-like replication → Hereditary information
3. Error-correction mechanisms → Stable reproduction
4. Resource competition → Evolution pressure

## Development Phases

### Phase 1: Foundation ✓ (This implementation)
- [x] Project structure
- [x] GDExtension setup
- [x] Basic particle system
- [x] Gravity simulation
- [x] Godot rendering integration

### Phase 2: Atomic Physics
- [ ] Quark color binding implementation
- [ ] Hadron formation rules
- [ ] Electron shell structure
- [ ] Basic atomic visualization

### Phase 3: Chemistry & Molecules
- [ ] Electron orbital rules
- [ ] Bond formation/dissociation
- [ ] Molecular visualization
- [ ] Chemical reaction network

### Phase 4: Stellar/Planetary Formation
- [ ] Nebular physics
- [ ] Stellar ignition
- [ ] Planetary accretion
- [ ] Orbital mechanics

### Phase 5: Life
- [ ] Protocell simulation
- [ ] Replication mechanisms
- [ ] Evolutionary algorithm
- [ ] Ecosystem dynamics

## Controls

| Input | Action |
|-------|--------|
| Mouse Wheel | Zoom in/out |
| Middle Mouse | Pan camera |
| Right Click + Drag | Orbit camera |
| Space | Pause/Resume simulation |
| 1-9 | Set time scale (1x to 1e9x) |
| Tab | Toggle UI |
| F | Focus on heaviest object |
| R | Reset simulation |

## Future Considerations

### Multiplayer
- Server-authoritative simulation
- Client-side interpolation
- Observable universe synchronization

### Modding
- JSON-based emergence rule definitions
- Custom particle types
- Scriptable simulation events

### Performance
- GPU compute for particle simulation
- Distributed simulation across machines
- Adaptive timestep based on activity

---

*"The universe is not only queerer than we suppose, but queerer than we can suppose." — J.B.S. Haldane*
