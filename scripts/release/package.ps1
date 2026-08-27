<#
.SYNOPSIS
    Build the release archive, and refuse to hand back one whose contents are not
    exactly the list.

.DESCRIPTION
    The archive is seven files and the list of them is the point. Two of the
    seven are a runtime contract rather than a convenience:

      * `conpty.dll` and `OpenConsole.exe` must sit in `folio.exe`'s OWN
        directory. `vendor/conpty/portable-pty/src/win/psuedocon.rs` looks for
        them beside `current_exe()` and nowhere else, and a build that cannot
        find them there refuses to start a shell rather than falling back to the
        system ConPTY. There is deliberately no `x64\OpenConsole.exe`: the NuGet
        package's native-targets layout mirrors the binary into that
        subdirectory, and this loader never reads it — carrying it would be 1.7
        MiB of a second copy nothing opens.

    Everything here is a check on what was actually produced rather than a
    description of what was meant to be:

      * every name on the list is present, and nothing else is in the archive;
      * `folio.exe`'s own `VERSIONINFO` says the version being packaged, which
        is what makes the file's name and the file's contents one claim;
      * every entry in the archive is byte-for-byte the size of what went in.

    Run it by hand exactly as the release workflow runs it. That is the whole
    reason it is a script and not a block of YAML — a packaging step that can
    only be exercised by pushing a tag is a packaging step nobody exercises.

.PARAMETER Version
    The version being packaged. Defaults to the workspace manifest's, which is
    the one place it is written (see `Cargo.toml`).

.PARAMETER Binaries
    Where the build put `folio.exe` and the ConPTY sidecar. Defaults to
    `target/release`.

.PARAMETER Documents
    Where the two licences and the third-party notices are. Defaults to the
    repository root.

.PARAMETER Output
    Where the archive and `SHA256SUMS.txt` are written. Defaults to
    `target/release-package`. Anything already there is hashed into
    `SHA256SUMS.txt` alongside the archive, which is how the SBOM written by
    `sbom.ps1` before this runs ends up covered.
#>

