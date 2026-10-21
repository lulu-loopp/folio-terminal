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
    function Add-Cut([string] $Name, [string] $Plan, [string[]] $Watch = @(), [string] $Start1Extra = '',
        [switch] $Overlay, [hashtable] $Expect = $null) {
        $rows[$Name] = @{ Name = $Name; Cut = $true; Plan = $Plan; Watch = $Watch; Start1Extra = $Start1Extra
            CollectAt = @(); Then = ''; Planted = $false; Overlay = [bool] $Overlay; Expect = $Expect; ExpectMut = $null }
    }
    function Add-Live([string] $Name, [string] $Plan, [string[]] $Watch, [int[]] $CollectAt, [string] $Then = '') {
        $rows[$Name] = @{ Name = $Name; Cut = $false; Plan = $Plan; Watch = $Watch; Start1Extra = ''
            CollectAt = $CollectAt; Then = $Then; Planted = $false; Overlay = $false; Expect = $null; ExpectMut = $null }
    }
    # **A planted row** (E1-b RW, design note section 4): no update is run. The host writes
    # `H\journal.json` from a journal the successor B wrote (`-Template`: `rolled-back`, B's own
    # `RolledBack` caught at RV-W11's marker, or `retired`, its `Retired{RolledBack}`), with one
    # word replaced by B's test-only word (`Get-PlantedJournal`); `guest\plant.ps1` puts it in
    # place with the rescue copy the row names (B's executable, the installed A's, none, or B's
    # held with no sharing by the watcher), then Folio is started plainly twice. `Expect` is the
    # table's cell for the build under test (A, or B itself for a `BC-` control row); `ExpectMut`
    # the mutation column's, for the build with the hatch removed (`run-row.ps1 -Mutant`).
    function Add-Planted([string] $Name, [string] $Template, [string] $Word, [string] $Rescue,
        [hashtable] $Expect, [hashtable] $ExpectMut = $null) {
        $watch = if ($Rescue -eq 'held') { @('-HoldRescueAt', '*', '-HoldSeconds', '600') } else { @('-NoRead') }
        $rows[$Name] = @{ Name = $Name; Cut = $false; Plan = ''; Watch = $watch; Start1Extra = ''; CollectAt = @()
            Then = ''; Planted = $true; Overlay = $false; Template = $Template; Word = $Word; Rescue = $Rescue
            Expect = $Expect; ExpectMut = $ExpectMut }
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

    # -- E1-b: the journal escape hatch (clean-vm.md 4.4, "E1-b") --
    # RV: B installed (`b-installed`), B -> C through the feed, A's archive laid over the install
    # at the W state (`-OverlayZip`), then the cut. The design note's guess for the destructive
    # states is not asserted; what is asserted is the hatch's promise: A never deletes or rewrites
    # a journal it did not settle (a change between two collections needs a rescue's
    # `BT_UPDATE_RECOVER ... recovery ended` line in that interval, or a header before it whose
    # class A acts on by its frozen promise: terminal, preparing, deferred - A1),
    # and a Folio window stands at the end.
    $guard = @{ JournalGuard = $true; Window = 'any' }
    Add-Cut 'RV-W1' $toUpdate -Overlay -Expect $guard
    Add-Cut 'RV-W2' "$toUpdate,waitverified" -Overlay -Expect $guard
    foreach ($w in 'W3', 'W4', 'W5', 'W6', 'W7', 'W8', 'W12') { Add-Cut "RV-$w" $toRestart -Overlay -Expect $guard }
    foreach ($w in 'W9', 'W11') { Add-Cut "RV-$w" $toRestart @('-KillNewInstallProcess') -Overlay -Expect $guard }
    Add-Cut 'RV-W10' $toRestart @('-KillNewInstallProcess', '-HoldPath', '.folio-update\*\backup\folio.msix', '-HoldAt', '*') -Overlay -Expect $guard

    # RW: A installed. The cells (the design note's RW table, re-read against E1-a1's, E1-a2's and
    # E3's DESIGN entries on main):
    #   settled: A's start hands the journal to B's rescue, which settles it (`recovery ended
    #     RolledBack`, the journal retired) and the start it opens shows *Previous version restored.*
    #   held: no rescue can be started: A continues with its writes held, the journal's bytes
    #     untouched, the card *An update by a newer Folio is not finished.* ... *Changes made in
    #     this session are not kept.* (over a body word too: the card is the newer Folio's whenever
    #     A cannot read the journal whole, `update_startup::unfinished`)
    #   substituted: A placed as the rescue (R2-19): A as the recovery stands aside, its exit guard
    #     opens the rescue copy, never the installed build plainly, and the window says what it
    #     sees - the newer Folio's card. On main the rescue copy shows no card (it reads its own
    #     home, `H\<txn>\rescue\.folio-update`): the row fails there, honestly (T-RESCUE-COPY-CARD).
    $handedOff = 'BT_UPDATE_START an update is being finished by .*\\rescue\\folio\.exe'
    $settled = @{ Journal = 'settled'; Card = 'restored'; Window = 'installed'
        Lines = @($handedOff, 'BT_UPDATE_RECOVER .*recovery ended RolledBack') }
    $heldCell = @{ Journal = 'kept'; Card = 'newer-held'; Window = 'installed'
        Lines = @('could not be started; Folio starts with its writes held') }
    $standsAside = @{ Journal = 'kept'; Card = 'newer'; Window = 'rescue'
        Lines = @($handedOff, 'BT_UPDATE_RECOVER .*recovery ended StoodAside\(') }
    # The mutation column (the hatch removed): over an unknown header A continues plainly with no
    # card and hands nothing over; over an unknown body the header still hands it over (the
    # control), a refused rescue continues plainly, and a substituted rescue is `Left`, no card.
    $plain = @{ Journal = 'kept'; Card = 'none'; Window = 'installed'; Lines = @('BT_UPDATE_START .* is left as it is: ')
        NoLines = @($handedOff) }
    $plainBody = @{ Journal = 'kept'; Card = 'none'; Window = 'installed'
        Lines = @('could not be started; Folio starts without it') }
    $leftBody = @{ Journal = 'kept'; Card = 'none'; Window = 'rescue'
        Lines = @($handedOff, 'BT_UPDATE_RECOVER .*recovery ended Left\(') }
    foreach ($word in 'class', 'outcome', 'v') {
        Add-Planted "RW-$word-B" 'rolled-back' $word 'B' $settled $plain
        Add-Planted "RW-$word-none" 'rolled-back' $word 'none' $heldCell $plain
        Add-Planted "RW-$word-held" 'rolled-back' $word 'held' $heldCell $plain
        Add-Planted "RW-$word-A" 'rolled-back' $word 'A' $standsAside $plain
        # R2-17: B itself installed meets the planted journal and settles it (`b-installed`).
        Add-Planted "BC-$word" 'rolled-back' $word 'B' $settled
    }
    foreach ($word in 'phase', 'layout', 'adapter') {
        Add-Planted "RW-$word-B" 'rolled-back' $word 'B' $settled $settled
        Add-Planted "RW-$word-none" 'rolled-back' $word 'none' $heldCell $plainBody
        Add-Planted "RW-$word-held" 'rolled-back' $word 'held' $heldCell $plainBody
        Add-Planted "RW-$word-A" 'rolled-back' $word 'A' $standsAside $leftBody
        Add-Planted "BC-$word" 'rolled-back' $word 'B' $settled
    }
    # A terminal header is retired on its frozen promise (A1), whatever its body says; a plain
    # start that retires a rollback says nothing (its card is the one a rollback's start was sent with).
    $retired = @{ Journal = 'settled'; Card = 'none'; Window = 'installed'; NoLines = @($handedOff) }
    Add-Planted 'RW-retired-outcome-B' 'retired' 'retired-outcome' 'B' $retired $retired
    Add-Planted 'BC-retired-outcome' 'retired' 'retired-outcome' 'B' $retired
    return $rows
}

