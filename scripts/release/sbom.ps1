<#
.SYNOPSIS
    The release's bill of materials, in CycloneDX 1.5, out of the lock file.

.DESCRIPTION
    Every package that goes into `folio.exe` on Windows, with its version, its
    licence expression and its `pkg:cargo/...` purl — which is what a scanner
    needs in order to tell a user of this release that one of the 550 crates
    underneath it has an advisory.

    Built from `cargo metadata --locked` rather than by a tool installed for the
    purpose. The two candidates (`cargo-sbom`, `cargo-cyclonedx`) each read the
    same `cargo metadata` and reshape it; buying that reshaping costs a
    `cargo install` of an unpinned toolchain-dependent binary inside the release
    job, which is a heavier thing to trust than the sixty lines below. If a
    future need arrives that this cannot serve — VEX, component hashes, a
    signature — that is the day to take the dependency, with the need as the
    argument.

    `--filter-platform` is what makes this a bill for the thing actually
    shipped: the lock file carries every crate every platform needs, and a
    Windows archive that listed `nix` and `wayland-client` would be describing a
    build that does not exist.

    Deterministic except for the timestamp: the components are sorted, and the
    document's serial number is derived from them, so two runs over the same
    lock file produce the same identity for the same set of parts.

.PARAMETER Output
    Where to write the document. Defaults to
    `target/release-package/folio-<version>.cdx.json`.
#>

[CmdletBinding()]
param(
    [string] $Output
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
$target = 'x86_64-pc-windows-msvc'

# The diagnostics go to a file rather than into the pipeline: cargo writes a
# resolver warning about the vendored member on stderr every time, and `2>&1`
# puts that sentence at the front of what is supposed to be a JSON document.
$diagnostics = [System.IO.Path]::GetTempFileName()
Push-Location $root
try {
    $raw = & cargo metadata --format-version 1 --locked --filter-platform $target 2> $diagnostics
    if ($LASTEXITCODE -ne 0) {
        Get-Content -LiteralPath $diagnostics | Write-Host
        throw "cargo metadata failed: exit $LASTEXITCODE"
    }
}
finally {
    Pop-Location
    Remove-Item -LiteralPath $diagnostics -Force -ErrorAction SilentlyContinue
}

$metadata = ($raw -join '') | ConvertFrom-Json
$packages = @($metadata.packages)
$product = $packages | Where-Object { $_.name -eq 'bt-app' }
if (-not $product) { throw 'cargo metadata does not know bt-app' }

function New-Component($package, [string] $kind) {
    $purl = "pkg:cargo/$($package.name)@$($package.version)"
    $component = [ordered]@{
        type      = $kind
        'bom-ref' = $purl
        name      = $package.name
        version   = $package.version
        purl      = $purl
    }
    if ($package.description) { $component.description = $package.description }
    if ($package.license) {
        # An expression and not a list of ids: `MIT OR Apache-2.0` is a choice
        # the recipient makes, and splitting it into two licence entries would
        # state that both apply.
        $component.licenses = @(@{ expression = $package.license })
    }
    elseif ($package.license_file) {
        # No SPDX expression, only a file in the crate. Named rather than
        # guessed at — `THIRD-PARTY-NOTICES.md` is where its text lives.
        $component.licenses = @(@{ license = @{ name = "see $($package.license_file) in the crate" } })
    }
    $references = @()
    if ($package.repository) {
        $references += @{ type = 'vcs'; url = $package.repository }
    }
    if ($package.source) {
        $references += @{ type = 'distribution'; url = "https://crates.io/crates/$($package.name)/$($package.version)" }
    }
    if ($references.Count -gt 0) { $component.externalReferences = $references }
    return $component
}

$components = @(
    $packages |
        Where-Object { $_.id -ne $product.id } |
        Sort-Object name, version |
        ForEach-Object { New-Component $_ 'library' }
)

# A serial number that is a function of the parts, so the same lock file always
# names the same bill. Shaped as a v4 UUID because that is the shape consumers
# validate; its bits are a digest rather than randomness, which is the point.
$fingerprint = ($components | ForEach-Object { $_.'bom-ref' }) -join "`n"
$sha = [System.Security.Cryptography.SHA256]::Create()
$digest = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($fingerprint))
$hex = (($digest[0..15] | ForEach-Object { $_.ToString('x2') }) -join '')
$serial = '{0}-{1}-4{2}-a{3}-{4}' -f $hex.Substring(0, 8), $hex.Substring(8, 4),
    $hex.Substring(13, 3), $hex.Substring(17, 3), $hex.Substring(20, 12)

$document = [ordered]@{
    bomFormat    = 'CycloneDX'
    specVersion  = '1.5'
    serialNumber = "urn:uuid:$serial"
    version      = 1
    metadata     = [ordered]@{
        timestamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        tools     = @(@{ vendor = 'Folio'; name = 'scripts/release/sbom.ps1'; version = $product.version })
        component = (New-Component $product 'application')
        properties = @(@{ name = 'cargo:filter-platform'; value = $target })
    }
    components   = $components
}

if (-not $Output) {
    $Output = Join-Path $root ('target\release-package\folio-{0}.cdx.json' -f $product.version)
}
[System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($Output)) | Out-Null
$json = $document | ConvertTo-Json -Depth 12
# UTF-8 without a byte-order mark, written outright rather than through
# `Set-Content -Encoding utf8`: that switch means "with a BOM" on Windows
# PowerShell and "without" on 7, and a JSON document that begins with one is a
# document half the parsers in the world refuse.
[System.IO.File]::WriteAllText($Output, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "$Output — $($components.Count) components for $target"
