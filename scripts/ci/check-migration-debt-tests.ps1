# Scratch-repository tests for check-migration-debt.ps1: the debt list only shrinks, judged by
# the six identity columns found by name, so a column added or moved beside them is no new row.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-migration-debt.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-migration-debt-" + [guid]::NewGuid())
$relative = "docs/plans/MIGRATION-DEBT.tsv"
$header = "file`towner`tmechanism`tsubject`tticket`tdisturbed_by_2a`tneeded_seam`tverdict`n"
$row = "crates/bt-a/src/lib.rs`ttests::reads`tinclude_str!`tlib.rs`tP3`tno`tan item query`tDEBT`n"
$other = "crates/bt-a/src/lib.rs`ttests::writes`tinclude_str!`tlib.rs`tP3`tno`tan item query`tDEBT`n"
# The same two rows with a `category` column among the identity columns, and with the
# explanation's two columns in the other order: a gate that read the first six fields by position
# would see two new rows.
$categorised = "file`towner`tmechanism`tsubject`tcategory`tticket`tdisturbed_by_2a`tverdict`tneeded_seam`n"
$rowCategorised = "crates/bt-a/src/lib.rs`ttests::reads`tinclude_str!`tlib.rs`texception`tP3`tno`tDEBT`tan item query`n"
$otherCategorised = "crates/bt-a/src/lib.rs`ttests::writes`tinclude_str!`tlib.rs`texception`tP3`tno`tDEBT`tan item query`n"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email migration-debt@example.invalid
        & git config user.name migration-debt
        & git config core.autocrlf false
        Write-List ($header + $row + $other)
        & git add .
        & git commit -q -m base
        $base = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $base

        $output = Run-Gate 0 "unchanged"
        if ($output -notmatch "2 rows -> 2, 0 added, 0 removed") { throw "the unchanged list was not compared: $output" }

        Write-List ($categorised + $rowCategorised + $otherCategorised)
        $output = Run-Gate 0 "a column added and the explanation reordered"
        if ($output -notmatch "2 rows -> 2, 0 added, 0 removed") {
            throw "a new column made a retained row look added: $output"
        }

        Write-List ($categorised + $rowCategorised)
        $output = Run-Gate 0 "shrunk"
        if ($output -notmatch "2 rows -> 1, 0 added, 1 removed") { throw "the removal was not counted: $output" }

        Write-List ($categorised + $rowCategorised + $otherCategorised +
            "crates/bt-a/src/lib.rs`ttests::planted`tinclude_str!`tlib.rs`texception`tP3`tno`tDEBT`tan item query`n")
        $output = Run-Gate 1 "added row"
        if ($output -notmatch "only shrinks" -or $output -notmatch "tests::planted") {
            throw "the addition was refused for the wrong reason: $output"
        }

        Write-List ($categorised + $rowCategorised + $otherCategorised.Replace("tests::writes", "tests::rewritten"))
        $output = Run-Gate 1 "an identity edited in place"
        if ($output -notmatch "only shrinks" -or $output -notmatch "tests::rewritten") {
            throw "the edited identity was refused for the wrong reason: $output"
        }

        Write-List ($categorised + $rowCategorised + $rowCategorised + $otherCategorised)
        $output = Run-Gate 1 "duplicated row"
        if ($output -notmatch "only shrinks") { throw "the duplicate was refused for the wrong reason: $output" }

        Write-List ("file`towner`tmechanism`tsubject`tticket`tcategory`tverdict`n" +
            "crates/bt-a/src/lib.rs`ttests::reads`tinclude_str!`tlib.rs`tP3`texception`tDEBT`n")
        $output = Run-Gate 1 "an identity column gone"
        if ($output -notmatch "no ``disturbed_by_2a`` column") {
            throw "a header without an identity column was refused for the wrong reason: $output"
        }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-migration-debt: unchanged, new column, shrink, growth, in-place edit, duplicate and missing-column cases pass"
exit 0
