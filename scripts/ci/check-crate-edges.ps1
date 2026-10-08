# every_first_party_edge_goes_down_a_layer
#
# The dependency direction of docs/ARCHITECTURE.md section 3, read from the manifests by cargo
# rather than by a text search. `cargo metadata --no-deps` lists every dependency a package
# declares, under every target table and whatever name it is renamed to, without resolving for
# the platform it runs on, so a `[target.'cfg(windows)'.dependencies]` edge is seen on any runner.
#
# FIRST-PARTY. A workspace member whose directory is under `crates/`. The vendored crates under
# `vendor/` are upstream's and are not judged. A dependency is first-party when its `path` is a
# first-party package's directory, so a renamed dependency is judged by the package it names.
#
# THE RULE. Every normal and build dependency between two first-party crates goes from a higher
# layer of `crate-layers.tsv` to a lower one. A same-layer or upward edge fails unless
# `crate-edge-exemptions.tsv` names it (from, to, kind).
#
# DEV-DEPENDENCIES ARE NOT LAYER EDGES. They are listed and not judged: they are not in the shipped
# graph, and Cargo allows them in both directions. Two pairs do exactly that: `bt-term` and `bt-pty`
# each name the other as a dev-dependency, and so do `bt-platform` and `bt-pty` (the latter under
# `cfg(unix)`). Section 3.2 rules that `bt-pty` naming `bt-term` only as a dev-dependency is the
# repair of that edge (D-13).
#
# WHAT ELSE FAILS. A first-party crate with no layer row (a new crate is placed on purpose) and a
# layer row naming no first-party crate. An exemption whose edge no longer exists or now goes down
# a layer: the change that removes the edge deletes the row, so the list cannot keep a row nobody
# needs. An exemption that is not at the merge base with origin/main: the list only shrinks. When
# the base has no exemption list (the change that introduces it) that comparison is skipped and
# says so; when there is no merge base at all the gate exits 2, because a comparison that did not
# happen must not look like one that agreed. Reading zero first-party crates is a refusal.
#
# -Metadata <file> reads that file in place of running cargo; `gates-can-fail` plants an edge in a
# copy of the real metadata this way. -Repo names the tree whose lists and history are read.
# `cargo metadata` is decoded as strict UTF-8.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Metadata
)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$strictUtf8 = [Text.UTF8Encoding]::new($false, $true)
$layersRelative = "scripts/ci/crate-layers.tsv"
$exemptionsRelative = "scripts/ci/crate-edge-exemptions.tsv"

function Read-MetadataText {
    if ($Metadata) {
        return $strictUtf8.GetString([IO.File]::ReadAllBytes($Metadata))
    }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = "cargo"
    $start.WorkingDirectory = $Repo
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @("metadata", "--format-version", "1", "--no-deps", "--locked", "--offline")) {
        $start.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "cargo metadata could not start" }
        $errorText = $process.StandardError.ReadToEndAsync()
        $memory = [IO.MemoryStream]::new()
        $process.StandardOutput.BaseStream.CopyTo($memory)
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "cargo metadata failed: $($errorText.Result)" }
        return $strictUtf8.GetString($memory.ToArray())
    } finally {
        $process.Dispose()
    }
}

