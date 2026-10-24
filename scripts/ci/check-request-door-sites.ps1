# the_request_door_sites_only_shrink
#
# `docs/plans/REQUEST-DOOR-SITES.tsv` lists, by SITE, every failure the C1 guard meets
# (T-REQUEST-DOOR-GUARD; bt-source's `request_doors::every_request_door_returns_at_once` walks each
# request door of crates/bt-app/src/window_waits.tsv's `# request-doors` to its boundary): an
# unresolved edge, an effect, a blocking primitive, an owner door or a worker body reached, another
# window door inherited, a boundary never met. Within one tree the test holds the list equal to the
# code. This is the other half, across commits, in the shape of `check-unfollowed-edges.ps1`: a row
# is keyed by its first four columns, `file`, `item`, `kind` and `name` - never a line, so a move of
# an item re-keys only its file and item - and `sites` is its measurement. `verdict` is `std` or
# `follows <key>; ...` (read: nothing behind it), `unreviewed` (not read yet) or `violation` (a C1
# breach, the finding named in the reason).
#
# **Against the merge base, a key added, a key whose `sites` grew, and a key whose verdict moved to
# `unreviewed` or `violation` from another are refused unless its `reason` is dated** -
# `YYYY-MM-DD: <why>`, a real day, the why not blank - **and, for a key the baseline had, other than
# the reason its row had there**. A key that disappeared or shrank, or a verdict that moved to `std`
# or `follows` (a site read), is the list shrinking. The script prints the `unreviewed` and
# `violation` counts against the baseline's: both are counted debts that only shrink but for a
# dated arrival.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When the merge base has
# no list (the branch that introduces it), the baseline is the list as T-REQUEST-DOOR-GUARD seeded
# it, pinned below by commit. There is no "nothing to compare, so pass" road: no origin/main, no
# merge base, or an unreadable seed is exit 2.
#
# For its own tests (`check-request-door-sites-tests.ps1`), `-ListPath` and `-BaselinePath` replace
# the working tree's list and the baseline read through git; `-Repo` and `-Seed` run the git road in
# a planted repository with a planted seed.

param(
    [string] $ListPath,
    [string] $BaselinePath,
    [string] $Repo,
    [string] $Seed
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$repo = if ($Repo) { $Repo } else { Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }
$relative = "docs/plans/REQUEST-DOOR-SITES.tsv"
$header = "file`titem`tkind`tname`tsites`tverdict`treason"
$kinds = @("Dynamic", "Ambiguous", "Macro", "OtherArm", "Generated", "Path", "Effect", "Blocking",
    "OwnerDoor", "WorkerBody", "Entrance", "Inherits", "Unmet", "HandedCalled", "Implicit")
# The commit that seeded the list (T-REQUEST-DOOR-GUARD).
$seed = if ($Seed) { $Seed } else { "SEED-NOT-YET-PINNED" }

function Format-Key([string]$key) {
    $columns = $key.Split("`t")
    return "$($columns[2]) $($columns[3]) in $($columns[1]) ($($columns[0]))"
}

# The verdict's word: `std`, `follows`, `unreviewed` or `violation`, or $null for anything else.
function Get-Verdict([string]$cell) {
    if ($cell -ceq "std" -or $cell -ceq "unreviewed" -or $cell -ceq "violation") { return $cell }
    if ($cell -cmatch '^follows \S') { return "follows" }
    return $null
}

function Read-Sites([string[]]$lines, [string]$where) {
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
        if ($columns.Count -ne 7) { throw "$where has a row that is not seven columns: $line" }
        if ($kinds -cnotcontains $columns[2]) { throw "$where has a row whose kind is not one of $($kinds -join ', '): $line" }
        $sites = 0
        if (-not [int]::TryParse($columns[4], [ref]$sites) -or $sites -lt 1) {
            throw "$where has a row whose sites column is not a positive integer: $line"
        }
        $verdict = Get-Verdict $columns[5]
        if (-not $verdict) { throw "$where has a row whose verdict is not std, follows <key>, unreviewed or violation: $line" }
        $key = $columns[0..3] -join "`t"
        if ($rows.ContainsKey($key)) { throw "$where carries $(Format-Key $key) on two rows; each key has one row" }
        $rows[$key] = [pscustomobject]@{ Sites = $sites; Verdict = $verdict; Reason = $columns[6] }
    }
    if (-not $seen) { throw "$where has no column header" }
    return $rows
}

