$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw "ASSERTION FAILED: $Message"
    }
}

$subject = (Resolve-Path (Join-Path $PSScriptRoot "..\windows-wslc-ci.ps1")).Path
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "laminaria-wslc-helper-test-$([Guid]::NewGuid().ToString('N'))"
$testState = "$testRoot-state"
$testScripts = Join-Path $testRoot "scripts"
$global:FakeWslcCalls = [System.Collections.ArrayList]::new()
$global:FakeRunExitCode = 0
$global:FakeCleanupExitCode = 0
$global:FakeVmcomputeStatus = "Running"
$global:FakeWslcProcesses = @()

function global:Get-Service {
    param([string]$Name, $ErrorAction)
    if ($Name -eq "vmcompute") {
        return [pscustomobject]@{ Name = $Name; Status = $global:FakeVmcomputeStatus }
    }
    return [pscustomobject]@{ Name = $Name; Status = "Running" }
}

function global:Get-Process {
    param([string]$Name, $ErrorAction)
    if ($Name -eq "wslc") {
        return $global:FakeWslcProcesses
    }
    return @()
}

function global:wslc {
    $call = [string[]]$args
    $null = $global:FakeWslcCalls.Add($call)
    $exitCode = 0

    if ($call.Count -gt 0 -and $call[0] -eq "build") {
        $iidIndex = [Array]::IndexOf($call, "--iidfile")
        if ($iidIndex -lt 0) {
            throw "fake wslc: build did not request --iidfile"
        }
        Set-Content -LiteralPath $call[$iidIndex + 1] -Value "sha256:integration-test-image" -NoNewline
    }
    elseif ($call.Count -gt 0 -and $call[0] -eq "run") {
        $cidIndex = [Array]::IndexOf($call, "--cidfile")
        if ($cidIndex -ge 0) {
            Set-Content -LiteralPath $call[$cidIndex + 1] -Value "integration-test-container-id" -NoNewline
        }
        $exitCode = $global:FakeRunExitCode
    }
    elseif ($call.Count -gt 1 -and $call[0] -eq "container" -and $call[1] -eq "rm") {
        $exitCode = $global:FakeCleanupExitCode
    }

    $global:LASTEXITCODE = $exitCode
}

