<#
.SYNOPSIS
    Build one of the two clean-machine virtual machines from nothing but a
    Microsoft installation ISO, install Windows into it unattended, and leave a
    `clean` snapshot behind for `run-smoke-in-vm.ps1` to revert to.

.DESCRIPTION
    Gate 5 used to end with three steps left to a person: build the machine in
    the Workstation wizard, attach two discs and watch Setup, then install
    VMware Tools by hand and take the snapshot. This script is those three
    steps. What is left to a person is downloading the ISO and saying where it
    is.

    The shape is two stages, and `-Stage` picks which of them runs:

        build     write the .vmx, create the disk, build the answer ISO
        install   power on, wait for the guest to say it is done, snapshot

    `all` (the default) runs both — except on the Windows 11 machine, where the
    vTPM has to be added through the Workstation window and `build` therefore
    stops and prints the one thing it cannot do. See **The Windows 11 vTPM**
    below; it is the single reason this script is not one command for both
    machines.

    **Nothing here uses the Workstation GUI.** A `.vmx` is a text file of
    key/value pairs, `vmware-vdiskmanager.exe` writes the disk, and every power
    and snapshot operation is `vmrun` — the same binary, found the same way, as
    `run-smoke-in-vm.ps1` finds it.

    ── How the script knows Windows finished installing ────────────────────────

    It cannot ask. Every `vmrun` command under GUEST OS COMMANDS —
    `fileExistsInGuest` included — is a request to the VMware Tools service
    inside the guest, and during Setup there is no guest, let alone a Tools
    service. `getGuestIPAddress` is in that same list and answers no earlier.

    So the guest is made to install the Tools itself, and then to say so:

      * the VM gets a **third CD-ROM** holding `windows.iso`, the Tools image
        that ships inside the Workstation installation directory;
      * the answer file's `FirstLogonCommands` finds that disc by the one file
        only it carries, `VMwareToolsUpgrader.exe`, and runs
        `setup.exe /S /v"/qn REBOOT=R"` — Broadcom's documented silent install,
        with the reboot suppressed so the command can be followed by two more;
      * the next command writes a `RunOnce` entry that will create
        `C:\folio-vm\oobe-done.txt`, and the last one restarts the machine. The
        sentinel is therefore written **after** the restart that puts the Tools
        drivers in place, never before it;
      * this script polls `fileExistsInGuest` for that sentinel. The poll needs
        the Tools and the sentinel needs the restart, so one answer covers both
        facts: Windows is installed, and the machine can be driven.

    A poll that never succeeds is a poll that says why: the message names the
    Tools install as the thing to look at, because it is the only step between
    a working Setup and a silent guest.

    ── The Windows 11 vTPM ─────────────────────────────────────────────────────

    Windows 11 Setup wants a TPM 2.0, and `docs/plans/release/clean-vm.md` §2.2
    rules out bypassing the check: a machine that lied its way past Setup is not
    the machine users have. Workstation gets a guest a TPM by encrypting the VM,
    and there are two ways to ask for that.

      * `managedvm.autoAddVTPM = "software"` — the widely-passed-around line
        that adds a vTPM without full-disk encryption. **This script does not
        write it by default, and the reason is `vmrun`.** The key appears in no
        Broadcom documentation (checked 2026-08-27, see clean-vm.md §2.2a); the
        VM it produces is marked encrypted with a password its owner never
        chose; and the reports of that combination all say `vmrun -T ws start`
        answers `The operation is not supported`. Gate 5 is `vmrun` from end to
        end, so a machine `vmrun` cannot start is not a machine this gate can
        use. `-VTpm software` writes the key anyway, for whoever wants to find
        out whether 17.6.4 still behaves that way.

      * **Encryption with a password you choose, then a TPM device** — the path
        clean-vm.md §2.2 already describes, and the one `vmrun -vp` is for.
        There is no command line for it: `vmcli` on this host offers Chipset,
        ConfigParams, Disk, Ethernet, Guest, HGFS, MKS, Nvme, Power, Sata,
        Serial, Snapshot, Tools, VM, VMTemplate and VProbes, and no module that
        encrypts anything. So `build` writes everything else and stops with the
        two clicks printed, and `-Stage install -VmPassword <the password>`
        picks the machine up afterwards.

    The Windows 10 machine has no TPM, is never encrypted, and runs both stages
    in one command with nothing to answer.

    ── What the machine is ─────────────────────────────────────────────────────

    Only what clean-vm.md §2.1 asks for, and nothing that would give the guest
    something to notice: no sound card, no USB controllers, no serial, no
    parallel, no floppy. 3D **stays on** — Folio draws with the GPU, and a
    machine with the 3D pipeline switched off would be testing a fallback path
    rather than the product. The Windows 10 machine has no network adapter at
    all, which is the hard version of clean-vm.md §2.4: an adapter that is
    merely disconnected can be reconnected by a stray click, and the whole
    absent-WebView2 half of the gate rests on that never happening. Guest
    operations do not go over the network — they ride the VMCI backdoor — so the
    machine with no adapter is driven exactly like the one with.

