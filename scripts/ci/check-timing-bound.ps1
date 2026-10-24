# the_timing_bound_test_list_only_shrinks
#
# `docs/plans/TIMING-BOUND-TESTS.tsv` names every `#[test]` function whose own
# body waits on the real clock through a stable std wait (`sleep`, `recv_timeout`,
# `wait_timeout`, `park_timeout` and their variants; the list is in bt-source's
# `timing` test, which also refuses a workspace function by one of those names) or
# measures it (`elapsed`). Existing rows are migration work; a new row is a new
# flaky-test liability. This script is the history half: it compares with the
# merge base, whole rows and multiplicity included, so the list can only
# shrink. The discovery half is bt-source's `timing` test
# (`the_timing_bound_tests_are_exactly_the_listed_ones`), which holds the tree
# to the list both ways, so a new timing-bound test is red there if it is not
# listed and red here if it is.
#
# Neither half sees a clock a test reaches only through a product function it
# calls (a product function handed a deadline): review owns discovering those.
#
# **A widening admits exactly its rows** (T-TIMING-BOUND-STRAYS, 2026-10-24).
# When the scan's grammar is widened (clock arithmetic in a test's own body, a
# budget measured in a test-only helper), the tests it newly sees are rows the
# tree already had, not new liabilities. A comment line
# `# widened <yyyy-mm-dd> by <ticket> (+<n> rows): <what>` that is new against
# the merge base admits exactly <n> added rows; more than one new widening line,
# a count that is not the number of rows added, or a widening line with no rows
# added is refused, and rows added with no new widening line are refused as
# before. Whether the admitted rows are the widened grammar's is the discovery
# half's to hold: it lists exactly what the tree implies.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "docs/plans/TIMING-BOUND-TESTS.tsv",
    [string]$MainRef = "origin/main"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$list = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $list)) {
    throw "$Relative is not in the tree - it is the timing-bound test census"
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

function Read-TimingRows([string[]]$Lines) {
    $rows = @()
    $header = $false
    $tests = @{}
    foreach ($line in $Lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne "test`tcrate`twall_clock_assumption`tdeterministic_seam") {
                throw "$Relative has the wrong column header: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 4 -or @($columns | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
            throw "$Relative row is not four non-empty columns: $line"
        }
        if ($tests.ContainsKey($columns[0])) {
            throw "$Relative names test $($columns[0]) more than once"
        }
        $tests[$columns[0]] = $true
        $rows += $line
    }
    if (-not $header) { throw "$Relative has no column header" }
    return , $rows
}

Push-Location $Repo
try {
    $lines = [IO.File]::ReadAllLines($list)
    $now = Read-TimingRows $lines
    $widenedNow = Read-Widenings $lines
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the timing comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $status = $LASTEXITCODE
    $base = if ($status -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the timing comparison did not run"
        exit 2
    }
    $text = (& git show "${base}:${Relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$Relative is not in $base - no base list exists; current census has $($now.Count) rows. PASS"
        exit 0
    }
    $baseLines = $text -split "`r?`n"
    $before = Read-TimingRows $baseLines
    $widenedBefore = @((Read-Widenings $baseLines) | ForEach-Object { $_.Line })
} finally {
    Pop-Location
}

$allowed = @{}
foreach ($row in $before) {
    if ($allowed.ContainsKey($row)) { $allowed[$row] += 1 } else { $allowed[$row] = 1 }
}
$added = @()
foreach ($row in $now) {
    if ($allowed.ContainsKey($row) -and $allowed[$row] -gt 0) {
        $allowed[$row] -= 1
    } else {
        $added += $row
    }
}
$removed = $before.Count - ($now.Count - $added.Count)
Write-Host "$Relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed."
$widenings = @($widenedNow | Where-Object { $widenedBefore -notcontains $_.Line })
if ($widenings.Count -gt 1) {
    throw ("$($widenings.Count) widening lines are new against the base, and one change widens the scan once:" +
        [Environment]::NewLine + (($widenings | ForEach-Object { "    $($_.Line)" }) -join [Environment]::NewLine))
}
if ($widenings.Count -eq 1) {
    $widening = $widenings[0]
    if ($widening.Count -ne $added.Count) {
        throw ("the widening line admits $($widening.Count) row(s) and $($added.Count) were added:" +
            [Environment]::NewLine + "    $($widening.Line)")
    }
    Write-Host "the widening admits its $($added.Count) row(s): $($widening.Line)"
    Write-Host "timing-bound test census grew by exactly its widening. PASS"
    exit 0
}
if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw ("$($added.Count) timing-bound test row(s) were added, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Replace the wall-clock assumption with a controlled clock, sleeper, receiver, or completion signal.")
}

Write-Host "timing-bound test census only shrank. PASS"
exit 0
