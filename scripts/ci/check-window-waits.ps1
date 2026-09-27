# the_bare_site_inventory_only_shrinks
#
# `docs/plans/window-thread-bare-sites.tsv` is the inventory of every vocabulary
# site in the product that sits outside a registered door (thread-door note
# 2026-09-26, revisions (j)1, (j)11.5 and (j)11.6). The lint cannot be switched
# on while such sites exist, so until A2e the file stands in for it, and its
# whole value is the direction it moves in: a ticket that routes a site through a
# door removes its row or lowers its count; nothing adds one.
#
# Two halves hold it. The equality half is a test,
# `hang_watch::window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site`:
# the file is exactly what the code has. This is the historical half: the file
# is no larger than the baseline, key by key.
#
#   * A row's key is (crate, arm, item, entry); its count is a positive integer;
#     a key written twice is refused.
#   * The baseline is the file at the pull request's merge base with origin/main
#     when the merge base has it, and otherwise the seed: the file as commit
#     $Seed wrote it. A branch whose merge base predates the seed is therefore
#     compared with the seed, without rebasing, and a working-tree plant at the
#     seed's own commit is an addition against the seed. There is no road on
#     which a missing baseline passes.
#   * current[key] <= baseline[key], a key missing from the baseline read as
#     zero: a new key is refused, and so is a count that grew. A function that
#     carries a bare site and moves is one removal and one refused addition.
#
# No merge base at all (no origin/main in the clone) is not a pass either: the
# comparison did not happen. CI checks out with full history on both jobs that
# run this.
#
# Prove it fires before trusting it: `gates-can-fail` adds a bare site with its
# row and requires this to go red.

param(
    # The seed: the commit that wrote the inventory first (A2a S1). Pinned, never
    # computed.
    [string]$Seed = "2cc59a833f22cf462a8a7b11b4a48609624b1930"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/window-thread-bare-sites.tsv"
$inventory = Join-Path $repo $relative
$columns = "crate`tarm`titem`tentry`tcount"

# The rows of one version of the file: key -> count, refusing what is not well formed.
function Read-Inventory([string[]]$lines, [string]$where) {
    $rows = [ordered]@{}
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne $columns) { throw "$where`: the columns are '$line', not '$columns'" }
            $header = $true
            continue
        }
        $cells = $line -split "`t"
        if ($cells.Count -ne 5) { throw "$where`: not five cells: $line" }
        $count = 0
        if (-not [int]::TryParse($cells[4], [ref]$count) -or $count -lt 1 -or "$count" -ne $cells[4]) {
            throw "$where`: the count is not a positive integer: $line"
        }
        $key = ($cells[0..3] -join "`t")
        if ($rows.Contains($key)) { throw "$where`: a key written twice: $line" }
        $rows[$key] = $count
    }
    if (-not $header) { throw "$where has no column header" }
    return $rows
}

if (-not (Test-Path -LiteralPath $inventory)) {
    throw "$relative is not in the tree - until A2e deletes it after proving it empty, the inventory is what stands in for the lint"
}

Push-Location $repo
try {
    $now = Read-Inventory ([IO.File]::ReadAllLines($inventory)) "the working tree's $relative"

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
    $against = "the merge base $($base.Substring(0, 12))"
    if ($LASTEXITCODE -ne 0) {
        $text = (& git show "${Seed}:${relative}" 2>$null) | Out-String
        if ($LASTEXITCODE -ne 0) {
            throw "$relative is not at the merge base $base, and the pinned seed $Seed cannot be read: fetch the history that holds it"
        }
        $against = "the seed $($Seed.Substring(0, 12)) (the merge base $($base.Substring(0, 12)) predates the inventory)"
    }
    $before = Read-Inventory ($text -split "`r?`n") "$relative at $against"
} finally {
    Pop-Location
}

$added = @()
$grown = @()
$shrunk = 0
foreach ($key in $now.Keys) {
    $held = if ($before.Contains($key)) { $before[$key] } else { 0 }
    if ($held -eq 0) {
        $added += "    $($key -replace "`t", ' | ') ($($now[$key]))"
    } elseif ($now[$key] -gt $held) {
        $grown += "    $($key -replace "`t", ' | '): $held -> $($now[$key])"
    } elseif ($now[$key] -lt $held) {
        $shrunk += $held - $now[$key]
    }
}
foreach ($key in $before.Keys) {
    if (-not $now.Contains($key)) { $shrunk += $before[$key] }
}

$total = 0
foreach ($count in $now.Values) { $total += $count }
$was = 0
foreach ($count in $before.Values) { $was += $count }
Write-Host "$relative against $against`: $was sites -> $total, $($added.Count) key(s) added, $($grown.Count) grown, $shrunk site(s) removed."

if ($added.Count -gt 0 -or $grown.Count -gt 0) {
    $details = @()
    if ($added.Count -gt 0) { $details += "added (a key the baseline does not have):"; $details += $added }
    if ($grown.Count -gt 0) { $details += "grown (a count above the baseline's):"; $details += $grown }
    throw (
        "the bare-site inventory only shrinks, and this adds to it:" + [Environment]::NewLine +
        ($details -join [Environment]::NewLine) + [Environment]::NewLine +
        "A new effect goes through its door (docs/ARCHITECTURE.md section 6); a function that carries a bare " +
        "site is moved only after its site goes through one (thread-door note, revision (j)1)."
    )
}
Write-Host "the window thread's bare sites: $total pending (docs/ARCHITECTURE.md section 6)."