function Test-Dated([string]$reason) {
    if ($reason -cnotmatch '^(?<date>\d{4}-\d{2}-\d{2}): \S') { return $false }
    $parsed = [DateTime]::MinValue
    return [DateTime]::TryParseExact($Matches['date'], 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture,
        [Globalization.DateTimeStyles]::None, [ref]$parsed)
}

function Get-Count($rows, [string]$verdict) {
    return @($rows.Values | Where-Object { $_.Verdict -ceq $verdict }).Count
}

if ($ListPath) {
    $now = Read-Sites ([IO.File]::ReadAllLines($ListPath)) $ListPath
    $before = Read-Sites ([IO.File]::ReadAllLines($BaselinePath)) $BaselinePath
    $from = "the baseline $BaselinePath"
} else {
    $list = Join-Path $repo $relative
    if (-not (Test-Path -LiteralPath $list)) {
        throw "$relative is not in the tree - bt-source's request_doors test holds the code to it"
    }
    Push-Location $repo
    try {
        $now = Read-Sites ([IO.File]::ReadAllLines($list)) "$relative (working tree)"
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
                Write-Host "the merge base $($base.Substring(0, 12)) has no $relative, and the seed $seed cannot be read in this clone."
                Write-Host "This is not a pass: the comparison did not happen."
                exit 2
            }
            $from = "the seed $($seed.Substring(0, 12))"
        }
        $before = Read-Sites ($text -split "`r?`n") "$relative at $from"
    } finally {
        Pop-Location
    }
}

$refused = @()
$allowed = 0
$shrunk = 0
foreach ($key in $now.Keys) {
    $row = $now[$key]
    $changes = @()
    if (-not $before.ContainsKey($key)) {
        $changes += "added ($($row.Sites) site(s), $($row.Verdict))"
    } else {
        $then = $before[$key]
        if ($row.Sites -gt $then.Sites) { $changes += "grown from $($then.Sites) to $($row.Sites) site(s)" }
        elseif ($row.Sites -lt $then.Sites) { $shrunk += 1 }
        if (($row.Verdict -ceq "unreviewed" -or $row.Verdict -ceq "violation") -and $row.Verdict -cne $then.Verdict) {
            $changes += "its verdict moved from $($then.Verdict) to $($row.Verdict)"
        }
    }
    if ($changes.Count -eq 0) { continue }
    $change = $changes -join ", "
    if ($before.ContainsKey($key) -and $row.Reason -ceq $before[$key].Reason) {
        $refused += "    $(Format-Key $key): $change, under the reason its row had at ${from} ('$($row.Reason)'): the change takes a new dated reason"
        continue
    }
    if (Test-Dated $row.Reason) {
        $allowed += 1
    } else {
        $refused += "    $(Format-Key $key): $change, reason '$($row.Reason)'"
    }
}
$removed = @($before.Keys | Where-Object { -not $now.ContainsKey($_) }).Count
Write-Host ("$relative against ${from}: $($before.Count) keys -> $($now.Count), " +
    "$allowed added, grown or re-judged with a dated reason, $($refused.Count) refused, $shrunk shrunk, $removed removed; " +
    "unreviewed $(Get-Count $before 'unreviewed') -> $(Get-Count $now 'unreviewed'), " +
    "violation $(Get-Count $before 'violation') -> $(Get-Count $now 'violation').")

if ($refused.Count -gt 0) {
    throw (
        "the request-door sites grew against ${from} with no dated reason:" + [Environment]::NewLine +
        ($refused -join [Environment]::NewLine) + [Environment]::NewLine +
        "Keep the door from reaching it (C1: a request door returns at once), or read the site and write " +
        "its verdict and 'YYYY-MM-DD: <why>' in the row's reason cell."
    )
}
Write-Host "the request-door sites only shrank, or grew with a dated reason. PASS"
exit 0
