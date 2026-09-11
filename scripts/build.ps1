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
#   pwsh scripts/build.ps1 -DebugBuild -RunEditor # debug build then open editor
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
    [switch]$RunEditor,
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

function Install-AurumAddonSource {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SourceAddon,
        [Parameter(Mandatory = $true)]
        [string]$TargetAddon
    )

    $sourceRoot = [System.IO.Path]::GetFullPath($SourceAddon)
    $targetRoot = [System.IO.Path]::GetFullPath($TargetAddon)
    Get-ChildItem -LiteralPath $sourceRoot -Recurse -File -Force | ForEach-Object {
        $relativePath = [System.IO.Path]::GetRelativePath($sourceRoot, $_.FullName)
        if ($relativePath -match '^(bin[\\/]).*\.dll$') {
            return
        }
        $destination = Join-Path $targetRoot $relativePath
        $destinationDirectory = Split-Path $destination -Parent
        if (-not (Test-Path -LiteralPath $destinationDirectory)) {
            New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
        }
        Copy-Item -LiteralPath $_.FullName -Destination $destination -Force
    }
}

function Install-AurumDllTransaction {
    param(
        [Parameter(Mandatory = $true)]
        [string]$DllSource,
        [Parameter(Mandatory = $true)]
        [string]$DllTarget,
        [Parameter(Mandatory = $true)]
        [string]$DllTargetName,
        [Parameter(Mandatory = $true)]
        [string]$Profile
    )

    $transactionId = [guid]::NewGuid().ToString("N")
    $stagedTarget = "$DllTarget.stage.$transactionId"
    $backupTarget = "$DllTarget.backup.$transactionId"
    foreach ($transientPath in @($stagedTarget, $backupTarget)) {
        if (Test-Path -LiteralPath $transientPath) {
            throw "Transaction artifact already exists and will not be overwritten: $transientPath"
        }
    }

    $sourceHash = $null
    for ($stableAttempt = 0; $stableAttempt -lt 10; $stableAttempt++) {
        $firstHash = (Get-FileHash -LiteralPath $DllSource -Algorithm SHA256).Hash
        Start-Sleep -Milliseconds 250
        $secondHash = (Get-FileHash -LiteralPath $DllSource -Algorithm SHA256).Hash
        if ($firstHash -eq $secondHash) {
            $sourceHash = $secondHash
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($sourceHash)) {
        throw "$Profile DLL did not become stable: $DllSource"
    }

    $committed = $false
    try {
        for ($copyAttempt = 1; $copyAttempt -le 5; $copyAttempt++) {
            try {
                Copy-Item -LiteralPath $DllSource -Destination $stagedTarget -Force
                $stagedHash = (Get-FileHash -LiteralPath $stagedTarget -Algorithm SHA256).Hash
                if ($stagedHash -ne $sourceHash) {
                    throw "Staged DLL hash does not match source: $stagedTarget"
                }
            } catch {
                if ($copyAttempt -eq 5) {
                    throw "Could not prepare $DllTargetName for atomic installation: $($_.Exception.Message)"
                }
                if (Test-Path -LiteralPath $stagedTarget) {
                    Remove-Item -LiteralPath $stagedTarget -Force -ErrorAction SilentlyContinue
                }
                Start-Sleep -Milliseconds 250
                continue
            }

            try {
                if (Test-Path -LiteralPath $DllTarget) {
                    [System.IO.File]::Replace($stagedTarget, $DllTarget, $backupTarget, $true)
                } else {
                    [System.IO.File]::Move($stagedTarget, $DllTarget)
                }
            } catch {
                throw "Could not atomically install ${DllTargetName}: $($_.Exception.Message)"
            }

            $committed = $true
            break
        }

        if (-not $committed) {
            throw "DLL installation did not complete: $DllTarget"
        }
    } finally {
        if (Test-Path -LiteralPath $stagedTarget) {
            Remove-Item -LiteralPath $stagedTarget -Force -ErrorAction SilentlyContinue
        }
        if ($committed -and (Test-Path -LiteralPath $backupTarget)) {
            Remove-Item -LiteralPath $backupTarget -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host "==> Installed $DllTargetName to $DllTarget" -ForegroundColor Green
    return $sourceHash
}

function Resolve-AurumLaunchMode {
    param(
        [switch]$DebugBuild,
        [switch]$Run,
        [switch]$Editor,
        [switch]$RunEditor
    )

    if ($RunEditor) {
        $Run = $true
        $Editor = $true
    }
    if ($Editor -and -not $DebugBuild) {
        throw "Opening the editor requires -DebugBuild so aurum_godot.debug.dll is installed"
    }
    return [pscustomobject]@{
        Run = [bool]$Run
        Editor = [bool]$Editor
    }
}

function Publish-AurumDebugReloadMarker {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ProjectPath,
        [Parameter(Mandatory = $true)]
        [string]$VerifiedDllHash
    )

    if ($VerifiedDllHash -cnotmatch '^[0-9A-F]{64}$') {
        throw "Verified debug DLL hash is not a SHA-256 value"
    }

    $projectRoot = [System.IO.Path]::GetFullPath($ProjectPath)
    $markerDirectory = Join-Path $projectRoot ".godot\aurum"
    $markerPath = Join-Path $markerDirectory "aurum_godot.debug.reload"
    $stagedMarker = "$markerPath.stage.$([guid]::NewGuid().ToString('N'))"
    try {
        if (-not (Test-Path -LiteralPath $markerDirectory)) {
            New-Item -ItemType Directory -Path $markerDirectory -Force | Out-Null
        }
        if (-not (Test-Path -LiteralPath $markerDirectory -PathType Container)) {
            throw "Debug reload marker directory is not a directory: $markerDirectory"
        }

        $encoding = [System.Text.UTF8Encoding]::new($false)
        [System.IO.File]::WriteAllText(
            $stagedMarker,
            "$VerifiedDllHash`n",
            $encoding)
        $stagedHash = (Get-Content -LiteralPath $stagedMarker -Raw).Trim()
        if ($stagedHash -cne $VerifiedDllHash) {
            throw "Staged debug reload marker did not preserve the verified DLL hash"
        }
        [System.IO.File]::Move($stagedMarker, $markerPath, $true)

        $publishedHash = (Get-Content -LiteralPath $markerPath -Raw).Trim()
        if ($publishedHash -cne $VerifiedDllHash) {
            throw "Published debug reload marker did not preserve the verified DLL hash"
        }
    } finally {
        if (Test-Path -LiteralPath $stagedMarker) {
            Remove-Item -LiteralPath $stagedMarker -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host "==> Published Aurum debug reload marker $markerPath" -ForegroundColor Green
}

$launchMode = Resolve-AurumLaunchMode `
    -DebugBuild:$DebugBuild `
    -Run:$Run `
    -Editor:$Editor `
    -RunEditor:$RunEditor
$Run = $launchMode.Run
$Editor = $launchMode.Editor

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
    $DllTargetName = "aurum_godot.debug.dll"
} else {
    $DllSource = Join-Path $Workspace "target\release\aurum_godot.dll"
    $DllTargetName = "aurum_godot.dll"
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
    Install-AurumAddonSource -SourceAddon $SourceAddon -TargetAddon $TargetAddon
    $SourceRuntime = Join-Path $Workspace "godot\scripts\aurum_runtime.gd"
    $TargetRuntime = Join-Path $TargetAddon "scripts\aurum_runtime.gd"
    Copy-Item -Path $SourceRuntime -Destination $TargetRuntime -Force
    Write-Host "==> Installed Aurum add-on source to $TargetAddon" -ForegroundColor Green
}

# Copy the native binary after installing the source tree. The source add-on
# also contains a reference binary, so copying in the other order can replace
# the freshly built DLL with a stale one.
$DllTarget = Join-Path $AddOnBin $DllTargetName
$installedDllHash = Install-AurumDllTransaction `
    -DllSource $DllSource `
    -DllTarget $DllTarget `
    -DllTargetName $DllTargetName `
    -Profile $Profile
if ($DebugBuild) {
    try {
        Publish-AurumDebugReloadMarker `
            -ProjectPath $GodotProject `
            -VerifiedDllHash $installedDllHash
    } catch {
        Write-Warning (
            "The debug DLL was installed and verified, but its reload marker " +
            "could not be published: $($_.Exception.Message). " +
            "Save editor work and use a controlled editor restart if needed.")
    }
}

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