.PARAMETER Name
    `win11` or `win10`. Picks the guest OS id, the answer file, the machine
    name, and whether there is a network adapter.

.PARAMETER Iso
    The Microsoft installation ISO. Required by the `build` stage and ignored by
    `install`.

.PARAMETER VmRoot
    Where the machines live. Each gets `<VmRoot>\folio-<name>`.

.PARAMETER Memory
    Guest RAM in megabytes.

.PARAMETER Cpus
    Virtual processors, one core per socket.

.PARAMETER DiskGB
    Virtual disk size. Growable and split into 2 GB files, so the host gives up
    a few hundred megabytes now rather than the whole number.

.PARAMETER Stage
    `build`, `install`, or `all`.

.PARAMETER VTpm
    `auto` (the default) means `encrypted` for `win11` and `none` for `win10`.
    `software` writes `managedvm.autoAddVTPM`; read the description first.
    `none` builds a Windows 11 machine that Setup will refuse — it exists so
    that a person deliberately testing something else does not have to edit this
    script.

.PARAMETER VmPassword
    The VM's encryption password, for a Windows 11 machine that has been
    encrypted. Passed to `vmrun` as `-vp`.

.PARAMETER GuestUser
.PARAMETER GuestPassword
    The account the answer file creates, and the credentials the sentinel poll
    authenticates with. `folio` / `folio`, matching the other scripts here.

.PARAMETER AnswerIso
    The answer-file ISO, when it is somewhere other than
    `target/cleanvm/autounattend-<name>.iso`. Built by `new-answer-iso.ps1` if
    it is not there yet.

.PARAMETER ForceAnswerIso
    Rebuild the answer ISO even though one exists — after editing the `.xml`.

.PARAMETER ToolsIso
    `windows.iso`, when Workstation is somewhere this script would not look.

.PARAMETER HardwareVersion
    `virtualHW.version`. 21 is what Workstation 17.6.4 on this host writes for a
    machine it creates itself; 20 is the floor for all of 17.x if a machine ever
    has to open on an older Workstation.

.PARAMETER InstallTimeoutMinutes
    How long the sentinel poll waits. Ninety minutes is a slow host installing
    Windows 11 with room to spare; a machine that has not finished by then has
    stopped rather than slowed.

.PARAMETER KeepMedia
    Leave the three CD-ROM devices connected at power on. Off by default: the
    `clean` snapshot should not carry the installation ISO, or every revert
    spends ten seconds on "press any key to boot from CD".

.PARAMETER SkipImageCheck
    Do not mount the installation ISO to compare its edition names against the
    one the answer file asks for.

.PARAMETER VmrunPath
.PARAMETER VdiskManagerPath
    The two Workstation binaries, when they are not where this script looks.

.EXAMPLE
    ./new-vm.ps1 -Name win10 -Iso D:\iso\Win10_22H2_English_x64.iso

    Builds and installs the whole Windows 10 machine with no further input.

.EXAMPLE
    ./new-vm.ps1 -Name win11 -Iso D:\iso\Win11_25H2_EnterpriseEval_x64.iso
    ./new-vm.ps1 -Name win11 -Stage install -VmPassword 'the one you chose'

    The Windows 11 machine, with the encryption and the TPM device added in the
    Workstation window between the two commands.

.EXAMPLE
    ./new-vm.ps1 -Name win11 -Iso anything.iso -WhatIf

    Prints every step and the entire .vmx it would write, and creates nothing.
    Runs on a host with no ISO at all, which is the point: the script can be
    wrong about its own arguments long before anybody has an ISO to spend an
    hour on.
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [ValidateSet('win11', 'win10')] [string] $Name,
    [string] $Iso,
    [string] $VmRoot = 'D:\VMs\folio-cleanvm',
    [ValidateRange(1024, 262144)] [int] $Memory = 8192,
    [ValidateRange(1, 64)] [int] $Cpus = 2,
    [ValidateRange(32, 2048)] [int] $DiskGB = 64,
    [ValidateSet('build', 'install', 'all')] [string] $Stage = 'all',
    [ValidateSet('auto', 'none', 'software', 'encrypted')] [string] $VTpm = 'auto',
    [string] $VmPassword,
    [string] $GuestUser = 'folio',
    [string] $GuestPassword = 'folio',
    [string] $AnswerIso,
    [switch] $ForceAnswerIso,
    [string] $ToolsIso,
    [ValidateRange(19, 21)] [int] $HardwareVersion = 21,
    [ValidateRange(5, 600)] [int] $InstallTimeoutMinutes = 90,
    [switch] $KeepMedia,
    [switch] $SkipImageCheck,
    [string] $VmrunPath,
    [string] $VdiskManagerPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# `-WhatIf` means "tell me what you would do". Held once, here, so that every
