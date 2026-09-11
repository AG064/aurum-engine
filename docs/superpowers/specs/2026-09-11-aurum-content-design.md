# Aurum content authoring design

Status: draft, 2026-09-11
Builds on: `docs/superpowers/specs/2026-09-11-aurum-mcp-design.md` (the MCP
stack), `docs/HOT_RELOAD.md` (the development loop)

## Summary

Aurum must be authorable by an AI: meshes, scenes, animations, sprites, and
scripts. The work must be **Rust-first** so it is fast and testable, and
**Blender must never be required**.

The decision is that **glTF 2.0 is the interchange boundary**, and that Aurum
owns a Rust content model that writes it.

| Want | Aurum-native (Rust, always available) | Optional accelerator |
|---|---|---|
| Modelling | Procedural mesh generation → glTF | Blender MCP → glTF |
| Scene work, placing objects | Aurum scene graph → glTF | Godot editor via MCP Toolkit |
| Animation | Aurum animation tracks → glTF | Blender MCP → glTF |
| Sprite work | Rust atlas packing → PNG + layout JSON | Aseprite / external tools |
| Scripting | Rust modules, plus file-level GDScript tools | — |
| Runtime control | `aurum-mcp` (already built) | Toolkit runtime server |

Blender appears only in the right-hand column. Removing that column leaves a
complete, working pipeline.

## Why glTF

1. **Rust can write it with zero dependencies.** glTF is JSON plus one binary
   buffer. `serde_json` is already locked in the workspace, so the exporter
   costs nothing in supply-chain terms — which matters, given the project's
   dependency policy.
2. **Godot imports it natively.** glTF is Godot's first-class interchange
   format, so no custom importer is needed and no `.tscn` is hand-written.
3. **Blender exports it natively.** That is what makes Blender optional
   rather than load-bearing: it becomes one possible producer of a format
   Aurum already speaks.
4. **It carries all four asset kinds.** Meshes, materials, node hierarchies
   (scenes), and animations are all core glTF. Nothing needs a side channel.

### Why not write `.tscn` directly

Phase 0's research recorded the reason: `.tscn`, `.tres`, and `project.godot`
are fragile to hand-edit because UIDs, subresources, and signal blocks must
stay consistent, and a malformed file fails at load with poor diagnostics.

Godot imports glTF through its own importer, which owns that complexity. Aurum
therefore never emits a Godot-native scene file; it emits glTF and lets Godot
convert. The MCP Toolkit remains available for editing Godot-native resources
when that is genuinely what is wanted.

## Architecture

```
aurum-content (Rust, zero external deps)
├── mesh.rs      procedural geometry: primitives, transforms, merge, normals
├── scene.rs     node graph, transforms, materials
├── anim.rs      tracks, keyframes, interpolation
├── sprite.rs    atlas packing, PNG encoding
└── gltf.rs      glTF 2.0 exporter (JSON + .bin)

        │  .gltf + .bin  /  .png + .json
        ▼
Godot importer  ──►  .tscn / Mesh / Animation resources
        ▲
        │  .gltf
Blender (optional)
```

`aurum-mcp` exposes the content model as `aurum_content_*` tools, so an agent
composes geometry, scenes, and animations headlessly and writes glTF — with no
Godot process and no Blender install.

## Scripting

Scripting stays file-level plus the existing runtime surface:

- Rust systems and modules are the fast path, and the engine's whole point.
- GDScript files can be created and edited as text through the MCP file tools.
- Attaching a script to a Godot node is a Godot-native resource edit, so it
  goes through the MCP Toolkit rather than through glTF (glTF nodes can carry
  `extras`, and Godot maps a `script` extension, but relying on that is more
  fragile than using the toolkit).

## Non-goals

- A full DCC modeller. Aurum generates primitives, extrusions, lathes, and
  merged/transformed geometry. Sculpting, retopology, and UV unwrapping by
  hand remain Blender's job — and remain optional.
- Replacing Godot's importer, renderer, or resource system.
- Requiring any external process for the baseline pipeline.

## Phasing

- **C1 — Content model.** `aurum-content` with mesh primitives, scene graph,
  materials, animation tracks, and a glTF 2.0 exporter. Tests assert the
  emitted JSON against the glTF spec's structural rules.
- **C2 — Sprites.** Atlas packing and a dependency-free PNG encoder, so sprite
  work needs no external image library.
- **C3 — MCP surface.** `aurum_content_*` tools: create primitives, transform,
  merge, build scenes, author animations, export glTF and atlases.
- **C4 — Godot verification.** Import a generated glTF in Godot 4.7 headless
  and assert the mesh, materials, and animations load.
- **C5 — Blender bridge (optional).** Document the Blender MCP path, and add a
  glTF ingest so a Blender-authored mesh can be loaded back into the Aurum
  content model for further Rust-side processing.
