<#
.SYNOPSIS
    Hard-reset a clean VM at a named updater state row and collect the evidence
    of what the next start does.

.DESCRIPTION
    The updater's recovery contract (self-update-2026-09-16.md, (b).2) defines
    what is durable at each point where the machine can die, and what the next
    actor does. This script brings a VMware guest to one of those points, cuts
    its power (`vmrun stop <vmx> hard`), restarts it, and copies out the
    evidence that says what happened.

    The shape follows `run-smoke-in-vm.ps1`: the same parameters, the same
    `Invoke-Vmrun` wrapper, `-WhatIf` prints every `vmrun` in order. The
    difference is that the stop is `hard` — the virtual power cord is pulled,
    not a shutdown request — and the guest-side half (`in-guest-updater.ps1`)
    watches the journal phase rather than running smoke stages.

    **Do not run this against a guest the owner is using.** Reverting the
    snapshot erases whatever the guest was doing, and the hard stop is, on
    purpose, not clean.

    The guest writes a marker file when it reaches the target row's state.
    This script waits for that file, then hard-stops, restarts, waits for the
    guest to settle, and copies out:
      - journal.json (the updater's transaction journal)
      - diagnostics.log
      - a recursive directory listing of the install folder
      - the HKCU Run key export (Windows) or LaunchAgent plists (macOS)

    Evidence lands in `target/cleanvm/<vm>-<stamp>/updater/<row>/`.

.PARAMETER Vmx
    The virtual machine's `.vmx` file.

.PARAMETER Row
    Which W/M row to test: W1..W13 or M1..M11.

.PARAMETER Snapshot
    The snapshot to revert to before each run.

.PARAMETER GuestUser
    The guest account.

.PARAMETER GuestPassword
    That account's password.

.PARAMETER VmPassword
    The encryption password, for a VM encrypted to carry a vTPM.

.PARAMETER Results
    Where the evidence lands on the host. Defaults to
    `target/cleanvm/<vm name>-<timestamp>/updater/<row>`.

.PARAMETER VmrunPath
    `vmrun.exe`, when it is somewhere this script would not look.

.PARAMETER GuestHome
    The working directory inside the guest.

.PARAMETER Feed
    A release feed folder on the host (clean-vm.md §4.4, precondition 3):
    `releases.json` and the successor's files it names as `file:///C:/feed/…`.
    When given, every file in it is copied into the guest's `C:\feed\` and the
    guest-side watcher starts the installed Folio with
    `--update-feed file:///C:/feed/` before it watches. Without it nothing
    starts Folio in the guest; a person does.

.PARAMETER MarkerTimeoutSeconds
    How long to wait for the guest to reach the target row.

.PARAMETER SettleTimeoutSeconds
    How long to wait for the guest to be ready after a restart.

.EXAMPLE
    ./hard-reset-in-vm.ps1 -Vmx D:\vm\folio-win11\folio-win11.vmx -Row W1 -WhatIf

.EXAMPLE
    ./hard-reset-in-vm.ps1 -Vmx D:\vm\folio-win11\folio-win11.vmx -Row W5 -VmPassword s3cret
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Vmx,
    [Parameter(Mandatory)]
    [ValidatePattern('^[WM]\d{1,2}$')]
    [string] $Row,
    [string] $Snapshot = 'clean',
    [string] $GuestUser = 'folio',
    [string] $GuestPassword = 'folio',
    [string] $VmPassword,
    [string] $Results,
    [string] $VmrunPath,
    [string] $GuestHome = 'C:\folio-vm',
    [string] $Feed,
    [int] $MarkerTimeoutSeconds = 600,
    [int] $SettleTimeoutSeconds = 300
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$planning = [bool] $WhatIfPreference
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')).Path

# ── Finding vmrun ────────────────────────────────────────────────────────────

function Resolve-Vmrun {
    param([string] $Given)

    if ($Given) {
        if (-not (Test-Path -LiteralPath $Given -PathType Leaf)) {
            throw "no vmrun.exe at $Given"
        }
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
$banner = (& $vmrun 2>&1 | Select-Object -First 3) -join ' '
Write-Host "vmrun    : $vmrun"
Write-Host "         : $($banner.Trim())"
Write-Host "row      : $Row"

# ── Every call goes through here ─────────────────────────────────────────────

function Get-VmrunFlags {
    param([switch] $InGuest)
    $flags = @('-T', 'ws')
    if ($VmPassword) { $flags += @('-vp', $VmPassword) }
    if ($InGuest) { $flags += @('-gu', $GuestUser, '-gp', $GuestPassword) }
    return $flags
}

function Invoke-Vmrun {
    param(
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string[]] $Arguments,
        [switch] $InGuest,
        [switch] $Tolerant
    )

    $all = (Get-VmrunFlags -InGuest:$InGuest) + $Arguments

    $shown = @()
    for ($i = 0; $i -lt $all.Count; $i++) {
        $shown += if ($i -gt 0 -and $all[$i - 1] -in @('-gp', '-vp')) { '<hidden>' }
                  elseif ($all[$i] -match '\s') { '"' + $all[$i] + '"' }
                  else { $all[$i] }
    }
    Write-Host ("  {0,-14} vmrun {1}" -f $Step, ($shown -join ' '))
    if ($planning) { return '' }

    $output = (& $vmrun @all 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        if ($Tolerant) { return $output }
        throw @"
$Step failed: vmrun exited $LASTEXITCODE
$output
"@
    }
    if ($output) { Write-Host "               $output" }
    return $output
}

function Invoke-GuestPowerShell {
    param(
        [Parameter(Mandatory)] [string] $Step,
        [Parameter(Mandatory)] [string[]] $PowerShellArguments
    )
    Invoke-Vmrun -Step $Step -InGuest -Arguments (@(
            'runProgramInGuest', $Vmx, '-interactive', '-activeWindow',
            'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
            '-NoProfile', '-ExecutionPolicy', 'Bypass') + $PowerShellArguments)
}

# ── Resolve output directory ─────────────────────────────────────────────────

$vmxLeaf = [IO.Path]::GetFileNameWithoutExtension($Vmx)
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $Results) {
    $Results = Join-Path $root "target\cleanvm\$vmxLeaf-$stamp\updater\$Row"
}

if (-not $planning) {
    [IO.Directory]::CreateDirectory($Results) | Out-Null
}
Write-Host "results  : $Results"
Write-Host ''

# ── The marker file the guest writes when it reaches the target row ──────────

$markerFile = "$GuestHome\updater-marker-$Row.txt"

# ── 1. Revert to clean snapshot ─────────────────────────────────────────────

Invoke-Vmrun -Step 'revert' -Arguments @('revertToSnapshot', $Vmx, $Snapshot) | Out-Null

# ── 2. Start the guest ──────────────────────────────────────────────────────

Invoke-Vmrun -Step 'start' -Arguments @('start', $Vmx, 'nogui') | Out-Null

# ── 3. Wait for the guest to be ready ───────────────────────────────────────

if (-not $planning) {
    Write-Host '  waiting for guest readiness...'
    $deadline = [DateTime]::UtcNow.AddSeconds($SettleTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $probe = Invoke-Vmrun -Step 'ready?' -Tolerant -InGuest `
            -Arguments @('fileExistsInGuest', $Vmx, 'C:\Windows\System32\cmd.exe')
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Seconds 5
    }
    if ($LASTEXITCODE -ne 0) {
        throw "guest did not become ready within $SettleTimeoutSeconds seconds"
    }
    # Wait for interactive logon (vmrun -interactive needs it).
    $logonDeadline = [DateTime]::UtcNow.AddSeconds(120)
    while ([DateTime]::UtcNow -lt $logonDeadline) {
        try {
            Invoke-Vmrun -Step 'logon?' -InGuest -Tolerant `
                -Arguments @('runProgramInGuest', $Vmx, '-interactive',
                    'C:\Windows\System32\cmd.exe', '"/c exit 0"') | Out-Null
            if ($LASTEXITCODE -eq 0) { break }
        } catch { }
        Start-Sleep -Seconds 5
    }
}

