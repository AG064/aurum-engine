# Aurum Studio design

Status: approved in chat on 2026-08-31

## Summary

Aurum Studio is a Windows-first desktop control plane for developing Aurum games. It owns project discovery, environment validation, Rust builds, hot reload, Godot launch and supervision, logs, MCP lifecycle, and recovery. Godot remains the stock editor, renderer, asset pipeline, scene system, and runtime under the hood.

The primary product requirement is that routine development must not require restarting the Godot editor. GDScript, scene, resource, shader, add-on script, and safe Rust implementation changes must reload while the same editor process remains alive. Editor restarts are reserved for native structural changes that Godot cannot safely reload.

## Goals

- Give the user one Windows application named Aurum Studio for daily development.
- Import existing Aurum projects, including projects with their own build scripts.
- Create a reliable development session with one action.
- Make the normal edit, build, reload, and play loop require zero editor restarts.
- Distinguish safe reloads, gameplay restarts, and unavoidable editor restarts.
- Keep Rust, Godot, Aurum add-on, and installed DLL versions consistent.
- Manage the existing Godot MCP Toolkit without making it a required Studio dependency.
- Keep all communication local and telemetry-free.
- Preserve existing projects and unsaved editor work.

## Non-goals for the first usable version

- Rebranding or forking the Godot editor.
- Replacing Godot rendering, scenes, input, physics, or asset import.
- Supporting Linux or macOS.
- Public distribution, automatic updates, marketplace integration, or team accounts.
- Cloud project storage or remote build execution.
- Automatically rewriting imported projects without presenting the proposed changes.

## Product boundary

The user launches Aurum Studio, not the Godot Project Manager or a collection of PowerShell scripts. The first version uses a configured and verified Godot 4.7 binary. Managed runtime adoption and downloads follow in phase 4. Godot remains visible inside the editor and is credited in Studio's legal notices.

Studio remains open while the editor is running. If Studio exits, Godot continues working. Studio prompts before stopping any helper or editor process and only controls processes it launched.

## Architecture

### `aurum-studio-core`

A new Rust library containing all orchestration without UI dependencies:

- Project registry and portable project configuration.
- Path validation and project discovery.
- Godot and Rust toolchain discovery and version checks.
- Process ownership and supervision.
- Build queue and file watching.
- Aurum add-on and DLL installation.
- Reload classification.
- Local bridge protocol and authentication.
- MCP lifecycle and permission state.
- Structured logs and health diagnostics.

The core exposes typed commands and events. It must be testable with fake executables and temporary projects without starting the GUI.

### `aurum-studio`

A native Windows desktop binary built with Rust and `eframe`. The UI is an adapter over `aurum-studio-core`, so a future UI framework change does not affect orchestration.

The first interface contains:

- Project list with Import. New Project is added with templates in phase 4.
- Project health summary.
- Build, Develop, Open Editor, Run, Stop, and Restart Editor actions.
- Rust, Godot, bridge, and MCP status.
- Combined structured log view.
- Explicit MCP disabled, read-only, and writable modes.
- Clear explanation when an editor restart is required.

The UI thread never waits for builds, file operations, or child processes. A supervisor worker owns asynchronous work and sends bounded events to the UI.

### `aurum` CLI

The existing `aurum-cli` stub becomes a command-line adapter over the same core. Initial commands are:

- `aurum doctor <project>`
- `aurum build <project> [--debug|--release]`
- `aurum dev <project>`
- `aurum editor <project>`
- `aurum run <project>`
- `aurum stop <project>`

The CLI provides deterministic automation for tests, scripts, and AI agents. It must not have a second implementation of Studio behavior.

`aurum dev` is a foreground supervised session. Detached editor and game sessions receive an ownership record containing the executable path, process identifier, process start time, project path, and session identifier. `aurum stop` acts only after all ownership fields still match, preventing it from terminating an unrelated process that reused an old process identifier.

### Aurum editor bridge

A small GDScript `EditorPlugin` connects the running editor to Studio. Studio listens on an ephemeral loopback port and passes the port, protocol version, and random session token to the Godot process through environment variables.

The bridge reports:

- Editor ready and project path.
- Aurum add-on and native class availability.
- Play started and stopped.
- Current reload result.
- Whether unsaved editor state prevents a controlled restart.

Studio may request:

- Stop or start the current game.
- Rescan scripts and resources using supported editor APIs.
- Save before a user-approved controlled restart.
- Exit the editor after explicit user approval.

The bridge is optional for basic launch and build operations. Studio displays a degraded-state warning when it is absent rather than blocking the project.

### Godot MCP Toolkit

MCP stays a separate optional integration. Studio manages its process, project path, port, and read-only or writable state. Studio does not reimplement MCP tools and does not depend on MCP for builds, reloads, or editor supervision.

