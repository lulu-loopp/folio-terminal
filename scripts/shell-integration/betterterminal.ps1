# BetterTerminal OSC 133 shell integration for PowerShell 7 and Windows PowerShell 5.1.
# Opt in from $PROFILE after prompt customizers (for example oh-my-posh):
#   . 'D:\path\to\BetterTerminal\scripts\shell-integration\betterterminal.ps1'

if ($Global:__BetterTerminalShellIntegration -and
    $Global:__BetterTerminalShellIntegration.Installed) {
    return
}

# Hyperlink capability declaration, honest and scoped: BetterTerminal renders OSC 8, but
# identity-allowlisted CLIs (Claude Code 2.1.220 keys on WT_SESSION / TERM_PROGRAM=iTerm.app)
# downgrade links for unknown terminals. FORCE_HYPERLINK is their documented capability override,
# so declare it — only inside BetterTerminal sessions, and never clobber a user's explicit choice.
# Known trade-off: supports-hyperlinks-family CLIs honor this even with redirected output, so a
# command piping to a file may carry OSC 8 bytes; revisit as a setting when the settings slice lands.
if ($env:TERM_PROGRAM -eq 'BetterTerminal' -and -not (Test-Path env:FORCE_HYPERLINK)) {
    $env:FORCE_HYPERLINK = '1'
}

# PSConsoleHostReadLine is the supported console-host extension point. Importing PSReadLine here
# makes its original entry point available on both supported PowerShell generations.
Import-Module PSReadLine -ErrorAction SilentlyContinue

$originalPrompt = (Get-Command prompt -CommandType Function -ErrorAction Stop).ScriptBlock
$readLineCommand = Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue
if (-not $readLineCommand) {
    Write-Warning 'BetterTerminal shell integration requires PSReadLine; no changes were installed.'
    return
}
$originalReadLine = if ($readLineCommand -is [System.Management.Automation.FunctionInfo]) {
    $readLineCommand.ScriptBlock
} else {
    $readLineCommand
}

$Global:__BetterTerminalShellIntegration = @{
    Installed = $true
    OriginalPrompt = $originalPrompt
    OriginalReadLine = $originalReadLine
    CommandStarted = $false
}

function Global:PSConsoleHostReadLine {
    $original = $Global:__BetterTerminalShellIntegration.OriginalReadLine
    $commandLine = & $original
    # [char]27: Windows PowerShell 5.1 has no `e escape; this form works on both generations.
    [Console]::Write(([string][char]27) + ']133;C' + [char]7)
    return $commandLine
}

function Global:prompt {
    # Capture status before prompt code or history inspection can overwrite it.
    $lastSucceeded = $?
    $nativeExitCode = $Global:LASTEXITCODE
    $state = $Global:__BetterTerminalShellIntegration
    $esc = [string][char]27
    $bel = [string][char]7
    $out = ''

    if ($state.CommandStarted) {
        if ($lastSucceeded) {
            $exitCode = 0
        } elseif ($null -ne $nativeExitCode) {
            $exitCode = $nativeExitCode
        } else {
            $exitCode = 1
        }
        $out += $esc + ']133;D;' + $exitCode + $bel
    }

    $out += $esc + ']133;A' + $bel
    $out += (& $state.OriginalPrompt)
    $out += $esc + ']133;B' + $bel
    $state.CommandStarted = $true
    return $out
}
