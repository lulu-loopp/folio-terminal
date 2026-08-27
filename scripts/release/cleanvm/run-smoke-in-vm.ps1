<#
.SYNOPSIS
    Run the release archive's own checks on a clean Windows virtual machine, from
    the `clean` snapshot, and bring the evidence home.

.DESCRIPTION
    Gate 5 of the release plan asks for something no continuous-integration
    runner can answer: what happens on a Windows that has never had a compiler,
    a Visual C++ redistributable, or anything else a build machine leaves lying
    around. This drives that machine.

    The shape is one straight line, and every step is `vmrun`:

        revertToSnapshot clean      the machine is the same machine every time
        start                       and it boots into an automatic logon
        (wait for VMware Tools)     because nothing below works without them
        copy in                     the archive, smoke.ps1, in-guest.ps1
        run  unpack                 extract, and write down what this machine is
        run  smoke                  scripts/release/smoke.ps1, unchanged
        run  web                    a page, and whichever WebView2 card it earns
        run  notice                 the message box a console-less launch raises
        run  explorer               the icon and the version resource
        copy out                    one zip of everything the phases wrote
        stop

    **Every step stops the run when it fails, and prints what `vmrun` said.**
    A gate whose failures are a bare exit code is a gate somebody re-runs by
    hand, which is a gate nobody runs.

    **VMware Tools is not optional.** `copyFileFromHostToGuest`,
    `runProgramInGuest` and `checkToolsState` are all guest operations, and a
    guest operation is a request to the Tools service inside the VM. A machine
    without them is a machine this script can start and nothing else.

    Snapshots, not fresh installs: reverting to `clean` is what makes the gate
    repeatable, and it is why the checklist in
    `docs/plans/release/clean-vm.md` says to take that snapshot before the
    machine has ever seen this product.

.PARAMETER Vmx
    The virtual machine's `.vmx` file.

.PARAMETER Zip
    The release archive to test. Defaults to the single
    `folio-*-windows-x64.zip` under `target/release-package`.

.PARAMETER Snapshot
    The snapshot to revert to. `clean`, and there is no reason for another.

.PARAMETER GuestUser
    The guest account. `folio`, matching the answer files beside this script.

.PARAMETER GuestPassword
    That account's password.

.PARAMETER VmPassword
    The encryption password, for a VM encrypted to carry a vTPM — which is how
    the Windows 11 machine gets its TPM 2.0. Omit for an unencrypted VM.

.PARAMETER Results
    Where the evidence lands on the host. Defaults to
    `target/cleanvm/<vm name>-<timestamp>`.

.PARAMETER VmrunPath
    `vmrun.exe`, when it is somewhere this script would not look.

.PARAMETER NoGui
    Start the VM without opening its window on the host. The guest still has a
    desktop and the screenshots still work; what goes away is the host-side view
    of it.

.PARAMETER KeepRunning
    Leave the machine up at the end, for looking at something by hand. The next
    run reverts the snapshot anyway.

.EXAMPLE
    ./run-smoke-in-vm.ps1 -Vmx D:\vm\folio-win10\folio-win10.vmx

.EXAMPLE
    ./run-smoke-in-vm.ps1 -Vmx anything.vmx -WhatIf

    Prints the vmrun it would run, in order, and checks everything on the host
    that has to be there first. Runs on a machine with no virtual machines at
    all, which is the point: the script can be wrong about its own arguments
    long before anybody has an hour to spend on a Windows install.
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Vmx,
    [string] $Zip,
    [string] $Snapshot = 'clean',
    [string] $GuestUser = 'folio',
    [string] $GuestPassword = 'folio',
    [string] $VmPassword,
    [string] $Results,
    [string] $VmrunPath,
    [switch] $NoGui,
    [switch] $KeepRunning
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# `-WhatIf` means "tell me what you would do". Held once, here, so that every
# step below reads the same answer and the plan a dry run prints is the run a
# real one performs.
$planning = [bool] $WhatIfPreference
# A native program's exit code is read here, deliberately, at every call. Newer
# PowerShell can be configured to turn a non-zero one into a terminating error
# before this script ever sees it, which would take the `vmrun` output — the
# only useful half of a failure — with it.
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')).Path
$guestHome = 'C:\folio-vm'

# ── Finding vmrun ────────────────────────────────────────────────────────────

function Resolve-Vmrun {
    param([string] $Given)

    if ($Given) {
        if (-not (Test-Path -LiteralPath $Given -PathType Leaf)) {
            throw "no vmrun.exe at $Given"
        }
        return (Resolve-Path -LiteralPath $Given).Path
    }

    # Where Workstation puts it, where Player puts it, and — because an install
    # can be moved — what the installer wrote down. Workstation is 32-bit code
    # and lands under Program Files (x86) on a 64-bit Windows; Player is beside
    # it under its own name.
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
        if ($entry -and $entry.InstallPath) {
            $candidates += (Join-Path $entry.InstallPath 'vmrun.exe')
        }
    }
    $onPath = Get-Command vmrun.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += $onPath.Source }

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw @"
vmrun.exe was not found. It ships with VMware Workstation and with VMware Player;
looked in:
$($candidates | ForEach-Object { "  $_" } | Out-String)
Pass -VmrunPath if it is somewhere else.
"@
}

