# no_tracked_text_file_contains_a_conflict_marker
#
# Git's unmerged-index check cannot see markers that were staged and committed.
# Read raw bytes and refuse only the two line-leading forms Git writes. A NUL in
# the same scope is owned by check-no-nul.ps1.

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $PSScriptRoot 'tracked-text-files.ps1')

$hits = @()
$scanned = 0
foreach ($file in Get-TrackedTextFiles $repo) {
    $bytes = [IO.File]::ReadAllBytes($file.Full)
    # Latin-1 is a one-byte-to-one-character view, so no invalid text can be
    # skipped and ASCII marker bytes stay exactly themselves.
    $text = [Text.Encoding]::Latin1.GetString($bytes)
    foreach ($match in [regex]::Matches($text, '(?m)^(?:<<<<<<< |>>>>>>> )')) {
        $line = 1 + $text.Substring(0, $match.Index).Split("`n").Count - 1
        $hits += "$($file.Relative):$line"
    }
    $scanned++
}

if ($hits.Count -gt 0) {
    throw ("tracked text files contain committed conflict markers:" + [Environment]::NewLine + ($hits -join [Environment]::NewLine))
}
Write-Host "$scanned tracked text files contain no committed conflict marker"
