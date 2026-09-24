# every_vendored_diff_declares_itself
#
# Apache-2.0 section 4(b) asks every file we changed under `vendor/` to say so,
# prominently, in the file itself. A list of changed files kept by hand rots the
# first time someone touches a twenty-fourth file, so this asks the bytes:
# compare each vendored file with the crates.io archive of the same version, and
# demand that "differs" and "carries the notice" are the same set.
#
# The upstream copy comes from the local cargo registry when it is there — but a
# workspace that carries these crates only as path dependencies never asks cargo
# to unpack them, so a machine that has built this tree may still not have the
# source. Then the pristine archive is taken instead, from the registry cache or
# from crates.io, and held to a pinned checksum either way.
#
# One entry per vendored crate. A crate vendored without an entry here is a crate
# whose changes nothing checks, which is the failure this file exists to make
# noisy — so add the row in the same commit as the directory.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

# `sha256` is the checksum of the published `.crate` archive, as `Cargo.lock`
# carried it before the crate was patched to a path (patching removes the line,
# which is exactly why it is copied here).
# `added` is what is ours rather than a modified upstream file.
$crates = @(
    @{
        name   = "alacritty_terminal"
        version = "0.26.0"
        dir    = "vendor\alacritty_terminal"
        sha256 = "BDA177466B9524D59F1B12F0DD30B68696788E9992A7E959021C4A0ED96FCF59"
        added  = @("CHANGES-FOLIO.md")
    },
    @{
        name   = "mitex"
        version = "0.2.4"
        dir    = "vendor\mitex"
        sha256 = "81F1EB466EFDD212FE624F8BF24083F21D582F2D08E3B879BC4A059B25969D6C"
        added  = @("CHANGES-FOLIO.md")
    },
    @{
        name   = "mitex-parser"
        version = "0.2.4"
        dir    = "vendor\mitex-parser"
        sha256 = "1A42A2F86C46D250802262387DFB1D6BB18AFFD2D99328BBE4DBD12CE294C3A4"
        added  = @("CHANGES-FOLIO.md", "src/depth.rs")
    }
)

# Not upstream's source: cargo writes these into the unpacked copy itself.
$ignored = @(".cargo-ok", ".cargo-checksum.json", ".cargo_vcs_info.json")

$mark = "MODIFIED BY THE FOLIO CONTRIBUTORS"

function Get-RelativeFiles([string]$root, [string[]]$ignored) {
    $map = @{}
    foreach ($f in Get-ChildItem -Path $root -Recurse -File) {
        $rel = $f.FullName.Substring($root.Length + 1).Replace("\", "/")
        if ($ignored -contains (Split-Path $rel -Leaf)) { continue }
        $map[$rel] = $f.FullName
    }
    return $map
}

function Get-UpstreamSource([string]$name, [string]$version, [string]$pinned) {
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
    $candidates = @(Get-ChildItem -Path (Join-Path $cargoHome "registry\src") -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "$name-$version" } |
        Where-Object { Test-Path $_ })
    if ($candidates.Count -gt 0) { return $candidates[0] }

    $work = Join-Path ([System.IO.Path]::GetTempPath()) "folio-vendor-gate-$name-$version"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Path $work | Out-Null
    $file = Join-Path $work "$name-$version.crate"
    $cached = Get-ChildItem -Path (Join-Path $cargoHome "registry\cache") -Recurse `
        -Filter "$name-$version.crate" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cached) {
        Copy-Item $cached.FullName $file
    } else {
        Invoke-WebRequest -Uri "https://static.crates.io/crates/$name/$name-$version.crate" -OutFile $file
    }
    if ((Get-FileHash $file -Algorithm SHA256).Hash -ne $pinned) {
        throw "$name-$version.crate does not match the pinned crates.io checksum"
    }
    tar -xzf $file -C $work
    if ($LASTEXITCODE -ne 0) { throw "could not unpack $name-$version.crate" }
    return (Join-Path $work "$name-$version")
}

foreach ($crate in $crates) {
    $vendor = Join-Path $repo $crate.dir
    $upstream = Get-UpstreamSource $crate.name $crate.version $crate.sha256

    $up = Get-RelativeFiles $upstream $ignored
    $vd = Get-RelativeFiles $vendor $ignored

    $problems = New-Object System.Collections.Generic.List[string]

    foreach ($rel in ($vd.Keys | Sort-Object)) {
        if (-not $up.ContainsKey($rel)) {
            if ($crate.added -notcontains $rel) {
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
            $problems.Add("$rel differs from upstream $($crate.version) but carries no modification notice")
        }
        if ($same -and $declares) {
            $problems.Add("$rel carries a modification notice but is byte-identical to upstream $($crate.version)")
        }
    }

    foreach ($rel in ($up.Keys | Sort-Object)) {
        if (-not $vd.ContainsKey($rel)) {
            $problems.Add("$rel is in upstream $($crate.version) but missing from $($crate.dir)")
        }
    }

    if ($problems.Count -gt 0) {
        throw ("vendored $($crate.name) does not declare its changes:" +
            [Environment]::NewLine + ($problems -join [Environment]::NewLine))
    }

    $declared = @($vd.Keys | Where-Object {
        $up.ContainsKey($_) -and
        (Get-FileHash $vd[$_] -Algorithm SHA256).Hash -ne (Get-FileHash $up[$_] -Algorithm SHA256).Hash
    }).Count
    $where = $crate.dir.Replace("\", "/")
    Write-Host "${where}: $declared modified files, all declared"
}
