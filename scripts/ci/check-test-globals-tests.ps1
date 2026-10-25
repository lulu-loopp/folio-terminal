# Scratch-repository tests for check-test-globals.ps1.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-test-globals.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-test-globals-gate-" + [guid]::NewGuid())
$relative = "docs/plans/TEST-GLOBALS.tsv"
$header = "file`tglobal`tbinary`tclass`treason`n"
$row = "a.rs`tcrate::OLD`tp:lib:p`tlock:TURN`t2026-10-24: every test takes TURN`n"
$checkedRow = "a.rs`tcrate::LOCKED`tp:lib:p`tlock:TURN`t2026-10-25: every test takes TURN`n"
$writerRow = "a.rs`tcrate::ONE`tp:lib:p`tsingle writer`t2026-10-25: one test writes it`n"
$handRow = "a.rs`tcrate::GROWS`tp:lib:p`tmonotone`t2026-10-25: only ever grows`n"
$otherHand = "a.rs`tcrate::KEYED`tp:lib:p`tper-test key`t2026-10-25: keyed by each test's path`n"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Write-Scan([string]$Text) {
    $path = Join-Path $root "crates/bt-source/tests/test_globals.rs"
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root -Relative $relative 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email test-globals-gate@example.invalid
        & git config user.name test-globals-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        Write-Scan "the scan"
        & git add .
        & git commit -q -m base
        $emptyBase = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $emptyBase

        Write-List ($header + $row)
        $output = Run-Gate 0 "no base register"
        if ($output -notmatch "no base register exists" -or $output -notmatch "PASS") {
            throw "the no-base-register pass was not loud: $output"
        }

        & git add .
        & git commit -q -m register
        $base = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $base
        Run-Gate 0 "unchanged" | Out-Null

        Write-List $header
        Run-Gate 0 "shrunk" | Out-Null

        Write-List ($header + $row + $checkedRow + $writerRow)
        $output = Run-Gate 0 "added rows of a checked class"
        if ($output -notmatch "admitted, its class is checked" -or $output -notmatch "only by rows the guard checks") {
            throw "the checked-class pass was not loud: $output"
        }

        Write-List ($header + $row + $handRow)
        $output = Run-Gate 1 "added by-hand row"
        if ($output -notmatch "a by-hand row only shrinks") { throw "a by-hand addition was refused for the wrong reason: $output" }

        Write-List ($header + "a.rs`tcrate::OLD`tp:lib:p`tmonotone`t2026-10-24: every test takes TURN`n")
        $output = Run-Gate 1 "a checked row turned by-hand"
        if ($output -notmatch "a by-hand row only shrinks") { throw "a class change to by-hand was refused for the wrong reason: $output" }

        Write-List ($header + $row + "a.rs`tcrate::LOCKED`tp:lib:p`tlock:TURN`tevery test takes TURN`n")
        $output = Run-Gate 1 "undated reason"
        if ($output -notmatch "not dated") { throw "an undated reason was refused for the wrong reason: $output" }

        Write-List ($header + $row + "a.rs`tcrate::LOCKED`tp:lib:p`tfine`t2026-10-25: it is`n")
        $output = Run-Gate 1 "unknown class"
        if ($output -notmatch "has no class 'fine'") { throw "an unknown class was refused for the wrong reason: $output" }

        $widened = "# widened 2026-10-25 by T-PLANT (+2 rows): a planted shape`n"
        Write-List ($widened + $header + $row + $handRow + $otherHand + $checkedRow)
        Write-Scan "an edit not committed"
        $output = Run-Gate 1 "widening with the scan unchanged in HEAD"
        if ($output -notmatch "unchanged scan") { throw "a widening without a scan change was refused for the wrong reason: $output" }

        Write-Scan "the widened scan"
        & git add crates/bt-source/tests/test_globals.rs
        & git commit -q -m widened-scan
        $output = Run-Gate 0 "widening with its count, beside a checked row"
        if ($output -notmatch "admits its 2 by-hand row") { throw "the widening pass was not loud: $output" }

        Write-List ("# widened 2026-10-25 by T-PLANT (+1 row): a planted shape`n" + $header + $row + $handRow + $otherHand)
        $output = Run-Gate 1 "widening with the wrong count"
        if ($output -notmatch "admits 1 by-hand row\(s\) and 2 were added") { throw "the wrong count was refused for the wrong reason: $output" }

        Write-List ($widened + $header + $row + $checkedRow)
        $output = Run-Gate 1 "widening that a checked row does not use"
        if ($output -notmatch "admits 2 by-hand row\(s\) and 0 were added") { throw "a widening over checked rows only was refused for the wrong reason: $output" }

        Write-List ("# widened 2026-10-25 by T-PLANT (+0 rows): a planted shape`n" + $header + $row)
        $output = Run-Gate 1 "widening of no rows"
        if ($output -notmatch "empty widening") { throw "the +0 widening was refused for the wrong reason: $output" }

        Write-List ($widened + "# widened 2026-10-26 by T-OTHER (+0 rows): another`n" + $header + $row + $handRow + $otherHand)
        $output = Run-Gate 1 "two widenings"
        if ($output -notmatch "2 widening lines are new") { throw "two widenings were refused for the wrong reason: $output" }

        Write-List ($widened + $header + $row)
        & git add .
        & git commit -q -m widened-base
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Write-List ($widened + $header + $row + $handRow + $otherHand)
        $output = Run-Gate 1 "widening line already at the base"
        if ($output -notmatch "a by-hand row only shrinks") { throw "a base widening admitted rows: $output" }

        Write-List ($header + $row + $row)
        $output = Run-Gate 1 "duplicate row"
        if ($output -notmatch "more than once") { throw "duplicate was refused for the wrong reason: $output" }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-test-globals: no-base, unchanged, shrink, checked growth, by-hand growth, class turned by-hand, undated, unknown class, widening (scan unchanged in HEAD though edited, exact beside a checked row, wrong count, unused, +0, two, already at base) and duplicate cases pass"
exit 0
