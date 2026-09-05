<#
.SYNOPSIS
    Build the release archive, and refuse to hand back one whose contents are not
    exactly the list.

.DESCRIPTION
    The archive is nine files and the list of them is the point. Two of the
    nine are a runtime contract rather than a convenience:

      * `conpty.dll` and `OpenConsole.exe` must sit in `folio.exe`'s OWN
        directory. `vendor/conpty/portable-pty/src/win/psuedocon.rs` looks for
        them beside `current_exe()` and nowhere else, and a build that cannot
        find them there refuses to start a shell rather than falling back to the
        system ConPTY. There is deliberately no `x64\OpenConsole.exe`: the NuGet
        package's native-targets layout mirrors the binary into that
        subdirectory, and this loader never reads it — carrying it would be 1.7
        MiB of a second copy nothing opens.

    One of the nine is not copied from anywhere: `folio.msix` is packed here,
    out of `packaging/msix/`. It is a **sparse** package and there is no program
    inside it — it is the identity and the COM class that put "Open in Folio" on
    the first page of the Windows 11 right-click menu, and the program it names
    lives at an external location, which is whatever folder the recipient
    extracted this archive into. That is why it ships beside `folio.exe` rather
    than being downloaded separately: the package and the executable it points at
    have to arrive in the same folder or the registration names a path with
    nothing at it. It is inert until somebody turns the row on in
    `Settings ▸ General ▸ First page of that menu`, which registers it for that user and
    needs no elevation. Nothing here registers anything on the machine that built
    it.

    The copy it is packed as stays in the output directory as well as going into
    the zip. `SHA256SUMS.txt` covers it there, and it is the file
    `smoke.ps1 -ExpectSigned` opens to read the package identity out of — which
    it could not do to a copy that only exists inside an archive.

    There is deliberately no `README.md` in it. Every relative link and every
    image in that file resolves against the repository, and inside a zip it
    resolves against nothing: a reader who opens it offline gets a page of dead
    references to screenshots they do not have. The archive carries what a
    recipient is owed rather than what the project would like to show them —
    the two licences, the third-party notices, and `TRADEMARK.md`, which says
    what the two licences do not grant.

    One asset is written beside the archive rather than into it:
    `option-ext-0.2.0.crate`. The MPL-2.0 asks that the Source Code Form be
    available to recipients, not that it be handed to each of them, and a 7 KB
    crate archive in the folder somebody extracts a terminal into is noise.
    `SHA256SUMS.txt` covers it, so the offer in `THIRD-PARTY-NOTICES.md` is made
    good by this release rather than by crates.io still being there.

    Everything here is a check on what was actually produced rather than a
    description of what was meant to be:

      * every name on the list is present, and nothing else is in the archive;
      * `folio.exe`'s own `VERSIONINFO` says the version being packaged, which
        is what makes the file's name and the file's contents one claim;
      * `AppxManifest.xml` in the tree still says `Version="0.0.0.0"`, so the
        version that reaches the package is this run's and not a second one
        somebody wrote down;
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
    Where the two licences, the third-party notices and the trademark notice
    are. Defaults to the repository root.

.PARAMETER Packaging
    Where the files that exist only to be shipped are. Defaults to
    `packaging/`. Today that is `folio-here.cmd`.

.PARAMETER Output
    Where the archive and `SHA256SUMS.txt` are written. Defaults to
    `target/release-package`. Anything already there is hashed into
    `SHA256SUMS.txt` alongside the archive, which is how the SBOM written by
    `sbom.ps1` before this runs ends up covered.

.PARAMETER ToolsOnly
    Find `makeappx.exe`, say which one, and stop. Nothing is read, built,
    packed or written.

    It exists for the release workflow, and for one reason: `makeappx` comes
    out of the Windows SDK, the SDK is on the runner image rather than in this
    repository, and an image that stopped carrying it would be found out
    **after** a ten-minute build and a `cargo install`. Asked first, it is found
    out in seconds. It is deliberately not a second copy of the search — it is
    the same `Find-MakeAppx` the pack below calls, which is the only thing that
    makes the answer worth anything.

