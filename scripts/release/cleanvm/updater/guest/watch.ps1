<#
.SYNOPSIS
    Guest side of the updater rows (clean-vm.md 4.4): watch the journal, freeze
    Folio at a row's state, write the marker the host cuts the power on; or hold
    a file open for the rows that test a refused write.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1, started by
    `updater/run-row.ps1` with `-noWait` in the logged-on session.

    **The journal is `H\journal.json`** (`update_txn.rs` `Home::journal`), where
    H is `<install>\.folio-update`; a transaction's files are `H\<txn>\...`.

    **The journal is opened only when its directory entry changed, and closed at
    once.** An open handle on `journal.json` makes the writer's rename over it
    fail with Access is denied: a watcher that read it every 25 ms made the
    applier fail at Armed -> Moving (U-31, W4/W12, H-6). The entry (time, size,
    creation) is read with FindFirstFile, which opens nothing. `-StopReadingAt`
    stops opening it at all once that phase has been read; `-NoRead` never
    opens it.

    **At the instant a row's condition holds, every folio.exe is suspended**
    (NtSuspendProcess), a flushed record of what was seen is written, then the
    marker. The host cuts the power one to five seconds later, so a state that
    lasts milliseconds is the state at the cut. The caveat: the guest's lazy
    writer runs in those seconds, so a flush the product omits is hidden, never
    exposed. Rows whose state lasts milliseconds spin without sleeping once the
    phase before them is read.

    The rows without a cut use the holds: `-HoldJournalOnRun` (W14: the journal
    opened for reading without delete or write sharing the moment the Run value
    appears), `-HoldRescueAt` (W15: the rescue copy held with no execute or
    delete sharing), `-HoldPath` (E-7, W10: a file of the install held with no
    sharing). `-KillNewInstallProcess` ends the first new folio.exe of the
    install folder after the one running at the start: the trial the applier
    started (the rollback rows).

    Writes, in -GuestHome: updater-watch-<Tag>.txt (the log),
    updater-journal-<Tag>.txt (every version of the journal),
    updater-atmarker-<Tag>.txt and updater-marker-<Row>.txt.
