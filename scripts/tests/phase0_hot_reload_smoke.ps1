[CmdletBinding()]
param(
    [string]$GodotBinary = "A:\RecoveredProjects\C_Drive\Game_Development\godot\Godot_v4.7-stable_win64.exe",
    [ValidateRange(1, 20)]
    [int]$Iterations = 5,
    [ValidateRange(30, 600)]
    [int]$TimeoutSeconds = 180,
    [switch]$KeepEditorOpen,
    [switch]$OwnershipHelperTest,
    [switch]$ProvenanceContractTest
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$InspectorPackageName = "@modelcontextprotocol/inspector"
$InspectorPackageVersion = "2.4.0"
$InspectorPackageSpec = "@modelcontextprotocol/inspector@2.4.0"
$sessionId = [Guid]::NewGuid().ToString("N")
$workspace = Join-Path $PSScriptRoot "..\.."
$sessionRoot = Join-Path $workspace "target\aurum-hot-reload-smoke\$sessionId"
$projectRoot = Join-Path $sessionRoot "project"
$addonsRoot = Join-Path $projectRoot "addons"
$evidencePath = Join-Path $sessionRoot "evidence.json"
$mcpConfigPath = Join-Path $sessionRoot "mcp.json"
$editor = $null
$editorStartTime = $null
$originalFingerprint = $null
$originalFingerprintCaptured = $false
$inspectorProvenance = $null
$observations = [System.Collections.Generic.List[object]]::new()
$attempts = [System.Collections.Generic.List[object]]::new()
$evidence = [ordered]@{
    schema_version = 3
    session_id = $sessionId
    passed = $false
    project_path = $projectRoot
    godot_binary = $GodotBinary
    editor_pid = $null
    editor_start_time_utc = $null
    inspector_requests_started = 0
    inspector_cleanups_verified = 0
    inspector_package_spec = $InspectorPackageSpec
    inspector_offline = $true
    inspector_package_integrity = $null
    inspector_package_json_sha256 = $null
    inspector_bin_mapping = $null
    inspector_entry_path = $null
    inspector_entry_sha256 = $null
    node_version = $null
    npm_npx_version = $null
    editor_cleanup = "not_started"
    reload_driver = "Aurum EditorPlugin debug marker polling"
    attempts = $attempts
    observations = $observations
    error = $null
}

function Write-Utf8File {
    param([string]$Path, [string]$Content)
    $parent = Split-Path $Path -Parent
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Select-UniqueInspectorCacheCandidate {
    param([AllowEmptyCollection()][object[]]$Candidates)

    $candidateList = @($Candidates)
    if ($candidateList.Count -ne 1) {
        throw (
            "Expected exactly one exact-version cached Inspector installation; " +
            "found $($candidateList.Count)")
    }
    return $candidateList[0]
}

function Assert-ResolvedPathWithinRoot {
    param(
        [string]$RootPath,
        [string]$CandidatePath
    )

    $rootFullPath = [System.IO.Path]::GetFullPath($RootPath).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar)
    $candidateFullPath = [System.IO.Path]::GetFullPath($CandidatePath)
    $relativePath = [System.IO.Path]::GetRelativePath(
        $rootFullPath, $candidateFullPath)
    if ([System.IO.Path]::IsPathRooted($relativePath) -or
        $relativePath -eq ".." -or
        $relativePath.StartsWith(
            "..$([System.IO.Path]::DirectorySeparatorChar)",
            [System.StringComparison]::Ordinal)) {
        throw "Inspector entry resolves outside its package root"
    }
    return $candidateFullPath
}

function Resolve-VerifiedInspectorEntry {
    param([string]$PackageJsonPath)

    $resolvedPackageJson = (Resolve-Path $PackageJsonPath).Path
    $packageRoot = Split-Path $resolvedPackageJson -Parent
    $metadata = Get-Content -LiteralPath $resolvedPackageJson -Raw |
        ConvertFrom-Json -Depth 30
    if ([string]$metadata.name -ne $InspectorPackageName -or
        [string]$metadata.version -ne $InspectorPackageVersion) {
        throw "Inspector entry package name or version differs from $InspectorPackageSpec"
    }

    $binProperties = @($metadata.bin.PSObject.Properties)
    if ($binProperties.Count -ne 1 -or
        $binProperties[0].Name -ne "mcp-inspector" -or
        [string]$binProperties[0].Value -ne
            "./clients/launcher/build/index.js") {
        throw "Inspector bin mapping must be exactly ./clients/launcher/build/index.js"
    }

    $entryCandidate = Join-Path $packageRoot (
        [string]$binProperties[0].Value)
    $entryPath = (Resolve-Path $entryCandidate).Path
    $entryPath = Assert-ResolvedPathWithinRoot `
        -RootPath $packageRoot `
        -CandidatePath $entryPath
    $relativeEntry = [System.IO.Path]::GetRelativePath($packageRoot, $entryPath)
    $currentPath = $packageRoot
    foreach ($component in $relativeEntry.Split(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.StringSplitOptions]::RemoveEmptyEntries)) {
        $currentPath = Join-Path $currentPath $component
        $item = Get-Item -LiteralPath $currentPath -Force
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Inspector entry path cannot contain reparse points"
        }
    }

    return [pscustomobject]@{
        bin_name = "mcp-inspector"
        bin_mapping = "./clients/launcher/build/index.js"
        entry_path = $entryPath
        entry_sha256 = (
            Get-FileHash -LiteralPath $entryPath -Algorithm SHA256).Hash
    }
}

