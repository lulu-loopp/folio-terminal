[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

function Show-Page {
    param(
        [Parameter(Mandatory)] [string] $Title,
        [Parameter(Mandatory)] [string[]] $Lines
    )

    Clear-Host
    Write-Host "M1.6 | $Title" -ForegroundColor Cyan
    $Lines | ForEach-Object { Write-Host $_ }
    [void] (Read-Host 'Finish this page, record any failure, then press Enter')
}

Show-Page '1/4 PSReadLine history and word movement' @(
    'In BetterTerminal run: Write-Output M16-HISTORY'
    'Press Up, then Down. Expect: history appears, then the current line returns.'
    'Type without Enter: echo alpha beta'
    'Press Ctrl+Left/Right. Expect: movement by word; Shift+arrows print no garbage.'
)

Show-Page '2/4 Line editing' @(
    'Type without Enter: Write-Output ABCDEF'
    'Use Home/End, Left/Right, then Delete at a character.'
    'Expect: movement and deletion work; no literal ^[[ sequences appear.'
    'Also press Shift+Tab. Expect: no literal CSI Z text.'
)

Show-Page '3/4 Safe multiline paste' @(
    'Copy these two lines:'
    'Write-Output M16-PASTE-ONE'
    'Write-Output M16-PASTE-TWO'
    'Paste with Ctrl+V, then retry with Shift+Insert.'
    'Expect: same behavior and correct newlines; press Enter once if PSReadLine waits.'
    'Ctrl+V is temporary; its vim block-selection conflict belongs to P2-7.'
)

$pager = if (Get-Command vim -ErrorAction SilentlyContinue) {
    'vim'
} elseif (Get-Command less -ErrorAction SilentlyContinue) {
    'less'
} else {
    $null
}

if ($pager -eq 'vim') {
    $pagerSteps = @(
        'Run: vim'
        'Enter a few lines, press Esc, then use arrows, Home/End, PageUp/PageDown.'
        'Expect: movement/paging works with no literal ^[[A. Use :q! to exit.'
    )
} elseif ($pager -eq 'less') {
    $pagerSteps = @(
        'Run: Get-Help about_* | Out-String | less'
        'Use arrows, Home/End, and PageUp/PageDown.'
        'Expect: movement/paging works with no literal ^[[A. Press q to exit.'
    )
} else {
    $pagerSteps = @(
        'Neither vim nor less was found; record this page as N/A.'
        'When either is available, retest navigation in its full-screen mode.'
        'Expect: the program recognizes SS3 sequences while DECCKM is active.'
    )
}
Show-Page '4/4 Full-screen program (DECCKM)' $pagerSteps

Clear-Host
Write-Host 'M1.6 manual acceptance complete. Report PASS/FAIL for each page.' -ForegroundColor Green
Write-Host 'Known limit: ui-probe keys do not reach winit; evidence is unit tests + this script.'
