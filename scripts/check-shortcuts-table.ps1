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
#
# **And it pins two dialects, not one.** The comparison above is against whatever
# the renderer emits, so it cannot on its own say that the renderer still emits a
# macOS column: a renderer that dropped one, followed by a regenerate, would
# leave a file that agrees with itself. So the second half below reads the
# checked-in file on its own terms and refuses three things — a language section
# whose table is not five columns headed `Windows` and `macOS`, a surfaced row
# with an empty macOS cell, and a table whose two dialects are the same column
# twice, which is what a mac column filled from the Windows one looks like.

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

# The two dialects, read off the file itself.
$document = Join-Path $repo "docs/shortcuts.md"
$lines = [IO.File]::ReadAllLines($document)

$headings = @($lines | Where-Object { $_ -match '^##\s' })
if ($headings.Count -lt 2) {
    throw "docs/shortcuts.md has $($headings.Count) language section(s) - it carries one table per language"
}

$headerRows = @($lines | Where-Object { $_ -match '^\|\s*Windows\s*\|\s*macOS\s*\|' })
if ($headerRows.Count -ne $headings.Count) {
    throw "docs/shortcuts.md has $($headings.Count) language section(s) but $($headerRows.Count) table(s) headed 'Windows | macOS' - a dialect column has gone"
}

$rows = 0
$differ = 0
foreach ($line in $lines) {
    if ($line -notmatch '^\|') { continue }
    if ($line -match '^\|\s*-{3}') { continue }
    if ($line -match '^\|\s*Windows\s*\|') { continue }

    # `| a | b | c | d | e |` splits to seven, with an empty end on each side.
    $cells = $line -split '\|'
    if ($cells.Count -ne 7) {
        throw "docs/shortcuts.md row is not five columns: $line"
    }
    $windows = $cells[1].Trim()
    $mac = $cells[2].Trim()
    if ($windows -eq "") { throw "docs/shortcuts.md row has no Windows key: $line" }
    if ($mac -eq "") { throw "docs/shortcuts.md row has no macOS key: $line" }
    $rows++
    if ($windows -ne $mac) { $differ++ }
}

if ($rows -eq 0) { throw "docs/shortcuts.md has no rows - the gate has nothing to check" }
if ($differ -eq 0) {
    throw "docs/shortcuts.md says the same key in both columns on every row - the macOS dialect is a copy of the Windows one"
}

Write-Host "docs/shortcuts.md matches BINDINGS, in $($headings.Count) languages and two dialects ($rows rows, $differ of them differing)"
