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

        Write-List ($header + $row + $row)
        $output = Run-Gate 1 "duplicate test"
        if ($output -notmatch "more than once") { throw "duplicate was refused for the wrong reason: $output" }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-timing-bound: no-base, unchanged, shrink, growth and duplicate cases pass"
exit 0
