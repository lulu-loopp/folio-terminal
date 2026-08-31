# every_vendored_diff_declares_itself
#
# Apache-2.0 section 4(b) asks every file we changed in `vendor/alacritty_terminal`
# to say so, prominently, in the file itself. A list of changed files kept by hand
# rots the first time someone touches a twenty-third file, so this asks the bytes:
# compare each vendored file with the crates.io archive of the same version, and
# demand that "differs" and "carries the notice" are the same set.
#
# The upstream copy comes from the local cargo registry when it is there — but a
# workspace that carries this crate only as a path dependency never asks cargo to
# unpack it, so a machine that has built this tree may still not have the source.
# Then the pristine archive is taken instead, from the registry cache or from
# crates.io, and held to a pinned checksum either way.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$vendor = Join-Path $repo "vendor\alacritty_terminal"

$version = "0.26.0"
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
$candidates = @(Get-ChildItem -Path (Join-Path $cargoHome "registry\src") -Directory -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName "alacritty_terminal-$version" } |
    Where-Object { Test-Path $_ })

if ($candidates.Count -eq 0) {
    # sha256 of alacritty_terminal-0.26.0.crate as published on crates.io.
    $pinned = "BDA177466B9524D59F1B12F0DD30B68696788E9992A7E959021C4A0ED96FCF59"
    $work = Join-Path ([System.IO.Path]::GetTempPath()) "folio-vendor-gate-alacritty_terminal-$version"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Path $work | Out-Null
    $file = Join-Path $work "alacritty_terminal-$version.crate"
    $cached = Get-ChildItem -Path (Join-Path $cargoHome "registry\cache") -Recurse `
        -Filter "alacritty_terminal-$version.crate" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cached) {
        Copy-Item $cached.FullName $file
    } else {
        Invoke-WebRequest -Uri "https://static.crates.io/crates/alacritty_terminal/alacritty_terminal-$version.crate" -OutFile $file
    }
    if ((Get-FileHash $file -Algorithm SHA256).Hash -ne $pinned) {
        throw "alacritty_terminal-$version.crate does not match the pinned crates.io checksum"
    }
    tar -xzf $file -C $work
    if ($LASTEXITCODE -ne 0) { throw "could not unpack alacritty_terminal-$version.crate" }
    $candidates = @(Join-Path $work "alacritty_terminal-$version")
}
$upstream = $candidates[0]

# Not upstream's source: cargo writes these into the unpacked copy itself.
$ignored = @(".cargo-ok", ".cargo-checksum.json", ".cargo_vcs_info.json")
# Ours, and not a modified upstream file: the index the notices point at.
$added = @("CHANGES-FOLIO.md")

$mark = "MODIFIED BY THE FOLIO CONTRIBUTORS"

function Get-RelativeFiles([string]$root) {
    $map = @{}
    foreach ($f in Get-ChildItem -Path $root -Recurse -File) {
        $rel = $f.FullName.Substring($root.Length + 1).Replace("\", "/")
        if ($ignored -contains (Split-Path $rel -Leaf)) { continue }
        $map[$rel] = $f.FullName
    }
    return $map
}

$up = Get-RelativeFiles $upstream
$vd = Get-RelativeFiles $vendor

$problems = New-Object System.Collections.Generic.List[string]

foreach ($rel in ($vd.Keys | Sort-Object)) {
    if (-not $up.ContainsKey($rel)) {
        if ($added -notcontains $rel) {
            $problems.Add("$rel is not an upstream file and is not in this script's `$added list")
        }
        continue
    }

    $same = (Get-FileHash $vd[$rel] -Algorithm SHA256).Hash -eq (Get-FileHash $up[$rel] -Algorithm SHA256).Hash
    # Read only the head: the notice is required to be prominent, so finding it
    # buried three thousand lines down would not satisfy 4(b) anyway.
    $head = (Get-Content -LiteralPath $vd[$rel] -TotalCount 12 -ErrorAction SilentlyContinue) -join "`n"
    $declares = $head -like "*$mark*"

    if ((-not $same) -and (-not $declares)) {
        $problems.Add("$rel differs from upstream $version but carries no modification notice")
    }
    if ($same -and $declares) {
        $problems.Add("$rel carries a modification notice but is byte-identical to upstream $version")
    }
}

foreach ($rel in ($up.Keys | Sort-Object)) {
    if (-not $vd.ContainsKey($rel)) {
        $problems.Add("$rel is in upstream $version but missing from vendor/")
    }
}

if ($problems.Count -gt 0) {
    throw ("vendored alacritty_terminal does not declare its changes:" +
        [Environment]::NewLine + ($problems -join [Environment]::NewLine))
}

$declared = @($vd.Keys | Where-Object {
    $up.ContainsKey($_) -and
    (Get-FileHash $vd[$_] -Algorithm SHA256).Hash -ne (Get-FileHash $up[$_] -Algorithm SHA256).Hash
}).Count
Write-Host "vendor/alacritty_terminal: $declared modified files, all declared"
