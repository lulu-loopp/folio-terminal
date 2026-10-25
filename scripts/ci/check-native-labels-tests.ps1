# Scratch-repository tests for check-native-labels.ps1.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-native-labels.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-native-labels-gate-" + [guid]::NewGuid())
$relative = "crates/bt-source/tests/native_labels.rs"

# A guard: its grammar line, its widening lines, its ALLOWED and MACROS rows, and its BOUNDARY and
# UNREAD rows - which -Old leaves out, as the guard was before T-NATIVE-LABELS-GUARD-REACH.
function Write-Guard([string]$Grammar = "fn judge() {}", [string[]]$Widened = @(), [string[]]$Allowed = @(), [string[]]$Macros = @(),
    [switch]$Old, [string[]]$Boundary = @(), [string[]]$Unread = @()) {
    $nl = "`n"
    $text = "//! the guard$nl" + $Grammar + $nl
    foreach ($line in $Widened) { $text += $line + $nl }
    $text += "const ALLOWED: &[(&str, &str, &str)] = &[$nl"
    foreach ($row in $Allowed) { $text += "    $row,$nl" }
    $text += "];$nl$nl"
    $text += "const MACROS: &[(&str, &str)] = &[$nl"
    foreach ($row in $Macros) { $text += "    $row,$nl" }
    $text += "];$nl"
    if (-not $Old) {
        $text += "const BOUNDARY: &[(&str, &str)] = &[$nl"
        foreach ($row in $Boundary) { $text += "    $row,$nl" }
        $text += "];$nl"
        $text += "const UNREAD: &[(&str, &str, &str)] = &[$nl"
        foreach ($row in $Unread) { $text += "    $row,$nl" }
        $text += "];$nl"
    }
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $text)
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root -Relative $relative 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

function Expect([string]$Output, [string]$Pattern, [string]$Case) {
    if ($Output -notmatch $Pattern) { throw "$Case gave the wrong verdict (wanted /$Pattern/): $Output" }
}

$json = '("src/lib.rs", "JSON", "a format''s name")'
$entity = '("src/lib.rs", "&amp;", "an entity")'
$widened1 = "// widened 2026-10-25 by T-PLANT (+1 row): a planted shape"

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email native-labels-gate@example.invalid
        & git config user.name native-labels-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        & git add .
        & git commit -q -m empty
        $empty = (& git rev-parse HEAD).Trim()

        Write-Guard -Allowed $json
        Run-Gate 2 "no origin/main" | Out-Null
        & git update-ref refs/remotes/origin/main $empty

        Expect (Run-Gate 0 "no base guard") "no base lists exist" "no base guard"

        Write-Guard -Allowed $json -Old
        & git add .
        & git commit -q -m guard
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Write-Guard -Allowed $json
        Expect (Run-Gate 0 "unchanged, with the lists the base lacks empty") "0 added, 0 removed" "unchanged"

        Write-Guard -Allowed '("src/lib.rs", "JSON", "a reason reworded")'
        Expect (Run-Gate 0 "reason reworded") "0 added, 0 removed" "reason reworded"

        Write-Guard
        Expect (Run-Gate 0 "shrunk") "1 removed" "shrunk"

        Write-Guard -Allowed $json, $entity
        Expect (Run-Gate 1 "grown with no line") "only shrink" "grown with no line"

        Write-Guard -Allowed $json -Macros '("label", "a listed macro")'
        Expect (Run-Gate 1 "a MACROS row grown with no line") "MACROS label" "a MACROS row grown with no line"

        Write-Guard -Allowed $json -Boundary '("bt-platform crate::Gone::push", "a planted stop")'
        Expect (Run-Gate 1 "a BOUNDARY row grown with no line") "BOUNDARY bt-platform crate::Gone::push" "a BOUNDARY row grown with no line"

        Write-Guard -Allowed $json -Unread '("bt-platform crate::f", "g", "a planted review")'
        Expect (Run-Gate 1 "an UNREAD row grown with no line") "UNREAD bt-platform crate::f g" "an UNREAD row grown with no line"

        Write-Guard -Widened $widened1 -Allowed $json, $entity
        Expect (Run-Gate 1 "grown with a line, the grammar unchanged") "unchanged grammar" "grown with a line, the grammar unchanged"

        Write-Guard -Grammar "fn judge() { widened(); }" -Widened $widened1 -Allowed $json, $entity
        Expect (Run-Gate 0 "grown with a line") "admits its 1 row" "grown with a line"

        Write-Guard -Grammar "fn judge() { widened(); }" -Widened "// widened 2026-10-25 by T-PLANT (+2 rows): a planted shape" -Allowed $json, $entity
        Expect (Run-Gate 1 "a line with the wrong count") "admits 2 row\(s\) and 1 were added" "a line with the wrong count"

        Write-Guard -Grammar "fn judge() { widened(); }" -Widened "// widened 2026-10-25 by T-PLANT (+0 rows): nothing" -Allowed $json
        Expect (Run-Gate 1 "a +0 line") "empty widening" "a +0 line"

        Write-Guard -Grammar "fn judge() { widened(); }" -Widened $widened1, "// widened 2026-10-26 by T-OTHER (+1 row): another" -Allowed $json, $entity
        Expect (Run-Gate 1 "two lines") "2 widening lines are new" "two lines"

        Write-Guard -Allowed '("src/pickers.rs", "JSON", "a format''s name")'
        Expect (Run-Gate 0 "moved") "moved\s+ALLOWED src/pickers.rs JSON \(from src/lib.rs\)" "moved"

        Write-Guard -Allowed '("src/pickers.rs", "JSON", "one")', '("src/pickers.rs", "JSON", "two")'
        Expect (Run-Gate 1 "moved and grown") "a move does not grow" "moved and grown"

        Write-Guard -Allowed '("src/lib.rs", "JSON")'
        Expect (Run-Gate 1 "a malformed row") "not a row of 3 strings" "a malformed row"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-native-labels: no origin/main, no base, unchanged, reworded reason, shrink, lists the base lacks, growth (ALLOWED, MACROS, BOUNDARY, UNREAD), widening (grammar unchanged, exact, wrong count, +0, two), moved, moved and grown and malformed-row cases pass"
exit 0
