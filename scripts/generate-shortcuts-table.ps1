# Writes `docs/shortcuts.md` from the shortcut table in the source.
#
# The renderer is not in this file. It is
# `bt_app::shortcuts::tests::docs_shortcuts_md_is_the_bindings_table`, which
# walks `BINDINGS`, asks each row for its own name, chord and scope in both
# languages, and leaves the rendering in `target/shortcuts-table.md`. A copy of
# that walk written in PowerShell would have to parse Rust to find the table and
# then parse the string table to find the words — two parsers that agree with the
# program only until somebody edits it.
#
# So this script runs that test and copies what it wrote. The test fails when the
# checked-in file is out of date, which is the point: the copy below is how you
# bring it back into date, and `scripts/check-shortcuts-table.ps1` is the same
# test run as a gate.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here — it is what "the file is
# out of date" looks like — so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot
$generated = Join-Path $repo "target/shortcuts-table.md"
$checkedIn = Join-Path $repo "docs/shortcuts.md"

if (Test-Path $generated) { Remove-Item $generated -Force }

Push-Location $repo
try {
    & cargo test --package bt-app --bin folio --locked -- --exact `
        shortcuts::tests::docs_shortcuts_md_is_the_bindings_table | Out-Host
} finally {
    Pop-Location
}

if (-not (Test-Path $generated)) {
    throw "the renderer never ran: $generated was not written"
}

Copy-Item $generated $checkedIn -Force
Write-Host "wrote $checkedIn"

# The cargo run above is expected to have failed when the file was out of date,
# and its exit code must not become this script's: writing the file is what this
# script was asked to do, and it did it.
exit 0
