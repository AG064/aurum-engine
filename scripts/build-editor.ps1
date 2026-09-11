# Build the AurumEditor extension and install it into the Godot project.
#
# Deliberately separate from build.ps1. That script is covered by the Phase 0
# contract gate, and the editor extension is an independent concern with its
# own manifest, so keeping them apart means neither can break the other.
#
#   pwsh scripts/build-editor.ps1
#   pwsh scripts/build-editor.ps1 -Release
#   pwsh scripts/build-editor.ps1 -GodotProject path/to/godot

[CmdletBinding()]
param(
    [switch]$Release,
    [string]$GodotProject = '',
    [switch]$NoTests
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

if (-not $GodotProject) {
    $GodotProject = Join-Path $repo 'godot'
}
if (-not (Test-Path $GodotProject)) {
    throw "Godot project not found: $GodotProject"
}

$profile = if ($Release) { 'release' } else { 'debug' }
$arguments = @('build', '-p', 'aurum-editor')
if ($Release) { $arguments += '--release' }

Write-Host "building aurum-editor ($profile)..."
& cargo @arguments --manifest-path (Join-Path $repo 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

if (-not $NoTests) {
    Write-Host 'running aurum-editor tests...'
    & cargo test -p aurum-editor --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'cargo test -p aurum-editor failed' }
}

# Cargo names cdylib output with underscores.
$source = Join-Path $repo "target\$profile\aurum_editor.dll"
if (-not (Test-Path $source)) { throw "expected the built extension at $source" }

$destination = Join-Path $GodotProject 'addons\aurum_editor\bin'
New-Item -ItemType Directory -Path $destination -Force | Out-Null

# The manifest names both profiles, so install under the name it expects.
$target = Join-Path $destination $(if ($Release) { 'aurum_editor.dll' } else { 'aurum_editor.debug.dll' })
Copy-Item $source $target -Force

$hash = (Get-FileHash $target -Algorithm SHA256).Hash
$size = (Get-Item $target).Length
Write-Host "installed $target"
Write-Host "  sha256 : $hash"
Write-Host "  bytes  : $size"
Write-Host ''
Write-Host 'Enable it in Godot under Project > Project Settings > Plugins > Aurum Editor.'
Write-Host 'The plugin prints its bridge directory on startup; pass that to:'
Write-Host '  aurum mcp --root . --editor-bridge <dir>'
Write-Host 'EDITOR_BUILD_OK'
