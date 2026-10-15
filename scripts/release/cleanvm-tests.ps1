<#
.SYNOPSIS
    The clean-VM scripts, asserted without a virtual machine: what
    `run-smoke-in-vm.ps1` copies into the guest, and what the updater harness
    (`cleanvm/updater/`) would run, read from their `-WhatIf` plans.

.DESCRIPTION
    Every case runs the real script as a child `pwsh` with `-WhatIf` and a stub
    `vmrun` (a `.cmd` that answers 0), in a scratch copy of the scripts where a
    case needs to change one, and reads the plan it prints. Nothing is started.

    **run_smoke_copies_what_smoke_loads** (T-CLEANVM-SCRIPTS, RC-046 H-10) —
    every file `smoke.ps1` dot-sources, and every file those name beside
    themselves, is copied into the guest beside `smoke.ps1`. The expectation is
    read out of `smoke.ps1` here with a pattern of its own, not with
    `guest-files.ps1`, so a copy list written by hand is caught: a dot-source
    added to `smoke.ps1` names the copy that is missing.
    MUTATION: in `run-smoke-in-vm.ps1`, drop the `foreach ($loaded in
    $smokeLoads)` that adds the derived copies.

    **a_guest_bound_script_without_a_bom_is_refused** — `release-manifest.ps1`
    without its byte-order mark stops the run on the host, by name.
    MUTATION: in `guest-files.ps1` `Assert-GuestReadable`, return at once.

    **the_updater_rows_plan** — `updater/run-row.ps1 -Row all -WhatIf` plans
    every row of `rows.ps1`, including W14 (the journal held 3 s and 120 s) and
    W15 (the rescue copy held), starts every guest driver with `-noWait` in a
    hidden console and never with `-activeWindow` (U-31 H-2), and hides the
    passwords.

    **update_and_restart_are_shift_tab** — the key driver lights Update and
    Restart with Shift+Tab: Tab lights Skip or Later (U-31 H-4).

    **every_guest_script_carries_a_bom** — the updater's guest scripts and
    `ui-probe.ps1` start with a UTF-8 byte-order mark (U-31 H-3).

    **the_kill_waits_on_the_handle_within_its_bound** (H6) — `waits.ps1`
    `Stop-Within` on a sleeping `pwsh` this case starts (no window; ended by
    this case on its own handle at the end): a real kill answers true and the
    process has exited; a kill that has not ended it (a no-op kill, the stand-in
    for a process the kernel has not let go of) answers false at the bound, and
    the process is still running.
    MUTATION: in `Stop-Within`, `return $true` in place of the handle's
    `WaitForExit`.

    **the_w4_wait_fails_at_its_bound** (H6) — `Wait-RunValue` on a Run key
    with no FolioUpdate-* value and a journal entry that does not change
    answers `bound`, with what it last saw, once its bound has passed; a value
    answers `held`; an entry that changes answers `gone`. `Wait-Fact` refuses a
    pause over 250 ms. MUTATION: in `Wait-Fact`, `if ($true)` in place of
    `if ($wfLeft -le 0)` (the bound ends at once: the elapsed check goes red).

    **the_mac_capture_refuses_without_a_window** (H6) — `mac/row.sh
    --capture-dry-run` under bash, with `osascript`, `ps` and `screencapture`
    stubbed: a window list with no window of the name refuses (exit 1, a
    `REFUSED` line), as does an unreadable list; a window found is named and
    not captured; `screencapture` is never run. MUTATION: in
    `capture_folio_window`, drop the `-z "${id:-}"` refusal. Not dry
    (`--capture-window`): a screencapture that fails after a partial write, or
    writes an empty file, leaves no file and says FAILED; an image written is
    kept; screencapture is only asked for the window found (`-l`). MUTATION:
    drop the `rm -f "$out"`.

    **the_host_reports_what_the_guest_saw** (H6) — `rows.ps1`
    `Get-RowVerdict`: a cut row's marker of FAILED, MISSED or NOT_REACHED, a
    cut row with no marker, and a `ROW <row> FAILED:` line in a live row's
    watcher log each fail the row on the host; a reached marker and a log with
    no failure line are ok. MUTATION: drop `'FAILED'` from the failing phases.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') { $PSNativeCommandUseErrorActionPreference = $false }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$pwsh = (Get-Process -Id $PID).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-cleanvm-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$failures = [System.Collections.Generic.List[string]]::new()