# A list's rows: not blank, not a comment, after the exact column header; each has exactly the
# header's columns, none empty.
function Read-Rows([string[]]$Lines, [string]$Header, [string]$Relative) {
    $columns = $Header.Split("`t").Count
    $rows = @()
    $seenHeader = $false
    foreach ($line in $Lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $seenHeader) {
            if ($line -ne $Header) { throw "$Relative must begin its rows with the header '$($Header.Replace("`t", '<TAB>'))'" }
            $seenHeader = $true
            continue
        }
        $parts = $line.Split("`t")
        if ($parts.Count -ne $columns -or @($parts | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
            throw "$Relative has a row without its $columns tab-separated columns: $line"
        }
        $rows += , $parts
    }
    if (-not $seenHeader) { throw "$Relative has no column header" }
    return , $rows
}

function Read-ListFile([string]$Relative) {
    $path = Join-Path $Repo $Relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "$Relative is not in the tree" }
    return , ($strictUtf8.GetString([IO.File]::ReadAllBytes($path)) -split "`r?`n")
}

function Get-FullDirectory([string]$Path) {
    return [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

$meta = Read-MetadataText | ConvertFrom-Json

# --- the first-party crates and the edges between them ---------------------------------------
$cratesDirectory = Get-FullDirectory (Join-Path $meta.workspace_root "crates")
$members = [Collections.Generic.HashSet[string]]::new([string[]]@($meta.workspace_members))
$byDirectory = @{}
foreach ($package in $meta.packages) {
    $directory = Get-FullDirectory ([IO.Path]::GetDirectoryName($package.manifest_path))
    if ($members.Contains($package.id) -and (Get-FullDirectory ([IO.Path]::GetDirectoryName($directory))) -eq $cratesDirectory) {
        $byDirectory[$directory] = $package
    }
}
$crates = @($byDirectory.Values | ForEach-Object { $_.name } | Sort-Object)
if ($crates.Count -eq 0) { throw "the crate-edge gate read zero first-party crates under $cratesDirectory" }

$edges = [ordered]@{}
$devEdges = [Collections.Generic.SortedSet[string]]::new([StringComparer]::Ordinal)
foreach ($package in ($byDirectory.Values | Sort-Object { $_.name })) {
    foreach ($dependency in $package.dependencies) {
        if (-not $dependency.path) { continue }
        $target = $byDirectory[(Get-FullDirectory $dependency.path)]
        if (-not $target) { continue }
        $kind = if ($dependency.kind) { $dependency.kind } else { "normal" }
        if ($kind -eq "dev") {
            if ($target.name -ne $package.name) { $null = $devEdges.Add("$($package.name) -> $($target.name)") }
            continue
        }
        $key = "$($package.name)`t$($target.name)`t$kind"
        if (-not $edges.Contains($key)) {
            $edges[$key] = [pscustomobject]@{ From = $package.name; To = $target.name; Kind = $kind }
        }
    }
}

# --- the layers -------------------------------------------------------------------------------
$problems = [Collections.Generic.List[string]]::new()
$layer = @{}
foreach ($row in (Read-Rows (Read-ListFile $layersRelative) "crate`tlayer" $layersRelative)) {
    $name = $row[0]
    $number = 0
    if (-not [int]::TryParse($row[1], [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$number)) {
        throw "$layersRelative gives $name the layer '$($row[1])', which is not a whole number"
    }
    if ($layer.ContainsKey($name)) { throw "$layersRelative places $name twice" }
    $layer[$name] = $number
}
foreach ($name in ($layer.Keys | Sort-Object)) {
    if ($crates -notcontains $name) {
        $problems.Add("$layersRelative places $name, which is not a first-party crate of this workspace: delete its row")
    }
}
foreach ($name in $crates) {
    if (-not $layer.ContainsKey($name)) {
        $problems.Add("$name is a first-party crate missing from ${layersRelative}: place it in a layer on purpose (docs/ARCHITECTURE.md section 3)")
    }
}

# --- the exemptions ---------------------------------------------------------------------------
$exemptionRows = Read-Rows (Read-ListFile $exemptionsRelative) "from`tto`tkind`tledger`treason" $exemptionsRelative
$exempt = [ordered]@{}
foreach ($row in $exemptionRows) {
    if ($row[2] -ne "normal" -and $row[2] -ne "build") {
        throw "$exemptionsRelative names the kind '$($row[2])' for $($row[0]) -> $($row[1]); a layer edge is normal or build"
    }
    $key = "$($row[0])`t$($row[1])`t$($row[2])"
    if ($exempt.Contains($key)) { throw "$exemptionsRelative lists $($row[0]) -> $($row[1]) ($($row[2])) twice" }
    $exempt[$key] = [pscustomobject]@{ From = $row[0]; To = $row[1]; Kind = $row[2]; Ledger = $row[3] }
}

# --- every edge against the layers ------------------------------------------------------------
$down = 0
$exempted = @()
foreach ($key in $edges.Keys) {
    $edge = $edges[$key]
    if (-not $layer.ContainsKey($edge.From) -or -not $layer.ContainsKey($edge.To)) { continue }
    if ($layer[$edge.From] -gt $layer[$edge.To]) { $down++; continue }
    if ($exempt.Contains($key)) {
        $exempted += "$($edge.From) -> $($edge.To) ($($edge.Kind), $($exempt[$key].Ledger))"
        continue
    }
    $problems.Add(
        "$($edge.From) -> $($edge.To) ($($edge.Kind)) goes from layer $($layer[$edge.From]) to layer $($layer[$edge.To]): " +
        "an edge must go to a lower layer, and this one is not in $exemptionsRelative")
}
foreach ($key in $exempt.Keys) {
    $row = $exempt[$key]
    $name = "$($row.From) -> $($row.To) ($($row.Kind), $($row.Ledger))"
    if (-not $edges.Contains($key)) {
        $problems.Add("the exemption $name names an edge that no longer exists: delete its row in the same change")
    } elseif ($layer.ContainsKey($row.From) -and $layer.ContainsKey($row.To) -and $layer[$row.From] -gt $layer[$row.To]) {
        $problems.Add("the exemption $name names an edge that now goes down a layer: delete its row in the same change")
    }
}

# --- the exemption list against the merge base ------------------------------------------------
$noBase = $false
Push-Location $Repo
try {
    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        $found = & git merge-base HEAD origin/main 2>$null
        if ($LASTEXITCODE -eq 0) { $base = @($found)[0] }
    }
    if (-not $base) {
        $noBase = $true
    } else {
        $text = (& git show "${base}:${exemptionsRelative}" 2>$null) | Out-String
        if ($LASTEXITCODE -ne 0) {
            Write-Host "$exemptionsRelative is not at the merge base $($base.Substring(0, 12)): this change introduces it, so its rows are not compared."
        } else {
            $before = @{}
            foreach ($row in (Read-Rows ($text -split "`r?`n") "from`tto`tkind`tledger`treason" "$exemptionsRelative at $($base.Substring(0, 12))")) {
                $before["$($row[0])`t$($row[1])`t$($row[2])"] = $true
            }
            foreach ($key in $exempt.Keys) {
                if (-not $before.ContainsKey($key)) {
                    $row = $exempt[$key]
                    $problems.Add(
                        "the exemption $($row.From) -> $($row.To) ($($row.Kind), $($row.Ledger)) was added since the merge base " +
                        "$($base.Substring(0, 12)), and $exemptionsRelative only shrinks: an edge the layers refuse is removed, " +
                        "or the layers in $layersRelative change with docs/ARCHITECTURE.md section 3")
                }
            }
        }
    }
} finally {
    Pop-Location
}

Write-Host "$($crates.Count) first-party crates in $(@($layer.Values | Sort-Object -Unique).Count) layers; $($edges.Count) normal and build edges between them, $down down a layer, $($exempted.Count) exempted."
foreach ($line in $exempted) { Write-Host "  exempted: $line" }
Write-Host "$($devEdges.Count) dev-dependency edges, not layer edges: $($devEdges -join ', ')"

if ($problems.Count -gt 0) {
    throw ("the crate dependency direction is broken:" + [Environment]::NewLine + (($problems | ForEach-Object { "  $_" }) -join [Environment]::NewLine))
}
if ($noBase) {
    Write-Host "no merge base with origin/main in this clone - the exemption list was not compared with one."
    Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
    exit 2
}
Write-Host "every one of the $($edges.Count) normal and build edges between $($crates.Count) first-party crates goes down a layer or is one of $($exempt.Count) exemption(s)"
