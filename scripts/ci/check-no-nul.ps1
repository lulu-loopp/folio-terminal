# no_tracked_text_file_contains_nul
#
# Source searchers commonly classify a file containing NUL as binary and skip it. The shared byte
# rule admits only strict UTF-8 without NUL, requires every intentional binary by exact path, and
# rejects stale binary rows; this gate then refuses zero scanned text files.

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $PSScriptRoot 'tracked-text-files.ps1')

$hits = @()
$scanned = 0
foreach ($file in Get-TrackedTextFiles $repo) {
    $bytes = $file.Bytes
    $at = [Array]::IndexOf($bytes, [byte]0)
    if ($at -ge 0) { $hits += "$($file.Relative): byte $at" }
    $scanned++
}

if ($scanned -eq 0) { throw 'the no-NUL gate scanned zero tracked text files' }

if ($hits.Count -gt 0) {
    throw ("tracked text files contain NUL bytes:" + [Environment]::NewLine + ($hits -join [Environment]::NewLine))
}
Write-Host "$scanned tracked text files contain no NUL byte"
