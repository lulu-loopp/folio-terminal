# Scratch-repository tests for check-timing-bound.ps1.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-timing-bound.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-timing-gate-" + [guid]::NewGuid())
$relative = "docs/plans/TIMING-BOUND-TESTS.tsv"
$header = "test`tcrate`twall_clock_assumption`tdeterministic_seam`n"
$row = "crate::tests::old`tcrate`tanswer arrives within 1 s`tcontrolled receiver`n"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Write-Scan([string]$Text) {
    $path = Join-Path $root "crates/bt-source/tests/timing.rs"
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
        & git config user.email timing-gate@example.invalid
        & git config user.name timing-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        Write-Scan "the scan"
        & git add .
        & git commit -q -m base
        $emptyBase = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $emptyBase

        Write-List ($header + $row)
        $output = Run-Gate 0 "no base list"
        if ($output -notmatch "no base list exists" -or $output -notmatch "PASS") {
            throw "the no-base-list pass was not loud: $output"
        }

        & git add .
        & git commit -q -m census
        $base = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $base
        Run-Gate 0 "unchanged" | Out-Null

        Write-List $header
        Run-Gate 0 "shrunk" | Out-Null

        Write-List ($header + $row + "crate::tests::new`tcrate`tanswer arrives within 2 s`tcontrolled receiver`n")
        $output = Run-Gate 1 "added row"
        if ($output -notmatch "only shrinks") { throw "addition was refused for the wrong reason: $output" }

        $new = "crate::tests::new`tcrate`tanswer arrives within 2 s`tcontrolled receiver`n"
        $other = "crate::tests::other`tcrate`tanswer arrives within 3 s`tcontrolled receiver`n"
        $widened = "# widened 2026-10-24 by T-PLANT (+2 rows): a planted shape`n"
        Write-List ($widened + $header + $row + $new + $other)
        $output = Run-Gate 1 "widening with the scan unchanged"
        if ($output -notmatch "unchanged scan") { throw "a widening without a scan change was refused for the wrong reason: $output" }

        Write-Scan "the widened scan"
        $output = Run-Gate 0 "widening with its count"
        if ($output -notmatch "admits its 2 row") { throw "the widening pass was not loud: $output" }

        Write-List ("# widened 2026-10-24 by T-PLANT (+1 row): a planted shape`n" + $header + $row + $new + $other)
        $output = Run-Gate 1 "widening with the wrong count"
        if ($output -notmatch "admits 1 row\(s\) and 2 were added") { throw "the wrong count was refused for the wrong reason: $output" }

        Write-List ($widened + $header + $row)
        $output = Run-Gate 1 "widening with nothing added"
        if ($output -notmatch "admits 2 row\(s\) and 0 were added") { throw "the empty widening was refused for the wrong reason: $output" }

        Write-List ("# widened 2026-10-24 by T-PLANT (+0 rows): a planted shape`n" + $header + $row)
        $output = Run-Gate 1 "widening of no rows"
        if ($output -notmatch "empty widening") { throw "the +0 widening was refused for the wrong reason: $output" }

        Write-List ($widened + "# widened 2026-10-25 by T-OTHER (+0 rows): another`n" + $header + $row + $new + $other)
        $output = Run-Gate 1 "two widenings"
        if ($output -notmatch "2 widening lines are new") { throw "two widenings were refused for the wrong reason: $output" }

        Write-List ($widened + $header + $row)
        & git add .
        & git commit -q -m widened-base
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Write-List ($widened + $header + $row + $new + $other)
        $output = Run-Gate 1 "widening line already at the base"
        if ($output -notmatch "only shrinks") { throw "a base widening admitted rows: $output" }

        Write-List ($header + $row + $row)
        $output = Run-Gate 1 "duplicate test"
        if ($output -notmatch "more than once") { throw "duplicate was refused for the wrong reason: $output" }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-timing-bound: no-base, unchanged, shrink, growth, widening (scan unchanged, exact, wrong count, empty, +0, two, already at base) and duplicate cases pass"
exit 0
