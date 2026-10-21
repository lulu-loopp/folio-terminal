# Writes docs/plans/UNFOLLOWED-EDGES.tsv from the query (T-BT-SOURCE-CALL-GRAPH round 3).
#
# The query is not in this file. It is bt_source::call_graph, walked from every product callable
# of bt-app by bt-source's `call_graph` test `the_unfollowed_edges_are_the_committed_list`, which
# compares the sites it meets with the committed list and leaves its rendering in
# target/unfollowed-edges/UNFOLLOWED-EDGES.tsv — every key with its `sites`, and the committed
# `reason` of every key the list already carries. A copy written in PowerShell would have to parse
# Rust, which the tripwire refuses: bt-source is the one sanctioned reader.
#
# So this script runs that test and copies what it rendered, on the pattern of
# scripts/generate-runtime-dep-map.ps1. A new key's reason is blank in the rendering: a reviewer who lets
# the door reach go blind at that call writes `YYYY-MM-DD: <why>` there before committing, or
# `scripts/ci/check-unfollowed-edges.ps1` refuses the row.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot
$rendered = Join-Path $repo "target/unfollowed-edges/UNFOLLOWED-EDGES.tsv"
$committed = Join-Path $repo "docs/plans/UNFOLLOWED-EDGES.tsv"

if (Test-Path $rendered) { Remove-Item $rendered -Force }

Push-Location $repo
try {
    & cargo test -p bt-source --locked --test call_graph -- --exact `
        the_unfollowed_edges_are_the_committed_list | Out-Host
} finally {
    Pop-Location
}

if (-not (Test-Path $rendered)) {
    Write-Host "target/unfollowed-edges/UNFOLLOWED-EDGES.tsv was not rendered: the test did not reach its rendering; its message says why."
    exit 1
}
Copy-Item -LiteralPath $rendered -Destination $committed -Force
Write-Host "wrote docs/plans/UNFOLLOWED-EDGES.tsv"
