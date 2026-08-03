# BetterTerminal OSC 133 + OSC 7 shell integration for PowerShell 7 and Windows PowerShell 5.1.
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
    # OSC 7 working-directory URI builder, kept in this table rather than as a global function so
    # the integration adds no command name to the user's session.
    WorkingDirectoryUri = {
        param([string]$literalPath)
        # RFC 3986 path characters that never need escaping: unreserved + sub-delims + ':' + '@',
        # plus '/', which is the separator itself. Everything else — space, '%', '#', '?' and every
        # non-ASCII character — is percent-encoded byte by byte from UTF-8, the only encoding a
        # file URI has. This is minimal and correct: 'D:/My Pictures/图片' becomes
        # 'D:/My%20Pictures/%E5%9B%BE%E7%89%87' and nothing else moves.
        $safe = '-._~!$&''()*+,;=:@/'
        $builder = New-Object System.Text.StringBuilder
        foreach ($byte in [System.Text.Encoding]::UTF8.GetBytes($literalPath.Replace('\', '/'))) {
            if (($byte -ge 0x41 -and $byte -le 0x5A) -or
                ($byte -ge 0x61 -and $byte -le 0x7A) -or
                ($byte -ge 0x30 -and $byte -le 0x39) -or
                $safe.IndexOf([char]$byte) -ge 0) {
                [void]$builder.Append([char]$byte)
            } else {
                [void]$builder.AppendFormat('%{0:X2}', $byte)
            }
        }
        # Empty authority: the file-URI spelling of "this host". BetterTerminal also accepts
        # 'localhost' and this machine's name, but the empty form needs no lookup per prompt and
        # cannot go stale.
        return 'file:///' + $builder.ToString().TrimStart('/')
    }
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

    # OSC 7: the authoritative working directory, reported once per prompt. It is what lets
    # BetterTerminal resolve './x.png' and '../a/b.svg' in this session's output; a terminal that
    # is never told a directory deliberately leaves relative paths undetected rather than guessing
    # one. A location on a non-filesystem provider (HKLM:, Cert:, …) has no directory to resolve
    # against, so the report is sent empty, which retracts the previous one instead of leaving it
    # to answer for a place the shell has left.
    $location = $ExecutionContext.SessionState.Path.CurrentLocation
    if ($location.Provider.Name -eq 'FileSystem') {
        $out += $esc + ']7;' + (& $state.WorkingDirectoryUri $location.ProviderPath) + $bel
    } else {
        $out += $esc + ']7;' + $bel
    }

    $out += $esc + ']133;A' + $bel
    $out += (& $state.OriginalPrompt)
    $out += $esc + ']133;B' + $bel
    $state.CommandStarted = $true
    return $out
}