# step below reads the same answer and the plan a dry run prints is the run a
# real one performs.
$planning = [bool] $WhatIfPreference
# A native program's exit code is read here, deliberately, at every call. Newer
# PowerShell can be configured to turn a non-zero one into a terminating error
# before this script ever sees it, which would take the program's output — the
# only useful half of a failure — with it.
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')).Path

# ── Finding what Workstation installed ───────────────────────────────────────
#
# Same three places `run-smoke-in-vm.ps1` looks, for the same reason: an install
# can be moved, and the registry is the only thing that knows when it has been.

function Get-WorkstationDirectory {
    $directories = @(
        (Join-Path ${env:ProgramFiles(x86)} 'VMware\VMware Workstation')
        (Join-Path $env:ProgramFiles 'VMware\VMware Workstation')
        (Join-Path ${env:ProgramFiles(x86)} 'VMware\VMware Player')
        (Join-Path $env:ProgramFiles 'VMware\VMware Player')
    )
    foreach ($key in @(
            'HKLM:\SOFTWARE\WOW6432Node\VMware, Inc.\VMware Workstation'
            'HKLM:\SOFTWARE\VMware, Inc.\VMware Workstation'
            'HKLM:\SOFTWARE\WOW6432Node\VMware, Inc.\VMware Player'
            'HKLM:\SOFTWARE\VMware, Inc.\VMware Player')) {
        $entry = Get-ItemProperty -LiteralPath $key -Name InstallPath -ErrorAction SilentlyContinue
        if ($entry -and $entry.InstallPath) { $directories += $entry.InstallPath }
    }
    return $directories
}

function Resolve-WorkstationFile {
    param(
        [Parameter(Mandatory)] [string] $FileName,
        [string] $Given,
        [string] $What
    )

    if ($Given) {
        if (-not (Test-Path -LiteralPath $Given -PathType Leaf)) { throw "no $What at $Given" }
        return (Resolve-Path -LiteralPath $Given).Path
    }

    $candidates = @(Get-WorkstationDirectory | ForEach-Object { Join-Path $_ $FileName })
    $onPath = Get-Command $FileName -ErrorAction SilentlyContinue
    if ($onPath) { $candidates += $onPath.Source }

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw @"
$FileName was not found. It ships with VMware Workstation; looked in:
$($candidates | ForEach-Object { "  $_" } | Out-String)
Pass the matching -…Path parameter if it is somewhere else.
"@
}

