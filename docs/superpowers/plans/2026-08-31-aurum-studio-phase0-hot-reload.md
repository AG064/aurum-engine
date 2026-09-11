# Aurum Studio Phase 0 Hot Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove a repeatable Windows development loop in which safe Rust changes reload inside one live Godot 4.7 editor process, then record whether Aurum Studio should supervise direct GDExtension reload or the runtime-worker fallback.

**Architecture:** Keep `AurumNode` as the stable Godot-facing boundary. Development builds produce a dedicated debug DLL declared reloadable in the GDExtension manifest, while release builds keep their existing filename. A compile-time runtime fingerprint and an isolated Godot test project let the MCP Toolkit query the live native implementation repeatedly and prove that the editor PID never changes.

**Tech Stack:** Rust 1.95 and Cargo, `godot-rust` 0.5.4 at the locked Git revision, Godot 4.7 stable, PowerShell 7, Godot MCP Toolkit 1.0.0, MCP Inspector CLI, Windows x86-64.

**Spec:** `docs/superpowers/specs/2026-08-31-aurum-studio-design.md`

## Global Constraints

- Windows x86-64 is the only Phase 0 target.
- Routine GDScript and safe Rust implementation changes must keep the same Godot editor PID alive.
- Native class, method, property, signal, inheritance, initialization-level, and manifest mapping changes remain exceptional restart cases.
- Direct reload is not accepted until five consecutive fingerprint changes succeed in one editor process.
- If direct reload remains unreliable after three isolated root-cause fixes, stop and select the `aurum-runtime-host` fallback from the spec.
- Development uses the Cargo debug profile. Release builds remain explicit.
- `aurum_godot.debug.dll` is the Windows editor development artifact. `aurum_godot.dll` remains the Windows release artifact.
- Failed builds or failed installation must leave the last working DLL intact.
- Add-on source is installed before the corresponding DLL, then source and destination DLL hashes must match.
- No test may terminate a Godot process it did not start and identify by executable path, PID, process start time, and project path.
- MCP remains loopback-only and telemetry-free.
- Preserve the current dirty checkout. Do not reset, stash, checkout, or overwrite pre-existing changes.
- `Cargo.toml`, `Cargo.lock`, `crates/aurum-godot/Cargo.toml`, `crates/aurum-godot/src/lib.rs`, `godot/project.godot`, and `godot/scripts/aurum_runtime.gd` already contain user work. Review their diffs before every overlapping edit.
- Do not stage or commit in the current checkout without explicit user authorization. Each task ends with a scoped diff checkpoint instead of an automatic commit.
- Keep the current file encoding and line endings. In particular, do not rewrite `Cargo.lock` or `godot/project.godot` as text.
- Generated smoke projects and evidence live under ignored `target/aurum-hot-reload-smoke/`.

## Scope boundary

This plan implements Phase 0 only. It must finish with a direct-reload pass or a documented direct-reload failure that selects the runtime-worker architecture. The Studio core, CLI, desktop UI, editor bridge, and managed MCP lifecycle each receive a separate implementation plan after this decision because their process model depends on the result.

## File map

- `godot/addons/aurum/bin/aurum.gdextension`: Windows debug/release library selection and reloadable flag.
- `scripts/build.ps1`: profile-specific artifact installation, staging, atomic replacement, and hash verification.
- `scripts/dev.ps1`: self-contained debug watcher with no `cargo-watch` dependency.
- `scripts/tests/phase0_contract.ps1`: static executable contract for manifest and scripts.
- `scripts/tests/dev_once_smoke.ps1`: real debug build and installed-hash smoke test.
- `crates/aurum-godot/src/build_info.rs`: compile-time runtime fingerprint with unit tests.
- `crates/aurum-godot/src/lib.rs`: stable `AurumNode.runtime_fingerprint()` Godot API.
- `godot/scripts/aurum_runtime.gd`: GDScript fingerprint proxy for projects that use the autoload.
- `scripts/tests/runtime_fingerprint_build.ps1`: proves Cargo recompiles when the fingerprint environment changes.
- `scripts/tests/phase0_hot_reload_smoke.ps1`: isolated live-editor, MCP, repeated-reload, and PID test.
- `docs/HOT_RELOAD.md`: user-facing reload boundary and selected Phase 0 result.
- `README.md`: accurate reload-time summary.
- `docs/GETTING_STARTED.md`: self-contained development instructions with no restart-by-default claim.

---

### Task 1: Lock the debug and release artifact contract

**Files:**
- Create: `scripts/tests/phase0_contract.ps1`
- Modify: `godot/addons/aurum/bin/aurum.gdextension:1-11`
- Modify: `scripts/build.ps1:113-197`
- Test: `scripts/tests/phase0_contract.ps1`

**Interfaces:**
- Consumes: current `aurum-godot` Cargo output at `target/debug/aurum_godot.dll` and `target/release/aurum_godot.dll`.
- Produces: `aurum_godot.debug.dll` for Windows editor development and `aurum_godot.dll` for Windows release use.
- Produces: `reloadable = true` in the source-of-truth GDExtension manifest.

- [ ] **Step 1: Re-read overlapping files and record the pre-task diff**

Run:

```powershell
git diff -- godot/addons/aurum/bin/aurum.gdextension scripts/build.ps1
git status --short
```

Expected: the two task files have no pre-existing tracked edits. The unrelated dirty files remain visible and untouched.

- [ ] **Step 2: Write the failing executable contract**

Create `scripts/tests/phase0_contract.ps1` with:

```powershell
[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$failures = [System.Collections.Generic.List[string]]::new()

function Require-Match {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Failure
    )
    if ($Text -notmatch $Pattern) {
        $failures.Add($Failure)
    }
}

function Require-NotMatch {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Failure
    )
    if ($Text -match $Pattern) {
        $failures.Add($Failure)
    }
}

$manifestPath = Join-Path $WorkspaceRoot "godot\addons\aurum\bin\aurum.gdextension"
$buildPath = Join-Path $WorkspaceRoot "scripts\build.ps1"
$devPath = Join-Path $WorkspaceRoot "scripts\dev.ps1"

$manifest = Get-Content -Raw -LiteralPath $manifestPath
$build = Get-Content -Raw -LiteralPath $buildPath
$dev = Get-Content -Raw -LiteralPath $devPath

Require-Match $manifest '(?m)^reloadable\s*=\s*true\s*$' `
    "GDExtension manifest must set reloadable = true"
Require-Match $manifest '(?m)^windows\.debug\.x86_64\s*=\s*"res://addons/aurum/bin/aurum_godot\.debug\.dll"\s*$' `
    "Windows debug mapping must use aurum_godot.debug.dll"
Require-Match $manifest '(?m)^windows\.release\.x86_64\s*=\s*"res://addons/aurum/bin/aurum_godot\.dll"\s*$' `
    "Windows release mapping must keep aurum_godot.dll"
