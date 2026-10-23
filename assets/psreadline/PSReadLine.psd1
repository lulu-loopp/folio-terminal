# Folio's manifest for PSReadLine 2.4.5 (BSD 2-Clause, Copyright (c) 2013, Jason Shirk; the
# binaries are Microsoft's, unmodified, from the PowerShell Gallery package pinned in
# vendor/psreadline/). Derived from upstream's PSReadLine.psd1: the module is binary-only, so
# RootModule is the assembly and there is no PSReadLine.psm1, no PSReadLine.format.ps1xml and no
# FunctionsToExport. Execution policy governs those two kinds of file and not a manifest or an
# assembly, so this module loads under every policy. PSConsoleHostReadLine, the one function the
# upstream .psm1 defines, is defined by Folio's shell-integration loader instead.
@{
RootModule = 'Microsoft.PowerShell.PSReadLine.dll'
ModuleVersion = '2.4.5'
GUID = '5714753b-2afd-4492-a5fd-01d9e2cff8b5'
Author = 'Microsoft Corporation'
CompanyName = 'Microsoft Corporation'
Copyright = '(c) Microsoft Corporation. All rights reserved.'
Description = 'Great command line editing in the PowerShell console host'
PowerShellVersion = '5.1'
AliasesToExport = @()
CmdletsToExport = 'Get-PSReadLineKeyHandler','Set-PSReadLineKeyHandler','Remove-PSReadLineKeyHandler',
                  'Get-PSReadLineOption','Set-PSReadLineOption'
HelpInfoURI = 'https://aka.ms/powershell75-help'
}