# ── 4. Copy in the guest-side script ────────────────────────────────────────

$guestScript = Join-Path $root 'scripts\release\cleanvm\in-guest-updater.ps1'
$guestDest = "$GuestHome\in-guest-updater.ps1"

Invoke-Vmrun -Step 'copy in' -InGuest `
    -Arguments @('copyFileFromHostToGuest', $Vmx, $guestScript, $guestDest) | Out-Null

# ── 4a. Copy in the release feed ────────────────────────────────────────────

# The successor, as a local release feed (clean-vm.md §4.4, precondition 3):
# every file of the host folder into the guest's feed folder, whose URL the
# watcher starts Folio with.
$guestFeed = 'C:\feed'
$guestFeedUrl = 'file:///C:/feed/'
if ($Feed) {
    if (-not (Test-Path -LiteralPath (Join-Path $Feed 'releases.json') -PathType Leaf)) {
        throw "no releases.json in $Feed"
    }
    Invoke-Vmrun -Step 'feed folder' -InGuest -Tolerant `
        -Arguments @('createDirectoryInGuest', $Vmx, $guestFeed) | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath $Feed -File) {
        Invoke-Vmrun -Step 'copy feed' -InGuest -Arguments @(
            'copyFileFromHostToGuest', $Vmx, $file.FullName, "$guestFeed\$($file.Name)"
        ) | Out-Null
    }
}

