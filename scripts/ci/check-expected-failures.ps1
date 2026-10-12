# the_lane_expected_failures_only_shrink
#
# `lane::EXPECTED_FAILURES` (crates/bt-app/src/lane.rs) is the list of claims of
# the lane contract a lane is known to fail, each with the structural-debt row
# that repairs it. The suite holds the tree to the list both ways
# (`lane_contract_tests::every_lane_runs_every_claim_and_fails_only_where_the_ledger_says`:
# an undeclared failure is red, and so is a declared one that passes). This
# script is the history half: against the merge base with origin/main, no lane
# gains a declared failure. A row is keyed by its lane and its claim; a key the
# merge base does not have is refused - unless its claim is not a variant of the
# merge base's `Claim` at all, because a claim new to the suite arrives with the
# rows of the lanes that do not keep it yet, and those rows are its baseline
# (S-3, 2026-10-11). A row whose failure kind changed is reported, not refused:
# the claim still fails, and the suite says how.
#
# The rows are read from the source text: the block from
# `pub(crate) const EXPECTED_FAILURES` to its closing `];`, each row's
# `lane: LaneName::X`, `claim: Claim::Y` and `failure: FailureKind::Z` in that
# order. It passes, loudly, when the merge base has no such block; it refuses
# (exit 2) when there is no merge base at all.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "crates/bt-app/src/lane.rs",
    [string]$MainRef = "origin/main"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$source = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $source)) {
    throw "$Relative is not in the tree - it holds the lane contract's expected failures"
}

# The declared failures as an ordered map "Lane x Claim" -> failure kind, or $null when the text
# has no `EXPECTED_FAILURES` block.
function Read-ExpectedFailures([string]$Text) {
    $start = $Text.IndexOf("pub(crate) const EXPECTED_FAILURES")
    if ($start -lt 0) { return $null }
    $end = $Text.IndexOf("`n];", $start)
    if ($end -lt 0) { throw "$Relative has an EXPECTED_FAILURES block with no closing '];'" }
    $block = $Text.Substring($start, $end - $start)
    $rows = [ordered]@{}
    $pattern = 'ExpectedFailure\s*\{\s*lane:\s*LaneName::(\w+),\s*claim:\s*Claim::(\w+),\s*failure:\s*FailureKind::(\w+),'
    foreach ($match in [regex]::Matches($block, $pattern)) {
        $key = "$($match.Groups[1].Value) x $($match.Groups[2].Value)"
        if ($rows.Contains($key)) { throw "$Relative declares $key twice" }
        $rows[$key] = $match.Groups[3].Value
    }
    $declared = [regex]::Matches($block, 'ExpectedFailure\s*\{').Count
    if ($declared -ne $rows.Count) {
        throw "$Relative has $declared expected-failure rows and $($rows.Count) read as lane, claim, failure"
    }
    return $rows
}

# The variants of `enum Claim` in the text: its body runs to the first line that is a lone `}`.
function Read-Claims([string]$Text) {
    $match = [regex]::Match($Text, '(?ms)^pub\(crate\) enum Claim \{\r?\n(?<body>.*?)^\}')
    if (-not $match.Success) { return @() }
    return @([regex]::Matches($match.Groups['body'].Value, '(?m)^    (\w+),\s*$') | ForEach-Object { $_.Groups[1].Value })
}

Push-Location $Repo
try {
    $now = Read-ExpectedFailures ([IO.File]::ReadAllText($source))
    if ($null -eq $now) { throw "$Relative has no EXPECTED_FAILURES block" }
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the expected-failures comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $status = $LASTEXITCODE
    $base = if ($status -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the expected-failures comparison did not run"
        exit 2
    }
    $text = (& git show "${base}:${Relative}" 2>$null) | Out-String
    $before = if ($LASTEXITCODE -eq 0) { Read-ExpectedFailures $text } else { $null }
    if ($null -eq $before) {
        Write-Host "$Relative has no EXPECTED_FAILURES at $base - no base list exists; the tree declares $($now.Count) rows. PASS"
        exit 0
    }
    $claimsBefore = Read-Claims $text
} finally {
    Pop-Location
}

$added = @()
$baseline = @()
foreach ($key in $now.Keys) {
    if ($before.Contains($key)) {
        if ($before[$key] -ne $now[$key]) {
            Write-Host "  $key now fails with $($now[$key]), at the merge base with $($before[$key])"
        }
        continue
    }
    $claim = ($key -split ' x ')[1]
    if ($claimsBefore -contains $claim) { $added += $key } else { $baseline += $key }
}
$removed = @($before.Keys | Where-Object { -not $now.Contains($_) }).Count
Write-Host ("$Relative EXPECTED_FAILURES against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), " +
    "$removed removed, $($baseline.Count) brought by claims new to the suite, $($added.Count) added.")
if ($added.Count -gt 0) {
    $details = ($added | ForEach-Object { "    $_" }) -join [Environment]::NewLine
    throw ("$($added.Count) expected failure(s) were added for claims the merge base already had, and this list only shrinks:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Repair the lane, or restore the claim's behaviour; a lane does not gain a declared failure.")
}

Write-Host "the lane contract's expected failures only shrank. PASS"
exit 0
