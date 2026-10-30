# the_generated_stamps_name_a_clean_ancestor
#
# The four files of the runtime dependency map
# (`docs/plans/design/runtime-dep-map/{fields,topics,doors,unclassified}.tsv`)
# carry, as their third line, the commit they were measured at, written by
# `scripts/generate-runtime-dep-map.ps1`:
#
#     # measured at <sha>[ with uncommitted changes]
#
# where <sha> is `git rev-parse --short=8 HEAD` (eight hex digits, or more when
# eight are ambiguous: 8 to 40 lowercase hex) and the suffix is written when
# `git status --porcelain --untracked-files=no` said anything. bt-source's
# `the_committed_runtime_dep_map_is_the_code` compares every other line and
# skips this one, so nothing else reads it. This is what reads it
# (T-GENERATED-STAMP-REFUSES-DIRTY-TREES, plan row 99). Each file is refused:
#
# - with no stamp line, or with more than one (the generator writes exactly one,
#   so a file without it was copied by hand out of `target/depmap/`);
# - with a line that starts `# measured at ` and is not the grammar above;
# - when the line carries ` with uncommitted changes` - the map was measured on
#   a tree that is not any commit, and a list that says so is not committed
#   (twice in 2026-10: plan row 89's `e7c89e27`, O-5b's `060bb9bb`);
# - when <sha> names no commit of this clone;
# - when <sha> is not an ancestor of HEAD (`git merge-base --is-ancestor`; a
#   commit is its own ancestor) - the map was measured on another branch's tree
#   and carried here by a cherry-pick or a copy.
#
# And the four are refused together when their shas differ: the generator
# writes all four from one measurement, so two shas mean a partial regenerate.
#
# A shallow clone in which <sha> does not resolve is not a pass and not a
# refusal of the file: the check did not happen, and it exits 2 saying so (the
# rule `check-migration-debt.ps1` follows). It does not deepen the clone. CI runs
# it in the `logic` job, whose checkout fetches the whole history.
# `check-generated-stamps-tests.ps1` runs it against scratch repositories
# (`-Repo`), one case per refusal.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$directory = "docs/plans/design/runtime-dep-map"
$names = @("fields.tsv", "topics.tsv", "doors.tsv", "unclassified.tsv")
$prefix = "# measured at "
$grammar = '^# measured at (?<sha>[0-9a-f]{8,40})(?<dirty> with uncommitted changes)?$'

$refused = [System.Collections.Generic.List[string]]::new()
$shas = [ordered]@{}
$unchecked = $false

Push-Location $Repo
try {
    $shallow = ((& git rev-parse --is-shallow-repository) | Out-String).Trim() -eq "true"
    foreach ($name in $names) {
        $relative = "$directory/$name"
        $path = Join-Path $Repo $relative
        if (-not (Test-Path -LiteralPath $path)) {
            $refused.Add("${relative}: the file is missing")
            continue
        }
        $lines = [IO.File]::ReadAllLines($path)
        $stamps = @()
        for ($at = 0; $at -lt $lines.Length; $at++) {
            if ($lines[$at].StartsWith($prefix)) { $stamps += , @(($at + 1), $lines[$at]) }
        }
        if ($stamps.Count -eq 0) {
            $refused.Add("${relative}: no '# measured at' line - the generator writes one; run scripts/generate-runtime-dep-map.ps1")
            continue
        }
        if ($stamps.Count -gt 1) {
            $refused.Add("${relative}: $($stamps.Count) '# measured at' lines (lines $(($stamps | ForEach-Object { $_[0] }) -join ', ')) - the generator writes one")
            continue
        }
        $line = $stamps[0][0]
        $text = $stamps[0][1]
        $match = [regex]::Match($text, $grammar)
        if (-not $match.Success) {
            $refused.Add("${relative}:${line}: '$text' is not '# measured at <8-40 hex>[ with uncommitted changes]'")
            continue
        }
        $sha = $match.Groups["sha"].Value
        $shas[$relative] = $sha
        if ($match.Groups["dirty"].Success) {
            $refused.Add("${relative}:${line}: '$text' - measured on a tree with uncommitted changes; commit first, then regenerate")
            continue
        }
        & git rev-parse --verify --quiet "$sha^{commit}" *> $null
        if ($LASTEXITCODE -ne 0) {
            if ($shallow) {
                Write-Host "${relative}:${line}: $sha does not resolve in this shallow clone."
                $unchecked = $true
            } else {
                $refused.Add("${relative}:${line}: $sha names no commit of this repository")
            }
            continue
        }
        & git merge-base --is-ancestor $sha HEAD *> $null
        $status = $LASTEXITCODE
        if ($status -eq 1) {
            $refused.Add("${relative}:${line}: $sha is not an ancestor of HEAD - the map was measured on another tree")
        } elseif ($status -ne 0) {
            throw "git merge-base --is-ancestor $sha HEAD exited $status"
        }
    }
} finally {
    Pop-Location
}

$distinct = @($shas.Values | Sort-Object -Unique)
if ($distinct.Count -gt 1) {
    $refused.Add("the four files name $($distinct.Count) commits ($(($shas.GetEnumerator() | ForEach-Object { "$($_.Key) $($_.Value)" }) -join '; ')) - they are written by one measurement")
}

if ($refused.Count -gt 0) {
    Write-Host "the runtime dependency map's stamps are refused ($($refused.Count)):"
    foreach ($why in $refused) { Write-Host "  $why" }
    exit 1
}
if ($unchecked) {
    Write-Host "This is not a pass: the ancestry was not checked. Fetch the whole history (fetch-depth 0, or 'git fetch --unshallow') and run it again."
    exit 2
}
Write-Host "the runtime dependency map's four stamps name $($distinct[0]), a clean ancestor of HEAD."
exit 0
