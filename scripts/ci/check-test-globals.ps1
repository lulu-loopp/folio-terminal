# the_test_globals_register_grows_only_by_rows_the_guard_checks
#
# `docs/plans/TEST-GLOBALS.tsv` names every process-global that two tests of
# one binary reach, or that a test writes, with the class that makes it safe
# (T-TEST-PROCESS-GLOBALS, 2026-10-24). bt-source's `test_globals` test is the
# discovery half: it holds the tree to the register both ways and checks the
# classes it can check. This script is the history half: it compares the rows
# with the merge base's, keyed by file, global and binary.
#
# **An added row is admitted when its class is one the guard checks itself**
# (`lock:<NAME>`, `single writer`, `one main`, `run=alone`, `run=serial`).
# Every row needs a reason dated `YYYY-MM-DD: why`, so an admitted row carries
# its date too. The check is the proof: a new test that shares a global under
# one lock, or a target without the harness, is safe by a fact the guard
# holds on every run.
#
# **An added row of a by-hand class** (`per-test key`, `monotone`,
# `write-once`, `not on a test thread`, `serial helpers`) is a reason nobody
# re-checks. It is refused unless a widening line admits it, as in the timing
# list: a comment line `# widened <yyyy-mm-dd> by <ticket> (+<n> rows): <what>`,
# new against the merge base, admits exactly <n> added by-hand rows. More than
# one new widening line, a count that is not the number of by-hand rows added,
# a widening of `+0` rows, or a widening line in a change that leaves the scan
# (`crates/bt-source/tests/test_globals.rs`) as the base has it, is refused.
# A row whose class changed counts as added under its new class. A removed row
# is always admitted: the register shrinks freely.
#
# Neither half sees a global a test reaches only through product functions:
# that is the guard's stated blind spot, printed as a measure
# (`TEST_GLOBALS_PRODUCT_REACH`), not a gate.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "docs/plans/TEST-GLOBALS.tsv",
    [string]$MainRef = "origin/main",
    [string]$Scan = "crates/bt-source/tests/test_globals.rs"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$checked = @("single writer", "one main", "run=alone", "run=serial")
$byHand = @("per-test key", "monotone", "write-once", "not on a test thread", "serial helpers")

$list = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $list)) {
    throw "$Relative is not in the tree - it is the test process-globals register"
}

function Test-Checked([string]$Class) {
    return ($Class -match '^lock:\S+$') -or ($checked -contains $Class)
}

function Read-Widenings([string[]]$Lines) {
    $found = @()
    foreach ($line in $Lines) {
        if ($line -match '^# widened (\d{4}-\d{2}-\d{2}) by (\S+) \(\+(\d+) rows?\)') {
            $found += [pscustomobject]@{ Line = $line; Count = [int]$Matches[3] }
        }
    }
    return , $found
}

function Read-GlobalRows([string[]]$Lines) {
    $rows = @{}
    $header = $false
    foreach ($line in $Lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne "file`tglobal`tbinary`tclass`treason") {
                throw "$Relative has the wrong column header: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 5 -or @($columns | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
            throw "$Relative row is not five non-empty columns: $line"
        }
        $class = $columns[3]
        if (-not (Test-Checked $class) -and ($byHand -notcontains $class)) {
            throw "$Relative row has no class '$class': $line"
        }
        if ($columns[4] -notmatch '^\d{4}-\d{2}-\d{2}: \S') {
            throw "$Relative row's reason is not dated (YYYY-MM-DD: why): $line"
        }
        $key = "$($columns[0])`t$($columns[1])`t$($columns[2])"
        if ($rows.ContainsKey($key)) {
            throw "$Relative names $key more than once"
        }
        $rows[$key] = [pscustomobject]@{ Line = $line; Class = $class }
    }
    if (-not $header) { throw "$Relative has no column header" }
    return $rows
}

Push-Location $Repo
try {
    $lines = [IO.File]::ReadAllLines($list)
    $now = Read-GlobalRows $lines
    $widenedNow = Read-Widenings $lines
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the test-globals comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $status = $LASTEXITCODE
    $base = if ($status -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the test-globals comparison did not run"
        exit 2
    }
    $text = (& git show "${base}:${Relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$Relative is not in $base - no base register exists; the register has $($now.Count) rows. PASS"
        exit 0
    }
    $baseLines = $text -split "`r?`n"
    $before = Read-GlobalRows $baseLines
    $widenedBefore = @((Read-Widenings $baseLines) | ForEach-Object { $_.Line })
    & git diff --quiet $base HEAD -- $Scan 2>$null
    $scanChanged = $LASTEXITCODE -ne 0
} finally {
    Pop-Location
}

$added = @()
foreach ($key in ($now.Keys | Sort-Object)) {
    $row = $now[$key]
    if ($before.ContainsKey($key) -and $before[$key].Class -eq $row.Class) { continue }
    $added += $row
}
$removed = @($before.Keys | Where-Object { -not $now.ContainsKey($_) }).Count
$checkedAdded = @($added | Where-Object { Test-Checked $_.Class })
$handAdded = @($added | Where-Object { -not (Test-Checked $_.Class) })
Write-Host "$Relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added ($($checkedAdded.Count) of a checked class, $($handAdded.Count) by hand), $removed removed."
foreach ($row in $checkedAdded) {
    Write-Host "  admitted, its class is checked by the guard: $($row.Line)"
}

$widenings = @($widenedNow | Where-Object { $widenedBefore -notcontains $_.Line })
if ($widenings.Count -gt 1) {
    throw ("$($widenings.Count) widening lines are new against the base, and one change widens the scan once:" +
        [Environment]::NewLine + (($widenings | ForEach-Object { "    $($_.Line)" }) -join [Environment]::NewLine))
}
if ($widenings.Count -eq 1) {
    $widening = $widenings[0]
    if ($widening.Count -eq 0) {
        throw ("empty widening: the line admits no rows, and a widening that admits nothing is not one:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    if (-not $scanChanged) {
        throw ("unchanged scan: the widening line is new, and $Scan is as $($base.Substring(0, 12)) has it - a widening is a change to the scan's grammar:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    if ($widening.Count -ne $handAdded.Count) {
        throw ("the widening line admits $($widening.Count) by-hand row(s) and $($handAdded.Count) were added:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    Write-Host "the widening admits its $($handAdded.Count) by-hand row(s): $($widening.Line)"
    Write-Host "test-globals register grew by checked rows and exactly its widening. PASS"
    exit 0
}
if ($handAdded.Count -gt 0) {
    $details = ($handAdded | ForEach-Object { "    $($_.Line)" }) -join [Environment]::NewLine
    throw ("$($handAdded.Count) row(s) of a by-hand class were added, and a by-hand row only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Make the state the test's own, take one lock in every test that reaches it (lock:<NAME>), or widen the scan.")
}

Write-Host "test-globals register grew only by rows the guard checks. PASS"
exit 0
