[CmdletBinding()]
param()

# TELEPROMPTER, not a test: run this in a SEPARATE ordinary terminal (WT or
# any PowerShell), and perform every action in the BetterTerminal window.
# Running it inside BetterTerminal captures the prompt behind Read-Host and
# leaves you nowhere to type the commands under test (user-reported).

$ErrorActionPreference = 'Stop'

function Show-Page {
    param(
        [Parameter(Mandatory)] [string] $Title,
        [Parameter(Mandatory)] [string[]] $Lines
    )

    Clear-Host
    Write-Host "M1.7 | $Title" -ForegroundColor Cyan
    $Lines | ForEach-Object { Write-Host $_ }
    [void] (Read-Host 'Finish this page, record PASS/FAIL, then press Enter')
}

Show-Page '1/5 Frozen scrollback and scroll indicator' @(
    'In BetterTerminal run:'
    '1..200 | % { "M17-{0:D3} abcdefghijklmnopqrstuvwxyz" -f $_ }; Start-Sleep 5; 201..220 | % { "M17-{0:D3} abcdefghijklmnopqrstuvwxyz" -f $_; Start-Sleep -Milliseconds 25 }'
    'During the five-second pause, wheel up and stop on an older numbered line.'
    'Expect: the static buffer immediately shows bottom-right "N lines below"; cursor is hidden.'
    'When the second batch starts, expect: old lines stay fixed and N grows.'
    'Wheel down to bottom. Expect: marker disappears and live following resumes.'
)

Show-Page '2/5 Keyboard paging and input snap' @(
    'After generating history, press Shift+PageUp/PageDown; expect exactly page-sized movement.'
    'Press Ctrl+Home, then Ctrl+End; expect top, then live bottom.'
    'Scroll up again and type one printable key.'
    'Expect: view snaps to bottom before the key reaches PowerShell.'
)

Show-Page '3/5 Linear, word, line, and wide selection' @(
    'Print: Write-Output "alpha beta 中🙂 tail"'
    'Drag across multiple output lines; double-click beta; triple-click the output line.'
    'Expect: blue background only, original foreground retained, no half-wide glyph selected.'
    'Use Ctrl+Shift+C, paste into Notepad, and verify soft visual wraps add no newline.'
    'Verify hard output rows paste with Windows CRLF and trailing padding is absent.'
)

Show-Page '4/5 Ctrl+C dual behavior' @(
    'Select text and press Ctrl+C. Expect: clipboard receives it and highlight clears.'
    'With no selection run: Start-Sleep 30'
    'Press Ctrl+C. Expect: PowerShell is interrupted (no clipboard command is taken).'
)

$vim = Get-Command vim -ErrorAction SilentlyContinue
if ($vim) {
    $mouseSteps = @(
        'Run vim, then :set mouse=a and open enough lines to scroll.'
        'Click to reposition, drag, and wheel. Expect: vim receives mouse events.'
        'Hold Shift and drag. Expect: BetterTerminal local selection instead of vim mouse.'
        'Hold Shift and wheel. Expect: no local alt-screen history movement.'
        'Start an IME composition, click/drag once, then finish it. Expect: no crash or lost commit.'
        'Exit with :q!.'
    )
} else {
    $mouseSteps = @(
        'vim was not found; record application-mouse checks as N/A.'
        'Still start an IME composition, click/drag once, then finish it.'
        'Expect: no crash, forced commit, or swallowed final commit.'
    )
}
Show-Page '5/5 Alt-screen mouse forwarding and IME' $mouseSteps

Clear-Host
Write-Host 'M1.7 manual acceptance complete. Report PASS/FAIL for each page.' -ForegroundColor Green
Write-Host 'Known automation limit: ui-probe input does not enter winit mouse/keyboard events.'
