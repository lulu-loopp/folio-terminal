# the_pointer_debt_list_only_shrinks
#
# `docs/plans/POINTER-DEBT.tsv` is every read of pointer data in bt-app outside
# `crate::runtime::pointer`, one row per occurrence (T-POINTER-CAPTURE,
# `docs/plans/design/pointer-capture-2026-10-09.md` section 4.2). Its whole value
# is the direction it moves in: a ticket that moves a reader behind the router
# deletes its rows; nothing adds one. The bt-app test
# `every_pointer_read_is_the_routers_or_a_captures` is what refuses a reader that
# is not in the list, and this is what refuses a reader that was *put* in the
# list to get past it.
#
# The rules are `check-migration-debt.ps1`'s, unchanged. The comparison is against
# the pull request's merge base, not against the previous commit, so a branch is
# judged on what it did rather than on what main did underneath it. Rows are
# compared whole and with multiplicity: a duplicated row is an added row, and
# because the list holds one row per occurrence, an owner reading one thing fewer
# is a row deleted, which passes. The commented header is not compared.
#
# A row is `owner`, `door` and `module`, the module the owner is written in;
# rows compare on `owner` and `door`. **A moved row is not added**
# (T-MOVED-NOT-ADDED-RULE, `moved-rows.ps1`): a row the base does not have whose
# owner, with its module taken off, and door are a gone base row's, at another
# module, is that row moved - when that owner and door left one module for one
# other, the old module no longer holds the owner, and the moved rows are no
# more than the rows it had there. A reader moved into a module brings its
# imports: an `(outside any item)` row the base does not have is accepted for a
# module and door only up to the number of owners moved into that module with
# that door in the same diff (ruling Q1 (b)), and refused beyond it. A base row
# with no `module` (the list before the column) is never paired: it only
# matches whole, or is removed. There is no rename arm here: a row has no reason
# of its own to carry the dated reason a rename needs, so a renamed owner is an
# added row, as before.
#
# It passes, loudly, when the base has no list - that is how the commit that
# introduces the list passes. It does not pass when there is no base at all: no
# `origin/main` in the clone, or no merge base, means the comparison did not
# happen, and a check that did not run must not look like a check that agreed.
# Locally it is one `git fetch origin main` away. `check-pointer-debt-tests.ps1`
# runs it against scratch repositories (`-Repo`), one case per rule.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$repo = $Repo
$relative = "docs/plans/POINTER-DEBT.tsv"
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the debt list is what every_pointer_read_is_the_routers_or_a_captures allows readers against"
}

# The spelling of a row whose occurrence is in no item.
$outside = " (outside any item)"

# A row is a data line: not blank, not a comment, not the column header. Its key is `owner` and
# `door`; `module` is $null in a list written before the column.
function Read-DebtRows([string[]]$lines) {
    $rows = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $columns = $line.Split("`t")
        if ($columns.Count -lt 2 -or $columns.Count -gt 3) { throw "$relative has a row that is not owner, door and module: $line" }
        $module = $null
        $tail = $null
        if ($columns.Count -eq 3) {
            $module = $columns[2]
            if ($columns[0] -ceq "$module$outside") {
                $tail = $null
            } elseif ($columns[0].StartsWith("${module}::", [StringComparison]::Ordinal)) {
                $tail = $columns[0].Substring($module.Length + 2)
            } else {
                throw "$relative has a row whose owner is not written in its module: $line"
            }
        }
        $rows += [pscustomobject]@{
            Key     = "$($columns[0])`t$($columns[1])"
            Door    = $columns[1]
            Module  = $module
            Tail    = $tail
            Outside = $columns[0].EndsWith($outside, [StringComparison]::Ordinal)
        }
    }
    if (-not $header) { throw "$relative has no column header" }
    return , $rows
}

# A row as the move rule reads it: located by its module, about its owner inside it, and the
# same row when its door is too.
function Get-MoveRow($row) {
    return [pscustomobject]@{
        Identity = "$($row.Tail)`t$($row.Door)"
        Location = if ($null -ne $row.Tail) { $row.Module } else { $null }
        Tail     = $row.Tail
        Row      = $row
    }
}

Push-Location $repo
try {
    $now = Read-DebtRows ([IO.File]::ReadAllLines($list))

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        # git's own status, read before any other stage can stand on it (see
        # check-migration-debt.ps1 for why this is not piped into Select-Object).
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
    $before = Read-DebtRows ($text -split "`r?`n")
} finally {
    Pop-Location
}

# Multiplicity, not membership: two identical rows are two rows.
$allowed = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new([StringComparer]::Ordinal)
foreach ($row in $before) {
    if (-not $allowed.ContainsKey($row.Key)) { $allowed[$row.Key] = [Collections.Generic.List[object]]::new() }
    $allowed[$row.Key].Add($row)
}

$unmatched = @()
foreach ($row in $now) {
    if ($allowed.ContainsKey($row.Key) -and $allowed[$row.Key].Count -gt 0) {
        $allowed[$row.Key].RemoveAt(0)
    } else {
        $unmatched += $row
    }
}
$leftover = @(foreach ($rows in $allowed.Values) { foreach ($row in $rows) { Get-MoveRow $row } })

$added = @()
$moves = 0
# Per module and door: the owners moved in, and the import rows accepted for them.
$movedOwners = [Collections.Generic.Dictionary[string, Collections.Generic.HashSet[string]]]::new([StringComparer]::Ordinal)
$results = Find-MovedRows -Unmatched @($unmatched | Where-Object { -not $_.Outside } | ForEach-Object { Get-MoveRow $_ }) `
    -Leftover $leftover -Tree @($now | ForEach-Object { Get-MoveRow $_ })
foreach ($result in $results) {
    $row = $result.Row
    if ($result.Verdict -eq "moved") {
        $moves += 1
        Write-Host "moved     $($row.Key -replace "`t", ' | ') (from $($result.Was))"
        $place = "$($row.Module)`t$($row.Door)"
        if (-not $movedOwners.ContainsKey($place)) { $movedOwners[$place] = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal) }
        [void]$movedOwners[$place].Add($row.Tail)
    } elseif ($result.Verdict -eq "added") {
        $added += $row.Key
    } else {
        $added += "$($row.Key): $(Format-MoveRefusal $result)"
    }
}
$imports = 0
$spent = [Collections.Generic.Dictionary[string, int]]::new([StringComparer]::Ordinal)
foreach ($row in @($unmatched | Where-Object { $_.Outside })) {
    $place = "$($row.Module)`t$($row.Door)"
    $owed = if ($null -ne $row.Module -and $movedOwners.ContainsKey($place)) { $movedOwners[$place].Count } else { 0 }
    $used = if ($spent.ContainsKey($place)) { $spent[$place] } else { 0 }
    if ($used -lt $owed) {
        $spent[$place] = $used + 1
        $imports += 1
        Write-Host "import    $($row.Key -replace "`t", ' | ') (for a reader moved into $($row.Module))"
    } elseif ($owed -gt 0) {
        $added += "$($row.Key): more import rows for $($row.Module) and $($row.Door) than the $owed owner(s) moved into it with that door"
    } else {
        $added += $row.Key
    }
}

$removed = $leftover.Count - $moves
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed, $moves moved, $imports import(s) of moved readers."

if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw (
        "$($added.Count) row(s) were added to $relative, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "A row is removed by the ticket that moves its reader behind the router " +
        "(docs/plans/design/pointer-capture-2026-10-09.md section 4.1). A new reader of pointer data " +
        "is written inside crate::runtime::pointer instead."
    )
}
