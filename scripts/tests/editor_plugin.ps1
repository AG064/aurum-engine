# Verifies the AurumEditor GDExtension surface headlessly.
#
# This is the plugin-validation gate the MCP design calls for: it proves the
# native class registers, and that every primitive on the stable surface
# actually works against a real Godot scene tree.
#
# The primitive list is deliberately the assertion list. Growing the surface
# without adding a case here should feel wrong, because the surface size is
# what decides whether hot reload stays useful.
#
#   pwsh scripts/tests/editor_plugin.ps1

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

Write-Host 'building aurum-editor...'
& cargo build -p aurum-editor --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'cargo build -p aurum-editor failed' }

$dll = Join-Path $repo 'target\debug\aurum_editor.dll'
if (-not (Test-Path $dll)) { throw "expected the extension at $dll" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("aurum-editor-" + [guid]::NewGuid().ToString('N'))
$bin = Join-Path $work 'addons\aurum_editor\bin'
New-Item -ItemType Directory -Path $bin -Force | Out-Null

try {
    Copy-Item $dll (Join-Path $bin 'aurum_editor.debug.dll')
    Copy-Item (Join-Path $repo 'godot\addons\aurum_editor\bin\aurum_editor.gdextension') $bin
    [System.IO.File]::WriteAllText(
        (Join-Path $work 'project.godot'),
        "config_version=5`n`n[application]`n`nconfig/name=`"EditorCheck`"`n")

    # A script for attach_script to load. MeshInstance3D inherits Node3D.
    [System.IO.File]::WriteAllText(
        (Join-Path $work 'spin.gd'), "extends Node3D`n`nvar speed := 2.0`n")

    $verify = @'
extends SceneTree

var failures: Array[String] = []

func check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)

func parse(text) -> Dictionary:
	var value = JSON.parse_string(str(text))
	if typeof(value) != TYPE_DICTIONARY:
		return {"ok": false, "error": "response was not JSON: " + str(text)}
	return value

func _initialize() -> void:
	var editor = AurumEditor.new()
	get_root().add_child(editor)

	# --- before a scene is open -------------------------------------------
	check(not editor.has_scene_root(), "reports a scene root before one is set")
	check(parse(editor.describe_scene()).get("ok") == false, "describe_scene should fail with no scene")
	var early = parse(editor.create_node(".", "Node3D", "Nope"))
	check(early.get("ok") == false, "create_node should fail with no scene")

	# --- scene context -----------------------------------------------------
	var level := Node3D.new()
	level.name = "Level"
	get_root().add_child(level)
	editor.set_scene_root(level)
	check(editor.has_scene_root(), "has_scene_root should be true after set_scene_root")

	# --- create_node -------------------------------------------------------
	var hero = parse(editor.create_node(".", "MeshInstance3D", "Hero"))
	check(hero.get("ok") == true, "create MeshInstance3D: " + str(hero))
	check(str(hero.get("name")) == "Hero", "created node name: " + str(hero))
	check(str(hero.get("class")) == "MeshInstance3D", "created node class: " + str(hero))
	check(str(hero.get("path")) == "Hero", "created node path: " + str(hero))

	var orb = parse(editor.create_node("Hero", "Node3D", "Orb"))
	check(orb.get("ok") == true, "nested create: " + str(orb))
	check(str(orb.get("path")) == "Hero/Orb", "nested path: " + str(orb))

	# Root itself is addressable as ".".
	var at_root = parse(editor.create_node(".", "Marker3D", "Marker"))
	check(at_root.get("ok") == true, "create under root: " + str(at_root))

	# Unknown classes are refused rather than silently skipped.
	var bogus = parse(editor.create_node(".", "NotARealClass", "X"))
	check(bogus.get("ok") == false, "unknown class should be refused")

	# A non-Node class must not land in the tree.
	var resource = parse(editor.create_node(".", "Resource", "X"))
	check(resource.get("ok") == false, "a non-Node class should be refused")

	# Missing parents are reported.
	var orphan = parse(editor.create_node("Nowhere", "Node3D", "Lonely"))
	check(orphan.get("ok") == false, "a missing parent should be refused")

	# --- node_count / describe_scene --------------------------------------
	check(editor.node_count() == 4, "node_count was %d, expected 4" % editor.node_count())

	var described = parse(editor.describe_scene())
	check(described.get("ok") == true, "describe_scene: " + str(described))
	var tree = described.get("root", {})
	check(str(tree.get("name")) == "Level", "described root name: " + str(tree))
	# Level holds Hero and Marker; Orb is under Hero.
	var children = tree.get("children", [])
	check(children.size() == 2, "described children: %d" % children.size())

	# --- set_node_property -------------------------------------------------
	# JSON has no vector type, so this exercises the type-aware conversion.
	var moved = parse(editor.set_node_property("Hero", "position", '{"x": 1.5, "y": 2.5, "z": -3.0}'))
	check(moved.get("ok") == true, "set position: " + str(moved))
	var actual = level.get_node("Hero").position
	check(abs(actual.x - 1.5) < 0.001 and abs(actual.y - 2.5) < 0.001 and abs(actual.z + 3.0) < 0.001,
		"position did not take: " + str(actual))

	# Rename a node we are not using afterwards, so the rename cannot
	# invalidate the paths the later checks rely on.
	var renamed = parse(editor.set_node_property("Marker", "name", '"Renamed"'))
	check(renamed.get("ok") == true, "set name: " + str(renamed))
	check(level.get_node_or_null("Renamed") != null, "the rename did not take effect")

	var bad_json = parse(editor.set_node_property("Hero", "position", 'not json'))
	check(bad_json.get("ok") == false, "malformed JSON should be refused")

	# --- attach_script -----------------------------------------------------
	var attached = parse(editor.attach_script("Hero", "res://spin.gd"))
	check(attached.get("ok") == true, "attach_script: " + str(attached))
	check(level.get_node_or_null("Hero").get_script() != null, "the script is not actually attached")

	var missing = parse(editor.attach_script("Hero", "res://nope.gd"))
	check(missing.get("ok") == false, "a missing script should be refused")

	var outside = parse(editor.attach_script("Hero", "user://x.gd"))
	check(outside.get("ok") == false, "a non-res:// script path should be refused")

	# --- reparent_node -----------------------------------------------------
	var moved_node = parse(editor.reparent_node("Hero/Orb", "Renamed"))
	check(moved_node.get("ok") == true, "reparent: " + str(moved_node))
	check(level.get_node_or_null("Renamed/Orb") != null, "the node did not move")
	check(level.get_node_or_null("Hero/Orb") == null, "the node is still in the old place")

	# Reparenting into your own subtree would detach the scene.
	var cycle = parse(editor.reparent_node("Renamed", "Renamed/Orb"))
	check(cycle.get("ok") == false, "a reparent into own subtree should be refused")

	# --- remove_node -------------------------------------------------------
	var removed = parse(editor.remove_node("Renamed"))
	check(removed.get("ok") == true, "remove_node: " + str(removed))
	check(level.get_node_or_null("Renamed") == null, "the node was not removed")
	check(level.get_node_or_null("Renamed/Orb") == null, "the subtree was not removed")
	check(editor.node_count() == 2, "node_count after removal was %d" % editor.node_count())

	var root_removal = parse(editor.remove_node("."))
	check(root_removal.get("ok") == false, "removing the scene root should be refused")

	# --- pack_scene --------------------------------------------------------
	check(editor.pack_scene() != null, "pack_scene returned null")

	# --- the file bridge ---------------------------------------------------
	var request_dir = ProjectSettings.globalize_path("user://aurum_editor/requests")
	var response_dir = ProjectSettings.globalize_path("user://aurum_editor/responses")
	DirAccess.make_dir_recursive_absolute(request_dir)
	DirAccess.make_dir_recursive_absolute(response_dir)

	var file := FileAccess.open(request_dir.path_join("001.json"), FileAccess.WRITE)
	file.store_string('{"op": "node_count"}')
	file.close()

	var handled = editor.pump_requests(request_dir, response_dir)
	check(handled == 1, "pump_requests handled %d, expected 1" % handled)

	var reply := FileAccess.open(response_dir.path_join("001.json"), FileAccess.READ)
	check(reply != null, "no response file was written")
	if reply != null:
		var payload = parse(reply.get_as_text())
		check(payload.get("ok") == true, "bridge response: " + str(payload))
		reply.close()
	check(not FileAccess.file_exists(request_dir.path_join("001.json")),
		"the request should be consumed once answered")

	# An unknown op must be reported, not ignored.
	var bad := FileAccess.open(request_dir.path_join("002.json"), FileAccess.WRITE)
	bad.store_string('{"op": "teleport"}')
	bad.close()
	editor.pump_requests(request_dir, response_dir)
	var bad_reply := FileAccess.open(response_dir.path_join("002.json"), FileAccess.READ)
	if bad_reply != null:
		check(parse(bad_reply.get_as_text()).get("ok") == false, "an unknown op should be refused")
		bad_reply.close()

	# --- clearing the scene ------------------------------------------------
	editor.clear_scene_root()
	check(not editor.has_scene_root(), "clear_scene_root did not take effect")

	if failures.is_empty():
		print("EDITOR_PLUGIN_OK")
		quit(0)
	else:
		for failure in failures:
			print("FAIL: ", failure)
		quit(1)
'@
    [System.IO.File]::WriteAllText((Join-Path $work 'verify.gd'), $verify)

    Write-Host 'registering the extension (Godot writes extension_list.cfg on import)...'
    & $godot --headless --path $work --import 2>&1 | Out-Null
    $extensionList = Join-Path $work '.godot\extension_list.cfg'
    if (-not (Test-Path $extensionList)) {
        Write-Host 'FAIL: Godot did not register the extension (no extension_list.cfg)'
        exit 1
    }
    Write-Host "  registered: $((Get-Content $extensionList -Raw).Trim())"

    Write-Host 'running the editor surface checks...'
    $output = & $godot --headless --path $work --script res://verify.gd --quit-after 900 2>&1
    $text = $output -join "`n"

    $relevant = $output | Where-Object { $_ -match 'EDITOR_PLUGIN_OK|^FAIL:|SCRIPT ERROR|Parse Error|Failed to load' }

    if ($text -match 'EDITOR_PLUGIN_OK') {
        Write-Host '  surface     : 12 primitives exercised'
        Write-Host '  bridge      : request/response round trip verified'
        Write-Host '  conversion  : JSON -> Vector3 via property type'
        Write-Host 'EDITOR_PLUGIN_OK'
        exit 0
    }

    Write-Host 'the editor surface checks did not pass:'
    if ($relevant) { $relevant | ForEach-Object { Write-Host $_ } } else { Write-Host $text }
    exit 1
}
finally {
    # The loaded DLL can still be mapped briefly after Godot exits.
    Start-Sleep -Milliseconds 300
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
