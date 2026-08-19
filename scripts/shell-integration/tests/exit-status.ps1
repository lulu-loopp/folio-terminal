# What `folio.ps1` puts in `OSC 133;D`, measured rather than remembered.
#
# Every case below starts a real PowerShell, installs the integration script into it, and hands it
# commands the way a console session does — through the integration's own `PSConsoleHostReadLine`
# wrapper, which is where a typed line arrives from. PSReadLine cannot read a redirected stdin and
# returns nothing there, so the wrapper's *inner* read-line is replaced by a queue holding the
# case's lines; everything downstream is the host's own: it runs each line, it sets `$?` and
# `$LASTEXITCODE`, and it calls `prompt`. The markers this collects are therefore the bytes the
# terminal would really receive, not a re-derivation of them.
#
#     pwsh -NoProfile -File scripts/shell-integration/tests/exit-status.ps1
#     pwsh -NoProfile -File scripts/shell-integration/tests/exit-status.ps1 -PowerShellHost powershell.exe
#
# The three install orders are the point of the exercise. `conda init powershell` writes its hook
# into the *all-hosts* profile, which loads before the per-host one that usually carries the `.` of
# this script — but either file can carry either, so both nestings are real, and a status that
# depends on which one a machine happens to have is not a status.

param(
    [string]$PowerShellHost = 'pwsh',
    [string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1')
)

$ErrorActionPreference = 'Stop'
$Script = (Resolve-Path -LiteralPath $Script).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ('folio-exit-status-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null

# A prompt with no text of its own, so the markers are all that is under test.
$stub = Join-Path $work 'stub-prompt.ps1'
Set-Content -LiteralPath $stub -Value "function global:prompt { 'P> ' }" -Encoding ascii

# Conda's own prompt nesting, copied from `<anaconda>\shell\condabin\Conda.psm1`. The `Write-Host`
# is the whole mechanism this file exists for: it runs before any prompt nested inside it, and it
# leaves `$?` reporting the success of *itself*.
$conda = Join-Path $work 'conda.ps1'
$condaBody = @(
    "`$Env:CONDA_PROMPT_MODIFIER = '(base) '",
    'if (Test-Path Function:\prompt) {',
    '    Rename-Item Function:\prompt CondaPromptBackup',
    '} else {',
    "    function CondaPromptBackup() { 'PS> ' }",
    '}',
    'function global:prompt() {',
    '    if ($Env:CONDA_PROMPT_MODIFIER) {',
    '        $Env:CONDA_PROMPT_MODIFIER | Write-Host -NoNewline',
    '    }',
    '    CondaPromptBackup;',
    '}')
Set-Content -LiteralPath $conda -Value ($condaBody -join [Environment]::NewLine) -Encoding ascii

# case name = @{ Lines; D; C } — the exact `133;D` payloads expected, in order, and how many
# `133;C` the case is owed. `<redraw>` is not a command: it is PSReadLine drawing the prompt again
# in the middle of a line, which is what Folio's own resize chord makes it do.
$cases = [ordered]@{
    'a native command that fails carries its own code' = @{
        Lines = @('cmd /c exit 3'); D = @('3'); C = 1
    }
    'a native command that succeeds carries zero' = @{
        Lines = @('cmd /c exit 0'); D = @('0'); C = 1
    }
    'a cmdlet failing behind a native success is not that success' = @{
        Lines = @('cmd /c exit 0', 'Get-Item folio-no-such-path'); D = @('0', '1'); C = 2
    }
    'a cmdlet failing with no native code at all reports one' = @{
        Lines = @('Get-Item folio-no-such-path'); D = @('1'); C = 1
    }
    'a cmdlet succeeding behind a native failure is a success' = @{
        Lines = @('cmd /c exit 3', 'Get-Date | Out-Null'); D = @('3', '0'); C = 2
    }
    'the same failure run twice keeps its code both times' = @{
        Lines = @('cmd /c exit 3', 'cmd /c exit 3'); D = @('3', '3'); C = 2
    }
    'a failing pipeline reports one' = @{
        Lines = @('Get-Item folio-no-such-path | Select-Object -First 1'); D = @('1'); C = 1
    }
    'a native failure inside a pipeline still carries its code' = @{
        Lines = @('cmd /c exit 3 | Out-Null'); D = @('3'); C = 1
    }
    'a terminating error reports one' = @{
        Lines = @('throw ''boom'''); D = @('1'); C = 1
    }
    'an assignment after a native failure is not that failure' = @{
        Lines = @('cmd /c exit 3', '$x = 1'); D = @('3', '0'); C = 2
    }
    'an empty line is not a command' = @{
        Lines = @('cmd /c exit 3', ''); D = @('3'); C = 1
    }
    'a line of whitespace is not a command' = @{
        Lines = @('cmd /c exit 3', '   '); D = @('3'); C = 1
    }
    'a comment is not a command' = @{
        Lines = @('cmd /c exit 3', '# nothing here'); D = @('3'); C = 1
    }
    'a line the parser refuses is a command, and it failed' = @{
        Lines = @('cmd /c exit 0', '}'); D = @('0', '1'); C = 2
    }
    'a prompt redrawn mid-line answers for nothing' = @{
        Lines = @('cmd /c exit 3', '<redraw>', 'Get-Date | Out-Null'); D = @('3', '0'); C = 2
    }
}

function Invoke-Case {
    param([string]$Order, [string[]]$Lines)

    $queue = Join-Path $work 'queue.ps1'
    $body = @('$Global:__q = New-Object System.Collections.Queue')
    $redraw = $false
    foreach ($line in $Lines) {
        if ($line -eq '<redraw>') { $redraw = $true; continue }
        $body += '$Global:__q.Enqueue(' + "'" + ($line -replace "'", "''") + "')"
    }
    if ($redraw) { $body += '$Global:__redraw = $true' }
    # The read-line the integration wraps. A `<redraw>` calls the prompt function from inside it,
    # which is exactly where `[PSConsoleReadLine]::InvokePrompt` calls it from. Returning $null
    # once the queue is empty puts the host back on its redirected stdin, which is at end of file.
    $body += '$Global:__FolioShellIntegration.OriginalReadLine = { if ($Global:__redraw) { $Global:__redraw = $false; [void](prompt) }; if ($Global:__q.Count) { $Global:__q.Dequeue() } else { $null } }'
    Set-Content -LiteralPath $queue -Value ($body -join [Environment]::NewLine) -Encoding ascii

    $setup = switch ($Order) {
        'folio alone' { @(". '$stub'", ". '$Script'") }
        'conda installed before us' { @(". '$stub'", ". '$conda'", ". '$Script'") }
        'conda installed after us' { @(". '$stub'", ". '$Script'", ". '$conda'") }
    }
    $setup += ". '$queue'"
    $setup += "'<<<CASE>>>'"

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = (Get-Command $PowerShellHost).Source
    # `Arguments` rather than `ArgumentList`: this file has to run under both generations, and
    # 5.1's .NET Framework has no `ArgumentList`. Neither switch needs quoting.
    $psi.Arguments = '-NoProfile -NoLogo'
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($psi)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.Write(($setup -join [Environment]::NewLine) + [Environment]::NewLine)
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(60000)) {
        $process.Kill()
        $process.WaitForExit()
        throw "the '$Order' session never reached the end of its input"
    }
    $text = $stdout.Result
    $index = $text.IndexOf('<<<CASE>>>')
    if ($index -ge 0) { $text = $text.Substring($index) }
    return [pscustomobject]@{
        D = @([regex]::Matches($text, "\]133;D;(-?\d+)") |
                ForEach-Object { $_.Groups[1].Value })
        C = ([regex]::Matches($text, "\]133;C")).Count
        Transcript = $text
        Errors = $stderr.Result
    }
}

$failures = @()
$checked = 0
foreach ($order in 'folio alone', 'conda installed before us', 'conda installed after us') {
    foreach ($case in $cases.Keys) {
        $expected = $cases[$case]
        $actual = Invoke-Case -Order $order -Lines $expected.Lines
        $checked++
        $wantD = ($expected.D -join ',')
        $gotD = ($actual.D -join ',')
        if ($wantD -ne $gotD -or $expected.C -ne $actual.C) {
            $failures += "[$order] $case" + [Environment]::NewLine +
                "    expected D=[$wantD] C=$($expected.C)" + [Environment]::NewLine +
                "    actual   D=[$gotD] C=$($actual.C)"
        }
    }
}

Remove-Item -LiteralPath $work -Recurse -Force

if ($failures.Count -gt 0) {
    throw ('OSC 133;D reports the wrong status:' + [Environment]::NewLine +
        ($failures -join [Environment]::NewLine))
}
Write-Host "$checked exit-status cases agree, across all three prompt install orders."