$vmrun = Resolve-WorkstationFile -FileName 'vmrun.exe' -Given $VmrunPath -What 'vmrun.exe'
$vdiskmanager = Resolve-WorkstationFile -FileName 'vmware-vdiskmanager.exe' `
    -Given $VdiskManagerPath -What 'vmware-vdiskmanager.exe'
# `windows.iso` is the VMware Tools image for every 64-bit Windows guest — the
# Workstation installation directory's own `isoimages_manifest.txt` maps both
# `windows11-64` and `windows9-64` to it.
$tools = Resolve-WorkstationFile -FileName 'windows.iso' -Given $ToolsIso -What 'the VMware Tools ISO'

# `vmrun` with no arguments prints its own version and usage. Asked always,
# including in a dry run, because "which vmrun" is exactly the fact a dry run
# exists to establish.
$banner = (& $vmrun 2>&1 | Select-Object -First 3) -join ' '
Write-Host "vmrun    : $vmrun"
Write-Host "         : $($banner.Trim())"
Write-Host "vdisk    : $vdiskmanager"
Write-Host "tools iso: $tools"

# ── The two machines ─────────────────────────────────────────────────────────
#
# `guestOS` is not a free-text field, and the Windows 10 value is the one that
# catches people: Workstation has no `windows10-64`. Windows 10 kept the id it
# was given when it was going to be Windows 9, and `vmcli VM Create -g` on this
# host rejects `windows10-64` by name while accepting `windows9-64`. The same
# two ids are the ones `isoimages_manifest.txt` maps to `windows.iso`.

$machines = @{
    win11 = @{
        GuestOS    = 'windows11-64'
        AnswerFile = 'autounattend-win11.xml'
        Ethernet   = $true
        Edition    = 'Windows 11 Enterprise Evaluation'
        DefaultTpm = 'encrypted'
    }
    win10 = @{
        GuestOS    = 'windows9-64'
        AnswerFile = 'autounattend-win10.xml'
        Ethernet   = $false
        Edition    = 'Windows 10 Pro'
        DefaultTpm = 'none'
    }
}
$machine = $machines[$Name]
if ($VTpm -eq 'auto') { $VTpm = $machine.DefaultTpm }

$vmName = "folio-$Name"
$vmDirectory = Join-Path $VmRoot $vmName
$vmx = Join-Path $vmDirectory "$vmName.vmx"
$vmdk = Join-Path $vmDirectory "$vmName.vmdk"
if (-not $AnswerIso) { $AnswerIso = Join-Path $root "target\cleanvm\autounattend-$Name.iso" }

$guestHome = 'C:\folio-vm'
$sentinel = "$guestHome\oobe-done.txt"

Write-Host "machine  : $vmName ($($machine.GuestOS)), $Cpus vCPU, $Memory MB, $DiskGB GB"
Write-Host "directory: $vmDirectory"
Write-Host "vTPM     : $VTpm"
Write-Host "stage    : $Stage"
if ($planning) { Write-Host 'MODE     : planning only — nothing is written, started or snapshotted.' }
Write-Host ''

# ── Every vmrun call goes through here ───────────────────────────────────────

function Invoke-Vmrun {
    param(
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string[]] $Arguments,
        [switch] $InGuest,
        # A step whose failure is information rather than an end — the sentinel
        # poll, which answers "not yet" by failing.
        [switch] $Tolerant,
        [switch] $Quiet
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
    if (-not $Quiet) { Write-Host ("  {0,-10} vmrun {1}" -f $Step, ($shown -join ' ')) }
    if ($planning) { return '' }

    $output = (& $vmrun @all 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        if ($Tolerant) { return "!$output" }
        throw @"
$Step failed: vmrun exited $LASTEXITCODE
$output
"@
    }
    if ($output -and -not $Quiet) { Write-Host "             $output" }
    return $output
}

# ── The .vmx ─────────────────────────────────────────────────────────────────

function New-VmxText {
    param([Parameter(Mandatory)] [bool] $MediaConnected)

    $connected = if ($MediaConnected) { 'TRUE' } else { 'FALSE' }
    # 1920 × 1080 × 4 bytes is 8,294,400, so the 16 MB Workstation gives an SVGA
    # adapter by default is already enough; it is written down anyway because
    # `svga.autodetect = "FALSE"` is what stops the guest asking for more, and a
    # fixed resolution with an unstated video memory is a resolution that
    # depends on a default (Broadcom KB 313896, and note the value has to divide
    # by 65536 for a Windows guest).
    $lines = [ordered]@{
        '.encoding'                                  = 'UTF-8'
        'config.version'                             = '8'
        'virtualHW.version'                          = "$HardwareVersion"
        'displayName'                                = $vmName
        'guestOS'                                    = $machine.GuestOS
        'annotation'                                 = "Folio release gate 5 clean machine. Built by scripts/release/cleanvm/new-vm.ps1. Do not install anything on it except VMware Tools."
        'nvram'                                      = "$vmName.nvram"

        # UEFI with Secure Boot, because that is what the Windows 11 machine has
        # to be and there is no reason for the two machines to differ.
        'firmware'                                   = 'efi'
        'uefi.secureBoot.enabled'                    = 'TRUE'
        # UNVERIFIED against Broadcom documentation: `bios.bootOrder` is the key
        # every third-party reference names and it is spelled `bios.` under EFI
        # too. It barely matters at install time — the disk is blank, so the
        # firmware falls through to the CD on its own — and after the install it
        # is flipped to `hdd,cdrom` along with the media being disconnected.
        'bios.bootOrder'                             = if ($MediaConnected) { 'cdrom,hdd' } else { 'hdd,cdrom' }

        'numvcpus'                                   = "$Cpus"
        'cpuid.coresPerSocket'                       = '1'
        'memsize'                                    = "$Memory"

        # NVMe, not SCSI: both Windows 10 and Windows 11 carry an inbox NVMe
        # driver, so Setup sees the disk with nothing loaded from a floppy, and
        # it is what Workstation itself gives a Windows 11 guest.
        'nvme0.present'                              = 'TRUE'
        'nvme0:0.present'                            = 'TRUE'
        'nvme0:0.fileName'                           = [IO.Path]::GetFileName($vmdk)

        # Three CD-ROM devices, in the order Windows Setup reads them. The
        # answer file is found because Setup searches removable read-only media
        # by drive letter for a root `Autounattend.xml`; the Tools disc is found
        # by the answer file itself, by the one file only it carries.
        'sata0.present'                              = 'TRUE'
        'sata0:0.present'                            = 'TRUE'
        'sata0:0.deviceType'                         = 'cdrom-image'
        'sata0:0.fileName'                           = $Iso
        'sata0:0.startConnected'                     = $connected
        'sata0:1.present'                            = 'TRUE'
        'sata0:1.deviceType'                         = 'cdrom-image'
        'sata0:1.fileName'                           = $AnswerIso
        'sata0:1.startConnected'                     = $connected
        'sata0:2.present'                            = 'TRUE'
        'sata0:2.deviceType'                         = 'cdrom-image'
        'sata0:2.fileName'                           = $tools
        'sata0:2.startConnected'                     = $connected

        # e1000e rather than vmxnet3: the Intel adapter has an inbox driver, so
        # the machine has a network before VMware Tools rather than after, and
        # Broadcom's own silent-install note warns that `REBOOT=R` on a vmxnet3
        # guest drops the network mid-install.
        'ethernet0.present'                          = if ($machine.Ethernet) { 'TRUE' } else { 'FALSE' }
        'ethernet0.connectionType'                   = 'nat'
        'ethernet0.virtualDev'                       = 'e1000e'
        'ethernet0.addressType'                      = 'generated'
        'ethernet0.startConnected'                   = if ($machine.Ethernet) { 'TRUE' } else { 'FALSE' }

        # Nothing the guest could notice, and nothing that could carry a file in
        # or out behind the script's back.
        'sound.present'                              = 'FALSE'
        'usb.present'                                = 'FALSE'
        'ehci.present'                               = 'FALSE'
        'usb_xhci.present'                           = 'FALSE'
        'serial0.present'                            = 'FALSE'
        'parallel0.present'                          = 'FALSE'
        'floppy0.present'                            = 'FALSE'

        # A fixed 1920 × 1080 desktop that does not follow the host window, so
        # that two runs a week apart photograph the same size of screen.
        'svga.present'                               = 'TRUE'
        'svga.autodetect'                            = 'FALSE'
        'svga.maxWidth'                              = '1920'
        'svga.maxHeight'                             = '1080'
        'svga.vramSize'                              = '16777216'
        # 3D stays on. Folio draws with the GPU; a machine with this off would
        # be evidence about a path the product does not take.
        'mks.enable3d'                               = 'TRUE'
        'gui.fitGuestUsingNativeDisplayResolution'   = 'FALSE'
        'gui.applyHostDisplayScalingToGuest'         = 'FALSE'
        'tools.guest.desktop.autoresize'             = 'FALSE'

        # The Tools' own settings. `isolation.tools.*` are written at their
        # defaults rather than left out, so that the file says what the machine
        # allows instead of leaving it to be looked up.
        'vmci0.present'                              = 'TRUE'
        'tools.syncTime'                             = 'TRUE'
        'tools.upgrade.policy'                       = 'manual'
        'isolation.tools.hgfs.disable'               = 'FALSE'
        'isolation.tools.copy.disable'               = 'FALSE'
        'isolation.tools.paste.disable'              = 'FALSE'
        'isolation.tools.dnd.disable'                = 'FALSE'

        # What Workstation writes for a machine it creates itself, on this host.
        'pciBridge0.present'                         = 'TRUE'
        'pciBridge4.present'                         = 'TRUE'
        'pciBridge4.virtualDev'                      = 'pcieRootPort'
        'pciBridge4.functions'                       = '8'
        'pciBridge5.present'                         = 'TRUE'
        'pciBridge5.virtualDev'                      = 'pcieRootPort'
        'pciBridge5.functions'                       = '8'
        'pciBridge6.present'                         = 'TRUE'
        'pciBridge6.virtualDev'                      = 'pcieRootPort'
        'pciBridge6.functions'                       = '8'
        'pciBridge7.present'                         = 'TRUE'
        'pciBridge7.virtualDev'                      = 'pcieRootPort'
        'pciBridge7.functions'                       = '8'
        'hpet0.present'                              = 'TRUE'

        # `msg.autoAnswer` is what makes a headless start headless: without it a
        # question about a moved ISO is a modal dialog on a machine nobody is
        # watching. `uuid.action` answers the one question this machine is
        # certain to be asked, the first time it is powered on from a directory
        # it was written into rather than copied into.
        'msg.autoAnswer'                             = 'TRUE'
        'uuid.action'                                = 'create'
        'powerType.powerOff'                         = 'soft'
        'powerType.powerOn'                          = 'soft'
        'powerType.suspend'                          = 'soft'
        'powerType.reset'                            = 'soft'
        'RemoteDisplay.vnc.enabled'                  = 'FALSE'
    }

    if ($VTpm -eq 'software') {
        # UNVERIFIED, and off by default. See the description: no Broadcom page
        # documents this key, and the machine it produces is reported to refuse
        # `vmrun start`.
        $lines['managedvm.autoAddVTPM'] = 'software'
    }

    return (($lines.Keys | ForEach-Object { '{0} = "{1}"' -f $_, $lines[$_] }) -join "`n") + "`n"
}

# ── build ────────────────────────────────────────────────────────────────────

function Invoke-BuildStage {
    # Idempotence is refusal, not overwriting. A second run against a directory
    # that already holds a machine would either destroy a Windows install or —
    # worse — half-destroy one.
    if (Test-Path -LiteralPath $vmDirectory) {
        throw @"
$vmDirectory already exists. This script does not overwrite a machine.
Delete it (or pass a different -VmRoot) to build again, or run
  -Stage install
to pick up the machine that is already there.
"@
    }

    if (-not $Iso) { throw '-Iso is required by the build stage: the Microsoft installation ISO to install from.' }

    Write-Host 'build'
    if (Test-Path -LiteralPath $Iso -PathType Leaf) {
        $script:Iso = (Resolve-Path -LiteralPath $Iso).Path
        Write-Host "  installer  $Iso ($('{0:N0}' -f (Get-Item -LiteralPath $Iso).Length) bytes)"
        Test-InstallationImage
    }
    elseif ($planning) {
        # The ISO is the one thing a dry run is allowed not to have: the whole
        # use of a dry run is checking this script before an hour has been spent
        # downloading one.
        Write-Host "  installer  $Iso (not on this host — planning only)"
    }
    else { throw "no installation ISO at $Iso" }

    New-AnswerIsoIfMissing

    Write-Host "  mkdir      $vmDirectory"
    if (-not $planning) { [IO.Directory]::CreateDirectory($vmDirectory) | Out-Null }

    # `-t 1` is growable and split into 2 GB files: the host gives up a few
    # hundred megabytes now instead of the whole $DiskGB, and copying or
    # deleting the machine later moves files rather than one huge one.
    #
    # `-a nvme` is accepted and the descriptor it writes says `lsilogic` all the
    # same — the adapter recorded in a .vmdk is a hint, and the controller the
    # guest sees is the one named in the .vmx. It is passed because it says what
    # the disk is for.
    $diskArguments = @('-c', '-s', "${DiskGB}GB", '-a', 'nvme', '-t', '1', $vmdk)
    Write-Host "  disk       vmware-vdiskmanager $($diskArguments -join ' ')"
    if (-not $planning) {
        $output = (& $vdiskmanager @diskArguments 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { throw "vmware-vdiskmanager exited $LASTEXITCODE`n$output" }
        Write-Host "             $output"
    }

    $text = New-VmxText -MediaConnected $true
    Write-Host "  vmx        $vmx"
    if ($planning) {
        Write-Host ''
        Write-Host '─── the .vmx this would write ───────────────────────────────────────────────'
        $text.TrimEnd("`n") -split "`n" | ForEach-Object { Write-Host "  $_" }
        Write-Host '─────────────────────────────────────────────────────────────────────────────'
    }
    else {
        # UTF-8 with no byte-order mark, matching the `.encoding` line above. A
        # BOM in front of the first key is the one thing that would make
        # Workstation read this file as something other than a .vmx.
        [IO.File]::WriteAllText($vmx, $text, (New-Object Text.UTF8Encoding($false)))
    }
}