Require-Match $build 'aurum_godot\.debug\.dll' `
    "build.ps1 must install a distinct Windows debug DLL"
Require-Match $build 'aurum_godot\.dll' `
    "build.ps1 must preserve the Windows release DLL name"
Require-Match $dev '\[switch\]\$Once' `
    "dev.ps1 must provide a one-build smoke mode"
Require-Match $dev '-DebugBuild' `
    "dev.ps1 must call the debug build path"
Require-NotMatch $dev 'cargo-watch' `
    "dev.ps1 must not require cargo-watch"
Require-NotMatch $dev 'build\s+--release\s+-p\s+aurum-godot' `
    "dev.ps1 must not use release builds in the development loop"

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Host "PHASE0_CONTRACT_OK"
```

- [ ] **Step 3: Run the contract and confirm the expected failures**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_contract.ps1
```

Expected: exit 1. Failures include missing `reloadable = true`, shared debug/release DLL name, `cargo-watch`, release-mode development build, and missing `-Once`.

- [ ] **Step 4: Update the Windows library mappings**

Change the manifest configuration to:

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = "4.6"
reloadable = true

[libraries]
windows.debug.x86_64 = "res://addons/aurum/bin/aurum_godot.debug.dll"
windows.release.x86_64 = "res://addons/aurum/bin/aurum_godot.dll"
linux.debug.x86_64 = "res://addons/aurum/bin/libaurum_godot.so"
linux.release.x86_64 = "res://addons/aurum/bin/libaurum_godot.so"
macos.debug = "res://addons/aurum/bin/libaurum_godot.framework"
macos.release = "res://addons/aurum/bin/libaurum_godot.framework"
```

Do not rename untested Linux or macOS files in the Windows-only phase.

- [ ] **Step 5: Make build installation profile-specific and transactional**

After selecting `$DllSource`, set the destination name explicitly:

```powershell
if ($DebugBuild) {
    $DllSource = Join-Path $Workspace "target\debug\aurum_godot.dll"
    $DllTargetName = "aurum_godot.debug.dll"
} else {
    $DllSource = Join-Path $Workspace "target\release\aurum_godot.dll"
    $DllTargetName = "aurum_godot.dll"
}
```

Replace the current direct-copy loop with this complete staging block:

```powershell
$DllTarget = Join-Path $AddOnBin $DllTargetName
$StagedTarget = "$DllTarget.stage.$PID"
$BackupTarget = "$DllTarget.backup.$PID"

$source_hash = $null
for ($stable_attempt = 0; $stable_attempt -lt 10; $stable_attempt++) {
    $first_hash = (Get-FileHash -LiteralPath $DllSource -Algorithm SHA256).Hash
    Start-Sleep -Milliseconds 250
    $second_hash = (Get-FileHash -LiteralPath $DllSource -Algorithm SHA256).Hash
    if ($first_hash -eq $second_hash) {
        $source_hash = $second_hash
        break
    }
}
if ([string]::IsNullOrWhiteSpace($source_hash)) {
    throw "$Profile DLL did not become stable: $DllSource"
}

Copy-Item -LiteralPath $DllSource -Destination $StagedTarget -Force
$staged_hash = (Get-FileHash -LiteralPath $StagedTarget -Algorithm SHA256).Hash
if ($staged_hash -ne $source_hash) {
    throw "Staged DLL hash does not match source: $StagedTarget"
}

$installed = $false
for ($copy_attempt = 1; $copy_attempt -le 5; $copy_attempt++) {
    try {
        if (Test-Path -LiteralPath $DllTarget) {
            [System.IO.File]::Replace($StagedTarget, $DllTarget, $BackupTarget, $true)
        } else {
            [System.IO.File]::Move($StagedTarget, $DllTarget)
        }
        $target_hash = (Get-FileHash -LiteralPath $DllTarget -Algorithm SHA256).Hash
        if ($target_hash -ne $source_hash) {
            if (Test-Path -LiteralPath $BackupTarget) {
                [System.IO.File]::Replace($BackupTarget, $DllTarget, $null, $true)
            }
            throw "Installed DLL hash does not match source: $DllTarget"
        }
        $installed = $true
        break
    } catch {
        if ($copy_attempt -eq 5) {
            throw "Could not install $DllTargetName without replacing the last working DLL: $($_.Exception.Message)"
        }
        if (-not (Test-Path -LiteralPath $StagedTarget)) {
            Copy-Item -LiteralPath $DllSource -Destination $StagedTarget -Force
        }
        Start-Sleep -Milliseconds 250
    }
}

if (-not $installed) {
    throw "DLL installation did not complete: $DllTarget"
}
if (Test-Path -LiteralPath $BackupTarget) {
    Remove-Item -LiteralPath $BackupTarget -Force
}
Write-Host "==> Installed $DllTargetName to $DllTarget" -ForegroundColor Green
```

Keep the existing add-on source copy before this block.

- [ ] **Step 6: Run the static contract and isolate the remaining dev-script failures**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_contract.ps1
```

Expected: exit 1 only for `dev.ps1` requirements addressed in Task 2. The manifest and build-script assertions pass.

- [ ] **Step 7: Record the scoped checkpoint**

Run:

```powershell
git diff --check -- godot/addons/aurum/bin/aurum.gdextension scripts/build.ps1 scripts/tests/phase0_contract.ps1
git diff -- godot/addons/aurum/bin/aurum.gdextension scripts/build.ps1
git status --short -- scripts/tests/phase0_contract.ps1
```

Expected: no whitespace errors and only the three task files appear. Do not stage or commit.

---

### Task 2: Replace the unavailable cargo-watch loop

**Files:**
- Modify: `scripts/dev.ps1:1-129`
- Create: `scripts/tests/dev_once_smoke.ps1`
- Test: `scripts/tests/phase0_contract.ps1`
- Test: `scripts/tests/dev_once_smoke.ps1`

**Interfaces:**
- Consumes: `scripts/build.ps1 -DebugBuild -NoTests` from Task 1.
- Produces: `scripts/dev.ps1 -Once` for one debug build and normal no-argument watch mode for continuous debug builds.
- Produces: metadata-based source stamps over Rust and Cargo inputs with a stable debounce window.

- [ ] **Step 1: Write the failing one-build smoke test**

Create `scripts/tests/dev_once_smoke.ps1` with:

