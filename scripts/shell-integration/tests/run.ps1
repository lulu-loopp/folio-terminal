# Every shell-integration measurement in this directory, in one run.
#
#     pwsh -NoProfile -File scripts/shell-integration/tests/run.ps1
#
# Each file below starts real shells and reads the bytes they put on the wire, so each one needs
# the shell it measures to be on this machine. A shell that is not installed is reported as
# skipped rather than passed: the difference between "these markers are right" and "nobody
# looked" is the whole value of the run.

param([switch]$SkipWindowsPowerShell)

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$failures = @()
$ran = 0
$skipped = @()

function Invoke-Suite {
    param([string]$Name, [string]$Program, [string[]]$Arguments)

    $found = Get-Command $Program -ErrorAction SilentlyContinue
    if ($null -eq $found) {
        $script:skipped += "$Name (no $Program on this machine)"
        return
    }
    Write-Host "── $Name"
    & $found.Source @Arguments
    if ($LASTEXITCODE -ne 0) {
        $script:failures += "$Name exited $LASTEXITCODE"
    }
    $script:ran++
}

Invoke-Suite -Name 'folio.bash hooks' -Program 'bash' `
    -Arguments @((Join-Path $here 'bash-hooks.sh'))
# There is no zsh on every machine, and the script cannot be measured without one. What can be
# asked of any machine that has a zsh is whether the file parses, which is the failure that would
# otherwise reach a reader as a shell that will not start.
Invoke-Suite -Name 'folio.zsh parses' -Program 'zsh' `
    -Arguments @('-n', (Join-Path $here '..' 'folio.zsh'))
Invoke-Suite -Name 'folio.ps1 exit status (pwsh)' -Program 'pwsh' `
    -Arguments @('-NoProfile', '-File', (Join-Path $here 'exit-status.ps1'))
Invoke-Suite -Name 'folio.ps1 prompt chain (pwsh)' -Program 'pwsh' `
    -Arguments @('-NoProfile', '-File', (Join-Path $here 'prompt-chain.ps1'))
if (-not $SkipWindowsPowerShell) {
    Invoke-Suite -Name 'folio.ps1 exit status (Windows PowerShell 5.1)' -Program 'powershell' `
        -Arguments @('-NoProfile', '-File', (Join-Path $here 'exit-status.ps1'),
            '-PowerShellHost', 'powershell.exe')
    Invoke-Suite -Name 'folio.ps1 prompt chain (Windows PowerShell 5.1)' -Program 'powershell' `
        -Arguments @('-NoProfile', '-File', (Join-Path $here 'prompt-chain.ps1'),
            '-PowerShellHost', 'powershell.exe')
}

foreach ($note in $skipped) { Write-Host "skipped: $note" }
if ($failures.Count -gt 0) {
    throw ($failures -join [Environment]::NewLine)
}
Write-Host "$ran shell-integration suites agree."