try {
    New-Item -ItemType Directory -Path $testScripts -Force | Out-Null
    New-Item -ItemType Directory -Path $testState -Force | Out-Null
    Copy-Item -LiteralPath $subject -Destination (Join-Path $testScripts "windows-wslc-ci.ps1")
    Set-Content -LiteralPath (Join-Path $testScripts "local-ci.sh") -Value "#!/usr/bin/env bash`nexit 0`n"
    Set-Content -LiteralPath (Join-Path $testRoot "input.txt") -Value "initial"
    Set-Content -LiteralPath (Join-Path $testRoot "Cargo.lock") -Value "version = 4`n"
    & git -C $testRoot init --quiet
    if ($LASTEXITCODE -ne 0) {
        throw "git init failed"
    }
    & git -C $testRoot add -- Cargo.lock
    if ($LASTEXITCODE -ne 0) {
        throw "git add Cargo.lock failed"
    }
    & git -C $testRoot -c user.name="WSLC test" -c user.email="wslc-test@example.invalid" commit --quiet -m "test baseline"
    if ($LASTEXITCODE -ne 0) {
        throw "git baseline commit failed"
    }

    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $mutexName = "Global\Asopitech.LaminariaBootstrap.Wslc.$sid"
    $readyPath = Join-Path $testRoot "mutex-ready"
    $holder = Start-Job -ArgumentList $mutexName, $readyPath -ScriptBlock {
        param($Name, $ReadyPath)
        $mutex = [System.Threading.Mutex]::new($false, $Name)
        $taken = $false
        try {
            $taken = $mutex.WaitOne()
            Set-Content -LiteralPath $ReadyPath -Value "ready"
            Start-Sleep -Seconds 10
        }
        finally {
            if ($taken) {
                $mutex.ReleaseMutex()
            }
            $mutex.Dispose()
        }
    }
    try {
        for ($attempt = 0; $attempt -lt 50 -and -not (Test-Path -LiteralPath $readyPath); $attempt++) {
            Start-Sleep -Milliseconds 100
        }
        Assert-True (Test-Path -LiteralPath $readyPath) "the competing mutex holder must become ready"
        $contentionRejected = $false
        try {
            & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -LockTimeoutSeconds 1 -StateDirectory $testState
        }
        catch {
            $contentionRejected = $_.Exception.Message -like "*Timed out waiting for the WSLC owner lock*"
        }
        Assert-True $contentionRejected "a second worktree/process must not enter the shared WSLC session"
        Assert-True ($global:FakeWslcCalls.Count -eq 0) "lock contention must be rejected before any WSLC call"
    }
    finally {
        Stop-Job -Job $holder -ErrorAction SilentlyContinue
        Remove-Job -Job $holder -Force -ErrorAction SilentlyContinue
    }

    $global:FakeVmcomputeStatus = "StartPending"
    $hostTransitionRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -StateDirectory $testState
    }
    catch {
        $hostTransitionRejected = $_.Exception.Message -like "*vmcompute is StartPending*"
    }
    Assert-True $hostTransitionRejected "a transitional HCS service must reject WSLC before the first client call"
    Assert-True ($global:FakeWslcCalls.Count -eq 0) "an unhealthy host must not receive any WSLC call"

    $global:FakeVmcomputeStatus = "Running"
    $global:FakeWslcProcesses = @([pscustomobject]@{ Id = 4242 })
    $externalClientRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -StateDirectory $testState
    }
    catch {
        $externalClientRejected = $_.Exception.Message -like "*external wslc client*4242*"
    }
    Assert-True $externalClientRejected "an already-running raw WSLC client must be rejected"
    Assert-True ($global:FakeWslcCalls.Count -eq 0) "external client rejection must precede every WSLC call"
    $global:FakeWslcProcesses = @()

    & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode full -StateDirectory $testState

    $buildCall = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "build" })[0]
    $runCall = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "run" })[0]
    Assert-True ($null -ne $buildCall) "the owner harness must build"
    Assert-True ($null -ne $runCall) "the owner harness must run"
    Assert-True ($buildCall -notcontains "--session") "build must use the CLI-created default per-user singleton"
    Assert-True ($runCall -notcontains "--session") "run must use the same default per-user singleton"
    Assert-True ($runCall -contains "sha256:integration-test-image") "run must consume the exact --iidfile image ID"
    Assert-True ($runCall -contains "--rm") "run must request automatic container removal"

    $nameIndex = [Array]::IndexOf($runCall, "--name")
    Assert-True ($nameIndex -ge 0) "run must assign an owned container name"
    $ownedContainer = $runCall[$nameIndex + 1]
    Assert-True ($ownedContainer -match '^laminaria-ci-[0-9a-f]{32}$') "the owned container name must be unique and constrained"

    $normalCleanupCalls = @($global:FakeWslcCalls | Where-Object { $_.Count -ge 4 -and $_[0] -eq "container" -and $_[1] -eq "rm" })
    Assert-True ($normalCleanupCalls.Count -eq 0) "a returned --rm run must not receive a competing explicit cleanup"

    & (Join-Path $testScripts "windows-wslc-ci.ps1") -VerifyReceipt -RequiredCoverage full -StateDirectory $testState

    Set-Content -LiteralPath (Join-Path $testRoot "input.txt") -Value "changed"
    $staleRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -VerifyReceipt -RequiredCoverage full -StateDirectory $testState
    }
    catch {
        $staleRejected = $_.Exception.Message -like "*receipt is stale*"
    }
    Assert-True $staleRejected "a source change must invalidate the receipt"

    $receiptPath = Join-Path (& git -C $testRoot rev-parse --absolute-git-dir) "laminaria-wslc-verification.json"
    Remove-Item -LiteralPath $receiptPath -Force
    $global:FakeWslcCalls.Clear()
    & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode lockfile-update -StateDirectory $testState
    $lockfileRun = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "run" })[0]
    Assert-True ($null -ne $lockfileRun) "lockfile-update must run under the WSLC owner harness"
    Assert-True ($lockfileRun -contains "--mount") "lockfile-update must bind only the repository into the owned container"
    Assert-True ($lockfileRun -contains "cargo") "lockfile-update must invoke Cargo inside the container"
    Assert-True ($lockfileRun -contains "update") "lockfile-update must use Cargo's lock updater"
    Assert-True ($lockfileRun -contains "--package") "lockfile-update must scope updates to one package"
    Assert-True ($lockfileRun -contains "cargo_metadata") "lockfile-update must scope updates to cargo_metadata"
    Assert-True ($lockfileRun -contains "--precise") "lockfile-update must pin the selected MSRV-compatible release"
    Assert-True ($lockfileRun -contains "0.18.1") "lockfile-update must use the declared compatible release"
    Assert-True ($lockfileRun -contains "--offline") "lockfile-update must use dependencies cached by the preceding image build"
    Assert-True (-not (Test-Path -LiteralPath $receiptPath)) "lockfile-update must not issue a verification receipt"

    $global:FakeWslcCalls.Clear()
    $global:FakeRunExitCode = 23
    $runFailureRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -StateDirectory $testState
    }
    catch {
        $runFailureRejected = $_.Exception.Message -like "*failed with exit code 23*"
    }
    Assert-True $runFailureRejected "a failing container run must fail the harness"

    $failureCleanupCalls = @($global:FakeWslcCalls | Where-Object {
        $_.Count -ge 4 -and $_[0] -eq "container" -and $_[1] -eq "rm"
    })
    Assert-True ($failureCleanupCalls.Count -eq 0) "a returned failing --rm run must not receive a competing explicit cleanup"
    $leasePath = Join-Path $testState "laminaria-wslc-owner-$sid.json"
    Assert-True (-not (Test-Path -LiteralPath $leasePath)) "a returned run must clear its owner lease after automatic removal"

    $abandonedContainer = "laminaria-ci-$([Guid]::NewGuid().ToString('N'))"
    [ordered]@{
        schema = "laminaria-wslc-owner/v2"
        session_identity = "default-per-user"
        container_name = $abandonedContainer
        owner_pid = 999999
        source_fingerprint = "abandoned"
        acquired_at_utc = [DateTimeOffset]::UtcNow.ToString("o")
    } | ConvertTo-Json | Set-Content -LiteralPath $leasePath -Encoding utf8

    $global:FakeWslcCalls.Clear()
    $global:FakeRunExitCode = 0
    $global:FakeCleanupExitCode = 17
    $cleanupFailureRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -StateDirectory $testState
    }
    catch {
        $cleanupFailureRejected = $_.Exception.Message -like "*lease is retained*"
    }
    Assert-True $cleanupFailureRejected "failed abandoned-owner cleanup must reject new work"
    Assert-True (Test-Path -LiteralPath $leasePath) "failed cleanup must retain the recovery lease"
    $callsAfterCleanupFailure = @($global:FakeWslcCalls)
    Assert-True ($callsAfterCleanupFailure.Count -eq 1) "failed recovery must stop before build or run"

    $global:FakeWslcCalls.Clear()
    $global:FakeCleanupExitCode = 0
    & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -StateDirectory $testState
    $recoveryCall = @($global:FakeWslcCalls)[0]
    Assert-True ($recoveryCall -notcontains "--session") "recovery must use the default per-user singleton"
    Assert-True ($recoveryCall[0] -eq "container" -and $recoveryCall[1] -eq "rm" -and $recoveryCall[2] -eq "--force") "recovery must explicitly remove one leased container"
    Assert-True ($recoveryCall[3] -eq $abandonedContainer) "recovery must use only the exact leased container name"
    Assert-True (-not (Test-Path -LiteralPath $leasePath)) "successful recovery and returned run must clear the lease"

    Write-Host "windows-wslc-ci integration tests passed."
}
finally {
    Remove-Item -LiteralPath function:\global:wslc -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath function:\global:Get-Service -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath function:\global:Get-Process -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
    if (Test-Path -LiteralPath $testState) {
        Remove-Item -LiteralPath $testState -Recurse -Force
    }
}
