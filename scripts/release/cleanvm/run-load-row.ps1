<#
.SYNOPSIS
    The load row of clean-vm.md 4.6 on the clean Windows guest: a candidate Folio, a person
    typing and scrolling in a pane that prints, and a machine-wide CPU load at Normal priority
    in the middle — the field report of 2026-10-18 (T-WINDOW-THREAD-UNDER-LOAD), on a machine
    where nothing else is running.

.DESCRIPTION
        revertToSnapshot <Snapshot>    the clean guest
        start nogui, wait              a guest operation, then an interactive program
        copy in                        the candidate (folio.exe, conpty.dll, OpenConsole.exe)
                                       into <GuestHome>\candidate; guest\load-row.ps1,
                                       guest\cpu-hog.ps1 and ui-probe.ps1 into <GuestHome>\load
        run                            load-row.ps1 (baseline, hog, after: -Seconds each)
        copy out                       load-<Tag>.txt, -perf.txt, -diagnostics.log, -hog.txt
        stop soft
        read                           read-load-row.ps1 over what came out: the row's cells

    Every script that goes into the guest is checked first, on the host, for a UTF-8
    byte-order mark and a parse by Windows PowerShell 5.1 (`guest-files.ps1`).

    **Do not run this against a guest anyone is using.** Reverting erases what the guest was
    doing. One driver per guest at a time.

.PARAMETER Vmx
    The guest's `.vmx`.

.PARAMETER Candidate
    A folder holding `folio.exe`, `conpty.dll` and `OpenConsole.exe` (a release build's
    `target\release`).

.PARAMETER Tag
    The run's name: `before` for the unmodified candidate, `after` for the fix.

.PARAMETER VmPassword
    The encryption password of a guest encrypted to carry a vTPM (a parameter; no password
    file is read).

.EXAMPLE
    ./run-load-row.ps1 -Vmx <win11.vmx> -VmPassword <password> -Candidate <target\release> -Tag before -WhatIf
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Vmx,
    [Parameter(Mandatory)] [string] $Candidate,
    [Parameter(Mandatory)] [string] $Tag,
    [string] $Snapshot = 'clean',
    [int] $Seconds = 60,
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
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
. (Join-Path $PSScriptRoot 'updater\vm-door.ps1')
. (Join-Path $PSScriptRoot 'guest-files.ps1')

$guestSide = Join-Path $PSScriptRoot 'guest'
$guestScripts = @(
    (Join-Path $guestSide 'load-row.ps1'),
    (Join-Path $guestSide 'cpu-hog.ps1'),
    (Join-Path $root 'scripts\dev\ui-probe.ps1')
)
Assert-GuestReadable -Paths $guestScripts
$binaries = @('folio.exe', 'conpty.dll', 'OpenConsole.exe') | ForEach-Object { Join-Path $Candidate $_ }
if (-not $planning) {
    foreach ($binary in $binaries) {
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "no $binary" }
    }
}
if (Test-Path -LiteralPath $Vmx -PathType Leaf) { $Vmx = (Resolve-Path -LiteralPath $Vmx).Path }
elseif (-not $planning) { throw "no virtual machine at $Vmx" }
$vmName = [IO.Path]::GetFileNameWithoutExtension($Vmx)
if (-not $Results) {
    $Results = Join-Path $root ("target\cleanvm\{0}-{1}\load-{2}" -f $vmName, (Get-Date -Format 'yyyyMMdd-HHmmss'), $Tag)
}
if (-not $PSCmdlet.ShouldProcess($vmName, "revert to '$Snapshot' and run the load row '$Tag'")) { $planning = $true }
if (-not $planning) { [IO.Directory]::CreateDirectory($Results) | Out-Null }
$door = New-VmDoor -Vmx $Vmx -VmrunPath $VmrunPath -VmPassword $VmPassword `
    -GuestUser $GuestUser -GuestPassword $GuestPassword -Planning $planning

Write-Host "machine  : $Vmx"
Write-Host "snapshot : $Snapshot"
Write-Host "results  : $Results"
if ($planning) { Write-Host 'MODE     : planning only - nothing is started, copied or run.' }

$guestLoad = "$GuestHome\load"
$guestCandidate = "$GuestHome\candidate"
Invoke-Vmrun -Door $door -Step 'revert' -Verb 'revertToSnapshot' -Arguments @($Snapshot) | Out-Null
Invoke-Vmrun -Door $door -Step 'start' -Verb 'start' -Arguments @('nogui') | Out-Null
Wait-GuestReady -Door $door
# **A guest reverted to `clean` must not sleep during the row.** Its power plan's idle timers run
# from boot: on 2026-10-21 three rows lost their guest to an ACPI S1 suspend some 13-21 minutes
# after boot (vmware.log `PMAccessPM got ACPI S1 request`), mid-row when the session was slow to
# come. Standby, monitor and hibernate timeouts on AC to 0, every row, before anything else —
# idempotent, and a guest operation that needs no interactive session.
foreach ($setting in @('standby-timeout-ac', 'monitor-timeout-ac', 'hibernate-timeout-ac')) {
    Invoke-Vmrun -Door $door -Step 'no sleep' -Verb 'runProgramInGuest' -InGuest `
        -Arguments @('C:\Windows\System32\powercfg.exe', "/change $setting 0") | Out-Null
}
try {
    # Idempotent and in any guest language: `cmd`'s own `mkdir` makes the intermediate folders,
    # and `if not exist` leaves a folder the snapshot already has alone (createDirectoryInGuest
    # refuses an existing one with a localised error).
    foreach ($folder in @($guestLoad, $guestCandidate)) {
        Invoke-Vmrun -Door $door -Step 'mkdir' -Verb 'runProgramInGuest' -InGuest `
            -Arguments @('C:\Windows\System32\cmd.exe', "/c if not exist $folder mkdir $folder") | Out-Null
    }
    foreach ($script in $guestScripts) {
        Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($script, "$guestLoad\$([IO.Path]::GetFileName($script))") | Out-Null
    }
    foreach ($binary in $binaries) {
        Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($binary, "$guestCandidate\$([IO.Path]::GetFileName($binary))") | Out-Null
    }
    Invoke-GuestScript -Door $door -Step 'load row' -Script "$guestLoad\load-row.ps1" `
        -Arguments @('-Candidate', $guestCandidate, '-Tag', $Tag, '-Seconds', "$Seconds", '-GuestHome', $GuestHome) -Tolerant | Out-Null
    foreach ($name in @("load-$Tag.txt", "load-$Tag-perf.txt", "load-$Tag-diagnostics.log", "load-$Tag-hog.txt")) {
        Invoke-Vmrun -Door $door -Step 'copy out' -Verb 'copyFileFromGuestToHost' -InGuest -Tolerant `
            -Arguments @("$GuestHome\$name", (Join-Path $Results $name)) | Out-Null
    }
} finally {
    Invoke-Vmrun -Door $door -Step 'stop' -Verb 'stop' -Arguments @('soft') -Tolerant | Out-Null
}
if (-not $planning) {
    & (Join-Path $PSScriptRoot 'read-load-row.ps1') -Results $Results -Tag $Tag
}
