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

    **the_watcher_reads_the_journal_where_the_build_writes_it** — the
    watcher's journal is `H\journal.json`, the name `update_txn.rs`
    `Home::journal` joins onto the home (U-31 H-1: the old watcher looked in
    `H\<txn>\journal.json` and never saw a phase).

    **update_and_restart_are_shift_tab** — the key driver lights Update and
    Restart with Shift+Tab: Tab lights Skip or Later (U-31 H-4).

    **every_guest_script_carries_a_bom** — the updater's guest scripts and
    `ui-probe.ps1` start with a UTF-8 byte-order mark (U-31 H-3).
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

    # ── the_watcher_reads_the_journal_where_the_build_writes_it ───────────────
    $txn = [IO.File]::ReadAllText((Join-Path $root 'crates\bt-app\src\update_txn.rs'))
    $home_ = [regex]::Match($txn, 'fn journal\(&self\) -> PathBuf \{\s*self\.root\.join\("([^"]+)"\)')
    Check $home_.Success 'update_txn.rs Home::journal joins one name onto the home'
    $watch = [IO.File]::ReadAllText((Join-Path $root 'scripts\release\cleanvm\updater\guest\watch.ps1'))
    Check ($watch -match [regex]::Escape("`$journalPath = Join-Path `$H '$($home_.Groups[1].Value)'")) `
        "watch.ps1 reads H\$($home_.Groups[1].Value), where Home::journal puts it"

    # ── update_and_restart_are_shift_tab ──────────────────────────────────────
    $keys = [IO.File]::ReadAllText((Join-Path $root 'scripts\release\cleanvm\updater\guest\keys.ps1'))
    foreach ($verb in 'update', 'restart') {
        $line = [regex]::Match($keys, "(?m)^\s*'\^$verb\`$'.*$").Value
        Check ($line -match "'chord', '-ProcId', `"\`$w`", '-Mods', 's', '-Name', 'Tab'" -and $line -match "'-Name', 'Enter'") `
            "keys.ps1 presses $verb as Shift+Tab, Enter"
    }

    # ── every_guest_script_carries_a_bom ──────────────────────────────────────
    foreach ($rel in @('scripts\release\cleanvm\updater\guest\watch.ps1', 'scripts\release\cleanvm\updater\guest\keys.ps1',
            'scripts\release\cleanvm\updater\guest\collect.ps1', 'scripts\dev\ui-probe.ps1',
            'scripts\release\release-manifest.ps1', 'scripts\release\smoke.ps1', 'scripts\release\cleanvm\in-guest.ps1')) {
        $head = [IO.File]::ReadAllBytes((Join-Path $root $rel))
        Check ($head.Length -ge 3 -and $head[0] -eq 0xEF -and $head[1] -eq 0xBB -and $head[2] -eq 0xBF) "$rel starts with a UTF-8 byte-order mark"
    }
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
