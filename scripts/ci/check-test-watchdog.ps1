# the_suite_watchdog_can_actually_fail
#
# Planted cases for cargo-test-watched.ps1 (T-TEST-SUITE-WATCHDOG, plan row 113). The watchdog is run
# against a stand-in in cargo's place — a pwsh script that prints the lines cargo and libtest print
# (`Running …`, `test <name> has been running for over 60 seconds`, `test <name> ... ok`) and then
# hangs, ends or exits as each case says — with a small limit, so the whole suite takes seconds. No
# cargo is run. Each verdict is planted: a hang is ended red with its name and its process tree gone
# (the stand-in and a grandchild it started, checked by pid), a slow test that ends is green, silence
# is red with no name, the wait before the first test binary is not timed, cargo's exit code and every
# word pass through, and each shape in which libtest prints no names is refused before anything runs.
#
# What this does not prove is that libtest 1.94.1 prints those lines; the real-binary run in the
# ticket's report does, and is re-run when rust-toolchain.toml moves (docs/ARCHITECTURE-GUARDS.md,
# "Suite watchdog").

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$watchdog = Join-Path $PSScriptRoot 'cargo-test-watched.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) ('folio-test-watchdog-' + [guid]::NewGuid())
[IO.Directory]::CreateDirectory($root) | Out-Null
$standIn = Join-Path $root 'stand-in.ps1'
$pids = Join-Path $root 'pids.txt'