```powershell
[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$project = Join-Path $workspace "target\phase0-dev-once-project"
$projectAddonBin = Join-Path $project "addons\aurum\bin"
New-Item -ItemType Directory -Path $projectAddonBin -Force | Out-Null

& (Join-Path $workspace "scripts\dev.ps1") `
    -Once `
    -GodotProject $project `
    -GodotBinary $GodotBinary
if ($LASTEXITCODE -ne 0) {
    throw "dev.ps1 -Once failed with exit code $LASTEXITCODE"
}

$source = Join-Path $workspace "target\debug\aurum_godot.dll"
$installed = Join-Path $projectAddonBin "aurum_godot.debug.dll"
if (-not (Test-Path -LiteralPath $installed)) {
    throw "Debug DLL was not installed: $installed"
}

$sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
$installedHash = (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
if ($sourceHash -ne $installedHash) {
    throw "Debug DLL hash mismatch: source=$sourceHash installed=$installedHash"
}

Write-Host "DEV_ONCE_SMOKE_OK hash=$installedHash"
```

- [ ] **Step 2: Run the smoke test and confirm the current dependency failure**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/dev_once_smoke.ps1
```

Expected: exit 1 because current `dev.ps1` has no `-Once` parameter and the machine has no `cargo-watch` command.

- [ ] **Step 3: Extend the parameters and add source-stamp helpers**

Replace the existing parameter block with:

```powershell
[CmdletBinding()]
param(
    [switch]$RunEditor,
    [switch]$Once,
    [ValidateRange(100, 5000)]
    [int]$PollMilliseconds = 250,
    [ValidateRange(100, 10000)]
    [int]$DebounceMilliseconds = 350,
    [string]$GodotProject,
    [string]$GodotBinary
)
```

Add these functions after `Find-AurumGodot`:

```powershell
function Get-AurumSourceStamp {
    param([string]$WorkspaceRoot)

    $files = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    foreach ($name in @("Cargo.toml", "Cargo.lock")) {
        $path = Join-Path $WorkspaceRoot $name
        if (Test-Path -LiteralPath $path) {
            $files.Add((Get-Item -LiteralPath $path))
        }
    }
    $crates = Join-Path $WorkspaceRoot "crates"
    if (Test-Path -LiteralPath $crates) {
        Get-ChildItem -LiteralPath $crates -Recurse -File |
            Where-Object { $_.Extension -in @(".rs", ".toml") } |
            ForEach-Object { $files.Add($_) }
    }

    $rows = $files |
        Sort-Object FullName |
        ForEach-Object {
            "$($_.FullName)|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)"
        }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($rows -join "`n"))
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToHexString($sha.ComputeHash($bytes))
    } finally {
        $sha.Dispose()
    }
}

