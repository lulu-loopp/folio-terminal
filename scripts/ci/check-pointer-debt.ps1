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
# A row is `owner`, `door`, `module` (the module the owner is written in) and `needle`;
# rows compare on `owner` and `door`. **A moved row is not added**
# (T-MOVED-NOT-ADDED-RULE, `moved-rows.ps1`): a row the base does not have whose
# owner, with its module taken off, and door are a gone base row's, at another
# module, is that row moved - when that owner and door left one module for one
# other, the old module no longer lists the owner, the new one did not at the
# base, and the moved rows are no more than the rows it had there. (A same-named
# owner in another module that takes over a read the old one dropped passes as
# moved, held to the base row count, as check-dead-code's U-44 rule does: the
# check reads the list, not the tree.) A reader moved into a module brings its
# imports: an `(outside any item)` row the base does not have is accepted for a
# module and door only for a needle that an owner moved into that module with
# that door in the same diff reads, and once per needle (ruled in K2 batch 23,
# 2026-10-27, replacing ruling Q1 (b)'s count of owners: an import names a
# needle, and two moved owners reading three button and wheel types bring three
# imports and no new reader). Every row's last column, `needle`, is the needle
# its occurrence matched, written by the query; the moved owner rows say which
# needles moved in, never the import's own text. So, per module and door: an
# import row whose needle a moved owner row there reads is accepted, a second
# import row of that needle is refused, an import row of a needle no moved owner
# there reads is refused naming the needle, and an import row with no needle
# column is refused. Rows compare on `owner` and `door` alone, so a base written
# before the `needle` column matches row for row; but every import row of one
# module and door shares that key, so an import row with a needle matches only a
# base import row of the same needle, or one written before the column, and
# never one of another needle - the imports judged are the ones added, in any
# order of the list (K2 batch 23's review, finding 1). A base row with no `module`
# (the list before that column) is never paired: it only matches whole, or is
# removed. There is no rename arm here: a row has no reason
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
# `door`; `module` is $null in a list written before that column, and `needle` in one written
# before the needle column.
function Read-DebtRows([string[]]$lines) {
    $rows = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $columns = $line.Split("`t")
        if ($columns.Count -lt 2 -or $columns.Count -gt 4) { throw "$relative has a row that is not owner, door, module and needle: $line" }
        $module = $null
        $tail = $null
        $needle = if ($columns.Count -eq 4) { $columns[3] } else { $null }
        if ($columns.Count -ge 3) {
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
            Needle  = $needle
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

# An import row's key is its module and door, which every import of that module and door shares,
# so an import row with a needle takes a base row of its own needle, else one written before the
# needle column, and never a base row of another needle: the rows left to judge are the imports
# that were added, whatever their order in the list (K2 batch 23's review, finding 1).
$unmatched = @()
foreach ($row in $now) {
    $at = -1
    if ($allowed.ContainsKey($row.Key) -and $allowed[$row.Key].Count -gt 0) {
        $pool = $allowed[$row.Key]
        if ($row.Outside -and $null -ne $row.Needle) {
            for ($i = 0; $i -lt $pool.Count -and $at -lt 0; $i++) { if ($pool[$i].Needle -ceq $row.Needle) { $at = $i } }
            for ($i = 0; $i -lt $pool.Count -and $at -lt 0; $i++) { if ($null -eq $pool[$i].Needle) { $at = $i } }
        } else {
            $at = 0
        }
    }
    if ($at -ge 0) {
        $allowed[$row.Key].RemoveAt($at)
    } else {
        $unmatched += $row
    }
}
$leftover = @(foreach ($rows in $allowed.Values) { foreach ($row in $rows) { Get-MoveRow $row } })

$added = @()
$moves = 0
# Per module and door: the needles the owners moved in read, and the import rows accepted for them.
$movedNeedles = [Collections.Generic.Dictionary[string, Collections.Generic.HashSet[string]]]::new([StringComparer]::Ordinal)
$results = Find-MovedRows -Unmatched @($unmatched | Where-Object { -not $_.Outside } | ForEach-Object { Get-MoveRow $_ }) `
    -Leftover $leftover -Tree @($now | ForEach-Object { Get-MoveRow $_ }) -Base @($before | ForEach-Object { Get-MoveRow $_ })
foreach ($result in $results) {
    $row = $result.Row
    if ($result.Verdict -eq "moved") {
        $moves += 1
        Write-Host "moved     $($row.Key -replace "`t", ' | ') (from $($result.Was))"
        $place = "$($row.Module)`t$($row.Door)"
        if (-not $movedNeedles.ContainsKey($place)) { $movedNeedles[$place] = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal) }
        if ($null -ne $row.Needle) { [void]$movedNeedles[$place].Add($row.Needle) }
    } elseif ($result.Verdict -eq "added") {
        $added += $row.Key
    } else {
        $added += "$($row.Key): $(Format-MoveRefusal $result)"
    }
}
$imports = 0
$spent = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($row in @($unmatched | Where-Object { $_.Outside })) {
    $place = "$($row.Module)`t$($row.Door)"
    if ($null -eq $row.Module -or -not $movedNeedles.ContainsKey($place)) {
        $added += $row.Key
    } elseif ($null -eq $row.Needle) {
        $added += "$($row.Key): an import row names its needle (the list's needle column), and this one names none"
    } elseif (-not $movedNeedles[$place].Contains($row.Needle)) {
        $read = (@($movedNeedles[$place]) | Sort-Object) -join ", "
        $added += "$($row.Key): it imports $($row.Needle), which no owner moved into $($row.Module) with $($row.Door) reads (they read: $read)"
    } elseif (-not $spent.Add("$place`t$($row.Needle)")) {
        $added += "$($row.Key): a second import row of $($row.Needle) for $($row.Module) and $($row.Door): one import row per needle a moved owner reads"
    } else {
        $imports += 1
        Write-Host "import    $($row.Key -replace "`t", ' | ') | $($row.Needle) (for a reader moved into $($row.Module))"
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
