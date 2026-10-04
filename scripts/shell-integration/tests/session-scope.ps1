# The persistent profile line is inert outside Folio and active in a Folio child session.
# This file is run under both PowerShell editions, always with -NoProfile.

param([string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1'))

$ErrorActionPreference = 'Stop'
$Script = (Resolve-Path -LiteralPath $Script).Path

Remove-Item Env:\TERM_PROGRAM -ErrorAction SilentlyContinue
Remove-Item Env:\FORCE_HYPERLINK -ErrorAction SilentlyContinue
Remove-Variable __FolioShellIntegration -Scope Global -ErrorAction SilentlyContinue
. $Script
if (Test-Path Variable:\Global:__FolioShellIntegration) {
    throw 'folio.ps1 installed outside a Folio session'
}
if (Test-Path Env:\FORCE_HYPERLINK) {
    throw 'folio.ps1 changed the environment outside a Folio session'
}

$env:TERM_PROGRAM = 'Folio'
. $Script
if (-not $Global:__FolioShellIntegration.Installed) {
    throw 'folio.ps1 did not install in a Folio session'
}

Write-Host 'PowerShell session scope agrees.'
