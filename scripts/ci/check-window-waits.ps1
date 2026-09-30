# the_window_threads_checks_that_are_not_rust
#
# Four checks the thread-door note 2026-09-26 gives A2a (revisions (j)1, (j)3,
# (j)11.4-(j)11.6; budget note C-2), each reported under its own name, all run
# before the script answers:
#
#   1. the bare-site inventory only shrinks (below);
#   2. the configuration fence: the five clippy.toml locations;
#      no other clippy.toml or .clippy.toml; no legacy .cargo/config; no
#      CLIPPY_CONF_DIR anywhere a build could read it, and none set now;
#   3. nothing lowers the lint on raw effects outside the source: no -A, -W,
#      --allow, --warn or --cap-lints touching it in a workflow, a
#      .cargo/config.toml, or a RUSTFLAGS/CLIPPY_FLAGS in scripts/; no lint
#      table names it but the workspace's and bt-platform's, each at the level
#      this script is told (M7e);
#   4. a registry row added without a ruling is refused, and the rows pending
#      a ruling only shrink (M9).
#
# ── 1. the_bare_site_inventory_only_shrinks
#
# `docs/plans/window-thread-bare-sites.tsv` is the inventory of every vocabulary
# site in the product that sits outside a registered door (thread-door note
# 2026-09-26, revisions (j)1, (j)11.5 and (j)11.6). The lint cannot be switched
# on while such sites exist, so until A2e the file stands in for it, and its
# whole value is the direction it moves in: a ticket that routes a site through a
# door removes its row or lowers its count; nothing adds one.
#
# Two halves hold it. The equality half is a test,
# `hang_watch::window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site`:
# the file is exactly what the code has. This is the historical half: the file
# is no larger than the baseline, key by key.
#
#   * The historical key is (crate, entry), with counts summed across arms and
#     items. Moving or renaming a function therefore changes no history.
#   * The baseline is the file at the pull request's merge base with origin/main.
#   * current[key] <= baseline[key], a key missing from the baseline read as
#     zero: a new effect family is refused, and so is a total that grew.
#
# No merge base at all (no origin/main in the clone) is not a pass either: the
# comparison did not happen. CI checks out with full history on both jobs that
# run this.
#
# Prove it fires before trusting it: `gates-can-fail` adds a bare site with its
# row and requires this to go red.

