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
# **A moved row is not added** (T-MOVED-NOT-ADDED-RULE, `moved-rows.ps1`). The
# eighth column, `module`, is the module the item is written in, so a row's item
# is its module and its path inside it. A row the base does not have is a gone
# base row moved when, in the same package (the file's path before `/src/`), its
# path inside its module is the base row's and its file and module are not -
# when that item left one place for one other, the old place no longer lists it
# and the new one did not at the base - and it is then held to that base row
# (so a same-named item at another place that takes over a read the old one
# dropped passes as moved, as check-dead-code's U-44 rule does: the check reads
# the list, not the tree): no name its base row did not read,
# no more refused shapes. A row with no item (`-`) is never moved, and neither is
# a base row with no `module` (the list before the column): it only matches by
# key, or is removed.
#
# **A renamed row is accepted only with a dated reason** (the rename arm of
# `moved-rows.ps1`): a row the base does not have whose file, module and `names`
# are exactly one gone base row's - the item renamed in place, and no other row
# of that file, module and names left or arrived - is that row renamed when its
# reason is `YYYY-MM-DD: <TICKET-ID> <why>` (a real day, new against the base
# row's) and it is held to the base row as a moved one is. Without the reason, or
# with two candidates on a side, it is added and refused.
#
# It passes, loudly, when the base has no list - that is how the commit that
# introduces the list passes. It does not pass when there is no base at all.

# `-Repo` and `-Relative` exist for `check-env-doors-tests.ps1`, which runs this
# gate against scratch repositories; CI runs it with the defaults.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "docs/plans/ENV-DOORS.tsv"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$repo = $Repo
$relative = $Relative
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - it is what crates/bt-source/tests/env_doors.rs holds the product's environment reads to"
}

# The rows by key: (file, item) -> @{ names = set; refused = count; move = the row as the move
# rule reads it }. A list written before the `module` column has seven columns.
function Read-DoorRows([string[]]$lines) {
    $rows = [ordered]@{}
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 7 -and $columns.Count -ne 8) { throw "$relative has a row that is not eight columns: $line" }
        $key = "$($columns[0]) $($columns[1])"
        if ($rows.Contains($key)) { throw "$relative names $key twice" }
        $names = @()
        if ($columns[3] -ne "-") { $names = $columns[3].Split(",") }
        $location = $null
        $tail = $null
        $package = $null
        $at = $columns[0].IndexOf("/src/", [StringComparison]::Ordinal)
        if ($columns.Count -eq 8 -and $columns[1] -cne "-" -and $at -ge 0) {
            $module = $columns[7]
            if (-not $columns[1].StartsWith("${module}::", [StringComparison]::Ordinal)) {
                throw "$relative has a row whose item is not written in its module: $line"
            }
            $location = "$($columns[0]) ($module)"
            $tail = $columns[1].Substring($module.Length + 2)
            $package = $columns[0].Substring(0, $at)
        }
        $rows[$key] = @{
            names   = $names
            refused = [int]$columns[4]
            reason  = $columns[6]
            group   = if ($columns.Count -eq 8 -and $columns[1] -cne "-") { "$($columns[0])`t$($columns[7])`t$($columns[3])" } else { $null }
            move    = [pscustomobject]@{ Identity = "$package`t$tail"; Location = $location; Tail = $tail; Row = $key }
        }
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
# A row against the base row it is: no name that row did not read, no more refused shapes.
function Test-Grown([string]$key, $is, $was, [string]$said) {
    foreach ($name in $is.names) {
        if ($was.names -notcontains $name) { $script:grown += "    $key reads $name, which its base row${said} does not" }
    }
    if ($is.refused -gt $was.refused) {
        $script:grown += "    $key holds $($is.refused) refused shape(s), its base row${said} $($was.refused)"
    }
}
$unmatched = @()
foreach ($key in $now.Keys) {
    if (-not $before.Contains($key)) {
        $unmatched += $now[$key].move
        continue
    }
    Test-Grown $key $now[$key] $before[$key] ""
}
$leftover = @($before.Keys | Where-Object { -not $now.Contains($_) } | ForEach-Object { $before[$_].move })
$moves = 0
$paired = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$unpaired = @()
foreach ($result in (Find-MovedRows -Unmatched $unmatched -Leftover $leftover -Tree @($now.Values | ForEach-Object { $_.move }) -Base @($before.Values | ForEach-Object { $_.move }))) {
    $key = $result.Row
    if ($result.Verdict -eq "moved") {
        $moves += 1
        [void]$paired.Add($result.From)
        Write-Host "moved     $key (from $($result.Was))"
        Test-Grown $key $now[$key] $before[$result.From] " (moved from $($result.Was))"
    } elseif ($result.Verdict -eq "added") {
        $unpaired += $key
    } else {
        $grown += "    $key is a row the base does not have: $(Format-MoveRefusal $result)"
    }
}
# The rename arm (moved-rows.ps1): an added row whose file, module and names are one gone base
# row's, and no other added row's, is that row renamed - with a dated reason naming its ticket.
$renames = 0
$gone = @($before.Keys | Where-Object { -not $now.Contains($_) -and -not $paired.Contains($_) -and $null -ne $before[$_].group } |
    ForEach-Object { [pscustomobject]@{ Group = $before[$_].group; Row = $_; Reason = $before[$_].reason } })
$arrived = @($unpaired | Where-Object { $null -ne $now[$_].group } |
    ForEach-Object { [pscustomobject]@{ Group = $now[$_].group; Row = $_; Reason = $now[$_].reason } })
foreach ($key in @($unpaired | Where-Object { $null -eq $now[$_].group })) {
    $grown += "    $key is a row the base does not have"
}
foreach ($result in (Find-RenamedRows -Added $arrived -Leftover $gone)) {
    $key = $result.Row
    if ($result.Verdict -eq "renamed") {
        $wrong = Test-RenameReason $result.Reason $result.WasReason
        if ($wrong) {
            $grown += "    $key is a row the base does not have: it would be $($result.From) renamed, and $wrong"
            continue
        }
        $renames += 1
        Write-Host "renamed   $key (from $($result.From))"
        Test-Grown $key $now[$key] $before[$result.From] " (renamed from $($result.From))"
    } elseif ($result.Verdict -eq "ambiguous") {
        $grown += "    $key is a row the base does not have: more than one row of its file, module and names left or arrived, so no rename can be paired"
    } else {
        $grown += "    $key is a row the base does not have"
    }
}
$removed = $leftover.Count - $moves - $renames
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $removed removed, $($grown.Count) grown, $moves moved, $renames renamed."

if ($grown.Count -gt 0) {
    throw (
        "$relative only shrinks, and this branch grew it:" + [Environment]::NewLine +
        ($grown -join [Environment]::NewLine) + [Environment]::NewLine +
        "A BT_ door is read once at the program's entry (bt_app::entry_doors) and handed down; " +
        "move the read there instead of listing it (docs/ARCHITECTURE-GUARDS.md, Environment doors)."
    )
}