function Test-InstallationImage {
    # A boundary check, and the failure it exists to prevent is the expensive
    # one: an answer file naming an edition the ISO does not hold does not fail
    # loudly. Setup draws its edition page and waits, and this script polls a
    # sentinel for ninety minutes before saying anything.
    if ($SkipImageCheck) { return }

    $wanted = $machine.Edition
    Write-Host "  image      checking the ISO holds `"$wanted`""

    # Two failures, told apart on purpose. *Not being able to look* — mounting an
    # ISO wants an elevated session — is reported and passed over, because
    # refusing to build a machine over an aid would be worse than the aid. *What
    # was seen disagreeing with the answer file* is the end of the run.
    $editions = $null
    $mounted = $null
    try {
        $mounted = Mount-DiskImage -ImagePath $Iso -PassThru -ErrorAction Stop
        $letter = ($mounted | Get-Volume).DriveLetter
        $source = @("${letter}:\sources\install.wim", "${letter}:\sources\install.esd") |
            Where-Object { Test-Path -LiteralPath $_ } |
            Select-Object -First 1
        if (-not $source) { throw 'no sources\install.wim or install.esd at the root of the ISO' }
        $editions = @(Get-WindowsImage -ImagePath $source -ErrorAction Stop | ForEach-Object { $_.ImageName })
    }
    catch {
        Write-Host "             not checked: $($_.Exception.Message.Trim())"
        Write-Host '             run elevated, or pass -SkipImageCheck to stop asking.'
    }
    finally {
        if ($mounted) { Dismount-DiskImage -ImagePath $Iso -ErrorAction SilentlyContinue | Out-Null }
    }

    if ($null -eq $editions) { return }
    if ($editions -notcontains $wanted) {
        throw @"
$([IO.Path]::GetFileName($Iso)) does not hold an edition called "$wanted".
It holds:
$($editions | ForEach-Object { "  $_" } | Out-String)
Change the /IMAGE/NAME value in scripts/release/cleanvm/$($machine.AnswerFile)
to one of those, rebuild the answer ISO with -ForceAnswerIso, and run this again.
"@
    }
    Write-Host "             $($editions.Count) edition(s), including the one the answer file asks for"
}

function New-AnswerIsoIfMissing {
    $exists = Test-Path -LiteralPath $AnswerIso -PathType Leaf
    if ($exists -and -not $ForceAnswerIso) {
        Write-Host "  answer     $AnswerIso (already built)"
        $script:AnswerIso = (Resolve-Path -LiteralPath $AnswerIso).Path
        return
    }

    $builder = Join-Path $PSScriptRoot 'new-answer-iso.ps1'
    if (-not (Test-Path -LiteralPath $builder -PathType Leaf)) { throw "missing $builder" }

    Write-Host "  answer     new-answer-iso.ps1 -AnswerFile $($machine.AnswerFile) -Output $AnswerIso"
    if ($planning) { return }

    & $builder -AnswerFile $machine.AnswerFile -Output $AnswerIso -Password $GuestPassword
    if (-not (Test-Path -LiteralPath $AnswerIso -PathType Leaf)) {
        throw "new-answer-iso.ps1 did not produce $AnswerIso"
    }
    $script:AnswerIso = (Resolve-Path -LiteralPath $AnswerIso).Path
}

# ── install ──────────────────────────────────────────────────────────────────

function Invoke-InstallStage {
    if (-not $planning -and -not (Test-Path -LiteralPath $vmx -PathType Leaf)) {
        throw "no virtual machine at $vmx — run -Stage build first."
    }

    Write-Host ''
    Write-Host 'install'
    # Resumable: the host side of this stage is only a poll, and the process
    # running it can die (a closed terminal, a restart of the tool that launched
    # it) while the guest carries on installing. `start` on a machine that is
    # already powered on is an error, not a no-op, so a machine already in the
    # running list is picked up where it is rather than started twice.
    $alreadyRunning = -not $planning -and
        ((& $vmrun -T ws list 2>&1 | Out-String) -split "`r?`n" | Where-Object { $_.Trim() -ieq $vmx })
    if ($alreadyRunning) {
        Write-Host "  start      already powered on - picking up the install in progress"
    }
    else {
        Invoke-Vmrun -Step 'start' -Arguments @('start', $vmx, 'nogui') | Out-Null
    }

    Write-Host "  wait       polling for $sentinel (VMware Tools answer it, so this covers both)"
    if (-not $planning) { Wait-ForSentinel }

    # Soft, so the machine that gets snapshotted is one that was shut down
    # rather than one whose power was cut mid-write.
    Invoke-Vmrun -Step 'stop' -Arguments @('stop', $vmx, 'soft') | Out-Null
    Wait-ForPowerOff

    if (-not $KeepMedia) { Disconnect-Media }

    # **A powered-off snapshot, and it has to be.** `run-smoke-in-vm.ps1` reverts
    # to `clean` and then calls `start`; a snapshot taken while the machine was
    # running would revert to a running machine, and `start` on one of those is
    # an error rather than a no-op.
    Invoke-Vmrun -Step 'snapshot' -Arguments @('snapshot', $vmx, 'clean') | Out-Null
}

function Wait-ForSentinel {
    $deadline = (Get-Date).AddMinutes($InstallTimeoutMinutes)
    $started = Get-Date
    $polls = 0
    while ($true) {
        $answer = Invoke-Vmrun -Step 'wait' -InGuest -Quiet -Tolerant `
            -Arguments @('fileExistsInGuest', $vmx, $sentinel)
        # `fileExistsInGuest` answers both ways with exit code 0 once the Tools
        # are up, so the poll reads its words. Before that it fails outright,
        # which `-Tolerant` turns into a leading `!` and this comparison into a
        # "no".
        if ($answer -match '^The file exists') { break }

        if ((Get-Date) -ge $deadline) {
            $state = (& $vmrun -T ws checkToolsState $vmx 2>&1 | Out-String).Trim()
            throw @"
$vmName did not finish installing within $InstallTimeoutMinutes minutes.
checkToolsState says '$state'; the last answer about the sentinel was:
$($answer.TrimStart('!'))

Look at the machine's screen — `vmrun -T ws start "$vmx" gui` will show it, and
`vmrun -T ws captureScreen "$vmx" shot.png` will photograph it without one:

  * Setup stopped on a page       the answer file disagrees with the ISO. The
                                  edition page is the usual one; see
                                  -SkipImageCheck above for what is compared.
  * a desktop, but no Tools       the FirstLogonCommands could not find the
                                  Tools disc, or setup.exe failed. Check that
                                  sata0:2 is connected and holds windows.iso.
  * a desktop with Tools running  the RunOnce entry did not fire; the sentinel
                                  is the only thing missing.
"@
        }

        $polls++
        if ($polls % 4 -eq 0) {
            $state = (& $vmrun -T ws checkToolsState $vmx 2>&1 | Out-String).Trim()
            $elapsed = [int]((Get-Date) - $started).TotalMinutes
            Write-Host "             ${elapsed} min — tools '$state', no sentinel yet"
        }
        Start-Sleep -Seconds 15
    }
    Write-Host '             the guest says the unattended install is finished'
}

