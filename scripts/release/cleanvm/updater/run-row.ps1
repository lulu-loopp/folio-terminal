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
        copy in                       guest\watch.ps1, waits.ps1, keys.ps1, collect.ps1, ui-probe.ps1
                                      into <GuestHome>\updater; the feed into C:\feed
        watch.ps1 -noWait             the row's watcher
        keys.ps1  -noWait             Folio started with --update-feed file:///C:/feed/, the card pressed
      a cut row (W1-W13):
        wait for updater-marker-<Row>.txt, copy it out (marker.txt)
        stop hard                     the virtual power cord, no ACPI, no flush
        start (no revert), wait       then, 45 s after logon: collect after-logon
        keys: a plain start           collect after-start1
        keys: close, a plain start    collect after-start2
      a live row:
        collect at each -CollectAt second; then the row's second plan, collect then
      stop soft
      the row's result (rows.ps1 Get-RowVerdict): FAILED for a cut row whose marker is
        missing or says MISSED, NOT_REACHED or FAILED, and for any row whose watcher log
        in a collection has `ROW <row> FAILED: ...`; after the last row a table of the
        results, and exit 1 when one failed

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

.PARAMETER OverlayZip
    E1-b's RV rows: the release archive of the build under test (A), copied into the
    guest and laid over the install at the row's state (`guest\watch.ps1 -OverlayZip`).

.PARAMETER TemplateDir
    E1-b's planted rows: the folder holding the successor's own journals the rows plant
    from, `rolled-back.json` and `retired.json` (`rows.ps1` `Get-PlantedJournal`).

.PARAMETER RescueB
    E1-b's planted rows: the successor B's `folio.exe`, placed as the rescue copy.

