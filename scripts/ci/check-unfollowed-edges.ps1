# the_unfollowed_edges_only_shrink
#
# `docs/plans/UNFOLLOWED-EDGES.tsv` lists, by SITE, every call the door reach cannot see through:
# the `Ambiguous` edges (an untyped receiver; not exactly one candidate) and the `Dynamic` edges
# (a stored closure, a function pointer, a trait object no first-party implementation answers, a
# foreign trait object) that bt_source::call_graph meets walking from every product callable of
# bt-app (T-BT-SOURCE-CALL-GRAPH round 3). Within one tree, bt-source's `call_graph` test
# `the_unfollowed_edges_are_the_committed_list` holds the list equal to the code. This is the
# other half, across commits, in the shape of `check-census-unknowns.ps1`: the list is keyed by
# a site's identity — its first four columns, `file`, `item`, `kind` and `name` (the call's name)
# — and `sites`, the number of calls behind that key, is its measurement.
#
# **Against the merge base, a key added or a key whose `sites` grew is refused unless its
# `reason` cell is filled and dated**: `YYYY-MM-DD: <why>`, the date a real calendar day, the why
# not blank. A dated reason is a reviewed decision to let the door reach go blind at that call;
# an undated new row is a call nobody looked at. A key that disappeared or whose `sites` went down
# is the list shrinking, which is what regenerating it with `scripts/generate-unfollowed-edges.ps1`
# after a fix writes. Keys compare ordinally.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When the merge base has
# no list (the branch that introduces it), the baseline is the list as T-BT-SOURCE-CALL-GRAPH seeded
# it, pinned below by commit (as check-census-unknowns.ps1 pins census-1's). There is no "nothing to
# compare, so pass" road: no origin/main, no merge base, or an unreadable seed is exit 2.
#
# For its own tests (`check-unfollowed-edges-tests.ps1`), `-ListPath` and `-BaselinePath` replace
# the working tree's list and the baseline read through git.

param(
    [string] $ListPath,
    [string] $BaselinePath
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/UNFOLLOWED-EDGES.tsv"
$header = "file`titem`tkind`tname`tsites`treason"
# The commit that seeded the list (T-BT-SOURCE-CALL-GRAPH round 3, on the tree merged with main).
$seed = "09388cb627a0739d18b9a7603c9a151f6544be56"

# A row is a data line: not blank, not a comment, not the header. Its key is its first four
# columns, tab-joined; a key on two rows is refused rather than read as either.
function Read-UnfollowedEdges([string[]]$lines, [string]$where) {
    $rows = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    $seen = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $seen) {
            if ($line -ne $header) { throw "$where has the wrong column header: $line" }
            $seen = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 6) { throw "$where has a row that is not six columns: $line" }
        if ($columns[2] -cne "Ambiguous" -and $columns[2] -cne "Dynamic") {
            throw "$where has a row whose kind is neither Ambiguous nor Dynamic: $line"
        }
        $sites = 0
        if (-not [int]::TryParse($columns[4], [ref]$sites) -or $sites -lt 1) {
            throw "$where has a row whose sites column is not a positive integer: $line"
        }
        $key = $columns[0..3] -join "`t"
        if ($rows.ContainsKey($key)) { throw "$where carries $(Format-Key $key) on two rows; each key has one row" }
        $rows[$key] = [pscustomobject]@{ Sites = $sites; Reason = $columns[5] }
    }
    if (-not $seen) { throw "$where has no column header" }
    return $rows
}

function Format-Key([string]$key) {
    $columns = $key.Split("`t")
    return "$($columns[2]) $($columns[3]) in $($columns[1]) ($($columns[0]))"
}

# A reason a reviewer wrote: `YYYY-MM-DD: <why>`, the date a real day.
function Test-Dated([string]$reason) {
    if ($reason -cnotmatch '^(?<date>\d{4}-\d{2}-\d{2}): \S') { return $false }
    $parsed = [DateTime]::MinValue
    return [DateTime]::TryParseExact($Matches['date'], 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture,
        [Globalization.DateTimeStyles]::None, [ref]$parsed)
}

if ($ListPath) {
    $now = Read-UnfollowedEdges ([IO.File]::ReadAllLines($ListPath)) $ListPath
    $before = Read-UnfollowedEdges ([IO.File]::ReadAllLines($BaselinePath)) $BaselinePath
    $from = "the baseline $BaselinePath"
} else {
    $list = Join-Path $repo $relative
    if (-not (Test-Path -LiteralPath $list)) {
        throw "$relative is not in the tree - bt-source's call_graph test holds the code to it"
    }
    Push-Location $repo
    try {
        $now = Read-UnfollowedEdges ([IO.File]::ReadAllLines($list)) "$relative (working tree)"
        $base = $null
        & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
        if ($LASTEXITCODE -eq 0) {
            $found = & git merge-base HEAD origin/main 2>$null
            if ($LASTEXITCODE -eq 0) { $base = @($found)[0] }
        }
        if (-not $base) {
            Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) keys and nothing to compare them against."
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
        $before = Read-UnfollowedEdges ($text -split "`r?`n") "$relative at $from"
    } finally {
        Pop-Location
    }
}

$refused = @()
$allowed = 0
$shrunk = 0
foreach ($key in $now.Keys) {
    $row = $now[$key]
    $change = $null
    if (-not $before.ContainsKey($key)) {
        $change = "added ($($row.Sites) site(s))"
    } elseif ($row.Sites -gt $before[$key].Sites) {
        $change = "grown from $($before[$key].Sites) to $($row.Sites) site(s)"
    } elseif ($row.Sites -lt $before[$key].Sites) {
        $shrunk += 1
    }
    if ($null -eq $change) { continue }
    if (Test-Dated $row.Reason) {
        $allowed += 1
    } else {
        $refused += "    $(Format-Key $key): $change, reason '$($row.Reason)'"
    }
}
$removed = @($before.Keys | Where-Object { -not $now.ContainsKey($_) }).Count
Write-Host ("$relative against ${from}: $($before.Count) keys -> $($now.Count), " +
    "$allowed added or grown with a dated reason, $($refused.Count) refused, $shrunk shrunk, $removed removed.")

if ($refused.Count -gt 0) {
    throw (
        "the unfollowed edges grew against ${from} with no dated reason:" + [Environment]::NewLine +
        ($refused -join [Environment]::NewLine) + [Environment]::NewLine +
        "Type the receiver or resolve the call (bt_source::call_graph's grammar), or, when the door reach " +
        "must go blind there, write 'YYYY-MM-DD: <why>' in the row's reason cell."
    )
}
Write-Host "the unfollowed edges only shrank, or grew with a dated reason. PASS"
exit 0
