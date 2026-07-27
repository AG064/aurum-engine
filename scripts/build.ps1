# Aurum build script.
#
# Builds the Rust workspace in release mode (or debug), copies the
# GDExtension DLL into the Godot project's add-on bin/, runs the test
# suite (release mode only), and optionally launches Godot.
#
# Usage:
#   pwsh scripts/build.ps1                       # release + copy DLL + tests
#   pwsh scripts/build.ps1 -DebugBuild           # debug, skip tests
#   pwsh scripts/build.ps1 -Run                  # build then run the demo
#   pwsh scripts/build.ps1 -Run -Editor         # build then open editor
#   pwsh scripts/build.ps1 -NoTests              # skip tests
#   pwsh scripts/build.ps1 -GodotProject <path>  # custom Godot project
#   pwsh scripts/build.ps1 -GodotBinary <path>   # custom Godot binary
#   pwsh scripts/build.ps1 -Workspace <path>    # custom workspace root

[CmdletBinding()]
param(
    [switch]$DebugBuild,
    [switch]$Run,
    [switch]$Editor,
    [switch]$NoTests,
    [string]$Workspace,
    [string]$GodotProject,
    [string]$GodotBinary
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Resolve paths: defaults assume the standard layout where the Godot
# project lives at `<workspace>/godot/`. Override with -GodotProject
# if you've moved it.
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
    # Fallback: look in $GodotProject/../godot/
    $Alt = Join-Path (Split-Path (Split-Path $GodotProject -Parent) -Parent) "godot\Godot_v4.7-stable_win64.exe"
    if (Test-Path $Alt) { $GodotBinary = $Alt }
}

$AddOnBin = Join-Path $GodotProject "addons\aurum\bin"
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
Write-Host ""

# Run cargo build
Push-Location $Workspace
try {
    Write-Host "==> cargo $($CargoArgs -join ' ')" -ForegroundColor Green
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

# Find the produced DLL.
if ($DebugBuild) {
    $DllSource = Join-Path $Workspace "target\debug\aurum_godot.dll"
} else {
    $DllSource = Join-Path $Workspace "target\release\aurum_godot.dll"
}

if (-not (Test-Path $DllSource)) {
    throw "Build succeeded but DLL not found at: $DllSource"
}

# Make sure the add-on bin exists.
if (-not (Test-Path $AddOnBin)) {
    New-Item -ItemType Directory -Path $AddOnBin -Force | Out-Null
}

$DllTarget = Join-Path $AddOnBin "aurum_godot.dll"
Copy-Item -Path $DllSource -Destination $DllTarget -Force
Write-Host "==> Copied DLL to $DllTarget" -ForegroundColor Green

# Run tests if asked (always on the first run, cheap).
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

# Optionally run Godot.
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
