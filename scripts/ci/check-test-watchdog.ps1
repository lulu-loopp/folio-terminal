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
function Running { [Console]::Error.WriteLine('     Running unittests (stand-in)'); [Console]::Error.Flush() }
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
        Out 'test planted::slow - should panic ... ok'
        Out 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 61.00s'
        exit 0
    }
    'slow-then-busy' {
        Running
        Out 'running 2 tests'
        Out 'test planted::plain has been running for over 60 seconds'
        Out 'test README.md - planted (line 3) has been running for over 60 seconds'
        Out 'test planted::plain ... ok'
        Out 'test README.md - planted (line 3) - compile fail ... ok'
        foreach ($i in 1..12) { Start-Sleep -Seconds 1; Out "busy line $i" }
        Out 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 73.00s'
        exit 0
    }
    'two-binaries' {
        Running
        Out 'running 1 test'
        Out 'test planted::dies has been running for over 60 seconds'
        [Console]::Error.WriteLine('error: test failed, to rerun pass `-p planted --lib`')
        Running
        Out 'running 1 test'
        foreach ($i in 1..8) { Start-Sleep -Seconds 1; Out "busy line $i" }
        Out 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.00s'
        exit 101
    }
    'boundary-before-name' {
        # cargo's `Running` (stderr) and libtest's lines (stdout) are two pipes, and the watchdog may read the
        # next binary's `Running` before the dead binary's 60 s line. Planted on ONE pipe, so that is the
        # order the watchdog reads, every time: the boundary it trusts must be libtest's own `running N tests`.
        Out '     Running unittests (stand-in)'
        Out 'running 1 test'
        Out '     Running unittests (stand-in)'
        Out 'test planted::dies has been running for over 60 seconds'
        Out 'running 1 test'
        foreach ($i in 1..8) { Start-Sleep -Seconds 1; Out "busy line $i" }
        Out 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.00s'
        exit 101
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
        $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
        foreach ($w in @('-NoProfile', '-File', $watchdog) + $Words) { $start.ArgumentList.Add([string]$w) }
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $p = [Diagnostics.Process]::Start($start)
        $out = $p.StandardOutput.ReadToEndAsync()
        $err = $p.StandardError.ReadToEndAsync()
        $clock = [Diagnostics.Stopwatch]::StartNew()
        if (-not $p.WaitForExit(120000)) { $p.Kill($true); return @{ code = -1; output = 'the watchdog itself did not end within 120 s' } }
        # The output is all here when both pipes are at their end, never after a sleep (row 171: a loaded
        # runner may deliver it later than any sleep). The 120 s bound is met only by a regression: a
        # grandchild that a regressed watchdog left alive holds the pipes open, and it is a FAIL in `Gone`.
        $left = [Math]::Max(0, 120000 - [int]$clock.ElapsedMilliseconds)
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($out, $err), $left)) {
            return @{ code = $p.ExitCode; output = 'the watchdog exited, but its output did not end within 120 s (a process it left holds its pipes)' }
        }
        return @{ code = $p.ExitCode; output = ($out.Result + "`n" + $err.Result) -replace "`r", '' }
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
    # Timed: the 60 s line backdates the test's start by 60 s, so with a 63 s limit the run ends about 3 s
    # after it. A clock that started at the 60 s line instead would take 63 s.
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $run = Watch 'hang' 63 @('-p', 'planted')
    $took = $timer.Elapsed.TotalSeconds
    Check "a named test is timed from its start, not from its 60 s line ($([int]$took) s)" ($took -lt 20) $run
    Check 'a named test that does not end is red with its name' ($run.code -eq 124 -and $run.output -match 'planted::hangs \(running for' -and $run.output -notmatch 'planted::ends \(running') $run
    Gone 'a hang' $run

    $run = Watch 'slow' 70 @()
    Check 'a named test that ends is green and listed as slow' ($run.code -eq 0 -and $run.output -match 'slow tests \(named by libtest at 60 s, ended\): planted::slow') $run

    # A plain `... ok` and a spaced doc-test name with a mode each take their test out of the tracked set:
    # the run goes on past both tests' limit and must stay green. Limit 70: the end lines have 10 s from the
    # 60 s lines to be read on a loaded runner (row 171), and the 12 busy seconds pass that limit.
    $run = Watch 'slow-then-busy' 70 @()
    Check 'ended tests leave the tracked set (a plain end, a doc-test name with a mode)' ($run.code -eq 0 -and $run.output -match 'slow tests \(named by libtest at 60 s, ended\): planted::plain, README.md - planted \(line 3\)') $run

    # A binary that ends while a named test runs (no result line) does not leave a phantom hang for the next one.
    $run = Watch 'two-binaries' 63 @('--no-fail-fast')
    Check "a test whose binary ended is not a later binary's hang" ($run.code -eq 101 -and $run.output -match 'ended with their binary, no result line: planted::dies') $run

    # The same, with the next binary's `Running` read before the dead binary's 60 s line (the order two pipes
    # can arrive in on a loaded runner; plan row 171): the binary boundary is libtest's `running N tests`.
    $run = Watch 'boundary-before-name' 63 @()
    Check "a test whose binary ended is not a later binary's hang, whichever pipe is read first" ($run.code -eq 101 -and $run.output -match 'ended with their binary, no result line: planted::dies') $run

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
        @{ words = @(); env = 'RUST_TEST_NOCAPTURE'; value = '1' },
        @{ words = @(); env = 'CARGO_TERM_QUIET'; value = 'true' }
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
