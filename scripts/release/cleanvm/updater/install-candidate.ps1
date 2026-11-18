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
    The candidate, `folio-<version>-windows-x64.zip`, or
    `folio-<version>-windows-x64-unsigned.zip` for one packed without `-Sign`
    (`FOLIO_UPDATER=on`, the update card enabled).

.PARAMETER Statement
    The release statement that lists the candidate, `RELEASE-STATEMENT.txt`
    (0.4.10 plan row 104). Given, the archive must be its `windows-x64` asset,
    the same name, size and SHA-256, before the guest is touched, and the line
    is printed. Its signature is not read here: `release-statement.ps1
    -Signature` checked it where the key holder signed it. Without it, the
    archive's SHA-256 is printed for the reader to hold to the statement.

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
    [string] $Statement,
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
$zipSha = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
$zipLine = '{0} {1} windows-x64 {2}' -f $zipSha, (Get-Item -LiteralPath $Zip).Length, [IO.Path]::GetFileName($Zip)
$statementLine = $null
if ($Statement) {
    # An asset line is the statement's only line of four words: <sha256> <size> <platform> <name>.
    if (-not (Test-Path -LiteralPath $Statement -PathType Leaf)) { throw "no statement at $Statement" }
    $statementText = [IO.File]::ReadAllLines($Statement)
    if ($statementText.Count -eq 0 -or $statementText[0] -cne 'folio-release-statement 1') { throw "$Statement is not a release statement (v1)" }
    $listed = @($statementText | Where-Object { $words = $_ -split ' '; $words.Count -eq 4 -and $words[2] -ceq 'windows-x64' })
    if ($listed.Count -ne 1) { throw "$Statement has $($listed.Count) windows-x64 lines; a statement lists one" }
    if ($listed[0] -cne $zipLine) {
        throw "the archive is not the one $Statement lists: the statement says '$($listed[0])', the archive is '$zipLine'"
    }
    $statementLine = $listed[0]
}
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
if ($statementLine) { Write-Host "statement: $statementLine (the archive is the statement's windows-x64 asset)" }
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
if ($statementLine) {
    Write-Host "snapshot '$Snapshot' holds the installed candidate, the statement's windows-x64 asset"
}
else {
    Write-Host "snapshot '$Snapshot' holds the installed candidate, sha256 $zipSha; hold it to its release statement's windows-x64 line"
}
