param([string]$Script = (Join-Path $PSScriptRoot '..\folio.ps1'))

$ErrorActionPreference = 'Stop'
$source = [IO.File]::ReadAllText($Script)
$both = 'Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine'

if (($source.Split($both).Count - 1) -lt 2) {
    throw 'folio.ps1 does not accept both registered PSReadLine module names at every gate'
}
if ($source -notmatch 'if \(-not \$psReadLineModule\)\s*\{\s*Import-Module PSReadLine') {
    throw 'folio.ps1 imports PSReadLine even when one of the accepted module names is loaded'
}
if ($source -notmatch '\$psReadLineVersion\s*=\s*\$psReadLineModule\.Version') {
    throw 'folio.ps1 does not read its version from the accepted loaded module'
}

Write-Host 'PSReadLine dual-name gates agree.'
