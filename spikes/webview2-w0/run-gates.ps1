# Run the ten gates.
#
# Each group is its own process, and the grouping is not arbitrary: gates 7 and
# 10 kill the browser process on purpose, so anything sharing a process with
# them would be measuring a corpse. Everything non-destructive runs first.
#
#   .\run-gates.ps1 -Shots C:\some\dir
param(
    [string]$Shots = (Join-Path $env:TEMP 'w0-shots'),
    [string[]]$Groups = @('1,2', '3,4', '5', '6', '8', '9', '7', '10')
)

$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'target\release\bt-spike-webview2-w0.exe'
if (-not (Test-Path $exe)) { throw "build it first: cargo build --release" }

New-Item -ItemType Directory -Force -Path $Shots | Out-Null
$log = Join-Path $Shots 'gates.jsonl'
if (Test-Path $log) { Remove-Item $log -Force }
# Created empty rather than left to the first run: the wait below reads this
# file, and with $ErrorActionPreference = 'Stop' a missing path is fatal even
# with -ErrorAction SilentlyContinue.
New-Item -ItemType File -Path $log | Out-Null
$env:BT_W0_LOG = $log

# **This waits on the evidence, not on the process.**
#
# Three ways of waiting for the probe were tried and all three hang long after
# it has exited: a pipeline (`& $exe | …`), `Start-Process -Wait`, and
# `Process.WaitForExit()`. The cause is the same each time — WebView2's browser
# children inherit this shell's handles and outlive the probe by a moment, and
# every one of those waits is really a wait on the handle rather than on the
# program. The probe's own last line is `{"event":"done"}`, so counting those in
# the log is a wait on the thing actually being waited for.
function Wait-ForGateRun {
    param([string]$Log, [int]$Before, [int]$TimeoutSeconds = 420)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        $now = @(Select-String -Path $Log -Pattern '"event":"done"' -ErrorAction SilentlyContinue).Count
        if ($now -gt $Before) { return $true }
    }
    return $false
}

# No msedgewebview2 left that belongs to this probe's user data folder.
#
# A new environment opened over a folder the previous browser tree still holds
# does not fail loudly — the controller callback simply never arrives, and the
# run dies two log lines in. Waiting for the folder's owners to go is cheaper
# than diagnosing that twice.
function Wait-ForEngineToClear {
    param([string]$Folder)
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        $mine = @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -like "*$Folder*" })
        if ($mine.Count -eq 0) { Start-Sleep -Milliseconds 1500; return }
        Start-Sleep -Milliseconds 500
    }
}

foreach ($group in $Groups) {
    Write-Host "── gates $group ─────────────────────────────────────────────"
    $before = @(Select-String -Path $log -Pattern '"event":"done"' -ErrorAction SilentlyContinue).Count
    Start-Process -FilePath $exe -ArgumentList @('--gates', $group, '--shots', $Shots) | Out-Null
    if (-not (Wait-ForGateRun -Log $log -Before $before)) {
        Write-Host "  (timed out waiting for gates $group to finish)"
    }
    Wait-ForEngineToClear -Folder $Shots
}

Write-Host ""
Write-Host "verdicts:"
Get-Content $log | Where-Object { $_ -match '"event":"verdict"' } | ForEach-Object {
    $row = ($_ -replace '^W0 ', '') | ConvertFrom-Json
    "{0,3}  {1,-8} {2}" -f $row.gate, $row.verdict, $row.note
}
Write-Host ""
Write-Host "log: $log"
