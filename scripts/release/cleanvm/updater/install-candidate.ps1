<#
.SYNOPSIS
    Install a candidate into the clean Windows guest and keep it as a snapshot:
    the starting point of every updater row (clean-vm.md 4.4, precondition 2).

.DESCRIPTION
        revertToSnapshot clean
        start nogui, wait           a guest operation, then an interactive program
        copy in                     the archive and in-guest.ps1 into <GuestHome>
        run  unpack                 in-guest.ps1's own phase, exactly as run-smoke-in-vm.ps1 runs it
        check                       <GuestHome>\folio\folio.exe exists
        copy out                    results\machine.txt (the archive's SHA-256, FileVersion)
        stop soft
        snapshot <Snapshot>

    Folio itself is never started here. Every row of `run-row.ps1` reverts to
    the snapshot this makes, so each starts from the same installed candidate.

.PARAMETER Vmx
    The guest's `.vmx`.

.PARAMETER Zip
    The candidate, `folio-<version>-windows-x64.zip` (signed, `FOLIO_UPDATER=on`,
    the update card enabled).

.PARAMETER Snapshot
    The name of the snapshot to take, e.g. `a-installed`.

.PARAMETER From
    The snapshot to start from. `clean`.

.PARAMETER VmPassword
    The encryption password of a guest encrypted to carry a vTPM (a parameter;
    no password file is read).

.PARAMETER Results
    Where machine.txt lands. Defaults to target/cleanvm/<vm>-<time>/install.

.EXAMPLE
    ./install-candidate.ps1 -Vmx <win11.vmx> -VmPassword <password> -Zip <folio-0.4.7-windows-x64.zip> -Snapshot a-installed -WhatIf
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Vmx,
    [Parameter(Mandatory)] [string] $Zip,
    [Parameter(Mandatory)] [string] $Snapshot,
    [string] $From = 'clean',
    [string] $VmPassword,
    [string] $GuestUser = 'folio',
    [string] $GuestPassword = 'folio',
    [string] $GuestHome = 'C:\folio-vm',
    [string] $Results,
    [string] $VmrunPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) { $PSNativeCommandUseErrorActionPreference = $false }

$planning = [bool] $WhatIfPreference
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
. (Join-Path $PSScriptRoot 'vm-door.ps1')
. (Join-Path $PSScriptRoot '..\guest-files.ps1')

$inGuest = Join-Path $PSScriptRoot '..\in-guest.ps1'
Assert-GuestReadable -Paths @($inGuest)
if (-not (Test-Path -LiteralPath $Zip -PathType Leaf)) { throw "no archive at $Zip" }
$Zip = (Resolve-Path -LiteralPath $Zip).Path
if (Test-Path -LiteralPath $Vmx -PathType Leaf) { $Vmx = (Resolve-Path -LiteralPath $Vmx).Path }
elseif (-not $planning) { throw "no virtual machine at $Vmx" }
$vmName = [IO.Path]::GetFileNameWithoutExtension($Vmx)
if (-not $Results) {
    $Results = Join-Path $root ("target\cleanvm\{0}-{1}\install" -f $vmName, (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
if (-not $PSCmdlet.ShouldProcess($vmName, "revert to '$From', install $([IO.Path]::GetFileName($Zip)), take snapshot '$Snapshot'")) {
    $planning = $true
}
if (-not $planning) { [IO.Directory]::CreateDirectory($Results) | Out-Null }
$door = New-VmDoor -Vmx $Vmx -VmrunPath $VmrunPath -VmPassword $VmPassword `
    -GuestUser $GuestUser -GuestPassword $GuestPassword -Planning $planning

Write-Host "vmrun    : $($door.Vmrun)"
Write-Host "machine  : $Vmx"
Write-Host "archive  : $Zip"
Write-Host "snapshot : $From -> $Snapshot"
Write-Host "results  : $Results"
if ($planning) { Write-Host 'MODE     : planning only - nothing is started, copied or run.' }

Invoke-Vmrun -Door $door -Step 'revert' -Verb 'revertToSnapshot' -Arguments @($From) | Out-Null
Invoke-Vmrun -Door $door -Step 'start' -Verb 'start' -Arguments @('nogui') | Out-Null
Wait-GuestReady -Door $door
Invoke-Vmrun -Door $door -Step 'mkdir' -Verb 'createDirectoryInGuest' -Arguments @($GuestHome) -InGuest -Tolerant | Out-Null
Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
    -Arguments @($Zip, "$GuestHome\$([IO.Path]::GetFileName($Zip))") | Out-Null
Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
    -Arguments @((Resolve-Path -LiteralPath $inGuest).Path, "$GuestHome\in-guest.ps1") | Out-Null
Invoke-Vmrun -Door $door -Step 'unpack' -Verb 'runProgramInGuest' -InGuest -Arguments @(
    '-interactive', '-activeWindow', 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$GuestHome\in-guest.ps1",
    '-Phase', 'unpack', '-GuestHome', $GuestHome) | Out-Null
Invoke-Vmrun -Door $door -Step 'installed?' -Verb 'fileExistsInGuest' -InGuest -Arguments @("$GuestHome\folio\folio.exe") | Out-Null
Invoke-Vmrun -Door $door -Step 'copy out' -Verb 'copyFileFromGuestToHost' -InGuest -Tolerant `
    -Arguments @("$GuestHome\results\machine.txt", (Join-Path $Results 'machine.txt')) | Out-Null
Invoke-Vmrun -Door $door -Step 'stop' -Verb 'stop' -Arguments @('soft') | Out-Null
Invoke-Vmrun -Door $door -Step 'snapshot' -Verb 'snapshot' -Arguments @($Snapshot) | Out-Null

Write-Host ''
if ($planning) { Write-Host 'planning only - nothing above was run.'; return }
$machine = Join-Path $Results 'machine.txt'
if (Test-Path -LiteralPath $machine) {
    Select-String -LiteralPath $machine -Pattern 'archive sha256|FileVersion' | ForEach-Object { Write-Host "  $($_.Line.Trim())" }
}
Write-Host "snapshot '$Snapshot' holds the installed candidate; check its sha256 against SHA256SUMS.txt"
