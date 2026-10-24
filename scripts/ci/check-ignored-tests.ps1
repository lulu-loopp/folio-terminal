<#
.SYNOPSIS
    The ignored-test gate: this tree's ignored tests are exactly the ones on the list.

.DESCRIPTION
    The gate this replaces asked `cargo test -- --ignored --list` for its output
    and passed when that output matched `0 tests, 0 benchmarks`. Two things were
    wrong with it and both are the same kind of wrong — it could not fail, and it
    could not tell you why:

      * The workspace has several test binaries. `--list` prints a summary line
        per binary, so ANY binary with nothing ignored printed the magic phrase
        and the gate passed with twenty ignored tests standing.
      * It never looked at cargo's exit code. A workspace that did not compile
        produced no matching phrase either, which the gate read as a violation
        and reported as "ignored tests are not allowed" — the one message
        guaranteed not to lead anybody to the compile error.

    And the policy behind it was wrong too. This repository keeps ignored tests
    on purpose: nineteen dev probes in `bt-pty` that drive real ConPTY sessions
    with host-sensitive timing, and one in `bt-app` that writes a picture. "No
    ignored tests" would mean deleting working diagnostics to satisfy a gate.

    So the gate is an allowlist and a two-way comparison. A test that starts
    being skipped is red because it is not on the list; a test that stops
    existing — deleted, renamed, or quietly un-ignored — is red because the list
    names something that is gone. Neither direction can be satisfied by
    accident, and every change to the set has to be a change to the file, in the
    same commit, where a reviewer sees it.

.PARAMETER Allowlist
    The file naming every test that may be `#[ignore]`d. One name per line;
    blank lines and `#` comments are skipped.

    2026-10-24 (T-TEST-ORDER-STRAYS): a name may be followed by a tab and the
    run condition the test needs, `run=serial` or `run=alone`, and by nothing
    else. Only the name takes part in the comparison; the condition is checked
    for its spelling here and read by the person who runs the test. Any other
    second field, or a third, is refused by line.
#>

[CmdletBinding()]
param(
    [string] $Allowlist = (Join-Path $PSScriptRoot 'ignored-tests.txt')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# The harness is asked what it will skip rather than the source being grepped for
# the attribute: a grep misses `#[ignore = "..."]`, matches the word inside a
# string literal, and cannot see a test that a `cfg` removed.
$output = @(cargo test --workspace --locked --color never -- --ignored --list 2>&1 |
    ForEach-Object { "$_" })
if ($LASTEXITCODE -ne 0) {
    $output | Write-Host
    throw "listing ignored tests failed: cargo exited $LASTEXITCODE"
}

# `--list` prints `path::to::test: test` for each one, plus a summary line per
# binary that this pattern does not match.
$found = @(
    $output |
        Where-Object { $_ -match ':\s+test$' } |
        ForEach-Object { ($_ -replace ':\s+test$', '').Trim() } |
        Sort-Object -Unique
)

if (-not (Test-Path -LiteralPath $Allowlist)) {
    throw "the allowlist is missing: $Allowlist"
}
# A row is `name` or `name<TAB>run=serial|alone` (see .PARAMETER Allowlist).
$conditions = @('run=serial', 'run=alone')
$rows = @(Get-Content -LiteralPath $Allowlist)
$names = [System.Collections.Generic.List[string]]::new()
$malformed = [System.Collections.Generic.List[string]]::new()
for ($line = 0; $line -lt $rows.Count; $line++) {
    $row = $rows[$line].Trim()
    if (-not $row -or $row.StartsWith('#')) {
        continue
    }
    $fields = $row -split "`t"
    if ($fields.Count -gt 2 -or ($fields.Count -eq 2 -and $fields[1] -cnotin $conditions)) {
        $malformed.Add("  line $($line + 1): $row")
        continue
    }
    $names.Add($fields[0].Trim())
}
if ($malformed.Count -gt 0) {
    $malformed | Write-Host
    throw "$($malformed.Count) row(s) of $Allowlist are not a name, or a name, a tab and one of $($conditions -join ', ')"
}
$expected = @($names | Sort-Object -Unique)

$difference = @(Compare-Object -ReferenceObject $expected -DifferenceObject $found)
if ($difference.Count -gt 0) {
    Write-Host "the set of ignored tests is not the set on the list ($Allowlist):"
    foreach ($entry in $difference) {
        if ($entry.SideIndicator -eq '=>') {
            Write-Host "  + $($entry.InputObject)   (newly ignored, and not on the list)"
        }
        else {
            Write-Host "  - $($entry.InputObject)   (on the list, and no longer ignored or no longer there)"
        }
    }
    Write-Host ''
    Write-Host 'If the change is intended, edit the list in the same commit.'
    throw "$($difference.Count) ignored test(s) differ from the allowlist"
}

Write-Host "$($found.Count) ignored tests, all of them on the list."
