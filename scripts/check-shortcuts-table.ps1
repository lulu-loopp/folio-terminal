# docs_shortcuts_md_is_the_bindings_table
#
# `README.md` sends a reader to `docs/shortcuts.md` for the keys. That file is
# rendered from `BINDINGS` in `crates/bt-app/src/shortcuts.rs`, and this gate is
# what keeps it from drifting into a second opinion about which keys exist: it
# runs the test that renders the table and compares it with the file in the tree.
#
# Prove it fires before trusting it: change a chord in `BINDINGS`, or delete a row
# from `docs/shortcuts.md`, and this must go red. Bring it back with
# `scripts/generate-shortcuts-table.ps1`.

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent $PSScriptRoot

Push-Location $repo
try {
    & cargo test --package bt-app --bin folio --locked -- --exact `
        shortcuts::tests::docs_shortcuts_md_is_the_bindings_table | Out-Host
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($code -ne 0) {
    throw "docs/shortcuts.md is not the shortcut table any more - run scripts/generate-shortcuts-table.ps1"
}

Write-Host "docs/shortcuts.md matches BINDINGS"
