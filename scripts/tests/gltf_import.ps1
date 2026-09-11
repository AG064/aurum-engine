# Verifies that glTF written by aurum-content imports into Godot 4.7.
#
# This is the real end-to-end gate for content authoring: generate a scene in
# Rust, hand it to Godot's own importer, and assert the meshes, hierarchy, and
# animations survive. It proves the interchange format rather than trusting it.
#
#   pwsh scripts/tests/gltf_import.ps1
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

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("aurum-gltf-import-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -Force | Out-Null

try {
    # 1. Generate the scene with the Rust example.
    Write-Host 'generating demo scene with aurum-content...'
    $gen = & cargo run -q -p aurum-content --example build_demo -- $work 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "build_demo failed:`n$gen"
    }
    $gltf = Join-Path $work 'aurum-demo.gltf'
    $bin = Join-Path $work 'aurum-demo.bin'
    if (-not (Test-Path $gltf)) { throw "expected $gltf to exist" }
    if (-not (Test-Path $bin)) { throw "expected the companion $bin to exist" }

    # 2. Minimal Godot project around it.
    [System.IO.File]::WriteAllText(
        (Join-Path $work 'project.godot'),
        "config_version=5`n`n[application]`n`nconfig/name=`"Aurum GLTF Check`"`n")

    # 3. Verification script. Named _initialize because a SceneTree script is
    #    entered through that method, and it must quit() or Godot runs forever.
    $verify = @'
extends SceneTree

func collect(node: Node, out: Array) -> void:
	if node is MeshInstance3D:
		out.append(node)
	for child in node.get_children():
		collect(child, out)

func _initialize() -> void:
	var packed = load("res://aurum-demo.gltf")
	if packed == null:
		print("RESULT=LOAD_FAILED")
		quit(1)
		return
	var root = packed.instantiate()

	var meshes: Array = []
	collect(root, meshes)

	var triangles := 0
	var names: Array = []
	for m in meshes:
		names.append(m.name)
		if m.mesh != null:
			for s in m.mesh.get_surface_count():
				var arrays = m.mesh.surface_get_arrays(s)
				var idx = arrays[Mesh.ARRAY_INDEX]
				if idx != null:
					triangles += idx.size() / 3

	var anims: Array = []
	var stack: Array = [root]
	while stack.size() > 0:
		var n = stack.pop_back()
		if n is AnimationPlayer:
			for a in n.get_animation_list():
				anims.append(a)
		for c in n.get_children():
			stack.append(c)

	print("MESH_NODES=", meshes.size())
	print("TRIANGLES=", triangles)
	print("ANIMATIONS=", anims.size())
	print("ANIM_NAMES=", ",".join(anims))
	print("NODE_NAMES=", ",".join(names))
	print("RESULT=OK")
	quit(0)
'@
    [System.IO.File]::WriteAllText((Join-Path $work 'verify.gd'), $verify)

    # 4. Import, then verify.
    Write-Host 'importing into Godot...'
    $import = & $godot --headless --path $work --import 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Godot import failed (exit $LASTEXITCODE):`n$($import -join "`n")"
    }

    Write-Host 'verifying imported scene...'
    $output = & $godot --headless --path $work --script res://verify.gd 2>&1
    $text = $output -join "`n"

    # Parse the key=value lines the script prints.
    $values = @{}
    foreach ($line in $output) {
        if ($line -match '^([A-Z_]+)=(.*)$') { $values[$Matches[1]] = $Matches[2] }
    }

    $failures = [System.Collections.Generic.List[string]]::new()

    if ($values['RESULT'] -ne 'OK') {
        $failures.Add("verification script did not report OK (got '$($values['RESULT'])')")
    }

    # The generator writes 9 nodes, one of which is a transform-only root.
    if ($values['MESH_NODES'] -ne '8') {
        $failures.Add("expected 8 mesh nodes, got '$($values['MESH_NODES'])'")
    }

    # 2354 triangles across 7 unique meshes, plus 80 for the second pillar
    # instance that shares the pillar mesh = 2434 across node instances.
    if ($values['TRIANGLES'] -ne '2434') {
        $failures.Add("expected 2434 triangles, got '$($values['TRIANGLES'])'")
    }

    if ($values['ANIMATIONS'] -ne '3') {
        $failures.Add("expected 3 animations, got '$($values['ANIMATIONS'])'")
    }
    foreach ($expected in 'HeroSpin', 'OrbBob', 'RingSpin') {
        if ($values['ANIM_NAMES'] -notlike "*$expected*") {
            $failures.Add("animation '$expected' is missing from '$($values['ANIM_NAMES'])'")
        }
    }

    foreach ($expected in 'Ground', 'Hero', 'OrbitingOrb', 'Ring', 'Pillar0', 'Pillar1', 'Spike', 'Arch') {
        if ($values['NODE_NAMES'] -notlike "*$expected*") {
            $failures.Add("node '$expected' is missing from '$($values['NODE_NAMES'])'")
        }
    }

    # A parse or load error means the file is not really valid glTF, even if
    # something still loaded.
    if ($text -match 'glTF.*error|Failed to load|Parse Error|SCRIPT ERROR') {
        $failures.Add("Godot reported an error while loading the glTF:`n$text")
    }

    if ($failures.Count -gt 0) {
        foreach ($failure in $failures) { Write-Host "FAIL: $failure" }
        Write-Host $text
        exit 1
    }

    Write-Host "  mesh nodes : $($values['MESH_NODES'])"
    Write-Host "  triangles  : $($values['TRIANGLES'])"
    Write-Host "  animations : $($values['ANIM_NAMES'])"
    Write-Host 'GLTF_IMPORT_OK'
    exit 0
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
