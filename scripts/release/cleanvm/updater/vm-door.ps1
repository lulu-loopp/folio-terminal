<#
.SYNOPSIS
    The one door to the clean guest for the updater's scripts: every `vmrun`
    call of `run-row.ps1` and `install-candidate.ps1` goes through here.
    Dot-sourced; it runs nothing on its own.

.DESCRIPTION
    The same finding, flags and printing as `run-smoke-in-vm.ps1`: `vmrun.exe`
    is looked for where Workstation and Player put it; the encryption password
    (`-vp`, the Windows 11 guest carries a vTPM) and the guest account travel
    as parameters and are printed as `<hidden>`, in the plan and in anything
    `vmrun` says back. **No password is read from a file here**: the caller
    passes it, as `run-smoke-in-vm.ps1 -VmPassword` has always taken it.

    `New-VmDoor` returns the door; every other function takes it as `-Door`.
    With `Planning` set (a `-WhatIf` run) each call prints its command line and
    returns without running it.
#>

Set-StrictMode -Version Latest

function Resolve-Vmrun {
    param([string] $Given)

    if ($Given) {
        if (-not (Test-Path -LiteralPath $Given -PathType Leaf)) { throw "no vmrun.exe at $Given" }
        return (Resolve-Path -LiteralPath $Given).Path
    }
    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} 'VMware\VMware Workstation\vmrun.exe')
        (Join-Path $env:ProgramFiles 'VMware\VMware Workstation\vmrun.exe')
        (Join-Path ${env:ProgramFiles(x86)} 'VMware\VMware Player\vmrun.exe')
        (Join-Path $env:ProgramFiles 'VMware\VMware Player\vmrun.exe')
    )
    foreach ($key in @(
            'HKLM:\SOFTWARE\WOW6432Node\VMware, Inc.\VMware Workstation'
            'HKLM:\SOFTWARE\VMware, Inc.\VMware Workstation'
            'HKLM:\SOFTWARE\WOW6432Node\VMware, Inc.\VMware Player'
            'HKLM:\SOFTWARE\VMware, Inc.\VMware Player')) {
        $entry = Get-ItemProperty -LiteralPath $key -Name InstallPath -ErrorAction SilentlyContinue
        if ($entry -and $entry.InstallPath) { $candidates += (Join-Path $entry.InstallPath 'vmrun.exe') }
    }
    $onPath = Get-Command vmrun.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += $onPath.Source }
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw "vmrun.exe was not found (it ships with VMware Workstation and Player); pass -VmrunPath"
}

function New-VmDoor {
    param(
        [Parameter(Mandatory)] [string] $Vmx,
        [string] $VmrunPath,
        [string] $VmPassword,
        [string] $GuestUser = 'folio',
        [string] $GuestPassword = 'folio',
        [bool] $Planning = $false
    )
    $vmrun = Resolve-Vmrun -Given $VmrunPath
    return @{
        Vmrun = $vmrun; Vmx = $Vmx; VmPassword = $VmPassword
        GuestUser = $GuestUser; GuestPassword = $GuestPassword; Planning = $Planning
    }
}

# What a line looks like once the two secrets are taken out of it.
function Hide-Secrets {
    param([hashtable] $Door, [string] $Text)
    foreach ($secret in @($Door.VmPassword, $Door.GuestPassword)) {
        if ($secret) { $Text = $Text.Replace($secret, '<hidden>') }
    }
    return $Text
}

<#
    One `vmrun` call: `-T ws`, `-vp` when the machine is encrypted, the guest
    account for a guest operation, then the verb, the `.vmx`, and the rest.
    Returns vmrun's output; `$Door.LastExit` holds its exit code. A failing call
    throws unless `-Tolerant` (a poll that answers "not yet" by failing).
