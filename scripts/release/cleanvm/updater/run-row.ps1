<#
.SYNOPSIS
    Run updater rows of clean-vm.md 4.4 on the clean Windows guest: bring the
    guest to a row's durable state, cut its power or hold a file, and record what
    the next actors do.

.DESCRIPTION
    The row table is `rows.ps1`; the expected results are clean-vm.md 4.4. For
    each row named in -Row:

        revertToSnapshot <Snapshot>   the candidate A, installed (install-candidate.ps1)
        start nogui, wait             a guest operation, then an interactive program
        copy in                       guest\watch.ps1, keys.ps1, collect.ps1, ui-probe.ps1
                                      into <GuestHome>\updater; the feed into C:\feed
        watch.ps1 -noWait             the row's watcher
        keys.ps1  -noWait             Folio started with --update-feed file:///C:/feed/, the card pressed
      a cut row (W1-W13):
        wait for updater-marker-<Row>.txt
        stop hard                     the virtual power cord, no ACPI, no flush
        start (no revert), wait       then, 45 s after logon: collect after-logon
        keys: a plain start           collect after-start1
        keys: close, a plain start    collect after-start2
      a live row:
        collect at each -CollectAt second; then the row's second plan, collect then
      stop soft

    **The drivers run with `-noWait` in a hidden console** (`vm-door.ps1`
    `Invoke-GuestScript`): the watcher used to be started without it, so the
    host sat through the watcher's own 600 s before it looked for a marker, and
    under `-activeWindow` its window stood over Folio (U-31, H-1/H-2). **The
    watcher opens the journal only when its directory entry changed** (H-6), and
    **Update and Restart are Shift+Tab, Enter** (H-4); `guest\watch.ps1` and
    `guest\keys.ps1` say why.

    Every script that goes into the guest is checked first, on the host, for a
    UTF-8 byte-order mark and a parse by Windows PowerShell 5.1
    (`guest-files.ps1`), as `run-smoke-in-vm.ps1` checks its own.

    Evidence: <Results>\<row>\ — each collection expanded into its own folder
    (install-listing.txt, journal.json, diagnostics.log, run-key.reg,
    processes.txt, the updater-*.txt logs, shots\), a host screen capture beside
    it, and run.log.

    **Do not run this against a guest anyone is using.** Reverting erases what
    the guest was doing, and the hard stop is, on purpose, not clean. Run one
    driver per guest at a time: a second driver on the same guest makes both
    runs void (H-7).

.PARAMETER Vmx
    The guest's `.vmx`.

.PARAMETER Row
    One or more rows of `rows.ps1` (W1..W15, W14long, happy, E7, rollback,
    console1, console2), run in the order given; `all` is every row in the
    table's order.

.PARAMETER Snapshot
    The snapshot with the candidate installed (`install-candidate.ps1`).

.PARAMETER Feed
    The release feed on the host (clean-vm.md 4.4, precondition 3):
    `releases.json` and the files it names as `file:///C:/feed/...`.

.PARAMETER VmPassword
    The encryption password of a guest encrypted to carry a vTPM. A parameter,
    as `run-smoke-in-vm.ps1` takes it; this script reads no password file.

.PARAMETER GuestUser
    The guest account. `folio`, matching the answer files.

.PARAMETER GuestPassword
    That account's password.

.PARAMETER GuestHome
    The working folder in the guest; the candidate is installed in its `folio`.

.PARAMETER Results
    Where the evidence lands. Defaults to target/cleanvm/<vm>-<time>/updater.

.PARAMETER VmrunPath
    `vmrun.exe`, when it is somewhere the door would not look.

.PARAMETER MarkerTimeoutSeconds
    How long a cut row waits for its marker before it cuts anyway.

.EXAMPLE
    ./run-row.ps1 -Vmx <win11.vmx> -VmPassword <password> -Snapshot a-installed -Feed <feed folder> -Row W1 -WhatIf

