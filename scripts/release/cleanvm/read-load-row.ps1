<#
.SYNOPSIS
    The cells of the load row (clean-vm.md 4.6), read from what `run-load-row.ps1` copied out
    of the guest.

.DESCRIPTION
    `load-<Tag>.txt` holds, at each phase boundary, the UTC instant and the byte lengths of
    diagnostics.log and perf.txt; a phase is the bytes between two boundaries. Per phase:

      turns/s          the budget lines' `turn=` counter, last less first, over the phase's
                       seconds (a lower bound: only turns that wrote a line are seen)
      stale turns      `stale us=` lines plus the turns they say were suppressed
      stale max/source the largest `stale us=` and every `source=` the stale lines named
      delay max        the largest `delay_us=` of the wall lines
      presents         `BT_PERF_TRACE present` lines; the median, 95th percentile and largest
                       `since_previous_us` (the frame cadence) and `event_to_present_us` per
                       `source=` (the echo of a key, a wheel notch, the shell's output)

    Writes cells-<Tag>.txt beside the inputs and prints it.
#>
param(
    [Parameter(Mandatory)] [string] $Results,
    [Parameter(Mandatory)] [string] $Tag
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$log = Join-Path $Results "load-$Tag.txt"
$diagnostics = Join-Path $Results "load-$Tag-diagnostics.log"
$perf = Join-Path $Results "load-$Tag-perf.txt"
$marks = @(Get-Content -LiteralPath $log | ForEach-Object {
        if ($_ -match '^(\S+) mark (\w+) diagnostics=(\d+) perf=(\d+)$') {
            [pscustomobject]@{
                At = [DateTime]::Parse($Matches[1]).ToUniversalTime(); Name = $Matches[2]
                Diagnostics = [long]$Matches[3]; Perf = [long]$Matches[4]
            }
        }
    })
# A row whose driver never got the keyboard ran no phase: it says so, and its close is still a cell.
$refused = @(Get-Content -LiteralPath $log | Where-Object { $_ -match 'FAILED foreground' })
if ($refused.Count) {
    $closing = @(Get-Content -LiteralPath $log | Where-Object { $_ -match 'WM_CLOSE|close ok|FAILED close|still running|hang report' })
    $lines = @("load row '$Tag' — $Results", ('phases    NOT RUN: ' + $refused[0].Substring(25)),
        ('close     ' + $(if ($closing.Count) { ($closing | ForEach-Object { $_.Substring(25) }) -join ' | ' } else { 'not recorded' })))
    $lines | Set-Content -LiteralPath (Join-Path $Results "cells-$Tag.txt") -Encoding utf8
    $lines | ForEach-Object { Write-Host $_ }
    return
}
if ($marks.Count -lt 4) { throw "$log has $($marks.Count) phase marks; the row did not finish" }

function Slice([string] $Path, [long] $From, [long] $To) {
    if (-not (Test-Path -LiteralPath $Path)) { return @() }
    $bytes = [IO.File]::ReadAllBytes($Path)
    $To = [Math]::Min($To, $bytes.Length)
    if ($To -le $From) { return @() }
    return [Text.Encoding]::UTF8.GetString($bytes, [int]$From, [int]($To - $From)) -split "`r?`n"
}
function Quantiles([double[]] $Values) {
    if ($Values.Count -eq 0) { return '-' }
    $sorted = @($Values | Sort-Object)
    $at = { param($q) $sorted[[Math]::Min($sorted.Count - 1, [int][Math]::Floor($q * $sorted.Count))] }
    return 'n={0} p50={1} p95={2} max={3}' -f $sorted.Count, (& $at 0.5), (& $at 0.95), $sorted[-1]
}

$out = New-Object System.Collections.Generic.List[string]
$out.Add("load row '$Tag' — $Results")
for ($i = 0; $i -lt 3; $i++) {
    $from = $marks[$i]; $to = $marks[$i + 1]
    $seconds = ($to.At - $from.At).TotalSeconds
    $budget = @(Slice $diagnostics $from.Diagnostics $to.Diagnostics | Where-Object { $_ -like 'Folio budget: turn=*' })
    $turns = @($budget | ForEach-Object { if ($_ -match 'turn=(\d+)') { [long]$Matches[1] } })
    $rate = if ($turns.Count -ge 2 -and $seconds -gt 0) { [Math]::Round((($turns | Measure-Object -Maximum).Maximum - ($turns | Measure-Object -Minimum).Minimum) / $seconds) } else { 0 }
    $stale = @($budget | Where-Object { $_ -match ' stale us=' })
    $staleTurns = 0; $staleMax = 0; $sources = @{}
    foreach ($line in $stale) {
        if ($line -match ' stale us=(\d+) source=(\S+) suppressed=(\d+)') {
            $staleTurns += 1 + [long]$Matches[3]
            $staleMax = [Math]::Max($staleMax, [long]$Matches[1])
            $sources[$Matches[2]] = $true
        }
    }
    $delays = @($budget | ForEach-Object { if ($_ -match 'delay_us=(\d+)') { [long]$Matches[1] } })
    $delayMax = if ($delays.Count) { ($delays | Measure-Object -Maximum).Maximum } else { 0 }
    $presents = @(Slice $perf $from.Perf $to.Perf | Where-Object { $_ -like 'BT_PERF_TRACE present *' })
    $cadence = [double[]]@($presents | ForEach-Object { if ($_ -match 'since_previous_us=(\d+)') { [double]$Matches[1] } })
    $out.Add(('{0,-9} {1,5:N0} s  turns/s>={2}  stale turns={3} max_us={4} sources={5}  delay max_us={6}' -f `
                $from.Name, $seconds, $rate, $staleTurns, $staleMax, ((@($sources.Keys) | Sort-Object) -join ',' | ForEach-Object { if ($_) { $_ } else { '-' } }), $delayMax))
    $out.Add(('          presents {0}  cadence(since_previous_us) {1}' -f $presents.Count, (Quantiles $cadence)))
    # **Whether the phase was exercised at all** (2026-10-21: two runs whose driver was refused
    # the foreground measured one present in three minutes). The driver says how many of its
    # cycles reached Folio; a phase with none is marked, so its cells are not read as a result.
    $keyboard = @($presents | Where-Object { $_ -match ' source=Keyboard ' }).Count
    $seen = @(Slice $perf $from.Perf $to.Perf | Where-Object { $_ -match 'BT_PERF_TRACE file_reads .*input_seen=true' }).Count
    $cycleLine = @(Get-Content -LiteralPath $log | Where-Object { $_ -match ' cycles \d+' })[$i]
    if ($cycleLine -and $cycleLine -match 'sent ([1-9]\d*)' -and $keyboard -eq 0) {
        $out.Add(("          KEYS NOT SEEN BY FOLIO: {0} cycles sent, 0 keyboard presents, {1} minute(s) with input_seen=true" -f $Matches[1], $seen))
    }
    if ($cycleLine -and $cycleLine -match 'cycles (\d+) sent (\d+)') {
        $out.Add(('          cycles {0}, {1} reached Folio{2}' -f $Matches[1], $Matches[2], $(if ([int]$Matches[2] -eq 0) { '  NOT EXERCISED' } else { '' })))
    } elseif ($presents.Count -lt 10) {
        $out.Add('          NOT EXERCISED (fewer than 10 presents; this run did not count its cycles)')
    }
    $bySource = @($presents | Group-Object { if ($_ -match ' source=(\S+)') { $Matches[1] } else { '?' } })
    foreach ($group in $bySource) {
        $latency = [double[]]@($group.Group | ForEach-Object { if ($_ -match 'event_to_present_us=(\d+)') { [double]$Matches[1] } })
        $out.Add(('          source={0,-12} event_to_present_us {1}' -f $group.Name, (Quantiles $latency)))
    }
}
# The close: Folio exiting on the OS's close is a cell of the row, and a Folio still running
# 30 s later is its failure (load-row.ps1 says which, and what was still standing).
$closeLines = @(Get-Content -LiteralPath $log | Where-Object { $_ -match 'WM_CLOSE|close ok|FAILED close|still running|hang report' })
$out.Add('close     ' + $(if ($closeLines.Count) { ($closeLines | ForEach-Object { $_.Substring(25) }) -join ' | ' } else { 'not recorded (a run before the close cell)' }))
$cells = Join-Path $Results "cells-$Tag.txt"
$out | Set-Content -LiteralPath $cells -Encoding utf8
$out | ForEach-Object { Write-Host $_ }