function Check([bool] $ok, [string] $what) {
    if ($ok) { Write-Host "ok   $what" } else { Write-Host "FAIL $what"; $failures.Add($what) }
}

$stub = Join-Path $scratch 'vmrun.cmd'
[IO.File]::WriteAllText($stub, "@echo vmrun stub`r`n@exit /b 0`r`n")
$zip = Join-Path $scratch 'folio-9.8.7-windows-x64.zip'
[IO.File]::WriteAllBytes($zip, [byte[]](0x50, 0x4B, 0x05, 0x06) + [byte[]]::new(18))
$bom = [byte[]](0xEF, 0xBB, 0xBF)

# A scratch tree holding the release scripts run-smoke-in-vm.ps1 reads, at their repo paths.
function New-ScratchTree([string] $name) {
    $tree = Join-Path $scratch $name
    foreach ($rel in @('scripts\release\smoke.ps1', 'scripts\release\release-manifest.ps1',
            'scripts\release\archive-members.txt', 'scripts\release\cleanvm\run-smoke-in-vm.ps1',
            'scripts\release\cleanvm\guest-files.ps1', 'scripts\release\cleanvm\in-guest.ps1')) {
        $to = Join-Path $tree $rel
        [IO.Directory]::CreateDirectory((Split-Path -Parent $to)) | Out-Null
        [IO.File]::Copy((Join-Path $root $rel), $to)
    }
    return $tree
}

# The plan's copies: "<guest path>" for every copyFileFromHostToGuest line.
function Invoke-Plan([string] $script, [string[]] $arguments) {
    $said = & $pwsh -NoProfile -File $script @arguments 2>&1 | Out-String
    return @{ Exit = $LASTEXITCODE; Text = $said }
}
function Get-GuestCopies([string] $plan) {
    return @([regex]::Matches($plan, 'copyFileFromHostToGuest \S+ (?:"[^"]+"|\S+) (\S+)') | ForEach-Object { $_.Groups[1].Value })
}

# What smoke.ps1 loads, read with a pattern of this file's own: its dot-sources, and what each of
# those names beside itself.
function Get-ExpectedLoads([string] $smoke) {
    $folder = Split-Path -Parent $smoke
    $names = @([regex]::Matches([IO.File]::ReadAllText($smoke), "(?m)^\s*\.\s+\(Join-Path\s+\`$(?:here|PSScriptRoot)\s+'([^']+)'\)") |
        ForEach-Object { $_.Groups[1].Value })
    $all = @($names)
    foreach ($name in $names) {
        $text = [IO.File]::ReadAllText((Join-Path $folder $name))
        $all += @([regex]::Matches($text, "Join-Path\s+\`$(?:here|PSScriptRoot)\s+'([^']+)'") |
            ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ -ne '..' -and $_ -ne '.' })
    }
    return @($all | Select-Object -Unique)
}

