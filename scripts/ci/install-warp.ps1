<#
.SYNOPSIS
    Put a software Direct3D adapter beside the test binaries, so the GPU job has one.

.DESCRIPTION
    `bt-render`'s device tests are real: they ask wgpu for an adapter and build a
    device on it. A GitHub Windows runner has no display adapter, and
    `request_adapter` with `force_fallback_adapter: false` — which is what the
    production path asks for and therefore what the tests must ask for — comes
    back with nothing.

    The answer is the one wgpu's own Windows CI uses: drop Microsoft's WARP
    (`d3d10warp.dll`) next to the executable. DXGI prefers a `d3d10warp.dll`
    found beside the running binary over the system's, and the adapter it
    enumerates from it is a real DX12 adapter — so the tests exercise the same
    code path they exercise on a machine with a GPU, at software speed.

    Pinned by version AND by hash, like `extract-conpty-sidecar.ps1`: this pulls
    a binary off the public internet into a job that then runs it, and a
    package feed is not a thing to trust on its name alone.

.PARAMETER TargetDir
    The build profile directory — `target/debug` or `target/release`. The DLL is
    written there and in its `deps` subdirectory, because that is where cargo
    puts the test executables that will load it.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $TargetDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$version = '1.0.20'
# The NuGet package, and the one file taken out of it. Both hashes recorded on
# 2026-08-27 against the official feed.
$packageHash = 'e5fe5de661ce98b58ef9cfb736e73c0a7a2623d3bbf5f14839b2d55566d87e40'
$entryName = 'build/native/bin/x64/d3d10warp.dll'
$entryHash = '2a08692cba4c130593329255627fb915d666c90cda53d284594bc8438fd4f49d'

$stamp = Join-Path $TargetDir 'warp.txt'
if ((Test-Path -LiteralPath $stamp) -and
    ((Get-Content -LiteralPath $stamp -Raw).Trim() -eq $version)) {
    Write-Host "WARP $version is already here."
    return
}

$package = Join-Path ([System.IO.Path]::GetTempPath()) "Microsoft.Direct3D.WARP.$version.nupkg"
$uri = "https://www.nuget.org/api/v2/package/Microsoft.Direct3D.WARP/$version"
Write-Host "Downloading WARP $version"
# curl.exe and not Invoke-WebRequest: it is present on every Windows image this
# runs on, it retries, and it does not buffer fifteen megabytes through a
# PowerShell pipeline.
& curl.exe -L --retry 5 --silent --show-error $uri -o $package
if ($LASTEXITCODE -ne 0) { throw "downloading WARP failed: curl exited $LASTEXITCODE" }

$actual = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $packageHash) {
    throw "the WARP package does not match the pinned asset: $actual"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($package)
try {
    $entry = $archive.GetEntry($entryName)
    if ($null -eq $entry) { throw "the WARP package has no $entryName" }

    $stream = $entry.Open()
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        $bytes = $sha.ComputeHash($stream)
        $actual = (($bytes | ForEach-Object { $_.ToString('x2') }) -join '')
    }
    finally { $stream.Dispose() }
    if ($actual -ne $entryHash) { throw "the WARP DLL does not match the pinned asset: $actual" }

    foreach ($directory in @($TargetDir, (Join-Path $TargetDir 'deps'))) {
        [System.IO.Directory]::CreateDirectory($directory) | Out-Null
        $stream = $entry.Open()
        try {
            $target = [System.IO.File]::Open(
                (Join-Path $directory 'd3d10warp.dll'),
                [System.IO.FileMode]::Create,
                [System.IO.FileAccess]::Write,
                [System.IO.FileShare]::None)
            try { $stream.CopyTo($target) } finally { $target.Dispose() }
        }
        finally { $stream.Dispose() }
        Write-Host "  $directory\d3d10warp.dll"
    }
}
finally { $archive.Dispose() }

Set-Content -LiteralPath $stamp -Value $version -NoNewline
Remove-Item -LiteralPath $package -Force
