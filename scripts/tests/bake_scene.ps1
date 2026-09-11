# Verifies that a generated bake script produces a real Godot scene.
#
# The chain under test:
#   Rust document -> glTF -> generated GDScript -> Godot runs it -> .tscn
#
# The point is script attachment, which glTF cannot express. The assertion is
# that the finished .tscn references the script AND that the node actually
# carries it, because a scene can exist without either.
#
#   pwsh scripts/tests/bake_scene.ps1
#
# Optional: -GodotBinary <path> to override the Godot executable.

[CmdletBinding()]
param(
    [string]$GodotBinary = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

function Find-Godot {
    param([string]$Explicit)
    if ($Explicit) {
        if (-not (Test-Path $Explicit)) { throw "Godot binary not found: $Explicit" }
        return $Explicit
    }
    $candidates = @(
        (Join-Path (Split-Path -Parent $repo) 'godot\Godot_v4.7-stable_win64_console.exe'),
        (Join-Path (Split-Path -Parent $repo) 'godot\Godot_v4.7-stable_win64.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    $onPath = Get-Command 'godot' -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    throw 'Godot 4.7 not found. Pass -GodotBinary <path>.'
}

$godot = Find-Godot -Explicit $GodotBinary
Write-Host "godot: $godot"

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("aurum-bake-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -Force | Out-Null

try {
    Write-Host 'generating scene and bake script with aurum-content...'
    $generated = & cargo run -q -p aurum-content --example bake_demo -- $work 2>&1
    if ($LASTEXITCODE -ne 0) { throw "bake_demo failed:`n$generated" }

    foreach ($required in 'baked.gltf', 'baked.bin', 'bake.gd') {
        if (-not (Test-Path (Join-Path $work $required))) {
            throw "expected $required to be generated"
        }
    }

    # A script for the bake to attach, plus a minimal project.
    New-Item -ItemType Directory -Path (Join-Path $work 'scripts') -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $work 'scripts\spin.gd'), "extends Node3D`n`nvar speed := 1.5`n")
    [System.IO.File]::WriteAllText(
        (Join-Path $work 'project.godot'),
        "config_version=5`n`n[application]`n`nconfig/name=`"BakeCheck`"`n")

    Write-Host 'importing the glTF...'
    & $godot --headless --path $work --import 2>&1 | Out-Null

    Write-Host 'running the generated bake script...'
    $output = & $godot --headless --path $work --script res://bake.gd 2>&1
    $text = $output -join "`n"

    $failures = [System.Collections.Generic.List[string]]::new()

    if ($LASTEXITCODE -ne 0) {
        $failures.Add("the bake script exited $LASTEXITCODE")
    }
    if ($text -notmatch 'attached res://scripts/spin\.gd to Hero') {
        $failures.Add("the bake did not report attaching the script:`n$text")
    }
    if ($text -match 'SCRIPT ERROR|Parse Error') {
        $failures.Add("Godot reported a script error:`n$text")
    }

    $scene = Join-Path $work 'baked.tscn'
    if (-not (Test-Path $scene)) {
        $failures.Add('no baked.tscn was produced')
    } else {
        $tscn = Get-Content $scene -Raw

        # The script must be imported as an external resource.
        if ($tscn -notmatch 'type="Script"\s+path="res://scripts/spin\.gd"') {
            $failures.Add('the .tscn does not reference spin.gd as an external Script resource')
        }
        # And the node must actually carry it. A scene can reference a resource
        # without using it, which would look like success and do nothing.
        if ($tscn -notmatch '(?s)\[node name="Hero".*?script = ExtResource') {
            $failures.Add('the Hero node does not have the script attached')
        }
        # The generated root name and hierarchy must survive the round trip.
        if ($tscn -notmatch '\[node name="BakedScene"') {
            $failures.Add('the generated root name BakedScene is missing')
        }
        if ($tscn -notmatch '\[node name="Orb" type="MeshInstance3D" parent="Hero"') {
            $failures.Add('the Orb child was not parented under Hero')
        }
        if ($tscn -notmatch 'HeroSpin') {
            $failures.Add('the animation did not survive into the baked scene')
        }
    }

    if ($failures.Count -gt 0) {
        foreach ($failure in $failures) { Write-Host "FAIL: $failure" }
        if (Test-Path $scene) {
            Write-Host '--- baked.tscn (first 40 lines) ---'
            Get-Content $scene | Select-Object -First 40 | ForEach-Object { Write-Host $_ }
        }
        exit 1
    }

    $size = (Get-Item $scene).Length
    Write-Host "  baked scene : baked.tscn ($size bytes)"
    Write-Host '  script      : res://scripts/spin.gd attached to Hero'
    Write-Host '  hierarchy   : BakedScene > Hero > Orb'
    Write-Host 'BAKE_SCENE_OK'
    exit 0
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
