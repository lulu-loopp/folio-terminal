# the_census_unknown_sites_only_shrink
#
# `docs/plans/design/ownership-census-unknowns.tsv` lists every write-shaped
# site the ownership census (bt_source::FieldCensus) could not resolve. Within
# one tree, bt-source's `census` test holds the code to it: the code's unknowns
# must be a subset of the list. This is the other half, across commits, as
# `check-migration-debt.ps1` is for MIGRATION-DEBT.tsv (the census note's
# revision (b)2 section 5): **the rows of unknown sites may only disappear
# against the merge base.** Rows are compared whole and with multiplicity, so a
# moved or replaced unknown is an added row even when another row disappeared
# and the total stayed level. A row added by hand to let a new unknown past the
# test is refused.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When
# the merge base has no list (the branch that introduces it), the baseline is
# the list as census-1 seeded it, pinned below by commit. There is no "nothing
# to compare, so pass" road: no origin/main, no merge base or an unreadable
# seed is exit 2, and a HEAD at the seed itself is compared like any other.
#
# It reads only the committed TSVs, through git and the working tree.
# `.github/workflows/ci.yml` runs it in `logic`, and `gates-can-fail` plants a
# hand-added row (it must go red), including when an old row is removed in the
# same change and the number of rows and sites stays level.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/design/ownership-census-unknowns.tsv"
$list = Join-Path $repo $relative
# The commit that seeded the list (census-1).
$seed = "7a53d4294eab7726813ca000119a496c27883708"

# A row is a data line: not blank, not a comment, not the column header.
function Read-CensusRows([string[]]$lines, [string]$where) {
    $rows = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne "site`tmodule`tfunction`treason`tsites") {
                throw "$where has the wrong column header: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 5) { throw "$where has a row that is not five columns: $line" }
        $sites = 0
        if (-not [int]::TryParse($columns[4], [ref]$sites) -or $sites -lt 1) {
            throw "$where has a row whose sites column is not a positive integer: $line"
        }
        $rows += $line
    }
    if (-not $header) { throw "$where has no column header" }
    return , $rows
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the census gate holds the code to it"
}

Push-Location $repo
try {
    $now = Read-CensusRows ([IO.File]::ReadAllLines($list)) "$relative (working tree)"

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        $status = $LASTEXITCODE
        if ($status -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) rows and nothing to compare them against."
        Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
        exit 2
    }

    $text = (& git show "${base}:${relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -eq 0) {
        $from = "the merge base $($base.Substring(0, 12))"
    } else {
        $text = (& git show "${seed}:${relative}" 2>$null) | Out-String
        if ($LASTEXITCODE -ne 0) {
            Write-Host "the merge base $($base.Substring(0, 12)) has no $relative, and the seed $($seed.Substring(0, 12)) cannot be read in this clone."
            Write-Host "This is not a pass: the comparison did not happen."
            exit 2
        }
        $from = "the seed $($seed.Substring(0, 12))"
    }
    $before = Read-CensusRows ($text -split "`r?`n") "$relative at $from"
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
Write-Host "$relative against ${from}: $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed."

if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw (
        "$($added.Count) row(s) were added to the census's unknown list against ${from}, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Resolve the new site in the code (the rules are bt_source::FieldCensus's), or restructure it so " +
        "the census can read it; a row is never added to $relative to let a new unknown past the census test."
    )
}

Write-Host "ownership-census unknown rows only shrank. PASS"
exit 0
