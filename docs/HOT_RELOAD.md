# Aurum hot reload

## Phase 0 decision

Direct GDExtension reload is the selected Phase 0 architecture. Godot 4.7 completed five consecutive safe Rust implementation reloads in one editor process.

The accepted path is the ordinary Aurum watcher and editor integration. The
acceptance harness did not call `GDExtensionManager.reload_extension` itself.

## No editor restart

The normal Aurum development loop keeps the Godot editor alive for:

- GDScript implementation changes.
- Scene, resource, and shader changes supported by Godot reload.
- Pure Rust simulation and algorithm changes behind the stable `AurumNode` API.
- Rust method-body changes that do not alter registered Godot classes or method signatures.

Development uses `aurum_godot.debug.dll`. Release packaging uses `aurum_godot.dll`.

## How native reload is triggered

After `build.ps1` validates a staged debug DLL and atomically installs it, it
publishes that staged SHA-256 hash to
`.godot/aurum/aurum_godot.debug.reload`. Marker publication failure is reported
as a warning and does not invalidate an already verified DLL installation.

The enabled Aurum `EditorPlugin` polls the marker at a bounded interval and
waits for a stable debounce. For each new hash it requires exactly one loaded
Aurum manifest, calls `GDExtensionManager.reload_extension`, and accepts only
Godot status `OK`. Reentrancy and same-hash failure guards prevent retry storms.
A failed native reload warns that a controlled editor restart may be required.

## Gameplay restart only

A running game may be stopped and relaunched after a change invalidates live scene instances. This does not restart the editor.

## Exceptional editor restart

An editor restart may still be required after changing native class registration, inheritance, Godot-facing methods, properties, signals, initialization levels, entry symbols, library mappings, or the Godot API version.

## Commands

```powershell
pwsh scripts/dev.ps1
pwsh scripts/dev.ps1 -RunEditor
pwsh scripts/tests/phase0_contract.ps1
pwsh scripts/tests/debug_reload_product_contract.ps1
pwsh scripts/tests/phase0_hot_reload_smoke.ps1 -Iterations 5
```

Build failures and DLL preparation failures keep the last working installed DLL.
After an atomic DLL replacement returns successfully, the build treats that
replacement as installed and product-marker publication remains nonfatal. These
paths do not terminate the editor.

## Acceptance evidence

The product-path gate used `dev.ps1 -Once` for each install and the enabled
Aurum plugin for reload. Evidence is stored at
`target/aurum-hot-reload-smoke/b809ef3d704f4e5e8c49d2a5e2ae2044/evidence.json`.
Schema 3 records initial state plus five reloads under editor PID `50228`, six exact
requested and observed fingerprints, six distinct DLL hashes, six matching
marker hashes, ten of ten Inspector cleanups, verified editor shutdown, and no
recorded error. Inspector provenance is bound to the uniquely cached
`@modelcontextprotocol/inspector@2.4.0` package and its verified JavaScript entry
SHA-256 `21CA6E6E031713E5AE040ECF71F8FD0B0EF9A4ECE4131FD03D3B01AEAE5415F4`.