param(
    # The level `disallowed_methods` stands at in the two lint tables that may name it: none
    # until A2e writes `deny` in both (budget note C-2 item 1, as revision (i)5 narrows it). The
    # one place these values are said.
    [string]$WorkspaceLintLevel = "",
    [string]$PlatformLintLevel = ""
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$failures = @()

# Every file under the tree, `target/`, `.git/` and `node_modules/` aside.
function Get-TreeFiles([string]$directory) {
    $out = @()
    foreach ($item in (Get-ChildItem -LiteralPath $directory -Force)) {
        if ($item.PSIsContainer) {
            if (@("target", ".git", "node_modules") -contains $item.Name) { continue }
            $out += Get-TreeFiles $item.FullName
        } else {
            $out += $item
        }
    }
    return $out
}

function Get-Relative([string]$path) {
    return [IO.Path]::GetRelativePath($repo, $path).Replace('\', '/')
}

# Lines that are not comments, for the text checks: a comment that names a flag is not the flag.
function Get-CodeLines([string]$path) {
    return @([IO.File]::ReadAllLines($path) | Where-Object { -not $_.TrimStart().StartsWith("#") })
}

$tree = Get-TreeFiles $repo

# ── 2. the configuration fence ────────────────────────────────────────────────────────────────
# The closed list. The root file holds today's three test settings and no vocabulary (A2e appends
# it); the three shields are each `disallowed-methods = []` under a comment naming revision (j);
# probe's is generated from the registry and pinned by `check-lint-probe.ps1`.
# Contents are owned by their own gates; a pasted hash is not a judgement.
$fence = @(
    "clippy.toml",
    "vendor/clippy.toml",
    "crates/bt-corpus/clippy.toml",
    "crates/bt-source/clippy.toml",
    "crates/bt-lint-probe/clippy.toml"
)
foreach ($file in $fence) {
    $path = Join-Path $repo $file
    if (-not (Test-Path -LiteralPath $path)) { $failures += "[fence] $file is one of the five and is missing"; continue }
}
foreach ($item in $tree) {
    $file = Get-Relative $item.FullName
    if (($item.Name -eq "clippy.toml" -or $item.Name -eq ".clippy.toml") -and $file -notin $fence) {
        $failures += "[fence] $file is a clippy configuration outside the closed list: clippy reads the nearest one, so it would replace the vocabulary for everything below it"
    }
    if ($item.Name -eq "config" -and $item.Directory.Name -eq ".cargo") {
        $failures += "[fence] $file is Cargo's legacy configuration file, which Cargo prefers to config.toml: the tree keeps its Cargo configuration in .cargo/config.toml only"
    }
    if ($item.Name -eq "config.toml" -and $item.Directory.Name -eq ".cargo") {
        $section = ""
        foreach ($line in (Get-CodeLines $item.FullName)) {
            if ($line.Trim().StartsWith("[")) { $section = $line.Trim() }
            # `[env] CLIPPY_CONF_DIR = …`, `[env.CLIPPY_CONF_DIR]`, and the dotted `env.CLIPPY_CONF_DIR = …`.
            if (($section.StartsWith("[env") -and $line -match 'CLIPPY_CONF_DIR') -or $line -match '^\s*env\.CLIPPY_CONF_DIR') {
                $failures += "[fence] $file sets CLIPPY_CONF_DIR in [env]: every cargo child would read that directory's clippy.toml instead of the nearest"
            }
        }
    }
}
if ($env:CLIPPY_CONF_DIR) {
    $failures += "[fence] CLIPPY_CONF_DIR is set in this job ('$env:CLIPPY_CONF_DIR'): clippy would read that directory instead of the fence"
}
$self = Get-Relative $PSCommandPath
foreach ($item in $tree) {
    $file = Get-Relative $item.FullName
    $workflow = $file -like ".github/workflows/*.yml" -or $file -like ".github/workflows/*.yaml"
    $script = $file -like "scripts/*"
    if ((-not $workflow -and -not $script) -or $file -eq $self) { continue }
    if ((Get-CodeLines $item.FullName) -match 'CLIPPY_CONF_DIR') {
        $failures += "[fence] $file names CLIPPY_CONF_DIR: no job and no script chooses clippy's configuration directory"
    }
}

# ── 3. nothing lowers the lint outside the source (M7e) ──────────────────────────────────────
$lowered = 'clippy::disallowed_methods|clippy::style|clippy::all|warnings'
$lowering = "(^|\s|[""'=\[,])(-A|-W|--allow|--warn)(\s*=?\s*|\s+)[""']?($lowered)\b|--cap-lints"
foreach ($item in $tree) {
    $file = Get-Relative $item.FullName
    $workflow = $file -like ".github/workflows/*.yml" -or $file -like ".github/workflows/*.yaml"
    $cargo = $item.Name -eq "config.toml" -and $item.Directory.Name -eq ".cargo"
    $script = $file -like "scripts/*" -and $file -ne $self
    if (-not ($workflow -or $cargo -or $script)) { continue }
    foreach ($line in (Get-CodeLines $item.FullName)) {
        if ($script -and $line -notmatch 'RUSTFLAGS|CLIPPY_FLAGS') { continue }
        if ($line -match $lowering) {
            $failures += "[lowering] $file lowers a lint on raw effects outside the source: $($line.Trim())"
        }
    }
}
# The lint tables: only the workspace's and bt-platform's may name the lint, at the given level.
$allowedTables = @{
    "Cargo.toml|[workspace.lints.clippy]"            = $WorkspaceLintLevel
    "crates/bt-platform/Cargo.toml|[lints.clippy]"   = $PlatformLintLevel
}
$seenLevels = @{}
foreach ($item in ($tree | Where-Object { $_.Name -eq "Cargo.toml" })) {
    $file = Get-Relative $item.FullName
    $section = ""
    foreach ($line in (Get-CodeLines $item.FullName)) {
        $trimmed = $line.Trim()
        if ($trimmed.StartsWith("[")) { $section = $trimmed; continue }
        if ($section -notmatch '^\[(workspace\.)?lints') { continue }
        if ($trimmed -match '^(disallowed_methods|disallowed-methods|style|all|warnings)\s*=\s*(.+)$' -or $trimmed -match 'disallowed[_-]methods') {
            $key = "$file|$section"
            if (-not $allowedTables.ContainsKey($key)) {
                $failures += "[lowering] $file's $section names the lint ($trimmed): only the workspace table and bt-platform's may"
                continue
            }
            $seenLevels[$key] = $trimmed
        }
    }
}
foreach ($key in $allowedTables.Keys) {
    $wanted = $allowedTables[$key]
    $found = $seenLevels[$key]
    $level = if ($found -and $found -match '=\s*"?([a-z-]+)"?') { $Matches[1] } else { "" }
    if ($level -ne $wanted) {
        $failures += "[lowering] $($key.Replace('|', ' ')) sets disallowed_methods to '$level', where this script is told '$wanted'"
    }
}

$relative = "docs/plans/window-thread-bare-sites.tsv"
$inventory = Join-Path $repo $relative
$columns = "crate`tarm`titem`tentry`tcount"

# The rows of one version: (crate, entry) -> total count. Item and arm remain
# report columns in the equality test, but moves do not become historical debt.
function Read-Inventory([string[]]$lines, [string]$where) {
    $rows = [ordered]@{}
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne $columns) { throw "$where`: the columns are '$line', not '$columns'" }
            $header = $true
            continue
        }
        $cells = $line -split "`t"
        if ($cells.Count -ne 5) { throw "$where`: not five cells: $line" }
        $count = 0
        if (-not [int]::TryParse($cells[4], [ref]$count) -or $count -lt 1 -or "$count" -ne $cells[4]) {
            throw "$where`: the count is not a positive integer: $line"
        }
        $key = "$($cells[0])`t$($cells[3])"
        if ($rows.Contains($key)) { $rows[$key] += $count } else { $rows[$key] = $count }
    }
    if (-not $header) { throw "$where has no column header" }
    return $rows
}