# **A row's result as the host reports it**, from what came back from the guest: the watcher's
# marker (a cut row: `row=<row> phase=<phase> at=<time>`, or nothing when none was written) and
# the lines of the watcher's log that came back in a collection. A `ROW <row> FAILED: ...` line,
# a marker of MISSED, NOT_REACHED or FAILED, and a cut row with no marker are failures; the
# answer is @{ Failed = <bool>; Line = <what the person running the rows reads> }.
function Get-RowVerdict {
    param([switch] $Cut, [AllowNull()] [string] $Marker, [string[]] $WatchLog = @())
    $failure = @($WatchLog | Where-Object { $_ -match 'ROW \S+ FAILED: ' } | Select-Object -First 1)
    if ($failure.Count -gt 0) { return @{ Failed = $true; Line = "watcher: $($failure[0].Trim())" } }
    if (-not $Cut) { return @{ Failed = $false; Line = "no failure in the watcher's log" } }
    $m = [regex]::Match([string] $Marker, 'phase=(\w+)')
    if (-not $m.Success) { return @{ Failed = $true; Line = 'no marker came back from the guest' } }
    $phase = $m.Groups[1].Value
    if ($phase -in 'MISSED', 'NOT_REACHED', 'FAILED') { return @{ Failed = $true; Line = "marker: $($Marker.Trim())" } }
    return @{ Failed = $false; Line = "reached $phase" }
}

