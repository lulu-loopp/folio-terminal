# the_doc_link_debt_list_only_shrinks
#
# `docs/plans/DOC-LINK-DEBT.tsv` is every broken intra-doc link in bt-app's documentation, one
# row per rustdoc warning (T-DOC-LINK-DEBT, 0.4.9 plan row 78). Its whole value is the direction
# it moves in: a ticket that fixes or rewords a link deletes its row; nothing adds one. The `docs`
# CI job (`scripts/ci/measure-doc-links.ps1` and bt-source's `doc_link_debt` target) is what
# refuses a broken link that is not in the list, and this is what refuses a link that was *put*
# in the list to get past it.
#
# The rules are `check-pointer-debt.ps1`'s, which this is a copy of, less its import arm. The
# comparison is against the pull request's merge base, not against the previous commit, so a
# branch is judged on what it did rather than on what main did underneath it. Rows are compared
# with multiplicity: a duplicated row is an added row, and because the list holds one row per
# warning, an owner with one broken link fewer is a row deleted, which passes. The commented
# header (and its `# measured at` stamp) is not compared.
#
# A row is `owner`, `target`, `shape`, `module` (the module the owner is written in) and `file`
# (where it is written today); rows compare on `owner`, `target` and `shape`, and `file` is never
# read. **A moved row is not added** (T-MOVED-NOT-ADDED-RULE, `moved-rows.ps1`): a row the base
# does not have whose owner, with its module taken off, target and shape are a gone base row's,
# at another module, is that row moved - when that owner left one module for one other, the old
# module no longer lists the owner, the new one did not at the base, and the moved rows are no
# more than the rows it had there. A `<module> (outside any item)` row (a module doc, the doc of
# a `mod x;` declaration or of an item the index keeps no record for) names no item and is never
# moved: one the base does not have is added. There is no rename arm: a row has no reason of its
# own to carry the dated reason a rename needs, so a renamed owner is an added row, and the
# renaming ticket fixes or rewords that link in the same commit.
#
# It passes, loudly, when the base has no list - that is how the commit that introduces the list
# passes. It does not pass when there is no base at all: no `origin/main` in the clone, or no
# merge base, means the comparison did not happen, and a check that did not run must not look
# like a check that agreed. Locally it is one `git fetch origin main` away.
# `check-doc-link-debt-tests.ps1` runs it against scratch repositories (`-Repo`), one case per
# rule; `gates-can-fail` plants a row in a clone whose base is its own head.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$repo = $Repo
$relative = "docs/plans/DOC-LINK-DEBT.tsv"
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the debt list is what the docs job holds bt-app's broken doc links against"
}

# The spelling of a row whose warning is in no record.
$outside = " (outside any item)"

# A row is a data line: not blank, not a comment, not the column header. Its key is `owner`,
# `target` and `shape`; its `module` locates it for the move rule.
function Read-DebtRows([string[]]$lines) {
    $rows = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -cne "owner`ttarget`tshape`tmodule`tfile") { throw "$relative has no column header 'owner, target, shape, module, file': $line" }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 5) { throw "$relative has a row that is not owner, target, shape, module and file: $line" }
        $module = $columns[3]
        if ($columns[0] -ceq "$module$outside") {
            $tail = $null
        } elseif ($columns[0].StartsWith("${module}::", [StringComparison]::Ordinal)) {
            $tail = $columns[0].Substring($module.Length + 2)
        } else {
            throw "$relative has a row whose owner is not written in its module: $line"
        }
        $rows += [pscustomobject]@{
            Key    = "$($columns[0])`t$($columns[1])`t$($columns[2])"
            What   = "$($columns[1])`t$($columns[2])"
            Module = $module
            Tail   = $tail
        }
    }
    if (-not $header) { throw "$relative has no column header" }
    return , $rows
}

# A row as the move rule reads it: located by its module, about its owner inside it, and the
# same row when its target and shape are too.
function Get-MoveRow($row) {
    return [pscustomobject]@{
        Identity = "$($row.Tail)`t$($row.What)"
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
$results = Find-MovedRows -Unmatched @($unmatched | ForEach-Object { Get-MoveRow $_ }) `
    -Leftover $leftover -Tree @($now | ForEach-Object { Get-MoveRow $_ }) -Base @($before | ForEach-Object { Get-MoveRow $_ })
foreach ($result in $results) {
    $row = $result.Row
    if ($result.Verdict -eq "moved") {
        $moves += 1
        Write-Host "moved     $($row.Key -replace "`t", ' | ') (from $($result.Was))"
    } elseif ($result.Verdict -eq "added") {
        $added += $row.Key
    } else {
        $added += "$($row.Key): $(Format-MoveRefusal $result)"
    }
}

$removed = $leftover.Count - $moves
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed, $moves moved."

if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw (
        "$($added.Count) row(s) were added to $relative, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "A broken doc link is fixed or reworded, never listed: write the path that resolves where " +
        "the doc is, a code span for a name that is no item, or say the fact in words. A row leaves " +
        "the list in the commit that fixes its link."
    )
}