function Invoke-InspectorProvenanceContractTest {
    $zeroRejected = $false
    try {
        $null = Select-UniqueInspectorCacheCandidate -Candidates @()
    } catch {
        $expected =
            "Expected exactly one exact-version cached Inspector installation; found 0"
        if ($_.Exception.Message -ne $expected) {
            throw "Zero-candidate rejection was not fail-closed: $($_.Exception.Message)"
        }
        $zeroRejected = $true
    }
    if (-not $zeroRejected) {
        throw "Zero exact-version Inspector cache candidates were accepted"
    }

    $onlyCandidate = [pscustomobject]@{
        package_json = "synthetic-one"
        integrity = "sha512-one"
        package_json_sha256 = "ONE"
    }
    $selected = Select-UniqueInspectorCacheCandidate `
        -Candidates @($onlyCandidate)
    if ($selected.package_json -ne $onlyCandidate.package_json -or
        $selected.integrity -ne $onlyCandidate.integrity -or
        $selected.package_json_sha256 -ne $onlyCandidate.package_json_sha256) {
        throw "One exact-version Inspector cache candidate was not preserved"
    }

    $multipleRejected = $false
    try {
        $null = Select-UniqueInspectorCacheCandidate -Candidates @(
            $onlyCandidate,
            [pscustomobject]@{
                package_json = "synthetic-two"
                integrity = "sha512-two"
                package_json_sha256 = "TWO"
            })
    } catch {
        $expected =
            "Expected exactly one exact-version cached Inspector installation; found 2"
        if ($_.Exception.Message -ne $expected) {
            throw "Multiple-candidate rejection was not fail-closed: $($_.Exception.Message)"
        }
        $multipleRejected = $true
    }
    if (-not $multipleRejected) {
        throw "Multiple exact-version Inspector cache candidates were accepted"
    }

    $contractRoot = Join-Path $sessionRoot "provenance-contract"
    $packageRoot = Join-Path $contractRoot "package"
    $packageJsonPath = Join-Path $packageRoot "package.json"
    $entryPath = Join-Path $packageRoot "clients\launcher\build\index.js"
    $packageMetadata = [ordered]@{
        name = $InspectorPackageName
        version = $InspectorPackageVersion
        bin = [ordered]@{
            "mcp-inspector" = "./clients/launcher/build/index.js"
        }
    }
    Write-Utf8File `
        -Path $packageJsonPath `
        -Content ($packageMetadata | ConvertTo-Json -Depth 10)
    Write-Utf8File -Path $entryPath -Content "synthetic Inspector entry"
    $entry = Resolve-VerifiedInspectorEntry -PackageJsonPath $packageJsonPath
    if ($entry.bin_mapping -ne "./clients/launcher/build/index.js" -or
        $entry.entry_path -ne (Resolve-Path $entryPath).Path -or
        $entry.entry_sha256 -ne (
            Get-FileHash -LiteralPath $entryPath -Algorithm SHA256).Hash) {
        throw "Exact Inspector entry mapping was not preserved"
    }

    $wrongPackageRoot = Join-Path $contractRoot "wrong-package"
    $wrongPackageJsonPath = Join-Path $wrongPackageRoot "package.json"
    $wrongMetadata = [ordered]@{
        name = $InspectorPackageName
        version = $InspectorPackageVersion
        bin = [ordered]@{ "mcp-inspector" = "./wrong.js" }
    }
    Write-Utf8File `
        -Path $wrongPackageJsonPath `
        -Content ($wrongMetadata | ConvertTo-Json -Depth 10)
    $wrongMappingRejected = $false
    try {
        $null = Resolve-VerifiedInspectorEntry `
            -PackageJsonPath $wrongPackageJsonPath
    } catch {
        $expected =
            "Inspector bin mapping must be exactly ./clients/launcher/build/index.js"
        if ($_.Exception.Message -ne $expected) {
            throw "Wrong Inspector bin mapping had an unexpected failure: $($_.Exception.Message)"
        }
        $wrongMappingRejected = $true
    }
    if (-not $wrongMappingRejected) {
        throw "Wrong Inspector bin mapping was accepted"
    }

    $containmentRejected = $false
    try {
        $null = Assert-ResolvedPathWithinRoot `
            -RootPath $packageRoot `
            -CandidatePath (Join-Path $contractRoot "outside.js")
    } catch {
        if ($_.Exception.Message -ne "Inspector entry resolves outside its package root") {
            throw "Inspector entry containment had an unexpected failure: $($_.Exception.Message)"
        }
        $containmentRejected = $true
    }
    if (-not $containmentRejected) {
        throw "Inspector entry outside its package root was accepted"
    }

    Write-Host (
        "INSPECTOR_PROVENANCE_CONTRACT_OK zero=reject one=accept " +
        "multiple=reject entry=verified mapping=reject containment=reject")
}

function Invoke-NodeCliText {
    param(
        [string]$NodePath,
        [string[]]$ArgumentList,
        [string]$Description
    )

    $output = @(& $NodePath @ArgumentList 2>&1)
    $exitCode = $LASTEXITCODE
    $text = (($output | ForEach-Object { [string]$_ }) -join "`n").Trim()
    if ($exitCode -ne 0) {
        throw "$Description failed with exit code $exitCode`: $text"
    }
    if ([string]::IsNullOrWhiteSpace($text)) {
        throw "$Description returned no output"
    }
    return $text
}

