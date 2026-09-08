# What `folio.ps1` tells the prompt it wraps, and what it tells the terminal about where that
# prompt left the shell — measured rather than remembered.
#
# Every case starts a real PowerShell, installs a prompt of its own, dot-sources the integration
# on top of it so that the reader's prompt is the *chained* one, and then hands the session a line
# through the integration's own `PSConsoleHostReadLine` wrapper — the same route
# `exit-status.ps1` uses, and for the same reason: what this collects is the bytes the terminal
# would really receive and the values the reader's own prompt would really see.
#
#     pwsh -NoProfile -File scripts/shell-integration/tests/prompt-chain.ps1
#     pwsh -NoProfile -File scripts/shell-integration/tests/prompt-chain.ps1 -PowerShellHost powershell.exe
#
# Nothing outside a temporary directory is written, and `$PROFILE` is never read: every session is
# started with `-NoProfile` and told what to load on its standard input.

param(
    [string]$PowerShellHost = 'pwsh',
    [string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1')
)

$ErrorActionPreference = 'Stop'
$Script = (Resolve-Path -LiteralPath $Script).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) ('folio-prompt-chain-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$elsewhere = Join-Path $work 'elsewhere'
New-Item -ItemType Directory -Path $elsewhere | Out-Null

# A prompt that moves the shell, installed *before* the integration so that the integration chains
# it. A prompt customizer that changes directory is not exotic — it is what a per-directory
# environment tool does — and the question the case asks is which of the two directories the
# terminal is told about.
$mover = Join-Path $work 'mover-prompt.ps1'
Set-Content -LiteralPath $mover -Encoding ascii -Value @(
    'function global:prompt {',
    '    $Global:__moves = $Global:__moves + 1',
    "    `$leaf = 'd' + `$Global:__moves",
    "    `$place = Join-Path '$elsewhere' `$leaf",
    '    New-Item -ItemType Directory -Path $place -Force | Out-Null',
    '    Set-Location -LiteralPath $place',
    '    [Console]::Write("MOVED[" + $leaf + "]" + [char]10)',
    "    'P> '",
    '}')

# A prompt that reports the one variable it is owed: `$?`, as the shell set it for the line that
# just ran. It has to be the first statement of the function, which is where a prompt customizer
# that cares about it puts it too.
$reporter = Join-Path $work 'reporter-prompt.ps1'
Set-Content -LiteralPath $reporter -Encoding ascii -Value @(
    'function global:prompt {',
    '    $seen = $?',
    '    [Console]::Write("QMARK[" + $seen + "]" + [char]10)',
    "    'P> '",
    '}')

function Invoke-Session {
    param([string]$Prompt, [string[]]$Lines)

    $queue = Join-Path $work 'queue.ps1'
    $body = @('$Global:__q = New-Object System.Collections.Queue')
    foreach ($line in $Lines) {
        $body += '$Global:__q.Enqueue(' + "'" + ($line -replace "'", "''") + "')"
    }
    $body += '$Global:__FolioShellIntegration.OriginalReadLine = { if ($Global:__q.Count) { $Global:__q.Dequeue() } else { $null } }'
    Set-Content -LiteralPath $queue -Value ($body -join [Environment]::NewLine) -Encoding ascii

    $setup = @(
        "Set-Location -LiteralPath '$work'",
        ". '$Prompt'",
        ". '$Script'",
        ". '$queue'",
        "''")

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = (Get-Command $PowerShellHost).Source
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
        throw 'the session never reached the end of its input'
    }
    # No marker to cut at: every prompt drawn after the integration was dot-sourced is one of
    # the prompts under test, and which report belongs to which prompt is the whole question.
    $text = $stdout.Result
    return [pscustomobject]@{
        QMarks = @([regex]::Matches($text, "QMARK\[(\w+)\]") | ForEach-Object { $_.Groups[1].Value })
        Transcript = $text
        Errors = $stderr.Result
    }
}

$failures = @()
$checked = 0

function Test-Case {
    param([string]$Name, [string]$Expected, [string]$Actual)
    $script:checked++
    if ($Expected -ne $Actual) {
        $script:failures += "$Name" + [Environment]::NewLine +
            "    expected $Expected" + [Environment]::NewLine +
            "    actual   $Actual"
    }
}

# ── R3-15 ───────────────────────────────────────────────────────────────────────────────
# The directory is reported for where the prompt *left* the shell, not for where it found it.
#
# The chained prompt moves somewhere new on every call, so the n-th report has exactly one right
# answer: the n-th directory. A report composed before the chain ran names the n-1-th, and the
# whole run is one prompt behind.
$moved = Invoke-Session -Prompt $mover -Lines @('Get-Date | Out-Null')
# The two tokens in the order the terminal received them: the move the chained prompt made, and
# the directory this prompt reported. A report belongs to the move that came before it, and it has
# exactly one right answer — the place that move left the shell in. A report composed before the
# chain ran names the move before that one, and the whole session is one prompt behind.
$standing = '<the directory the session started in>'
$behind = @()
$reports = 0
foreach ($token in [regex]::Matches($moved.Transcript, "MOVED\[(\w+)\]|\]7;([^`a]*)`a")) {
    if ($token.Groups[1].Success) {
        $standing = $token.Groups[1].Value
        continue
    }
    $reports++
    if (-not $token.Groups[2].Value.EndsWith('/' + $standing)) {
        $behind += "report $reports names $($token.Groups[2].Value), not …/$standing"
    }
}
Test-Case 'R3-15 every report names the directory the chained prompt left the shell in' `
    '' ($behind -join '; ')
Test-Case 'R3-15 the chained prompt was reached at all' `
    $true ($reports -gt 0)

# ── R3-16 ───────────────────────────────────────────────────────────────────────────────
# The chained prompt sees the success of the reader's line, not the success of the assignment the
# wrapper made on its way to calling it.
$failed = Invoke-Session -Prompt $reporter -Lines @('Get-Item folio-no-such-path')
Test-Case 'R3-16 a failed line reaches the chained prompt as a failure' `
    $true ($failed.QMarks -contains 'False')

$succeeded = Invoke-Session -Prompt $reporter -Lines @('Get-Date | Out-Null')
Test-Case 'R3-16 a line that succeeded reaches it as a success and nothing else does' `
    $false ($succeeded.QMarks -contains 'False')

Remove-Item -LiteralPath $work -Recurse -Force

if ($failures.Count -gt 0) {
    throw ('the prompt chain is told the wrong thing:' + [Environment]::NewLine +
        ($failures -join [Environment]::NewLine))
}
Write-Host "$checked prompt-chain cases agree."
