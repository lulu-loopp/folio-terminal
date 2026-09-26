# Writes the ownership census under docs/plans/design/ from the query.
#
# The query is not in this file. It is bt_source::FieldCensus, and the test
# `the_committed_census_is_what_the_code_says` in bt-source's `census` test
# target runs it over bt-app, compares it with the committed files, and leaves
# its rendering in target/ownership-census/. A copy of that reading written in
# PowerShell would have to parse Rust — and a script that reads Rust source is
# what the tripwire refuses, because a reader bound to a file stays green when
# its subject moves. bt-source is the one sanctioned reader.
#
# So this script runs that test and copies what it wrote, on the pattern of
# scripts/generate-shortcuts-table.ps1. It reads nothing but the rendering.
#
# Three files are copied: the inventory and the site rows always, and the
# unknown list only when the test rendered it — which it does only when the
# list has not grown against the committed one (or when there is no committed
# one yet). The unknown list only shrinks; a new unknown is resolved in the
# code, or added to the list by hand with its reason. The annotation file is
# hand-edited and never written here.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here — it is what "the census
# is out of date" looks like — so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot
$rendered = Join-Path $repo "target/ownership-census"
$design = Join-Path $repo "docs/plans/design"
$names = @(
    "ownership-census-inventory.tsv",
    "ownership-census-sites.tsv",
    "ownership-census-unknowns.tsv"
)

foreach ($name in $names) {
    $path = Join-Path $rendered $name
    if (Test-Path $path) { Remove-Item $path -Force }
}

Push-Location $repo
try {
    & cargo test -p bt-source --locked --test census -- --exact `
        the_committed_census_is_what_the_code_says | Out-Host
} finally {
    Pop-Location
}

foreach ($name in $names[0..1]) {
    $path = Join-Path $rendered $name
    if (-not (Test-Path $path)) {
        throw "the query never ran: $path was not written"
    }
    Copy-Item $path (Join-Path $design $name) -Force
    Write-Host "wrote docs/plans/design/$name"
}

$unknowns = Join-Path $rendered $names[2]
if (Test-Path $unknowns) {
    Copy-Item $unknowns (Join-Path $design $names[2]) -Force
    Write-Host "wrote docs/plans/design/$($names[2])"
} else {
    Write-Host ("docs/plans/design/$($names[2]) is unchanged: the code has an unknown site the list " +
        "does not carry. Resolve it, or add its row by hand with the reason; the test's message names it.")
}

# The cargo run above is expected to have failed when the census was out of
# date, and its exit code must not become this script's.
exit 0