function Invoke-AurumDebugBuild {
    param(
        [string]$WorkspaceRoot,
        [string]$ProjectPath,
        [string]$GodotPath
    )

    try {
        & (Join-Path $PSScriptRoot "build.ps1") `
            -DebugBuild `
            -NoTests `
            -Workspace $WorkspaceRoot `
            -GodotProject $ProjectPath `
            -GodotBinary $GodotPath
        return $LASTEXITCODE -eq 0
    } catch {
        Write-Warning "Aurum debug build failed: $($_.Exception.Message)"
        return $false
    }
}
```

- [ ] **Step 4: Replace cargo-watch execution with a self-contained loop**

Remove `$AddOnBinLinux`, the `Get-Command cargo-watch` gate, `$WatchArgs`, and `& cargo @WatchArgs`. Use:

```powershell
Write-Host "==> Aurum dev mode" -ForegroundColor Cyan
Write-Host "    Watching: $WorkspaceRoot\crates, Cargo.toml, Cargo.lock"
Write-Host "    Profile:  debug"
Write-Host "    Godot:    $GodotBinary"

$initialBuildOk = Invoke-AurumDebugBuild `
    -WorkspaceRoot $WorkspaceRoot `
    -ProjectPath $GodotProject `
    -GodotPath $GodotBinary

if ($Once) {
    if (-not $initialBuildOk) {
        throw "Initial Aurum debug build failed"
    }
    Write-Host "==> One debug build completed." -ForegroundColor Green
    return
}

if ($RunEditor -and $initialBuildOk) {
    Start-Process `
        -FilePath $GodotBinary `
        -ArgumentList @("--editor", "--path", "`"$GodotProject`"") |
        Out-Null
}

$lastStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
$pendingSince = $null
Write-Host "    Watching for changes. Press Ctrl+C to stop."

while ($true) {
    Start-Sleep -Milliseconds $PollMilliseconds
    $currentStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
    if ($currentStamp -ne $lastStamp) {
        $lastStamp = $currentStamp
        $pendingSince = [DateTime]::UtcNow
        continue
    }
    if ($null -eq $pendingSince) {
        continue
    }
    $stableFor = ([DateTime]::UtcNow - $pendingSince).TotalMilliseconds
    if ($stableFor -lt $DebounceMilliseconds) {
        continue
    }

    $pendingSince = $null
    $null = Invoke-AurumDebugBuild `
        -WorkspaceRoot $WorkspaceRoot `
        -ProjectPath $GodotProject `
        -GodotPath $GodotBinary
    $lastStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
}
```

Update the file header to say it uses PowerShell polling, debug builds, and in-editor reload. Remove installation instructions for `cargo-watch`.

- [ ] **Step 5: Run the contract and real one-build smoke**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_contract.ps1
pwsh -NoProfile -File scripts/tests/dev_once_smoke.ps1
```

Expected:

```text
PHASE0_CONTRACT_OK
DEV_ONCE_SMOKE_OK hash= followed by exactly 64 hexadecimal characters
```

- [ ] **Step 6: Record the scoped checkpoint**

Run:

```powershell
git diff --check -- scripts/dev.ps1 scripts/tests/dev_once_smoke.ps1 scripts/tests/phase0_contract.ps1
git diff -- scripts/dev.ps1
git status --short -- scripts/tests/dev_once_smoke.ps1 scripts/tests/phase0_contract.ps1
```

Expected: only the planned script changes. Do not stage or commit.

---

### Task 3: Add a stable runtime fingerprint API

**Files:**
- Create: `crates/aurum-godot/src/build_info.rs`
- Modify: `crates/aurum-godot/src/lib.rs:43-49,117-165`
- Modify: `godot/scripts/aurum_runtime.gd:48-68`
- Create: `scripts/tests/runtime_fingerprint_build.ps1`
- Test: `crates/aurum-godot/src/build_info.rs`
- Test: `scripts/tests/runtime_fingerprint_build.ps1`

**Interfaces:**
- Consumes: compile-time environment variable `AURUM_RUNTIME_FINGERPRINT`.
- Produces: `build_info::runtime_fingerprint() -> &'static str`.
- Produces: Godot method `AurumNode.runtime_fingerprint() -> GString` with a stable signature.
- Produces: GDScript method `Aurum.runtime_fingerprint() -> String`.

- [ ] **Step 1: Re-read and preserve overlapping user changes**

Run:

```powershell
git diff -- crates/aurum-godot/src/lib.rs godot/scripts/aurum_runtime.gd
```

Expected: the existing `aurum-space`, event-queue, and `AurumNode` work is visible. The new method must be inserted without rewriting or reverting those hunks.

- [ ] **Step 2: Write a failing unit test for the fingerprint module**

Create `crates/aurum-godot/src/build_info.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::runtime_fingerprint;

    #[test]
    fn runtime_fingerprint_is_never_empty() {
        assert!(!runtime_fingerprint().is_empty());
    }
}
```

Add this module declaration beside `mod bridge;` in `lib.rs`:

```rust
mod bridge;
mod build_info;
```

- [ ] **Step 3: Run the focused unit test and verify it fails**

Run:

```powershell
cargo test -p aurum-godot --lib build_info::tests::runtime_fingerprint_is_never_empty
```

Expected: compilation fails because `runtime_fingerprint` is not defined.

- [ ] **Step 4: Implement the minimal fingerprint function**

Place this before the test module:

```rust
pub(crate) fn runtime_fingerprint() -> &'static str {
    option_env!("AURUM_RUNTIME_FINGERPRINT").unwrap_or("aurum-unmanaged")
}
```

Cargo automatically tracks environment variables referenced through `option_env!`, so changing the value causes recompilation without a custom build script.

- [ ] **Step 5: Expose the stable Godot and GDScript methods**

Inside the existing `#[godot_api] impl AurumNode`, insert before the module-registration section:

```rust
    // ===== Build diagnostics =====

    /// Return the compile-time identifier of the loaded development runtime.
    #[func]
    fn runtime_fingerprint(&self) -> GString {
        GString::from(build_info::runtime_fingerprint())
    }
```

In `godot/scripts/aurum_runtime.gd`, insert before module registration:

```gdscript
# ===== Build diagnostics =====

func runtime_fingerprint() -> String:
	if _engine == null:
		return ""
	return _engine.runtime_fingerprint()
```

- [ ] **Step 6: Prove Cargo rebuilds for fingerprint changes**

Create `scripts/tests/runtime_fingerprint_build.ps1` with:

```powershell
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$dll = Join-Path $workspace "target\debug\aurum_godot.dll"
$original = [Environment]::GetEnvironmentVariable("AURUM_RUNTIME_FINGERPRINT", "Process")

try {
    $env:AURUM_RUNTIME_FINGERPRINT = "phase0-fingerprint-a"
    & cargo build -p aurum-godot --manifest-path (Join-Path $workspace "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "First fingerprint build failed" }
    $first = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash

    $env:AURUM_RUNTIME_FINGERPRINT = "phase0-fingerprint-b"
    & cargo build -p aurum-godot --manifest-path (Join-Path $workspace "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Second fingerprint build failed" }
    $second = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash

    if ($first -eq $second) {
        throw "Changing AURUM_RUNTIME_FINGERPRINT did not change the DLL hash"
    }
    Write-Host "RUNTIME_FINGERPRINT_BUILD_OK first=$first second=$second"
} finally {
    if ($null -eq $original) {
        Remove-Item Env:AURUM_RUNTIME_FINGERPRINT -ErrorAction SilentlyContinue
    } else {
        $env:AURUM_RUNTIME_FINGERPRINT = $original
    }
}
```

- [ ] **Step 7: Run focused tests and formatting**

Run:

```powershell
cargo test -p aurum-godot --lib build_info::tests::runtime_fingerprint_is_never_empty
pwsh -NoProfile -File scripts/tests/runtime_fingerprint_build.ps1
cargo fmt --check
```

Expected: the Rust test passes, the script prints two different hashes, and formatting passes.

- [ ] **Step 8: Record the scoped checkpoint**

Run:

```powershell
git diff --check -- crates/aurum-godot/src/build_info.rs crates/aurum-godot/src/lib.rs godot/scripts/aurum_runtime.gd scripts/tests/runtime_fingerprint_build.ps1
git diff -- crates/aurum-godot/src/lib.rs godot/scripts/aurum_runtime.gd
git status --short -- crates/aurum-godot/src/build_info.rs scripts/tests/runtime_fingerprint_build.ps1
```

Expected: existing user changes remain, with only the focused fingerprint additions layered on top. Do not stage or commit.

---

### Task 4: Build an isolated live-editor reload smoke test

**Files:**
- Create: `scripts/tests/phase0_hot_reload_smoke.ps1`
- Test: generated project under a session-specific subdirectory of `target/aurum-hot-reload-smoke/`.
- Evidence: generated `evidence.json` in the same session-specific subdirectory.

**Interfaces:**
- Consumes: debug installer from Task 1 and `AurumNode.runtime_fingerprint()` from Task 3.
- Consumes: eager MCP tools `scene_open`, `node_call_method`, and `extensions_refresh` through the Inspector CLI.
- Produces: evidence containing editor executable, PID, start time, project path, requested fingerprints, observed fingerprints, DLL hashes, and pass/fail state.

- [ ] **Step 1: Create the smoke script with strict process ownership**

Create `scripts/tests/phase0_hot_reload_smoke.ps1` with:

```powershell
[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe",
    [ValidateRange(1, 20)]
    [int]$Iterations = 5,
    [ValidateRange(30, 600)]
    [int]$TimeoutSeconds = 180,
    [switch]$KeepEditorOpen
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$sessionId = [Guid]::NewGuid().ToString("N")
$sessionRoot = Join-Path $workspace "target\aurum-hot-reload-smoke\$sessionId"
$projectRoot = Join-Path $sessionRoot "project"
$addonsRoot = Join-Path $projectRoot "addons"
$evidencePath = Join-Path $sessionRoot "evidence.json"
$mcpConfigPath = Join-Path $sessionRoot "mcp.json"
$editor = $null
$editorStartTime = $null
$originalFingerprint = [Environment]::GetEnvironmentVariable("AURUM_RUNTIME_FINGERPRINT", "Process")
$observations = [System.Collections.Generic.List[object]]::new()

function Write-Utf8File {
    param([string]$Path, [string]$Content)
    $parent = Split-Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Stop-OwnedInspectorTree {
    param(
        [int]$RootPid,
        [datetime]$RootStartTime
    )

    $root = Get-Process -Id $RootPid -ErrorAction SilentlyContinue
    if ($null -eq $root) { return }
    if ($root.StartTime -ne $RootStartTime) {
        throw "Refusing to stop Inspector PID ${RootPid}: process start time changed"
    }

    $snapshot = @(Get-CimInstance Win32_Process)
    $rootCim = $snapshot |
        Where-Object { [int]$_.ProcessId -eq $RootPid } |
        Select-Object -First 1
    if ($null -eq $rootCim -or
        [string]$rootCim.CommandLine -notmatch '@modelcontextprotocol/inspector' -or
        [string]$rootCim.CommandLine -notlike "*$mcpConfigPath*") {
        throw "Refusing to stop Inspector PID ${RootPid}: command line ownership check failed"
    }

    $tree = [System.Collections.Generic.List[int]]::new()
    $frontier = @($RootPid)
    while ($frontier.Count -gt 0) {
        $children = @(
            $snapshot |
                Where-Object { $frontier -contains [int]$_.ParentProcessId } |
                ForEach-Object { [int]$_.ProcessId }
        )
        foreach ($child in $children) {
            if (-not $tree.Contains($child)) { $tree.Add($child) }
        }
        $frontier = $children
    }

    $descendants = $tree.ToArray()
    [Array]::Reverse($descendants)
    foreach ($processId in $descendants) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
    Stop-Process -Id $RootPid -Force -ErrorAction SilentlyContinue
}

function Invoke-InspectorRequest {
    param(
        [string]$Method,
        [string]$ToolName,
        [hashtable]$Arguments
    )

    $npx = (Get-Command npx.cmd -ErrorAction Stop).Source
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $npx
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $inspectorArgs = [System.Collections.Generic.List[string]]::new()
    foreach ($value in @(
        '--yes',
        '@modelcontextprotocol/inspector',
        '--cli',
        '--config', $mcpConfigPath,
        '--server', 'godot',
        '--method', $Method,
        '--format', 'json'
    )) {
        $inspectorArgs.Add($value)
    }
    if (-not [string]::IsNullOrWhiteSpace($ToolName)) {
        $inspectorArgs.Add('--tool-name')
        $inspectorArgs.Add($ToolName)
        $inspectorArgs.Add('--tool-args-json')
        $inspectorArgs.Add(($Arguments | ConvertTo-Json -Compress -Depth 20))
    }
    foreach ($value in $inspectorArgs) {
        $startInfo.ArgumentList.Add($value)
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Could not start MCP Inspector"
    }
    $processStartTime = $process.StartTime
    $stdoutTask = $process.StandardOutput.ReadLineAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $responseLine = $null
    $requestFailure = $null

    try {
        if (-not $stdoutTask.Wait(15000)) {
            $requestFailure = "MCP Inspector produced no JSON response within 15 seconds"
        } else {
            $responseLine = $stdoutTask.GetAwaiter().GetResult()
            if ([string]::IsNullOrWhiteSpace($responseLine)) {
                $requestFailure = "MCP Inspector closed stdout without a JSON response"
            }
        }
    } catch {
        $requestFailure = $_.Exception.Message
    } finally {
        Stop-OwnedInspectorTree -RootPid $process.Id -RootStartTime $processStartTime
        $null = $process.WaitForExit(5000)
    }

    $stderr = ""
    try {
        if ($stderrTask.Wait(5000)) {
            $stderr = $stderrTask.GetAwaiter().GetResult()
        }
    } catch {
        $stderr = $_.Exception.Message
    } finally {
        $process.Dispose()
    }

    if ($null -ne $requestFailure) {
        throw "$requestFailure`n$stderr"
    }
    try {
        $response = $responseLine | ConvertFrom-Json -Depth 50
    } catch {
        throw "MCP Inspector returned invalid JSON: $responseLine`n$stderr"
    }
    if ($null -ne $response.PSObject.Properties['error']) {
        throw "MCP Inspector request failed: $($response.error | ConvertTo-Json -Compress -Depth 20)"
    }
    return $response
}

function Invoke-InspectorMethod {
    param([string]$Method)
    return Invoke-InspectorRequest -Method $Method -ToolName "" -Arguments @{}
}

function Invoke-InspectorTool {
    param(
        [string]$ToolName,
        [hashtable]$Arguments
    )
    $result = Invoke-InspectorRequest `
        -Method "tools/call" `
        -ToolName $ToolName `
        -Arguments $Arguments
    if ($null -ne $result.result.PSObject.Properties['isError'] -and $result.result.isError) {
        throw "MCP tool $ToolName returned an error result"
    }
    $textBlock = $result.result.content |
        Where-Object { $_.type -eq "text" } |
        Select-Object -First 1
    if ($null -eq $textBlock) {
        throw "MCP tool $ToolName returned no text payload"
    }
    $payload = $textBlock.text | ConvertFrom-Json -Depth 50
    if (-not $payload.success) {
        throw "MCP tool $ToolName reported failure: $($textBlock.text)"
    }
    return $payload
}

function Wait-ForMcp {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastError = "not attempted"
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $null = Invoke-InspectorMethod -Method "tools/list"
            return
        } catch {
            $lastError = $_.Exception.Message
            Start-Sleep -Seconds 1
        }
    }
    throw "MCP did not become ready: $lastError"
}

function Get-LiveFingerprint {
    $payload = Invoke-InspectorTool `
        -ToolName "node_call_method" `
        -Arguments @{
            node_path = "."
            method_name = "runtime_fingerprint"
            args = @()
        }
    return [string]$payload.result
}

function Wait-ForFingerprint {
    param([string]$Expected)
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastObserved = ""
    $lastError = ""
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($editor.HasExited) {
            throw "Godot editor exited with code $($editor.ExitCode)"
        }
        try {
            $lastObserved = Get-LiveFingerprint
            if ($lastObserved -eq $Expected) {
                return $lastObserved
            }
        } catch {
            $lastError = $_.Exception.Message
        }
        Start-Sleep -Seconds 1
    }
    throw "Fingerprint did not become '$Expected'; last='$lastObserved' error='$lastError'"
}

function Invoke-DebugInstall {
    param([string]$Fingerprint)
    $env:AURUM_RUNTIME_FINGERPRINT = $Fingerprint
    & (Join-Path $workspace "scripts\build.ps1") `
        -DebugBuild `
        -NoTests `
        -Workspace $workspace `
        -GodotProject $projectRoot `
        -GodotBinary $GodotBinary
    if ($LASTEXITCODE -ne 0) {
        throw "Debug install failed for fingerprint $Fingerprint"
    }
    $installed = Join-Path $projectRoot "addons\aurum\bin\aurum_godot.debug.dll"
    return (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
}

function Stop-OwnedEditor {
    if ($KeepEditorOpen -or $null -eq $editor) { return }
    $current = Get-Process -Id $editor.Id -ErrorAction SilentlyContinue
    if ($null -eq $current) { return }
    if ($current.StartTime -ne $editorStartTime) {
        throw "Refusing to stop PID $($editor.Id): process start time changed"
    }
    if ($current.Path -ne (Resolve-Path $GodotBinary).Path) {
        throw "Refusing to stop PID $($editor.Id): executable path changed"
    }
    $editorCim = Get-CimInstance Win32_Process -Filter "ProcessId = $($editor.Id)"
    if ($null -eq $editorCim -or
        [string]$editorCim.CommandLine -notlike "*$projectRoot*") {
        throw "Refusing to stop PID $($editor.Id): project path ownership check failed"
    }
    $null = $current.CloseMainWindow()
    if (-not $current.WaitForExit(10000)) {
        Stop-Process -Id $current.Id -Force
    }
}

New-Item -ItemType Directory -Path $addonsRoot -Force | Out-Null
$toolkitSource = Join-Path $workspace "godot\addons\godot_mcp_toolkit"
if (-not (Test-Path -LiteralPath $toolkitSource)) {
    throw "Godot MCP Toolkit is required for this smoke test: $toolkitSource"
}
Copy-Item -LiteralPath $toolkitSource -Destination $addonsRoot -Recurse -Force

$projectText = @'
config_version=5

[application]
config/name="Aurum Hot Reload Smoke"
run/main_scene="res://probe.tscn"
config/features=PackedStringArray("4.7")

[editor_plugins]
enabled=PackedStringArray("res://addons/aurum/plugin.cfg", "res://addons/godot_mcp_toolkit/plugin.cfg")

[rendering]
renderer/rendering_method="gl_compatibility"
'@
Write-Utf8File -Path (Join-Path $projectRoot "project.godot") -Content $projectText

$sceneText = @'
[gd_scene format=3]

[node name="Probe" type="AurumNode"]
'@
Write-Utf8File -Path (Join-Path $projectRoot "probe.tscn") -Content $sceneText

$mcpConfig = @{
    mcpServers = @{
        godot = @{
            command = "cmd"
            args = @("/c", "godot-mcp-server")
            env = @{
                GODOT_MCP_CONFIG_VERSION = "1"
                GODOT_MCP_PROJECT_PATH = $projectRoot
            }
        }
    }
}
Write-Utf8File `
    -Path $mcpConfigPath `
    -Content ($mcpConfig | ConvertTo-Json -Depth 10)

$evidence = [ordered]@{
    schema_version = 1
    session_id = $sessionId
    passed = $false
    project_path = $projectRoot
    godot_binary = (Resolve-Path $GodotBinary).Path
    editor_pid = $null
    editor_start_time_utc = $null
    observations = $observations
    error = $null
}

try {
    $initialFingerprint = "phase0-$sessionId-0"
    $initialHash = Invoke-DebugInstall -Fingerprint $initialFingerprint

    $editor = Start-Process `
        -FilePath $GodotBinary `
        -ArgumentList @("--editor", "--path", "`"$projectRoot`"") `
        -PassThru
    $editorStartTime = $editor.StartTime
    $evidence.editor_pid = $editor.Id
    $evidence.editor_start_time_utc = $editorStartTime.ToUniversalTime().ToString("O")

    Wait-ForMcp
    $null = Invoke-InspectorTool `
        -ToolName "scene_open" `
        -Arguments @{ file_path = "res://probe.tscn" }
    $observed = Wait-ForFingerprint -Expected $initialFingerprint
    $observations.Add([ordered]@{
        iteration = 0
        requested = $initialFingerprint
        observed = $observed
        dll_sha256 = $initialHash
        editor_pid = $editor.Id
    })

    for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
        $fingerprint = "phase0-$sessionId-$iteration"
        $hash = Invoke-DebugInstall -Fingerprint $fingerprint
        $null = Invoke-InspectorTool `
            -ToolName "extensions_refresh" `
            -Arguments @{}
        $observed = Wait-ForFingerprint -Expected $fingerprint
        $observations.Add([ordered]@{
            iteration = $iteration
            requested = $fingerprint
            observed = $observed
            dll_sha256 = $hash
            editor_pid = $editor.Id
        })
    }

    if ((Get-Process -Id $editor.Id).StartTime -ne $editorStartTime) {
        throw "Editor PID was reused by a different process"
    }
    if (($observations | Select-Object -ExpandProperty editor_pid -Unique).Count -ne 1) {
        throw "More than one editor PID appeared in observations"
    }

    $evidence.passed = $true
    Write-Host "PHASE0_HOT_RELOAD_OK pid=$($editor.Id) reloads=$Iterations evidence=$evidencePath"
} catch {
    $evidence.error = $_.Exception.Message
    throw
} finally {
    if ($null -eq $originalFingerprint) {
        Remove-Item Env:AURUM_RUNTIME_FINGERPRINT -ErrorAction SilentlyContinue
    } else {
        $env:AURUM_RUNTIME_FINGERPRINT = $originalFingerprint
    }
    $cleanupError = $null
    try {
        Stop-OwnedEditor
    } catch {
        $cleanupError = $_.Exception.Message
        $evidence.passed = $false
        if ([string]::IsNullOrWhiteSpace([string]$evidence.error)) {
            $evidence.error = "Editor cleanup failed: $cleanupError"
        } else {
            $evidence.error = "$($evidence.error) | Editor cleanup failed: $cleanupError"
        }
    }
    Write-Utf8File -Path $evidencePath -Content ($evidence | ConvertTo-Json -Depth 20)
    if ($null -ne $cleanupError) {
        throw $cleanupError
    }
}
```

- [ ] **Step 2: Run a syntax-only PowerShell parse check**

Run:

```powershell
$errors = $null
[System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path 'scripts/tests/phase0_hot_reload_smoke.ps1'),
    [ref]$null,
    [ref]$errors
) | Out-Null
if ($errors.Count -gt 0) { $errors | Format-List; exit 1 }
```

Expected: exit 0 with no parse errors.

- [ ] **Step 3: Run the smoke once before relying on its result**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_hot_reload_smoke.ps1 -Iterations 1
```

Expected on the direct-reload path:

```text
PHASE0_HOT_RELOAD_OK with one numeric PID, reloads=1, and an evidence.json path under target/aurum-hot-reload-smoke
```

If it fails, inspect the evidence JSON, exact Godot console, installed DLL hash, and process state before changing implementation. Do not weaken the PID or fingerprint assertions.

- [ ] **Step 4: Record the scoped checkpoint**

Run:

```powershell
git diff --check -- scripts/tests/phase0_hot_reload_smoke.ps1
git status --short -- scripts/tests/phase0_hot_reload_smoke.ps1
Get-ChildItem target/aurum-hot-reload-smoke -Recurse -Filter evidence.json |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1 FullName,Length,LastWriteTimeUtc
```

Expected: the smoke script is the only repository file in this task; generated projects stay ignored under `target/`.

---

### Task 5: Stress direct reload and make the architecture decision

**Files:**
- Create: `docs/HOT_RELOAD.md`
- Modify: `README.md:135-146`
- Modify: `docs/GETTING_STARTED.md:56-86`
- Evidence: latest `evidence.json` below `target/aurum-hot-reload-smoke/`.

**Interfaces:**
- Consumes: Task 4 smoke test and evidence schema.
- Produces: one recorded decision, `direct_gdextension_reload` or `runtime_worker_required`.
- Produces: accurate user instructions that never promise reload beyond the tested boundary.

- [ ] **Step 1: Run the five-reload acceptance test**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_hot_reload_smoke.ps1 -Iterations 5
```

Expected for direct reload acceptance:

- Six observations: initial fingerprint plus five changed fingerprints.
- Every requested fingerprint equals the observed fingerprint.
- Every observation has the same editor PID.
- Every DLL SHA-256 is present.
- The script exits 0 and prints `PHASE0_HOT_RELOAD_OK`.

- [ ] **Step 2: Apply the decision rule without hiding failures**

If the five-reload test passes, select `direct_gdextension_reload`.

If it fails, use the implementation workflow's systematic-debugging skill and test one root-cause hypothesis at a time. Examples of distinct root causes are wrong debug mapping, non-atomic installation, editor filesystem refresh, or a live native instance that blocks unload. After three isolated fixes fail, stop this plan, select `runtime_worker_required`, and write the observed failure evidence into `docs/HOT_RELOAD.md`. Do not begin the Studio core against an undecided process model.

- [ ] **Step 3: Write the tested reload contract**

If direct reload passed, create `docs/HOT_RELOAD.md` with:

````markdown
# Aurum hot reload

## Phase 0 decision

Direct GDExtension reload is the selected Phase 0 architecture. Godot 4.7 completed five consecutive safe Rust implementation reloads in one editor process.

## No editor restart

The normal Aurum development loop keeps the Godot editor alive for:

- GDScript implementation changes.
- Scene, resource, and shader changes supported by Godot reload.
- Pure Rust simulation and algorithm changes behind the stable `AurumNode` API.
- Rust method-body changes that do not alter registered Godot classes or method signatures.

Development uses `aurum_godot.debug.dll`. Release packaging uses `aurum_godot.dll`.

## Gameplay restart only

A running game may be stopped and relaunched after a change invalidates live scene instances. This does not restart the editor.

## Exceptional editor restart

An editor restart may still be required after changing native class registration, inheritance, Godot-facing methods, properties, signals, initialization levels, entry symbols, library mappings, or the Godot API version.

## Commands

```powershell
pwsh scripts/dev.ps1
pwsh scripts/dev.ps1 -RunEditor
pwsh scripts/tests/phase0_contract.ps1
pwsh scripts/tests/phase0_hot_reload_smoke.ps1 -Iterations 5
```

Build failures and DLL installation failures keep the last working installed DLL. They do not terminate the editor.
````

If the runtime worker was selected, create `docs/HOT_RELOAD.md` with:

```markdown
# Aurum hot reload

## Phase 0 decision

The runtime worker is the selected Phase 0 architecture. Direct GDExtension reload did not pass the five-reload gate after three isolated fixes.

## Current boundary

GDScript, scene, resource, and shader changes continue to use Godot's normal editor reload behavior. Safe Rust hot reload is not accepted through the direct GDExtension path.

Until `aurum-runtime-host` is implemented, a Rust rebuild may require a controlled editor restart. Aurum Studio must not describe Rust changes as restart-free before the worker acceptance test passes.

## Next architecture

Aurum Studio will keep Godot running and supervise a replaceable `aurum-runtime-host` process. Rust simulation code reloads by replacing that worker while the editor bridge remains stable.

## Commands

Run `pwsh scripts/dev.ps1 -Once` for a checked debug build. Do not run the continuous watcher as if direct native reload were accepted.
```

For the fallback document, append a `## Direct reload evidence` section containing the exact final error text, the number of successful observations before failure, and the three tested root-cause fixes with their results. Copy literal facts from the final evidence JSON and implementation log.

- [ ] **Step 4: Correct the README reload table**

On the direct-reload path, replace the Rust rows and following paragraph with:

```markdown
| Layer                              | Normal feedback | How                                      |
|------------------------------------|-----------------|------------------------------------------|
| GDScript                           | Immediate       | Godot reloads scripts                    |
| `.tscn` scenes and resources       | Immediate       | Godot reloads editor resources           |
| Safe Rust implementation changes   | Debug build     | Reloadable GDExtension, same editor PID  |
| Native Godot API structure changes | Controlled      | Exceptional editor restart               |

Run `pwsh scripts/dev.ps1` for the self-contained debug watcher. It does not
require `cargo-watch`. A failed build keeps the last working DLL. See
`docs/HOT_RELOAD.md` for the tested boundary and live acceptance evidence.
```

On the runtime-worker path, use this table and paragraph instead:

```markdown
| Layer                              | Normal feedback       | How                                      |
|------------------------------------|-----------------------|------------------------------------------|
| GDScript                           | Immediate             | Godot reloads scripts                    |
| `.tscn` scenes and resources       | Immediate             | Godot reloads editor resources           |
| Safe Rust implementation changes   | Worker implementation | Direct GDExtension reload was not accepted |
| Native Godot API structure changes | Controlled            | Exceptional editor restart               |

Run `pwsh scripts/dev.ps1 -Once` for a checked debug build. Restart-free Rust
iteration remains unavailable until `aurum-runtime-host` passes its acceptance
test. See `docs/HOT_RELOAD.md` for the Phase 0 evidence and selected architecture.
```

- [ ] **Step 5: Correct Getting Started**

On the direct-reload path, replace the hot-reload and DLL-lock guidance with:

````markdown
## Develop without routine editor restarts

Run the self-contained watcher:

```pwsh
pwsh scripts/dev.ps1 -RunEditor
```

The watcher uses debug builds and installs `aurum_godot.debug.dll`. GDScript,
scene, resource, shader, and safe Rust implementation changes keep the editor
process alive. A running game may restart independently.

Native class registration, inheritance, exported method or signal signatures,
entry symbols, and Godot API-version changes may require a controlled editor
restart. See `docs/HOT_RELOAD.md` for the exact tested boundary.

## Common pitfalls

- **"AurumNode class not found"**: run `pwsh scripts/dev.ps1 -Once` and verify
  that `addons/aurum/bin/aurum_godot.debug.dll` exists.
- **DLL installation failed**: the last working DLL remains installed. Close
  programs that independently locked the staged file, then let the watcher retry.
- **Native structure changed**: save editor work and use the controlled restart
  path. Do not treat this exceptional case as the normal development loop.
````

On the runtime-worker path, use this guidance instead:

````markdown
## Development loop while the runtime worker is pending

GDScript, scene, resource, and shader changes keep using Godot's normal live
reload behavior. Build Rust changes with:

```pwsh
pwsh scripts/dev.ps1 -Once
```

Direct GDExtension hot reload did not pass Phase 0. Rust rebuilds may require a
controlled editor restart until `aurum-runtime-host` is implemented and tested.
Do not use the continuous watcher as a restart-free workflow yet.

## Common pitfalls

- **"AurumNode class not found"**: run `pwsh scripts/dev.ps1 -Once` and verify
  that `addons/aurum/bin/aurum_godot.debug.dll` exists.
- **DLL installation failed**: the last working DLL remains installed. Close
  programs that independently locked the staged file, then rebuild.
- **Rust change is still stale**: save editor work and use the controlled restart
  path. Check `docs/HOT_RELOAD.md` for the runtime-worker decision.
````

- [ ] **Step 6: Record the scoped checkpoint**

Run:

```powershell
git diff --check -- docs/HOT_RELOAD.md README.md docs/GETTING_STARTED.md
git diff -- README.md docs/GETTING_STARTED.md
git status --short -- docs/HOT_RELOAD.md
```

Expected: documentation matches the selected evidence and contains no claim that configuration alone proves hot reload. Do not stage or commit.

---

### Task 6: Run the complete Phase 0 verification gate

**Files:**
- Verify: all Phase 0 files listed in the file map.
- Verify: pre-existing dirty files remain preserved.

**Interfaces:**
- Consumes: all earlier tasks.
- Produces: final Phase 0 pass evidence and the architecture input for the Aurum Studio core plan.

- [ ] **Step 1: Run static and focused tests**

Run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_contract.ps1
pwsh -NoProfile -File scripts/tests/dev_once_smoke.ps1
pwsh -NoProfile -File scripts/tests/runtime_fingerprint_build.ps1
cargo test -p aurum-godot --lib
cargo fmt --check
```

Expected: all commands exit 0. The existing dead-code warning for `AurumNode.world` may remain, but no new warning is accepted without explanation.

- [ ] **Step 2: Run workspace and Godot validation**

Run:

```powershell
cargo test --workspace
pwsh -NoProfile -File scripts/build.ps1 -DebugBuild -NoTests
$godotOutput = & 'A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe' `
    --headless `
    --editor `
    --path 'A:\RecoveredProjects\C_Drive\Game_Development\aurum-engine\godot' `
    --quit-after 3 2>&1
$godotExit = $LASTEXITCODE
$godotOutput
if ($godotExit -ne 0 -or $godotOutput -match '(?m)SCRIPT ERROR|Parse Error|Failed to load script|GDExtension.*failed') {
    throw "Godot validation reported a relevant import, script, or GDExtension failure"
}
```

Expected: workspace tests pass, debug build installs and verifies hashes, and Godot exits 0 without relevant script or GDExtension errors.

- [ ] **Step 3: Apply the selected-path live verification**

Read `docs/HOT_RELOAD.md`. If direct reload was selected, run:

```powershell
pwsh -NoProfile -File scripts/tests/phase0_hot_reload_smoke.ps1 -Iterations 5
if ($LASTEXITCODE -ne 0) { throw 'Final direct-reload acceptance failed' }
```

Expected: exit 0, five consecutive safe Rust reloads, one editor PID, and a fresh evidence JSON.

If the runtime worker was selected, do not rerun the rejected direct path. Preserve its final failure evidence and verify that `docs/HOT_RELOAD.md` contains `The runtime worker is the selected Phase 0 architecture.`

- [ ] **Step 4: Verify the final artifact and process evidence**

Run:

```powershell
$evidence = Get-ChildItem target/aurum-hot-reload-smoke -Recurse -Filter evidence.json |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1 |
    Get-Content -Raw |
    ConvertFrom-Json
$decision = Get-Content -Raw -LiteralPath docs/HOT_RELOAD.md

if ($decision -match 'Direct GDExtension reload is the selected Phase 0 architecture') {
    if (-not $evidence.passed) { throw $evidence.error }
    if (($evidence.observations.editor_pid | Select-Object -Unique).Count -ne 1) {
        throw 'Live smoke used more than one editor PID'
    }
    if ($evidence.observations.Count -ne 6) {
        throw "Expected 6 observations, got $($evidence.observations.Count)"
    }
    foreach ($observation in $evidence.observations) {
        if ($observation.requested -ne $observation.observed) {
            throw "Fingerprint mismatch at iteration $($observation.iteration)"
        }
        if ([string]::IsNullOrWhiteSpace([string]$observation.dll_sha256)) {
            throw "Missing DLL hash at iteration $($observation.iteration)"
        }
    }
} elseif ($decision -match 'The runtime worker is the selected Phase 0 architecture') {
    if ($evidence.passed) {
        throw 'Worker fallback document conflicts with passing direct-reload evidence'
    }
    if ([string]::IsNullOrWhiteSpace([string]$evidence.error)) {
        throw 'Rejected direct-reload evidence has no error'
    }
} else {
    throw 'HOT_RELOAD.md contains no recognized Phase 0 decision'
}
$evidence | ConvertTo-Json -Depth 20
```

Expected: direct reload has one editor PID and six matching observations, or the worker fallback has a non-empty final failure record consistent with the document.

- [ ] **Step 5: Audit the working tree and preserve user changes**

Run:

```powershell
git diff --check
git status --short --branch
git diff --stat
git diff -- Cargo.toml Cargo.lock crates/aurum-core/src/events/mod.rs crates/aurum-godot/Cargo.toml godot/project.godot godot/scripts/aurum_2d_kinematics.gd godot/scripts/aurum_entity.gd
```

Expected: no whitespace errors. The pre-existing Cargo, Aurum Space, event, project, and GDScript changes remain intact. Phase 0 additions are visible but unstaged.

- [ ] **Step 6: Final review checkpoint**

Summarize:

- Direct reload or runtime-worker decision.
- Godot and Rust versions.
- Exact editor PID and number of successful reloads.
- Latest evidence JSON path.
- Test commands and exit results.
- Any remaining exceptional-restart boundary.
- Full list of modified and created files.

Do not claim Aurum Studio itself exists after Phase 0. This phase proves and selects the reload mechanism that Studio will supervise.
