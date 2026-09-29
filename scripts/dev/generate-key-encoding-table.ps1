# Writes `docs/key-encoding.md` from the keyboard encoder's table, `crates/bt-app/src/key_encoding.tsv`,
# and `crates/bt-app/src/key_records_windows_us.tsv`, the record pairs the encoder writes on Windows
# with a US layout (T-KEYBOARD-RECORDS), which the real-ConPTY test sends.
#
# The renderers are not in this file. They are `input::tests::the_key_encoding_document_is_the_table`,
# which renders the table, leaves the rendering in `target/key-encoding.md`, and fails when
# `docs/key-encoding.md` is not that rendering, and
# `input::tests::the_windows_us_records_file_is_what_the_encoder_writes`, which does the same with
# `target/key-records-windows-us.tsv`. A second renderer written here would be a second opinion about
# the same encoder (the pattern of `scripts/dev/generate-window-waits-table.ps1`).
#
# So this script runs those tests and copies what they wrote into place. The tests are the gate: CI
# runs them with the rest of `bt-app`'s tests.

$ErrorActionPreference = "Stop"

# A non-zero exit from cargo is the expected case here - it is what "the page is out of date" looks
# like - so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$pairs = @(
    @((Join-Path $repo "target\key-encoding.md"), (Join-Path $repo "docs\key-encoding.md")),
    @((Join-Path $repo "target\key-records-windows-us.tsv"), (Join-Path $repo "crates\bt-app\src\key_records_windows_us.tsv"))
)

foreach ($pair in $pairs) {
    if (Test-Path $pair[0]) { Remove-Item $pair[0] -Force }
}

Push-Location $repo
try {
    & cargo test -p bt-app --locked -j 6 -- --exact `
        input::tests::the_key_encoding_document_is_the_table `
        input::tests::the_windows_us_records_file_is_what_the_encoder_writes | Out-Host
} finally {
    Pop-Location
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
foreach ($pair in $pairs) {
    if (-not (Test-Path $pair[0])) {
        throw "the renderer never ran: $($pair[0]) was not written"
    }
    [IO.File]::WriteAllText($pair[1], [IO.File]::ReadAllText($pair[0], $utf8), $utf8)
    Write-Host "wrote $($pair[1])"
}

# The cargo run above is expected to have failed when a file was out of date, and its exit code
# must not become this script's: writing the files is what this script was asked to do.
exit 0
