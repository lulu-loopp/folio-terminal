<#
.SYNOPSIS
    Write a release's statement, `RELEASE-STATEMENT.txt`, from the files in the
    package directory; or, given its signature, check the release against it.

.DESCRIPTION
    **What a copy of Folio believes about an update is what this file says**
    (0.4.10 plan row 102, T-UPDATES-TRUST-OUR-OWN-KEY): the release's version
    and tag, the oldest version that may update to it, whether its Windows
    files carry our signing service's signature, the SHA-256 and size of each
    asset and of every member of the Windows archive — signed with the release
    key, Ed25519, outside this repository. The updater refuses a release whose
    statement does not verify with a key the running build embeds, whatever
    Authenticode says about it, so a release is vouched for by its statement
    and not by its certificate.

    Two steps, run in this order on the machine the release is made from, after
    `package.ps1` has packed the Windows half and the macOS assets have been
    fetched into the same directory, and before anything is uploaded
    (`docs/RELEASING.md`, the release order):

    1. **Without `-Signature`, it writes the statement.** The version is the
       workspace's; `min_from` is the `min_updater` the archive's own
       `folio.exe` carries in its release manifest; `authenticode` is `false`
       for an archive named `-unsigned`, and `true` only for an archive whose
       `folio.exe` and `folio.msix` `sign.ps1 -VerifyOnly` accepts and that
       carry one certificate whose subject is the Publisher the package
       declares (`signer-names.ps1`) — a signed name on files that are not
       signed, or not signed by the one signer, is refused. The text itself is
       written by `check-release-statement compose`, which is the product's
       own grammar (`bt_winres::release_statement`), so there is no second
       spelling of it here. A `RELEASE-STATEMENT.txt.sig` left from an earlier
       statement is removed, because it signs other bytes. It then prints the
       one command that signs, which is the key holder's to type.

    2. **With `-Signature`, it checks.** The signature is put beside the
       statement as `RELEASE-STATEMENT.txt.sig` and `check-release-statement
       verify` reads the directory as the published page will be read: the
       signature against the keys the binary embeds, the tag, every asset's
       size and SHA-256, the archive's members. Anything it refuses stops
       here, with its words, and exit 1. This check is mandatory before the
       upload (ruling R10); the release workflow makes it again over what was
       published.

    **No private key is read, written or named by this script.** The signing
    command it prints names the key file by a placeholder.

    **The check binary is prebuilt** (`-CheckBinary`), never compiled here: it
    is the release target's, built with the release from the same commit,
    `cargo build --release --locked -p bt-winres --features
    release-key,inflate --bin check-release-statement`.

.PARAMETER PackageDirectory
    The package directory: `target/release-package` unless given.

.PARAMETER Tag
    The tag the release goes out under. Defaults to `v<version>-preview`.

.PARAMETER Key
    The slot of the key that signs, `live` (the default) or `spare`. The
    statement names that key's id; a key retire statement is signed by `spare`.

.PARAMETER Retire
    Key ids this statement retires (sixteen lower-case hex digits each). Only
    the spare may retire the live key; the grammar refuses anything else.

.PARAMETER Date
    `YYYY-MM-DD`, information only. Today's date, in UTC, unless given.

.PARAMETER WindowsOnly
    State the Windows archive only, for a feed that carries no macOS image (a
    clean-machine rehearsal). A release page always carries both, and without
    this switch a missing image is refused.

.PARAMETER Signature
    The signature file the key holder made over `RELEASE-STATEMENT.txt`; given,
    the script checks rather than writes.

.PARAMETER CheckBinary
    The `check-release-statement` executable. Defaults to the release target's,
    `target/release/check-release-statement.exe`.

.EXAMPLE
    ./scripts/release/release-statement.ps1

    Write `target/release-package/RELEASE-STATEMENT.txt` and print the signing
    command.

.EXAMPLE
    ./scripts/release/release-statement.ps1 -Signature target/release-package/RELEASE-STATEMENT.txt.sig

    Check the package directory against the signed statement.
#>

