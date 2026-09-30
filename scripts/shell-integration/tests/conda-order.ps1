# PIN (existing behaviour, T-RESET-MODES follow-up): a conda-shaped prompt
# wrapper installed after folio.ps1 is adopted into Folio's chain, Folio takes
# the global prompt name back on the first draw, and every draw carries the
# OSC 133 prompt marks with each prompt in the chain run exactly once.
#
# The wrapper is synthesised here in the shape of conda's PowerShell module
# (`Conda.psm1`'s prompt change): rename the current `prompt` to
# `CondaPromptBackup`, install a global `prompt` that writes
# CONDA_PROMPT_MODIFIER and then calls the backup. No conda install is read or
# run. Always launched with `-NoProfile`.
#
# MUTATION: remove the `Set-Item -LiteralPath Function:Global:prompt` re-take
# in folio.ps1 and this fails with "Folio did not retake ...".

param([string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1'))

$ErrorActionPreference = 'Stop'
$Script = (Resolve-Path -LiteralPath $Script).Path
$env:TERM_PROGRAM = 'Folio'
$Global:BaseDraws = 0
$Global:CondaDraws = 0
# A mixed-script prompt, spelled by code point so Windows PowerShell 5.1 reads
# this BOM-less file the same way PowerShell 7 does.
$Global:BaseText = 'BASE ' + [char]0x4E3B + [char]0x5C4F + '> '
function global:prompt { $Global:BaseDraws++; $Global:BaseText }

. $Script
$selfPrompt = $Global:__FolioShellIntegration.SelfPrompt

# Conda's order when its initialization runs after folio.ps1.
$Env:CONDA_PROMPT_MODIFIER = '(base) '
Rename-Item Function:\prompt CondaPromptBackup
function global:prompt {
    $Global:CondaDraws++
    if ($Env:CONDA_PROMPT_MODIFIER) {
        $Env:CONDA_PROMPT_MODIFIER | Write-Host -NoNewline
    }
    CondaPromptBackup
}

$esc = [string][char]27
$bel = [string][char]7
$first = [string](prompt)
$installed = Get-Command prompt -CommandType Function -ErrorAction Stop
if (-not [object]::ReferenceEquals($installed.ScriptBlock, $selfPrompt)) {
    throw 'Folio did not retake the global prompt name after conda wrapped it.'
}
$second = [string](prompt)
foreach ($draw in @($first, $second)) {
    if (-not $draw.Contains($esc + ']133;A' + $bel) -or
        -not $draw.Contains($esc + ']133;B' + $bel)) {
        throw 'A prompt draw after the conda override did not contain OSC 133 A/B marks.'
    }
    if (-not $draw.Contains($Global:BaseText)) {
        throw 'The prompt conda wrapped was lost from the prompt chain.'
    }
}
if ($Global:BaseDraws -ne 2 -or $Global:CondaDraws -ne 2) {
    throw "Each prompt in the chain runs once per draw; after two draws the original ran $($Global:BaseDraws) times and conda's $($Global:CondaDraws)."
}
if ($Global:__FolioShellIntegration.PromptChain.Count -ne 2) {
    throw "Expected conda and the original prompt in the chain; found $($Global:__FolioShellIntegration.PromptChain.Count)."
}

Write-Host 'conda-after-Folio prompt order keeps OSC 133 marks and re-hoists Folio.'
exit 0
