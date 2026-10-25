# the_window_seam_native_list_only_shrinks
#
# `docs/plans/window-seam-native.tsv` lists every place bt-app names a backend
# holder (App, WindowRuntime, Runtime, FolioApp, ...) for the native backend,
# with why it cannot be written for the backend parameter
# (T-HEADLESS-RUNTIME-SEAM; ARCHITECTURE.md section 4.5, ARCHITECTURE-GUARDS.md
# "Window seam"). Within one tree, bt-source's `window_seam` test holds the code
# to it: every such place is a row, and every row is still used. This is the
# other half, across commits, as `check-census-unknowns.ps1` is for the census's
# unknowns: **the list only shrinks against the merge base.** A row's identity
# is its first three columns — `kind`, `owner`, `name`; its reason may be
# reworded. A row the merge base does not carry is refused, by identity,
# whatever else disappeared beside it - unless it is a base row **moved**
# (T-MOVED-NOT-ADDED-RULE, `moved-rows.ps1`). The guard's `owner` is a location
# for exactly two shapes: a `function` or `type` row whose owner is a module
# (`crate`, `crate::...`, or the guard's `crate, crate::...` for a file reached
# twice; no type is spelled so, `crate` being a keyword), and a `const` row,
# whose owner is its file (`....rs`). A row of one of those shapes whose kind and
# name are a gone base row's of the same shape, at another module or file, is
# that row moved - when it is the one row of that kind and name on each side and
# the old owner no longer lists it and the new one did not at the base (a
# same-named item taking over a place the old one dropped passes as moved, as
# check-dead-code's U-44 rule does: the check reads the list, not the tree). A `method` or `trait` row's owner is its
# type, which a move between files does not change: a changed owner there is
# another type, an added row. **A renamed row is accepted only with a dated
# reason** (the rename arm of `moved-rows.ps1`): a row the base does not carry
# whose kind and owner are exactly one gone base row's - the item renamed in
# place, and no other row of that kind and owner left or arrived - is that row
# renamed when its reason is `YYYY-MM-DD: <TICKET-ID> <why>` (a real day, new
# against the base row's). Without it, or with two candidates on a side, it is
# added and refused.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When
# the merge base has no list (the branch that introduces it), the baseline is
# the seed below: the rows the list was introduced with. There is no "nothing
# to compare, so pass" road: no origin/main or no merge base is exit 2.
#
# It reads only the committed TSV, through git and the working tree.
# `.github/workflows/ci.yml` runs it in `logic`, and `gates-can-fail` plants a
# row (it must go red, naming it). `check-window-seam-native-tests.ps1` runs it
# against scratch repositories (`-Repo`), one case per rule of the move.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$repo = $Repo
$relative = "docs/plans/window-seam-native.tsv"
$list = Join-Path $repo $relative

# The rows the list was introduced with (T-HEADLESS-RUNTIME-SEAM round 2), by identity; and
# `main`, which the guard reads since round 3 taught it expression position (`main` builds the
# native `FolioApp` by inference through `run_app`) — seeded in the commit that taught it.
$seed = @(
    "function`tcrate`tmain",
    "method`tRuntime`tcreate",
    "method`tRuntime`topen_window",
    "trait`tFolioApp`tApplicationHandler",
    "function`tcrate`trebuild_every_native_surface",
    "const`tmain.rs`t_"
)

# The rows of one copy of the list, by identity (`kind`, `owner`, `name`).
# The reason of each row read, by identity, for the rename arm.
$reasons = [Collections.Generic.Dictionary[string, string]]::new([StringComparer]::Ordinal)
function Read-NativeRows([string[]] $lines, [string] $what, [string] $side) {
    $rows = [System.Collections.Generic.List[string]]::new()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne "kind`towner`tname`treason") {
                throw "$what does not begin with the header kind/owner/name/reason: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 4) {
            throw "$what has a row that is not four columns: $line"
        }
        $rows.Add("$($columns[0])`t$($columns[1])`t$($columns[2])")
        $reasons["$side`t$($columns[0])`t$($columns[1])`t$($columns[2])"] = $columns[3]
    }
    return , $rows
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the window-seam guard holds the code to it"
}