$vmrun = Resolve-Vmrun -Given $VmrunPath
# `vmrun` with no arguments prints its own version and usage. Asked always,
# including in a dry run, because "which vmrun" is exactly the fact a dry run
# exists to establish.
$banner = (& $vmrun 2>&1 | Select-Object -First 3) -join ' '
Write-Host "vmrun    : $vmrun"
Write-Host "         : $($banner.Trim())"

# ── Every call goes through here ─────────────────────────────────────────────

function Invoke-Vmrun {
    param(
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string[]] $Arguments,
        # Guest operations need the account; power and snapshot operations do
        # not, and passing credentials to them is noise in the printed plan.
        [switch] $InGuest,
        # A step whose failure is information rather than an end — the Tools
        # poll, which answers "not yet" by failing.
        [switch] $Tolerant
    )

    $flags = @('-T', 'ws')
    if ($VmPassword) { $flags += @('-vp', $VmPassword) }
    if ($InGuest) { $flags += @('-gu', $GuestUser, '-gp', $GuestPassword) }
    $all = $flags + $Arguments

    # The printed line is what a person would type, with the two secrets held
    # back: a transcript of this run is evidence and goes in a repository.
    $shown = @()
    for ($i = 0; $i -lt $all.Count; $i++) {
        $shown += if ($i -gt 0 -and $all[$i - 1] -in @('-gp', '-vp')) { '<hidden>' }
                  elseif ($all[$i] -match '\s') { '"' + $all[$i] + '"' }
                  else { $all[$i] }
    }
    Write-Host ("  {0,-10} vmrun {1}" -f $Step, ($shown -join ' '))
    if ($planning) { return '' }

    $output = (& $vmrun @all 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        if ($Tolerant) { return $output }
        throw @"
$Step failed: vmrun exited $LASTEXITCODE
$output
"@
    }
    if ($output) { Write-Host "             $output" }
    return $output
}

function Invoke-GuestPowerShell {
    param(
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string[]] $PowerShellArguments
    )
    # `-interactive` puts the program in the logged-on session rather than in a
    # station of its own. Everything here opens a window and photographs it, and
    # a window drawn on a desktop nobody is looking at photographs as black.
    Invoke-Vmrun -Step $Step -InGuest -Arguments (@(
            'runProgramInGuest', $Vmx, '-interactive', '-activeWindow',
            'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
            '-NoProfile', '-ExecutionPolicy', 'Bypass') + $PowerShellArguments)
}

# ── What has to be on the host before any of it means anything ───────────────

if (-not $Zip) {
    $packaged = Join-Path $root 'target\release-package'
    $archives = @(
        if (Test-Path -LiteralPath $packaged) {
            Get-ChildItem -LiteralPath $packaged -Filter 'folio-*-windows-x64.zip' -File
        }
    )
    if ($archives.Count -ne 1) {
        throw @"
-Zip was not given and $packaged does not hold exactly one folio-*-windows-x64.zip
(found $($archives.Count)). Build one with scripts/release/package.ps1, or name it.
"@
    }
    $Zip = $archives[0].FullName
}
if (-not (Test-Path -LiteralPath $Zip -PathType Leaf)) { throw "no archive at $Zip" }
$Zip = (Resolve-Path -LiteralPath $Zip).Path

$smoke = Join-Path $root 'scripts\release\smoke.ps1'
$inGuest = Join-Path $PSScriptRoot 'in-guest.ps1'
foreach ($required in @($smoke, $inGuest)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "missing $required" }
}

# The `.vmx` is the one thing a dry run is allowed not to have: the whole use of
# a dry run is checking this script before there is a virtual machine.
if (Test-Path -LiteralPath $Vmx -PathType Leaf) {
    $Vmx = (Resolve-Path -LiteralPath $Vmx).Path
}
elseif (-not $planning) {
    throw "no virtual machine at $Vmx"
}

