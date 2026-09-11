# Aurum MCP design

Status: draft for review, 2026-09-11
Depends on: Phase 0 hot reload (`docs/HOT_RELOAD.md`), Aurum Studio design
(`docs/superpowers/specs/2026-08-31-aurum-studio-design.md`)

## Summary

The goal is that an AI model can control the Aurum engine the way Blender MCP
lets a model control Blender.

The decision is a **three-layer** architecture:

1. **Substrate** — the third-party Godot MCP Toolkit, used as-is and kept
   optional. It already provides transport, authentication, auditing, undo,
   scene leasing, screenshots, and playtest control.
2. **Engine tools** — an Aurum-owned `MCPToolkitExtension` that exposes
   `AurumNode` semantics as MCP tools. This is the missing capability and the
   main deliverable.
3. **Headless core server** — a later `aurum-mcp` Rust crate driving
   `aurum-core` with no Godot process at all. Because Aurum's authoritative
   simulation is Rust rather than GDScript, this is the *higher-fidelity*
   surface for typed, deterministic AI control, not merely a CI convenience.

No toolkit is written from scratch. No fork.

## 1. What "control the engine" actually means

Two capabilities are usually conflated under "AI controls the engine". They
have different owners and different primitives.

| | Capability | Domain | Who can serve it |
|---|---|---|---|
| **A** | Editor and authoring control: create and edit scenes, resources, tilesets, files; run the game; screenshot it; read logs; drive playtests | Godot | The MCP Toolkit — **already complete** |
| **B** | Engine semantics: spawn entities, attach components, emit and drain events, read and write typed state, set time scale, step simulation, save and load | Aurum | **Nothing exists — this is the gap** |

The toolkit cannot serve **B**, structurally. Aurum's ECS, event bus, typed
state, fixed timestep, and space simulation live in Rust behind `AurumNode`.
The toolkit is GDScript inside Godot and knows only the Godot side of that
boundary.

Blender MCP is a useful comparison. It is a thin addon plus a local socket,
and its power comes from `bpy` being a complete, documented scripting API.
Aurum has the equivalent already: `AurumNode` is explicitly documented as
"the only Rust surface to GDScript", with **56 exposed methods** forming a
flat, typed, string/number-based API. That is close to an ideal MCP tool
surface, and the seam was designed before MCP was a consideration. Very
little new engine surface is needed.

## 2. Decision: do not write our own toolkit

The vendored toolkit is not a toy. Measured on the copy in this repository:

| Property | Value |
|---|---|
| GDScript files | 127 |
| Lines of GDScript | 24,077 |
| Servers | editor **and** runtime (`mcp_runtime_server.gd`, 62 KB) |
| Security | `auth.gd`, `audit.gd`, `file_guard.gd`, `scrubber.gd`, `untrusted.gd` |
| Correctness | scene leasing, undo/redo, signal pair resolution, safe scene ops |
| Contract | path guards, version gating, timeout clamping, response contract enforcement |
| Extension system | reflection discovery, collision guards, hot-reload watcher |
| Documentation | `docs/extending.md`, 63 KB |
| Proven here | 36 tools served, 10/10 Inspector request cleanups in the Phase 0 gate |

Rebuilding that is months of work spent re-solving solved problems, and the
security and undo layers are exactly the parts most likely to be got wrong
first. **Forking is also rejected**: it means owning all 24k lines forever to
gain control we largely do not need, while losing upstream fixes.

## 3. Architecture

### Layer 1 — substrate (third-party, optional)

```
MCP client (Claude Code, Codex, Inspector, ...)
      |  stdio
      v
@npgamedev/godot-mcp-server        (Node.js, npx)
      |  WebSocket, localhost
      v
Godot MCP Toolkit addon            (editor plugin and/or runtime autoload)
      |  command registry
      v
built-in commands  +  Aurum extension commands
```

