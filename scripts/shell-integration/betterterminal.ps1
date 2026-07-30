# BetterTerminal OSC 133 shell integration for PowerShell 7 and Windows PowerShell 5.1.
# Opt in from $PROFILE after prompt customizers (for example oh-my-posh):
#   . 'D:\path\to\BetterTerminal\scripts\shell-integration\betterterminal.ps1'

if ($Global:__BetterTerminalShellIntegration -and
    $Global:__BetterTerminalShellIntegration.Installed) {
    return
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
    [Console]::Write("`e]133;C`a")
    return $commandLine
}

function Global:prompt {
    # Capture status before prompt code or history inspection can overwrite it.
    $lastSucceeded = $?
    $nativeExitCode = $Global:LASTEXITCODE
    $state = $Global:__BetterTerminalShellIntegration
    $out = ''

    if ($state.CommandStarted) {
        if ($lastSucceeded) {
            $exitCode = 0
        } elseif ($null -ne $nativeExitCode) {
            $exitCode = $nativeExitCode
        } else {
            $exitCode = 1
        }
        $out += "`e]133;D;$exitCode`a"
    }

    $out += "`e]133;A`a"
    $out += (& $state.OriginalPrompt)
    $out += "`e]133;B`a"
    $state.CommandStarted = $true
    return $out
}
