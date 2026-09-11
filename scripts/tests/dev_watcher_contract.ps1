[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$devPath = Join-Path $WorkspaceRoot "scripts\dev.ps1"
$devText = Get-Content -Raw -LiteralPath $devPath
$failures = [System.Collections.Generic.List[string]]::new()

function Require-Match {
    param(
        [string]$Pattern,
        [string]$Failure
    )

    if ($devText -notmatch $Pattern) {
        $failures.Add($Failure)
    }
}

function Require-NotMatch {
    param(
        [string]$Pattern,
        [string]$Failure
    )

    if ($devText -match $Pattern) {
        $failures.Add($Failure)
    }
}

Require-Match '\[System\.Diagnostics\.ProcessStartInfo\]::new\(\)' `
    "RunEditor must use ProcessStartInfo for exact Windows argument handling"
Require-Match 'ArgumentList\.Add\(\$GodotProject\)' `
    "RunEditor must pass GodotProject as one ArgumentList value"
Require-NotMatch '\bStart-Process\b' `
    "RunEditor must not flatten manually quoted arguments through Start-Process"
Require-Match '(?s)\$pendingSince\s*=\s*\$null\s*\r?\n\s*\$null\s*=\s*Invoke-AurumDebugBuild.*?\r?\n\s*\$postBuildStamp\s*=\s*Get-AurumSourceStamp' `
    "Watched builds must collect a source stamp after each build"
Require-Match '(?s)if\s*\(\$postBuildStamp\s*-ne\s*\$lastStamp\)\s*\{\s*\$lastStamp\s*=\s*\$postBuildStamp\s*\$pendingSince\s*=\s*\[DateTime\]::UtcNow' `
    "A source change during a watched build must schedule a follow-up build"

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Host "DEV_WATCHER_CONTRACT_OK"