# ── 1. the inventory (continued) ─────────────────────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $inventory)) {
    throw "$relative is not in the tree - until A2e deletes it after proving it empty, the inventory is what stands in for the lint"
}

Push-Location $repo
try {
    $now = Read-Inventory ([IO.File]::ReadAllLines($inventory)) "the working tree's $relative"

    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        $status = $LASTEXITCODE
        if ($status -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) rows and nothing to compare them against."
        Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
        exit 2
    }

    $text = (& git show "${base}:${relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        throw "$relative is not at the merge base $base; this inventory now needs a real merge-base baseline"
    }
    $against = "the merge base $($base.Substring(0, 12))"
    $before = Read-Inventory ($text -split "`r?`n") "$relative at $against"

    # ── 4. a registry row without a ruling (M9) ──────────────────────────────────────────────
    $registry = "crates/bt-app/src/window_waits.tsv"
    $then = (& git show "${base}:${registry}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) { throw "$registry is not at the merge base $base" }
} finally {
    Pop-Location
}

# The `# rows` section of a registry text: row -> (status, disposition).
function Read-RegistryRows([string[]]$lines) {
    $rows = [ordered]@{}
    $inside = $false
    $header = $false
    foreach ($line in $lines) {
        if ($line.StartsWith("##")) { continue }
        if ($line.StartsWith("# ")) { $inside = ($line -eq "# rows"); continue }
        if (-not $inside -or $line.Length -eq 0) { continue }
        if (-not $header) { $header = $true; continue }
        $cells = $line -split "`t"
        $rows[$cells[0]] = @{ status = $cells[1]; disposition = $cells[4] }
    }
    return $rows
}
$rowsNow = Read-RegistryRows ([IO.File]::ReadAllLines((Join-Path $repo $registry)))
$rowsThen = Read-RegistryRows ($then -split "`r?`n")
$designText = [IO.File]::ReadAllText((Join-Path $repo "docs/DESIGN.md"))
foreach ($row in $rowsNow.Keys) {
    $current = $rowsNow[$row]
    $previous = $rowsThen[$row]
    if ($current.status -eq "pending" -and -not ($previous -and $previous.status -eq "pending")) {
        $failures += "[ruling] registry row $row is 'pending' and was not pending at the merge base: the rows pending a ruling only shrink; a new row carries its ruling"
    }
    if (-not $previous -and $current.status -ne "pending") {
        if ($current.disposition -notmatch 'DESIGN\.md`?, (20\d\d-\d\d-\d\d)') {
            $failures += "[ruling] registry row $row is new and its disposition cites no dated DESIGN.md ruling"
        } else {
            $date = $Matches[1]
            if ($designText -notmatch "(?m)^### $([regex]::Escape($date)) —") {
                $failures += "[ruling] registry row $row cites DESIGN.md $date, but no dated heading exists"
            }
        }
    }
}

