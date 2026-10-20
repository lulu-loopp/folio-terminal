# the_env_doors_list_only_shrinks
#
# `docs/plans/ENV-DOORS.tsv` names every product item that reads the process
# environment in a way `crates/bt-source/tests/env_doors.rs` lists rather than
# refuses (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY): the program's entries, the doors not
# yet moved to the entry, and the shapes the guard's grammar cannot read. That
# test holds the list to the tree as an equality; this holds the list to its one
# direction: a branch may remove a row, or let a row read less, and may not add
# one. A new reader of a `BT_` door is moved to the entry instead.
#
# The comparison is against the pull request's merge base, as
# `check-pointer-debt.ps1`'s is, so a branch is judged on what it did. A row is
# keyed by its first two columns (file, item). Refused: a key the base does not
# have; a row whose `names` holds a name its base row did not; a row whose
# `refused` count is larger than its base row's. Category, ticket and reason may
# be rewritten (a door moved to the entry, a reason corrected). The commented
# header is not compared.
#
# It passes, loudly, when the base has no list - that is how the commit that
# introduces the list passes. It does not pass when there is no base at all.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/ENV-DOORS.tsv"
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - it is what crates/bt-source/tests/env_doors.rs holds the product's environment reads to"
}

# The rows by key: (file, item) -> @{ names = set; refused = count }.
function Read-DoorRows([string[]]$lines) {
    $rows = [ordered]@{}
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 7) { throw "$relative has a row that is not seven columns: $line" }
        $key = "$($columns[0]) $($columns[1])"
        if ($rows.Contains($key)) { throw "$relative names $key twice" }
        $names = @()
        if ($columns[3] -ne "-") { $names = $columns[3].Split(",") }
        $rows[$key] = @{ names = $names; refused = [int]$columns[4] }
    }
    if (-not $header) { throw "$relative has no column header" }
    return $rows
}

Push-Location $repo
try {
    $now = Read-DoorRows ([IO.File]::ReadAllLines($list))

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
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$relative is not in $base - this is the commit that introduces it, and it has $($now.Count) rows."
        exit 0
    }
    $before = Read-DoorRows ($text -split "`r?`n")
} finally {
    Pop-Location
}

$grown = @()
foreach ($key in $now.Keys) {
    if (-not $before.Contains($key)) {
        $grown += "    $key is a row the base does not have"
        continue
    }
    $was = $before[$key]
    $is = $now[$key]
    foreach ($name in $is.names) {
        if ($was.names -notcontains $name) { $grown += "    $key reads $name, which its base row does not" }
    }
    if ($is.refused -gt $was.refused) {
        $grown += "    $key holds $($is.refused) refused shape(s), its base row $($was.refused)"
    }
}
$removed = @($before.Keys | Where-Object { -not $now.Contains($_) }).Count
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $removed removed, $($grown.Count) grown."

if ($grown.Count -gt 0) {
    throw (
        "$relative only shrinks, and this branch grew it:" + [Environment]::NewLine +
        ($grown -join [Environment]::NewLine) + [Environment]::NewLine +
        "A BT_ door is read once at the program's entry (bt_app::entry_doors) and handed down; " +
        "move the read there instead of listing it (docs/ARCHITECTURE-GUARDS.md, Environment doors)."
    )
}
