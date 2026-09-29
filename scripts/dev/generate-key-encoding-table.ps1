# Writes `docs/key-encoding.md` from the keyboard encoder's table, `crates/bt-app/src/key_encoding.tsv`.
#
# The renderer is not in this file. It is `input::tests::the_key_encoding_document_is_the_table`,
# which renders the table, leaves the rendering in `target/key-encoding.md`, and fails when
# `docs/key-encoding.md` is not that rendering. A second renderer written here would be a second
# opinion about the same table (the pattern of `scripts/dev/generate-window-waits-table.ps1`).
#
# So this script runs that test and copies what it wrote into place. The test is the gate: CI runs
# it with the rest of `bt-app`'s tests.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here - it is what "the page is out of date" looks
# like - so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$generated = Join-Path $repo "target\key-encoding.md"
$document = Join-Path $repo "docs\key-encoding.md"

if (Test-Path $generated) { Remove-Item $generated -Force }

Push-Location $repo
try {
    & cargo test -p bt-app --locked -j 6 -- --exact `
        input::tests::the_key_encoding_document_is_the_table | Out-Host
} finally {
    Pop-Location
}

if (-not (Test-Path $generated)) {
    throw "the renderer never ran: $generated was not written"
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($document, [IO.File]::ReadAllText($generated, $utf8), $utf8)
Write-Host "wrote $document"

# The cargo run above is expected to have failed when the page was out of date, and its exit code
# must not become this script's: writing the page is what this script was asked to do.
exit 0