#>
function Invoke-Vmrun {
    param(
        [Parameter(Mandatory)] [hashtable] $Door,
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string] $Verb,
        [string[]] $Arguments = @(),
        [switch] $InGuest,
        [switch] $Tolerant,
        [switch] $Quiet
    )
    $all = @('-T', 'ws')
    if ($Door.VmPassword) { $all += @('-vp', $Door.VmPassword) }
    if ($InGuest) { $all += @('-gu', $Door.GuestUser, '-gp', $Door.GuestPassword) }
    $all += @($Verb, $Door.Vmx) + $Arguments

    $shown = @()
    for ($i = 0; $i -lt $all.Count; $i++) {
        $shown += if ($i -gt 0 -and $all[$i - 1] -in @('-gp', '-vp')) { '<hidden>' }
                  elseif ($all[$i] -match '\s') { '"' + $all[$i] + '"' }
                  else { $all[$i] }
    }
    if (-not $Quiet -or $Door.Planning) { Write-Host ("  {0,-12} vmrun {1}" -f $Step, ($shown -join ' ')) }
    if ($Door.Planning) { $Door.LastExit = 0; return '' }

    $output = Hide-Secrets -Door $Door -Text ((& $Door.Vmrun @all 2>&1 | Out-String).Trim())
    $Door.LastExit = $LASTEXITCODE
    if ($Door.LastExit -ne 0 -and -not $Tolerant) {
        throw "$Step failed: vmrun exited $($Door.LastExit)`n$output"
    }
    if ($output -and -not $Quiet) { Write-Host "               $output" }
    return $output
}

<#
    Wait until the guest answers a guest operation, then an interactive program
    (clean-vm.md §3.4c: Tools state is not the verdict; every step here runs
    `-interactive`, which is refused until the automatic logon has a desktop),
    then give the shell 20 s to finish drawing itself.
#>
function Wait-GuestReady {
    param([Parameter(Mandatory)] [hashtable] $Door, [int] $Seconds = 360)
    if ($Door.Planning) {
        Write-Host ("  {0,-12} (wait for a guest operation, then an interactive program, up to $Seconds s)" -f 'ready?')
        return
    }
    $deadline = (Get-Date).AddSeconds($Seconds)
    foreach ($probe in @(
            @{ Verb = 'fileExistsInGuest'; Arguments = @('C:\Windows\System32\cmd.exe') },
            @{ Verb = 'runProgramInGuest'; Arguments = @('-interactive', 'C:\Windows\System32\cmd.exe', '/c exit 0') })) {
        while ($true) {
            Invoke-Vmrun -Door $Door -Step 'ready?' -Verb $probe.Verb -Arguments $probe.Arguments -InGuest -Tolerant -Quiet | Out-Null
            if ($Door.LastExit -eq 0) { break }
            if ((Get-Date) -ge $deadline) { throw "the guest never answered $($probe.Verb) within $Seconds s" }
            Start-Sleep -Seconds 5
        }
    }
    Start-Sleep -Seconds 20
    Write-Host '               the guest answers an interactive program'
}

<#
    A guest-side script, in the logged-on session, under Windows PowerShell 5.1.
    **`conhost.exe` with a hidden window, never `-activeWindow`**: the guest's
    default terminal is Windows Terminal, whose window stood over Folio and made
    ui-probe refuse to photograph or type (U-31, H-2). **`-NoWait` for the
    drivers** (the watcher and the key driver run for minutes; the host has to
    go on and wait for the marker: H-2). `Arguments` travel as one command line,
    as `vmrun` passes them; none may contain a space.
#>
function Invoke-GuestScript {
    param(
        [Parameter(Mandatory)] [hashtable] $Door,
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string] $Script,
        [string[]] $Arguments = @(),
        [switch] $NoWait,
        [switch] $Tolerant
    )
    foreach ($argument in $Arguments) {
        if ($argument -match '\s') { throw "a guest argument may not contain a space: '$argument'" }
    }
    $run = @('-interactive')
    if ($NoWait) { $run += '-noWait' }
    $run += @('C:\Windows\System32\conhost.exe',
        'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
        '-WindowStyle', 'Hidden', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Script) + $Arguments
    return Invoke-Vmrun -Door $Door -Step $Step -Verb 'runProgramInGuest' -Arguments $run -InGuest -Tolerant:$Tolerant
}