[CmdletBinding()]
param(
    [string] $Version,
    [string] $Binaries,
    [string] $Documents,
    [string] $Output
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
if (-not $Binaries) { $Binaries = Join-Path $root 'target\release' }
if (-not $Documents) { $Documents = $root }
if (-not $Output) { $Output = Join-Path $root 'target\release-package' }

function Get-WorkspaceVersion {
    $manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
    if ($manifest -notmatch '(?ms)^\[workspace\.package\](.*?)^\[') {
        throw 'Cargo.toml has no [workspace.package] table'
    }
    if ($Matches[1] -notmatch '(?m)^\s*version\s*=\s*"([^"]+)"') {
        throw '[workspace.package] declares no version'
    }
    return $Matches[1]
}

if (-not $Version) { $Version = Get-WorkspaceVersion }

# The archive, in the order a person opening it should meet it: the program,
# the two files it cannot start without, then what it is under.
#
# **The README is deliberately not here.** It is written for a repository page:
# every link in it is relative (`docs/PRIVACY.md`, `CONTRIBUTING.md`) and every
# picture it shows is a file under `docs/screenshots/` and `assets/readme/`.
# Dropped into an archive on its own it is a page of dead links and broken
# images, which is worse than no page at all. It is read where it works - the
# repository and the releases page - and what ships here is what the licences
# require to ship.
$manifest = @(
    @{ Name = 'folio.exe';                From = $Binaries },
    @{ Name = 'conpty.dll';               From = $Binaries },
    @{ Name = 'OpenConsole.exe';          From = $Binaries },
    @{ Name = 'LICENSE-MIT';              From = $Documents },
    @{ Name = 'LICENSE-APACHE';           From = $Documents },
    @{ Name = 'THIRD-PARTY-NOTICES.md';   From = $Documents }
)

$missing = @()
foreach ($item in $manifest) {
    $item.Path = Join-Path $item.From $item.Name
    if (Test-Path -LiteralPath $item.Path -PathType Leaf) {
        $item.Length = (Get-Item -LiteralPath $item.Path).Length
    }
    else {
        $missing += $item.Path
    }
}
if ($missing.Count -gt 0) {
    Write-Host 'the archive cannot be built; these are not there:'
    $missing | ForEach-Object { Write-Host "  $_" }
    throw "$($missing.Count) file(s) missing"
}

# **The file's own claim about itself.** `folio.exe` carries the version in its
# PE resources (`crates/bt-app/build.rs`), and this is the point at which that
# claim and the version in the archive's name are checked against each other —
# an archive called 0.1.0 holding a binary that says 0.0.0 is the exact failure
# a single-source version rule exists to prevent, and the only place it can be
# caught is here, on the artefact.
$info = (Get-Item -LiteralPath (Join-Path $Binaries 'folio.exe')).VersionInfo
$stamped = "$($info.FileMajorPart).$($info.FileMinorPart).$($info.FileBuildPart)"
$core = ($Version -split '[-+]')[0]
if ($stamped -ne $core) {
    throw "folio.exe says its version is $stamped; this archive says $Version"
}
if ($info.ProductVersion.Trim() -ne $Version) {
    throw "folio.exe's ProductVersion string is '$($info.ProductVersion)'; expected '$Version'"
}

$folder = "folio-$Version"
$staging = Join-Path $Output $folder
if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
[System.IO.Directory]::CreateDirectory($staging) | Out-Null
foreach ($item in $manifest) {
    Copy-Item -LiteralPath $item.Path -Destination (Join-Path $staging $item.Name) -Force
}

$archive = Join-Path $Output "$folder-windows-x64.zip"
if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $staging,
    $archive,
    [System.IO.Compression.CompressionLevel]::Optimal,
    # The folder goes in, so that extracting into a downloads directory produces
    # one folder and not seven loose files — three of which only work while they
    # are beside each other.
    $true)
Remove-Item -LiteralPath $staging -Recurse -Force

# **Read back what was written**, rather than trusting what was copied. The
# archive is the artefact; the staging directory is not.
$expected = @{}
foreach ($item in $manifest) { $expected["$folder/$($item.Name)"] = $item.Length }

$zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
try {
    $entries = @($zip.Entries | Where-Object { $_.FullName -notmatch '/$' })
    $names = @($entries | ForEach-Object { $_.FullName } | Sort-Object)
    $wanted = @($expected.Keys | Sort-Object)
    $difference = @(Compare-Object -ReferenceObject $wanted -DifferenceObject $names)
    if ($difference.Count -gt 0) {
        foreach ($entry in $difference) {
            $side = if ($entry.SideIndicator -eq '=>') { 'unexpected' } else { 'missing' }
            Write-Host "  $side : $($entry.InputObject)"
        }
        throw 'the archive does not hold exactly the listed files'
    }
    foreach ($entry in $entries) {
        if ($entry.Length -ne $expected[$entry.FullName]) {
            throw "$($entry.FullName) is $($entry.Length) bytes in the archive, $($expected[$entry.FullName]) on disk"
        }
    }
    Write-Host "$([IO.Path]::GetFileName($archive)) — $((Get-Item -LiteralPath $archive).Length) bytes"
    foreach ($entry in $entries | Sort-Object FullName) {
        '{0,12:N0}  {1}' -f $entry.Length, $entry.FullName | Write-Host
    }
}
finally { $zip.Dispose() }

# Every asset this release publishes, in the format `sha256sum -c` reads.
$sums = Join-Path $Output 'SHA256SUMS.txt'
if (Test-Path -LiteralPath $sums) { Remove-Item -LiteralPath $sums -Force }
$lines = @(
    Get-ChildItem -LiteralPath $Output -File |
        Sort-Object Name |
        ForEach-Object {
            '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
        }
)
Set-Content -LiteralPath $sums -Value $lines -Encoding ascii
Write-Host ''
$lines | Write-Host
