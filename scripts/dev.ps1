# Aurum development script using PowerShell polling, debug builds, and in-editor reload.
#
# Watches Rust and Cargo inputs, then rebuilds the GDExtension after a stable
# debounce window. Each successful build installs the debug DLL through the
# transactional installer in build.ps1.
#
# Usage:
#   pwsh scripts/dev.ps1
#   pwsh scripts/dev.ps1 -RunEditor
#   pwsh scripts/dev.ps1 -Once
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
    [switch]$Once,
    [ValidateRange(100, 5000)]
    [int]$PollMilliseconds = 250,
    [ValidateRange(100, 10000)]
    [int]$DebounceMilliseconds = 350,
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

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    return $null
}

function Get-AurumSourceStamp {
    param([string]$WorkspaceRoot)

    $files = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    foreach ($name in @("Cargo.toml", "Cargo.lock")) {
        $path = Join-Path $WorkspaceRoot $name
        if (Test-Path -LiteralPath $path) {
            $files.Add((Get-Item -LiteralPath $path))
        }
    }
    $crates = Join-Path $WorkspaceRoot "crates"
    if (Test-Path -LiteralPath $crates) {
        Get-ChildItem -LiteralPath $crates -Recurse -File |
            Where-Object { $_.Extension -in @(".rs", ".toml") } |
            ForEach-Object { $files.Add($_) }
    }

    $rows = $files |
        Sort-Object FullName |
        ForEach-Object {
            "$($_.FullName)|$($_.Length)|$($_.LastWriteTimeUtc.Ticks)"
        }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($rows -join "`n"))
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [Convert]::ToHexString($sha.ComputeHash($bytes))
    } finally {
        $sha.Dispose()
    }
}

function Invoke-AurumDebugBuild {
    param(
        [string]$WorkspaceRoot,
        [string]$ProjectPath,
        [string]$GodotPath
    )

    try {
        & (Join-Path $PSScriptRoot "build.ps1") `
            -DebugBuild `
            -NoTests `
            -Workspace $WorkspaceRoot `
            -GodotProject $ProjectPath `
            -GodotBinary $GodotPath
        return $LASTEXITCODE -eq 0
    } catch {
        Write-Warning "Aurum debug build failed: $($_.Exception.Message)"
        return $false
    }
}

$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
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

Write-Host "==> Aurum dev mode" -ForegroundColor Cyan
Write-Host "    Watching: $WorkspaceRoot\crates, Cargo.toml, Cargo.lock"
Write-Host "    Profile:  debug"
Write-Host "    Godot:    $GodotBinary"

$initialBuildOk = Invoke-AurumDebugBuild `
    -WorkspaceRoot $WorkspaceRoot `
    -ProjectPath $GodotProject `
    -GodotPath $GodotBinary

if ($Once) {
    if (-not $initialBuildOk) {
        throw "Initial Aurum debug build failed"
    }
    Write-Host "==> One debug build completed." -ForegroundColor Green
    return
}

if ($RunEditor -and $initialBuildOk) {
    $editorStartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $editorStartInfo.FileName = $GodotBinary
    $editorStartInfo.UseShellExecute = $false
    [void]$editorStartInfo.ArgumentList.Add("--editor")
    [void]$editorStartInfo.ArgumentList.Add("--path")
    [void]$editorStartInfo.ArgumentList.Add($GodotProject)
    [void][System.Diagnostics.Process]::Start($editorStartInfo)
}

$lastStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
$pendingSince = $null
Write-Host "    Watching for changes. Press Ctrl+C to stop."

while ($true) {
    Start-Sleep -Milliseconds $PollMilliseconds
    $currentStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
    if ($currentStamp -ne $lastStamp) {
        $lastStamp = $currentStamp
        $pendingSince = [DateTime]::UtcNow
        continue
    }
    if ($null -eq $pendingSince) {
        continue
    }
    $stableFor = ([DateTime]::UtcNow - $pendingSince).TotalMilliseconds
    if ($stableFor -lt $DebounceMilliseconds) {
        continue
    }

    $pendingSince = $null
    $null = Invoke-AurumDebugBuild `
        -WorkspaceRoot $WorkspaceRoot `
        -ProjectPath $GodotProject `
        -GodotPath $GodotBinary
    $postBuildStamp = Get-AurumSourceStamp -WorkspaceRoot $WorkspaceRoot
    if ($postBuildStamp -ne $lastStamp) {
        $lastStamp = $postBuildStamp
        $pendingSince = [DateTime]::UtcNow
    }
}
