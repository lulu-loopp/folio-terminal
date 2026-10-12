# the_lane_expected_failures_only_shrink
#
# `lane::EXPECTED_FAILURES` (crates/bt-app/src/lane.rs) is the list of claims of
# the lane contract a lane is known to fail, each with the structural-debt row
# that repairs it. The suite holds the tree to the list both ways
# (`lane_contract_tests::every_lane_runs_every_claim_and_fails_only_where_the_ledger_says`:
# an undeclared failure is red, and so is a declared one that passes), and
# `lane_contract_tests::the_expected_failures_are_this_list` holds the code equal
# to `crates/bt-app/src/lane_expected_failures.tsv`: its `# claims` section is
# every claim of the suite, its `# rows` section every declared failure.
#
# This script is the history half, over that file: against the merge base with
# origin/main, no lane gains a declared failure. A row is keyed by its lane and
# its claim; a key the merge base does not have is refused - unless its claim is
# not in the merge base's `# claims`, because a claim new to the suite arrives
# with the rows of the lanes that do not keep it yet, and those rows are its
# baseline. A row whose failure kind or repair changed is reported, not
# refused: the claim still fails, and the suite says how. It passes, loudly,
# when the merge base has no list; it refuses (exit 2) when there is no merge
# base at all.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "crates/bt-app/src/lane_expected_failures.tsv",
    [string]$MainRef = "origin/main"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$list = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $list)) {
    throw "$Relative is not in the tree - it is the lane contract's declared failures"
}

# The list as @{ Claims = [claim names]; Rows = ordered "Lane x Claim" -> "failure repair" }.
function Read-List([string[]]$Lines) {
    $claims = @()
    $rows = [ordered]@{}
    $section = $null
    $header = $false
    foreach ($line in $Lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("##")) { continue }
        if ($line.StartsWith("# ")) {
            $section = $line.Substring(2)
            $header = $false
            continue
        }
        if (-not $header) {
            $expected = if ($section -eq "claims") { "claim" } else { "lane`tclaim`tfailure`trepair" }
            if ($line -ne $expected) { throw "$Relative # $section has the wrong column header: $line" }
            $header = $true
            continue
        }
        switch ($section) {
            "claims" { $claims += $line }
            "rows" {
                $columns = $line.Split("`t")
                if ($columns.Count -ne 4 -or @($columns | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
                    throw "$Relative row is not four non-empty columns: $line"
                }
                $key = "$($columns[0]) x $($columns[1])"
                if ($rows.Contains($key)) { throw "$Relative declares $key twice" }
                $rows[$key] = "$($columns[2]) $($columns[3])"
            }
            default { throw "$Relative has a line outside # claims and # rows: $line" }
        }
    }
    return @{ Claims = $claims; Rows = $rows }
}

Push-Location $Repo
try {
    $now = Read-List ([IO.File]::ReadAllLines($list))
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the expected-failures comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $status = $LASTEXITCODE
    $base = if ($status -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the expected-failures comparison did not run"
        exit 2
    }
    $text = (& git show "${base}:${Relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$Relative is not in $base - no base list exists; the tree declares $($now.Rows.Count) rows. PASS"
        exit 0
    }
    $before = Read-List ($text -split "`r?`n")
} finally {
    Pop-Location
}

$added = @()
$baseline = @()
foreach ($key in $now.Rows.Keys) {
    if ($before.Rows.Contains($key)) {
        if ($before.Rows[$key] -ne $now.Rows[$key]) {
            Write-Host "  $key now reads $($now.Rows[$key]), at the merge base $($before.Rows[$key])"
        }
        continue
    }
    $claim = ($key -split ' x ')[1]
    if ($before.Claims -contains $claim) { $added += $key } else { $baseline += $key }
}
$removed = @($before.Rows.Keys | Where-Object { -not $now.Rows.Contains($_) }).Count
Write-Host ("$Relative against $($base.Substring(0, 12)): $($before.Rows.Count) rows -> $($now.Rows.Count), " +
    "$removed removed, $($baseline.Count) brought by claims new to the suite, $($added.Count) added.")
if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw ("$($added.Count) expected failure(s) were added for claims the merge base already had, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Repair the lane, or restore the claim's behaviour; a lane does not gain a declared failure.")
}

Write-Host "the lane contract's expected failures only shrank. PASS"
exit 0
