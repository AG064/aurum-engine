# Aurum build script.
#
# Builds the Rust workspace in release mode (or debug), copies the
# GDExtension DLL into the Godot project's add-on bin, runs the test
# suite (release mode only), and optionally launches Godot.
#
# Usage:
#   pwsh scripts/build.ps1                       # release + copy DLL + tests
#   pwsh scripts/build.ps1 -DebugBuild           # debug, skip tests
#   pwsh scripts/build.ps1 -Run                  # build then run the demo
#   pwsh scripts/build.ps1 -Run -Editor          # build then open editor
#   pwsh scripts/build.ps1 -NoTests              # skip tests
#   pwsh scripts/build.ps1 -GodotProject <path> # custom Godot project
#   pwsh scripts/build.ps1 -GodotBinary <path>  # custom Godot binary
#   pwsh scripts/build.ps1 -Workspace <path>    # custom workspace root
#
# Godot binary resolution (in order):
#   1. -GodotBinary parameter
#   2. $env:AURUM_GODOT
#   3. "godot" or "godot4" on PATH
#   4. $Workspace/godot/Godot_v4.7-stable_*.exe (or /Godot_v4.7-stable_*.x86_64)
#   5. Common sibling locations of $GodotProject

[CmdletBinding()]
param(
    [switch]$DebugBuild,
    [switch]$Run,
    [switch]$Editor,
    [switch]$NoTests,
    [string]$Workspace,
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

    # PATH lookup — accept "godot" or "godot4"
    foreach ($name in @("godot", "godot4")) {
        $cmd = Get-Command $name -ErrorAction SilentlyContinue
        if ($cmd -and $cmd.Source) {
            $candidates += $cmd.Source
            break
        }
    }

    # Workspace-relative default
    if ($WorkspaceRoot) {
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_win64.exe"
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_linux.x86_64"
        $candidates += Join-Path $WorkspaceRoot "godot/Godot_v4.7-stable_macos.universal"
    }

    # Walk up from the project path looking for sibling godot/ folders
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

if (-not $Workspace) {
    $Workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
} else {
    $Workspace = Resolve-Path $Workspace
}
if (-not $GodotProject) {
    $GodotProject = Join-Path $Workspace "godot"
}

if (-not $GodotBinary) {
    $GodotBinary = Find-AurumGodot -WorkspaceRoot $Workspace -ProjectPath $GodotProject -Explicit $null
    if (-not $GodotBinary) {
        throw "Could not locate a Godot binary. Set one of:`n" +
              "  -GodotBinary <path>`n" +
              "  `$env:AURUM_GODOT = <path>`n" +
              "  or place Godot_v4.7-stable_*.{exe,x86_64,universal} at:`n" +
              "    $Workspace/godot/`n" +
              "    or alongside the Godot project (sibling 'godot/' folder)."
    }
}

$AddOnBin = Join-Path $GodotProject "addons\aurum\bin"
$SourceAddon = [System.IO.Path]::GetFullPath((Join-Path $Workspace "godot\addons\aurum"))
$TargetAddon = [System.IO.Path]::GetFullPath((Join-Path $GodotProject "addons\aurum"))

if ($DebugBuild) {
    $Profile = "debug"
    $CargoArgs = @("build", "-p", "aurum-godot")
} else {
    $Profile = "release"
    $CargoArgs = @("build", "--release", "-p", "aurum-godot")
}

Write-Host "==> Aurum build ($Profile)" -ForegroundColor Cyan
Write-Host "    Workspace:    $Workspace"
Write-Host "    Godot project: $GodotProject"
Write-Host "    Godot binary:  $GodotBinary"
Write-Host "    Add-on bin:    $AddOnBin"
Write-Host ""

# ---------- 1. Build aurum-godot ----------

Push-Location $Workspace
try {
    Write-Host "==> cargo $($CargoArgs -join ' ')" -ForegroundColor Green
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build (aurum-godot) failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if ($DebugBuild) {
    $DllSource = Join-Path $Workspace "target\debug\aurum_godot.dll"
} else {
    $DllSource = Join-Path $Workspace "target\release\aurum_godot.dll"
}

if (-not (Test-Path $DllSource)) {
    throw "Build succeeded but DLL not found at: $DllSource"
}

if (-not (Test-Path $AddOnBin)) {
    New-Item -ItemType Directory -Path $AddOnBin -Force | Out-Null
}

# Install the engine add-on source as well as the binary. This keeps game
# projects on the same Aurum runtime API as the DLL they receive.
if ($SourceAddon.TrimEnd('\') -ne $TargetAddon.TrimEnd('\')) {
    if (-not (Test-Path $TargetAddon)) {
        New-Item -ItemType Directory -Path $TargetAddon -Force | Out-Null
    }
    Copy-Item -Path (Join-Path $SourceAddon '*') -Destination $TargetAddon -Recurse -Force
    $SourceRuntime = Join-Path $Workspace "godot\scripts\aurum_runtime.gd"
    $TargetRuntime = Join-Path $TargetAddon "scripts\aurum_runtime.gd"
    Copy-Item -Path $SourceRuntime -Destination $TargetRuntime -Force
    Write-Host "==> Installed Aurum add-on source to $TargetAddon" -ForegroundColor Green
}

# Copy the native binary after installing the source tree. The source add-on
# also contains a reference binary, so copying in the other order can replace
# the freshly built DLL with a stale one.
$DllTarget = Join-Path $AddOnBin "aurum_godot.dll"
$dll_verified = $false
for ($copy_attempt = 0; $copy_attempt -lt 5; $copy_attempt++) {
    $source_hash = $null
    for ($stable_attempt = 0; $stable_attempt -lt 10; $stable_attempt++) {
        $first_hash = (Get-FileHash -Path $DllSource -Algorithm SHA256).Hash
        Start-Sleep -Milliseconds 250
        $second_hash = (Get-FileHash -Path $DllSource -Algorithm SHA256).Hash
        if ($first_hash -eq $second_hash) {
            $source_hash = $second_hash
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($source_hash)) {
        throw "Release DLL did not become stable: $DllSource"
    }
    Copy-Item -Path $DllSource -Destination $DllTarget -Force
    $target_hash = (Get-FileHash -Path $DllTarget -Algorithm SHA256).Hash
    if ($source_hash -eq $target_hash) {
        $dll_verified = $true
        break
    }
}
if (-not $dll_verified) {
    throw "Copied DLL hash does not match source: $DllSource"
}
Write-Host "==> Copied aurum_godot.dll to $DllTarget" -ForegroundColor Green

# ---------- 2. Tests ----------

if (-not $DebugBuild -and -not $NoTests) {
    Write-Host ""
    Write-Host "==> Running workspace tests" -ForegroundColor Green
    Push-Location $Workspace
    try {
        & cargo test --workspace --quiet
    } finally {
        Pop-Location
    }
}

# ---------- 3. Optionally run Godot ----------

if ($Run) {
    if (-not (Test-Path $GodotBinary)) {
        throw "Godot binary not found at: $GodotBinary"
    }
    $Args = @()
    if ($Editor) {
        $Args += "--editor"
    } else {
        $Args += "--path"
        $Args += $GodotProject
    }
    Write-Host ""
    Write-Host "==> Launching Godot: $GodotBinary $($Args -join ' ')" -ForegroundColor Magenta
    & $GodotBinary @Args
}

Write-Host ""
Write-Host "==> Done." -ForegroundColor Cyan