.PARAMETER Sign
    Sign `folio.exe` and `folio.msix` with the Artifact Signing certificate
    profile before they go into the archive, and check that the two ConPTY files
    still carry Microsoft's own signature.

    The package needs the signature more than the executable does. An unsigned
    `folio.exe` is a program Windows warns about and runs; an unsigned
    `folio.msix` cannot be registered at all, so the Explorer menu row is a row
    that fails for everyone who turns it on. It is still packed without `-Sign`,
    on the same bargain the unsigned executable already makes: a file that is
    real and says what it is, rather than a script that cannot be exercised
    without a credential.

    **Off by default, and the archive is a real archive without it.** Signing
    needs somebody signed in to Azure, and the two places this script runs — a
    release workflow on a runner and a checkout on a laptop — mostly have nobody
    signed in. A packaging script that refused to package without a credential
    would be a packaging script that could not be exercised, which is the thing
    the note at the top of this file exists to prevent.

    Everything downstream sees the signed bytes: `folio.exe` is signed where the
    build left it, so the archive, `SHA256SUMS.txt` and whatever `smoke.ps1` is
    pointed at afterwards are all the same file. See `sign.ps1`.
#>

[CmdletBinding()]
param(
    [string] $Version,
    [string] $Binaries,
    [string] $Documents,
    [string] $Packaging,
    [string] $Output,
    [switch] $Sign,
    [switch] $ToolsOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
if (-not $Binaries) { $Binaries = Join-Path $root 'target\release' }
if (-not $Documents) { $Documents = $root }
if (-not $Packaging) { $Packaging = Join-Path $root 'packaging' }
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

# **`makeappx.exe`, found the way `sign.ps1` finds `signtool.exe`.** They come
# out of the same Windows SDK install and neither is on anybody's PATH: the SDK
# puts its tools under `Windows Kits\10\bin\<sdk version>\<arch>\` and adds
# nothing to the environment. The newest is taken because a kit left behind by an
# older Visual Studio is still on most machines that have a new one, and the
# question "which one" has to have an answer that does not depend on the order
# `Get-ChildItem` returns directories in.
function Get-FileVersionNumber {
    param([string] $Path)

    $info = (Get-Item -LiteralPath $Path).VersionInfo
    return [version] ('{0}.{1}.{2}.{3}' -f
        $info.FileMajorPart, $info.FileMinorPart, $info.FileBuildPart, $info.FilePrivatePart)
}

function Find-MakeAppx {
    # x64, for the same reason `sign.ps1` takes the x64 signtool: it is the
    # architecture this release is built and packaged on, and a kit that has an
    # arm64 directory has an x64 one beside it.
    $roots = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "$env:ProgramFiles\Windows Kits\10\bin")
    $candidates = @()
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        foreach ($sdk in (Get-ChildItem -LiteralPath $root -Directory)) {
            $exe = Join-Path (Join-Path $sdk.FullName 'x64') 'makeappx.exe'
            if (Test-Path -LiteralPath $exe -PathType Leaf) { $candidates += $exe }
        }
    }
    if ($candidates.Count -eq 0) {
        throw ('no x64 makeappx.exe under any Windows Kit. Install the Windows SDK — the same ' +
               'install sign.ps1 takes signtool.exe from, and the signing tools feature is enough.')
    }

    $newest = $candidates |
        Sort-Object -Property @{ Expression = { Get-FileVersionNumber -Path $_ } } -Descending |
        Select-Object -First 1
    Write-Host "makeappx: $newest ($(Get-FileVersionNumber -Path $newest))"
    return $newest
}

if ($ToolsOnly) {
    Find-MakeAppx | Out-Null
    return
}

if (-not $Version) { $Version = Get-WorkspaceVersion }

