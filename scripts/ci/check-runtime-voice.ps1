# the_runtime_voice_list_only_shrinks
#
# `docs/plans/runtime-voice-direct-lines.tsv` lists every runtime road (an `impl` block of
# bt-app's `Runtime`) that still says its diagnostics.log line directly to `diagnostics::note`
# rather than through the window's door, `WindowHost::note`, with why no headless test reads it
# yet (T-DIAGNOSTICS-TEST-SINK; ARCHITECTURE-GUARDS.md "Runtime voice"). Within one tree,
# bt-source's `runtime_voice` test holds the code to it: every direct line is a row, and every
# row's count is its road's. This is the other half, across commits, on the pattern of
# `check-window-seam-native.ps1`: **the list only shrinks against the merge base.** A row's
# identity is its first two columns - `road`, `shape`; its reason may be reworded. A row the
# merge base does not carry is refused, by identity, and so is a row whose `count` grew.
#
# The baseline is the list committed at `git merge-base HEAD origin/main`. When the merge base
# has no list (the branch that introduces it), the baseline is the seed below: the rows the list
# was introduced with. There is no "nothing to compare, so pass" road: no origin/main or no
# merge base is exit 2.
#
# It reads only the committed TSV, through git and the working tree. `.github/workflows/ci.yml`
# runs it in `logic`, and `gates-can-fail` plants a row (it must go red, naming it).

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/runtime-voice-direct-lines.tsv"
$list = Join-Path $repo $relative

# The rows the list was introduced with (T-DIAGNOSTICS-TEST-SINK), by identity and count.
$seed = @(
    "crate::runtime::files::Runtime::ask_for_file_indexes_on_open`tcall`t1",
    "crate::runtime::panes::Runtime::land_birth_death_fallback`tvalue`t3",
    "crate::runtime::panes::Runtime::land_own_shell`tvalue`t3",
    "crate::runtime::panes::Runtime::land_pane_birth`tvalue`t1",
    "crate::runtime::panes::Runtime::land_restart`tvalue`t4",
    "crate::runtime::preview::Runtime::adopt_clipboard_picture`tcall`t1",
    "crate::runtime::preview::Runtime::check_picture_freshness`tcall`t2",
    "crate::runtime::tabs::Runtime::reap_exited_tabs`tcall`t1",
    "crate::runtime::tabs::Runtime::reap_exited_tabs`tvalue`t1",
    "crate::runtime::web::Runtime::make_spare_web_controller`tcall`t4",
    "crate::runtime::web::Runtime::warm_web_engine`tcall`t1",
    "crate::runtime::web::Runtime::warm_web_engine`tvalue`t1",
    "crate::runtime::windows::Runtime::keep_unsaved_edits`tcall`t1"
)

# The rows of one copy of the list: identity (`road`, `shape`) to count.
function Read-VoiceRows([string[]] $lines, [string] $what) {
    $rows = [System.Collections.Generic.Dictionary[string, int]]::new()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne "road`tshape`tcount`treason") {
                throw "$what does not begin with the header road/shape/count/reason: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 4) {
            throw "$what has a row that is not four columns: $line"
        }
        $count = 0
        if (-not [int]::TryParse($columns[2], [ref] $count)) {
            throw "$what has a row whose count is not a number: $line"
        }
        $key = "$($columns[0])`t$($columns[1])"
        if ($rows.ContainsKey($key)) {
            throw "$what has a road and shape written twice: $line"
        }
        $rows[$key] = $count
    }
    return , $rows
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - the runtime-voice guard holds the code to it"
}

Push-Location $repo
try {
    $now = Read-VoiceRows ([IO.File]::ReadAllLines($list)) "$relative (working tree)"

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        if ($LASTEXITCODE -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) rows and nothing to compare them against."
        Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
        exit 2
    }

    $text = (& git show "${base}:${relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -eq 0) {
        $from = "the merge base $($base.Substring(0, 12))"
        $before = Read-VoiceRows ($text -split "`r?`n") "$relative at $from"
    } else {
        $from = "the list's seed (the merge base $($base.Substring(0, 12)) has none)"
        $before = [System.Collections.Generic.Dictionary[string, int]]::new()
        foreach ($row in $seed) {
            $columns = $row.Split("`t")
            $before["$($columns[0])`t$($columns[1])"] = [int] $columns[2]
        }
    }
} finally {
    Pop-Location
}

$refused = [System.Collections.Generic.List[string]]::new()
foreach ($key in $now.Keys) {
    $named = $key -replace "`t", " "
    if (-not $before.ContainsKey($key)) {
        $refused.Add("    $named (not at ${from})")
    } elseif ($now[$key] -gt $before[$key]) {
        $refused.Add("    $named (count $($before[$key]) -> $($now[$key]))")
    }
}
if ($refused.Count -gt 0) {
    $lines = $refused -join [Environment]::NewLine
    throw "$relative only shrinks: $($refused.Count) row(s) added or grown against ${from}:$([Environment]::NewLine)$lines$([Environment]::NewLine)Say the line through the window's door (WindowHost::note) rather than listing it."
}
Write-Host "$relative against ${from}: $($before.Count) rows -> $($now.Count), 0 added, 0 grown."
exit 0
