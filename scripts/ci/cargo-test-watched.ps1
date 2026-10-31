# cargo-test-watched.ps1 — `cargo test <args>`, ended red and named when a test hangs
#
#   pwsh -NoProfile -File scripts/ci/cargo-test-watched.ps1 <the arguments of `cargo test`>
#   (on a lane: the lane tools' `ccheck.sh scripts/ci/cargo-test-watched.ps1 -p bt-app --no-fail-fast`)
#
# **Why** (T-TEST-SUITE-WATCHDOG, plan row 113, 2026-10-31). libtest has no timeout on the
# stable toolchain: `--ensure-time` and `--report-time` are nightly-only, and even there they judge a
# test when it ends, which a hung test never does. A hung test therefore held the whole bt-app run for
# two hours on a lane (row 85, 2026-10-29) and holds CI's until the step's clock cancels it with no
# name. What stable libtest does give is the name: in its default (pretty, parallel) output every test
# still running 60 s after it started is announced once — `test <name> has been running for over 60
# seconds` — and every test that ends is announced again — `test <name> ... ok|FAILED|ignored`. This
# script runs cargo as its child, passes every line through as text (each to the stream it came from), and
# reads those two lines.
#
# **The limit is one number, `$limit` below: 300 s per test** (30 x `lane::PATIENCE`).
#   * A test libtest has named and that has not ended 300 s after it started ends the run.
#   * So does a run that prints nothing at all for 300 s once its first test binary started (cargo's
#     `Running` / `Doc-tests` line): the harness itself is stuck, or a binary did not exit after its
#     summary. An honest run is never silent that long — libtest's own 60 s line breaks any silence.
#   Compiling is never timed (the clock starts at the first `Running` line); CI's step clocks bound it.
#   Measured 2026-10-31, on the watched runs: no bt-app test reaches 60 s on any CI runner, and the
#   slowest watched test is about 61 s (bt-source on `logic`). Unwatched, core-linux's bt-source tests
#   take about 110 s. Re-derive the limit, not bump it, if a watched test passes 150 s (half the limit).
#
# Ending the run: the names, each with its age, then the whole process tree this script started (cargo,
# the test binary, any child a test started) is ended by the pid it holds, and the exit code is 124 —
# told apart from a failing test's 101. Otherwise cargo's own exit code is passed through, after a
# list of the tests libtest named at 60 s that did end (information, never a failure).
#
# **Shapes in which libtest prints no names to read are refused** (exit 2), never run unwatched:
# `-q`/`--quiet` (or `CARGO_TERM_QUIET=true`: cargo hands libtest `--quiet`, whose terse output has the 60 s
# line but no end-of-test names; a `term.quiet` in a cargo config file is not seen here and must not be set),
# `--format`, `--test-threads=1` (or `RUST_TEST_THREADS=1`; libtest's serial path
# has no 60 s line), `--nocapture`/`--no-capture` (or `RUST_TEST_NOCAPTURE`; a test's own output would
# break the harness's lines).
#
# What it does not name (docs/ARCHITECTURE-GUARDS.md, "Suite watchdog"): a hang that holds the
# process's real stdout lock (the harness cannot print either: red by silence, with no name), and a
# child process its parent test no longer waits for (not in the tree once its parent is gone).
#
# **No param block, on purpose** (as in the lane tools' `capped.ps1`): with one, pwsh's binder would read
# `-p` as an ambiguous common parameter before anything ran; every word goes to cargo as written.
#
# Two variables exist for `check-test-watchdog.ps1` alone and are printed when set:
# FOLIO_TEST_WATCHDOG_STAND_IN (a .ps1 run in cargo's place) and FOLIO_TEST_WATCHDOG_LIMIT_SECONDS.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$limit = 300
$me = 'cargo-test-watched'

function Refuse([string]$why) {
    [Console]::Error.WriteLine("${me}: refused: $why (libtest would print no names to read)")
    exit 2
}

$words = @($args)
$afterDashes = $false
for ($i = 0; $i -lt $words.Count; $i++) {
    $w = [string]$words[$i]
    if ($w -eq '--' -and -not $afterDashes) { $afterDashes = $true; continue }
    if ($w -eq '-q' -or $w -eq '--quiet') { Refuse "'$w'" }
    if ($w -eq '--nocapture' -or $w -eq '--no-capture') { Refuse "'$w'" }
    if ($afterDashes -and ($w -eq '--format' -or $w.StartsWith('--format='))) { Refuse "'$w'" }
    if ($afterDashes -and $w -eq '--test-threads=1') { Refuse "'$w'" }
    if ($afterDashes -and $w -eq '--test-threads' -and $i + 1 -lt $words.Count -and [string]$words[$i + 1] -eq '1') { Refuse "'--test-threads 1'" }
}
if ($env:CARGO_TERM_QUIET -eq 'true') { Refuse 'CARGO_TERM_QUIET=true' }
if ($env:RUST_TEST_THREADS -eq '1') { Refuse 'RUST_TEST_THREADS=1' }
if ($env:RUST_TEST_NOCAPTURE -and $env:RUST_TEST_NOCAPTURE -ne '0') { Refuse "RUST_TEST_NOCAPTURE=$($env:RUST_TEST_NOCAPTURE)" }

