# The planted cases of check-unfollowed-edges.ps1: each list against a baseline, with the verdict
# the gate must give. An added or grown key without a dated reason is refused; with one, it passes;
# a grown key under its old reason is refused; a shrunk or removed key passes; a malformed reason
# (no date, an impossible date, a blank why) is refused; a malformed row or header is refused. The
# git road is planted in a scratch repository: a missing list, no origin/main, a merge base without
# the list read against the seed (and an unreadable seed) each give their verdict.

$ErrorActionPreference = "Stop"
$gate = Join-Path $PSScriptRoot "check-unfollowed-edges.ps1"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("unfollowed-edges-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $scratch | Out-Null

$header = "file`titem`tkind`tname`tsites`treason"
$base = @(
    "# baseline",
    $header,
    "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t2`t",
    "crates/a/src/lib.rs`ta crate::g`tDynamic`t(value)`t1`t"
)

function Invoke-Case([string]$name, [string[]]$now, [bool]$passes) {
    $listPath = Join-Path $scratch "$name.now.tsv"
    $basePath = Join-Path $scratch "$name.base.tsv"
    [IO.File]::WriteAllLines($listPath, $now)
    [IO.File]::WriteAllLines($basePath, $base)
    $output = & pwsh -NoProfile -File $gate -ListPath $listPath -BaselinePath $basePath 2>&1 | Out-String
    $passed = $LASTEXITCODE -eq 0
    if ($passed -ne $passes) {
        throw "case '$name': expected $(if ($passes) { 'a pass' } else { 'a refusal' }), got exit $LASTEXITCODE`n$output"
    }
    Write-Host "ok  $name"
}

try {
    Invoke-Case "unchanged" $base $true
    Invoke-Case "shrunk" @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t1`t") $true
    Invoke-Case "added-undated" ($base + "crates/a/src/lib.rs`ta crate::h`tAmbiguous`tget`t1`t") $false
    Invoke-Case "added-dated" ($base + "crates/a/src/lib.rs`ta crate::h`tAmbiguous`tget`t1`t2026-10-18: a closure the walk cannot type") $true
    Invoke-Case "added-impossible-date" ($base + "crates/a/src/lib.rs`ta crate::h`tAmbiguous`tget`t1`t2026-02-30: why") $false
    Invoke-Case "added-blank-why" ($base + "crates/a/src/lib.rs`ta crate::h`tAmbiguous`tget`t1`t2026-10-18: ") $false
    Invoke-Case "grown-undated" @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t3`t", $base[3]) $false
    Invoke-Case "grown-dated" @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t3`t2026-10-18: one more untyped len", $base[3]) $true
    $dated = @("# baseline", $header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t2`t2026-10-01: two untyped len", $base[3])
    $listPath = Join-Path $scratch "grown-old-reason.now.tsv"
    $basePath = Join-Path $scratch "grown-old-reason.base.tsv"
    [IO.File]::WriteAllLines($listPath, @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t3`t2026-10-01: two untyped len", $base[3]))
    [IO.File]::WriteAllLines($basePath, $dated)
    & pwsh -NoProfile -File $gate -ListPath $listPath -BaselinePath $basePath *> $null
    if ($LASTEXITCODE -eq 0) { throw "case 'grown-old-reason': a key grown under its old dated reason passed" }
    Write-Host "ok  grown-old-reason"
    Invoke-Case "wrong-header" @("file`titem`tkind`tname`tcount`treason", $base[2]) $false
    Invoke-Case "swapped-same-count" @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t2`t", "crates/a/src/lib.rs`ta crate::k`tDynamic`t(value)`t1`t") $false
    Invoke-Case "bad-kind" @($header, "crates/a/src/lib.rs`ta crate::f`tPath`tlen`t2`t") $false
    Invoke-Case "duplicate-key" @($header, $base[2], $base[2]) $false
    # The git road, in a scratch repository.
    $repo = Join-Path $scratch "repo"
    New-Item -ItemType Directory -Path (Join-Path $repo "docs/plans") | Out-Null
    $git = { param([string[]]$arguments) & git -C $repo @arguments *> $null; if ($LASTEXITCODE -ne 0) { throw "git $arguments" } }
    & $git @("init", "-q")
    & $git @("config", "user.email", "planted@example.invalid")
    & $git @("config", "user.name", "planted")
    $listFile = Join-Path $repo "docs/plans/UNFOLLOWED-EDGES.tsv"
    [IO.File]::WriteAllText((Join-Path $repo "README"), "planted`n")
    & $git @("add", "README")
    & $git @("commit", "-q", "-m", "base without the list")
    $baseCommit = (& git -C $repo rev-parse HEAD).Trim()

    function Invoke-Repo([string]$name, [string]$seed, [int]$expected) {
        $arguments = @("-NoProfile", "-File", $gate, "-Repo", $repo)
        if ($seed) { $arguments += @("-Seed", $seed) }
        $output = & pwsh @arguments 2>&1 | Out-String
        $code = $LASTEXITCODE
        $ok = if ($expected -eq 1) { $code -ne 0 -and $code -ne 2 } else { $code -eq $expected }
        if (-not $ok) { throw "case '$name': expected exit $expected, got $code`n$output" }
        Write-Host "ok  $name"
    }

    Invoke-Repo "missing-list" "" 1
    [IO.File]::WriteAllLines($listFile, $base)
    & $git @("add", "docs/plans/UNFOLLOWED-EDGES.tsv")
    & $git @("commit", "-q", "-m", "the list")
    $seedCommit = (& git -C $repo rev-parse HEAD).Trim()
    Invoke-Repo "no-origin-main" $seedCommit 2
    & $git @("update-ref", "refs/remotes/origin/main", $baseCommit)
    Invoke-Repo "seed-fallback-unchanged" $seedCommit 0
    [IO.File]::WriteAllLines($listFile, $base + "crates/a/src/lib.rs`ta crate::h`tAmbiguous`tget`t1`t")
    Invoke-Repo "seed-fallback-added-undated" $seedCommit 1
    Invoke-Repo "seed-unreadable" ("0" * 40) 2
    & $git @("update-ref", "refs/remotes/origin/main", $seedCommit)
    Invoke-Repo "merge-base-added-undated" $seedCommit 1

    Write-Host "check-unfollowed-edges.ps1: every planted case gave its verdict. PASS"
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
