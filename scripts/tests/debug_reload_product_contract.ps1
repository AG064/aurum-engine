[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$buildScript = Join-Path $workspace "scripts\build.ps1"
$pluginScript = Join-Path $workspace "godot\addons\aurum\plugin.gd"
$contractParent = [System.IO.Path]::GetFullPath(
    (Join-Path $workspace "target\aurum-debug-reload-contract"))
$contractRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $contractParent ([Guid]::NewGuid().ToString("N"))))
$allowedPrefix = $contractParent.TrimEnd('\') + '\'
if (-not $contractRoot.StartsWith(
        $allowedPrefix,
        [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to use contract path outside $contractParent"
}

function Invoke-DebugBuild {
    param([string]$ProjectPath)

    & $buildScript `
        -DebugBuild `
        -NoTests `
        -Workspace $workspace `
        -GodotProject $ProjectPath `
        -GodotBinary $GodotBinary
    if ($LASTEXITCODE -ne 0) {
        throw "Debug build returned exit code $LASTEXITCODE"
    }
}

function Assert-Match {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Failure
    )
    if ($Text -notmatch $Pattern) {
        throw $Failure
    }
}

try {
    $normalProject = Join-Path $contractRoot "normal-project"
    New-Item -ItemType Directory -Path $normalProject -Force | Out-Null
    Invoke-DebugBuild -ProjectPath $normalProject

    $installedDll = Join-Path `
        $normalProject `
        "addons\aurum\bin\aurum_godot.debug.dll"
    $marker = Join-Path `
        $normalProject `
        ".godot\aurum\aurum_godot.debug.reload"
    if (-not (Test-Path -LiteralPath $installedDll -PathType Leaf)) {
        throw "Debug build did not install the DLL: $installedDll"
    }
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Debug build did not publish the reload marker: $marker"
    }
    $installedHash = (Get-FileHash -LiteralPath $installedDll -Algorithm SHA256).Hash
    $markerHash = (Get-Content -LiteralPath $marker -Raw).Trim()
    if ($markerHash -cne $installedHash) {
        throw "Reload marker hash '$markerHash' did not match installed DLL hash '$installedHash'"
    }

    $blockedProject = Join-Path $contractRoot "blocked-marker-project"
    $blockedGodotDirectory = Join-Path $blockedProject ".godot"
    New-Item -ItemType Directory -Path $blockedGodotDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $blockedGodotDirectory "aurum"),
        "marker directory intentionally blocked")
    Invoke-DebugBuild -ProjectPath $blockedProject
    $blockedInstalledDll = Join-Path `
        $blockedProject `
        "addons\aurum\bin\aurum_godot.debug.dll"
    if (-not (Test-Path -LiteralPath $blockedInstalledDll -PathType Leaf)) {
        throw "Marker publication failure invalidated the verified DLL install"
    }

    $plugin = Get-Content -LiteralPath $pluginScript -Raw
    Assert-Match $plugin '(?m)^@tool\s*\r?\nextends EditorPlugin\s*$' `
        "Aurum native reload must be owned by the existing tool EditorPlugin"
    Assert-Match $plugin `
        'res://\.godot/aurum/aurum_godot\.debug\.reload' `
        "EditorPlugin must poll the debug reload marker"
    Assert-Match $plugin 'GDExtensionManager\.get_loaded_extensions\(\)' `
        "EditorPlugin must inspect loaded manifests"
    Assert-Match $plugin 'aurum_extensions\.size\(\) != 1' `
        "EditorPlugin must require exactly one loaded Aurum manifest"
    Assert-Match $plugin 'GDExtensionManager\.reload_extension\(' `
        "EditorPlugin must request native reload"
    Assert-Match $plugin 'status == OK' `
        "EditorPlugin must accept reload only when Godot returns OK"
    Assert-Match $plugin '_reload_in_progress' `
        "EditorPlugin must guard reload reentrancy"
    Assert-Match $plugin '_failed_marker_hash' `
        "EditorPlugin must suppress retry storms for a failed marker"
    Assert-Match $plugin 'controlled editor restart' `
        "EditorPlugin must warn about the controlled restart fallback"

    Write-Host (
        "DEBUG_RELOAD_PRODUCT_CONTRACT_OK " +
        "marker_sha256=$markerHash blocked_marker_install=verified")
} finally {
    if (Test-Path -LiteralPath $contractRoot) {
        Remove-Item -LiteralPath $contractRoot -Recurse -Force
    }
}