Push-Location $repo
try {
    $now = Read-NativeRows ([IO.File]::ReadAllLines($list)) "$relative (working tree)" "now"

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        if ($LASTEXITCODE -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) rows and nothing to compare them against."
        Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
        exit 2
    }

    $text = (& git show "${base}:${relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -eq 0) {
        $from = "the merge base $($base.Substring(0, 12))"
        $before = Read-NativeRows ($text -split "`r?`n") "$relative at $from" "base"
    } else {
        $from = "the list's seed (the merge base $($base.Substring(0, 12)) has none)"
        $before = [System.Collections.Generic.List[string]]::new()
        foreach ($row in $seed) { $before.Add($row) }
    }
} finally {
    Pop-Location
}

# A row as the move rule reads it: its owner is its location when the guard writes a module or
# a file there, and the row is about its kind and name.
function Get-MoveRow([string]$row) {
    $kind, $owner, $name = $row.Split("`t")
    $located = (($kind -ceq "function" -or $kind -ceq "type") -and $owner -cmatch '^crate($|::|, )') -or
        $kind -ceq "const"
    return [pscustomobject]@{
        Identity = "$kind`t$name"
        Location = if ($located) { $owner } else { $null }
        Tail     = "$kind $name"
        Row      = $row
    }
}

$unmatched = @($now | Where-Object { -not $before.Contains($_) } | ForEach-Object { Get-MoveRow $_ })
$leftover = @($before | Where-Object { -not $now.Contains($_) } | ForEach-Object { Get-MoveRow $_ })
$tree = @($now | ForEach-Object { Get-MoveRow $_ })
$added = @()
$moves = 0
$paired = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$unpaired = @()
foreach ($result in (Find-MovedRows -Unmatched $unmatched -Leftover $leftover -Tree $tree -Base @($before | ForEach-Object { Get-MoveRow $_ }))) {
    $spelled = $result.Row -replace "`t", " "
    if ($result.Verdict -eq "moved") {
        $moves += 1
        [void]$paired.Add($result.From)
        Write-Host "moved     $spelled (from $($result.Was))"
    } elseif ($result.Verdict -eq "added") {
        $unpaired += $result.Row
    } else {
        $added += "    ${spelled}: $(Format-MoveRefusal $result)"
    }
}
# The rename arm (moved-rows.ps1): an added row whose kind and owner are one gone base row's, and
# no other added row's, is that row renamed - with a dated reason naming its ticket.
function Get-RenameRow([string]$row, [string]$side) {
    $kind, $owner, $name = $row.Split("`t")
    $reason = if ($reasons.ContainsKey("$side`t$row")) { $reasons["$side`t$row"] } else { $null }
    return [pscustomobject]@{ Group = "$kind`t$owner"; Row = $row; Reason = $reason }
}
$renames = 0
$gone = @($leftover | Where-Object { -not $paired.Contains($_.Row) } | ForEach-Object { Get-RenameRow $_.Row "base" })
foreach ($result in (Find-RenamedRows -Added @($unpaired | ForEach-Object { Get-RenameRow $_ "now" }) -Leftover $gone)) {
    $spelled = $result.Row -replace "`t", " "
    if ($result.Verdict -eq "renamed") {
        $wrong = Test-RenameReason $result.Reason $result.WasReason
        if ($wrong) {
            $added += "    ${spelled}: it would be $($result.From -replace "`t", " ") renamed, and $wrong"
        } else {
            $renames += 1
            Write-Host "renamed   $spelled (from $($result.From -replace "`t", " "))"
        }
    } elseif ($result.Verdict -eq "ambiguous") {
        $added += "    ${spelled}: more than one row of its kind and owner left or arrived, so no rename can be paired"
    } else {
        $added += "    $spelled"
    }
}
if ($added.Count -gt 0) {
    $named = $added -join [Environment]::NewLine
    throw "$relative only shrinks: $($added.Count) row(s) not at ${from}:$([Environment]::NewLine)$named$([Environment]::NewLine)Write the code for the backend parameter, through a door of the seam, rather than listing it."
}
Write-Host "$relative against ${from}: $($before.Count) rows -> $($now.Count), 0 added, $moves moved, $renames renamed."
exit 0
