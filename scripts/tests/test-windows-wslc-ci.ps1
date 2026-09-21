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
$testScripts = Join-Path $testRoot "scripts"
$global:FakeWslcCalls = [System.Collections.ArrayList]::new()
$global:FakeRunExitCode = 0

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

    $global:LASTEXITCODE = $exitCode
}

try {
    New-Item -ItemType Directory -Path $testScripts -Force | Out-Null
    Copy-Item -LiteralPath $subject -Destination (Join-Path $testScripts "windows-wslc-ci.ps1")
    Set-Content -LiteralPath (Join-Path $testScripts "local-ci.sh") -Value "#!/usr/bin/env bash`nexit 0`n"
    Set-Content -LiteralPath (Join-Path $testRoot "input.txt") -Value "initial"
    & git -C $testRoot init --quiet
    if ($LASTEXITCODE -ne 0) {
        throw "git init failed"
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
            & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast -LockTimeoutSeconds 1
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

    & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode full

    $buildCall = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "build" })[0]
    $runCall = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "run" })[0]
    Assert-True ($null -ne $buildCall) "the owner harness must build"
    Assert-True ($null -ne $runCall) "the owner harness must run"
    Assert-True ($runCall -contains "sha256:integration-test-image") "run must consume the exact --iidfile image ID"
    Assert-True ($runCall -contains "--rm") "run must request automatic container removal"

    $nameIndex = [Array]::IndexOf($runCall, "--name")
    Assert-True ($nameIndex -ge 0) "run must assign an owned container name"
    $ownedContainer = $runCall[$nameIndex + 1]
    Assert-True ($ownedContainer -match '^laminaria-ci-[0-9a-f]{32}$') "the owned container name must be unique and constrained"

    & (Join-Path $testScripts "windows-wslc-ci.ps1") -VerifyReceipt -RequiredCoverage full

    Set-Content -LiteralPath (Join-Path $testRoot "input.txt") -Value "changed"
    $staleRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -VerifyReceipt -RequiredCoverage full
    }
    catch {
        $staleRejected = $_.Exception.Message -like "*receipt is stale*"
    }
    Assert-True $staleRejected "a source change must invalidate the receipt"

    $global:FakeWslcCalls.Clear()
    $global:FakeRunExitCode = 23
    $runFailureRejected = $false
    try {
        & (Join-Path $testScripts "windows-wslc-ci.ps1") -Mode fast
    }
    catch {
        $runFailureRejected = $_.Exception.Message -like "*failed with exit code 23*"
    }
    Assert-True $runFailureRejected "a failing container run must fail the harness"

    $failedRun = @($global:FakeWslcCalls | Where-Object { $_[0] -eq "run" })[0]
    $failedNameIndex = [Array]::IndexOf($failedRun, "--name")
    $failedContainer = $failedRun[$failedNameIndex + 1]
    $cleanupCall = @($global:FakeWslcCalls | Where-Object {
        $_.Count -ge 4 -and $_[0] -eq "container" -and $_[1] -eq "rm"
    })[-1]
    Assert-True ($cleanupCall[2] -eq "--force") "failure cleanup must be forceful rather than waiting on broad service shutdown"
    Assert-True ($cleanupCall[3] -eq $failedContainer) "failure cleanup must target only the invocation's exact container"

    Write-Host "windows-wslc-ci integration tests passed."
}
finally {
    Remove-Item -LiteralPath function:\global:wslc -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