.EXAMPLE
    ./run-row.ps1 -Vmx <win11.vmx> -VmPassword <password> -Snapshot a-installed -Feed <feed folder> -Row all
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Vmx,
    [Parameter(Mandatory)] [string[]] $Row,
    [Parameter(Mandatory)] [string] $Snapshot,
    [Parameter(Mandatory)] [string] $Feed,
    [string] $VmPassword,
    [string] $GuestUser = 'folio',
    [string] $GuestPassword = 'folio',
    [string] $GuestHome = 'C:\folio-vm',
    [string] $Results,
    [string] $VmrunPath,
    [int] $MarkerTimeoutSeconds = 600
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) { $PSNativeCommandUseErrorActionPreference = $false }

$planning = [bool] $WhatIfPreference
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
. (Join-Path $PSScriptRoot 'vm-door.ps1')
. (Join-Path $PSScriptRoot 'rows.ps1')
. (Join-Path $PSScriptRoot '..\guest-files.ps1')

# ── The rows ─────────────────────────────────────────────────────────────────
$table = Get-UpdaterRows
$names = @()
# `-Row W1,W2` from `pwsh -File` arrives as one string: commas separate rows either way.
foreach ($name in @($Row | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })) {
    if ($name -eq 'all') { $names += @($table.Keys) }
    elseif ($table.Contains($name)) { $names += $name }
    else { throw "no row '$name' in rows.ps1 (rows: $(@($table.Keys) -join ', '))" }
}

# ── What goes into the guest, checked before anything is copied ─────────────
$guestSide = Join-Path $PSScriptRoot 'guest'
$uiProbe = Join-Path $root 'scripts\dev\ui-probe.ps1'
$guestScripts = @(
    (Join-Path $guestSide 'watch.ps1'),
    (Join-Path $guestSide 'keys.ps1'),
    (Join-Path $guestSide 'collect.ps1'),
    $uiProbe
)
Assert-GuestReadable -Paths $guestScripts
$guestUpdater = "$GuestHome\updater"
$guestFeed = 'C:\feed'

if (-not $planning) {
    if (-not (Test-Path -LiteralPath (Join-Path $Feed 'releases.json') -PathType Leaf)) { throw "no releases.json in $Feed" }
}
$feedFiles = @(if (Test-Path -LiteralPath $Feed) { Get-ChildItem -LiteralPath $Feed -File })
if (Test-Path -LiteralPath $Vmx -PathType Leaf) { $Vmx = (Resolve-Path -LiteralPath $Vmx).Path }
elseif (-not $planning) { throw "no virtual machine at $Vmx" }

