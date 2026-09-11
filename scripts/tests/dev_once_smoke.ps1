[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$project = Join-Path $workspace ("target\phase0-dev-once-smoke-" + [guid]::NewGuid().ToString("N"))
$projectAddonBin = Join-Path $project "addons\aurum\bin"
if (Test-Path -LiteralPath $project) {
    throw "Smoke project already exists: $project"
}

& (Join-Path $workspace "scripts\dev.ps1") `
    -Once `
    -GodotProject $project `
    -GodotBinary $GodotBinary
if ($LASTEXITCODE -ne 0) {
    throw "dev.ps1 -Once failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path -LiteralPath $project)) {
    throw "Smoke project was not created: $project"
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
