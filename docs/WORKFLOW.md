# The Aurum workflow

How we actually work on Aurum day to day, and what the AI can and cannot do.

Blender is **optional at every step**. Remove it from this document and the
pipeline still works end to end.

## The stack

| Layer | What it is | Depends on |
|---|---|---|
| `aurum-core` | ECS, events, state, time, dynamic world | nothing |
| `aurum-content` | Meshes, scenes, animations, sprites, glTF export | nothing |
| `aurum-2d/3d/space/vn` | Genre modules | aurum-core |
| `aurum-godot` | The `AurumNode` GDExtension shim | Godot at runtime |
| `aurum-mcp` | 45 MCP tools: runtime control + content authoring | nothing |
| Godot 4.7 | Editor, renderer, importer, runtime | — |
| Blender | Optional mesh authoring | — |

Everything above `aurum-godot` is plain Rust with no external dependencies.
`cargo test --workspace` runs 237 tests with no network and no Godot.

## Connecting an AI client

```json
{
  "mcpServers": {
    "aurum": {
      "command": "aurum",
      "args": ["mcp", "--root", "A:/path/to/project"]
    }
  }
}
```

`--root` confines every file the server writes. Add `--read-only` to remove
all mutating tools, so an agent can explore but not change anything.

## What the AI can do

Every row below is implemented and tested. "Tool" names are the MCP tools.

| You want | Ask for | Tools |
|---|---|---|
| **Create / place objects** | "add a crate at (0, 1, 0)" | `aurum_node_add`, `aurum_node_transform` |
| **Model** | "make a ring from a torus, merge two pillars into one mesh" | `aurum_mesh_add`, `aurum_mesh_transform`, `aurum_mesh_merge` |
| **Scene work** | "parent the orb to the hero, remove the old ground" | `aurum_node_add`, `aurum_node_remove`, `aurum_content_state` |
| **Animate** | "spin the hero twice over three seconds" | `aurum_animation_spin`, `aurum_animation_add` |
| **Sprites** | "pack these sprites into a 64px atlas and give me a layout" | `aurum_sprite_atlas` |
| **Materials** | "make it emissive teal, metalness 0.3" | `aurum_material_add` |
| **Ship to Godot** | "export the scene" | `aurum_content_export` → glTF → Godot imports it |
| **Run the game logic** | "spawn 10 enemies, set score to 0" | `aurum_entity_spawn`, `aurum_component_set`, `aurum_state_set` |
| **Story / VN** | "load the story, advance, pick choice 0" | `aurum_story_*` |
| **Space flight** | "set throttle, step one second" | `aurum_space_*` |
| **Save / restore** | "save to slot1, reset, load slot1" | `aurum_save`, `aurum_load`, `aurum_reset` |
| **Scripting** | "attach hero.gd to the Hero node" | `aurum_scene_bake` → run the generated script in Godot |

## The everyday loop

```
1.  aurum mcp            (or point your AI client at it)
2.  Ask the AI to build or change something.
3.  aurum_content_export -> models/scene.gltf     (geometry, materials, animation)
4.  aurum_scene_bake     -> gen/bake.gd           (attaches scripts, saves a .tscn)
5.  Godot imports it. Press play.
6.  Rust changes? pwsh scripts/dev.ps1  -> hot reload, editor stays open.
```

Step 5 is the Phase 0 loop: pure-Rust implementation changes reload into a
running editor with no restart. Only native class, method, signature, or
Godot-API changes need an editor restart.

## Where the AI works

Two surfaces, and it matters which you use:

**Headless (no Godot running).** `aurum-content` and the runtime tools. Fast,
deterministic, testable, works in CI. This is where scene construction,
modelling, and animation should happen — it is Rust, so it is quick, and the
result is a file you can diff.

**In the editor (Godot running).** The optional Godot MCP Toolkit for editing
Godot-native resources: attaching scripts to nodes, editing `.tres`, using the
editor's own undo stack. This is the one part that needs third-party tooling,
and it remains optional.