function Test-SmokeCopies([string] $tree, [string] $label) {
    $smoke = Join-Path $tree 'scripts\release\smoke.ps1'
    $plan = Invoke-Plan (Join-Path $tree 'scripts\release\cleanvm\run-smoke-in-vm.ps1') @(
        '-Vmx', 'x.vmx', '-Zip', $zip, '-VmrunPath', $stub, '-WhatIf')
    Check ($plan.Exit -eq 0) "${label}: run-smoke-in-vm.ps1 -WhatIf exits 0 ($($plan.Exit))"
    $copies = Get-GuestCopies $plan.Text
    $expected = Get-ExpectedLoads $smoke
    Check ($expected.Count -gt 0) "${label}: smoke.ps1 dot-sources something ($($expected -join ', '))"
    foreach ($name in $expected) {
        $guest = "C:\folio-vm\scripts\release\$name"
        Check ($copies -contains $guest) "${label}: run-smoke-in-vm.ps1 copies $name, which smoke.ps1 loads, to $guest"
    }
    Check ($copies -contains 'C:\folio-vm\scripts\release\smoke.ps1') "${label}: smoke.ps1 itself is copied two folders deep"
}

try {
    # ── run_smoke_copies_what_smoke_loads ─────────────────────────────────────
    Test-SmokeCopies (New-ScratchTree 'as-is') 'as it is'

    $mutated = New-ScratchTree 'one-more-dot-source'
    $smoke = Join-Path $mutated 'scripts\release\smoke.ps1'
    $text = [IO.File]::ReadAllText($smoke)
    $anchor = ". (Join-Path `$here 'release-manifest.ps1')"
    Check ($text.Contains($anchor)) "smoke.ps1 dot-sources release-manifest.ps1 as '$anchor'"
    [IO.File]::WriteAllText($smoke, $text.Replace($anchor, "$anchor`r`n. (Join-Path `$here 'extra-helper.ps1')"),
        [Text.UTF8Encoding]::new($true))
    [IO.File]::WriteAllBytes((Join-Path $mutated 'scripts\release\extra-helper.ps1'),
        $bom + [Text.Encoding]::UTF8.GetBytes("`$extra = Join-Path `$PSScriptRoot 'extra-data.txt'`r`n"))
    [IO.File]::WriteAllText((Join-Path $mutated 'scripts\release\extra-data.txt'), "data`n")
    Test-SmokeCopies $mutated 'with a dot-source added'

    # ── a_guest_bound_script_without_a_bom_is_refused ─────────────────────────
    $bare = New-ScratchTree 'no-bom'
    $manifest = Join-Path $bare 'scripts\release\release-manifest.ps1'
    $bytes = [IO.File]::ReadAllBytes($manifest)
    Check ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) 'release-manifest.ps1 carries a UTF-8 byte-order mark'
    [IO.File]::WriteAllBytes($manifest, $bytes[3..($bytes.Length - 1)])
    $plan = Invoke-Plan (Join-Path $bare 'scripts\release\cleanvm\run-smoke-in-vm.ps1') @(
        '-Vmx', 'x.vmx', '-Zip', $zip, '-VmrunPath', $stub, '-WhatIf')
    Check ($plan.Exit -ne 0 -and $plan.Text -match 'release-manifest\.ps1 has no UTF-8 byte-order mark') `
        "release-manifest.ps1 without its BOM stops run-smoke-in-vm.ps1 by name (exit $($plan.Exit))"

    # ── the_updater_rows_plan ─────────────────────────────────────────────────
    $feed = Join-Path $scratch 'feed'
    [IO.Directory]::CreateDirectory($feed) | Out-Null
    [IO.File]::WriteAllText((Join-Path $feed 'releases.json'), "[]`n")
    $plan = Invoke-Plan (Join-Path $root 'scripts\release\cleanvm\updater\run-row.ps1') @(
        '-Vmx', 'x.vmx', '-VmPassword', 'vm-s3cret', '-GuestPassword', 'guest-s3cret', '-Snapshot', 'a-installed',
        '-Feed', $feed, '-Row', 'all', '-VmrunPath', $stub, '-WhatIf')
    Check ($plan.Exit -eq 0) "run-row.ps1 -Row all -WhatIf exits 0 ($($plan.Exit))"
    . (Join-Path $root 'scripts\release\cleanvm\updater\rows.ps1')
    $rows = Get-UpdaterRows
    foreach ($name in @('W1', 'W2', 'W3', 'W4', 'W5', 'W6', 'W7', 'W8', 'W9', 'W10', 'W11', 'W12', 'W13', 'W14', 'W14long', 'W15', 'happy', 'E7', 'rollback')) {
        Check ($rows.Contains($name)) "rows.ps1 has the row $name"
        Check ($plan.Text -match "row $name \((power cut|live)\)") "run-row.ps1 plans the row $name"
    }
    Check ($plan.Text -match 'watch\.ps1 -Tag W14 .*-HoldJournalOnRun 3(\s|$)') 'W14 holds the journal (no delete sharing) 3 s from the Run value'
    Check ($plan.Text -match 'watch\.ps1 -Tag W14long .*-HoldJournalOnRun 120(\s|$)') 'W14long holds it 120 s'
    Check ($plan.Text -match 'watch\.ps1 -Tag W15 .*-HoldRescueAt Prepared') 'W15 holds the rescue copy from Prepared'
    $drivers = @([regex]::Matches($plan.Text, '(?m)^.*runProgramInGuest.*\\(watch|keys)\.ps1.*$') | ForEach-Object { $_.Value })
    Check ($drivers.Count -gt 0) "the plan starts guest drivers ($($drivers.Count))"
    Check (@($drivers | Where-Object { $_ -notmatch ' -noWait ' }).Count -eq 0) 'every watcher and key driver is started with -noWait'
    Check (@($drivers | Where-Object { $_ -notmatch 'conhost\.exe .*-WindowStyle Hidden' }).Count -eq 0) 'every guest driver runs in a hidden console'
    Check ($plan.Text -notmatch '-activeWindow') 'no updater step runs -activeWindow'
    Check ($plan.Text -notmatch 's3cret') 'the plan prints neither password'
    Check ($plan.Text -match 'HARD STOP\s+vmrun .* stop x\.vmx hard') 'a cut row pulls the power with stop hard'

    $install = Invoke-Plan (Join-Path $root 'scripts\release\cleanvm\updater\install-candidate.ps1') @(
        '-Vmx', 'x.vmx', '-VmPassword', 'vm-s3cret', '-Zip', $zip, '-Snapshot', 'a-installed', '-VmrunPath', $stub, '-WhatIf')
    Check ($install.Exit -eq 0 -and $install.Text -match 'snapshot x\.vmx a-installed' -and $install.Text -match '-Phase unpack') `
        "install-candidate.ps1 -WhatIf plans the unpack phase and the snapshot (exit $($install.Exit))"

    # ── update_and_restart_are_shift_tab ──────────────────────────────────────
    $keys = [IO.File]::ReadAllText((Join-Path $root 'scripts\release\cleanvm\updater\guest\keys.ps1'))
    foreach ($verb in 'update', 'restart') {
        $line = [regex]::Match($keys, "(?m)^\s*'\^$verb\`$'.*$").Value
        Check ($line -match "'chord', '-ProcId', `"\`$w`", '-Mods', 's', '-Name', 'Tab'" -and $line -match "'-Name', 'Enter'") `
            "keys.ps1 presses $verb as Shift+Tab, Enter"
    }

    # ── every_guest_script_carries_a_bom ──────────────────────────────────────
    foreach ($rel in @('scripts\release\cleanvm\updater\guest\watch.ps1', 'scripts\release\cleanvm\updater\guest\waits.ps1',
            'scripts\release\cleanvm\updater\guest\keys.ps1',
            'scripts\release\cleanvm\updater\guest\collect.ps1', 'scripts\dev\ui-probe.ps1',
            'scripts\release\release-manifest.ps1', 'scripts\release\smoke.ps1', 'scripts\release\cleanvm\in-guest.ps1')) {
        $head = [IO.File]::ReadAllBytes((Join-Path $root $rel))
        Check ($head.Length -ge 3 -and $head[0] -eq 0xEF -and $head[1] -eq 0xBB -and $head[2] -eq 0xBF) "$rel starts with a UTF-8 byte-order mark"
    }

    # ── the_kill_waits_on_the_handle_within_its_bound ─────────────────────────
    . (Join-Path $root 'scripts\release\cleanvm\updater\guest\waits.ps1')
    Check ($KillWithinMs -eq 2000) "the kill's bound is the product's CHILD_EXIT_BUDGET, 2 s ($KillWithinMs ms)"
    # A sleeping pwsh of this case's own, with no window; every one is ended here on its handle.
    $sleepers = [System.Collections.Generic.List[Diagnostics.Process]]::new()
    function Start-Sleeper {
        $info = [Diagnostics.ProcessStartInfo]::new($pwsh)
        foreach ($a in '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 600') { $info.ArgumentList.Add($a) }
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $p = [Diagnostics.Process]::Start($info)
        $sleepers.Add($p)
        return $p
    }
    try {
        $killed = Start-Sleeper
        $ended = Stop-Within -Process $killed -Kill { param($x) $x.Kill() }
        Check ($ended -and $killed.HasExited) "a killed process is waited for on its handle and has exited (pid $($killed.Id))"

        $stuck = Start-Sleeper
        $ended = Stop-Within -Process $stuck -Kill { param($x) } -WithinMs 300
        Check (-not $ended -and -not $stuck.HasExited) "a process still running at the bound answers false (pid $($stuck.Id))"
    }
    finally {
        foreach ($p in $sleepers) {
            if (-not $p.HasExited) { $p.Kill(); [void]$p.WaitForExit(5000) }
            Check $p.HasExited "this case's sleeper pid $($p.Id) is ended"
            $p.Dispose()
        }
    }

    # ── the_w4_wait_fails_at_its_bound ────────────────────────────────────────
    Check ($W4RunWithinMs -eq 90000) "W4's bound is old_within 60 s + 30 s ($W4RunWithinMs ms)"
    $noValue = [pscustomobject]@{}
    $noValue | Add-Member -MemberType ScriptMethod -Name GetValueNames -Value { @('OneDrive', 'SecurityHealth') }
    $w4 = Wait-RunValue -RunKey $noValue -EntrySig { '638000000000000000:812:637999999999999999' } -WithinMs 300
    Check ($w4.Outcome -eq 'bound' -and $w4.ElapsedMs -ge 300) "no Run value: the W4 wait ends at its bound ($($w4.Outcome) after $($w4.ElapsedMs) ms)"
    Check ($w4.Seen -eq 'run=[] journal entry 638000000000000000:812:637999999999999999') "the W4 wait says what it last saw ($($w4.Seen))"
    $withValue = [pscustomobject]@{}
    $withValue | Add-Member -MemberType ScriptMethod -Name GetValueNames -Value { @('OneDrive', 'FolioUpdate-0123abcd') }
    $w4 = Wait-RunValue -RunKey $withValue -EntrySig { 'x' } -WithinMs 300
    Check ($w4.Outcome -eq 'held' -and $w4.Seen -eq 'run=[FolioUpdate-0123abcd]') "a FolioUpdate-* value holds W4 ($($w4.Outcome): $($w4.Seen))"
    $script:entries = 0
    $w4 = Wait-RunValue -RunKey $noValue -EntrySig { $script:entries++; "sig$([Math]::Min($script:entries, 2))" } -WithinMs 30000
    Check ($w4.Outcome -eq 'gone' -and $w4.Seen -eq 'run=[] journal entry sig1 -> sig2') "a journal entry that changes with no Run value ends W4's wait ($($w4.Outcome): $($w4.Seen))"
    $refused = $false
    try { [void](Wait-Fact -Look { @{ Holds = $true } } -WithinMs 10 -PauseMs 251) } catch { $refused = $true }
    Check $refused 'a fact wait refuses a pause over 250 ms'
    Check ($SpinWithinMs -eq 180000) "the millisecond rows' bound is 170 s of product bounds + 10 s ($SpinWithinMs ms)"

    # ── the_mac_capture_refuses_without_a_window ──────────────────────────────
    # Git for Windows' bash, never WSL's bash.exe: `git --exec-path` is <git>/<mingw64>/libexec/git-core
    # whichever git.exe answers, and the bash is <git>/bin/bash.exe. Elsewhere the system's.
    $bash = if ($IsWindows) { Join-Path ((& git --exec-path) | Split-Path -Parent | Split-Path -Parent | Split-Path -Parent) 'bin\bash.exe' } else { '/bin/bash' }
    Check (Test-Path -LiteralPath $bash) "a bash to run row.sh with ($bash)"
    function ConvertTo-Posix([string] $path) {
        if (-not $IsWindows) { return $path }
        return '/' + $path.Substring(0, 1).ToLowerInvariant() + $path.Substring(2).Replace('\', '/')
    }
    $stubs = Join-Path $scratch 'mac-stubs'
    [IO.Directory]::CreateDirectory($stubs) | Out-Null
    $called = Join-Path $scratch 'screencapture-called.txt'
    [IO.File]::WriteAllText((Join-Path $stubs 'osascript'), "#!/bin/sh`nprintf '%s\n' `"`$STUB_WINDOWS`"`n")
    [IO.File]::WriteAllText((Join-Path $stubs 'ps'), "#!/bin/sh`nprintf '%s\n' 101 202`n")
    [IO.File]::WriteAllText((Join-Path $stubs 'screencapture'), "#!/bin/sh`necho `"`$*`" >> '$(ConvertTo-Posix $called)'`n")
    $rowSh = Join-Path $root 'scripts\release\cleanvm\updater\mac\row.sh'
    function Invoke-DryCapture([string] $windows) {
        $env:STUB_WINDOWS = $windows
        try {
            $said = & $bash -c 'PATH="$1:$PATH"; export PATH; bash "$2" --capture-dry-run NoSuchApp' _ (ConvertTo-Posix $stubs) (ConvertTo-Posix $rowSh) 2>&1 | Out-String
            return @{ Exit = $LASTEXITCODE; Text = $said.Trim() }
        }
        finally { Remove-Item Env:STUB_WINDOWS }
    }
    $none = Invoke-DryCapture '43 '
    Check ($none.Exit -eq 1 -and $none.Text -eq 'screenshot dry-run.png REFUSED: no on-screen NoSuchApp window of pids [101 202] among 43 windows; nothing captured') `
        "no window of the name: the capture refuses (exit $($none.Exit): $($none.Text))"
    $unread = Invoke-DryCapture ''
    Check ($unread.Exit -eq 1 -and $unread.Text -match 'REFUSED: the window list could not be read') "an unreadable window list: the capture refuses (exit $($unread.Exit): $($unread.Text))"
    $found = Invoke-DryCapture '43 812'
    Check ($found.Exit -eq 0 -and $found.Text -eq 'dry run: window 812 of NoSuchApp found among 43 windows; nothing captured') "a window found is named, not captured (exit $($found.Exit): $($found.Text))"
    Check (-not (Test-Path -LiteralPath $called)) 'the dry run never runs screencapture'

    # A capture that is not dry: screencapture stubbed to fail after a partial write, to write an
    # empty file, or to write an image. No file may be left when the log says nothing was captured.
    [IO.File]::WriteAllText((Join-Path $stubs 'screencapture'), (@(
                '#!/bin/sh', "echo `"`$*`" >> '$(ConvertTo-Posix $called)'", 'for a; do out=$a; done',
                'case "$STUB_SHOT" in', "  fail) printf 'PN' > `"`$out`"; exit 1 ;;", '  empty) : > "$out" ;;',
                "  ok) printf 'PNG-bytes' > `"`$out`" ;;", 'esac') -join "`n") + "`n")
    function Invoke-WindowCapture([string] $shot, [string] $png) {
        $env:STUB_WINDOWS = '43 812'; $env:STUB_SHOT = $shot
        try {
            $said = & $bash -c 'PATH="$1:$PATH"; export PATH; bash "$2" --capture-window "$3" Folio 101' _ (ConvertTo-Posix $stubs) (ConvertTo-Posix $rowSh) (ConvertTo-Posix $png) 2>&1 | Out-String
            return @{ Exit = $LASTEXITCODE; Text = $said.Trim() }
        }
        finally { Remove-Item Env:STUB_WINDOWS, Env:STUB_SHOT }
    }
    foreach ($shot in 'fail', 'empty') {
        $png = Join-Path $scratch "shot-$shot.png"
        $r = Invoke-WindowCapture $shot $png
        Check ($r.Exit -eq 1 -and $r.Text -eq "screenshot shot-$shot.png FAILED: screencapture -l 812 wrote no image; nothing captured" -and -not (Test-Path -LiteralPath $png)) `
            "screencapture that $(if ($shot -eq 'fail') { 'fails after a partial write' } else { 'writes an empty file' }): no file is left (exit $($r.Exit): $($r.Text))"
    }
    $png = Join-Path $scratch 'shot-ok.png'
    $r = Invoke-WindowCapture 'ok' $png
    Check ($r.Exit -eq 0 -and $r.Text -eq 'screenshot shot-ok.png (window 812)' -and (Test-Path -LiteralPath $png)) "an image written is kept (exit $($r.Exit): $($r.Text))"
    $args812 = @(Get-Content -LiteralPath $called)
    Check ($args812.Count -eq 3 -and @($args812 | Where-Object { $_ -notmatch '^-x -o -l 812 ' }).Count -eq 0) "screencapture is only ever asked for window 812 ($($args812 -join ' | '))"

    # ── the_host_reports_what_the_guest_saw ───────────────────────────────────
    $v = Get-RowVerdict -Cut -Marker "row=W4 phase=FAILED at=10:00:01.250`r`n" -WatchLog @()
    Check ($v.Failed -and $v.Line -eq 'marker: row=W4 phase=FAILED at=10:00:01.250') "a FAILED marker fails the row on the host ($($v.Line))"
    foreach ($phase in 'MISSED', 'NOT_REACHED') {
        $v = Get-RowVerdict -Cut -Marker "row=W5 phase=$phase at=10:00:02.000" -WatchLog @()
        Check $v.Failed "a $phase marker fails the row on the host ($($v.Line))"
    }
    $v = Get-RowVerdict -Cut -Marker $null -WatchLog @()
    Check ($v.Failed -and $v.Line -eq 'no marker came back from the guest') "a cut row with no marker fails ($($v.Line))"
    $v = Get-RowVerdict -Cut -Marker 'row=W1 phase=Allocated at=10:00:03.000' -WatchLog @('10:00:00.000 watch start row=W1')
    Check (-not $v.Failed -and $v.Line -eq 'reached Allocated') "a reached marker is ok ($($v.Line))"
    $log = @('10:00:00.000 watch start row= freeze=False', '10:02:00.000 ROW rollback FAILED: the trial pid=4242 was killed and is still running 2000 ms later')
    $v = Get-RowVerdict -WatchLog $log
    Check ($v.Failed -and $v.Line -eq 'watcher: 10:02:00.000 ROW rollback FAILED: the trial pid=4242 was killed and is still running 2000 ms later') "a live row's FAILED log line fails it on the host ($($v.Line))"
    $v = Get-RowVerdict -WatchLog @('10:00:00.000 watch start', '10:00:05.000 watch end')
    Check (-not $v.Failed) "a live row with no failure line is ok ($($v.Line))"
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) check(s) failed"
    exit 1
}
Write-Host 'the clean-VM scripts copy what they load and plan the rows they name'
