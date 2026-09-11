# Install the aurum command for this user on this machine.
#
# Deliberately not an installer in the distribution sense. There is no
# uninstaller, no version manifest, no update feed, and nothing is written
# outside the user's own profile. Those things exist to solve problems that
# come from shipping software to strangers -- upgrading a copy you cannot see,
# removing one you were never told about -- and this is a tool for one person
# on one machine. Adding them early would be inventing requirements.
#
# What it does, in full:
#
#   1. builds the release binary, unless told to use one already built
#   2. copies it to %LOCALAPPDATA%\AurumStudio\bin
#   3. adds that directory to the *user* PATH, never the machine PATH
#
# Every one of those is reversible by hand, and the last line it prints says
# how. An installer that leaves you unable to undo it is the thing worth
# avoiding, not the absence of an uninstaller.
#
#   pwsh scripts/install.ps1
#   pwsh scripts/install.ps1 -NoBuild        # use target/release/aurum.exe
#   pwsh scripts/install.ps1 -NoPath         # do not touch PATH
#   pwsh scripts/install.ps1 -WhatIf         # say what would happen

[CmdletBinding(SupportsShouldProcess)]
param(
    [switch]$NoBuild,
    [switch]$NoPath,
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'AurumStudio\bin')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Split-Path -Parent $PSScriptRoot
$binaryName = 'aurum.exe'
$built = Join-Path $repo "target\release\$binaryName"
$target = Join-Path $Destination $binaryName

function Write-Step {
    param([string]$Text)
    Write-Host "==> $Text"
}

# --- 1. the binary ---------------------------------------------------------

if ($NoBuild) {
    if (-not (Test-Path -LiteralPath $built -PathType Leaf)) {
        throw "no release binary at $built; run without -NoBuild, or build it with: cargo build --release -p aurum-cli"
    }
    Write-Step "using the existing $built"
} else {
    Write-Step 'building the release binary'
    & cargo build --release -p aurum-cli --manifest-path (Join-Path $repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) {
        throw "the release build failed with exit code $LASTEXITCODE"
    }
    if (-not (Test-Path -LiteralPath $built -PathType Leaf)) {
        throw "the build reported success but produced no binary at $built"
    }
}

# --- 2. copy it into place -------------------------------------------------

# Refused rather than worked around: overwriting whatever is there would be
# writing over a file this script did not put there, and the whole point of
# installing into the user's own profile is that nothing here is surprising.
if ((Test-Path -LiteralPath $target -PathType Leaf) -and
    -not (Test-Path -LiteralPath $Destination -PathType Container)) {
    throw "$target exists and is not in a directory this installer manages"
}

if ($PSCmdlet.ShouldProcess($target, 'install')) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Copy-Item -LiteralPath $built -Destination $target -Force
}
Write-Step "installed $target"

# The version is read back from the thing that was just installed rather than
# from the build, so what gets reported is what will actually run. A stale
# binary already on PATH is exactly the confusion this avoids.
#
# Skipped under -WhatIf, where nothing was installed and running the path
# would be asking a file that is not there to introduce itself.
if (-not $WhatIfPreference) {
    $version = (& $target --version) 2>&1 | Select-Object -First 1
    Write-Step "reports itself as: $version"
}

# --- 3. PATH, for this user only -------------------------------------------

$addedToPath = $false
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $userPath) { $userPath = '' }

    $entries = @($userPath -split ';' | Where-Object { $_ -ne '' })
    $alreadyThere = @($entries | Where-Object {
            $_.TrimEnd('\') -ieq $Destination.TrimEnd('\')
        }).Count -gt 0

    if ($alreadyThere) {
        Write-Step 'already on the user PATH'
    } elseif ($PSCmdlet.ShouldProcess('user PATH', "add $Destination")) {
        # The *user* PATH, not the machine one. The machine PATH needs
        # elevation, affects every account, and is the kind of change that
        # outlives the reason it was made.
        $updated = (@($entries) + $Destination) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
        $addedToPath = $true
        Write-Step "added $Destination to the user PATH"
    }
}

# --- what happened, and how to undo it -------------------------------------

Write-Host ''
Write-Host 'Installed. Open a new terminal for PATH changes to take effect, then:'
Write-Host '  aurum doctor'
Write-Host ''
Write-Host 'To undo this completely:'
Write-Host "  Remove-Item -Recurse -Force '$Destination'"
if ($addedToPath) {
    Write-Host '  and remove the entry from your user PATH:'
    Write-Host '  [Environment]::SetEnvironmentVariable(''Path'', (([Environment]::GetEnvironmentVariable(''Path'',''User'') -split '';'' |'
    Write-Host "      Where-Object { `$_.TrimEnd('\') -ine '$($Destination.TrimEnd('\'))' }) -join ';'), 'User')"
}
Write-Host ''
Write-Host 'Nothing outside your own profile was changed, and no machine-wide'
Write-Host 'setting was touched.'
