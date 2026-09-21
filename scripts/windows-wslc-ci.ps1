<#
.SYNOPSIS
Owns the repository's use of the shared per-user WSLC session.

.DESCRIPTION
Serializes all worktrees with a named Windows mutex, binds execution to the
exact image ID produced under that ownership, recovers only a uniquely leased
container after an abandoned owner, and emits a source-bound Git-hook receipt.
This script never shuts down WSL, kills wslcsession, or stops WSLService.
#>
[CmdletBinding()]
param(
    [ValidateSet("full", "fast", "test-only", "doctor")]
    [string]$Mode = "full",
    [string]$TestFilter,
    [ValidateRange(1, 86400)]
    [int]$LockTimeoutSeconds = 1800,
    [switch]$FingerprintOnly,
    [switch]$VerifyReceipt,
    [ValidateSet("structural", "full")]
    [string]$RequiredCoverage = "structural"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Invoke-Git {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $output = & git -C $repositoryRoot @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
    return $output
}

function Get-SourceFingerprint {
    $paths = @(Invoke-Git -Arguments @("ls-files", "-co", "--exclude-standard")) |
        Where-Object { $_ -ne "" } |
        Sort-Object -CaseSensitive

    $blobIds = @($paths | & git -C $repositoryRoot hash-object --no-filters --stdin-paths)
    if ($LASTEXITCODE -ne 0) {
        throw "git hash-object --no-filters --stdin-paths failed with exit code $LASTEXITCODE"
    }
    if ($blobIds.Count -ne $paths.Count) {
        throw "git returned $($blobIds.Count) blob IDs for $($paths.Count) repository inputs."
    }

    $records = for ($index = 0; $index -lt $paths.Count; $index++) {
        "$($paths[$index])`0$($blobIds[$index])`n"
    }

    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($records -join ""))
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha256.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
}

function Invoke-Wslc {
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [switch]$IgnoreFailure
    )

    & wslc @Arguments
    $exitCode = $LASTEXITCODE
    if (-not $IgnoreFailure -and $exitCode -ne 0) {
        throw "wslc $($Arguments -join ' ') failed with exit code $exitCode"
    }
}

if ($Mode -eq "test-only" -and [string]::IsNullOrWhiteSpace($TestFilter)) {
    throw "-Mode test-only requires -TestFilter."
}
if ($Mode -ne "test-only" -and -not [string]::IsNullOrWhiteSpace($TestFilter)) {
    throw "-TestFilter is valid only with -Mode test-only."
}

$sourceFingerprint = Get-SourceFingerprint
if ($FingerprintOnly) {
    Write-Output $sourceFingerprint
    exit 0
}

$gitDirectory = (Invoke-Git -Arguments @("rev-parse", "--absolute-git-dir") | Select-Object -First 1)
$receiptPath = Join-Path $gitDirectory "laminaria-wslc-verification.json"

if ($VerifyReceipt) {
    if (-not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        throw "No WSLC verification receipt exists. Run scripts/windows-wslc-ci.ps1 first."
    }
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    if ($receipt.schema -ne "laminaria-wslc-verification/v1") {
        throw "The WSLC verification receipt has an unsupported schema."
    }
    if ($receipt.source_fingerprint -ne $sourceFingerprint) {
        throw "The WSLC verification receipt is stale: repository-owned inputs changed after verification."
    }
    if ($RequiredCoverage -eq "full" -and $receipt.mode -ne "full") {
        throw "A full WSLC verification receipt is required; the current receipt records mode '$($receipt.mode)'."
    }
    Write-Host "WSLC verification receipt accepted ($($receipt.mode), image $($receipt.image_id))."
    exit 0
}

if (-not (Get-Command wslc -ErrorAction SilentlyContinue)) {
    throw "wslc is not available on PATH."
}

$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$sid = $identity.User.Value
$mutexName = "Global\Asopitech.LaminariaBootstrap.Wslc.$sid"
$mutex = [System.Threading.Mutex]::new($false, $mutexName)
$lockTaken = $false
$containerCreated = $false
$containerName = "laminaria-ci-$([Guid]::NewGuid().ToString('N'))"
$iidFile = Join-Path ([System.IO.Path]::GetTempPath()) "laminaria-$([Guid]::NewGuid().ToString('N')).iid"
$cidFile = Join-Path ([System.IO.Path]::GetTempPath()) "laminaria-$([Guid]::NewGuid().ToString('N')).cid"
$leasePath = Join-Path ([System.IO.Path]::GetTempPath()) "laminaria-wslc-owner-$sid.json"

