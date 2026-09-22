# the_migration_debt_list_only_shrinks
#
# `docs/plans/MIGRATION-DEBT.tsv` is one of the two lists of
# `docs/plans/bt-app-split-prep.md` §6.1: every source reader in this workspace
# that still names a file, one row each. Its whole value is the direction it
# moves in. A ticket that migrates a reader deletes its row; nothing adds one.
# `crates/bt-source/tests/tripwire.rs` is what refuses a reader on neither list,
# and this is what refuses a reader that was *put* on this one to get past it.
#
# The comparison is against the pull request's merge base, not against the
# previous commit, so a branch is judged on what it did rather than on what main
# did underneath it. Rows are compared whole and with multiplicity: a duplicated
# row is an added row. The commented header is not compared, so the prose above
# the list can be rewritten freely.
#
# It passes, loudly, when the base has no list — that is how the commit that
# introduces the list passes. It does not pass when there is no base at all: no
# `origin/main` in the clone, or no merge base, means the comparison did not
# happen, and a check that did not run must not look like a check that agreed.
# CI never reaches that: both jobs that run it check out with `fetch-depth: 0`.
# Locally it is one `git fetch origin main` away.
#
# Prove it fires before trusting it: add a row to the list and this must go red.
# `.github/workflows/ci.yml` does exactly that, in `gates-can-fail`.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/MIGRATION-DEBT.tsv"
$list = Join-Path $repo $relative

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the debt list is what the tripwire allows readers against"
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
        # git's own status, read before any other stage can stand on it. Piping
        # a native command into `Select-Object -First 1` stops it mid-stream,
        # and Windows PowerShell 5.1 answers that stop by throwing the output
        # away and setting $LASTEXITCODE to -1. This check spent that reading as
        # "no merge base" and exited green on every clone that had one, under
        # the very shell the ticket briefs invoke it with.
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
        "A row is removed by the ticket that migrates its reader (docs/plans/bt-app-split-prep.md " +
        "section 6.1). A new reader asks the crate about the item instead; and if its subject " +
        "really is a file, it takes a bt_source::FileScoped variant whose doc comment says why."
    )
}
