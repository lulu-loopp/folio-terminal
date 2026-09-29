<#
.SYNOPSIS
    The Windows updater rows of clean-vm.md 4.4, as data: how each is reached
    (the key plan), what the guest watcher does, and what is recorded after.
    Dot-sourced by `run-row.ps1`; the expected results are the table in
    clean-vm.md 4.4, which names each row the way this file does.

.DESCRIPTION
    Two kinds of row.

    **A cut row** (`Cut = $true`: W1-W13) runs the key plan until the watcher
    sees the row's state, freezes Folio and writes `updater-marker-<Row>.txt`;
    the host then pulls the virtual power (`vmrun stop hard`), starts the guest
    again without a revert, and records the next actors: 45 s after logon (the
    Run entrance), a plain start (`start1`), then a close and a second plain
    start (`start2`).

    **A live row** (`Cut = $false`: happy, E7, rollback, W14, W14long, W15,
    console1, console2) runs with no power cut and is collected at fixed
    seconds after the drivers start (`CollectAt`), then optionally runs a second
    key plan (`Then`) and is collected once more.

    Every key plan starts with `nosleep,launchfeed` (the guest's display and
    standby timeouts off; the installed Folio started with `--update-feed`),
    and every offer is reached through the two first-run cards (`later`, H-5).
#>

Set-StrictMode -Version Latest

function Get-UpdaterRows {
    $toUpdate = 'offer,later,sleep:4,later,sleep:4,shot:offer-card,update'
    $toRestart = "$toUpdate,waitverified,restart,sleep:150"
    $live = ',sleep:1,shot:downloading,waitverified,restart,sleep:150'
    $plainTwice = 'close,sleep:8,launchplain,sleep:25,shot:plain1,close,sleep:8,launchplain,sleep:25,shot:plain2'

    $rows = [ordered]@{}
    function Add-Cut([string] $Name, [string] $Plan, [string[]] $Watch = @(), [string] $Start1Extra = '') {
        $rows[$Name] = @{ Name = $Name; Cut = $true; Plan = $Plan; Watch = $Watch; Start1Extra = $Start1Extra
            CollectAt = @(); Then = '' }
    }
    function Add-Live([string] $Name, [string] $Plan, [string[]] $Watch, [int[]] $CollectAt, [string] $Then = '') {
        $rows[$Name] = @{ Name = $Name; Cut = $false; Plan = $Plan; Watch = $Watch; Start1Extra = ''
            CollectAt = $CollectAt; Then = $Then }
    }

    Add-Live 'happy' "$toUpdate$live" @('-StopReadingAt', 'Handoff') @(200) $plainTwice
    Add-Cut 'W1' $toUpdate
    Add-Cut 'W2' "$toUpdate,waitverified"
    foreach ($row in 'W3', 'W4', 'W5', 'W6', 'W7', 'W8') { Add-Cut $row $toRestart }
    Add-Cut 'W9' $toRestart @('-KillNewInstallProcess')
    # W10: the trial ended at once, and the new folio.msix held with no sharing so the swap back
    # cannot move it: Stuck.
    Add-Cut 'W10' $toRestart @('-KillNewInstallProcess', '-HoldPath', '.folio-update\*\backup\folio.msix', '-HoldAt', '*')
    Add-Cut 'W11' $toRestart @('-KillNewInstallProcess')
    Add-Cut 'W12' $toRestart
    # W13: Update and Cancel within a download of a few seconds, through the preloaded probe; the
    # first start after the cut presses Update without --update-feed (D-10: it asks github.com).
    Add-Cut 'W13' 'offer,later,sleep:4,later,sleep:4,shot:offer-card,preload,fastupdate,waitalloc,fastcancel,sleep:3,shot:after-cancel' `
        @() ',update,sleep:12,shot:after-press'
    # W14: the journal opened for reading without delete sharing the moment the Run value appears
    # (the applier arms just before it writes Armed): 3 s, which the rename retry must carry, and
    # 120 s, which it cannot (D-14 is open).
    Add-Live 'W14' "$toUpdate$live" @('-StopReadingAt', 'Handoff', '-HoldJournalOnRun', '3') @(200)
    Add-Live 'W14long' "$toUpdate$live" @('-StopReadingAt', 'Handoff', '-HoldJournalOnRun', '120') @(200, 300)
    # W15: the rescue copy held (read, share none) from Prepared for 40 s: its start is refused.
    Add-Live 'W15' "$toUpdate$live" @('-StopReadingAt', 'Handoff', '-HoldRescueAt', 'Prepared', '-HoldSeconds', '40') @(200, 260)
    # E-7: a file of the install held open with no sharing from Prepared for 200 s: nothing moves,
    # the old build reopens; after the release, Restart from the staged set.
    Add-Live 'E7' "$toUpdate$live" @('-StopReadingAt', 'Handoff', '-HoldPath', 'THIRD-PARTY-NOTICES.md', '-HoldAt', 'Prepared', '-HoldSeconds', '200') `
        @(200, 260, 330) 'shot:before-press,update,sleep:15,shot:after-press'
    # The rollback (U-24): the trial ended by the watcher the moment it starts; the journal is never opened.
    Add-Live 'rollback' "$toUpdate$live" @('-NoRead', '-KillNewInstallProcess') @(200, 260)
    # The console Folio was started from goes away at Restart (the key driver ends there).
    foreach ($row in 'console1', 'console2') {
        Add-Live $row "$toUpdate,waitverified,restart" @('-NoRead') @(200)
    }
    return $rows
}
