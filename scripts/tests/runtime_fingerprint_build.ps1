[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspace = [System.IO.Path]::GetFullPath((Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path)
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path $workspace "target"))
$runTargetDir = [System.IO.Path]::GetFullPath((Join-Path $targetRoot ("runtime-fingerprint-build-" + [guid]::NewGuid().ToString("N"))))
$targetRootPrefix = $targetRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $runTargetDir.StartsWith($targetRootPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
    [System.IO.Path]::GetFileName($runTargetDir) -notmatch '^runtime-fingerprint-build-[0-9a-f]{32}$') {
    throw "Fingerprint target directory must be a unique child of workspace target: $runTargetDir"
}
$dll = Join-Path $runTargetDir "debug\aurum_godot.dll"
$original = [Environment]::GetEnvironmentVariable("AURUM_RUNTIME_FINGERPRINT", "Process")
$originalTargetDir = [Environment]::GetEnvironmentVariable("CARGO_TARGET_DIR", "Process")

try {
    $env:CARGO_TARGET_DIR = $runTargetDir
    $env:AURUM_RUNTIME_FINGERPRINT = "phase0-fingerprint-a"
    & cargo build -p aurum-godot --manifest-path (Join-Path $workspace "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "First fingerprint build failed" }
    $first = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash

    $env:AURUM_RUNTIME_FINGERPRINT = "phase0-fingerprint-b"
    & cargo build -p aurum-godot --manifest-path (Join-Path $workspace "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Second fingerprint build failed" }
    $second = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash

    if ($first -eq $second) {
        throw "Changing AURUM_RUNTIME_FINGERPRINT did not change the DLL hash"
    }
    Write-Host "RUNTIME_FINGERPRINT_BUILD_OK first=$first second=$second"
    Write-Host "RUNTIME_FINGERPRINT_BUILD_BOUNDARY artifact hashes do not prove runtime-returned values; Task 4 supplies live proof"
} finally {
    try {
        if (Test-Path -LiteralPath $runTargetDir) {
            $cleanupTarget = [System.IO.Path]::GetFullPath($runTargetDir)
            if (-not $cleanupTarget.StartsWith($targetRootPrefix, [System.StringComparison]::OrdinalIgnoreCase) -or
                [System.IO.Path]::GetFileName($cleanupTarget) -notmatch '^runtime-fingerprint-build-[0-9a-f]{32}$') {
                throw "Refusing to remove unexpected fingerprint target directory: $cleanupTarget"
            }
            Remove-Item -LiteralPath $cleanupTarget -Recurse -Force
            if (Test-Path -LiteralPath $cleanupTarget) {
                throw "Fingerprint target cleanup failed: $cleanupTarget"
            }
            Write-Host "RUNTIME_FINGERPRINT_BUILD_CLEANUP_OK target=$cleanupTarget"
        }
    } finally {
        if ($null -eq $original) {
            Remove-Item Env:AURUM_RUNTIME_FINGERPRINT -ErrorAction SilentlyContinue
        } else {
            $env:AURUM_RUNTIME_FINGERPRINT = $original
        }
        if ($null -eq $originalTargetDir) {
            Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        } else {
            $env:CARGO_TARGET_DIR = $originalTargetDir
        }
    }
}
