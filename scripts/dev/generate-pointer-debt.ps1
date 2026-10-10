# Writes `docs/plans/POINTER-DEBT.tsv` from the index (T-POINTER-CAPTURE,
# `docs/plans/design/pointer-capture-2026-10-09.md` section 4.2).
#
# The query is not in this file. It is
# `pointer_app_tests::every_pointer_read_is_the_routers_or_a_captures`, which asks the
# `bt-source` index for every occurrence of the six doors' needles outside
# `crate::runtime::pointer`, renders one row per occurrence into `target/pointer-debt.tsv`,
# and fails when the committed file is not that rendering. A second query written here would
# be a second opinion about the same list.
#
# So this script runs that test and copies what it wrote over the committed file. The test is
# the gate: CI runs it with the rest of bt-app's tests, and `scripts/ci/check-pointer-debt.ps1`
# refuses a row the merge base did not have.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here - it is what "the list is out of date"
# looks like - so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$generated = Join-Path $repo "target\pointer-debt.tsv"
$list = Join-Path $repo "docs\plans\POINTER-DEBT.tsv"

if (Test-Path $generated) { Remove-Item $generated -Force }

Push-Location $repo
try {
    & cargo test -p bt-app --bin folio --locked -- --exact `
        pointer_app_tests::every_pointer_read_is_the_routers_or_a_captures | Out-Host
} finally {
    Pop-Location
}

if (-not (Test-Path $generated)) {
    throw "the query never ran: $generated was not written"
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($list, [IO.File]::ReadAllText($generated, $utf8), $utf8)
Write-Host "wrote $list"

# The cargo run above is expected to have failed when the list was out of date, and its exit code
# must not become this script's: writing the list is what this script was asked to do.
exit 0