MCP remains bound to loopback. Writable mode is visible in the Studio UI and applies only to the selected project.

## Configuration and project discovery

Machine-specific state lives under `%LOCALAPPDATA%\AurumStudio` and includes:

- Registered project paths.
- Selected Godot installations.
- Window and UI state.
- Recent session metadata and bounded logs.

Portable project settings live in `aurum.toml` at the project root. They may include:

- Configuration schema version.
- Supported Godot version.
- Aurum engine relationship or relative path hint.
- Rust package that produces the GDExtension.
- Add-on destination.
- Enabled Aurum modules.
- Optional validation and export commands.

Absolute machine paths and session tokens never enter `aurum.toml`.

Import begins read-only. Studio discovers `project.godot`, the Aurum add-on, the GDExtension manifest, Cargo workspace, build scripts, and likely Godot binaries. It reports missing or inconsistent state before offering a migration.

## Development session lifecycle

1. The user selects a project and chooses Develop.
2. Studio canonicalizes all paths and verifies that the project remains inside its selected root.
3. Studio validates Godot, Cargo, the Aurum engine, add-on source, GDExtension manifest, and installed DLL.
4. Studio creates a session identifier, random authentication token, loopback listener, and structured log.
5. Studio starts one serialized Rust build worker and debounced file watcher.
6. Studio launches Godot with the project path and bridge environment.
7. The editor bridge authenticates and reports editor state.
8. Studio starts MCP only when it is enabled for that project.
9. File changes enter the reload policy below.
10. On exit, Studio stops only helpers it owns. Godot is left running unless the user asks Studio to close it.

Only one build may write an Aurum artifact for a project at a time. A newer filesystem event may supersede a queued build, but Studio does not terminate an active Cargo process unless the user explicitly cancels it.

## Hot reload and restart policy

### Editor restart not allowed in the normal loop

The following changes must keep the same Godot editor process alive:

- GDScript method implementation changes.
- Scene and resource changes.
- Shader changes.
- Aurum editor and runtime GDScript changes supported by Godot reload.
- Rust implementation changes that do not alter the Godot-facing native schema.
- Pure Rust simulation, state, event, and algorithm changes behind the stable `AurumNode` boundary.

Rust development builds use the debug Cargo profile. The GDExtension manifest sets `reloadable = true`. Debug and release libraries use distinct destination filenames so a development build cannot silently replace a packaged release library.

After a successful debug build, Studio waits for the output file to become stable, calculates its hash, installs add-on source changes first, stages the DLL in the destination directory, and replaces the debug DLL only when the complete staged artifact is ready. Studio then observes the editor reload result through the bridge.

### Gameplay restart allowed

Studio may stop and relaunch the running game while keeping the editor alive when a change invalidates live gameplay instances or runtime state. This is not reported as an editor restart.

The bridge requests a clean play stop, waits for confirmation, lets Godot reload the changed resources or extension, and starts play again only when automatic replay is enabled.

### Editor restart exceptional

The following may require an editor restart because native registration cannot always be migrated safely:

- Changing a registered GDExtension class parent.
- Adding or removing native classes when live registration cannot reload.
- Changing Godot-facing method, property, or signal signatures.
- Changing GDExtension initialization levels or entry symbols.
- Changing the manifest's library mapping.
- Upgrading the Godot engine or incompatible `godot-rust` API level.
- A failed native reload that leaves Godot unable to restore the previous extension.

Studio displays the exact reason and provides a controlled restart action. It never restarts automatically while the editor reports unsaved work. If Studio cannot prove that restart is safe, it asks the user to save or cancel.

The acceptance target is not a literal guarantee that native structural changes never restart. It is that routine gameplay and engine implementation work has zero editor restarts, while unavoidable native boundary changes are rare, explicit, and handled by one controlled action.

### Fallback if native reload is not reliable

Phase 0 is a decision gate. Aurum Studio must not assume that Windows GDExtension reload is reliable merely because the manifest accepts `reloadable = true`.

If repeated safe Rust reload tests expose file locking, stale code, crashes, or state corruption, the stable `AurumNode` GDExtension remains loaded and frequently changing pure Rust simulation moves behind an `aurum-runtime-host` worker process. Studio restarts that worker after a successful build while the Godot editor stays alive. The bridge reconnects and restores explicitly serializable state when available; otherwise it restarts gameplay, not the editor.

This fallback adds an IPC boundary and state-transfer cost, so it is used only if the native reload stress test fails. It still preserves the primary requirement: routine Rust development does not restart the editor.

## Build and installation integrity

