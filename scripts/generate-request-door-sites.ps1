# Writes docs/plans/REQUEST-DOOR-SITES.tsv from the C1 guard (T-REQUEST-DOOR-GUARD).
#
# The query is not in this file. It is bt-source's `request_doors` test
# `every_request_door_returns_at_once`, which walks every request door of
# crates/bt-app/src/window_waits.tsv's `# request-doors` over bt_source::call_graph, compares what
# it meets with the committed list and leaves its rendering in
# target/request-door-sites/REQUEST-DOOR-SITES.tsv - every key with its `sites`, and the committed
# verdict and reason of every key the list already carries. A copy written in PowerShell would have
# to parse Rust, which the tripwire refuses: bt-source is the one sanctioned reader.
#
# So this script runs that test and copies what it rendered, on the pattern of
# scripts/generate-unfollowed-edges.ps1. It compiles: run it only while holding a cargo lane. A new
# key is `unreviewed` with a blank reason in the rendering: a reviewer reads the site and writes its
# verdict and `YYYY-MM-DD: <why>` before committing, or `scripts/ci/check-request-door-sites.ps1`
# refuses the row.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot
$rendered = Join-Path $repo "target/request-door-sites/REQUEST-DOOR-SITES.tsv"
$committed = Join-Path $repo "docs/plans/REQUEST-DOOR-SITES.tsv"

if (Test-Path $rendered) { Remove-Item $rendered -Force }

Push-Location $repo
try {
    & cargo test -p bt-source --locked --test request_doors -- --exact `
        every_request_door_returns_at_once | Out-Host
} finally {
    Pop-Location
}

if (-not (Test-Path $rendered)) {
    Write-Host "target/request-door-sites/REQUEST-DOOR-SITES.tsv was not rendered: the test did not reach its rendering; its message says why."
    exit 1
}
Copy-Item -LiteralPath $rendered -Destination $committed -Force
Write-Host "wrote docs/plans/REQUEST-DOOR-SITES.tsv"