function Get-InspectorProvenance {
    $npxCommand = (Get-Command npx.cmd -ErrorAction Stop).Source
    $nodePath = (Resolve-Path (
        (Get-Command node.exe -ErrorAction Stop).Source)).Path
    $npmRoot = Split-Path $npxCommand -Parent
    $npxCliPath = Join-Path $npmRoot "node_modules\npm\bin\npx-cli.js"
    $npmCliPath = Join-Path $npmRoot "node_modules\npm\bin\npm-cli.js"
    foreach ($cliPath in $npxCliPath, $npmCliPath) {
        if (-not (Test-Path -LiteralPath $cliPath -PathType Leaf)) {
            throw "Required local npm CLI entry point is missing: $cliPath"
        }
    }
    $npxCliPath = (Resolve-Path $npxCliPath).Path
    $npmCliPath = (Resolve-Path $npmCliPath).Path

    $nodeVersion = Invoke-NodeCliText `
        -NodePath $nodePath `
        -ArgumentList @("--version") `
        -Description "Node version probe"
    if ($nodeVersion -notmatch '^v\d+\.\d+\.\d+') {
        throw "Node returned an invalid version: $nodeVersion"
    }
    $npxVersion = Invoke-NodeCliText `
        -NodePath $nodePath `
        -ArgumentList @($npxCliPath, "--version") `
        -Description "npx version probe"
    $npmVersion = Invoke-NodeCliText `
        -NodePath $nodePath `
        -ArgumentList @($npmCliPath, "--version") `
        -Description "npm version probe"
    if ($npxVersion -notmatch '^\d+\.\d+\.\d+' -or
        $npmVersion -notmatch '^\d+\.\d+\.\d+') {
        throw "npm or npx returned an invalid version: npm=$npmVersion npx=$npxVersion"
    }
    if ($npmVersion -ne $npxVersion) {
        throw "npm and npx versions differ: npm=$npmVersion npx=$npxVersion"
    }

    $npmCache = Invoke-NodeCliText `
        -NodePath $nodePath `
        -ArgumentList @($npmCliPath, "config", "get", "cache", "--location=user") `
        -Description "npm cache path probe"
    if (-not (Test-Path -LiteralPath $npmCache -PathType Container)) {
        throw "npm cache path does not exist: $npmCache"
    }
    $npxCacheRoot = Join-Path $npmCache "_npx"
    if (-not (Test-Path -LiteralPath $npxCacheRoot -PathType Container)) {
        throw "npx cache is missing; $InspectorPackageSpec must already be cached: $npxCacheRoot"
    }

    $cachedPackages = [System.Collections.Generic.List[object]]::new()
    foreach ($hashDirectory in @(
        Get-ChildItem -LiteralPath $npxCacheRoot -Directory -ErrorAction Stop
    )) {
        $packageJsonPath = Join-Path $hashDirectory.FullName (
            "node_modules\$InspectorPackageName\package.json")
        if (-not (Test-Path -LiteralPath $packageJsonPath -PathType Leaf)) {
            continue
        }
        $packageMetadata = Get-Content -LiteralPath $packageJsonPath -Raw |
            ConvertFrom-Json -Depth 20
        if ([string]$packageMetadata.name -ne $InspectorPackageName -or
            [string]$packageMetadata.version -ne $InspectorPackageVersion) {
            continue
        }

        $lockPath = Join-Path $hashDirectory.FullName "package-lock.json"
        if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
            continue
        }
        $lock = Get-Content -LiteralPath $lockPath -Raw |
            ConvertFrom-Json -AsHashtable -Depth 100
        $lockEntry = $lock["packages"]["node_modules/$InspectorPackageName"]
        if ($null -eq $lockEntry) { continue }
        if ([string]$lockEntry["version"] -ne $InspectorPackageVersion -or
            [string]::IsNullOrWhiteSpace([string]$lockEntry["integrity"])) {
            continue
        }
        $cachedPackages.Add([pscustomobject]@{
            package_json = (Resolve-Path $packageJsonPath).Path
            integrity = [string]$lockEntry["integrity"]
            package_json_sha256 = (
                Get-FileHash -LiteralPath $packageJsonPath -Algorithm SHA256).Hash
        })
    }
    $cachedPackage = Select-UniqueInspectorCacheCandidate `
        -Candidates @($cachedPackages)
    $verifiedEntry = Resolve-VerifiedInspectorEntry `
        -PackageJsonPath $cachedPackage.package_json
    return [pscustomobject]@{
        inspector_package_spec = $InspectorPackageSpec
        inspector_package_integrity = $cachedPackage.integrity
        inspector_package_json_sha256 = $cachedPackage.package_json_sha256
        inspector_bin_mapping = $verifiedEntry.bin_mapping
        inspector_entry_path = $verifiedEntry.entry_path
        inspector_entry_sha256 = $verifiedEntry.entry_sha256
        node_version = $nodeVersion
        npm_npx_version = $npmVersion
        node_path = $nodePath
        npx_cli_path = $npxCliPath
    }
}

function Initialize-InspectorProvenance {
    $script:inspectorProvenance = Get-InspectorProvenance
    $evidence.inspector_package_spec =
        $script:inspectorProvenance.inspector_package_spec
    $evidence.inspector_package_integrity =
        $script:inspectorProvenance.inspector_package_integrity
    $evidence.inspector_package_json_sha256 =
        $script:inspectorProvenance.inspector_package_json_sha256
    $evidence.inspector_bin_mapping =
        $script:inspectorProvenance.inspector_bin_mapping
    $evidence.inspector_entry_path =
        $script:inspectorProvenance.inspector_entry_path
    $evidence.inspector_entry_sha256 =
        $script:inspectorProvenance.inspector_entry_sha256
    $evidence.node_version = $script:inspectorProvenance.node_version
    $evidence.npm_npx_version = $script:inspectorProvenance.npm_npx_version
}

