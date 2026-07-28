# Aurum build script.
#
# Builds the Rust workspace in release mode (or debug), copies the
# GDExtension DLLs into the Godot project's add-on bin/, runs the test
# suite (release mode only), and optionally launches Godot.
#
# Two GDExtensions are built:
#   1. `aurum-godot` — the main engine shim. Goes to addons/aurum/bin/.
#   2. `life_evolution` — the GPU simulation core. Lives in
#      godot/demos/life_evolution/GDExtension/ and has its own Rust
#      crate. We build it from outside the workspace because it has
#      its own dependencies and a separate target/ directory.
#
# Usage:
#   pwsh scripts/build.ps1                       # release + copy DLLs + tests
#   pwsh scripts/build.ps1 -DebugBuild           # debug, skip tests
#   pwsh scripts/build.ps1 -NoLife               # skip the life_evolution build
#   pwsh scripts/build.ps1 -Run                  # build then run the demo
#   pwsh scripts/build.ps1 -Run -Editor         # build then open editor
#   pwsh scripts/build.ps1 -NoTests              # skip tests
#   pwsh scripts/build.ps1 -GodotProject <path>  # custom Godot project
#   pwsh scripts/build.ps1 -GodotBinary <path>   # custom Godot binary
#   pwsh scripts/build.ps1 -Workspace <path>    # custom workspace root

[CmdletBinding()]
param(
    [switch]$DebugBuild,
    [switch]$NoLife,
    [switch]$Run,
    [switch]$Editor,
    [switch]$NoTests,
    [string]$Workspace,
    [string]$GodotProject,
    [string]$GodotBinary
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $Workspace) {
    $Workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
} else {
    $Workspace = Resolve-Path $Workspace
}
if (-not $GodotProject) {
    $GodotProject = Join-Path $Workspace "godot"
}
if (-not $GodotBinary) {
    $GodotBinary = "C:\Game_Development\godot\Godot_v4.7-stable_win64.exe"
}
if (-not (Test-Path $GodotBinary)) {
    $Alt = Join-Path (Split-Path (Split-Path $GodotProject -Parent) -Parent) "godot\Godot_v4.7-stable_win64.exe"
    if (Test-Path $Alt) { $GodotBinary = $Alt }
}

$AddOnBin = Join-Path $GodotProject "addons\aurum\bin"
$LifeExtDir = Join-Path $GodotProject "demos\life_evolution\GDExtension"
$LifeCrate = Join-Path $LifeExtDir "rust"

if ($DebugBuild) {
    $Profile = "debug"
    $CargoArgs = @("build", "-p", "aurum-godot")
} else {
    $Profile = "release"
    $CargoArgs = @("build", "--release", "-p", "aurum-godot")
}

Write-Host "==> Aurum build ($Profile)" -ForegroundColor Cyan
Write-Host "    Workspace:    $Workspace"
Write-Host "    Godot project: $GodotProject"
Write-Host "    Godot binary:  $GodotBinary"
Write-Host "    Add-on bin:    $AddOnBin"
Write-Host "    Life ext dir:  $LifeExtDir"
Write-Host ""

# ---------- 1. Build aurum-godot ----------

Push-Location $Workspace
try {
    Write-Host "==> cargo $($CargoArgs -join ' ')" -ForegroundColor Green
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build (aurum-godot) failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if ($DebugBuild) {
    $DllSource = Join-Path $Workspace "target\debug\aurum_godot.dll"
} else {
    $DllSource = Join-Path $Workspace "target\release\aurum_godot.dll"
}

if (-not (Test-Path $DllSource)) {
    throw "Build succeeded but DLL not found at: $DllSource"
}

if (-not (Test-Path $AddOnBin)) {
    New-Item -ItemType Directory -Path $AddOnBin -Force | Out-Null
}

$DllTarget = Join-Path $AddOnBin "aurum_godot.dll"
Copy-Item -Path $DllSource -Destination $DllTarget -Force
Write-Host "==> Copied aurum_godot.dll to $DllTarget" -ForegroundColor Green

# ---------- 2. Build life_evolution (if present and not skipped) ----------

if (-not $NoLife) {
    if (Test-Path $LifeCrate) {
        Write-Host ""
        Write-Host "==> Building life_evolution GDExtension" -ForegroundColor Green
        Push-Location $LifeCrate
        try {
            if ($DebugBuild) {
                & cargo build
            } else {
                & cargo build --release
            }
            if ($LASTEXITCODE -ne 0) {
                throw "cargo build (life_evolution) failed with exit code $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
        Write-Host "==> life_evolution built (DLL at $LifeCrate\target\$Profile\life_evolution.dll)" -ForegroundColor Green
    } else {
        Write-Host ""
        Write-Host "(skipping life_evolution: $LifeCrate not found)" -ForegroundColor DarkGray
    }
}

# ---------- 3. Tests ----------

if (-not $DebugBuild -and -not $NoTests) {
    Write-Host ""
    Write-Host "==> Running workspace tests" -ForegroundColor Green
    Push-Location $Workspace
    try {
        & cargo test --workspace --quiet
    } finally {
        Pop-Location
    }
}

# ---------- 4. Optionally run Godot ----------

if ($Run) {
    if (-not (Test-Path $GodotBinary)) {
        throw "Godot binary not found at: $GodotBinary"
    }
    $Args = @()
    if ($Editor) {
        $Args += "--editor"
    } else {
        $Args += "--path"
        $Args += $GodotProject
    }
    Write-Host ""
    Write-Host "==> Launching Godot: $GodotBinary $($Args -join ' ')" -ForegroundColor Magenta
    & $GodotBinary @Args
}

Write-Host ""
Write-Host "==> Done." -ForegroundColor Cyan
