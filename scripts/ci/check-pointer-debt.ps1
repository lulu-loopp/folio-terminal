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
# It passes, loudly, when the base has no list - that is how the commit that
# introduces the list passes. It does not pass when there is no base at all: no
# `origin/main` in the clone, or no merge base, means the comparison did not
# happen, and a check that did not run must not look like a check that agreed.
# Locally it is one `git fetch origin main` away.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/POINTER-DEBT.tsv"
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the debt list is what every_pointer_read_is_the_routers_or_a_captures allows readers against"
}

# A row is a data line: not blank, not a comment, not the column header.
function Read-DebtRows([string[]]$lines) {
    $rows = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) { $header = $true; continue }
        $rows += $line
    }
    if (-not $header) { throw "$relative has no column header" }
    return , $rows
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
$allowed = @{}
foreach ($row in $before) {
    if ($allowed.ContainsKey($row)) { $allowed[$row] += 1 } else { $allowed[$row] = 1 }
}

$added = @()
foreach ($row in $now) {
    if ($allowed.ContainsKey($row) -and $allowed[$row] -gt 0) {
        $allowed[$row] -= 1
    } else {
        $added += $row
    }
}

$removed = $before.Count - ($now.Count - $added.Count)
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed."

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
