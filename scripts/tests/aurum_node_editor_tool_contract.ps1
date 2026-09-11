[CmdletBinding()]
param(
    [string]$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$sourcePath = Join-Path $WorkspaceRoot "crates\aurum-godot\src\lib.rs"
$source = Get-Content -LiteralPath $sourcePath -Raw
$declarationPattern =
    '(?ms)#\[derive\(GodotClass\)\]\s*#\[class\((?<arguments>[^\]]*)\)\]\s*pub struct AurumNode\b'
$declarations = [regex]::Matches($source, $declarationPattern)

if ($declarations.Count -ne 1) {
    throw "Expected exactly one GodotClass declaration for AurumNode, found $($declarations.Count)"
}

$arguments = @(
    $declarations[0].Groups['arguments'].Value.Split(',') |
        ForEach-Object { $_.Trim() } |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
)

if ($arguments -notcontains 'base=Node') {
    throw "AurumNode must remain based on Node"
}
if ($arguments -notcontains 'rename=AurumNode') {
    throw "AurumNode must remain registered as AurumNode"
}
$toolArguments = @($arguments | Where-Object { $_ -eq 'tool' })
if ($toolArguments.Count -ne 1) {
    throw "AurumNode must declare the GodotClass tool attribute exactly once"
}

Write-Host "AURUM_NODE_EDITOR_TOOL_CONTRACT_OK"
