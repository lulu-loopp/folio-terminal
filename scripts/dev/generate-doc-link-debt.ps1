# Writes `docs/plans/DOC-LINK-DEBT.tsv` from rustdoc's own warnings (T-DOC-LINK-DEBT, 0.4.9 plan
# row 78).
#
# The measurement and the key are not in this file. `scripts/ci/measure-doc-links.ps1` runs
# `cargo doc` and then bt-source's `doc_link_debt` test target, which keys every warning and
# leaves its rendering in `target/doc-link-debt.tsv` (and writes none when a warning stands where
# its grammar does not read). A second key written here would be a second opinion about the same
# list.
#
# So this script runs that measurement, then copies the rendering over the committed list with
# one line added under its first two: the commit it was measured at, and whether the tree had
# uncommitted changes (`# measured at <sha>[ with uncommitted changes]`, which
# `scripts/ci/check-generated-stamps.ps1` refuses unless it names a clean ancestor). Commit the
# code first, then run this, then commit its output. It compiles: run it only on a lane.

$ErrorActionPreference = "Stop"

# A non-zero exit from the measurement is the expected case here - it is what "the list is out of
# date" looks like - so it must not end the script before the copy.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$rendered = Join-Path $repo "target\doc-link-debt.tsv"
$list = Join-Path $repo "docs\plans\DOC-LINK-DEBT.tsv"

# A rendering left by an earlier run must not be copied when this run's measurement never reached
# the key.
if (Test-Path -LiteralPath $rendered) { Remove-Item -LiteralPath $rendered -Force }

& pwsh -NoProfile -File (Join-Path $repo "scripts\ci\measure-doc-links.ps1") | Out-Host

if (-not (Test-Path -LiteralPath $rendered)) {
    Write-Host "target/doc-link-debt.tsv was not rendered: the measurement or the key stopped first; its message says why."
    exit 1
}

Push-Location $repo
try {
    $commit = (& git rev-parse --short=8 HEAD).Trim()
    $dirty = [bool](& git status --porcelain --untracked-files=no)
} finally {
    Pop-Location
}
$measured = "# measured at $commit"
if ($dirty) { $measured += " with uncommitted changes" }

$utf8 = New-Object System.Text.UTF8Encoding($false)
$lines = [IO.File]::ReadAllLines($rendered, $utf8)
$out = @($lines[0], $lines[1], $measured) + $lines[2..($lines.Length - 1)]
[IO.File]::WriteAllText($list, (($out -join "`n") + "`n"), $utf8)
Write-Host "wrote docs/plans/DOC-LINK-DEBT.tsv ($measured)"

# The measurement above is expected to have failed when the list was out of date, and its exit
# code must not become this script's: writing the list is what this script was asked to do.
exit 0