# The archive, in the order a person opening it should meet it: the program,
# the two files it cannot start without, then what it is under and what that
# does not give them.
#
# **The README is deliberately not here.** It is written for a repository page:
# every link in it is relative (`docs/PRIVACY.md`, `CONTRIBUTING.md`) and every
# picture it shows is a file under `docs/screenshots/` and `assets/readme/`.
# Dropped into an archive on its own it is a page of dead links and broken
# images, which is worse than no page at all. It is read where it works - the
# repository and the releases page - and what ships here is what the licences
# require to ship.
#
# `folio-here.cmd` is here because it only works from here. It is one line —
# `folio.exe --cwd` on the directory it was started in — and `%~dp0` is what
# makes it a sibling reference rather than a path somebody has to edit: a
# program that opens an external terminal by running a command with no
# arguments (VS Code's `terminal.external.windowsExec` is the one it was
# written for) has nowhere to say which folder it means, and this says it for
# them. Outside the folder `folio.exe` was unpacked into it names nothing.
$manifest = @(
    @{ Name = 'folio.exe';                From = $Binaries },
    # The sparse package that puts "Open in Folio" on the first page of the
    # right-click menu. Packed further down out of `packaging/msix/` rather than
    # copied from a build directory, which is why it is marked `Packed` — it is
    # the one entry that does not exist yet when the list is checked. It is inert
    # in the archive: nothing registers until a user switches the row on in
    # `Settings ▸ General ▸ First page of that menu`.
    @{ Name = 'folio.msix';               From = $Output; Packed = $true },
    @{ Name = 'conpty.dll';               From = $Binaries },
    @{ Name = 'OpenConsole.exe';          From = $Binaries },
    @{ Name = 'folio-here.cmd';           From = $Packaging },
    @{ Name = 'LICENSE-MIT';              From = $Documents },
    @{ Name = 'LICENSE-APACHE';           From = $Documents },
    @{ Name = 'THIRD-PARTY-NOTICES.md';   From = $Documents },
    @{ Name = 'TRADEMARK.md';             From = $Documents }
)