# **The planted journal** (E1-b RW): the template's bytes - a journal B wrote - with exactly one
# word replaced by B's test-only word of that vocabulary, each a value the shipped grammar
# refuses and B's reads as the word it replaces (the successor branch `rehearsal/successor-e1b`).
# `written_by` stays the template's: B's version. The replacement must match exactly once, so a
# template that does not carry the word fails here rather than planting the wrong journal.
function Get-PlantedJournal {
    param([Parameter(Mandatory)] [string] $Template, [Parameter(Mandatory)] [string] $Word)
    $swaps = @{
        'class'           = @('"class"\s*:\s*"destructive"', '"class":"rehearsal_destructive"')
        'outcome'         = @('"outcome"\s*:\s*"rolled_back"', '"outcome":"rehearsal_rolled_back"')
        'v'               = @('"v"\s*:\s*1(?=\s*[,}])', '"v":2')
        'phase'           = @('"phase"\s*:\s*"RolledBack"', '"phase":"RehearsalRolledBack"')
        'layout'          = @('"layout"\s*:\s*"Members"', '"layout":"RehearsalMembers"')
        'retired-outcome' = @('"outcome"\s*:\s*"RolledBack"', '"outcome":"RehearsalRolledBack"')
    }
    if ($Word -eq 'adapter') {
        # `Ours` is absent on the wire (0.4.6's bytes); the word is the key, added to the body.
        if ($Template -match '"adapter"\s*:') { throw 'the template already names an adapter' }
        $hits = [regex]::Matches($Template, '"body"\s*:\s*\{')
        if ($hits.Count -ne 1) { throw "the template has $($hits.Count) bodies" }
        return $Template.Substring(0, $hits[0].Index + $hits[0].Length) + '"adapter":"RehearsalOurs",' +
            $Template.Substring($hits[0].Index + $hits[0].Length)
    }
    if (-not $swaps.ContainsKey($Word)) { throw "no test-only word for '$Word'" }
    $pattern, $with = $swaps[$Word]
    $hits = [regex]::Matches($Template, $pattern)
    if ($hits.Count -ne 1) { throw "the template has $($hits.Count) matches of $pattern, not one" }
    return $Template.Substring(0, $hits[0].Index) + $with + $Template.Substring($hits[0].Index + $hits[0].Length)
}

# **What a card's OCR text says** (guest\card.ps1): `none` when no Folio window stood, else the
# first of `newer-held` (*An update by a newer Folio is not finished.* with *... not kept.*),
# `newer`, `unreadable`, `incomplete`, `restored` (*Previous version restored.*), `interrupted`,
# or `none`. The OCR splits and misreads words (measured on Segoe UI: *newer FOIi0*, *ke pt*), so
# the text is folded to lower-case letters and digits with no spaces, `0` read as `o`, and the
# match is on runs of words without the product's name.
function Get-CardKind {
    param([string[]] $Lines = @())
    if ($Lines.Count -eq 0 -or $Lines[0] -match '^window none') { return 'no-window' }
    $t = ((($Lines | Select-Object -Skip 1) -join ' ').ToLowerInvariant() -replace '[^a-z0-9]', '') -replace '0', 'o'
    if ($t -match 'anupdatebyanewer') { if ($t -match 'notkept') { return 'newer-held' } else { return 'newer' } }
    if ($t -match 'recordcannotberead') { return 'unreadable' }
    if ($t -match 'updateincomplete') { return 'incomplete' }
    if ($t -match 'versionrestored') { return 'restored' }
    if ($t -match 'interrupted') { return 'interrupted' }
    return 'none'
}

