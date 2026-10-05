# no_tracked_text_file_contains_a_conflict_marker
#
# Git's unmerged-index check cannot see markers that were staged and committed. The shared byte
# rule admits only strict UTF-8 text without NUL. Refuse the two line-leading forms Git writes,
# including after a first-line UTF-8 BOM; an intentional fixture needs an exact, live reason row.

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $PSScriptRoot 'tracked-text-files.ps1')

$hits = @()
$scanned = 0
$exemptions = Read-ReasonList (Join-Path $PSScriptRoot 'tracked-conflict-marker-exemptions.tsv') "path`treason"
$seenExemptions = @{}
foreach ($file in Get-TrackedTextFiles $repo) {
    $text = $script:StrictUtf8.GetString($file.Bytes)
    if ($text.StartsWith([char]0xFEFF)) { $text = $text.Substring(1) }
    foreach ($match in [regex]::Matches($text, '(?m)^(?:<<<<<<< |>>>>>>> )')) {
        $line = 1 + $text.Substring(0, $match.Index).Split("`n").Count - 1
        if ($exemptions.ContainsKey($file.Relative)) {
            $seenExemptions[$file.Relative] = $true
        } else {
            $hits += "$($file.Relative):$line"
        }
    }
    $scanned++
}

if ($scanned -eq 0) { throw 'the conflict-marker gate scanned zero tracked text files' }
foreach ($relative in $exemptions.Keys) {
    if (-not $seenExemptions.ContainsKey($relative)) {
        throw "$relative is listed as containing conflict markers but is missing, binary, or no longer needs the exemption"
    }
}

if ($hits.Count -gt 0) {
    throw ("tracked text files contain committed conflict markers:" + [Environment]::NewLine + ($hits -join [Environment]::NewLine))
}
Write-Host "$scanned tracked text files contain no committed conflict marker"
