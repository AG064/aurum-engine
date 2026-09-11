[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Require {
    param(
        [bool]$Condition,
        [string]$Failure
    )
    if (-not $Condition) {
        throw $Failure
    }
}

function Get-BuildFunction {
    param(
        [string]$BuildPath,
        [string]$Name
    )

    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $BuildPath,
        [ref]$tokens,
        [ref]$parseErrors
    )
    if ($parseErrors.Count -gt 0) {
        throw "build.ps1 has parse errors"
    }
    $functionAst = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $Name
        }, $true)
    if (-not $functionAst) {
        throw "build.ps1 must define $Name"
    }
    return $functionAst.Extent.Text
}

$buildPath = Join-Path $WorkspaceRoot "scripts\build.ps1"
. ([scriptblock]::Create((Get-BuildFunction -BuildPath $buildPath -Name "Resolve-AurumLaunchMode")))

$readme = Get-Content -Raw -LiteralPath (Join-Path $WorkspaceRoot "README.md")
$gettingStarted = Get-Content -Raw -LiteralPath (Join-Path $WorkspaceRoot "docs\GETTING_STARTED.md")
Require ($readme -match 'pwsh scripts/build\.ps1 -DebugBuild -RunEditor') `
    "README quick start must use -DebugBuild -RunEditor"
Require ($gettingStarted -match 'pwsh scripts/build\.ps1 -DebugBuild -RunEditor') `
    "GETTING_STARTED editor command must use -DebugBuild -RunEditor"

$debugEditor = Resolve-AurumLaunchMode -DebugBuild -RunEditor
Require $debugEditor.Run "-DebugBuild -RunEditor must request a run"
Require $debugEditor.Editor "-DebugBuild -RunEditor must request the editor"

$rejected = $false
try {
    Resolve-AurumLaunchMode -RunEditor
} catch {
    $rejected = $true
}
Require $rejected "-RunEditor without -DebugBuild must be rejected before work"

$legacyRejected = $false
try {
    Resolve-AurumLaunchMode -Run -Editor
} catch {
    $legacyRejected = $true
}
Require $legacyRejected "-Run -Editor without -DebugBuild must be rejected before work"

$rejectionOutput = (& pwsh -NoProfile -File $buildPath -RunEditor 2>&1 | Out-String)
$rejectionExitCode = $LASTEXITCODE
Require ($rejectionExitCode -eq 1) "build.ps1 -RunEditor must fail before starting a build"
Require ($rejectionOutput -match 'requires -DebugBuild') `
    "build.ps1 -RunEditor must explain the debug-editor requirement"

Write-Host "EDITOR_DEBUG_PAIRING_CONTRACT_OK"