function Wait-ForPowerOff {
    Write-Host '  off        waiting for the machine to leave the running list'
    if ($planning) { return }
    $deadline = (Get-Date).AddMinutes(10)
    while ($true) {
        $running = (& $vmrun -T ws list 2>&1 | Out-String)
        if ($running -notmatch [regex]::Escape($vmx)) { break }
        if ((Get-Date) -ge $deadline) { throw "$vmName was still running ten minutes after a soft stop" }
        Start-Sleep -Seconds 5
    }
}

function Disconnect-Media {
    # An edit of the file rather than a rewrite of it. By now Workstation has
    # added the machine's UUIDs, its generated MAC address and whatever else it
    # decides a machine needs on first power-on, and rewriting the file from the
    # template would throw all of that away.
    Write-Host '  media      disconnecting the three CD-ROM devices in the .vmx'
    if ($planning) { return }

    $wanted = @{
        'sata0:0.startConnected' = 'FALSE'
        'sata0:1.startConnected' = 'FALSE'
        'sata0:2.startConnected' = 'FALSE'
        'bios.bootOrder'         = 'hdd,cdrom'
    }
    $lines = [IO.File]::ReadAllLines($vmx)
    $seen = @{}
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^\s*([^\s=]+)\s*=') {
            $key = $Matches[1]
            if ($wanted.ContainsKey($key)) {
                $lines[$i] = '{0} = "{1}"' -f $key, $wanted[$key]
                $seen[$key] = $true
            }
        }
    }
    $tail = @($wanted.Keys | Where-Object { -not $seen.ContainsKey($_) } |
        ForEach-Object { '{0} = "{1}"' -f $_, $wanted[$_] })
    [IO.File]::WriteAllLines($vmx, ($lines + $tail), (New-Object Text.UTF8Encoding($false)))
}