- Development uses `cargo build -p aurum-godot` without `--release`.
- Release builds remain explicit packaging operations.
- Cargo runs from the verified Aurum workspace with locked dependency state when the lockfile is expected.
- A failed or cancelled build leaves the installed DLL untouched.
- Studio verifies the source artifact hash against the installed artifact hash.
- Add-on source is synchronized before the corresponding DLL is activated.
- File-copy and file-lock errors are bounded, visible failures. Studio does not kill Godot to force a copy.
- Studio records the active Cargo profile, crate, Git revision when available, source hash, destination hash, and Godot version in session diagnostics.

## IPC security

- Studio binds only to `127.0.0.1` on an ephemeral port.
- Every session uses a cryptographically random token passed through the child process environment.
- The first bridge message must authenticate and provide the supported protocol version.
- Messages use a small versioned JSON schema with bounded line and payload sizes.
- Unknown messages, invalid paths, oversized payloads, and repeated authentication failures close the connection.
- No bridge command executes an arbitrary shell string. Commands are typed operations with validated arguments.
- Tokens and environment values are redacted from logs.

## Error handling and recovery

### Build failure

Keep the last working DLL, show compiler output, and continue watching. A later edit may trigger another build.

### DLL installation failure

Keep the last working DLL and staged artifact, report the exact lock or filesystem error, and offer Retry. Do not restart or terminate the editor automatically.

### Godot crash

Keep Studio and build watchers alive, capture the exit code and relevant logs, and offer Restart Editor. Do not claim project corruption without evidence.

### Studio crash or exit

Godot remains alive. On the next Studio launch, detect the existing project process and offer either a supervised relaunch or an unmanaged continuation. Do not attach control to an unrelated process based only on its executable name.

### Bridge failure

Build and launch actions continue in degraded mode. Disable commands that require verified editor state and explain why.

### MCP failure

Report MCP separately. It must not block non-AI development.

## Delivery phases

### Phase 0: prove restart-light Aurum development

- Add reloadable debug GDExtension configuration.
- Separate debug and release library destinations.
- Change the continuous watcher to debug builds.
- Prove a safe Rust implementation change reloads in a live editor without changing the editor PID.
- Stress repeated safe reloads and choose either direct GDExtension reload or the runtime-worker fallback before building Studio supervision around it.
- Document the structural-change restart boundary.

### Phase 1: core and CLI vertical slice

- Project import and registry.
- `doctor`, `build`, `dev`, `editor`, `run`, and `stop` commands.
- Process supervision, structured logs, safe installation, and reload classification.
- Unit and fake-process integration tests.

### Phase 2: Windows Studio application

- Native project dashboard and controls.
- Background supervisor integration.
- Health and log views.
- Existing Aurum project import.

### Phase 3: editor bridge and managed MCP

- Authenticated editor state channel.
- Play restart without editor restart.
- Controlled exceptional editor restart.
- MCP status and permission controls.

### Phase 4: broader Studio management

- New project templates.
- Module management.
- Godot runtime adoption and managed downloads with verified hashes.
- Export preset management and release packaging.
- Windows installer and update strategy.

## Verification strategy

### Unit tests

- Project and engine path validation.
- Configuration parsing and schema migration.
- Reload classification.
- Process ownership records.
- Build queue serialization and event coalescing.
- Artifact stability and hash checks.
- IPC authentication, protocol versioning, and payload bounds.

### Integration tests

- Fake Cargo and Godot executables for success, failure, cancellation, crash, and log routing.
- Temporary projects for import, missing dependencies, and inconsistent add-on state.
- A failed build proving the last working DLL remains unchanged.
- A failed copy proving Studio does not terminate the editor.

### Engine validation

- Rust workspace tests.
- Godot headless import and project boot.
- Aurum native class availability.
- Existing project-specific validation where available.
- Source and installed add-on hash comparison.

### Live no-restart acceptance test

With one normal Godot editor process open:

1. Record the editor PID.
2. Change a GDScript method and observe reload.
3. Change a scene or resource and observe reload.
4. Change pure Rust simulation behavior behind the stable `AurumNode` API.
5. Build and install the debug DLL.
6. Observe successful GDExtension reload.
7. Run the changed behavior.
8. Confirm the editor PID is unchanged and logs contain no relevant script or native-extension errors.
9. Repeat safe Rust changes enough times to expose watcher, file-lock, and stale-artifact races.

Completion requires live evidence. Configuration alone is not proof of hot reload.

## Acceptance criteria for the first usable Studio

- Aurum Studio starts as a Windows desktop application.
- It imports the existing Aurum engine project without damaging current files.
- `doctor` distinguishes healthy, warning, and blocked state with evidence.
- Develop launches the correct project, build watcher, bridge, and optional MCP service.
- Build errors are visible and never replace the working DLL.
- GDScript and safe Rust implementation changes complete without restarting the editor.
- Gameplay may restart independently of the editor.
- Native structural changes produce a specific exceptional-restart reason.
- Studio never discards unsaved work or terminates an unrelated process.
- Telemetry remains absent and all control traffic remains local.
