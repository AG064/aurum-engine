# aurum-mcp

A headless [Model Context Protocol](https://modelcontextprotocol.io) server for
the Aurum engine.

It lets an AI model drive the engine directly — spawn entities, attach
components, emit events, read and write state, step the space simulation, and
save or load a session — with **no Godot process running**.

## Why headless

Aurum's authoritative simulation lives in Rust, not GDScript. That makes a
headless server the *higher-fidelity* surface for an agent rather than a
convenience: tool calls are typed, deterministic, fast, and runnable in CI.

This is layer 3 of
[`docs/superpowers/specs/2026-09-11-aurum-mcp-design.md`](../../docs/superpowers/specs/2026-09-11-aurum-mcp-design.md).

## Zero new dependencies

The crate adds **no external packages** to the workspace. It uses only
`aurum-core`, `aurum-space`, and the `serde` / `serde_json` / `thiserror`
dependencies the workspace already locked.

The MCP stdio transport is newline-delimited JSON-RPC 2.0 — a small enough
subset to implement directly rather than adopt a protocol SDK. You can verify
the claim yourself:

```pwsh
cargo tree -p aurum-mcp --edges normal      # only workspace crates + serde stack
```

## Running it

```pwsh
aurum mcp                                  # via the CLI
aurum mcp --root ./saves --read-only       # confined file access, no mutations
cargo run -p aurum-mcp -- --trace          # standalone, protocol echoed to stderr
```

`--root` (default: the working directory) confines `aurum_save` and
`aurum_load`. `..` escapes are refused lexically, and existing paths are
re-checked through symlinks.

### MCP client configuration

```json
{
  "mcpServers": {
    "aurum": {
      "command": "aurum",
      "args": ["mcp", "--root", "."]
    }
  }
}
```

Point it at the absolute path to `aurum.exe` if it is not on your `PATH`.

## Tools

31 tools, all prefixed `aurum_`: 12 read-only and 19 mutating. Every read-only
tool carries the `readOnlyHint` annotation, so a client can filter without a
second code path.

| Read-only | Mutating |
|---|---|
| `aurum_world_snapshot` | `aurum_entity_spawn` |
| `aurum_entity_list` | `aurum_entity_despawn` |
| `aurum_component_get` | `aurum_component_set` |
| `aurum_state_get` | `aurum_component_remove` |
| `aurum_state_list` | `aurum_event_emit` |
| `aurum_module_list` | `aurum_event_drain` |
| `aurum_fingerprint` | `aurum_state_set` |
| `aurum_space_state` | `aurum_time_set_scale` |
| `aurum_time_get` | `aurum_module_register` |
| `aurum_story_state` | `aurum_space_step` |
| `aurum_story_get_variable` | `aurum_save` |
| `aurum_story_export_state` | `aurum_load` |
| | `aurum_reset` |
| | `aurum_story_load` |
| | `aurum_story_advance` |
| | `aurum_story_pick_choice` |
| | `aurum_story_jump_to` |
| | `aurum_story_set_variable` |
| | `aurum_story_import_state` |

**Start with `aurum_world_snapshot`.** It returns the whole session — entities
and their components, global state, time scale, modules, pending event count,
the space snapshot, and the story cursor — in one call. That is deliberate: an
agent grounded in one round trip does not need an N+1 exploration loop. Entity
payloads are bounded by `entity_limit` (default 200) and report
`entities_truncated`.

### Story / visual novel

`aurum_story_load` accepts a story **file** (`path`, confined to the server
root) or **inline JSON** (`story`). `aurum_story_advance` then returns one
event per call — `Dialogue`, `Choice`, `SceneEnded`, `Quit`, `Goto`,
`Command`, or `Error` — and a `Choice` event carries its own indices, so
`aurum_story_pick_choice` needs no guessing. Story variables are `bool`,
`number`, or `string`, matching the interpreter's model.

A loaded story travels in the save payload together with its own definition, so
`aurum_save` produces a self-contained file that `aurum_load` can rebuild
without the story file still being on disk.

## Read-only mode

`--read-only` removes mutating tools from `tools/list` *and* refuses them if
called anyway. A read-only client sees a smaller, honest tool surface rather
than a set of tools that fail at call time.

## Compatibility with the Godot surface

`aurum_save` writes the same JSON shape as `AurumNode.save_to_json` —
`next_entity_id`, `time_scale`, `state`, `components`, `space`. A session saved
from a running Godot editor loads here, and vice versa. Component blobs are the
same dynamic `{ "entity id": { "TypeName": <json> } }` model, so tool
semantics match across surfaces.

## Errors

A tool that runs and fails returns `isError: true` inside a successful
JSON-RPC result, so the model can read the message and correct itself. A
malformed request, an unknown tool, or a protocol violation is a JSON-RPC
error. The distinction is the MCP specification's, and getting it backwards
makes agents retry things that cannot succeed.

## Layout

| File | Role |
|---|---|
| `protocol.rs` | JSON-RPC 2.0 / MCP wire types |
| `engine.rs` | The headless session and its save format |
| `tools.rs` | The `aurum_*` catalog, handlers, and the path guard |
| `server.rs` | The stdio loop |
| `cli.rs` | Argument parsing shared by `aurum-mcp` and `aurum mcp` |

Handlers are thin adapters. All simulation semantics live in `aurum-core` and
`aurum-space` — deliberately, so the engine keeps one source of truth.

## Testing

```pwsh
cargo test -p aurum-mcp
```

The whole protocol is exercised in memory by driving `serve()` with a `Cursor`,
so no subprocess is needed to test a full session.
