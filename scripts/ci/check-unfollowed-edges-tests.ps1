# The planted cases of check-unfollowed-edges.ps1: each list against a baseline, with the verdict
# the gate must give. An added or grown key without a dated reason is refused; with one, it passes;
# a shrunk or removed key passes; a malformed reason (no date, an impossible date, a blank why) is
# refused; a malformed row is refused.

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
    Invoke-Case "swapped-same-count" @($header, "crates/a/src/lib.rs`ta crate::f`tAmbiguous`tlen`t2`t", "crates/a/src/lib.rs`ta crate::k`tDynamic`t(value)`t1`t") $false
    Invoke-Case "bad-kind" @($header, "crates/a/src/lib.rs`ta crate::f`tPath`tlen`t2`t") $false
    Invoke-Case "duplicate-key" @($header, $base[2], $base[2]) $false
    Write-Host "check-unfollowed-edges.ps1: every planted case gave its verdict. PASS"
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