# **A planted or overlay row's result against its expected cell** (E1-b): read from the row's
# evidence folder (`run-row.ps1`: `planted-journal.json`; per collection `after-start1\`,
# `after-start2\` (and `after-logon\` for a cut row) with `journal.json` and `diagnostics.log`;
# the card's OCR text `shots\<tag>-card-<name>.txt` in the collection after it was read).
# Cells, each compared only when the expectation names it:
#   Journal  `kept`: the journal after start2 is the planted bytes; `settled`: none is left
#   Card     the start1 card's kind (Get-CardKind)
#   Window   which folio.exe owns the window start1's card was read from: `installed`,
#            `rescue` (`H\<txn>\rescue\folio.exe`) or `any`
#   Lines / NoLines   patterns that must / must not appear in start2's diagnostics.log
#   JournalGuard      between consecutive collections a journal that changed or went away
#            was settled by a rescue (a `BT_UPDATE_RECOVER ... recovery ended` line among the
#            lines that interval added) or had a header A acts on by its frozen promise before (A1)
# Answers @{ Failed; Cells = [ordered]@{ <cell> = @{ Want; Got; Ok } }; Line }.
function Get-PlantedVerdict {
    param([Parameter(Mandatory)] [hashtable] $Expect, [Parameter(Mandatory)] [string] $Evidence)
    $cells = [ordered]@{}
    function Text([string] $path) { if (Test-Path -LiteralPath $path) { [IO.File]::ReadAllText($path) } else { $null } }
    function Lines([string] $path) { if (Test-Path -LiteralPath $path) { @([IO.File]::ReadAllLines($path)) } else { @() } }
    function Cell([string] $name, $want, $got) { $cells[$name] = @{ Want = $want; Got = $got; Ok = ($want -eq $got) } }
    $start1 = Join-Path $Evidence 'after-start1'
    $start2 = Join-Path $Evidence 'after-start2'
    if ($Expect.ContainsKey('Journal')) {
        $planted = Text (Join-Path $Evidence 'planted-journal.json')
        $after = Text (Join-Path $start2 'journal.json')
        $got = if ($null -eq $after) { 'settled' } elseif ($after -ceq $planted) { 'kept' } else { 'rewritten' }
        Cell 'Journal' $Expect.Journal $got
    }
    $card = @(Get-ChildItem -LiteralPath $start1 -Recurse -File -Filter '*-card-start1.txt' -ErrorAction SilentlyContinue | Select-Object -First 1)
    $cardLines = @(if ($card.Count -gt 0) { [IO.File]::ReadAllLines($card[0].FullName) })
    if ($Expect.ContainsKey('Card')) { Cell 'Card' $Expect.Card (Get-CardKind -Lines $cardLines) }
    if ($Expect.ContainsKey('Window')) {
        $path = if ($cardLines.Count -gt 0) { [regex]::Match($cardLines[0], 'path=(.*?) size=').Groups[1].Value } else { '' }
        $got = if (-not $path) { 'no-window' }
        elseif ($path -match '\\\.folio-update\\[0-9a-f]{32}\\rescue\\folio\.exe$') { 'rescue' }
        else { 'installed' }
        $want = $Expect.Window
        if ($want -eq 'any') {
            $procs = Text (Join-Path $start2 'processes.txt')
            $got = if ($procs -and $procs -match 'folio\.exe') { 'any' } else { 'no-folio' }
        }
        Cell 'Window' $want $got
    }
    $diag = Lines (Join-Path $start2 'diagnostics.log')
    foreach ($pattern in @(if ($Expect.ContainsKey('Lines')) { $Expect.Lines })) {
        $hit = @($diag | Where-Object { $_ -match $pattern }).Count -gt 0
        Cell "Line /$pattern/" 'present' $(if ($hit) { 'present' } else { 'absent' })
    }
    foreach ($pattern in @(if ($Expect.ContainsKey('NoLines')) { $Expect.NoLines })) {
        $hit = @($diag | Where-Object { $_ -match $pattern }).Count -gt 0
        Cell "NoLine /$pattern/" 'absent' $(if ($hit) { 'present' } else { 'absent' })
    }
    if ($Expect.ContainsKey('JournalGuard') -and $Expect.JournalGuard) {
        $order = @(@('after-logon', 'after-start1', 'after-start2') | ForEach-Object { Join-Path $Evidence $_ } |
            Where-Object { Test-Path -LiteralPath $_ })
        $broken = @()
        for ($i = 1; $i -lt $order.Count; $i++) {
            $before = Text (Join-Path $order[$i - 1] 'journal.json')
            $now = Text (Join-Path $order[$i] 'journal.json')
            if ($null -eq $before -or $now -ceq $before) { continue }
            $was = @(Lines (Join-Path $order[$i - 1] 'diagnostics.log'))
            $added = @(Lines (Join-Path $order[$i] 'diagnostics.log') | Select-Object -Skip $was.Count)
            $terminal = $before -match '"class"\s*:\s*"(terminal|preparing|deferred)"'
            $byRescue = @($added | Where-Object { $_ -match 'BT_UPDATE_RECOVER .*recovery ended ' }).Count -gt 0
            if (-not ($terminal -or $byRescue)) {
                $broken += "$(Split-Path -Leaf $order[$i]): the journal $(if ($null -eq $now) { 'went away' } else { 'changed' }) with no rescue's settling line and no terminal header"
            }
        }
        $cells['JournalGuard'] = @{ Want = 'held'; Got = $(if ($broken.Count) { $broken -join '; ' } else { 'held' }); Ok = ($broken.Count -eq 0) }
    }
    $bad = @($cells.Keys | Where-Object { -not $cells[$_].Ok })
    $line = if ($bad.Count -eq 0) { "every cell as expected ($($cells.Keys.Count))" }
    else { ($bad | ForEach-Object { "$_ want $($cells[$_].Want) got $($cells[$_].Got)" }) -join '; ' }
    return @{ Failed = ($bad.Count -gt 0); Cells = $cells; Line = $line }
}
