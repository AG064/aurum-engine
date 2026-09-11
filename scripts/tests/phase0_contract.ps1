[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$failures = [System.Collections.Generic.List[string]]::new()

function Require-Match {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Failure
    )
    if ($Text -notmatch $Pattern) {
        $failures.Add($Failure)
    }
}

function Require-NotMatch {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Failure
    )
    if ($Text -match $Pattern) {
        $failures.Add($Failure)
    }
}

function Require {
    param(
        [bool]$Condition,
        [string]$Failure
    )
    if (-not $Condition) {
        $failures.Add($Failure)
    }
}

function Get-ScriptAst {
    param(
        [string]$Path,
        [string]$Name
    )

    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -gt 0) {
        $failures.Add("$Name must parse without errors")
        return $null
    }
    return $ast
}

function Get-FunctionAst {
    param(
        [System.Management.Automation.Language.ScriptBlockAst]$Ast,
        [string]$Name
    )

    if ($null -eq $Ast) {
        return $null
    }
    return $Ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $Name
        }, $true)
}

$manifestPath = Join-Path $WorkspaceRoot "godot\addons\aurum\bin\aurum.gdextension"
$buildPath = Join-Path $WorkspaceRoot "scripts\build.ps1"
$devPath = Join-Path $WorkspaceRoot "scripts\dev.ps1"

$manifest = Get-Content -Raw -LiteralPath $manifestPath
$buildAst = Get-ScriptAst -Path $buildPath -Name "build.ps1"
$devAst = Get-ScriptAst -Path $devPath -Name "dev.ps1"

Require-Match $manifest '(?m)^reloadable\s*=\s*true\s*$' `
    "GDExtension manifest must set reloadable = true"
Require-Match $manifest '(?m)^windows\.debug\.x86_64\s*=\s*"res://addons/aurum/bin/aurum_godot\.debug\.dll"\s*$' `
    "Windows debug mapping must use aurum_godot.debug.dll"
Require-Match $manifest '(?m)^windows\.release\.x86_64\s*=\s*"res://addons/aurum/bin/aurum_godot\.dll"\s*$' `
    "Windows release mapping must keep aurum_godot.dll"
if ($buildAst) {
    $targetNameAssignments = @($buildAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
                    $node.Left.VariablePath.UserPath -eq "DllTargetName"
            }, $true) | ForEach-Object { $_.Right.Extent.Text.Trim('"') })
    Require ($targetNameAssignments -contains "aurum_godot.debug.dll") `
        "build.ps1 must assign aurum_godot.debug.dll to the debug target"
    Require ($targetNameAssignments -contains "aurum_godot.dll") `
        "build.ps1 must assign aurum_godot.dll to the release target"

    $profileBranches = @($buildAst.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.IfStatementAst] -and
                $node.Clauses.Count -eq 1 -and
                $node.Clauses[0].Item1.Extent.Text.Trim() -eq '$DebugBuild'
        }, $true))
    $profileBranch = $profileBranches | Where-Object {
        $targetNameAssignment = $_.Clauses[0].Item2.Find({
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
                    $node.Left.VariablePath.UserPath -eq "DllTargetName"
            }, $true)
        $null -ne $targetNameAssignment
    } | Select-Object -Last 1
    Require ($null -ne $profileBranch) "build.ps1 must select DLL targets with the DebugBuild branch"
    if ($profileBranch) {
        $debugTargetAssignments = @($profileBranch.Clauses[0].Item2.FindAll({
                    param($node)
                    $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
                        $node.Left.VariablePath.UserPath -eq "DllTargetName"
                }, $true) | ForEach-Object { $_.Right.Extent.Text.Trim('"') })
        $releaseTargetAssignments = @($profileBranch.ElseClause.FindAll({
                    param($node)
                    $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
                        $node.Left.VariablePath.UserPath -eq "DllTargetName"
                }, $true) | ForEach-Object { $_.Right.Extent.Text.Trim('"') })
        Require ($debugTargetAssignments -contains "aurum_godot.debug.dll") `
            "DebugBuild must select aurum_godot.debug.dll"
        Require ($releaseTargetAssignments -contains "aurum_godot.dll") `
            "Release builds must select aurum_godot.dll"
    }

    $sourceInstaller = Get-FunctionAst -Ast $buildAst -Name "Install-AurumAddonSource"
    $dllInstaller = Get-FunctionAst -Ast $buildAst -Name "Install-AurumDllTransaction"
    Require ($null -ne $sourceInstaller) "build.ps1 must define Install-AurumAddonSource"
    Require ($null -ne $dllInstaller) "build.ps1 must define Install-AurumDllTransaction"

    $installCalls = @($buildAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -in @("Install-AurumAddonSource", "Install-AurumDllTransaction")
            }, $true))
    $sourceInstallCall = $installCalls | Where-Object { $_.GetCommandName() -eq "Install-AurumAddonSource" } | Select-Object -Last 1
    $dllInstallCall = $installCalls | Where-Object { $_.GetCommandName() -eq "Install-AurumDllTransaction" } | Select-Object -Last 1
    Require ($null -ne $sourceInstallCall) "build.ps1 must call Install-AurumAddonSource"
    Require ($null -ne $dllInstallCall) "build.ps1 must call Install-AurumDllTransaction"
    if ($sourceInstallCall -and $dllInstallCall) {
        Require ($sourceInstallCall.Extent.StartOffset -lt $dllInstallCall.Extent.StartOffset) `
            "build.ps1 must install add-on source before the managed DLL"
    }
}

if ($devAst) {
    $devParameterNames = @($devAst.ParamBlock.Parameters | ForEach-Object { $_.Name.VariablePath.UserPath })
    Require ($devParameterNames -contains "Once") "dev.ps1 must provide a one-build smoke mode"

    $devCommandParameters = @($devAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.CommandParameterAst] -and
                    $node.ParameterName -eq "DebugBuild"
            }, $true))
    Require ($devCommandParameters.Count -gt 0) "dev.ps1 must call the debug build path"

    $devStrings = @($devAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.StringConstantExpressionAst]
            }, $true) | ForEach-Object { $_.Value })
    Require-NotMatch ($devStrings -join "`n") 'cargo-watch' `
        "dev.ps1 must not require cargo-watch"
    Require-NotMatch ($devStrings -join "`n") 'build\s+--release\s+-p\s+aurum-godot' `
        "dev.ps1 must not use release builds in the development loop"
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Error $failure -ErrorAction Continue
    }
    exit 1
}

Write-Host "PHASE0_CONTRACT_OK"