This layer is consumed exactly as shipped. It is **not** a required dependency
of Aurum, matching the Studio spec ("Manage the existing Godot MCP Toolkit
without making it a required Studio dependency").

### Layer 2 — `aurum_mcp` extension (ours, engine-owned)

An `MCPToolkitExtension` subclass registering `aurum_*` tools whose handlers
call the live `AurumNode`. This is the actual product of this design.

Why this is the right seam:

- Discovery is **by base class** — no naming, no directory requirement.
- Registered tools appear in `tools/list` **alongside built-ins**; the Node
  server is not patched or forked.
- Extension commands are published to the bridge through
  `extensions.list` / `extensions.refresh` / `extensions.changed`, built from
  one shared wire-shape function (`extension_meta_commands.gd`).
- Version gating, path guards, timeout clamping, cancellation, audit logging,
  and undo builders are inherited for free.

#### Placement constraint (important)

The extension must **not** live in `godot/addons/` in the default Godot
project.

`extends MCPToolkitExtension` is a **parse error** when the toolkit is not
installed. Godot's editor filesystem scan parses scripts to build the global
class list, so a plain checkout of this repository — which deliberately does
not track `godot/addons/godot_mcp_toolkit/` — would report script errors.

That is not cosmetic. The Phase 0 acceptance gate asserts "zero relevant
script, parse, load, or GDExtension failure matches" during headless
validation (`scripts/tests/phase0_contract.ps1`,
`docs/HOT_RELOAD.md`). A permanently-broken script would break that gate.

**Therefore:** the source lives at `integrations/aurum_mcp/` and is *installed*
into `godot/addons/aurum_mcp/` only when the toolkit is detected. Installation
is a script and, later, an Aurum Studio action. A fresh clone keeps zero MCP
dependency and a clean headless validation.

#### Runtime control is not extension-capable

Extension support exists **only on the editor side**.

`runtime/mcp_runtime_server.gd` does not use the command registry at all. It
dispatches through a fixed `match method:` block over exactly 14 methods:

```
echo, ping, runtime.screenshot, runtime.get_node_state, runtime.set_property,
debugger.get_log, signal.list, signal.connect, signal.disconnect, signal.emit,
input.simulate, animation_player.control, runtime.get_script_vars, execute.code
```

So the extension mechanism cannot add typed `aurum_*` tools to a **running
game**. This is a property of the toolkit, not of Aurum, and it must shape the
plan rather than surprise it later.

Three consequences:

1. **Layer 2 is editor-side engine control.** Tools operate on the `AurumNode`
   living in the edited scene, which is exactly right for authoring, inspection,
   and iterating on a scene.
2. **An interim runtime path already exists.** `execute.code` evaluates a
   single GDScript expression in the running game, so an agent can reach
   `AurumNode` methods today without writing any new code — for example calling
   `entity_count()` on the live node. It is untyped and awkward, and it must
   not become the permanent answer, but it means runtime control is not blocked.
3. **The durable answer is the headless Rust server.** Because Aurum's
   authoritative simulation lives in `aurum-core` rather than in GDScript, the
   strongest AI-control story is to drive the simulation headless, where the
   tools are typed and deterministic, and treat Godot as presentation. That
   inverts the usual assumption: for Aurum, the *headless* server is not the
   fallback, it is the higher-fidelity surface.

Consequently, any tool that must work against a running game should be designed
to work **either** through the generic Godot runtime methods **or** through the
headless core server — not through an editor extension that cannot reach it.

### Layer 3 — `aurum-mcp` Rust crate (ours, later)

`aurum-core` is pure Rust with no Godot dependency and is already fully
unit-tested. A headless MCP server can therefore drive the engine with **no
Godot process at all**:

- deterministic, reproducible simulation for AI-driven playtests;
- fast iteration without an editor or GPU;
- CI-friendly regression runs;
- the same `aurum-core` semantics the editor uses, so tools behave identically.

The toolkit structurally cannot provide this. It is Aurum's real
differentiator and it aligns with the already-planned `aurum-cli` adapter. The
server should be a library crate plus an `aurum mcp` subcommand.

## 4. Tool surface

Naming: flat snake_case with an `aurum_` prefix (`aurum_entity_spawn`), chosen
for maximum MCP client compatibility. Handlers stay thin — they validate,
call `AurumNode`, and serialize.

### Mapping to the existing `AurumNode` surface

| Domain | `AurumNode` methods | Proposed tools |
|---|---|---|
| Entities / components | `spawn`, `despawn`, `entity_exists`, `entity_count`, `set_component`, `get_component`, `has_component`, `remove_component`, `get_entities_with`, `get_entities_with_all` | `aurum_entity_spawn`, `aurum_entity_despawn`, `aurum_entity_list`, `aurum_component_set`, `aurum_component_get`, `aurum_component_remove` |
| Events | `emit_event`, `dispatch_events`, `event_received` | `aurum_event_emit`, `aurum_event_dispatch` |
| State | `state_get`, `state_set`, `state_has`, `state_remove`, `state_clear`, `state_to_json` | `aurum_state_get`, `aurum_state_set`, `aurum_state_list` |
| Time | `set_time_scale`, `get_time_scale` | `aurum_time_get`, `aurum_time_set_scale` |
| Modules | `register_module`, `list_modules`, `has_module` | `aurum_module_list` |
| Persistence | `save_to_json`, `load_from_json`, `components_to_json` | `aurum_save`, `aurum_load` |
| Diagnostics | `runtime_fingerprint` | `aurum_fingerprint` |
| Space | `space_configure`, `space_step`, `space_set_input`, `space_snapshot`, `space_set_transform`, `space_set_status`, `space_consume_fuel`, `space_apply_damage`, `space_repair_full`, `space_refuel_full`, `space_stop_motion`, `space_set_docked`, `space_reset`, `set_space_value` | `aurum_space_state`, `aurum_space_step`, `aurum_space_input`, `aurum_space_apply` |
| Story (VN) | `story_load`, `story_advance`, `story_pick_choice`, `story_jump_to`, `story_get_variable`, `story_set_variable`, `story_export_state`, `story_import_state`, `story_current_scene`, `story_is_loaded` | `aurum_story_*` |

### v1 scope

Ship the **read** surface completely and the **write** surface conservatively:

- **Read (all `mark_read_only()`)**: `aurum_world_snapshot`,
  `aurum_entity_list`, `aurum_component_get`, `aurum_state_get`,
  `aurum_state_list`, `aurum_module_list`, `aurum_fingerprint`,
  `aurum_space_state`.
- **Write**: `aurum_entity_spawn`, `aurum_entity_despawn`,
  `aurum_component_set`, `aurum_component_remove`, `aurum_event_emit`,
  `aurum_state_set`, `aurum_time_set_scale`, `aurum_save`, `aurum_load`.

`aurum_world_snapshot` is the highest-value single tool. Agents are only as
good as their grounding, and one call returning entities, their components,
and current state gives a model the whole world in one round trip instead of
an N+1 exploration loop. It should be built first.

### Explicit non-goal: no arbitrary code execution

Blender MCP's headline feature is executing arbitrary Python. The toolkit's
`execute.code` evaluates a single GDScript expression, not statements, so
there is no direct equivalent and we should not add one.

Typed `aurum_*` tools are the better contract: validated, annotated,
auditable, and durable across engine refactors. An arbitrary-code escape
hatch lets a model silently corrupt a project and makes failures
unreproducible.

## 5. Security and modes

The toolkit already supplies the machinery; the design obligation is to use it
correctly.

- **Three postures** — disabled, read-only, writable — map onto
  `GODOT_MCP_READ_ONLY=1` plus annotation filtering. `mark_read_only()` is the
  single source of truth and applies to extension tools identically to
  built-ins.
- **Safe defaults** — omitting annotations means the tool is treated as
  mutating, non-destructive, non-idempotent, and is excluded in read-only
  mode. Every read tool must therefore *explicitly* opt in.
- **Path guards** — declare guards if any tool ever accepts a filesystem path.
  Prefer tools that do not take paths at all.
- **Audit** — every call is already logged by the registry.
- **Local only** — no telemetry, no remote transport. This matches the Studio
  spec's local-only requirement.

## 6. Dependency and provenance policy

- The toolkit is **third-party and stays untracked** (see `.gitignore`). Its
  code is MIT but its logo, icons, banner, and name are explicitly *not*
  licensed for reuse — so it must never be rebranded, and Aurum Studio should
  present it as an optional, user-installed component.
- Pin the bridge server to an exact version. Phase 0 already set this
  precedent by binding @modelcontextprotocol/inspector to one verified
  version and hashing its entry point; the same discipline applies to
  `@npgamedev/godot-mcp-server`.
- Prefer a pinned local install over `npx -y` in any verified path, for
  determinism and offline operation.
- This extension depends on the toolkit's `MCPToolkitExtension` base class and
  registry signatures. Treat a toolkit version bump as a **breaking-change
  review**, version-gate accordingly, and keep a compatibility contract test.

## 7. Export safety

The toolkit registers an export plugin that strips, at export time:

- the entire `addons/godot_mcp_toolkit/` folder;
- `res://.mcp.json`;
- **every `.gd` file that is a direct subclass of `MCPToolkitExtension`,
  wherever it lives.**

So `aurum_mcp` cannot leak into a shipped game — **provided it remains a
direct subclass**. Multi-level inheritance is unsupported by the discovery
algorithm and would also break the export strip. This is a hard constraint on
the extension's class design and belongs in a contract test.

## 8. Proposed repository layout

```
aurum-engine/
├── integrations/
│   └── aurum_mcp/                 # our extension source (not auto-loaded)
│       ├── aurum_mcp_extension.gd # direct MCPToolkitExtension subclass
│       ├── aurum_mcp_tools_*.gd   # tool groups
│       └── README.md              # install + compatibility contract
├── scripts/
│   └── mcp.ps1                    # detect toolkit, install/remove extension
├── crates/
│   └── aurum-mcp/                 # layer 3 (later): headless core server
└── docs/
    └── superpowers/specs/2026-09-11-aurum-mcp-design.md
```

Installed state (untracked, like the toolkit itself):
`godot/addons/aurum_mcp/`.

## 9. Risks and open questions

| # | Risk | Status | Verification |
|---|---|---|---|
| R1 | The Node bridge may not surface extension tools to `tools/list` without a server-side catalogue entry | **Likely fine** — `extensions.list`/`refresh`/`changed` and one shared wire builder exist for this purpose | Spike: register one tool, connect a real client, confirm it appears |
| R2 | Some clients cache deferred tools; mid-session additions need `/mcp` reconnect | Documented toolkit limitation | Accept and document; install before connecting |
| R3 | Runtime server cannot host extensions | **Resolved — confirmed** | `mcp_runtime_server.gd` uses a fixed 14-method `match` dispatch and never touches the registry. See "Runtime control is not extension-capable" |
| R4 | Toolkit upgrade changes the base class or registry API | Inherent | Pin version; compatibility contract test |
| R5 | Extension edits need editor focus or `extensions_refresh` to hot-reload | Documented | Use `extensions_refresh` in automated flows |
| R6 | `aurum_*` handlers duplicate logic that `aurum-core` already owns | Design risk | Keep handlers pure adapters; all semantics stay in Rust |

## 10. Phasing

- **M1 — Spike.** Install the toolkit, register `aurum_fingerprint` and
  `aurum_world_snapshot` as **editor-side** extension tools, connect a real MCP
  client, and prove R1. Small, and it de-risks everything after it.
- **M2 — Read surface.** All read tools, read-only posture verified, plus the
  install/uninstall script and compatibility contract test.
- **M3 — Write surface.** Mutation tools with undo where the toolkit allows,
  save/load, and a documented recovery story.
- **M4 — Runtime reach.** Document and harden the `execute.code` interim path
  for reaching `AurumNode` in a running game, and decide whether a dedicated
  Aurum runtime bridge is justified. Depends on the finding in
  "Runtime control is not extension-capable".
- **M5 — Space and story.** `aurum-space` and `aurum-vn` tool groups.
- **M6 — Headless `aurum-mcp` crate.** No Godot; typed deterministic AI
  control of `aurum-core`, usable in CI. This is the higher-fidelity surface
  for simulation and the long-term answer for runtime control.

## 11. Non-goals

- Writing or forking an MCP toolkit.
- Rebranding or redistributing the toolkit's brand assets.
- Replacing Godot's editor, rendering, scenes, or asset pipeline.
- Arbitrary code execution as a tool.
- Making the toolkit a required Aurum or Studio dependency.
- Remote or cloud MCP transport; everything stays local.
