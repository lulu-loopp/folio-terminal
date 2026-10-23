# Scratch-repository tests for check-env-doors.ps1: the no-base and unchanged
# passes, a shrink and a row that reads less pass, and each of the gate's three
# refusals fails for its own reason (an added row, a row reading a name its base
# row did not, a row holding more refused shapes than its base row).

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-env-doors.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-env-doors-gate-" + [guid]::NewGuid())
$relative = "docs/plans/ENV-DOORS.tsv"
$header = "# a comment the gate does not compare`nfile`titem`tcategory`tnames`trefused`tticket`treason`n"
$door = "crates/x/src/lib.rs`tcrate::door`tdoor`tBT_A,BT_B`t1`tT-X`ta door not moved yet`n"
$entry = "crates/x/src/main.rs`tcrate::main`tentry`tBT_C`t0`tT-X`tthe entry`n"

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

function Refused([string]$Text, [string]$Case, [string]$Reason) {
    Write-List $Text
    $output = Run-Gate 1 $Case
    # The refusal is a thrown error, which the host wraps at its width and decorates: compare
    # with the colours, the decorations and every blank taken out.
    $plain = ($output -replace '\x1b\[[0-9;]*m', '') -replace '[\s|]', ''
    if ($plain -notmatch [regex]::Escape(($Reason -replace '\s', ''))) {
        throw "$Case was refused for the wrong reason (wanted '$Reason'): $output"
    }
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email env-doors-gate@example.invalid
        & git config user.name env-doors-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        & git add .
        & git commit -q -m base
        $emptyBase = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $emptyBase

        Write-List ($header + $door + $entry)
        $output = Run-Gate 0 "no base list"
        if ($output -notmatch "the commit that introduces it") {
            throw "the no-base-list pass was not loud: $output"
        }

        & git add .
        & git commit -q -m list
        $base = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $base
        $output = Run-Gate 0 "unchanged"
        if ($output -notmatch "2 rows -> 2, 0 removed, 0 grown") { throw "the unchanged pass said: $output" }

        Write-List ($header + $door)
        Run-Gate 0 "a row removed" | Out-Null

        Write-List ($header + $door.Replace("BT_A,BT_B`t1", "BT_A`t0").Replace("`tdoor`t", "`tentry`t") + $entry)
        Run-Gate 0 "a row that reads less, its category rewritten" | Out-Null

        Refused ($header + $door + $entry + "crates/x/src/new.rs`tcrate::new`tdoor`tBT_D`t0`tT-X`ta new reader`n") `
            "an added row" "crates/x/src/new.rs crate::new is a row the base does not have"
        Refused ($header + $door.Replace("BT_A,BT_B", "BT_A,BT_B,BT_E") + $entry) `
            "a row reading a new name" "crates/x/src/lib.rs crate::door reads BT_E, which its base row does not"
        Refused ($header + $door.Replace("BT_A,BT_B`t1", "BT_A,BT_B`t2") + $entry) `
            "a row holding more refusals" "crates/x/src/lib.rs crate::door holds 2 refused shape(s), its base row 1"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-env-doors: no-base, unchanged, shrink, read-less, added-row, new-name and more-refusals cases pass"
exit 0