# ── 5. Start the guest-side watcher ─────────────────────────────────────────

# The watcher runs in the background: it monitors the journal phase and writes
# the marker file when the target row is reached. It exits after writing. Given
# a feed, it first starts the installed Folio on it.
$watcherArguments = @(
    '-File', $guestDest,
    '-Row', $Row,
    '-GuestHome', $GuestHome
)
if ($Feed) { $watcherArguments += @('-FeedUrl', $guestFeedUrl) }
Invoke-GuestPowerShell -Step 'watcher' -PowerShellArguments $watcherArguments | Out-Null

# ── 6. Wait for the marker file ─────────────────────────────────────────────

if (-not $planning) {
    Write-Host "  waiting for marker ($markerFile)..."
    $deadline = [DateTime]::UtcNow.AddSeconds($MarkerTimeoutSeconds)
    $found = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        $probe = Invoke-Vmrun -Step 'marker?' -Tolerant -InGuest `
            -Arguments @('fileExistsInGuest', $Vmx, $markerFile)
        if ($LASTEXITCODE -eq 0) {
            $found = $true
            break
        }
        Start-Sleep -Seconds 5
    }
    if (-not $found) {
        Write-Host "  marker not found within $MarkerTimeoutSeconds seconds; hard-stopping anyway"
    }
} else {
    Write-Host ("  {0,-14} (wait for $markerFile, up to $MarkerTimeoutSeconds s)" -f 'marker?')
}

# ── 7. HARD STOP — the power cut ────────────────────────────────────────────

# This is the point: `vmrun stop <vmx> hard` is an immediate virtual power-off,
# not a guest shutdown. No ACPI signal, no flush, no warning. Whatever the
# journal's durable state says at this instant is what the next start sees.
Invoke-Vmrun -Step 'HARD STOP' -Arguments @('stop', $Vmx, 'hard') | Out-Null

# ── 8. Restart and wait for readiness ───────────────────────────────────────

Invoke-Vmrun -Step 'restart' -Arguments @('start', $Vmx, 'nogui') | Out-Null

if (-not $planning) {
    Write-Host '  waiting for guest readiness after restart...'
    $deadline = [DateTime]::UtcNow.AddSeconds($SettleTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $probe = Invoke-Vmrun -Step 'ready?' -Tolerant -InGuest `
            -Arguments @('fileExistsInGuest', $Vmx, 'C:\Windows\System32\cmd.exe')
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Seconds 5
    }
    if ($LASTEXITCODE -ne 0) {
        throw "guest did not become ready after restart within $SettleTimeoutSeconds seconds"
    }
    $logonDeadline = [DateTime]::UtcNow.AddSeconds(120)
    while ([DateTime]::UtcNow -lt $logonDeadline) {
        try {
            Invoke-Vmrun -Step 'logon?' -InGuest -Tolerant `
                -Arguments @('runProgramInGuest', $Vmx, '-interactive',
                    'C:\Windows\System32\cmd.exe', '"/c exit 0"') | Out-Null
            if ($LASTEXITCODE -eq 0) { break }
        } catch { }
        Start-Sleep -Seconds 5
    }
}

# ── 9. Collect evidence ─────────────────────────────────────────────────────

# A small script that gathers the journal, diagnostics, dir listing and
# registry, then packs them into a zip.
$collectScript = @'
param([string] $GuestHome, [string] $Row)
$ErrorActionPreference = 'Stop'
$out = Join-Path $GuestHome "updater-evidence-$Row"
[IO.Directory]::CreateDirectory($out) | Out-Null
$folio = Join-Path $GuestHome 'folio'

# Journal: the .folio-update directory, if it exists.
$updateHome = Join-Path $folio '.folio-update'
if (Test-Path -LiteralPath $updateHome) {
    Copy-Item -LiteralPath $updateHome -Destination (Join-Path $out 'folio-update') -Recurse -Force
}

# diagnostics.log from the data directory.
$appdata = [Environment]::GetFolderPath('ApplicationData')
$diagLog = Join-Path $appdata 'Folio\diagnostics.log'
if (Test-Path -LiteralPath $diagLog) {
    Copy-Item -LiteralPath $diagLog -Destination $out -Force
}

# Directory listing of the install folder.
$dirListing = Join-Path $out 'install-dir.txt'
cmd.exe "/c dir /s /a `"$folio`"" | Out-File -FilePath $dirListing -Encoding utf8

# Run key export (Windows).
$regFile = Join-Path $out 'run-key.reg'
reg.exe export 'HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' $regFile /y 2>&1 | Out-Null

# Pack it all up.
$zipPath = Join-Path $GuestHome "updater-evidence-$Row.zip"
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }
Compress-Archive -Path "$out\*" -DestinationPath $zipPath -Force
'@

$collectPath = Join-Path ([IO.Path]::GetTempPath()) ('folio-collect-' + [Guid]::NewGuid().ToString('n') + '.ps1')
Set-Content -LiteralPath $collectPath -Value $collectScript -Encoding UTF8 -NoNewline -WhatIf:$false
# Add BOM for Windows PowerShell 5.1 in the guest.
$bytes = [System.IO.File]::ReadAllBytes($collectPath)
$bom = [byte[]]@(0xEF, 0xBB, 0xBF)
[System.IO.File]::WriteAllBytes($collectPath, $bom + $bytes)

$guestCollect = "$GuestHome\collect-evidence.ps1"
Invoke-Vmrun -Step 'copy collect' -InGuest `
    -Arguments @('copyFileFromHostToGuest', $Vmx, $collectPath, $guestCollect) | Out-Null

Invoke-GuestPowerShell -Step 'collect' -PowerShellArguments @(
    '-File', $guestCollect,
    '-GuestHome', $GuestHome,
    '-Row', $Row
) | Out-Null

$guestZip = "$GuestHome\updater-evidence-$Row.zip"
$hostZip = Join-Path $Results "updater-evidence-$Row.zip"
Invoke-Vmrun -Step 'copy out' -InGuest `
    -Arguments @('copyFileFromGuestToHost', $Vmx, $guestZip, $hostZip) | Out-Null

# ── 10. Stop (soft this time) ────────────────────────────────────────────────

Invoke-Vmrun -Step 'stop' -Arguments @('stop', $Vmx, 'soft') -Tolerant | Out-Null

# ── Done ─────────────────────────────────────────────────────────────────────

if ($planning) {
    Write-Host ''
    Write-Host 'planning only — nothing above was run.'
    # Clean up the temp file even in planning mode.
    if (Test-Path -LiteralPath $collectPath) { Remove-Item -LiteralPath $collectPath -Force }
    return
}

if (Test-Path -LiteralPath $collectPath) { Remove-Item -LiteralPath $collectPath -Force }

if (Test-Path -LiteralPath $hostZip) {
    Expand-Archive -LiteralPath $hostZip -DestinationPath $Results -Force
    Write-Host ''
    Write-Host "evidence in $Results"
    Get-ChildItem -LiteralPath $Results -Recurse -File |
        Sort-Object FullName |
        ForEach-Object { '  {0,12:N0}  {1}' -f $_.Length, $_.FullName.Substring($Results.Length + 1) } |
        Write-Host
} else {
    Write-Host ''
    Write-Host "no evidence zip was copied back for $Row"
}
