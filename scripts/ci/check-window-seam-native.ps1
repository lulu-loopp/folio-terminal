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
# whatever else disappeared beside it.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When
# the merge base has no list (the branch that introduces it), the baseline is
# the seed below: the rows the list was introduced with. There is no "nothing
# to compare, so pass" road: no origin/main or no merge base is exit 2.
#
# It reads only the committed TSV, through git and the working tree.
# `.github/workflows/ci.yml` runs it in `logic`, and `gates-can-fail` plants a
# row (it must go red, naming it).

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/window-seam-native.tsv"
$list = Join-Path $repo $relative

# The rows the list was introduced with (T-HEADLESS-RUNTIME-SEAM round 2), by identity.
$seed = @(
    "method`tRuntime`tcreate",
    "method`tRuntime`topen_window",
    "trait`tFolioApp`tApplicationHandler",
    "function`tcrate`trebuild_every_native_surface",
    "const`tmain.rs`t_"
)

# The rows of one copy of the list, by identity (`kind`, `owner`, `name`).
function Read-NativeRows([string[]] $lines, [string] $what) {
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
    }
    return , $rows
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the window-seam guard holds the code to it"
}

Push-Location $repo
try {
    $now = Read-NativeRows ([IO.File]::ReadAllLines($list)) "$relative (working tree)"

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
        $before = Read-NativeRows ($text -split "`r?`n") "$relative at $from"
    } else {
        $from = "the list's seed (the merge base $($base.Substring(0, 12)) has none)"
        $before = [System.Collections.Generic.List[string]]::new()
        foreach ($row in $seed) { $before.Add($row) }
    }
} finally {
    Pop-Location
}

$added = @($now | Where-Object { -not $before.Contains($_) })
if ($added.Count -gt 0) {
    $named = ($added | ForEach-Object { "    " + ($_ -replace "`t", " ") }) -join [Environment]::NewLine
    throw "$relative only shrinks: $($added.Count) row(s) not at ${from}:$([Environment]::NewLine)$named$([Environment]::NewLine)Write the code for the backend parameter, through a door of the seam, rather than listing it."
}
Write-Host "$relative against ${from}: $($before.Count) rows -> $($now.Count), 0 added."
exit 0