.PARAMETER Mutant
    E1-b: the snapshot holds the build with the hatch removed; a planted row's verdict is
    taken against the mutation column (`ExpectMut`) and, beside it, against the hatch's
    cell (`Expect`), which such a build must fail.

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
    [int] $MarkerTimeoutSeconds = 600,
    [string] $OverlayZip,
    [string] $TemplateDir,
    [string] $RescueB,
    [switch] $Mutant
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
    (Join-Path $guestSide 'waits.ps1'),
    (Join-Path $guestSide 'keys.ps1'),
    (Join-Path $guestSide 'collect.ps1'),
    (Join-Path $guestSide 'plant.ps1'),
    (Join-Path $guestSide 'card.ps1'),
    $uiProbe
)
Assert-GuestReadable -Paths $guestScripts
$guestUpdater = "$GuestHome\updater"
$guestFeed = 'C:\feed'
# The first-run card answered before Folio's first plain start (cleanvm-harness pitfall 4): the
# old-schema settings document load-row.ps1 places, which keys.ps1's `answerfirstrun` copies in.
$firstRun = Join-Path $PSScriptRoot '..\guest\load-row-settings.json'
$guestOverlay = "$guestUpdater\overlay-a.zip"
$planted = @($names | Where-Object { $table[$_].Planted })
if ($planted.Count -gt 0 -and -not $planning) {
    if (-not $TemplateDir -or -not (Test-Path -LiteralPath $TemplateDir -PathType Container)) { throw 'planted rows need -TemplateDir' }
    if (@($planted | Where-Object { $table[$_].Rescue -in 'B', 'held' }).Count -gt 0 -and
        -not ($RescueB -and (Test-Path -LiteralPath $RescueB -PathType Leaf))) { throw 'planted rows with a B rescue need -RescueB' }
}
if (@($names | Where-Object { $table[$_].Overlay }).Count -gt 0 -and -not $planning -and
    -not ($OverlayZip -and (Test-Path -LiteralPath $OverlayZip -PathType Leaf))) { throw 'overlay rows need -OverlayZip' }

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
    # **The guest must not sleep during the row** (cleanvm-harness pitfall 1; run-load-row.ps1
    # says why): standby, monitor and hibernate timeouts to 0 the moment it answers, through
    # `cmd /c` (vmrun hands powercfg its words as one argument otherwise).
    foreach ($setting in @('standby-timeout-ac', 'monitor-timeout-ac', 'hibernate-timeout-ac')) {
        Invoke-Vmrun -Door $door -Step 'no sleep' -Verb 'runProgramInGuest' -InGuest `
            -Arguments @('C:\Windows\System32\cmd.exe', "/c powercfg /change $setting 0") | Out-Null
    }
}

function Push-GuestFiles {
    foreach ($folder in @($GuestHome, $guestUpdater, $guestFeed)) {
        Invoke-Vmrun -Door $door -Step 'mkdir' -Verb 'createDirectoryInGuest' -Arguments @($folder) -InGuest -Tolerant | Out-Null
    }
    foreach ($script in $guestScripts) {
        Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($script, "$guestUpdater\$([IO.Path]::GetFileName($script))") | Out-Null
    }
    Invoke-Vmrun -Door $door -Step 'copy in' -Verb 'copyFileFromHostToGuest' -InGuest `
        -Arguments @((Resolve-Path -LiteralPath $firstRun).Path, "$guestUpdater\first-run-settings.json") | Out-Null
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

# **A planted row's journal and rescue** (E1-b RW): the planted bytes written on the host from the
# template (kept in the evidence as `planted-journal.json`, the bytes the verdict compares), copied
# in with B's executable when the row's rescue is B's, then `guest\plant.ps1`.
function Push-Planted([hashtable] $spec, [string] $evidence) {
    $file = Join-Path $evidence 'planted-journal.json'
    if (-not $planning) {
        $template = Join-Path $TemplateDir "$($spec.Template).json"
        $bytes = Get-PlantedJournal -Template ([IO.File]::ReadAllText($template)) -Word $spec.Word
        [IO.File]::WriteAllText($file, $bytes, [Text.UTF8Encoding]::new($false))
    }
    Invoke-Vmrun -Door $door -Step 'copy plant' -Verb 'copyFileFromHostToGuest' -InGuest `
        -Arguments @($file, "$guestUpdater\planted-journal.json") | Out-Null
    if ($spec.Rescue -in 'B', 'held') {
        Invoke-Vmrun -Door $door -Step 'copy rescue' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @("$RescueB", "$guestUpdater\rescue-b.exe") | Out-Null
    }
    Invoke-GuestScript -Door $door -Step 'plant' -Script "$guestUpdater\plant.ps1" `
        -Arguments @('-Rescue', $spec.Rescue, '-Tag', $spec.Name, '-GuestHome', $GuestHome) | Out-Null
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
    if ($spec.Overlay) {
        Invoke-Vmrun -Door $door -Step 'copy A' -Verb 'copyFileFromHostToGuest' -InGuest `
            -Arguments @($OverlayZip, $guestOverlay) | Out-Null
    }
    if ($spec.Planted) { Push-Planted $spec $evidence }
    $watch = @('-Tag', $name, '-GuestHome', $GuestHome, '-PollMs', '25')
    if ($spec.Cut) { $watch += @('-Row', $name, '-Freeze', '-TimeoutSeconds', "$MarkerTimeoutSeconds") }
    else { $watch += @('-TimeoutSeconds', '1500') }
    if ($spec.Overlay) { $watch += @('-OverlayZip', $guestOverlay) }
    $watch += $spec.Watch
    Invoke-GuestScript -Door $door -Step 'watch' -Script "$guestUpdater\watch.ps1" -NoWait -Arguments $watch | Out-Null
    $started = Get-Date

    if ($spec.Planted) {
        # Two plain starts over the planted journal, each read on the card it shows. A held rescue
        # is waited for (the watcher's `RESCUE HOLD taken`) before the first start.
        Save-Evidence 'planted' $evidence
        $hold = if ($spec.Rescue -eq 'held') { 'waithold,' } else { '' }
        Start-Keys 'start1' "nosleep,answerfirstrun,$($hold)launchplain,sleep:40,card:start1"
        Wait-KeysEnd 'start1' $evidence
        Save-Evidence 'after-start1' $evidence
        Start-Keys 'start2' 'close,sleep:8,launchplain,sleep:40,card:start2'
        Wait-KeysEnd 'start2' $evidence
        Save-Evidence 'after-start2' $evidence
        Invoke-Vmrun -Door $door -Step 'stop' -Verb 'stop' -Arguments @('soft') -Tolerant | Out-Null
        if (-not $planning) {
            $expect = if ($Mutant) { $spec.ExpectMut } else { $spec.Expect }
            $verdict = Get-PlantedVerdict -Expect $expect -Evidence $evidence
            if ($Mutant) {
                $hatch = Get-PlantedVerdict -Expect $spec.Expect -Evidence $evidence
                $verdict.Line = "mutation column: $($verdict.Line) || the hatch's cell: $(if ($hatch.Failed) { 'RED' } else { 'GREEN' }): $($hatch.Line)"
                $verdict.HatchRed = $hatch.Failed
            }
            Save-Verdict $name $verdict $evidence
        }
        Say "row $name done"
        return
    }
    $answer = if ($spec.Overlay) { 'answerfirstrun,' } else { '' }
    Start-Keys $name "nosleep,$($answer)launchfeed,$($spec.Plan)"

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
            $markerCopy = Join-Path $evidence 'marker.txt'
            if ($found) { Read-GuestLog "updater-marker-$name.txt" $markerCopy }
            $markerText = if (Test-Path -LiteralPath $markerCopy) { [IO.File]::ReadAllText($markerCopy) } else { $null }
        }
        # The point of the row: an immediate virtual power-off. Whatever is durable now is what
        # the next start sees.
        Invoke-Vmrun -Door $door -Step 'HARD STOP' -Verb 'stop' -Arguments @('hard') | Out-Null
        Say 'power cut; starting again (no revert)'
        Start-Guest
        if (-not $planning) { Start-Sleep -Seconds 45 }
        Save-Evidence 'after-logon' $evidence
        Start-Keys 'start1' "nosleep,launchplain,sleep:25,shot:start1,card:start1$($spec.Start1Extra)"
        Wait-KeysEnd 'start1' $evidence
        Save-Evidence 'after-start1' $evidence
        Start-Keys 'start2' 'close,sleep:8,launchplain,sleep:25,shot:start2,card:start2'
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
    if (-not $planning) {
        # What the guest saw reaches the person running the rows: the marker of a cut row, and a
        # FAILED line of the watcher's log in any collection.
        $watchLog = @(Get-ChildItem -LiteralPath $evidence -Recurse -File -Filter "updater-watch-$name.txt" |
                ForEach-Object { [IO.File]::ReadAllLines($_.FullName) })
        $verdict = if ($spec.Cut) { Get-RowVerdict -Cut -Marker $markerText -WatchLog $watchLog } else { Get-RowVerdict -WatchLog $watchLog }
        if (-not $verdict.Failed -and $spec.Expect) {
            # E1-b's overlay rows: reaching the state is not the result; the expected cells are.
            $verdict = Get-PlantedVerdict -Expect $spec.Expect -Evidence $evidence
        }
        Save-Verdict $name $verdict $evidence
    }
    Say "row $name done"
}

# A row's verdict: kept for the table at the end, written beside the evidence (`verdict.txt`,
# each expected cell with what it got), and said.
function Save-Verdict([string] $name, [hashtable] $verdict, [string] $evidence) {
    $script:verdicts[$name] = $verdict
    if ($verdict.ContainsKey('Cells')) {
        $text = @("row $name $(if ($verdict.Failed) { 'FAILED' } else { 'ok' })") +
            @($verdict.Cells.Keys | ForEach-Object { "  {0,-4} {1}: want {2}, got {3}" -f $(if ($verdict.Cells[$_].Ok) { 'ok' } else { 'DIFF' }), $_, $verdict.Cells[$_].Want, $verdict.Cells[$_].Got })
        if ($verdict.ContainsKey('HatchRed')) { $text += "  the hatch's cell on this build: $(if ($verdict.HatchRed) { 'RED' } else { 'GREEN' })" }
        Set-Content -LiteralPath (Join-Path $evidence 'verdict.txt') -Value $text -Encoding UTF8
    }
    Say "row $name $(if ($verdict.Failed) { 'FAILED' } else { 'ok' }): $($verdict.Line)"
}

$script:verdicts = [ordered]@{}
foreach ($name in $names) { Invoke-Row $table[$name] }

Write-Host ''
if ($planning) { Write-Host 'planning only - nothing above was run.' }
else {
    Write-Host "evidence in $Results"
    $failed = @($script:verdicts.Keys | Where-Object { $script:verdicts[$_].Failed })
    foreach ($name in $script:verdicts.Keys) {
        Write-Host ("  {0,-12} {1} {2}" -f $name, $(if ($script:verdicts[$name].Failed) { 'FAILED' } else { 'ok    ' }), $script:verdicts[$name].Line)
    }
    if ($failed.Count -gt 0) {
        Write-Host "$($failed.Count) row(s) failed in the guest: $($failed -join ', ')"
        exit 1
    }
}
