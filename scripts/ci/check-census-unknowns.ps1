# the_census_unknown_sites_only_shrink
#
# `docs/plans/design/ownership-census-unknowns.tsv` lists every write-shaped
# site the ownership census (bt_source::FieldCensus) could not resolve. Within
# one tree, bt-source's `census` test holds the code to it: the code's unknowns
# must be a subset of the list. This is the other half, across commits, as
# `check-migration-debt.ps1` is for MIGRATION-DEBT.tsv (the census note's
# revision (b)2 section 5): **the total number of unknown sites must not grow
# against the merge base.** It compares totals, not rows, so a move of a
# function (census-7) changes a row's key and passes as long as the total does
# not rise; a row added by hand to let a new unknown past the test is refused.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When
# the merge base has no list (the branch that introduces it), the baseline is
# the list as census-1 seeded it, pinned below by commit. There is no "nothing
# to compare, so pass" road: no origin/main, no merge base or an unreadable
# seed is exit 2, and a HEAD at the seed itself is compared like any other.
#
# It reads only the committed TSVs, through git and the working tree.
# `.github/workflows/ci.yml` runs it in `logic`, and `gates-can-fail` plants a
# hand-added row (it must go red) and a moved key (it must stay green).

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/design/ownership-census-unknowns.tsv"
$list = Join-Path $repo $relative
# The commit that seeded the list (census-1).
$seed = "7a53d4294eab7726813ca000119a496c27883708"

# The total of the `sites` column over the data rows: not blank, not a comment,
# not the column header.
function Get-SiteTotal([string[]]$lines, [string]$where) {
    $total = 0
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 5) { throw "$where has a row that is not five columns: $line" }
        $total += [int]$columns[4]
    }
    if (-not $header) { throw "$where has no column header" }
    return $total
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the census gate holds the code to it"
}

Push-Location $repo
try {
    $now = Get-SiteTotal ([IO.File]::ReadAllLines($list)) "$relative (working tree)"

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        $status = $LASTEXITCODE
        if ($status -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $now unknown sites and nothing to compare them against."
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
    $before = Get-SiteTotal ($text -split "`r?`n") "$relative at $from"
} finally {
    Pop-Location
}

Write-Host "$relative against ${from}: $before unknown sites -> $now."

if ($now -gt $before) {
    throw (
        "the census's unknown sites grew from $before to $now against ${from}, and this list only shrinks. " +
        "Resolve the new site in the code (the rules are bt_source::FieldCensus's), or restructure it so " +
        "the census can read it; a row is never added to $relative to let a new unknown past the census test."
    )
}
