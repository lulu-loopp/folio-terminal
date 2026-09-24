[CmdletBinding()]
param([Parameter(Mandatory)] [string] $Out)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/ci-build.ps1"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$Out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)
if (Test-Path -LiteralPath $Out) { throw 'BUILDINFO output must be a new directory' }
Push-Location $root
try {
    $commit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'cannot resolve HEAD' }
    $rustc = (& rustc -Vv) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'rustc -Vv failed' }
    $banner = Get-CiBuildVersion -Exe "$root/target/release/folio.exe"
    if ($banner -notmatch '^Folio (\S+) \([0-9a-f]+\)$') { throw 'invalid version banner' }
    $version = $Matches[1]
    New-Item -ItemType Directory -Path $Out | Out-Null
    foreach ($name in @('folio.exe', 'folio.pdb', 'conpty.dll', 'OpenConsole.exe')) {
        Copy-Item -LiteralPath "$root/target/release/$name" -Destination $Out
    }
    Copy-Item -LiteralPath "$root/target/release-package/folio-$version.cdx.json" -Destination $Out
    $hashes = [ordered]@{}
    Get-ChildItem -LiteralPath $Out -File | Sort-Object Name | ForEach-Object {
        $hashes[$_.Name] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $info = [ordered]@{
        schema = 1; commit = $commit; version = $version; rustc = $rustc
        profile = 'release'; command = 'cargo build --release --locked -p bt-app'; files = $hashes
    }
    [IO.File]::WriteAllText((Join-Path $Out 'BUILDINFO.txt'), ($info | ConvertTo-Json -Depth 5) + "`n")
    Assert-CiBuild -Directory $Out -Commit $commit | Out-Null
} finally { Pop-Location }