[CmdletBinding()]
param(
    [string] $PackageDirectory,
    [string] $Tag,
    [ValidateSet('live', 'spare')] [string] $Key = 'live',
    [string[]] $Retire = @(),
    [string] $Date,
    [switch] $WindowsOnly,
    [string] $Signature,
    [string] $CheckBinary
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path
. (Join-Path $here 'release-manifest.ps1')
. (Join-Path $here 'signer-names.ps1')

$statementName = 'RELEASE-STATEMENT.txt'
$signatureName = 'RELEASE-STATEMENT.txt.sig'

if (-not $PackageDirectory) { $PackageDirectory = Join-Path $root 'target\release-package' }
$PackageDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PackageDirectory)
if (-not (Test-Path -LiteralPath $PackageDirectory -PathType Container)) {
    throw "there is no package directory at $PackageDirectory; package.ps1 writes it"
}
if (-not $CheckBinary) { $CheckBinary = Join-Path $root 'target\release\check-release-statement.exe' }
$CheckBinary = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CheckBinary)
if (-not (Test-Path -LiteralPath $CheckBinary -PathType Leaf)) {
    throw ("there is no check binary at $CheckBinary. It is built with the release: cargo build " +
           '--release --locked -p bt-winres --features release-key,inflate --bin check-release-statement')
}

$manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\](.*?)^\[') { throw 'Cargo.toml has no [workspace.package] table' }
if ($Matches[1] -notmatch '(?m)^\s*version\s*=\s*"(\d+\.\d+\.\d+)"') { throw '[workspace.package] declares no three-number version' }
$version = $Matches[1]
if (-not $Tag) { $Tag = "v$version-preview" }

# The binary's words, and its exit code, which is what decides.
function Invoke-Check {
    param([string[]] $Arguments)

    $ErrorActionPreference = 'Continue'
    $said = @(& $CheckBinary @Arguments 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    foreach ($line in $said) { Write-Host "  $line" }
    return [pscustomobject]@{ Exit = $code; Text = ($said -join "`n") }
}

$statementPath = Join-Path $PackageDirectory $statementName
$signaturePath = Join-Path $PackageDirectory $signatureName

# ── 2: the signed statement, checked against the directory ──────────────────

if ($Signature) {
    $Signature = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Signature)
    if (-not (Test-Path -LiteralPath $Signature -PathType Leaf)) { throw "there is no signature at $Signature" }
    if (-not (Test-Path -LiteralPath $statementPath -PathType Leaf)) {
        throw "there is no $statementName in $PackageDirectory; run this without -Signature first"
    }
    if ([IO.Path]::GetFullPath($Signature) -ne [IO.Path]::GetFullPath($signaturePath)) {
        Copy-Item -LiteralPath $Signature -Destination $signaturePath -Force
    }
    Write-Host "checking $PackageDirectory against its statement, for ${Tag}:"
    $checked = Invoke-Check @('verify', $PackageDirectory, $Tag)
    if ($checked.Exit -ne 0) {
        throw ("check-release-statement refused the release (exit $($checked.Exit)); nothing in " +
               "$PackageDirectory may be uploaded as it is")
    }
    Write-Host ''
    Write-Host "the release in $PackageDirectory is the one its statement signs"
    return
}

# ── 1: the statement, written from the directory ────────────────────────────

if ($Date) {
    if ($Date -notmatch '^\d{4}-\d{2}-\d{2}$') { throw "-Date is '$Date', which is not YYYY-MM-DD" }
}
else {
    $Date = [DateTime]::UtcNow.ToString('yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture)
}

# The Windows archive: one, under this version's name, signed or `-unsigned`
# (ruling R6). The copy under the stable name is not an asset the statement
# lists; the updater fetches the versioned one.
$archives = @(Get-ChildItem -LiteralPath $PackageDirectory -File |
    Where-Object { $_.Name -cmatch "^folio-$([regex]::Escape($version))-windows-x64(-unsigned)?\.zip$" })
if ($archives.Count -ne 1) {
    throw ("$PackageDirectory holds $($archives.Count) Windows archives of $version " +
           "($(($archives | ForEach-Object { $_.Name }) -join ', ')); a statement states exactly one")
}
$archive = $archives[0]
$unsigned = $archive.Name.EndsWith('-unsigned.zip', [StringComparison]::Ordinal)
$assets = @("windows-x64=$($archive.Name)")

$imageName = "Folio-$version-macos-arm64.dmg"
if (-not $WindowsOnly) {
    if (-not (Test-Path -LiteralPath (Join-Path $PackageDirectory $imageName) -PathType Leaf)) {
        throw ("there is no $imageName in $PackageDirectory. Fetch the macOS assets first; a " +
               'release states both halves (-WindowsOnly is for a feed that carries no image)')
    }
    $assets += "macos-arm64=$imageName"
}

# `min_from`, and the two files `authenticode` is about, out of the archive.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-release-statement-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
try {
    $rootFolder = "folio-$version/"
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive.FullName)
    try {
        foreach ($name in @('folio.exe', 'folio.msix')) {
            $entry = @($zip.Entries | Where-Object { $_.FullName -ceq "$rootFolder$name" })
            if ($entry.Count -ne 1) { throw "$($archive.Name) holds $($entry.Count) $rootFolder$name" }
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry[0], (Join-Path $scratch $name), $true)
        }
    }
    finally { $zip.Dispose() }

    $release = Read-ReleaseManifest -Exe (Join-Path $scratch 'folio.exe')
    if ($release.Version -ne $version) {
        throw "the folio.exe in $($archive.Name) says it is $($release.Version); the workspace is $version"
    }
    $minFrom = $release.MinUpdater

    # **A signed name is a claim, and it is checked** (R4: the updater reads the
    # signature itself and never takes this line's word; this keeps the line
    # true for everyone else who reads it). `-VerifyOnly` asks for a valid,
    # time-stamped signature and not whose; the line says our signing
    # service's, so both files must also carry one certificate whose subject is
    # the Publisher the package declares (the 1b+2 review's P2-7).
    if ($unsigned) {
        $authenticode = 'false'
    }
    else {
        Write-Host "$($archive.Name) carries no -unsigned, so its two files must be signed:"
        $exePath = Join-Path $scratch 'folio.exe'
        $msixPath = Join-Path $scratch 'folio.msix'
        & (Join-Path $here 'sign.ps1') -VerifyOnly -Files @($exePath, $msixPath)
        $refusal = Get-OneSignerRefusal `
            -Executable (Get-AuthenticodeSignature -LiteralPath $exePath).SignerCertificate `
            -Package (Get-AuthenticodeSignature -LiteralPath $msixPath).SignerCertificate `
            -Publisher (Get-PackagePublisher -Msix $msixPath)
        if ($refusal) { throw "$($archive.Name) is named as signed, but $refusal" }
        $authenticode = 'true'
    }
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}

if (Test-Path -LiteralPath $signaturePath) {
    Write-Host "removing $signatureName, which signs an earlier statement"
    Remove-Item -LiteralPath $signaturePath -Force
}

Write-Host ''
Write-Host "the statement of $Tag ($($assets -join ', '); min_from $minFrom; authenticode $authenticode):"
$composed = Invoke-Check (@('compose', $PackageDirectory, $Tag, $Date, $minFrom, $authenticode, $Key) +
    $assets + @($Retire | ForEach-Object { "retire=$_" }))
if ($composed.Exit -ne 0) { throw "check-release-statement refused to compose the statement (exit $($composed.Exit))" }

Write-Host ''
Write-Host "Sign it with the $Key release key, by the key holder, with OpenSSL (its own passphrase prompt):"
Write-Host "  openssl pkeyutl -sign -inkey <the $Key key file> -rawin -in `"$statementPath`" -out `"$signaturePath`""
Write-Host 'and then check the directory against it before anything is uploaded:'
Write-Host "  ./scripts/release/release-statement.ps1 -Signature `"$signaturePath`""