#>
param(
  [string]$Row = '',
  [switch]$Freeze,
  [int]$PollMs = 25,
  [int]$TimeoutSeconds = 900,
  # a file of the install folder, relative to it ('*' allowed: the first match), held with no sharing
  [string]$HoldPath = '',
  # the phase at which -HoldPath is taken ('*' = as soon as the file exists)
  [string]$HoldAt = '',
  [int]$HoldSeconds = 0,
  [string]$Tag = 'run',
  [string]$GuestHome = 'C:\folio-vm',
  [switch]$NoRead,
  [switch]$KillNewInstallProcess,
  [string]$StopReadingAt = '',
  [int]$HoldJournalOnRun = 0,
  [string]$HoldRescueAt = ''
)
$ErrorActionPreference = 'Continue'
$install = Join-Path $GuestHome 'folio'
$H = Join-Path $install '.folio-update'
$journalPath = Join-Path $H 'journal.json'
$log = Join-Path $GuestHome "updater-watch-$Tag.txt"
$hist = Join-Path $GuestHome "updater-journal-$Tag.txt"
$marker = if ($Row) { Join-Path $GuestHome "updater-marker-$Row.txt" } else { $null }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class UpdaterWatch {
  [DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  public static int Suspend(int pid) {
    IntPtr h = OpenProcess(0x0800, false, pid);
    if (h == IntPtr.Zero) return -1;
    int r = NtSuspendProcess(h); CloseHandle(h); return r;
  }
}
'@

function Now { (Get-Date).ToString('HH:mm:ss.fff') }
$buf = New-Object System.Collections.Generic.List[string]
function Say($s) { $line = "$(Now) $s"; [void]$buf.Add($line); Add-Content -LiteralPath $log -Value $line -Encoding UTF8 }
# Written once, at the marker, with FlushFileBuffers: the hard stop that follows drops whatever
# the lazy writer has not written, and this record is the evidence of the instant.
function Write-Flushed($path, $text) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($text)
  $fs = New-Object IO.FileStream($path, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::Read)
  try { $fs.Write($bytes, 0, $bytes.Length); $fs.Flush($true) } finally { $fs.Dispose() }
}
function Read-Journal {
  try {
    $fs = [IO.File]::Open($journalPath, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
    try { $sr = New-Object IO.StreamReader($fs); return $sr.ReadToEnd() } finally { $fs.Dispose() }
  } catch { return $null }
}
function Phase-Of($text) {
  if (-not $text) { return $null }
  $m = [regex]::Match($text, '"body"\s*:\s*\{\s*"phase"\s*:\s*\{\s*"phase"\s*:\s*"(\w+)"')
  if ($m.Success) { return $m.Groups[1].Value }
  return '?'
}
function Run-Values {
  try {
    $k = Get-ItemProperty -LiteralPath 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' -ErrorAction Stop
    return @($k.PSObject.Properties | Where-Object { $_.Name -like 'FolioUpdate-*' } | ForEach-Object { "$($_.Name)=$($_.Value)" })
  } catch { return @() }
}
function Receipts { @(Get-ChildItem -LiteralPath $H -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Filter 'health-*' -File -ErrorAction SilentlyContinue } | ForEach-Object { $_.Name }) }
function Count-In($sub) { @(Get-ChildItem -LiteralPath $H -Directory -ErrorAction SilentlyContinue | ForEach-Object { Get-ChildItem -LiteralPath (Join-Path $_.FullName $sub) -File -ErrorAction SilentlyContinue }).Count }
function Folios { @(Get-CimInstance Win32_Process -Filter "Name='folio.exe'" -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProcessId):$($_.ExecutablePath) $($_.CommandLine)" }) }
function Freeze-All($first) {
  $frozen = @()
  foreach ($i in @($first)) { if ($i) { $frozen += "$i=$([UpdaterWatch]::Suspend($i))" } }
  foreach ($p in @(Get-Process folio -ErrorAction SilentlyContinue)) {
    if (@($first) -notcontains $p.Id) { $frozen += "$($p.Id)=$([UpdaterWatch]::Suspend($p.Id))" }
  }
  return $frozen
}
# The directory entry of journal.json, read without opening the file.
function Dir-Sig {
  try { $f = (New-Object IO.DirectoryInfo($H)).GetFiles('journal.json') } catch { return 'nodir' }
  if ($f.Count -eq 0) { return 'absent' }
  return "$($f[0].LastWriteTimeUtc.Ticks):$($f[0].Length):$($f[0].CreationTimeUtc.Ticks)"
}

# The W table's states (self-update-2026-09-16.md (b).2): the phase, and for W3/W4 the Run value,
# for W7/W8 the receipt.
function Reached($phase, $run, $rcpt) {
  switch ($Row) {
    'W1' { return $phase -eq 'Allocated' }
    'W2' { return $phase -eq 'Prepared' }
    'W3' { return ($phase -eq 'Handoff' -and $run.Count -eq 0) }
    'W4' { return ($phase -eq 'Handoff' -and $run.Count -gt 0) }
    'W5' { return $phase -eq 'Armed' }
    'W6' { return $phase -eq 'Moving' }
    'W7' { return ($phase -eq 'Trial' -and $rcpt.Count -eq 0) }
    'W8' { return ($phase -eq 'Trial' -and $rcpt.Count -gt 0) }
    'W9' { return $phase -eq 'RollbackIntent' }
    'W10' { return $phase -eq 'Stuck' }
    'W11' { return $phase -eq 'RolledBack' }
    'W12' { return $phase -eq 'Committed' }
    'W13' { return $phase -eq 'Abandoned' }
    default { return $false }
  }
}
# Phases that mean a row's state has already gone by.
$beyond = @{
  'W3' = @('Armed', 'Moving', 'Trial', 'Committed', 'RollbackIntent', 'Retired'); 'W4' = @('Armed', 'Moving', 'Trial', 'Committed', 'RollbackIntent', 'Retired')
  'W5' = @('Moving', 'Trial', 'Committed', 'RollbackIntent', 'Retired'); 'W6' = @('Trial', 'Committed', 'RollbackIntent', 'Retired')
  'W7' = @('Committed', 'RollbackIntent', 'Retired'); 'W8' = @('Committed', 'RollbackIntent', 'Retired')
}
# Rows whose state lasts milliseconds: once one of the phases before it is read, spin with no
# sleep on the directory entry (W8: on the receipts) and freeze the moment the target shows.
$spins = @{
  'W13' = @(@('Allocated'), 'Abandoned'); 'W5' = @(@('Handoff'), 'Armed'); 'W7' = @(@('Handoff', 'Armed', 'Moving'), 'Trial')
  'W6' = @(@('Handoff', 'Armed'), 'Moving'); 'W8' = @(@('Moving', 'Trial'), 'receipt'); 'W9' = @(@('Armed', 'Moving', 'Trial'), 'RollbackIntent')
  'W11' = @(@('Armed', 'Moving', 'Trial', 'RollbackIntent'), 'RolledBack'); 'W12' = @(@('Moving', 'Trial'), 'Committed')
}

$exePath = Join-Path $install 'folio.exe'
$known = @{}
foreach ($p in @(Get-Process folio -ErrorAction SilentlyContinue)) { $known[$p.Id] = $true }
# The trial is the first NEW folio.exe of the install after the processes of the start (O):
# Get-Process, not CIM (a CIM query took ~6 s here and the receipt came first).
function Kill-NewInstallProcess {
  foreach ($p in @(Get-Process folio -ErrorAction SilentlyContinue)) {
    if ($known.ContainsKey($p.Id)) { continue }
    $known[$p.Id] = $true
    $pp = $null; try { $pp = $p.Path } catch { }
    if ($pp -eq $exePath -and (Test-Path -LiteralPath $H)) {
      Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
      Say "new install process pid=$($p.Id) ended by the watcher (Stop-Process): the trial"
      return $true
    }
    Say "new folio process pid=$($p.Id) path=$pp (left alone)"
  }
  return $false
}

Say "watch start row=$Row freeze=$Freeze hold=$HoldPath@$HoldAt/$HoldSeconds journalHold=$HoldJournalOnRun rescueHold=$HoldRescueAt@$HoldSeconds noRead=$NoRead stopReadingAt=$StopReadingAt poll=${PollMs}ms"
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$lastText = $null; $lastRun = ''; $lastRcpt = ''
$seenJournal = $false; $done = $false
$hold = $null; $holdUntil = $null; $jhold = $null; $jholdUntil = $null; $rhold = $null; $rholdUntil = $null
$lastSig = ''; $text = $null; $phase = $null
$newKilled = $false
while ((Get-Date) -lt $deadline -and -not $done) {
  $sig = Dir-Sig
  if ($StopReadingAt -and -not $NoRead -and $phase -eq $StopReadingAt) { $NoRead = $true; Say "stop reading the journal (phase $phase seen)" }
  if ($NoRead -and $sig -ne $lastSig) { Say "journal entry sig=$sig"; $lastSig = $sig }
  elseif ($sig -ne $lastSig) {
    if ($sig -eq 'absent' -or $sig -eq 'nodir') { $text = $null; $lastSig = $sig }
    else {
      # A read refused mid-rename is not an absent journal: never decide on the old text.
      $got = Read-Journal
      if ($null -ne $got) { $text = $got; $lastSig = $sig }
      else { Start-Sleep -Milliseconds 5; continue }
    }
  }
  $phase = Phase-Of $text
  $run = Run-Values
  $rcpt = Receipts
  if ($text -ne $lastText) {
    if ($text) { $seenJournal = $true; Say "journal phase=$phase (backup=$(Count-In 'backup') set=$(Count-In 'set') rolledout=$(Count-In 'rolledout'))"; Add-Content -LiteralPath $hist -Value "$(Now) $text" -Encoding UTF8 }
    else { Say "journal absent"; Add-Content -LiteralPath $hist -Value "$(Now) <absent>" -Encoding UTF8 }
    $lastText = $text
  }
  $rs = ($run -join ';'); if ($rs -ne $lastRun) { Say "run=[$rs]"; $lastRun = $rs }
  $cs = ($rcpt -join ';'); if ($cs -ne $lastRcpt) { Say "receipts=[$cs]"; $lastRcpt = $cs }

  # W4's window (Run value written, flushed, read back; Armed not yet renamed in) is a few
  # milliseconds: once Handoff is read, spin on the Run key alone (one open key, no journal read,
  # no sleep), freeze on the first FolioUpdate-* value, and only then read the journal.
  if ($Row -eq 'W4' -and $phase -eq 'Handoff' -and $run.Count -eq 0) {
    $rk = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('SOFTWARE\Microsoft\Windows\CurrentVersion\Run')
    $spinUntil = (Get-Date).AddSeconds(120)
    while ((Get-Date) -lt $spinUntil) {
      if (@($rk.GetValueNames() | Where-Object { $_ -like 'FolioUpdate-*' }).Count -gt 0) {
        if ($Freeze) { [void](Freeze-All @()) }
        break
      }
    }
    $rk.Dispose()
    $text = Read-Journal; $phase = Phase-Of $text; $run = Run-Values; $lastSig = Dir-Sig
    Say "W4 spin ended: phase=$phase run=[$($run -join ';')]"
  }
  if ($Row -and $spins.ContainsKey($Row) -and ($spins[$Row][0] -contains $phase)) {
    $starts = $spins[$Row][0]; $target = $spins[$Row][1]
    $spinUntil = (Get-Date).AddSeconds(180)
    # the processes that exist now (O, the applier) are suspended first, from this list, so the
    # freeze does not wait for a process enumeration
    $pre = @(Get-Process folio -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    while ((Get-Date) -lt $spinUntil) {
      if ($KillNewInstallProcess -and -not $newKilled) { $newKilled = Kill-NewInstallProcess }
      if ($target -eq 'receipt') {
        if ((Receipts).Count -gt 0) { if ($Freeze) { [void](Freeze-All $pre) }; break }
      }
      $sg = Dir-Sig
      if ($sg -ne $lastSig) {
        if ($sg -eq 'absent') { $lastSig = $sg; $text = $null; break }
        $t2 = Read-Journal
        if ($null -eq $t2) { continue }
        $lastSig = $sg; $text = $t2; $p2 = Phase-Of $t2
        if ($p2 -eq $target) { if ($Freeze) { [void](Freeze-All $pre) }; break }
        if (-not ($starts -contains $p2)) { break }
      }
    }
    $phase = Phase-Of $text; $run = Run-Values; $rcpt = Receipts
    Say "$Row spin ended: phase=$phase run=[$($run -join ';')] receipts=[$($rcpt -join ';')]"
  }

  if ($Row -and (Reached $phase $run $rcpt)) {
    $frozen = @()
    if ($Freeze) { $frozen = Freeze-All @() }
    $again = Phase-Of (Read-Journal)
    Say "ROW $Row REACHED phase=$phase (after freeze: $again) run=[$($run -join ';')] receipts=[$($rcpt -join ';')] backup=$(Count-In 'backup') set=$(Count-In 'set') frozen=[$($frozen -join ',')]"
    Add-Content -LiteralPath $hist -Value "$(Now) AT-MARKER $(Read-Journal)" -Encoding UTF8
    Say "procs=$((Folios) -join ' | ')"
    $listing = @(Get-ChildItem -LiteralPath $install -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object { "{0,12} {1}" -f $(if ($_.PSIsContainer) { '<dir>' } else { $_.Length }), $_.FullName })
    $record = @('== watch log') + $buf + @('', '== journal at the marker', (Read-Journal), '', '== install folder at the marker') + $listing
    Write-Flushed (Join-Path $GuestHome "updater-atmarker-$Tag.txt") (($record -join "`r`n") + "`r`n")
    Write-Flushed $marker "row=$Row phase=$phase at=$(Now)`r`n"
    $done = $true; break
  }
  $missed = $null
  if ($Row -and $seenJournal -and ($phase -eq 'Retired' -or -not $text -or ($Row -ne 'W13' -and $phase -eq 'Abandoned'))) { $missed = "phase now $(if ($text) { $phase } else { '<absent>' })" }
  elseif ($Row -and $beyond.ContainsKey($Row) -and ($beyond[$Row] -contains $phase)) { $missed = "phase already $phase" }
  if ($missed) {
    Say "ROW $Row MISSED: $missed"
    Write-Flushed (Join-Path $GuestHome "updater-atmarker-$Tag.txt") ((@('== watch log (row missed)') + $buf) -join "`r`n")
    Write-Flushed $marker "row=$Row phase=MISSED at=$(Now)`r`n"
    $done = $true; break
  }

  if ($KillNewInstallProcess -and -not $newKilled) { $newKilled = Kill-NewInstallProcess }
  if ($HoldJournalOnRun -gt 0 -and -not $jhold -and $run.Count -gt 0) {
    try {
      $jhold = [IO.File]::Open($journalPath, 'Open', 'Read', 'Read')
      $jholdUntil = (Get-Date).AddSeconds($HoldJournalOnRun)
      Say "JOURNAL HOLD taken (Read, share Read: no delete, no write) for $HoldJournalOnRun s"
    } catch { Say "JOURNAL HOLD failed: $($_.Exception.Message)"; $HoldJournalOnRun = 0 }
  }
  if ($jhold -and (Get-Date) -gt $jholdUntil) { $jhold.Dispose(); $jhold = $null; $HoldJournalOnRun = 0; Say 'JOURNAL HOLD released' }
  if ($HoldRescueAt -and -not $rhold -and $phase -eq $HoldRescueAt) {
    $rx = @(Get-ChildItem -LiteralPath $H -Directory -ErrorAction SilentlyContinue | ForEach-Object { Join-Path (Join-Path $_.FullName 'rescue') 'folio.exe' } | Where-Object { Test-Path -LiteralPath $_ })
    if ($rx.Count -gt 0) {
      try {
        $rhold = [IO.File]::Open($rx[0], 'Open', 'Read', 'None')
        $rholdUntil = (Get-Date).AddSeconds($HoldSeconds)
        Say "RESCUE HOLD taken on $($rx[0]) (Read, share None) for $HoldSeconds s"
      } catch { Say "RESCUE HOLD failed: $($_.Exception.Message)"; $HoldRescueAt = '' }
    }
  }
  if ($rhold -and (Get-Date) -gt $rholdUntil) { $rhold.Dispose(); $rhold = $null; $HoldRescueAt = ''; Say 'RESCUE HOLD released' }
  if ($HoldPath -and -not $hold -and ($HoldAt -eq '*' -or $phase -eq $HoldAt)) {
    try {
      $hp = Join-Path $install $HoldPath
      if ($hp.Contains('*')) { $m1 = @(Get-Item -Path $hp -ErrorAction SilentlyContinue); if ($m1.Count -eq 0) { throw 'no match yet' }; $hp = $m1[0].FullName }
      $hold = [IO.File]::Open($hp, 'Open', 'Read', 'None')
      Say "HOLD taken on $hp (Read, share None) at phase=$phase"
      if ($HoldSeconds -gt 0) { $holdUntil = (Get-Date).AddSeconds($HoldSeconds) }
    } catch { if ($_.Exception.Message -notlike '*no match yet*') { Say "HOLD failed on ${HoldPath}: $($_.Exception.Message)"; $HoldPath = '' } }
  }
  if ($hold -and $holdUntil -and (Get-Date) -gt $holdUntil) { $hold.Dispose(); $hold = $null; $holdUntil = $null; $HoldPath = ''; Say 'HOLD released' }
  Start-Sleep -Milliseconds $PollMs
}
if ($hold) { $hold.Dispose(); Say 'HOLD released at exit' }
if ($jhold) { $jhold.Dispose(); Say 'JOURNAL HOLD released at exit' }
if ($rhold) { $rhold.Dispose(); Say 'RESCUE HOLD released at exit' }
if ($Row -and -not $done) { Set-Content -LiteralPath $marker -Value "row=$Row phase=NOT_REACHED at=$(Now)" -Encoding UTF8; Say "ROW $Row NOT_REACHED (timeout)" }
Say 'watch end'
