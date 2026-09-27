# the_lint_resolves_every_entry_on_this_target
#
# The positive control of the thread-door note 2026-09-26, revisions (j)4 and
# (j)11.3: clippy's `disallowed_methods` names a function by path, and a path
# that is misspelled, or that belongs to a crate this target does not load, is
# not an error - clippy says at most "does not refer to a reachable function",
# and -D warnings does not make that fatal. So the only proof that an entry of
# the registry's vocabulary is effective on a target is a call of it that the
# lint reports there. `crates/bt-lint-probe` holds one call per entry, each on a
# line that names its entry, each under the cfg of the targets the entry is
# assigned to; its clippy.toml holds the vocabulary for the probe alone.
#
# This script, on the platform it runs on (windows or macos):
#
#   1. regenerates the probe with scripts/dev/generate-lint-probe.ps1 and
#      requires `git diff --exit-code` over the crate and every generated file
#      tracked - the probe is the registry, or this is red;
#   2. runs `cargo clippy -p bt-lint-probe --all-targets -- --force-warn
#      clippy::disallowed_methods` (the force reaches through the probe root's
#      own allowance) and reads its diagnostics as JSON:
#        * a compile error is red - a misspelled path or a wrong target
#          assignment usually ends here;
#        * each disallowed_methods diagnostic is matched by its source span to
#          the entry the line names, and must be that entry;
#        * every entry assigned to this target has exactly one, and no entry
#          assigned elsewhere has any;
#        * a "does not refer to a reachable function" warning is fatal for an
#          entry assigned to this target and expected for one that is not
#          (revision (j)4's target-aware rule); nothing else is exempt.
#
# The ordinary product invocation (CI's clippy line) is separate and unchanged.

param(
    [string]$Registry = "crates/bt-app/src/window_waits.tsv"
)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$probe = 'crates/bt-lint-probe'
$generated = @('crates/bt-lint-probe/src/lib.rs', 'crates/bt-lint-probe/clippy.toml')

if ($IsWindows) { $target = "windows" } elseif ($IsMacOS) { $target = "macos" } else {
    throw "the lint probe's positive control runs on the product's targets, windows and macos"
}

# The registry's vocabulary: path -> targets.
$assigned = [ordered]@{}
$inside = $false
$header = $false
foreach ($line in [IO.File]::ReadAllLines((Join-Path $repo $Registry))) {
    if ($line.StartsWith("##")) { continue }
    if ($line.StartsWith("# ")) { $inside = ($line -eq "# vocabulary"); continue }
    if (-not $inside -or $line.Length -eq 0) { continue }
    if (-not $header) { $header = $true; continue }
    $cells = $line -split "`t"
    $targets = @($cells[1] -split ", " | Where-Object { $_ -ne "" })
    if ($targets.Count -eq 0) { throw "the vocabulary entry '$($cells[0])' is assigned to no target" }
    $assigned[$cells[0]] = ($targets -contains "all") -or ($targets -contains $target)
}
if ($assigned.Count -eq 0) { throw "$Registry has no vocabulary: this reads nothing" }

Push-Location $repo
try {
    # 1. The probe is what the registry generates, and it is tracked.
    & pwsh -NoProfile -File scripts/dev/generate-lint-probe.ps1 -Source $generated[0] -Config $generated[1]
    if ($LASTEXITCODE -ne 0) { throw "the lint probe's generator failed" }
    foreach ($file in $generated) {
        & git ls-files --error-unmatch -- $file *> $null
        if ($LASTEXITCODE -ne 0) { throw "$file is generated and must be tracked, and it is not" }
    }
    & git diff --exit-code -- $probe
    if ($LASTEXITCODE -ne 0) {
        throw "the lint probe is not what scripts/dev/generate-lint-probe.ps1 writes from the registry (the diff is above): run it and commit the result"
    }
    $untracked = @(& git ls-files --others --exclude-standard -- $probe)
    if ($untracked.Count -gt 0) { throw "untracked files in the lint probe: $($untracked -join ', ')" }

    # 2. The positive control.
    $output = & cargo clippy -p bt-lint-probe --all-targets --locked --message-format=json -- --force-warn clippy::disallowed_methods 2>$null
    $status = $LASTEXITCODE
} finally {
    Pop-Location
}

$failures = @()
if ($status -ne 0) { $failures += "cargo clippy on the probe exited $status" }
$seen = @{}
$counts = @{}
$expected = @()
foreach ($line in $output) {
    if (-not $line.StartsWith("{")) { continue }
    $message = $line | ConvertFrom-Json
    if ($message.reason -ne "compiler-message") { continue }
    $diagnostic = $message.message
    if ($diagnostic.level -eq "error") {
        $failures += "a compile error in the probe: $($diagnostic.rendered)"
        continue
    }
    if ($diagnostic.message -match '`([^`]+)` does not refer to') {
        $path = $Matches[1]
        if (-not $assigned.Contains($path)) {
            $failures += "clippy names '$path', which is no vocabulary entry: $($diagnostic.message)"
        } elseif ($assigned[$path]) {
            $failures += "'$path' is assigned to $target and clippy cannot resolve it here: $($diagnostic.message)"
        } else {
            $expected += $path
        }
        continue
    }
    if ($diagnostic.code.code -ne "clippy::disallowed_methods") { continue }
    $span = $diagnostic.spans | Where-Object { $_.is_primary } | Select-Object -First 1
    $key = "$($span.file_name):$($span.line_start):$($span.column_start)"
    if ($seen.ContainsKey($key)) { continue }
    $seen[$key] = $true
    $text = ($span.text | Select-Object -First 1).text
    if ($text -notmatch '// probe: (\S+)\s*$') {
        $failures += "a disallowed_methods diagnostic on a line that names no entry: $key '$text'"
        continue
    }
    $entry = $Matches[1]
    if ($diagnostic.message -notmatch [regex]::Escape("``$entry``")) {
        $failures += "the line for '$entry' was reported as another method: $($diagnostic.message)"
        continue
    }
    if ($counts.ContainsKey($entry)) { $counts[$entry] += 1 } else { $counts[$entry] = 1 }
}

$covered = 0
foreach ($path in $assigned.Keys) {
    $count = if ($counts.ContainsKey($path)) { $counts[$path] } else { 0 }
    if ($assigned[$path]) {
        if ($count -ne 1) {
            $failures += "'$path' is assigned to $target and the lint reported it $count times on the probe, where it must once"
        } else {
            $covered += 1
        }
    } elseif ($count -gt 0) {
        $failures += "'$path' is not assigned to $target and its call compiled here: its cfg is not its assignment"
    }
}

foreach ($path in $assigned.Keys) {
    $count = if ($counts.ContainsKey($path)) { $counts[$path] } else { 0 }
    Write-Host ("  {0,-55} {1,-8} {2}" -f $path, $(if ($assigned[$path]) { $target } else { "off" }), $count)
}
if ($expected.Count -gt 0) {
    Write-Host "off-target entries clippy could not resolve here, as expected: $(($expected | Sort-Object -Unique) -join ', ')"
}
if ($failures.Count -gt 0) {
    throw ("the lint probe's positive control failed on ${target}:" + [Environment]::NewLine +
        (($failures | ForEach-Object { "    $_" }) -join [Environment]::NewLine))
}
Write-Host "the lint probe on ${target}: every one of the $covered entries assigned here reported once."
