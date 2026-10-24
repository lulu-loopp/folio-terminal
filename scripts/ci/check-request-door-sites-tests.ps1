# The planted cases of check-request-door-sites.ps1: each list against a baseline, with the verdict
# the gate must give. An added or grown key, or a verdict moved to unreviewed or violation, without
# a dated reason is refused; with a new dated one, it passes; a change under the row's old reason is
# refused; a shrunk or removed key and a verdict moved to std passes; a malformed reason, row,
# kind, verdict or header is refused. The git road is planted in a scratch repository: a missing
# list, no origin/main, a merge base without the list read against the seed (and an unreadable
# seed) each give their verdict.

$ErrorActionPreference = "Stop"
$gate = Join-Path $PSScriptRoot "check-request-door-sites.ps1"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("request-door-sites-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $scratch | Out-Null

$header = "file`titem`tkind`tname`tsites`tverdict`treason"
$f = "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen"
$g = "crates/a/src/lib.rs`ta crate::g`tEffect`tstd::fs::write"
$base = @(
    "# baseline",
    $header,
    "$f`t2`tunreviewed`t",
    "$g`t1`tviolation`t2026-10-24: (a) the finding"
)

function Invoke-Case([string]$name, [string[]]$now, [bool]$passes, [string[]]$baseline = $base) {
    $listPath = Join-Path $scratch "$name.now.tsv"
    $basePath = Join-Path $scratch "$name.base.tsv"
    [IO.File]::WriteAllLines($listPath, $now)
    [IO.File]::WriteAllLines($basePath, $baseline)
    $output = & pwsh -NoProfile -File $gate -ListPath $listPath -BaselinePath $basePath 2>&1 | Out-String
    $passed = $LASTEXITCODE -eq 0
    if ($passed -ne $passes) {
        throw "case '$name': expected $(if ($passes) { 'a pass' } else { 'a refusal' }), got exit $LASTEXITCODE`n$output"
    }
    Write-Host "ok  $name"
}

try {
    Invoke-Case "unchanged" $base $true
    Invoke-Case "shrunk" @($header, "$f`t1`tunreviewed`t", $base[3]) $true
    Invoke-Case "removed" @($header, $base[3]) $true
    Invoke-Case "read-to-std" @($header, "$f`t2`tstd`t2026-10-24: a Vec's len", $base[3]) $true
    Invoke-Case "read-to-follows" @($header, "$f`t2`tfollows a crate::Lane::len`t2026-10-24: the lane's len", $base[3]) $true
    Invoke-Case "added-undated" ($base + "crates/a/src/lib.rs`ta crate::h`tDynamic`thook`t1`tunreviewed`t") $false
    Invoke-Case "added-dated" ($base + "crates/a/src/lib.rs`ta crate::h`tDynamic`thook`t1`tunreviewed`t2026-10-24: a stored hook, not read yet") $true
    Invoke-Case "added-impossible-date" ($base + "crates/a/src/lib.rs`ta crate::h`tDynamic`thook`t1`tstd`t2026-02-30: why") $false
    Invoke-Case "added-blank-why" ($base + "crates/a/src/lib.rs`ta crate::h`tDynamic`thook`t1`tstd`t2026-10-24: ") $false
    Invoke-Case "grown-undated" @($header, "$f`t3`tunreviewed`t", $base[3]) $false
    Invoke-Case "grown-dated" @($header, "$f`t3`tunreviewed`t2026-10-24: one more untyped len", $base[3]) $true
    Invoke-Case "grown-old-reason" @($header, $base[2], "$g`t2`tviolation`t2026-10-24: (a) the finding") $false
    $read = @("# baseline", $header, "$f`t2`tstd`t2026-10-20: a Vec's len", $base[3])
    Invoke-Case "to-unreviewed-undated" @($header, "$f`t2`tunreviewed`t", $base[3]) $false $read
    Invoke-Case "to-violation-old-reason" @($header, "$f`t2`tviolation`t2026-10-20: a Vec's len", $base[3]) $false $read
    Invoke-Case "to-violation-dated" @($header, "$f`t2`tviolation`t2026-10-24: (d) the receiver is a lock", $base[3]) $true $read
    Invoke-Case "wrong-header" @("file`titem`tkind`tname`tsites`treason", $base[2]) $false
    Invoke-Case "six-columns" @($header, "$f`t2`tunreviewed") $false
    Invoke-Case "bad-kind" @($header, "crates/a/src/lib.rs`ta crate::f`tCutoff`tx`t1`tunreviewed`t") $false
    Invoke-Case "bad-verdict" @($header, "$f`t2`treviewed`t2026-10-24: why", $base[3]) $false
    Invoke-Case "duplicate-key" @($header, $base[2], $base[2]) $false
    # The git road, in a scratch repository.
    $repo = Join-Path $scratch "repo"
    New-Item -ItemType Directory -Path (Join-Path $repo "docs/plans") | Out-Null
    $git = { param([string[]]$arguments) & git -C $repo @arguments *> $null; if ($LASTEXITCODE -ne 0) { throw "git $arguments" } }
    & $git @("init", "-q")
    & $git @("config", "user.email", "planted@example.invalid")
    & $git @("config", "user.name", "planted")
    $listFile = Join-Path $repo "docs/plans/REQUEST-DOOR-SITES.tsv"
    [IO.File]::WriteAllText((Join-Path $repo "README"), "planted`n")
    & $git @("add", "README")
    & $git @("commit", "-q", "-m", "base without the list")
    $baseCommit = (& git -C $repo rev-parse HEAD).Trim()

    function Invoke-Repo([string]$name, [string]$seed, [int]$expected, [string]$refusal = "") {
        $arguments = @("-NoProfile", "-File", $gate, "-Repo", $repo)
        if ($seed) { $arguments += @("-Seed", $seed) }
        $output = & pwsh @arguments 2>&1 | Out-String
        $code = $LASTEXITCODE
        $ok = if ($expected -eq 1) { $code -ne 0 -and $code -ne 2 -and $output.Contains($refusal) } else { $code -eq $expected }
        if (-not $ok) { throw "case '$name': expected exit $expected$(if ($refusal) { " with '$refusal'" }), got $code`n$output" }
        Write-Host "ok  $name"
    }

    Invoke-Repo "missing-list" "" 1 "is not in the tree"
    [IO.File]::WriteAllLines($listFile, $base)
    & $git @("add", "docs/plans/REQUEST-DOOR-SITES.tsv")
    & $git @("commit", "-q", "-m", "the list")
    $seedCommit = (& git -C $repo rev-parse HEAD).Trim()
    Invoke-Repo "no-origin-main" $seedCommit 2
    & $git @("update-ref", "refs/remotes/origin/main", $baseCommit)
    Invoke-Repo "seed-fallback-unchanged" $seedCommit 0
    [IO.File]::WriteAllLines($listFile, $base + "crates/a/src/lib.rs`ta crate::h`tDynamic`thook`t1`tunreviewed`t")
    Invoke-Repo "seed-fallback-added-undated" $seedCommit 1 "grew against the seed"
    Invoke-Repo "seed-unreadable" ("0" * 40) 2
    & $git @("update-ref", "refs/remotes/origin/main", $seedCommit)
    Invoke-Repo "merge-base-added-undated" $seedCommit 1 "grew against the merge base"

    Write-Host "check-request-door-sites.ps1: every planted case gave its verdict. PASS"
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