$vmName = [IO.Path]::GetFileNameWithoutExtension($Vmx)
if (-not $Results) {
    $Results = Join-Path $root ("target\cleanvm\{0}-{1}" -f $vmName, (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
if (-not $planning) { [IO.Directory]::CreateDirectory($Results) | Out-Null }

Write-Host "machine  : $Vmx"
Write-Host "archive  : $Zip ($((Get-Item -LiteralPath $Zip).Length) bytes)"
Write-Host "results  : $Results"
if ($planning) { Write-Host 'MODE     : planning only — nothing is started, copied or run.' }
Write-Host ''

# ── The run ──────────────────────────────────────────────────────────────────

# The one step that destroys state. Everything after it is a consequence of it,
# so this is where the confirmation belongs rather than on each call.
if (-not $PSCmdlet.ShouldProcess("$vmName", "revert to snapshot '$Snapshot' and run the clean-machine checks")) {
    $planning = $true
}

Invoke-Vmrun -Step 'revert' -Arguments @('revertToSnapshot', $Vmx, $Snapshot) | Out-Null
Invoke-Vmrun -Step 'start'  -Arguments @('start', $Vmx, $(if ($NoGui) { 'nogui' } else { 'gui' })) | Out-Null

# **Wait for the Tools, not for a clock.** A reverted machine reaches a desktop
# in tens of seconds or in three minutes depending on what the host is doing,
# and a fixed sleep is either a waste of two minutes or a failure nobody can
# reproduce.
Write-Host '  tools      waiting for VMware Tools to answer'
if (-not $planning) {
    $deadline = (Get-Date).AddMinutes(6)
    while ($true) {
        $state = (& $vmrun -T ws checkToolsState $Vmx 2>&1 | Out-String).Trim()
        if ($state -eq 'running') { break }
        if ((Get-Date) -ge $deadline) {
            throw "VMware Tools never came up; checkToolsState last said '$state'"
        }
        Start-Sleep -Seconds 5
    }
    # The Tools answer before the shell is finished drawing itself, and a program
    # started into a half-built session is a program that photographs a desktop
    # with no wallpaper on it.
    Start-Sleep -Seconds 20
    Write-Host '             running'
}

Invoke-Vmrun -Step 'mkdir' -InGuest -Arguments @('createDirectoryInGuest', $Vmx, $guestHome) -Tolerant | Out-Null
# `smoke.ps1` resolves the repository root as two directories above itself and
# uses it for its defaults, so it is put two directories deep in the guest as
# well. A script dropped at `C:\folio-vm\smoke.ps1` would try to resolve `C:\..`
# and fail before it ran a thing.
foreach ($directory in @("$guestHome\scripts", "$guestHome\scripts\release")) {
    Invoke-Vmrun -Step 'mkdir' -InGuest -Arguments @('createDirectoryInGuest', $Vmx, $directory) -Tolerant | Out-Null
}

$archiveInGuest = Join-Path $guestHome ([IO.Path]::GetFileName($Zip))
$copies = @(
    @{ From = $Zip;     To = $archiveInGuest }
    @{ From = $smoke;   To = "$guestHome\scripts\release\smoke.ps1" }
    @{ From = $inGuest; To = "$guestHome\in-guest.ps1" }
)
foreach ($copy in $copies) {
    Invoke-Vmrun -Step 'copy in' -InGuest `
        -Arguments @('copyFileFromHostToGuest', $Vmx, $copy.From, $copy.To) | Out-Null
}

# ── The phases ───────────────────────────────────────────────────────────────
#
# One `vmrun` each, so that a failure names a phase. `in-guest.ps1` documents
# what each one is for; this list is only the order, and the order matters once:
# `unpack` produces the `folio.exe` the other four use.

foreach ($phase in @('unpack', 'smoke', 'web', 'notice', 'explorer')) {
    Invoke-GuestPowerShell -Step $phase -PowerShellArguments @(
        '-File', "$guestHome\in-guest.ps1",
        '-Phase', $phase,
        '-GuestHome', $guestHome
    ) | Out-Null
}

# ── The web card, photographed a second time from outside ────────────────────
#
# `in-guest.ps1` takes the picture that matters — the window's own rectangle,
# captured by the guest. This is the host's independent one: `captureScreen`
# reads the machine's framebuffer rather than asking anything inside it, so a
# guest that has stopped answering still yields a picture, and a picture the
# guest produced can be compared against one it did not.
$fromOutside = Join-Path $Results 'guest-screen.png'
Invoke-Vmrun -Step 'screen' -Arguments @('captureScreen', $Vmx, $fromOutside) -Tolerant | Out-Null

# ── Home ─────────────────────────────────────────────────────────────────────

$bundleInGuest = "$guestHome\results.zip"
Invoke-GuestPowerShell -Step 'bundle' -PowerShellArguments @(
    '-Command',
    "Compress-Archive -Path '$guestHome\results\*' -DestinationPath '$bundleInGuest' -Force"
) | Out-Null

$bundle = Join-Path $Results 'results.zip'
Invoke-Vmrun -Step 'copy out' -InGuest `
    -Arguments @('copyFileFromGuestToHost', $Vmx, $bundleInGuest, $bundle) | Out-Null

if (-not $KeepRunning) {
    # Soft: the guest is asked to shut down rather than having its power cut, so
    # the next revert starts from a machine that was not mid-write.
    Invoke-Vmrun -Step 'stop' -Arguments @('stop', $Vmx, 'soft') -Tolerant | Out-Null
}

if ($planning) {
    Write-Host ''
    Write-Host 'planning only — nothing above was run.'
    return
}

Expand-Archive -LiteralPath $bundle -DestinationPath (Join-Path $Results 'results') -Force
Write-Host ''
Write-Host "evidence in $Results"
Get-ChildItem -LiteralPath (Join-Path $Results 'results') -Recurse -File |
    Sort-Object FullName |
    ForEach-Object { '  {0,12:N0}  {1}' -f $_.Length, $_.FullName.Substring($Results.Length + 1) } |
    Write-Host
Write-Host ''
Write-Host 'read machine.txt first: it says what this Windows is and what engine it had.'