# The stand-in: FOLIO_STAND_IN_CASE picks what it prints and does; `test` and the watched words arrive as its arguments.
[IO.File]::WriteAllText($standIn, @'
$case = $env:FOLIO_STAND_IN_CASE
function Out([string]$line) { [Console]::Out.WriteLine($line); [Console]::Out.Flush() }
function Running { [Console]::Error.WriteLine('     Running unittests src/main.rs (stand-in)'); [Console]::Error.Flush() }
switch ($case) {
    'hang' {
        Running
        Out 'running 2 tests'
        Out 'test planted::ends ... ok'
        $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
        foreach ($w in @('-NoProfile', '-Command', 'Start-Sleep -Seconds 600')) { $start.ArgumentList.Add($w) }
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $grandchild = [Diagnostics.Process]::Start($start)
        [IO.File]::WriteAllText($env:FOLIO_STAND_IN_PIDS, "$PID $($grandchild.Id)")
        Out 'test planted::hangs has been running for over 60 seconds'
        Start-Sleep -Seconds 600
    }
    'slow' {
        Running
        Out 'running 1 test'
        Out 'test planted::slow has been running for over 60 seconds'
        Start-Sleep -Seconds 1
        Out 'test planted::slow - should panic ... ok'
        Out 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 61.00s'
        exit 0
    }
    'silent' {
        [IO.File]::WriteAllText($env:FOLIO_STAND_IN_PIDS, "$PID")
        Running
        Start-Sleep -Seconds 600
    }
    'compiling' {
        [Console]::Error.WriteLine('   Compiling planted v0.0.0 (stand-in)')
        Start-Sleep -Seconds 5
        Running
        Out 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
        exit 0
    }
    'failing' {
        Running
        Out 'test planted::fails ... FAILED'
        Out 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s'
        exit 101
    }
    'words' {
        foreach ($a in $args) { Out "[$a]" }
        exit 0
    }
}
'@)

$failed = @()
function Watch([string]$Case, [int]$Limit, [string[]]$Words) {
    $env:FOLIO_TEST_WATCHDOG_STAND_IN = $standIn
    $env:FOLIO_TEST_WATCHDOG_LIMIT_SECONDS = "$Limit"
    $env:FOLIO_STAND_IN_CASE = $Case
    $env:FOLIO_STAND_IN_PIDS = $pids
    try {
        # Waited for by its exit, never by the end of its output: a grandchild that a regressed watchdog left
        # alive inherits the pipes and would hold them open for ten minutes; it is a FAIL in `Gone` instead.
        $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
        foreach ($w in @('-NoProfile', '-File', $watchdog) + $Words) { $start.ArgumentList.Add([string]$w) }
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $said = [Collections.Concurrent.ConcurrentQueue[string]]::new()
        $p = [Diagnostics.Process]::new()
        $p.StartInfo = $start
        $take = { if ($null -ne $EventArgs.Data) { $Event.MessageData.Enqueue($EventArgs.Data) } }
        $o = Register-ObjectEvent -InputObject $p -EventName OutputDataReceived -Action $take -MessageData $said
        $e = Register-ObjectEvent -InputObject $p -EventName ErrorDataReceived -Action $take -MessageData $said
        [void]$p.Start()
        $p.BeginOutputReadLine()
        $p.BeginErrorReadLine()
        if (-not $p.WaitForExit(120000)) { $p.Kill($true); return @{ code = -1; output = 'the watchdog itself did not end within 120 s' } }
        Start-Sleep -Milliseconds 500
        Unregister-Event -SourceIdentifier $o.Name
        Unregister-Event -SourceIdentifier $e.Name
        return @{ code = $p.ExitCode; output = (@($said.ToArray()) -join "`n") }
    } finally {
        foreach ($n in 'FOLIO_TEST_WATCHDOG_STAND_IN', 'FOLIO_TEST_WATCHDOG_LIMIT_SECONDS', 'FOLIO_STAND_IN_CASE', 'FOLIO_STAND_IN_PIDS') {
            Remove-Item "env:$n" -ErrorAction SilentlyContinue
        }
    }
}
function Check([string]$Name, [bool]$Ok, $Run) {
    if ($Ok) { Write-Host "PASS $Name" } else { Write-Host "FAIL $Name (exit $($Run.code))"; Write-Host $Run.output; $script:failed += $Name }
}
function Gone([string]$Name, $Run) {
    $alive = @()
    foreach ($p in ([IO.File]::ReadAllText($pids).Trim() -split ' ')) {
        if (Get-Process -Id ([int]$p) -ErrorAction SilentlyContinue) { $alive += $p; Stop-Process -Id ([int]$p) -Force }
    }
    Check "$Name, and the tree it started is gone" ($alive.Count -eq 0) $Run
    Remove-Item $pids
}

try {
    $run = Watch 'hang' 63 @('-p', 'planted')
    Check 'a named test that does not end is red with its name' ($run.code -eq 124 -and $run.output -match 'planted::hangs \(running for' -and $run.output -notmatch 'planted::ends \(running') $run
    Gone 'a hang' $run

    $run = Watch 'slow' 63 @()
    Check 'a named test that ends is green and listed as slow' ($run.code -eq 0 -and $run.output -match 'slow tests \(named by libtest at 60 s, ended\): planted::slow') $run

    $run = Watch 'silent' 3 @()
    Check 'silence once tests run is red with no name' ($run.code -eq 124 -and $run.output -match 'no test can be named') $run
    Gone 'silence' $run

    $run = Watch 'compiling' 3 @()
    Check 'the wait before the first test binary is not timed' ($run.code -eq 0) $run

    $run = Watch 'failing' 63 @()
    Check "cargo's exit code passes through" ($run.code -eq 101) $run

    $run = Watch 'words' 63 @('-p', 'bt-app', '', '--', '--skip', 'x', '--test-threads', '4')
    $want = "[test]`n[-p]`n[bt-app]`n[]`n[--]`n[--skip]`n[x]`n[--test-threads]`n[4]"
    Check 'every word arrives as written' ($run.code -eq 0 -and ($run.output -replace "`r", '').Contains($want)) $run

    $refused = @(
        @{ words = @('-q') }, @{ words = @('--quiet') },
        @{ words = @('--', '--nocapture') }, @{ words = @('--', '--no-capture') },
        @{ words = @('--', '--format', 'terse') }, @{ words = @('--', '--format=json') },
        @{ words = @('--', '--test-threads=1') }, @{ words = @('--', '--test-threads', '1') },
        @{ words = @(); env = 'RUST_TEST_THREADS'; value = '1' },
        @{ words = @(); env = 'RUST_TEST_NOCAPTURE'; value = '1' }
    )
    foreach ($r in $refused) {
        $label = ($r.words -join ' ')
        if ($r.ContainsKey('env')) { Set-Item "env:$($r.env)" $r.value; $label = "$($r.env)=$($r.value)" }
        try { $run = Watch 'words' 63 $r.words } finally { if ($r.ContainsKey('env')) { Remove-Item "env:$($r.env)" } }
        Check "refused before anything runs: $label" ($run.code -eq 2 -and $run.output -match 'refused' -and $run.output -notmatch 'stand-in') $run
    }
} finally {
    Remove-Item -Recurse -Force $root
}

if ($failed.Count -gt 0) { Write-Host "FAILED: $($failed -join '; ')"; exit 1 }
Write-Host 'the suite watchdog can actually fail: every case passed'
exit 0
