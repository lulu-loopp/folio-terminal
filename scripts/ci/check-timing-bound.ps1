# the_timing_bound_test_list_only_shrinks
#
# `docs/plans/TIMING-BOUND-TESTS.tsv` names tests whose verdict assumes real
# elapsed time. Existing rows are migration work; a new row is a new flaky-test
# liability. Compare with the pull request merge base, whole rows and
# multiplicity included, so the list can only shrink.

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
    $now = Read-TimingRows ([IO.File]::ReadAllLines($list))
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
    $before = Read-TimingRows ($text -split "`r?`n")
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
if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw ("$($added.Count) timing-bound test row(s) were added, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Replace the wall-clock assumption with a controlled clock, sleeper, receiver, or completion signal.")
}

Write-Host "timing-bound test census only shrank. PASS"
exit 0