try {
    Write-Host "Waiting for the per-user WSLC owner lock: $mutexName"
    try {
        $lockTaken = $mutex.WaitOne([TimeSpan]::FromSeconds($LockTimeoutSeconds))
    }
    catch [System.Threading.AbandonedMutexException] {
        $lockTaken = $true
        Write-Warning "Recovered an abandoned WSLC owner lock from a terminated process."
    }
    if (-not $lockTaken) {
        throw "Timed out waiting for the WSLC owner lock after $LockTimeoutSeconds seconds. Another repository process owns the shared WSLC session."
    }

    if (Test-Path -LiteralPath $leasePath -PathType Leaf) {
        $abandonedLease = Get-Content -LiteralPath $leasePath -Raw | ConvertFrom-Json
        $abandonedContainer = [string]$abandonedLease.container_name
        if ($abandonedContainer -notmatch '^laminaria-ci-[0-9a-f]{32}$') {
            throw "Refusing an invalid abandoned WSLC owner lease at $leasePath."
        }
        Write-Warning "Recovering the exact container from an abandoned owner lease: $abandonedContainer"
        Invoke-Wslc -Arguments @("container", "rm", "--force", $abandonedContainer) -IgnoreFailure | Out-Null
        Remove-Item -LiteralPath $leasePath -Force
    }

    [ordered]@{
        schema = "laminaria-wslc-owner/v1"
        container_name = $containerName
        owner_pid = $PID
        source_fingerprint = $sourceFingerprint
        acquired_at_utc = [DateTimeOffset]::UtcNow.ToString("o")
    } | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding utf8

    $lockedFingerprint = Get-SourceFingerprint
    if ($lockedFingerprint -ne $sourceFingerprint) {
        throw "Repository-owned inputs changed while waiting for the WSLC owner lock. Re-run verification."
    }

    Remove-Item -LiteralPath $iidFile, $cidFile -Force -ErrorAction SilentlyContinue
    $buildArguments = @(
        "build", "--progress", "plain",
        "--iidfile", $iidFile,
        "--label", "org.asopitech.laminaria.source=$sourceFingerprint",
        "-f", "docker/bootstrap.Dockerfile",
        "-t", "laminaria-bootstrap",
        "."
    )
    Push-Location $repositoryRoot
    try {
        Invoke-Wslc -Arguments $buildArguments
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path -LiteralPath $iidFile -PathType Leaf)) {
        throw "wslc build succeeded without writing the requested image ID file."
    }
    $imageId = (Get-Content -LiteralPath $iidFile -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($imageId)) {
        throw "wslc build wrote an empty image ID."
    }

    $runArguments = @(
        "run", "--rm", "--pull", "never",
        "--name", $containerName,
        "--cidfile", $cidFile,
        "--label", "org.asopitech.laminaria.owner=$containerName"
    )
    if ($Mode -eq "doctor") {
        $runArguments += @($imageId, "doctor")
    }
    else {
        $runArguments += @("--entrypoint", "bash", $imageId, "scripts/local-ci.sh")
    }
    if ($Mode -eq "fast") {
        $runArguments += "--fast"
    }
    elseif ($Mode -eq "test-only") {
        $runArguments += @("--test-only", $TestFilter)
    }

    $containerCreated = $true
    Invoke-Wslc -Arguments $runArguments

    $finalFingerprint = Get-SourceFingerprint
    if ($finalFingerprint -ne $sourceFingerprint) {
        throw "Repository-owned inputs changed during WSLC verification; no receipt was issued."
    }

    if ($Mode -ne "doctor") {
        $receipt = [ordered]@{
            schema = "laminaria-wslc-verification/v1"
            source_fingerprint = $sourceFingerprint
            image_id = $imageId
            mode = $Mode
            test_filter = if ($Mode -eq "test-only") { $TestFilter } else { $null }
            verified_at_utc = [DateTimeOffset]::UtcNow.ToString("o")
            owner_sid = $sid
        }
        $receipt | ConvertTo-Json | Set-Content -LiteralPath $receiptPath -Encoding utf8
        Write-Host "WSLC verification complete. Receipt: $receiptPath"
    }
}
finally {
    if ($containerCreated) {
        Invoke-Wslc -Arguments @("container", "rm", "--force", $containerName) -IgnoreFailure | Out-Null
    }
    Remove-Item -LiteralPath $iidFile, $cidFile -Force -ErrorAction SilentlyContinue
    if ($lockTaken) {
        Remove-Item -LiteralPath $leasePath -Force -ErrorAction SilentlyContinue
    }
    if ($lockTaken) {
        $mutex.ReleaseMutex()
    }
    $mutex.Dispose()
}
