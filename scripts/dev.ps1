# Aurum dev script — continuous rebuild on Rust file changes.
#
# Watches the `crates/` directory and rebuilds the GDExtension on any
# change. After each successful build, copies the DLL into the Godot
# add-on bin. You can run Godot in another window — the GDScript side
# picks up the new DLL on the next launch.
#
# This requires `cargo-watch`:
#   cargo install cargo-watch
#
# Usage:
#   pwsh scripts/dev.ps1
#   pwsh scripts/dev.ps1 -RunEditor

[CmdletBinding()]
param(
    [switch]$RunEditor,
    [string]$GodotProject,
    [string]$GodotBinary = "C:\Game_Development\godot\Godot_v4.7-stable_win64.exe"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$WorkspaceRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $GodotProject) {
    $GodotProject = Join-Path $WorkspaceRoot "godot"
}
$AddOnBin = Join-Path $GodotProject "addons\aurum\bin"
$AddOnBinLinux = $AddOnBin -replace '\\', '/'  # cargo-watch friendly path

# Sanity check
if (-not (Get-Command cargo-watch -ErrorAction SilentlyContinue)) {
    Write-Error "cargo-watch is not installed. Run: cargo install cargo-watch"
    exit 1
}

Write-Host "==> Aurum dev mode" -ForegroundColor Cyan
Write-Host "    Watching: crates/ (Rust changes will trigger a rebuild)"
Write-Host "    Output:   $AddOnBin\aurum_godot.dll"
Write-Host "    Press Ctrl+C to stop"
Write-Host ""

$WatchArgs = @(
    "watch",
    "-w", "crates",
    "-w", "Cargo.toml",
    "-w", "Cargo.lock",
    "-x", "build --release -p aurum-godot",
    "--post-watch",
    "powershell -NoProfile -Command `"Copy-Item -Force target/release/aurum_godot.dll '$AddOnBin\aurum_godot.dll' -ErrorAction SilentlyContinue; if (-not `$?) { exit 1 }`""
)

Push-Location $WorkspaceRoot
try {
    & cargo @WatchArgs
} finally {
    Pop-Location
}