# What has to be there already, which is everything some other step produced: a
# build, a checkout. The one entry this script packs itself cannot be asked for
# here, because it does not exist yet — makeappx's exit code is what says it was
# made, a few lines further down.
$missing = @()
foreach ($item in $manifest) {
    $item.Path = Join-Path $item.From $item.Name
    if ($item.ContainsKey('Packed')) { continue }
    if (-not (Test-Path -LiteralPath $item.Path -PathType Leaf)) { $missing += $item.Path }
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

# ── the sparse package ───────────────────────────────────────────────────────
#
# Packed after the check above and before the signing below, and the order is
# the whole of the reasoning:
#
#   * after the `VERSIONINFO` check, because a tree whose binary and whose
#     manifest disagree about the version is a tree nothing should be packed out
#     of. The cheapest disagreement to catch is the one already caught;
#   * before `-Sign`, because the package is signed too, and a file has to exist
#     before it can be signed;
#   * and the two signatures do not order each other. `folio.exe` is not inside
#     this package — that is what "sparse" means — so packing does not copy the
#     executable's bytes and a signature applied to either afterwards cannot
#     invalidate the other. Were the exe a payload of the package, it would have
#     to be signed first and this sequence would be a requirement rather than a
#     preference.

$packaging = Join-Path $root 'packaging\msix'
$layout = Join-Path $Output 'msix-layout'
if (Test-Path -LiteralPath $layout) { Remove-Item -LiteralPath $layout -Recurse -Force }
[System.IO.Directory]::CreateDirectory((Join-Path $layout 'images')) | Out-Null

# **The manifest is edited as a document and not as text.** The attribute that
# changes is `Version` on `<Identity>`, and the string `0.0.0.0` appears twice in
# that file — once as the attribute and once in the comment above it that
# explains why the attribute is a placeholder. A substitution over the text would
# rewrite the explanation as well, and the copy that reached the package would be
# the one nobody reads until something has already gone wrong.
#
# `PreserveWhitespace` before the load, because a document reloaded without it is
# saved with .NET's own indentation and every comment in that file — what the
# package is for, why `Publisher` may not be edited, why `AppListEntry` is
# `none` — moves. With it, the copy in the package is the file from the tree with
# one attribute changed, and it is worth reading when a registration fails. The
# one thing it cannot keep is where a start tag broke its line between two
# attributes: that spacing is markup rather than a node, and `<Identity>` and
# `<Package>` come back out on one line each.
$document = New-Object System.Xml.XmlDocument
$document.PreserveWhitespace = $true
$document.Load((Join-Path $packaging 'AppxManifest.xml'))

$namespaces = New-Object System.Xml.XmlNamespaceManager($document.NameTable)
$namespaces.AddNamespace('m', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
$identity = $document.SelectSingleNode('/m:Package/m:Identity', $namespaces)
if (-not $identity) { throw "packaging\msix\AppxManifest.xml has no <Identity> element" }

# **The placeholder is checked before it is replaced.** `0.0.0.0` in the tree is
# the statement that this file holds no version of its own; anything else there
# is a second version number somebody has written down, and it would be
# overwritten here without ever being read.
$placeholder = $identity.GetAttribute('Version')
if ($placeholder -ne '0.0.0.0') {
    throw ("packaging\msix\AppxManifest.xml says Version=`"$placeholder`". It is 0.0.0.0 in the " +
           'tree on purpose: the version is injected here out of Cargo.toml, and a real number in ' +
           'that file is a second one, free to disagree with the binary it ships beside.')
}

# **Four parts, and the fourth is zero.** A package version is four numbers and
# nothing else — no `-preview`, no `+meta`, and never three parts — so the
# suffix is cut off exactly the way the `VERSIONINFO` check above cuts it, and
# `$core` is that cut already made. What a pre-release ships is a package that
# says `0.2.1.0` beside a binary that says `0.2.1-preview`: the package identity
# is a version Windows compares, not a name a person reads.
$packageVersion = "$core.0"
$identity.SetAttribute('Version', $packageVersion)
$document.Save((Join-Path $layout 'AppxManifest.xml'))

# The three logos the manifest names, by name. A wildcard copy would carry
# whatever else ends up in that directory into the package, and the manifest is
# the list of what the package holds rather than a description of it.
foreach ($logo in @('Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png')) {
    Copy-Item -LiteralPath (Join-Path (Join-Path $packaging 'images') $logo) `
              -Destination (Join-Path (Join-Path $layout 'images') $logo) -Force
}

$makeappx = Find-MakeAppx
$msix = Join-Path $Output 'folio.msix'

# **`/nv`, and it is not a shortcut.** makeappx's semantic validation checks that
# every file a manifest names is in the package, and the whole point of a sparse
# package is that `folio.exe` is not: without the flag it refuses with
# `The file name "folio.exe" declared for element ".../Application" doesn't exist
# in the package`, which is a description of the design rather than a fault in
# it. Microsoft's own instructions for granting identity by external location
# pass it for this reason. The manifest's *structure* is still checked — a
# misspelled element or a namespace that is not declared fails with the flag on.
#
# **makeappx's exit code is read, not raised.** PowerShell 7 turns a native
# command's non-zero exit into a terminating error on its own while
# `$ErrorActionPreference` is `Stop`, which would throw one line before the check
# below and throw away everything makeappx said about which element it disliked.
# Turned off inside this scope only, so nothing else in this script changes
# behaviour; `sign.ps1` turns it off for the same reason and for the whole of
# itself.
$packing = & {
    $PSNativeCommandUseErrorActionPreference = $false
    & $makeappx pack /d $layout /p $msix /o /nv 2>&1
}
if ($LASTEXITCODE -ne 0) {
    $packing | ForEach-Object { Write-Host "  $_" }
    throw "makeappx pack exited $LASTEXITCODE"
}
Remove-Item -LiteralPath $layout -Recurse -Force
Write-Host "folio.msix: a sparse package, identity version $packageVersion"

# **Signing, before anything is measured or copied.** A signature is appended to
# the file, so it changes both the length and the hash: everything below this
# line — the lengths the archive is checked against, the archive itself,
# `SHA256SUMS.txt` — has to be taken from the signed bytes, and the only way to
# be sure of that is to sign first.
#
# That holds for the package as much as for the executable: a signature goes into
# an msix as a `AppxSignature.p7x` part, so the file it is added to is a
# different length and a different hash afterwards, and it is packed above this
# line and measured below it for exactly that reason.
#
# `folio.exe` and `folio.msix`, in one call — `signtool` signs a package with the
# command line it signs an executable with, and asking for both at once is one
# request to the service rather than two. `conpty.dll` and `OpenConsole.exe` are
# Microsoft's, and they arrive signed by Microsoft; putting our signature over
# that would replace a statement Windows already trusts with a newer and weaker
# one. What is checked about them is that the signature they came with is still
# valid and still time stamped — an unsigned ConPTY in the archive means the
# build pulled it from somewhere other than the package it is supposed to come
# from.
if ($Sign) {
    $signScript = Join-Path $PSScriptRoot 'sign.ps1'
    & $signScript -Files @((Join-Path $Binaries 'folio.exe'), $msix)
    Write-Host ''
    Write-Host 'the two ConPTY files, which are Microsoft-signed and not re-signed here:'
    & $signScript -VerifyOnly -Files @(
        (Join-Path $Binaries 'conpty.dll'),
        (Join-Path $Binaries 'OpenConsole.exe'))
    Write-Host ''
}

foreach ($item in $manifest) {
    $item.Length = (Get-Item -LiteralPath $item.Path).Length
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
    # one folder and not nine loose files — four of which only work while they
    # are beside each other, and a fifth (`folio.msix`) which names the folder
    # the other four are in.
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

# **The MPL-2.0 source offer, made good by the release rather than by a URL.**
# `option-ext` reaches this binary through `dirs` -> `dirs-sys`, and section 3.2
# obliges whoever distributes the executable to make the Source Code Form of the
# covered files available to recipients. `THIRD-PARTY-NOTICES.md` says where it
# is and pins it by hash; this puts the archive itself among the release assets,
# so the offer does not expire the day somebody else's host does.
#
# It is not put in the `.zip` — see the note at the top of this file.
$crateName = 'option-ext'
$crateVersion = '0.2.0'
$crateFile = "$crateName-$crateVersion.crate"

# The hash is read out of `Cargo.lock` rather than written here a second time.
# That entry is cargo's own record of the bytes it downloaded and verified, and a
# copy of it in this script is a copy that can go stale by itself.
$lock = Get-Content -LiteralPath (Join-Path $root 'Cargo.lock') -Raw
$entry = '(?ms)^\[\[package\]\]\r?\nname = "' + [regex]::Escape($crateName) +
         '"\r?\nversion = "' + [regex]::Escape($crateVersion) + '".*?^checksum = "([0-9a-f]{64})"'
if ($lock -notmatch $entry) {
    throw "Cargo.lock records no checksum for $crateName $crateVersion"
}
$crateSum = $Matches[1]

$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
$cache = Join-Path $cargoHome 'registry\cache'
$copies = @(Get-ChildItem -Path $cache -Recurse -Filter $crateFile -File -ErrorAction SilentlyContinue)
if ($copies.Count -eq 0) {
    throw "$crateFile is not under $cache - run ``cargo fetch --locked`` first"
}

# A registry cache can hold the same name under more than one index. Take the one
# whose bytes are the bytes `Cargo.lock` names, or none of them.
$exact = @($copies | Where-Object {
    (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $crateSum
})
if ($exact.Count -eq 0) {
    throw "no copy of $crateFile under $cargoHome hashes to the checksum Cargo.lock records ($crateSum)"
}
Copy-Item -LiteralPath $exact[0].FullName -Destination (Join-Path $Output $crateFile) -Force
Write-Host ''
Write-Host ('{0,12:N0}  {1}  (MPL-2.0 source, sha256 {2})' -f $exact[0].Length, $crateFile, $crateSum)

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
