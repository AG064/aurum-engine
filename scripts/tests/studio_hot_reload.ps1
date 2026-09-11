# Live no-restart acceptance test for Aurum Studio.
#
# The design says completion requires live evidence, because configuration
# alone is not proof of hot reload. This is that evidence, and it is gathered
# with nothing but this repository and a Godot binary.
#
# The chain it proves:
#
#   change Rust behind the stable AurumNode API
#     -> build and install a new DLL
#       -> the running editor picks it up
#         -> the editor's own process is the one it always was
#
# What makes it evidence rather than assertion is the last link. The reload
# marker `dev.ps1` publishes records a DLL hash, which proves a *file* was
# written. Only the live fingerprint proves the *running code* changed, because
# it is read out of the extension loaded inside the editor. The Aurum editor
# plugin publishes that value to
# `res://.godot/aurum/live-fingerprint.txt` on every change.
#
# An earlier version of this test drove the observation through an npm MCP
# Inspector and a third-party Godot MCP toolkit. It depended on node, npm, a
# cached npm package, and a helper on PATH, and it left orphaned node processes
# behind when its job-object cleanup reported success. Observing a file costs
# nothing to install and cannot leak a process.
#
#   pwsh scripts/tests/studio_hot_reload.ps1
#   pwsh scripts/tests/studio_hot_reload.ps1 -Iterations 5
#   pwsh scripts/tests/studio_hot_reload.ps1 -KeepEditorOpen

