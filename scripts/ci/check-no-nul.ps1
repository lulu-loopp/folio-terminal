# no_tracked_text_file_contains_nul
#
# Source searchers commonly classify a file containing NUL as binary and skip
# it. Read every tracked text file as bytes so that state is a refusal, never a
# reason another gate can report green without seeing the file.

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $PSScriptRoot 'tracked-text-files.ps1')

$hits = @()
$scanned = 0
foreach ($file in Get-TrackedTextFiles $repo) {
    $bytes = [IO.File]::ReadAllBytes($file.Full)
    $at = [Array]::IndexOf($bytes, [byte]0)
    if ($at -ge 0) { $hits += "$($file.Relative): byte $at" }
    $scanned++
}

if ($hits.Count -gt 0) {
    throw ("tracked text files contain NUL bytes:" + [Environment]::NewLine + ($hits -join [Environment]::NewLine))
}
Write-Host "$scanned tracked text files contain no NUL byte"