function Initialize-InspectorJobNative {
    if ($null -ne ("AurumInspectorJobNative" -as [type])) { return }

    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class AurumInspectorJobNative
{
    private const uint JobObjectExtendedLimitInformation = 9;
    private const uint JobObjectBasicAccountingInformation = 1;
    private const uint JobObjectLimitKillOnJobClose = 0x00002000;

    [StructLayout(LayoutKind.Sequential)]
    private struct JobObjectBasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobObjectExtendedLimitInformationData
    {
        public JobObjectBasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobObjectBasicAccountingInformationData
    {
        public long TotalUserTime;
        public long TotalKernelTime;
        public long ThisPeriodTotalUserTime;
        public long ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount;
        public uint TotalProcesses;
        public uint ActiveProcesses;
        public uint TotalTerminatedProcesses;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObjectW(IntPtr jobAttributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetInformationJobObject(
        IntPtr job,
        uint informationClass,
        ref JobObjectExtendedLimitInformationData information,
        uint informationLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool QueryInformationJobObject(
        IntPtr job,
        uint informationClass,
        out JobObjectBasicAccountingInformationData information,
        uint informationLength,
        IntPtr returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    private static Win32Exception LastError(string operation)
    {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }

    public static IntPtr CreateKillOnCloseJob()
    {
        IntPtr job = CreateJobObjectW(IntPtr.Zero, null);
        if (job == IntPtr.Zero)
            throw LastError("CreateJobObjectW failed");

        var information = new JobObjectExtendedLimitInformationData();
        information.BasicLimitInformation.LimitFlags = JobObjectLimitKillOnJobClose;
        uint length = (uint)Marshal.SizeOf<JobObjectExtendedLimitInformationData>();
        if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation,
                ref information, length))
        {
            int error = Marshal.GetLastWin32Error();
            CloseHandle(job);
            throw new Win32Exception(error, "SetInformationJobObject failed");
        }
        return job;
    }

    public static void Assign(IntPtr job, IntPtr process)
    {
        if (!AssignProcessToJobObject(job, process))
            throw LastError("AssignProcessToJobObject failed");
    }

    public static bool Contains(IntPtr job, IntPtr process)
    {
        bool result;
        if (!IsProcessInJob(process, job, out result))
            throw LastError("IsProcessInJob failed");
        return result;
    }

    public static uint ActiveProcessCount(IntPtr job)
    {
        JobObjectBasicAccountingInformationData information;
        uint length = (uint)Marshal.SizeOf<JobObjectBasicAccountingInformationData>();
        if (!QueryInformationJobObject(job, JobObjectBasicAccountingInformation,
                out information, length, IntPtr.Zero))
            throw LastError("QueryInformationJobObject failed");
        return information.ActiveProcesses;
    }

    public static void Terminate(IntPtr job, uint exitCode)
    {
        if (!TerminateJobObject(job, exitCode))
            throw LastError("TerminateJobObject failed");
    }

    public static void Close(IntPtr job)
    {
        if (job != IntPtr.Zero && !CloseHandle(job))
            throw LastError("CloseHandle failed");
    }
}
'@
}

function New-InspectorCleanupException {
    param([string]$Message)
    $exception = [System.InvalidOperationException]::new($Message)
    $exception.Data["AurumInspectorCleanupFailure"] = $true
    return $exception
}

function Test-InspectorCleanupException {
    param([System.Exception]$Exception)
    return $null -ne $Exception -and
        $Exception.Data.Contains("AurumInspectorCleanupFailure") -and
        [bool]$Exception.Data["AurumInspectorCleanupFailure"]
}

function Wait-InspectorJobEmpty {
    param(
        [IntPtr]$JobHandle,
        [int]$TimeoutMilliseconds = 5000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        $active = [AurumInspectorJobNative]::ActiveProcessCount($JobHandle)
        if ($active -eq 0) { return }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Inspector Job Object still has $active active process(es) after $TimeoutMilliseconds ms"
}

function Complete-InspectorJob {
    param(
        [IntPtr]$JobHandle,
        [System.Diagnostics.Process]$RootProcess
    )

    $failures = [System.Collections.Generic.List[string]]::new()
    try {
        [AurumInspectorJobNative]::Terminate($JobHandle, 1)
    } catch {
        $failures.Add("Job termination failed: $($_.Exception.Message)")
    }
    try {
        Wait-InspectorJobEmpty -JobHandle $JobHandle
    } catch {
        $failures.Add("Job exit verification failed: $($_.Exception.Message)")
    }
    if ($null -ne $RootProcess) {
        try {
            if (-not $RootProcess.WaitForExit(5000)) {
                $failures.Add("Inspector wrapper PID $($RootProcess.Id) did not exit")
            }
        } catch {
            $failures.Add("Inspector wrapper exit check failed: $($_.Exception.Message)")
        }
    }
    try {
        [AurumInspectorJobNative]::Close($JobHandle)
    } catch {
        $failures.Add("Job handle close failed: $($_.Exception.Message)")
    }
    if ($failures.Count -gt 0) {
        throw (New-InspectorCleanupException -Message ($failures -join " | "))
    }
}

function Assert-OwnedRootIdentity {
    param(
        [System.Diagnostics.Process]$Process,
        [datetime]$StartTime,
        [string]$ExpectedExecutable,
        [string[]]$RequiredCommandLineFragments
    )

    $deadline = [DateTime]::UtcNow.AddSeconds(2)
    $lastReason = "identity not inspected"
    do {
        $current = Get-Process -Id $Process.Id -ErrorAction SilentlyContinue
        if ($null -eq $current) {
            $lastReason = "process exited before ownership validation"
            break
        }
        if ($current.StartTime -ne $StartTime) {
            $lastReason = "process start time changed"
            break
        }
        if ($current.Path -ne $ExpectedExecutable) {
            $lastReason =
                "process executable path was '$($current.Path)', expected '$ExpectedExecutable'"
            Start-Sleep -Milliseconds 25
            continue
        }
        $rootCim = Get-CimInstance Win32_Process -Filter "ProcessId = $($Process.Id)"
        if ($null -eq $rootCim) {
            $lastReason = "CIM identity was unavailable"
        } else {
            $commandLine = [string]$rootCim.CommandLine
            $missing = @(
                $RequiredCommandLineFragments |
                    Where-Object { $commandLine -notlike "*$_*" }
            )
            if ($missing.Count -eq 0) { return }
            $lastReason = "command line omitted: $($missing -join ', ')"
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $deadline)

    throw "Inspector wrapper PID $($Process.Id) ownership validation failed: $lastReason"
}

function Stop-UnassignedBarrierRoot {
    param(
        [System.Diagnostics.Process]$Process,
        [datetime]$StartTime,
        [string]$ExpectedExecutable,
        [string[]]$RequiredCommandLineFragments
    )
    if ($Process.HasExited) { return }
    Assert-OwnedRootIdentity `
        -Process $Process `
        -StartTime $StartTime `
        -ExpectedExecutable $ExpectedExecutable `
        -RequiredCommandLineFragments $RequiredCommandLineFragments
    $Process.Kill($true)
    if (-not $Process.WaitForExit(5000)) {
        throw "Unassigned Inspector wrapper PID $($Process.Id) did not exit"
    }
}

function Invoke-InspectorRequest {
    param(
        [string]$Method,
        [string]$ToolName,
        [hashtable]$Arguments
    )

    if ($null -eq $script:inspectorProvenance) {
        throw "Inspector provenance must be validated before an Inspector request"
    }
    $node = [string]$script:inspectorProvenance.node_path
    $inspectorEntry = [string]$script:inspectorProvenance.inspector_entry_path
    $pwsh = (Get-Command pwsh -ErrorAction Stop).Source
    $resolvedPwsh = (Resolve-Path $pwsh).Path
    $requestId = [Guid]::NewGuid().ToString("N")
    $requestRoot = Join-Path $sessionRoot "inspector\$requestId"
    $wrapperPath = Join-Path $requestRoot "inspector-wrapper.ps1"
    $requestPath = Join-Path $requestRoot "request.json"
    $readyPath = Join-Path $requestRoot "job-assigned.ready"

    $inspectorArgs = [System.Collections.Generic.List[string]]::new()
    foreach ($value in @(
        $inspectorEntry,
        '--cli',
        '--config', $mcpConfigPath,
        '--server', 'godot',
        '--method', $Method,
        '--format', 'json'
    )) {
        $inspectorArgs.Add($value)
    }
    if (-not [string]::IsNullOrWhiteSpace($ToolName)) {
        $inspectorArgs.Add('--tool-name')
        $inspectorArgs.Add($ToolName)
        $inspectorArgs.Add('--tool-args-json')
        $inspectorArgs.Add(($Arguments | ConvertTo-Json -Compress -Depth 20))
    }

    $wrapperText = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ReadyPath,
    [Parameter(Mandatory)]
    [string]$RequestPath
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
while (-not [System.IO.File]::Exists($ReadyPath)) {
    Start-Sleep -Milliseconds 10
}
$request = Get-Content -LiteralPath $RequestPath -Raw | ConvertFrom-Json -Depth 20
$arguments = @($request.arguments | ForEach-Object { [string]$_ })
& ([string]$request.node_path) @arguments
exit $LASTEXITCODE
'@
    $request = [ordered]@{
        node_path = $node
        arguments = $inspectorArgs
    }
    Write-Utf8File -Path $wrapperPath -Content $wrapperText
    Write-Utf8File -Path $requestPath -Content ($request | ConvertTo-Json -Depth 30)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedPwsh
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.Environment["npm_config_offline"] = "true"
    $startInfo.Environment["npm_config_yes"] = "true"
    foreach ($value in @(
        '-NoProfile',
        '-NonInteractive',
        '-File', $wrapperPath,
        '-ReadyPath', $readyPath,
        '-RequestPath', $requestPath
    )) {
        $startInfo.ArgumentList.Add($value)
    }

    $jobHandle = [IntPtr]::Zero
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $processStarted = $false
    $jobAssigned = $false
    $processStartTime = $null
    $stdoutTask = $null
    $stderrTask = $null
    $responseLine = $null
    $requestFailure = $null
    $cleanupFailure = $null
    $stderr = ""
    $requiredRootFragments = @($wrapperPath, $readyPath, $requestPath)

    try {
        $jobHandle = [AurumInspectorJobNative]::CreateKillOnCloseJob()
        if (-not $process.Start()) {
            throw "Could not start MCP Inspector wrapper"
        }
        $processStarted = $true
        $processStartTime = $process.StartTime
        $evidence.inspector_requests_started =
            [int]$evidence.inspector_requests_started + 1

        [AurumInspectorJobNative]::Assign($jobHandle, $process.Handle)
        $jobAssigned = $true
        if (-not [AurumInspectorJobNative]::Contains($jobHandle, $process.Handle)) {
            throw "Inspector wrapper was not assigned to its Job Object"
        }
        Assert-OwnedRootIdentity `
            -Process $process `
            -StartTime $processStartTime `
            -ExpectedExecutable $resolvedPwsh `
            -RequiredCommandLineFragments $requiredRootFragments

        Write-Utf8File -Path $readyPath -Content $requestId
        $stdoutTask = $process.StandardOutput.ReadLineAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $stdoutTask.Wait(15000)) {
            $requestFailure = "MCP Inspector produced no JSON response within 15 seconds"
        } else {
            $responseLine = $stdoutTask.GetAwaiter().GetResult()
            if ([string]::IsNullOrWhiteSpace($responseLine)) {
                $requestFailure = "MCP Inspector closed stdout without a JSON response"
            }
        }
    } catch {
        $requestFailure = $_.Exception.Message
    } finally {
        if ($processStarted -and -not $jobAssigned) {
            try {
                Stop-UnassignedBarrierRoot `
                    -Process $process `
                    -StartTime $processStartTime `
                    -ExpectedExecutable $resolvedPwsh `
                    -RequiredCommandLineFragments $requiredRootFragments
            } catch {
                $cleanupFailure = New-InspectorCleanupException `
                    -Message "Unassigned Inspector wrapper cleanup failed: $($_.Exception.Message)"
            }
        }
        if ($jobHandle -ne [IntPtr]::Zero) {
            try {
                Complete-InspectorJob `
                    -JobHandle $jobHandle `
                    -RootProcess $(if ($processStarted -and $jobAssigned) { $process } else { $null })
                if ($jobAssigned) {
                    $evidence.inspector_cleanups_verified =
                        [int]$evidence.inspector_cleanups_verified + 1
                }
            } catch {
                if ($null -eq $cleanupFailure) {
                    if (Test-InspectorCleanupException -Exception $_.Exception) {
                        $cleanupFailure = $_.Exception
                    } else {
                        $cleanupFailure = New-InspectorCleanupException `
                            -Message "Inspector Job Object cleanup failed: $($_.Exception.Message)"
                    }
                }
            } finally {
                $jobHandle = [IntPtr]::Zero
            }
        }
        if ($null -ne $stderrTask) {
            try {
                if ($stderrTask.Wait(5000)) {
                    $stderr = $stderrTask.GetAwaiter().GetResult()
                } else {
                    $stderr = "Inspector stderr did not close after verified Job Object cleanup"
                    if ($null -eq $cleanupFailure) {
                        $cleanupFailure = New-InspectorCleanupException -Message $stderr
                    }
                }
            } catch {
                $stderr = $_.Exception.Message
                if ($null -eq $cleanupFailure) {
                    $cleanupFailure = New-InspectorCleanupException `
                        -Message "Inspector stderr cleanup failed: $stderr"
                }
            }
        }
        $process.Dispose()
    }

    if ($null -ne $cleanupFailure) {
        throw $cleanupFailure
    }
    if ($null -ne $requestFailure) {
        throw "$requestFailure`n$stderr"
    }
    try {
        $response = $responseLine | ConvertFrom-Json -Depth 50
    } catch {
        throw "MCP Inspector returned invalid JSON: $responseLine`n$stderr"
    }
    if ($null -ne $response.PSObject.Properties['error']) {
        throw "MCP Inspector request failed: $($response.error | ConvertTo-Json -Compress -Depth 20)"
    }
    return $response
}

function Invoke-InspectorMethod {
    param([string]$Method)
    return Invoke-InspectorRequest -Method $Method -ToolName "" -Arguments @{}
}

function Invoke-InspectorTool {
    param(
        [string]$ToolName,
        [hashtable]$Arguments
    )
    $result = Invoke-InspectorRequest `
        -Method "tools/call" `
        -ToolName $ToolName `
        -Arguments $Arguments
    if ($null -ne $result.result.PSObject.Properties['isError'] -and $result.result.isError) {
        $errorJson = $result | ConvertTo-Json -Compress -Depth 50
        throw "MCP tool $ToolName returned an error result: $errorJson"
    }
    $textBlock = $result.result.content |
        Where-Object { $_.type -eq "text" } |
        Select-Object -First 1
    if ($null -eq $textBlock) {
        throw "MCP tool $ToolName returned no text payload"
    }
    $payload = $textBlock.text | ConvertFrom-Json -Depth 50
    if (-not $payload.success) {
        throw "MCP tool $ToolName reported failure: $($textBlock.text)"
    }
    return $payload
}

function Wait-ForMcp {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastError = "not attempted"
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $null = Invoke-InspectorMethod -Method "tools/list"
            return
        } catch {
            if (Test-InspectorCleanupException -Exception $_.Exception) { throw }
            $lastError = $_.Exception.Message
            Start-Sleep -Seconds 1
        }
    }
    throw "MCP did not become ready: $lastError"
}

function Wait-ForSceneOpen {
    param([string]$FilePath)
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastError = "not attempted"
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($editor.HasExited) {
            throw "Godot editor exited with code $($editor.ExitCode)"
        }
        try {
            return Invoke-InspectorTool `
                -ToolName "scene_open" `
                -Arguments @{ file_path = $FilePath }
        } catch {
            if (Test-InspectorCleanupException -Exception $_.Exception) { throw }
            $lastError = $_.Exception.Message
            Start-Sleep -Seconds 1
        }
    }
    throw "Scene did not open through MCP: $lastError"
}

function Get-LiveFingerprint {
    $payload = Invoke-InspectorTool `
        -ToolName "node_call_method" `
        -Arguments @{
            node_path = "Aurum"
            method_name = "runtime_fingerprint"
            args = @()
        }
    if ($null -eq $payload.PSObject.Properties['result'] -or $null -eq $payload.result) {
        $payloadJson = $payload | ConvertTo-Json -Compress -Depth 50
        throw "node_call_method returned no runtime fingerprint: $payloadJson"
    }
    return [string]$payload.result
}

function Wait-ForFingerprint {
    param([string]$Expected)
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastObserved = ""
    $lastError = ""
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($editor.HasExited) {
            throw "Godot editor exited with code $($editor.ExitCode)"
        }
        try {
            $lastObserved = Get-LiveFingerprint
            if ($lastObserved -eq $Expected) {
                return $lastObserved
            }
        } catch {
            if (Test-InspectorCleanupException -Exception $_.Exception) { throw }
            $lastError = $_.Exception.Message
        }
        Start-Sleep -Seconds 1
    }
    throw "Fingerprint did not become '$Expected'; last='$lastObserved' error='$lastError'"
}

function Invoke-DebugInstall {
    param([string]$Fingerprint)
    $env:AURUM_RUNTIME_FINGERPRINT = $Fingerprint
    & (Join-Path $workspace "scripts\dev.ps1") `
        -Once `
        -GodotProject $projectRoot `
        -GodotBinary $GodotBinary
    if ($LASTEXITCODE -ne 0) {
        throw "Debug install failed for fingerprint $Fingerprint"
    }
    $installed = Join-Path $projectRoot "addons\aurum\bin\aurum_godot.debug.dll"
    $marker = Join-Path $projectRoot ".godot\aurum\aurum_godot.debug.reload"
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) {
        throw "Debug install did not publish its reload marker: $marker"
    }
    $dllHash = (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
    $markerHash = (Get-Content -LiteralPath $marker -Raw).Trim()
    if ($markerHash -cne $dllHash) {
        throw "Reload marker hash '$markerHash' did not match installed DLL hash '$dllHash'"
    }
    return [pscustomobject]@{
        dll_sha256 = $dllHash
        marker_sha256 = $markerHash
    }
}

function Assert-OwnedEditorIdentity {
    $current = Get-Process -Id $editor.Id -ErrorAction SilentlyContinue
    if ($null -eq $current) { return $null }
    if ($current.StartTime -ne $editorStartTime) {
        throw "Refusing to control PID $($editor.Id): process start time changed"
    }
    if ($current.Path -ne $evidence.godot_binary) {
        throw "Refusing to control PID $($editor.Id): executable path changed"
    }
    $editorCim = Get-CimInstance Win32_Process -Filter "ProcessId = $($editor.Id)"
    if ($null -eq $editorCim -or
        [string]$editorCim.CommandLine -notlike "*$projectRoot*") {
        throw "Refusing to control PID $($editor.Id): project path ownership check failed"
    }
    return $current
}

function Stop-OwnedEditor {
    if ($KeepEditorOpen) { return "kept_open_by_request" }
    if ($null -eq $editor) { return "not_started" }
    $current = Assert-OwnedEditorIdentity
    if ($null -eq $current) { return "already_exited" }
    $null = $current.CloseMainWindow()
    if (-not $current.WaitForExit(10000)) {
        $current = Assert-OwnedEditorIdentity
        if ($null -eq $current) { return "exited_after_graceful_request" }
        $editor.Kill($true)
        if (-not $editor.WaitForExit(5000)) {
            throw "Owned editor PID $($editor.Id) did not exit after force termination"
        }
    }
    if (-not $editor.HasExited) {
        throw "Owned editor PID $($editor.Id) exit could not be verified"
    }
    return "verified_stopped"
}

function Invoke-InspectorOwnershipHelperTest {
    Initialize-InspectorJobNative
    Initialize-InspectorProvenance
    if ($script:inspectorProvenance.inspector_package_spec -ne
            $InspectorPackageSpec -or
        $script:inspectorProvenance.node_version -notmatch '^v\d+\.\d+\.\d+' -or
        $script:inspectorProvenance.npm_npx_version -notmatch '^\d+\.\d+\.\d+') {
        throw "Inspector provenance helper assertions failed"
    }
    $helperWorkspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
    $helperId = [Guid]::NewGuid().ToString("N")
    $helperRoot = Join-Path $helperWorkspace "target\aurum-hot-reload-smoke\ownership-helper-$helperId"
    $wrapperPath = Join-Path $helperRoot "root-wrapper.ps1"
    $readyPath = Join-Path $helperRoot "job-assigned.ready"
    $childInfoPath = Join-Path $helperRoot "child.json"
    $pwsh = (Get-Command pwsh -ErrorAction Stop).Source
    $resolvedPwsh = (Resolve-Path $pwsh).Path

    $wrapperText = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$ReadyPath,
    [Parameter(Mandatory)]
    [string]$ChildInfoPath,
    [Parameter(Mandatory)]
    [string]$ChildExecutable
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
while (-not [System.IO.File]::Exists($ReadyPath)) {
    Start-Sleep -Milliseconds 10
}
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $ChildExecutable
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.ArgumentList.Add("-NoProfile")
$startInfo.ArgumentList.Add("-NonInteractive")
$startInfo.ArgumentList.Add("-Command")
$startInfo.ArgumentList.Add("Start-Sleep -Seconds 30")
$child = [System.Diagnostics.Process]::new()
$child.StartInfo = $startInfo
if (-not $child.Start()) { throw "Could not start owned helper child" }
$record = [ordered]@{
    pid = $child.Id
    start_time_utc = $child.StartTime.ToUniversalTime().ToString("O")
    executable = $child.Path
}
$encoding = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText(
    $ChildInfoPath,
    ($record | ConvertTo-Json -Compress),
    $encoding)
$child.Dispose()
'@
    Write-Utf8File -Path $wrapperPath -Content $wrapperText

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedPwsh
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    foreach ($value in @(
        '-NoProfile',
        '-NonInteractive',
        '-File', $wrapperPath,
        '-ReadyPath', $readyPath,
        '-ChildInfoPath', $childInfoPath,
        '-ChildExecutable', $resolvedPwsh
    )) {
        $startInfo.ArgumentList.Add($value)
    }

    $jobHandle = [IntPtr]::Zero
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $processStarted = $false
    $jobAssigned = $false
    $processStartTime = $null
    $requiredFragments = @($wrapperPath, $readyPath, $childInfoPath)
    $cleanupCompleted = $false
    try {
        $jobHandle = [AurumInspectorJobNative]::CreateKillOnCloseJob()
        if (-not $process.Start()) { throw "Could not start ownership helper wrapper" }
        $processStarted = $true
        $processStartTime = $process.StartTime
        [AurumInspectorJobNative]::Assign($jobHandle, $process.Handle)
        $jobAssigned = $true
        if (-not [AurumInspectorJobNative]::Contains($jobHandle, $process.Handle)) {
            throw "Ownership helper wrapper was not assigned to its Job Object"
        }
        Assert-OwnedRootIdentity `
            -Process $process `
            -StartTime $processStartTime `
            -ExpectedExecutable $resolvedPwsh `
            -RequiredCommandLineFragments $requiredFragments
        Write-Utf8File -Path $readyPath -Content $helperId

        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $childInfoPath) -and
            [DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 25
        }
        if (-not (Test-Path -LiteralPath $childInfoPath)) {
            throw "Ownership helper child identity was not recorded"
        }
        if (-not $process.WaitForExit(5000)) {
            throw "Ownership helper root did not exit after starting its child"
        }
        $activeAfterRootExit =
            [AurumInspectorJobNative]::ActiveProcessCount($jobHandle)
        if ($activeAfterRootExit -lt 1) {
            throw "Ownership helper did not retain its child after the root exited"
        }

        $childInfo = Get-Content -LiteralPath $childInfoPath -Raw |
            ConvertFrom-Json -Depth 10
        Complete-InspectorJob -JobHandle $jobHandle -RootProcess $process
        $jobHandle = [IntPtr]::Zero
        $cleanupCompleted = $true

        $currentChild = Get-Process -Id ([int]$childInfo.pid) -ErrorAction SilentlyContinue
        if ($null -ne $currentChild -and
            $currentChild.StartTime.ToUniversalTime().ToString("O") -eq
                [string]$childInfo.start_time_utc) {
            throw "Owned helper child PID $($childInfo.pid) remained active"
        }
        Write-Host (
            "INSPECTOR_JOB_OWNERSHIP_HELPER_OK " +
            "root_pid=$($process.Id) child_pid=$($childInfo.pid) " +
            "active_after_root_exit=$activeAfterRootExit " +
            "inspector=$InspectorPackageSpec " +
            "node=$($script:inspectorProvenance.node_version) " +
            "npm_npx=$($script:inspectorProvenance.npm_npx_version)")
    } finally {
        if (-not $cleanupCompleted) {
            if ($processStarted -and -not $jobAssigned) {
                Stop-UnassignedBarrierRoot `
                    -Process $process `
                    -StartTime $processStartTime `
                    -ExpectedExecutable $resolvedPwsh `
                    -RequiredCommandLineFragments $requiredFragments
            }
            if ($jobHandle -ne [IntPtr]::Zero) {
                try {
                    Complete-InspectorJob `
                        -JobHandle $jobHandle `
                        -RootProcess $(if ($processStarted -and $jobAssigned) { $process } else { $null })
                } finally {
                    $jobHandle = [IntPtr]::Zero
                }
            }
        }
        $process.Dispose()
    }
}

if ($ProvenanceContractTest) {
    Invoke-InspectorProvenanceContractTest
    return
}

if ($OwnershipHelperTest) {
    Invoke-InspectorOwnershipHelperTest
    return
}

$primaryError = $null
$cleanupFailures = [System.Collections.Generic.List[string]]::new()
$successMessage = $null
try {
    $workspace = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
    $sessionRoot = Join-Path $workspace "target\aurum-hot-reload-smoke\$sessionId"
    $projectRoot = Join-Path $sessionRoot "project"
    $addonsRoot = Join-Path $projectRoot "addons"
    $evidencePath = Join-Path $sessionRoot "evidence.json"
    $mcpConfigPath = Join-Path $sessionRoot "mcp.json"
    $evidence.project_path = $projectRoot
    New-Item -ItemType Directory -Path $sessionRoot -Force | Out-Null

    $originalFingerprint = [Environment]::GetEnvironmentVariable(
        "AURUM_RUNTIME_FINGERPRINT", "Process")
    $originalFingerprintCaptured = $true
    $evidence.godot_binary = (Resolve-Path $GodotBinary).Path
    Initialize-InspectorJobNative
    Initialize-InspectorProvenance

    $toolkitSource = Join-Path $workspace "godot\addons\godot_mcp_toolkit"
    if (-not (Test-Path -LiteralPath $toolkitSource)) {
        throw "Godot MCP Toolkit is required for this smoke test: $toolkitSource"
    }
    New-Item -ItemType Directory -Path $addonsRoot -Force | Out-Null
    Copy-Item -LiteralPath $toolkitSource -Destination $addonsRoot -Recurse -Force

    $projectText = @'
config_version=5

[application]
config/name="Aurum Hot Reload Smoke"
run/main_scene="res://probe.tscn"
config/features=PackedStringArray("4.7")

[autoload]
Aurum="*res://addons/aurum/scripts/aurum_runtime.gd"

[editor_plugins]
enabled=PackedStringArray("res://addons/aurum/plugin.cfg", "res://addons/godot_mcp_toolkit/plugin.cfg")

[rendering]
renderer/rendering_method="gl_compatibility"
'@
    Write-Utf8File `
        -Path (Join-Path $projectRoot "project.godot") `
        -Content $projectText

    $sceneText = @'
[gd_scene format=3]

[node name="Probe" type="Node"]

[node name="Aurum" type="AurumNode" parent="."]
'@
    Write-Utf8File `
        -Path (Join-Path $projectRoot "probe.tscn") `
        -Content $sceneText

    $mcpConfig = @{
        mcpServers = @{
            godot = @{
                command = "cmd"
                args = @("/c", "godot-mcp-server")
                env = @{
                    GODOT_MCP_CONFIG_VERSION = "1"
                    GODOT_MCP_PROJECT_PATH = $projectRoot
                }
            }
        }
    }
    Write-Utf8File `
        -Path $mcpConfigPath `
        -Content ($mcpConfig | ConvertTo-Json -Depth 10)

    $initialFingerprint = "phase0-$sessionId-0"
    $initialAttempt = [ordered]@{
        iteration = 0
        requested = $initialFingerprint
        observed = $null
        dll_sha256 = $null
        marker_sha256 = $null
        editor_pid = $null
    }
    $attempts.Add($initialAttempt)
    $initialInstall = Invoke-DebugInstall -Fingerprint $initialFingerprint
    $initialAttempt["dll_sha256"] = $initialInstall.dll_sha256
    $initialAttempt["marker_sha256"] = $initialInstall.marker_sha256

    $editor = Start-Process `
        -FilePath $GodotBinary `
        -ArgumentList @("--editor", "--path", "`"$projectRoot`"") `
        -PassThru
    $editorStartTime = $editor.StartTime
    $evidence.editor_pid = $editor.Id
    $evidence.editor_start_time_utc =
        $editorStartTime.ToUniversalTime().ToString("O")
    $initialAttempt["editor_pid"] = $editor.Id

    Wait-ForMcp
    $null = Wait-ForSceneOpen -FilePath "res://probe.tscn"
    $observed = Wait-ForFingerprint -Expected $initialFingerprint
    $initialAttempt["observed"] = $observed
    $observations.Add([ordered]@{
        iteration = 0
        requested = $initialFingerprint
        observed = $observed
        dll_sha256 = $initialInstall.dll_sha256
        marker_sha256 = $initialInstall.marker_sha256
        editor_pid = $editor.Id
    })

    for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
        $fingerprint = "phase0-$sessionId-$iteration"
        $attempt = [ordered]@{
            iteration = $iteration
            requested = $fingerprint
            observed = $null
            dll_sha256 = $null
            marker_sha256 = $null
            editor_pid = $editor.Id
        }
        $attempts.Add($attempt)
        $install = Invoke-DebugInstall -Fingerprint $fingerprint
        $attempt["dll_sha256"] = $install.dll_sha256
        $attempt["marker_sha256"] = $install.marker_sha256
        $observed = Wait-ForFingerprint -Expected $fingerprint
        $attempt["observed"] = $observed
        $observations.Add([ordered]@{
            iteration = $iteration
            requested = $fingerprint
            observed = $observed
            dll_sha256 = $install.dll_sha256
            marker_sha256 = $install.marker_sha256
            editor_pid = $editor.Id
        })
    }

    $currentEditor = Assert-OwnedEditorIdentity
    if ($null -eq $currentEditor) {
        throw "Owned editor exited before final identity verification"
    }
    $observedEditorPids = @(
        $observations |
            ForEach-Object { [int]$_['editor_pid'] } |
            Sort-Object -Unique
    )
    if ($observedEditorPids.Count -ne 1) {
        throw "More than one editor PID appeared in observations"
    }
    if ([int]$evidence.inspector_requests_started -ne
        [int]$evidence.inspector_cleanups_verified) {
        throw "Not every Inspector request has verified cleanup"
    }

    $evidence.passed = $true
    $successMessage =
        "PHASE0_HOT_RELOAD_OK pid=$($editor.Id) reloads=$Iterations evidence=$evidencePath"
} catch {
    $primaryError = $_.Exception
    $evidence.error = $primaryError.Message
    $evidence.passed = $false
} finally {
    if ($originalFingerprintCaptured) {
        try {
            if ($null -eq $originalFingerprint) {
                if (Test-Path Env:AURUM_RUNTIME_FINGERPRINT) {
                    Remove-Item Env:AURUM_RUNTIME_FINGERPRINT -ErrorAction Stop
                }
            } else {
                $env:AURUM_RUNTIME_FINGERPRINT = $originalFingerprint
            }
        } catch {
            $cleanupFailures.Add(
                "Fingerprint environment restoration failed: $($_.Exception.Message)")
        }
    }
    try {
        $evidence.editor_cleanup = Stop-OwnedEditor
    } catch {
        $cleanupFailures.Add("Editor cleanup failed: $($_.Exception.Message)")
    }
    if ($cleanupFailures.Count -gt 0) {
        $evidence.passed = $false
        $cleanupText = $cleanupFailures -join " | "
        if ([string]::IsNullOrWhiteSpace([string]$evidence.error)) {
            $evidence.error = $cleanupText
        } else {
            $evidence.error = "$($evidence.error) | $cleanupText"
        }
    }
    try {
        Write-Utf8File `
            -Path $evidencePath `
            -Content ($evidence | ConvertTo-Json -Depth 30)
    } catch {
        $cleanupFailures.Add("Evidence write failed: $($_.Exception.Message)")
    }
    if ($null -ne $editor) {
        $editor.Dispose()
    }
}

if ($cleanupFailures.Count -gt 0) {
    throw ($cleanupFailures -join " | ")
}
if ($null -ne $primaryError) {
    throw $primaryError
}
Write-Host $successMessage