if ($env:FOLIO_TEST_WATCHDOG_LIMIT_SECONDS) {
    $limit = [int]$env:FOLIO_TEST_WATCHDOG_LIMIT_SECONDS
    Write-Host "${me}: limit set by FOLIO_TEST_WATCHDOG_LIMIT_SECONDS"
}
$start = [Diagnostics.ProcessStartInfo]::new()
if ($env:FOLIO_TEST_WATCHDOG_STAND_IN) {
    Write-Host "${me}: stand-in $($env:FOLIO_TEST_WATCHDOG_STAND_IN) in cargo's place"
    $start.FileName = (Get-Process -Id $PID).Path
    foreach ($w in @('-NoProfile', '-File', $env:FOLIO_TEST_WATCHDOG_STAND_IN)) { $start.ArgumentList.Add($w) }
} else {
    $start.FileName = 'cargo'
}
$start.ArgumentList.Add('test')
foreach ($w in $words) { $start.ArgumentList.Add([string]$w) }
$start.UseShellExecute = $false
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
$start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
Write-Host "${me}: a test still running $limit s after it started, or $limit s of silence once tests run, ends the run (exit 124)"

$child = [Diagnostics.Process]::Start($start)
$clock = [Diagnostics.Stopwatch]::StartNew()
$ansi = [regex]'\x1b\[[0-9;]*[A-Za-z]'
$binaryStarts = [regex]'^\s*(Running|Doc-tests)\s'
$announced = [regex]'^test (.+) has been running for over (\d+) seconds$'
$ended = [regex]'^test (.+?) \.\.\. (ok|FAILED|ignored)\b'
# name -> started at (seconds on $clock); ordinal, as Rust's test names are case-sensitive
$running = [Collections.Generic.Dictionary[string, double]]::new([StringComparer]::Ordinal)
$slow = [Collections.Generic.List[string]]::new()
$watching = $false
$lastLine = 0.0
$lastText = ''
$exitSeen = $null

function Read-Line($line, [bool]$isError) {
    if ($isError) { [Console]::Error.WriteLine($line) } else { [Console]::Out.WriteLine($line) }
    $now = $clock.Elapsed.TotalSeconds
    $script:lastLine = $now
    $script:lastText = $line
    $plain = $ansi.Replace($line, '')
    if ($binaryStarts.IsMatch($plain)) {
        $script:watching = $true
        # cargo runs one test binary after another: a name still tracked when the next one starts belonged to
        # a binary that ended without its result line (a crash, an abort, a kill). It is not a hang of this one.
        if ($running.Count -gt 0) {
            [Console]::Out.WriteLine("${me}: ended with their binary, no result line: $(@($running.Keys) -join ', ')")
            $running.Clear()
        }
        return
    }
    $m = $announced.Match($plain)
    if ($m.Success) { $running[$m.Groups[1].Value] = $now - [double]$m.Groups[2].Value; return }
    $m = $ended.Match($plain)
    if ($m.Success) {
        $name = $m.Groups[1].Value
        # A test with a mode is ended as `test <name> - should panic ... ok`; its 60 s line has the bare name.
        foreach ($known in @($running.Keys)) {
            if ($name -ceq $known -or $name.StartsWith("$known - ", [StringComparison]::Ordinal)) { [void]$running.Remove($known); $slow.Add($known) }
        }
    }
}

function End-Run([string[]]$said) {
    foreach ($line in $said) { [Console]::Out.WriteLine("${me}: $line") }
    [Console]::Out.WriteLine("${me}: ending the process tree of pid $($child.Id); this run is red")
    [Console]::Out.Flush()
    try { $child.Kill($true) }
    catch [InvalidOperationException] { }   # it may have exited on its own meanwhile
    catch [AggregateException] {            # a descendant that could not be ended; the run is red all the same
        [Console]::Out.WriteLine("${me}: part of the tree could not be ended: $($_.Exception.InnerExceptions.Message -join '; ')")
    }
    $child.WaitForExit()
    exit 124
}

$outTask = $child.StandardOutput.ReadLineAsync()
$errTask = $child.StandardError.ReadLineAsync()
while ($outTask -or $errTask) {
    $pending = @(@($outTask, $errTask) | Where-Object { $_ })
    $woke = [Threading.Tasks.Task]::WaitAny([Threading.Tasks.Task[]]$pending, 1000)
    # Once cargo has exited, everything it wrote is already in the pipes: a quiet second, or 5 s in all,
    # ends the reading. A process a test left behind may hold the pipes open (and even keep writing to
    # them), and it is not this run.
    if ($child.HasExited) {
        if ($null -eq $exitSeen) { $exitSeen = $clock.Elapsed.TotalSeconds }
        if ($woke -eq -1 -or $clock.Elapsed.TotalSeconds - $exitSeen -ge 5) { break }
    }
    if ($outTask -and $outTask.IsCompleted) {
        $line = $outTask.Result
        if ($null -eq $line) { $outTask = $null } else { Read-Line $line $false; $outTask = $child.StandardOutput.ReadLineAsync() }
    }
    if ($errTask -and $errTask.IsCompleted) {
        $line = $errTask.Result
        if ($null -eq $line) { $errTask = $null } else { Read-Line $line $true; $errTask = $child.StandardError.ReadLineAsync() }
    }
    $now = $clock.Elapsed.TotalSeconds
    $over = @($running.Keys | Where-Object { $now - $running[$_] -ge $limit })
    if ($over.Count -gt 0) {
        $said = @("these tests were still running $limit s after they started:")
        foreach ($name in $running.Keys) { $said += "    $name (running for $([int]($now - $running[$name])) s)" }
        End-Run $said
    }
    if ($watching -and $running.Count -eq 0 -and $now - $lastLine -ge $limit) {
        End-Run @("the run printed nothing for $limit s; no test can be named (a hang that holds the process's stdout, or a binary that did not exit)", "the last line was: $lastText")
    }
}
$child.WaitForExit()
if ($slow.Count -gt 0) {
    [Console]::Out.WriteLine("${me}: slow tests (named by libtest at 60 s, ended): $($slow -join ', ')")
}
exit $child.ExitCode
