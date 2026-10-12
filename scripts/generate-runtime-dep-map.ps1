# Writes the runtime dependency map under docs/plans/design/runtime-dep-map/
# from the query (J3, T-RUNTIME-DEP-MAP).
#
# The query is not in this file. It is bt_source::DependencyMap, taken from
# bt_source::FieldCensus, and the test `the_committed_runtime_dep_map_is_the_code`
# in bt-source's `dep_map` test target runs it over bt-app, compares it with
# the committed files, and leaves its four renderings in target/depmap/. A copy
# written in PowerShell would have to parse Rust, and a script that reads Rust
# source is what the tripwire refuses: bt-source is the one sanctioned reader.
#
# So this script runs that test and copies what it rendered, on the pattern of
# scripts/generate-ownership-census.ps1. It reads nothing but the renderings,
# and writes one line into each under the generated header: the commit the map
# was measured at (and whether the tree had uncommitted changes), which is the
# one line the test does not compare.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here - it is what "the map
# is out of date" looks like - so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot
$rendered = Join-Path $repo "target/depmap"
$committed = Join-Path $repo "docs/plans/design/runtime-dep-map"
$names = @("fields.tsv", "topics.tsv", "doors.tsv", "unclassified.tsv")

foreach ($name in $names) {
    $path = Join-Path $rendered $name
    if (Test-Path $path) { Remove-Item $path -Force }
}

Push-Location $repo
try {
    & cargo test -p bt-source --locked --test dep_map -- --exact `
        the_committed_runtime_dep_map_is_the_code | Out-Host
    $commit = (& git rev-parse --short=8 HEAD).Trim()
    $dirty = [bool](& git status --porcelain --untracked-files=no)
} finally {
    Pop-Location
}

$measured = "# measured at $commit"
if ($dirty) { $measured += " with uncommitted changes" }

New-Item -ItemType Directory -Force -Path $committed | Out-Null
foreach ($name in $names) {
    $path = Join-Path $rendered $name
    if (-not (Test-Path $path)) {
        Write-Host "target/depmap/$name was not rendered: the test did not reach its rendering; its message says why."
        exit 1
    }
    $lines = [System.IO.File]::ReadAllLines($path)
    # The generated header is the first two lines; the commit line goes under it.
    $out = @($lines[0], $lines[1], $measured) + $lines[2..($lines.Length - 1)]
    [System.IO.File]::WriteAllText((Join-Path $committed $name), (($out -join "`n") + "`n"))
    Write-Host "wrote docs/plans/design/runtime-dep-map/$name"
}

# The cargo run above is expected to have failed when the map was out of date,
# and its exit code must not become this script's.
exit 0