$added = @()
$grown = @()
$shrunk = 0
foreach ($key in $now.Keys) {
    $held = if ($before.Contains($key)) { $before[$key] } else { 0 }
    if ($held -eq 0) {
        $added += "    $($key -replace "`t", ' | ') ($($now[$key]))"
    } elseif ($now[$key] -gt $held) {
        $grown += "    $($key -replace "`t", ' | '): $held -> $($now[$key])"
    } elseif ($now[$key] -lt $held) {
        $shrunk += $held - $now[$key]
    }
}
foreach ($key in $before.Keys) {
    if (-not $now.Contains($key)) { $shrunk += $before[$key] }
}

$total = 0
foreach ($count in $now.Values) { $total += $count }
$was = 0
foreach ($count in $before.Values) { $was += $count }
Write-Host "$relative against $against`: $was sites -> $total, $($added.Count) key(s) added, $($grown.Count) grown, $shrunk site(s) removed."

if ($added.Count -gt 0 -or $grown.Count -gt 0) {
    $details = @()
    if ($added.Count -gt 0) { $details += "added (a key the baseline does not have):"; $details += $added }
    if ($grown.Count -gt 0) { $details += "grown (a count above the baseline's):"; $details += $grown }
    $failures += (
        "[inventory] the bare-site inventory only shrinks, and this adds to it:" + [Environment]::NewLine +
        ($details -join [Environment]::NewLine) + [Environment]::NewLine +
        "A new effect goes through its door (docs/ARCHITECTURE.md section 6); moves and renames already pass because history is grouped by effect family."
    )
}
if ($failures.Count -gt 0) {
    throw ("the window thread's checks failed:" + [Environment]::NewLine +
        (($failures | ForEach-Object { "  $_" }) -join [Environment]::NewLine))
}
Write-Host "the configuration fence holds: the five clippy.toml locations, no other, no CLIPPY_CONF_DIR, no legacy .cargo/config."
Write-Host "no workflow, cargo configuration or script flag lowers the lint; its two tables stand at '$WorkspaceLintLevel' and '$PlatformLintLevel'."
Write-Host "no registry row was added without a ruling; $(@($rowsNow.Values | Where-Object { $_.status -eq 'pending' }).Count) row(s) pending."
Write-Host "the window thread's bare sites: $total pending (docs/ARCHITECTURE.md section 6)."
