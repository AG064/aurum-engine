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
#   pwsh scripts/dev.ps1 -GodotProject <path>
#   pwsh scripts/dev.ps1 -GodotBinary <path>
#
# Godot binary resolution (in order):
#   1. -GodotBinary parameter
#   2. $env:AURUM_GODOT
#   3. "godot" or "godot4" on PATH
#   4. $Workspace/godot/Godot_v4.7-stable_*.exe (or /Godot_v4.7-stable_*.x86_64)
#   5. Common sibling locations of $GodotProject

[CmdletBinding()]
param(
    [switch]$RunEditor,
    [string]$GodotProject,
    [string]$GodotBinary
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Find-AurumGodot {
    param(
        [string]$WorkspaceRoot,
        [string]$ProjectPath,
        [string]$Explicit
    )

    $candidates = @()

    if ($Explicit) { $candidates += $Explicit }
    if ($env:AURUM_GODOT) { $candidates += $env:AURUM_GODOT }

    foreach ($name in @("godot", "godot4")) {
        $cmd = Get-Command $name -ErrorAction SilentlyContinue
        if ($cmd -and $cmd.Source) {
            $candidates += $cmd.Source
            break
        }
    }

    if ($WorkspaceRoot) {
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_win64.exe"
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_linux.x86_64"
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_macos.universal"
    }

    if ($ProjectPath) {
        $cur = (Resolve-Path $ProjectPath -ErrorAction SilentlyContinue).Path
        if (-not $cur) { $cur = $ProjectPath }
        for ($i = 0; $i -lt 5; $i++) {
            $parent = Split-Path $cur -Parent
            if (-not $parent -or $parent -eq $cur) { break }
            $candidates += Join-Path $parent "godot/Godot_v4.7-stable_win64.exe"
            $candidates += Join-Path $parent "godot/Godot_v4.7-stable_linux.x86_64"
            $candidates += Join-Path $parent "godot/Godot_v4.7-stable_macos.universal"
            $cur = $parent
        }
    }

    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) {
            return (Resolve-Path $c).Path
        }
    }
    return $null
}

$WorkspaceRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $GodotProject) {
    $GodotProject = Join-Path $WorkspaceRoot "godot"
}

if (-not $GodotBinary) {
    $GodotBinary = Find-AurumGodot -WorkspaceRoot $WorkspaceRoot -ProjectPath $GodotProject -Explicit $null
    if (-not $GodotBinary) {
        throw "Could not locate a Godot binary. Set one of:`n" +
              "  -GodotBinary <path>`n" +
              "  `$env:AURUM_GODOT = <path>`n" +
              "  or place Godot_v4.7-stable_*.{exe,x86_64,universal} at:`n" +
              "    $WorkspaceRoot/godot/`n" +
              "    or alongside the Godot project (sibling 'godot/' folder)."
    }
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
Write-Host "    Godot:    $GodotBinary"
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