$vmName = [IO.Path]::GetFileNameWithoutExtension($Vmx)
if (-not $Results) {
    $Results = Join-Path $root ("target\cleanvm\{0}-{1}\updater" -f $vmName, (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
if (-not $PSCmdlet.ShouldProcess($vmName, "revert to '$Snapshot' and run $($names -join ', ')")) { $planning = $true }
$door = New-VmDoor -Vmx $Vmx -VmrunPath $VmrunPath -VmPassword $VmPassword `
    -GuestUser $GuestUser -GuestPassword $GuestPassword -Planning $planning

Write-Host "vmrun    : $($door.Vmrun)"
Write-Host "machine  : $Vmx"
Write-Host "snapshot : $Snapshot"
Write-Host "rows     : $($names -join ', ')"
Write-Host "results  : $Results"
if ($planning) { Write-Host 'MODE     : planning only - nothing is started, copied or run.' }

# ── Steps ────────────────────────────────────────────────────────────────────
$script:runLog = $null
function Say([string] $text) {
    $line = "$(Get-Date -Format 'HH:mm:ss.fff') $text"
    Write-Host $line
    if ($script:runLog) { Add-Content -LiteralPath $script:runLog -Value $line }
}

function Start-Guest([switch] $Revert) {
    if ($Revert) { Invoke-Vmrun -Door $door -Step 'revert' -Verb 'revertToSnapshot' -Arguments @($Snapshot) | Out-Null }
    Invoke-Vmrun -Door $door -Step 'start' -Verb 'start' -Arguments @('nogui') | Out-Null
    Wait-GuestReady -Door $door
}

function Push-GuestFiles {
    foreach ($folder in @($GuestHome, $guestUpdater, $guestFeed)) {
        Invoke-Vmrun -Door $door -Step 'mkdir' -Verb 'createDirectoryInGuest' -Arguments @($folder) -InGuest -Tolerant | Out-Null
    }
    foreach ($script in $guestScripts) {
        Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($script, "$guestUpdater\$([IO.Path]::GetFileName($script))") | Out-Null
    }
    foreach ($file in $feedFiles) {
        Invoke-Vmrun -Door $door -Step 'copy feed' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($file.FullName, "$guestFeed\$($file.Name)") | Out-Null
    }
    if ($feedFiles.Count -eq 0) { Write-Host ("  {0,-12} (every file of $Feed into $guestFeed)" -f 'copy feed') }
}

function Start-Keys([string] $Tag, [string] $Plan) {
    Invoke-GuestScript -Door $door -Step "keys $Tag" -Script "$guestUpdater\keys.ps1" -NoWait `
        -Arguments @('-Tag', $Tag, '-Plan', $Plan, '-GuestHome', $GuestHome) | Out-Null
}

# A log the guest is still appending to is copied inside the guest first, then out.
function Read-GuestLog([string] $Name, [string] $To) {
    if (Test-Path -LiteralPath $To) { Remove-Item -LiteralPath $To -Force }
    Invoke-Vmrun -Door $door -Step 'peek' -Verb 'runProgramInGuest' -InGuest -Tolerant -Quiet -Arguments @(
        'C:\Windows\System32\cmd.exe', "/c del /q $GuestHome\peek.tmp & copy /y $GuestHome\$Name $GuestHome\peek.tmp") | Out-Null
    Invoke-Vmrun -Door $door -Step 'peek' -Verb 'copyFileFromGuestToHost' -InGuest -Tolerant -Quiet `
        -Arguments @("$GuestHome\peek.tmp", $To) | Out-Null
}

function Wait-KeysEnd([string] $Tag, [string] $Evidence, [int] $Seconds = 300) {
    if ($planning) { Write-Host ("  {0,-12} (wait for 'keys end' in updater-keys-$Tag.txt, up to $Seconds s)" -f 'keys?'); return }
    $deadline = (Get-Date).AddSeconds($Seconds)
    $copy = Join-Path $Evidence "keys-$Tag.txt"
    while ((Get-Date) -lt $deadline) {
        Read-GuestLog "updater-keys-$Tag.txt" $copy
        if ((Test-Path -LiteralPath $copy) -and (Select-String -LiteralPath $copy -Pattern 'keys end' -SimpleMatch -Quiet)) { return }
        Start-Sleep -Seconds 5
    }
    Say "keys $Tag did not end within $Seconds s"
}

function Save-Evidence([string] $Tag, [string] $Evidence) {
    Invoke-GuestScript -Door $door -Step "collect $Tag" -Script "$guestUpdater\collect.ps1" -Tolerant `
        -Arguments @('-Tag', $Tag, '-GuestHome', $GuestHome) | Out-Null
    $zip = Join-Path $Evidence "updater-$Tag.zip"
    Invoke-Vmrun -Door $door -Step 'copy out' -Verb 'copyFileFromGuestToHost' -InGuest -Tolerant `
        -Arguments @("$GuestHome\updater-$Tag.zip", $zip) | Out-Null
    Invoke-Vmrun -Door $door -Step 'screen' -Verb 'captureScreen' -InGuest -Tolerant `
        -Arguments @((Join-Path $Evidence "screen-$Tag.png")) | Out-Null
    if ($planning) { return }
    if (Test-Path -LiteralPath $zip) {
        Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $Evidence $Tag) -Force
        Remove-Item -LiteralPath $zip -Force
        Say "collected $Tag"
    }
    else { Say "collect ${Tag}: no zip came back" }
}

function Invoke-Row([hashtable] $spec) {
    $name = $spec.Name
    $evidence = Join-Path $Results $name
    if (-not $planning) {
        [IO.Directory]::CreateDirectory($evidence) | Out-Null
        $script:runLog = Join-Path $evidence 'run.log'
    }
    Write-Host ''
    Say "row $name ($(if ($spec.Cut) { 'power cut' } else { 'live' }))"

    Start-Guest -Revert
    Push-GuestFiles
    $watch = @('-Tag', $name, '-GuestHome', $GuestHome, '-PollMs', '25')
    if ($spec.Cut) { $watch += @('-Row', $name, '-Freeze', '-TimeoutSeconds', "$MarkerTimeoutSeconds") }
    else { $watch += @('-TimeoutSeconds', '1500') }
    $watch += $spec.Watch
    Invoke-GuestScript -Door $door -Step 'watch' -Script "$guestUpdater\watch.ps1" -NoWait -Arguments $watch | Out-Null
    Start-Keys $name "nosleep,launchfeed,$($spec.Plan)"
    $started = Get-Date

    if ($spec.Cut) {
        $marker = "$GuestHome\updater-marker-$name.txt"
        if ($planning) { Write-Host ("  {0,-12} (wait for $marker, up to $MarkerTimeoutSeconds s)" -f 'marker?') }
        else {
            $found = $false
            $deadline = (Get-Date).AddSeconds($MarkerTimeoutSeconds + 60)
            while ((Get-Date) -lt $deadline) {
                Invoke-Vmrun -Door $door -Step 'marker?' -Verb 'fileExistsInGuest' -Arguments @($marker) -InGuest -Tolerant -Quiet | Out-Null
                if ($door.LastExit -eq 0) { $found = $true; break }
                Start-Sleep -Milliseconds 500
            }
            Say "marker found=$found"
        }
        # The point of the row: an immediate virtual power-off. Whatever is durable now is what
        # the next start sees.
        Invoke-Vmrun -Door $door -Step 'HARD STOP' -Verb 'stop' -Arguments @('hard') | Out-Null
        Say 'power cut; starting again (no revert)'
        Start-Guest
        if (-not $planning) { Start-Sleep -Seconds 45 }
        Save-Evidence 'after-logon' $evidence
        Start-Keys 'start1' "nosleep,launchplain,sleep:25,shot:start1$($spec.Start1Extra)"
        Wait-KeysEnd 'start1' $evidence
        Save-Evidence 'after-start1' $evidence
        Start-Keys 'start2' 'close,sleep:8,launchplain,sleep:25,shot:start2'
        Wait-KeysEnd 'start2' $evidence
        Save-Evidence 'after-start2' $evidence
    }
    else {
        foreach ($at in $spec.CollectAt) {
            if (-not $planning) {
                $wait = $at - ((Get-Date) - $started).TotalSeconds
                if ($wait -gt 0) { Start-Sleep -Seconds ([int][Math]::Ceiling($wait)) }
            }
            Save-Evidence "t$at" $evidence
        }
        if ($spec.Then) {
            Start-Keys 'then' $spec.Then
            Wait-KeysEnd 'then' $evidence
            Save-Evidence 'then' $evidence
        }
    }
    Invoke-Vmrun -Door $door -Step 'stop' -Verb 'stop' -Arguments @('soft') -Tolerant | Out-Null
    Say "row $name done"
}

foreach ($name in $names) { Invoke-Row $table[$name] }

Write-Host ''
if ($planning) { Write-Host 'planning only - nothing above was run.' }
else { Write-Host "evidence in $Results" }