[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe",
    [ValidateRange(1, 20)]
    [int]$Iterations = 3,
    [ValidateRange(30, 600)]
    [int]$TimeoutSeconds = 180,
    [switch]$KeepEditorOpen
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$sessionId = [Guid]::NewGuid().ToString("N")
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$sessionRoot = Join-Path $workspace "target\aurum-hot-reload-live\$sessionId"
$projectRoot = Join-Path $sessionRoot "project"
$fingerprintPath = Join-Path $projectRoot ".godot\aurum\live-fingerprint.txt"
$markerPath = Join-Path $projectRoot ".godot\aurum\aurum_godot.debug.reload"
$evidencePath = Join-Path $sessionRoot "evidence.json"

$editor = $null
$editorStartTime = $null
$observations = [System.Collections.Generic.List[object]]::new()
$evidence = [ordered]@{
    schema_version = 1
    session_id = $sessionId
    passed = $false
    driver = "Aurum editor plugin live fingerprint file"
    requires_node = $false
    project_path = $projectRoot
    godot_binary = $null
    editor_pid = $null
    editor_start_time_utc = $null
    editor_cleanup = "not_started"
    iterations = $Iterations
    observations = $observations
    error = $null
}

function Write-Utf8File {
    param([string]$Path, [string]$Content)
    $parent = Split-Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Save-Evidence {
    $parent = Split-Path $evidencePath -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    Write-Utf8File -Path $evidencePath -Content ($evidence | ConvertTo-Json -Depth 12)
}

function Get-LiveFingerprint {
    if (-not (Test-Path -LiteralPath $fingerprintPath -PathType Leaf)) { return $null }
    # The editor may be mid-write when this reads. A partial read simply
    # reports nothing and the caller polls again.
    try {
        $value = (Get-Content -LiteralPath $fingerprintPath -Raw -ErrorAction Stop).Trim()
    } catch {
        return $null
    }
    if ([string]::IsNullOrWhiteSpace($value)) { return $null }
    return $value
}

function Wait-ForFingerprint {
    param([string]$Expected)
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $last = "<absent>"
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($null -ne $editor -and $editor.HasExited) {
            throw "Godot editor exited with code $($editor.ExitCode) while waiting for '$Expected'"
        }
        $current = Get-LiveFingerprint
        if ($null -ne $current) {
            $last = $current
            if ($current -eq $Expected) { return $current }
        }
        Start-Sleep -Milliseconds 250
    }
    throw "The running extension never reported '$Expected'; last observed '$last'"
}

function Invoke-DebugInstall {
    param([string]$Fingerprint)
    $env:AURUM_RUNTIME_FINGERPRINT = $Fingerprint
    & (Join-Path $workspace "scripts\dev.ps1") `
        -Once `
        -GodotProject $projectRoot `
        -GodotBinary $GodotBinary *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Debug install failed for fingerprint $Fingerprint"
    }
    $installed = Join-Path $projectRoot "addons\aurum\bin\aurum_godot.debug.dll"
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) {
        throw "No installed library at $installed"
    }
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
        throw "The build did not publish its reload marker: $markerPath"
    }
    $dllHash = (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
    $markerHash = (Get-Content -LiteralPath $markerPath -Raw).Trim()
    if ($markerHash -cne $dllHash) {
        throw "Reload marker '$markerHash' does not match installed library '$dllHash'"
    }
    return $dllHash
}

function Assert-OwnedEditor {
    # Ownership is proven the way Studio proves it: the identifier alone is not
    # enough, because a recycled identifier would point at somebody else's
    # process.
    $current = Get-Process -Id $editor.Id -ErrorAction SilentlyContinue
    if ($null -eq $current) { return $null }
    if ($current.StartTime -ne $editorStartTime) {
        throw "Refusing to control PID $($editor.Id): its start time changed"
    }
    if ($current.Path -ne $evidence.godot_binary) {
        throw "Refusing to control PID $($editor.Id): its executable changed"
    }
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId = $($editor.Id)"
    if ($null -eq $cim -or [string]$cim.CommandLine -notlike "*$projectRoot*") {
        throw "Refusing to control PID $($editor.Id): it is not running our project"
    }
    return $current
}

function Stop-OwnedEditor {
    if ($KeepEditorOpen) { return "kept_open_by_request" }
    if ($null -eq $editor) { return "not_started" }
    $current = Assert-OwnedEditor
    if ($null -eq $current) { return "already_exited" }
    $null = $current.CloseMainWindow()
    if (-not $current.WaitForExit(15000)) {
        $current = Assert-OwnedEditor
        if ($null -eq $current) { return "exited_after_graceful_request" }
        $editor.Kill($true)
        if (-not $editor.WaitForExit(5000)) {
            throw "Owned editor PID $($editor.Id) did not exit after force termination"
        }
    }
    if (-not $editor.HasExited) {
        throw "Owned editor PID $($editor.Id) exit could not be verified"
    }
    return "verified_stopped"
}

$primaryError = $null
try {
    $evidence.godot_binary = (Resolve-Path $GodotBinary).Path
    New-Item -ItemType Directory -Path $sessionRoot -Force | Out-Null

    # The editor plugin under test, copied from the repository rather than from
    # the engine project, so this exercises the version in the tree.
    $pluginSource = Join-Path $workspace "godot\addons\aurum_editor"
    if (-not (Test-Path -LiteralPath $pluginSource -PathType Container)) {
        throw "The Aurum editor plugin is missing: $pluginSource"
    }

    # The project file has to exist before the first install: `dev.ps1` creates
    # the add-on directory but not the project around it.
    New-Item -ItemType Directory -Path $projectRoot -Force | Out-Null
    Write-Utf8File -Path (Join-Path $projectRoot "project.godot") -Content @'
config_version=5

[application]
config/name="Aurum Live Reload"
config/features=PackedStringArray("4.7")

[editor_plugins]
enabled=PackedStringArray("res://addons/aurum/plugin.cfg", "res://addons/aurum_editor/plugin.cfg")

[rendering]
renderer/rendering_method="gl_compatibility"
'@

    # Our own editor plugin only. No third-party toolkit is involved, which is
    # the point: verifying a reload must not require installing anything.
    $pluginTarget = Join-Path $projectRoot "addons\aurum_editor"
    New-Item -ItemType Directory -Path $pluginTarget -Force | Out-Null
    Copy-Item -Path (Join-Path $pluginSource "*") -Destination $pluginTarget -Recurse -Force

    $initialFingerprint = "live-$sessionId-0"
    $initialDll = Invoke-DebugInstall -Fingerprint $initialFingerprint

    Remove-Item -LiteralPath $fingerprintPath -ErrorAction SilentlyContinue

    $editor = Start-Process `
        -FilePath $GodotBinary `
        -ArgumentList @("--editor", "--path", "`"$projectRoot`"") `
        -PassThru
    $editorStartTime = $editor.StartTime
    $evidence.editor_pid = $editor.Id
    $evidence.editor_start_time_utc = $editorStartTime.ToUniversalTime().ToString("O")

    $observed = Wait-ForFingerprint -Expected $initialFingerprint
    $observations.Add([ordered]@{
        iteration = 0
        requested = $initialFingerprint
        observed = $observed
        dll_sha256 = $initialDll
        editor_pid = $editor.Id
    })

    for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
        $fingerprint = "live-$sessionId-$iteration"
        $dll = Invoke-DebugInstall -Fingerprint $fingerprint

        # A new install must not be mistaken for the previous one, so the
        # previous value is not accepted as proof of this reload.
        $observed = Wait-ForFingerprint -Expected $fingerprint
        $observations.Add([ordered]@{
            iteration = $iteration
            requested = $fingerprint
            observed = $observed
            dll_sha256 = $dll
            editor_pid = $editor.Id
        })
    }

    $current = Assert-OwnedEditor
    if ($null -eq $current) {
        throw "The owned editor exited during the run; a restart would invalidate the evidence"
    }
    $pids = @($observations | ForEach-Object { [int]$_['editor_pid'] } | Sort-Object -Unique)
    if ($pids.Count -ne 1) {
        throw "The editor process changed during the run: $($pids -join ', ')"
    }
    if ($current.Id -ne $editor.Id) {
        throw "The editor process is not the one that was started"
    }

    $evidence.passed = $true
    Write-Host (
        "STUDIO_HOT_RELOAD_OK iterations=$Iterations " +
        "editor_pid=$($editor.Id) " +
        "fingerprints=$(@($observations | ForEach-Object { $_.observed }) -join ',')")
    Write-Host (
        "  one editor process served every rebuild; " +
        "each new fingerprint was read out of the running extension")
} catch {
    $primaryError = $_
    $evidence.error = $_.Exception.Message
} finally {
    try {
        $evidence.editor_cleanup = Stop-OwnedEditor
    } catch {
        if ($null -eq $primaryError) { $primaryError = $_ }
    }
    Save-Evidence
}

if ($null -ne $primaryError) {
    Write-Host "evidence: $evidencePath"
    throw $primaryError
}
