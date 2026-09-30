# T-MARKS-CONDA: a prompt wrapper installed after folio.ps1 is adopted and
# Folio retakes the global prompt name on the first draw.
#
# The wrapper below is the prompt-changing body from the installed
# `<anaconda>\shell\condabin\Conda.psm1`: it renames the current prompt,
# writes CONDA_PROMPT_MODIFIER, and then calls the renamed prompt. This test is
# always launched with `-NoProfile`; it neither reads nor writes the owner's
# profile.

param([string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1'))

$ErrorActionPreference = 'Stop'
$Script = (Resolve-Path -LiteralPath $Script).Path
$env:TERM_PROGRAM = 'Folio'
function global:prompt { 'BASE> ' }

. $Script
$selfPrompt = $Global:__FolioShellIntegration.SelfPrompt

# Conda's order when its initialization appears later in a profile.
$Env:CONDA_PROMPT_MODIFIER = '(base) '
if (Test-Path Function:\prompt) {
    Rename-Item Function:\prompt CondaPromptBackup
} else {
    function CondaPromptBackup { 'PS> ' }
}
function global:prompt {
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
    if (-not $draw.Contains('BASE> ')) {
        throw 'The prompt conda wrapped was lost from the prompt chain.'
    }
}
if ($Global:__FolioShellIntegration.PromptChain.Count -ne 2) {
    throw "Expected conda and the original prompt in the chain; found $($Global:__FolioShellIntegration.PromptChain.Count)."
}

Write-Host 'conda-after-Folio prompt order keeps OSC 133 marks and re-hoists Folio.'
