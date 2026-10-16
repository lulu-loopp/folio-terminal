<#
.SYNOPSIS
    The guest watcher's bounded waits (clean-vm.md 4.4): every wait of
    `watch.ps1` that is not its own row timeout goes through one of these, with
    a named bound and its reason. Dot-sourced by `watch.ps1`; read by
    `cleanvm-tests.ps1` on the host.

.DESCRIPTION
    Windows PowerShell 5.1 dialect (the guest's edition), UTF-8 with a BOM.

    **$KillWithinMs — 2 s, the product's `CHILD_EXIT_BUDGET`** (`bt-pty`): a
    `TerminateProcess` a healthy process answers in microseconds; what the bound
    covers is a process inside a driver call the kernel will not interrupt. A
    trial the watcher killed that is still running after the bound has not
    ended, and the row is not run on as if it had.

    **$W4RunWithinMs — 90 s.** From `Handoff` read, the applier waits for the
    old build's claim within `Limits::PRODUCT.old_within` (60 s; past it the
    applier records `OldStayed`, abandons, and never writes a Run value), then
    checks the staged set against what was verified and writes the Run value
    (`at_handoff`). 30 s over the 60 s carries that check and the write, flush
    and read-back of the value; a Run value that has not come by then will not
    come in this row.

    **$SpinWithinMs — 180 s**, for the rows whose state lasts milliseconds
    (W5-W13 but W10): the longest chain of the product's own bounds a spin can
    stand in, `old_within` 60 s + the trial's 90 s (`TRIAL_DEADLINE_MS`) +
    `quit_within` 5 s + `end_within` 5 s + `JOURNAL_HELD_WITHIN` 10 s = 170 s,
    and 10 s for the moves between them.

    **$FactPauseCapMs — 250 ms**: no wait sleeps longer than this between two
    looks, so a fact that holds is seen within a quarter second of it.
#>

$KillWithinMs = 2000
$W4RunWithinMs = 90000
$SpinWithinMs = 180000
$FactPauseCapMs = 250

# **Ask `$Look` until the fact holds or its state has gone, and stop asking after `$WithinMs`.**
# `$Look` answers @{ Holds = <bool>; Gone = <bool>; Seen = <what it saw> }. The answer is
# @{ Outcome = 'held' | 'gone' | 'bound'; Seen = <the last look's Seen>; ElapsedMs = <ms> };
# 'bound' is the fact not holding when the bound ran out, which the caller fails the row on.
# `$PauseMs` (0 = a spin) is slept between two looks, never past the bound.
function Wait-Fact {
  param(
    [Parameter(Mandatory = $true)][scriptblock]$Look,
    [Parameter(Mandatory = $true)][int]$WithinMs,
    [ValidateRange(0, 250)][int]$PauseMs = 0
  )
  $wfClock = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    $wfSaw = & $Look
    if ($wfSaw.Holds) { return @{ Outcome = 'held'; Seen = $wfSaw.Seen; ElapsedMs = $wfClock.ElapsedMilliseconds } }
    if ($wfSaw.Gone) { return @{ Outcome = 'gone'; Seen = $wfSaw.Seen; ElapsedMs = $wfClock.ElapsedMilliseconds } }
    $wfLeft = $WithinMs - $wfClock.ElapsedMilliseconds
    if ($wfLeft -le 0) { return @{ Outcome = 'bound'; Seen = $wfSaw.Seen; ElapsedMs = $wfClock.ElapsedMilliseconds } }
    if ($PauseMs -gt 0) { [Threading.Thread]::Sleep([int][Math]::Min($PauseMs, $wfLeft)) }
  }
}

# **W4's wait: the first FolioUpdate-* value under the Run key**, on the open key `$RunKey`
# (anything with GetValueNames()), spinning (W4's window is a few milliseconds: the value is
# written, flushed and read back, then Armed is renamed in). `$EntrySig` gives journal.json's
# directory entry without opening the file: a change while no Run value is there is the
# applier recording something other than Armed (Abandoned, a revert), and W4's state is gone.
function Wait-RunValue {
  param(
    [Parameter(Mandatory = $true)]$RunKey,
    [Parameter(Mandatory = $true)][scriptblock]$EntrySig,
    [int]$WithinMs = $W4RunWithinMs
  )
  $rvFirst = & $EntrySig
  return Wait-Fact -WithinMs $WithinMs -PauseMs 0 -Look {
    $rvNames = @($RunKey.GetValueNames() | Where-Object { $_ -like 'FolioUpdate-*' })
    if ($rvNames.Count -gt 0) { return @{ Holds = $true; Gone = $false; Seen = "run=[$($rvNames -join ';')]" } }
    $rvSig = & $EntrySig
    if ($rvSig -ne $rvFirst) { return @{ Holds = $false; Gone = $true; Seen = "run=[] journal entry $rvFirst -> $rvSig" } }
    return @{ Holds = $false; Gone = $false; Seen = "run=[] journal entry $rvSig" }
  }
}

# **End `$Process` and wait on its handle for it to leave, within `$WithinMs`.** `$Kill` ends it
# (the watcher's is `{ param($p) $p.Kill() }`: TerminateProcess on the handle). True when the
# process has exited within the bound, false when it is still running at the end of it.
function Stop-Within {
  param(
    [Parameter(Mandatory = $true)][Diagnostics.Process]$Process,
    [Parameter(Mandatory = $true)][scriptblock]$Kill,
    [int]$WithinMs = $KillWithinMs
  )
  # Kill throws InvalidOperationException for a process that has already exited and
  # Win32Exception (access denied) for one already terminating: in both, the handle's wait
  # below is the answer.
  try { & $Kill $Process }
  catch [InvalidOperationException] { }
  catch [ComponentModel.Win32Exception] { }
  return $Process.WaitForExit($WithinMs)
}