The split exists because glTF cannot express everything Godot can. Nodes,
meshes, and animations travel; script attachment and editor-only resources do
not. Trying to make one format do both is how pipelines get fragile.

## Where Blender fits

Blender is a **producer of the same glTF Aurum writes**. Use it when the job
is genuinely a DCC job:

- Sculpting, retopology, hand-authored UVs.
- Texture painting.
- Anything where an artist wants a viewport and brushes.

Do **not** use it for:

- Primitives, blockouts, and compound shapes — `aurum_mesh_add` plus
  `aurum_mesh_merge` is faster and scriptable.
- Placing objects in a scene — that is `aurum_node_add`.
- Animation of transforms — that is `aurum_animation_*`.

A mesh from Blender and a mesh from Aurum meet at the same place, so mixing
them costs nothing:

```
Blender  --export .glb-->  aurum_content_import  -->  transform / merge
                                                          |
                                          aurum_content_export
                                                          |
                                                          v
                                              Godot imports the result
```

`aurum_content_import` reads Blender's default `.glb` output directly, so an
imported asset can be placed, merged with procedural geometry, and re-exported
in one session. Nothing in the build requires Blender to be installed, and a
fresh machine with only Rust and Godot can produce a complete, textured,
animated scene.

## Commands

```pwsh
# Build and test everything (no Godot, no network)
cargo test --workspace

# The content authoring gate: generate glTF, import into Godot 4.7, assert
pwsh scripts/tests/gltf_import.ps1

# Generate a demo scene to look at
cargo run -p aurum-content --example build_demo -- out/

# Run the MCP server
aurum mcp --root . --read-only

# Phase 0 hot-reload development loop
pwsh scripts/dev.ps1

# The Studio shell: a local page that drives the same supervisor
aurum studio
```

## The Studio shell

`aurum studio` starts a small local server and opens it. It drives the same
supervisor the CLI does — one orchestration, two front ends — so anything the
page can do, `aurum doctor`, `aurum build`, `aurum editor`, and `aurum stop` can
do, and the reverse.

It is a page rather than a native window for two reasons. A toolkit such as
`eframe` would pull in well over a hundred crates, every one of them code
running with your privileges; this costs none. And a plain HTTP surface is
drivable by tests, by scripts, and by an AI, where a window is drivable only by
a person.

Three things keep a server on your own machine from becoming a way for
something else to run commands on it:

1. It binds `127.0.0.1` and nothing else. That is not configurable, because it
   is the difference between a local tool and an open door.
2. Every request needs a session token — 256 bits from the operating system's
   generator — compared in constant time. It is printed in the URL so the
   browser can be opened at it; the page then moves it into `sessionStorage`
   and strips it from the address bar, so it does not linger in history.
3. The `Host` header must be a loopback literal. A hostile page can point its
   own domain at `127.0.0.1`, and the browser will then send requests here
   carrying that domain; refusing a non-loopback `Host` ends that before the
   token is even considered.

Useful flags: `--port <n>` to fix the port, `--no-open` to start without a
browser, `--json` to print the address as data for a script to read.

## Principles

1. **Rust first.** Anything the engine does repeatedly belongs in Rust, not
   GDScript. The MCP server is Rust for this reason: an agent making a hundred
   tool calls should not pay a scripting-language tax.
2. **No dependency is a feature.** The workspace adds none for the MCP or
   content layers. Check with `cargo tree -p aurum-mcp`.
3. **Interchange over integration.** glTF is a file, not an API. Producers and
   consumers stay decoupled, which is what keeps Blender optional.
4. **The engine is not Godot.** Aurum owns its simulation in Rust. Godot is the
   editor and renderer, and could be replaced without rewriting game logic.
5. **Verify against the real thing.** A test that only checks our own JSON
   proves nothing; `gltf_import.ps1` asks Godot itself.