# ── The run ──────────────────────────────────────────────────────────────────

$doBuild = $Stage -in @('build', 'all')
$doInstall = $Stage -in @('install', 'all')

# The Windows 11 machine cannot go from `build` straight to `install`: the two
# clicks that give it a TPM are not on any command line. Rather than pretend, it
# stops and hands over the exact command that resumes.
$pauseForVTpm = $doBuild -and $doInstall -and $VTpm -eq 'encrypted' -and -not $VmPassword
if ($pauseForVTpm) { $doInstall = $false }

$what = if ($doBuild -and $doInstall) { 'build it and install Windows into it' }
        elseif ($doBuild) { 'build it' }
        else { 'install Windows into it' }
if (-not $PSCmdlet.ShouldProcess($vmName, $what)) { $planning = $true }

if ($doBuild) { Invoke-BuildStage }
if ($doInstall) { Invoke-InstallStage }

Write-Host ''
if ($planning) { Write-Host 'planning only — nothing above was written, started or snapshotted.'; Write-Host '' }

if ($pauseForVTpm) {
    Write-Host @"
$vmName is built and has never been powered on. It has UEFI and Secure Boot and
no TPM, and Windows 11 Setup refuses a machine in that state.

Three things in the Workstation window, in this order — clean-vm.md §2.2 has the
reasoning, and this is the whole of what is left to a person:

  1. File → Open → $vmx
  2. VM → Settings → Options → Access Control → Encrypt.
     Choose "Encrypt only the files needed to support the TPM", and set a
     password you keep — Workstation cannot tell you a lost one.
  3. VM → Settings → Hardware → Add → Trusted Platform Module → Finish.

Then the machine installs itself:

  pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name $Name -Stage install -VmPassword <that password>
"@
    return
}

if ($planning) { return }

if ($doBuild -and -not $doInstall) {
    Write-Host "$vmName is built at $vmx and has never been powered on."
    Write-Host "Install Windows into it with:"
    Write-Host "  pwsh -File scripts/release/cleanvm/new-vm.ps1 -Name $Name -Stage install"
    return
}

Write-Host "$vmName is installed, and its 'clean' snapshot is taken."
Write-Host ''
Write-Host "  $vmx"
Invoke-Vmrun -Step 'snapshots' -Arguments @('listSnapshots', $vmx) | Out-Null
Write-Host ''
Write-Host 'Next, from the repository root:'
Write-Host '  pwsh -File scripts/release/package.ps1'
$vp = if ($VmPassword) { ' -VmPassword <the encryption password>' } else { '' }
Write-Host "  pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx `"$vmx`"$vp -WhatIf"
Write-Host "  pwsh -File scripts/release/cleanvm/run-smoke-in-vm.ps1 -Vmx `"$vmx`"$vp"
