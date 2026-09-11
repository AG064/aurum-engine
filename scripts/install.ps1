# Install the aurum command for this user, with all its state on A:.
#
# Deliberately not an installer in the distribution sense. There is no
# uninstaller, no version manifest, no update feed, and nothing is written
# outside this user's own profile and the A: drive. Those things exist to solve
# problems that come from shipping software to strangers -- upgrading a copy you
# cannot see, removing one you were never told about -- and this is a tool for
# one person on one machine.
#
# ## Why A:
#
# Studio keeps machine-specific state: which projects are registered, and the
# session records and logs for the processes it launched. By default that lands
# under %LOCALAPPDATA%, which puts it on the system drive, mixed in with every
# other application's data and easy to lose track of.
#
# This machine keeps its work on A:, so everything goes there instead: the
# binary, the registry, the sessions, the bridge, and anything fetched later.
#
# It does that by setting AURUM_STUDIO_HOME, which the code already honours,
# rather than by hardcoding a path in the source. That matters: an absolute
# machine path compiled into the tool would be wrong on every other machine,
# and this repository's own configuration says so out loud.
#
# What it does, in full:
#
#   1. builds the release binary, unless told to use one already built
#   2. copies it to <Home>\bin
#   3. points AURUM_STUDIO_HOME at <Home> for this user
#   4. moves anything already under %LOCALAPPDATA%\AurumStudio to <Home>
#   5. adds <Home>\bin to the *user* PATH, never the machine PATH
#
#   pwsh scripts/install.ps1
#   pwsh scripts/install.ps1 -NoBuild        # use target/release/aurum.exe
#   pwsh scripts/install.ps1 -NoPath         # do not touch PATH
#   pwsh scripts/install.ps1 -WhatIf         # say what would happen
#   pwsh scripts/install.ps1 -Home 'A:\Other'

[CmdletBinding(SupportsShouldProcess)]
param(
    [switch]$NoBuild,
    [switch]$NoPath,
    [string]$StudioHome = 'A:\AurumStudio'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Split-Path -Parent $PSScriptRoot
$binaryName = 'aurum.exe'
$built = Join-Path $repo "target\release\$binaryName"
$binDirectory = Join-Path $StudioHome 'bin'
$target = Join-Path $binDirectory $binaryName
$legacy = Join-Path $env:LOCALAPPDATA 'AurumStudio'

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

if ($PSCmdlet.ShouldProcess($target, 'install')) {
    New-Item -ItemType Directory -Path $binDirectory -Force | Out-Null
    Copy-Item -LiteralPath $built -Destination $target -Force
}
Write-Step "installed $target"

# Skipped under -WhatIf, where nothing was installed and running the path would
# be asking a file that is not there to introduce itself.
if (-not $WhatIfPreference) {
    $version = (& $target --version) 2>&1 | Select-Object -First 1
    Write-Step "reports itself as: $version"
}

# --- 3. point Studio's state at <Home> -------------------------------------

$currentHome = [Environment]::GetEnvironmentVariable('AURUM_STUDIO_HOME', 'User')
if ($currentHome -eq $StudioHome) {
    Write-Step "AURUM_STUDIO_HOME is already $StudioHome"
} elseif ($PSCmdlet.ShouldProcess('AURUM_STUDIO_HOME', "set to $StudioHome")) {
    # The *user* environment, not the machine one. A machine-wide variable needs
    # elevation, affects every account, and outlives the reason it was set.
    [Environment]::SetEnvironmentVariable('AURUM_STUDIO_HOME', $StudioHome, 'User')
    Write-Step "AURUM_STUDIO_HOME set to $StudioHome"
    if ($currentHome) { Write-Step "  (was $currentHome)" }
}

# --- 4. bring anything already written across ------------------------------

# Moved rather than copied, and only after the destination is known to exist, so
# a failure part-way through leaves the old tree intact rather than splitting
# the state across two places with no record of which is live.
if ((Test-Path -LiteralPath $legacy -PathType Container) -and
    $legacy.TrimEnd('\') -ine $StudioHome.TrimEnd('\')) {

    $stale = @(Get-ChildItem -LiteralPath $legacy -Force -ErrorAction SilentlyContinue)
    if ($stale.Count -gt 0) {
        Write-Step "moving existing state from $legacy"
        if ($PSCmdlet.ShouldProcess($legacy, "move to $StudioHome")) {
            New-Item -ItemType Directory -Path $StudioHome -Force | Out-Null
            foreach ($item in $stale) {
                # `bin` is skipped rather than merged. Step 2 has already put
                # the binary where it belongs, and copying a directory onto a
                # destination that already exists nests it — which produced a
                # bin\bin\aurum.exe the first time this ran.
                if ($item.Name -ieq 'bin') {
                    Write-Step '  bin (skipped: already installed above)'
                    continue
                }

                $destination = Join-Path $StudioHome $item.Name
                if (Test-Path -LiteralPath $destination) {
                    if ($item.PSIsContainer) {
                        Copy-Item -LiteralPath $item.FullName -Destination $destination -Recurse -Force
                    } else {
                        Copy-Item -LiteralPath $item.FullName -Destination $destination -Force
                    }
                } else {
                    Move-Item -LiteralPath $item.FullName -Destination $destination -Force
                }
                Write-Step "  $($item.Name)"
            }

            # Removed whether or not anything was skipped. The `bin` left behind
            # is a superseded copy of the binary, and keeping it would leave the
            # old install alive on the system drive, which is the thing this
            # step exists to end.
            Remove-Item -LiteralPath $legacy -Recurse -Force
            Write-Step "removed $legacy"
        }
    } else {
        Write-Step "nothing to move from $legacy"
        if ($PSCmdlet.ShouldProcess($legacy, 'remove the empty directory')) {
            Remove-Item -LiteralPath $legacy -Recurse -Force
        }
    }
}

# --- 5. PATH, for this user only -------------------------------------------

$addedToPath = $false
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $userPath) { $userPath = '' }

    $entries = @($userPath -split ';' | Where-Object { $_ -ne '' })
    $alreadyThere = @($entries | Where-Object {
            $_.TrimEnd('\') -ieq $binDirectory.TrimEnd('\')
        }).Count -gt 0

    if ($alreadyThere) {
        Write-Step 'already on the user PATH'
    } elseif ($PSCmdlet.ShouldProcess('user PATH', "add $binDirectory")) {
        $updated = (@($entries) + $binDirectory) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
        $addedToPath = $true
        Write-Step "added $binDirectory to the user PATH"
    }
}

# --- what happened, and how to undo it -------------------------------------

Write-Host ''
Write-Host "Installed to $StudioHome. Open a new terminal for PATH changes, then:"
Write-Host '  aurum doctor'
Write-Host ''
Write-Host 'To undo this completely:'
Write-Host "  Remove-Item -Recurse -Force '$StudioHome'"
Write-Host '  and clear the two user variables:'
Write-Host "  [Environment]::SetEnvironmentVariable('AURUM_STUDIO_HOME', `$null, 'User')"
Write-Host "  [Environment]::SetEnvironmentVariable('Path', (([Environment]::GetEnvironmentVariable('Path','User') -split ';' |"
Write-Host "      Where-Object { `$_.TrimEnd('\') -ine '$($binDirectory.TrimEnd('\'))' }) -join ';'), 'User')"
Write-Host ''
Write-Host 'No machine-wide setting was touched.'
