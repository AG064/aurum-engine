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

$script:stageHashFailuresRemaining = 0
$script:stageHashCalls = 0
function Get-FileHash {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LiteralPath,
        [Parameter(Mandatory = $true)]
        [string]$Algorithm
    )

    $result = Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm $Algorithm
    if ($LiteralPath -like "*.stage.*") {
        $script:stageHashCalls++
        if ($script:stageHashFailuresRemaining -gt 0) {
            $script:stageHashFailuresRemaining--
            return [pscustomobject]@{ Hash = "0" * 64 }
        }
    }
    return $result
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

function Require-NoArtifacts {
    param(
        [string]$Directory,
        [string]$TargetName
    )

    foreach ($suffix in @("stage", "backup", "rollback")) {
        Require (-not (Get-ChildItem -LiteralPath $Directory -Filter "$TargetName.$suffix.*" -Force)) `
            "$TargetName must not leave $suffix artifacts"
    }
}

$buildPath = Join-Path $WorkspaceRoot "scripts\build.ps1"
$installerSource = Get-BuildFunction -BuildPath $buildPath -Name "Install-AurumDllTransaction"
Require ($installerSource -notmatch 'VerifyInstalled') `
    "Install-AurumDllTransaction must not expose post-commit verification"
. ([scriptblock]::Create((Get-BuildFunction -BuildPath $buildPath -Name "Install-AurumAddonSource")))
. ([scriptblock]::Create($installerSource))

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("aurum-phase0-transaction-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null

try {
    $sourceAddon = Join-Path $tempRoot "source-addon"
    $targetAddon = Join-Path $tempRoot "target-addon"
    $sourceBin = Join-Path $sourceAddon "bin"
    $targetBin = Join-Path $targetAddon "bin"
    New-Item -ItemType Directory -Path $sourceBin -Force | Out-Null
    New-Item -ItemType Directory -Path $targetBin -Force | Out-Null

    Set-Content -LiteralPath (Join-Path $sourceAddon "plugin.cfg") -Value "source-plugin" -NoNewline
    Set-Content -LiteralPath (Join-Path $sourceBin "aurum.gdextension") -Value "source-manifest" -NoNewline
    Set-Content -LiteralPath (Join-Path $sourceBin "aurum_godot.dll") -Value "source-native" -NoNewline
    Set-Content -LiteralPath (Join-Path $targetBin "aurum_godot.dll") -Value "previous-release" -NoNewline

    Install-AurumAddonSource -SourceAddon $sourceAddon -TargetAddon $targetAddon
    Require ((Get-Content -Raw -LiteralPath (Join-Path $targetAddon "plugin.cfg")) -eq "source-plugin") `
        "Source add-on files must install"
    Require ((Get-Content -Raw -LiteralPath (Join-Path $targetBin "aurum.gdextension")) -eq "source-manifest") `
        "Source manifest must install"
    Require ((Get-Content -Raw -LiteralPath (Join-Path $targetBin "aurum_godot.dll")) -eq "previous-release") `
        "Source add-on copy must not overwrite a managed DLL"

    $dllSource = Join-Path $tempRoot "aurum_godot.dll"
    Set-Content -LiteralPath $dllSource -Value "new-build" -NoNewline
    $sourceHash = (Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $dllSource -Algorithm SHA256).Hash

    $absentSuccessTarget = Join-Path $targetBin "absent-success.dll"
    $installedHash = Install-AurumDllTransaction -DllSource $dllSource -DllTarget $absentSuccessTarget -DllTargetName "absent-success.dll" -Profile "debug"
    Require ($installedHash -eq $sourceHash) "Successful absent-target install must return the staged source hash"
    Require ((Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $absentSuccessTarget -Algorithm SHA256).Hash -eq $sourceHash) `
        "Successful absent-target install must hash-match the source"
    Require-NoArtifacts -Directory $targetBin -TargetName "absent-success.dll"

    $existingSuccessTarget = Join-Path $targetBin "existing-success.dll"
    Set-Content -LiteralPath $existingSuccessTarget -Value "previous-existing" -NoNewline
    $installedHash = Install-AurumDllTransaction -DllSource $dllSource -DllTarget $existingSuccessTarget -DllTargetName "existing-success.dll" -Profile "debug"
    Require ($installedHash -eq $sourceHash) "Successful existing-target install must return the staged source hash"
    Require ((Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $existingSuccessTarget -Algorithm SHA256).Hash -eq $sourceHash) `
        "Successful existing-target install must hash-match the source"
    Require-NoArtifacts -Directory $targetBin -TargetName "existing-success.dll"

    $failedExistingTarget = Join-Path $targetBin "failed-existing.dll"
    Set-Content -LiteralPath $failedExistingTarget -Value "previous-failed-existing" -NoNewline
    $script:stageHashFailuresRemaining = 5
    $script:stageHashCalls = 0
    $preCommitFailure = $false
    try {
        Install-AurumDllTransaction -DllSource $dllSource -DllTarget $failedExistingTarget -DllTargetName "failed-existing.dll" -Profile "debug"
    } catch {
        $preCommitFailure = $true
    }
    Require $preCommitFailure "A persistent staged-hash mismatch must fail before commit"
    Require ($script:stageHashCalls -eq 5) "Pre-commit failure must verify a fresh stage for every retry, got $script:stageHashCalls"
    Require ((Get-Content -Raw -LiteralPath $failedExistingTarget) -eq "previous-failed-existing") `
        "Pre-commit failure must leave an existing target byte-identical"
    Require-NoArtifacts -Directory $targetBin -TargetName "failed-existing.dll"

    $failedAbsentTarget = Join-Path $targetBin "failed-absent.dll"
    $script:stageHashFailuresRemaining = 5
    $preCommitFailure = $false
    try {
        Install-AurumDllTransaction -DllSource $dllSource -DllTarget $failedAbsentTarget -DllTargetName "failed-absent.dll" -Profile "debug"
    } catch {
        $preCommitFailure = $true
    }
    Require $preCommitFailure "A persistent staged-hash mismatch must fail before absent-target commit"
    Require (-not (Test-Path -LiteralPath $failedAbsentTarget)) `
        "Pre-commit failure must leave an absent target absent"
    Require-NoArtifacts -Directory $targetBin -TargetName "failed-absent.dll"

    $retryTarget = Join-Path $targetBin "retry.dll"
    Set-Content -LiteralPath $retryTarget -Value "previous-retry" -NoNewline
    $script:stageHashFailuresRemaining = 1
    $script:stageHashCalls = 0
    $installedHash = Install-AurumDllTransaction -DllSource $dllSource -DllTarget $retryTarget -DllTargetName "retry.dll" -Profile "debug"
    Require ($script:stageHashCalls -eq 2) "A transient pre-commit failure must retry with a fresh stage"
    Require ($installedHash -eq $sourceHash) "Successful retry must return the staged source hash"
    Require ((Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $retryTarget -Algorithm SHA256).Hash -eq $sourceHash) `
        "Successful retry must install source-matching content"
    Require-NoArtifacts -Directory $targetBin -TargetName "retry.dll"

    Write-Host "PHASE0_TRANSACTION_OK"
} finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
