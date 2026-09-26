<#
.SYNOPSIS
    Build the release archive, and refuse to hand back one whose contents are not
    exactly the list.

.DESCRIPTION
    The archive is ten files and the list of them is the point. Two of the
    ten are a runtime contract rather than a convenience:

      * `conpty.dll` and `OpenConsole.exe` must sit in `folio.exe`'s OWN
        directory. `vendor/conpty/portable-pty/src/win/psuedocon.rs` looks for
        them beside `current_exe()` and nowhere else, and a build that cannot
        find them there refuses to start a shell rather than falling back to the
        system ConPTY. There is deliberately no `x64\OpenConsole.exe`: the NuGet
        package's native-targets layout mirrors the binary into that
        subdirectory, and this loader never reads it — carrying it would be 1.7
        MiB of a second copy nothing opens.

    One of the ten is not copied from anywhere: `folio.msix` is packed here,
    out of `packaging/msix/`. It is a **sparse** package and there is no program
    inside it — it is the identity and the COM class that put "Open in Folio" on
    the first page of the Windows 11 right-click menu, and the program it names
    lives at an external location, which is whatever folder the recipient
    extracted this archive into. That is why it ships beside `folio.exe` rather
    than being downloaded separately: the package and the executable it points at
    have to arrive in the same folder or the registration names a path with
    nothing at it. It is inert until somebody sets
    `Settings ▸ General ▸ Explorer context menu` to `On the first page`, which
    registers it for that user and needs no elevation. Nothing here registers
    anything on the machine that built it.

    **It is packed in a working directory and leaves only inside the zip.** It
    used to stay beside the archive as an asset of its own; a file called
    `folio.msix` on a release page reads as an installer, and it was downloaded
    on its own by people who then had a package naming a folder with no
    `folio.exe` in it. The working directory is taken away once the archive has
    been read back, so what the output directory holds is exactly what the
    release page carries: the archive, the copy of it under the name that never
    changes, the bill of materials and `SHA256SUMS.txt`.
    `smoke.ps1 -ExpectSigned` reads the package identity out of the copy in the
    archive, which is the copy a recipient registers.

    There is deliberately no `README.md` in it. Every relative link and every
    image in that file resolves against the repository, and inside a zip it
    resolves against nothing: a reader who opens it offline gets a page of dead
    references to screenshots they do not have. The archive carries what a
    recipient is owed rather than what the project would like to show them —
    the two licences, the third-party notices, and `TRADEMARK.md`, which says
    what the two licences do not grant.

    **No crate archive is written.** `option-ext` is MPL-2.0 and section 3.2
    asks that the Source Code Form be available to recipients and that they be
    told how to get it, which `THIRD-PARTY-NOTICES.md` does by naming the exact
    version, the crates.io address it is served from and the SHA-256 that
    `Cargo.lock` records for it. A 7 KB file on the release page next to a
    terminal was one more asset to explain and nobody fetched it.

    Everything here is a check on what was actually produced rather than a
    description of what was meant to be:

      * every name on the list is present, and nothing else is in the archive;
      * `folio.exe`'s own `VERSIONINFO` says the version being packaged, which
        is what makes the file's name and the file's contents one claim;
      * **every other member is byte for byte what `folio.exe` says it is.**
        The executable carries the manifest of its own archive
        (`FOLIO_RELEASE_MANIFEST`, 0.4.6 ticket U-9): the name, SHA-256 and size
        of every member but itself and `folio.msix`, computed by `build.rs` when
        it was compiled. It is read out of the file — never by running it — and
        a member that is missing, unlisted, or whose bytes differ stops the run
        before anything is packed. `folio.exe`'s own signature is what signs
        those eight files, so a release whose `uninstall.cmd` changed after the
        build would ship a hash that no longer matches; this is where that is
        caught, rather than on a reader's machine;
      * `AppxManifest.xml` in the tree still says `Version="0.0.0.0"`, so the
        version that reaches the package is this run's and not a second one
        somebody wrote down;
      * every entry in the archive is byte-for-byte the size of what went in;
      * `folio-windows-x64.zip` — the copy under the name that is the same in
        every release — hashes to exactly what the archive hashes to.

    Run it by hand exactly as the release workflow runs it. That is the whole
    reason it is a script and not a block of YAML — a packaging step that can
    only be exercised by pushing a tag is a packaging step nobody exercises.

.PARAMETER Version
    The version being packaged. Defaults to the workspace manifest's, which is
    the one place it is written (see `Cargo.toml`).

.PARAMETER Binaries
    Where the build put `folio.exe` and the ConPTY sidecar. Defaults to
    `target/release`. Legacy packaging-only input for the CI rehearsal.

.PARAMETER Binary
    An unsigned directory downloaded by fetch-ci-build.ps1. Rechecks BUILDINFO,
    hashes and --version against this checkout's HEAD before packaging. Its SBOM
    is copied into Output. -Sign signs this directory's exe in place, so keep an
    unsigned copy if it will be packaged again.

.PARAMETER BuildLocal
    Explicitly opt into a local release build before packaging. This can consume
    over 8 GB in one rustc and disrupt an active workstation. Prefer -Binary.

.PARAMETER Documents
    Where the two licences, the third-party notices and the trademark notice
    are. Defaults to the repository root.

.PARAMETER Packaging
    Where the files that exist only to be shipped are. Defaults to
    `packaging/`. Today those are `folio-here.cmd` and `uninstall.cmd`.

.PARAMETER Output
    Where the archive, the copy of it under the stable name and `SHA256SUMS.txt`
    are written. Defaults to `target/release-package`. Anything left there is
    hashed into `SHA256SUMS.txt` alongside the archive, which is how the SBOM
    written by `sbom.ps1` before this runs ends up covered.

    **It is emptied first**, apart from that SBOM. The directory is the release
    page — `gh release create` is handed all of it — so a file the previous
    release left behind is a file the next release publishes, which is what
    0.4.0 nearly did with 0.3.0's archive.

    **What it empties is what this lane writes**, by name: the archive and the
    copy of it under the stable name, `SHA256SUMS.txt`, a bill of materials, the
    three macOS assets that are fetched in after this script has run, and the two
    directories this script works in. Anything else there is refused with its own
    name rather than deleted — a `-Output` pointed somewhere unexpected is the
    one way this script could take a file that is not a previous release's.

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
    [string] $Binary,
    [switch] $BuildLocal,
    [string] $Documents,
    [string] $Packaging,
    [string] $Output,
    [switch] $Sign,
    [switch] $ToolsOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..')).Path
# The archive's member list and the manifest `folio.exe` carries of it.
. (Join-Path $PSScriptRoot 'release-manifest.ps1')
if (-not $Binaries) { $Binaries = Join-Path $root 'target\release' }
if (-not $Documents) { $Documents = $root }
if (-not $Packaging) { $Packaging = Join-Path $root 'packaging' }
if (-not $Output) { $Output = Join-Path $root 'target\release-package' }
$Output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)

# **Where the package is built, and where it does not stay.** `$Output` is the
# release page: every file in it is uploaded and every file in it is hashed into
# `SHA256SUMS.txt`. `folio.msix` is not one of those files — it travels in the
# archive — so it is packed one directory down and that directory is removed
# once the archive has been read back. A subdirectory would be passed over by
# the hashing below in any case, which reads files and not folders; it is taken
# away so that what is left after a run is the three assets and nothing to
# wonder about.
$work = Join-Path $Output 'package-work'

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

if ($Binary -and ($PSBoundParameters.ContainsKey('Binaries') -or $BuildLocal)) {
    throw '-Binary cannot be combined with -Binaries or -BuildLocal'
}
if ($BuildLocal) {
    if ($PSBoundParameters.ContainsKey('Binaries')) { throw '-BuildLocal uses target/release; do not pass -Binaries' }
    Write-Warning 'Local fat-LTO release builds can use over 8 GB in one rustc and freeze an active workstation. Prefer CI and -Binary.'
    Push-Location $root
    try {
        & cargo build --release --locked -p bt-app
        if ($LASTEXITCODE -ne 0) { throw 'local release build failed' }
        & "$PSScriptRoot/sbom.ps1" -Output (Join-Path $Output "folio-$(Get-WorkspaceVersion).cdx.json")
    } finally { Pop-Location }
}
if ($Binary) {
    . "$PSScriptRoot/ci-build.ps1"
    $Binaries = (Resolve-Path -LiteralPath $Binary).Path
    $inputPrefix = $Binaries.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    $outputPrefix = $Output.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if ($inputPrefix.StartsWith($outputPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        $outputPrefix.StartsWith($inputPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw '-Binary and -Output must be separate, non-nested directories'
    }
    $commit = (& git -C $root rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'cannot resolve packaging checkout HEAD' }
    $buildInfo = Assert-CiBuild -Directory $Binaries -Commit $commit
    if ($buildInfo.version -ne (Get-WorkspaceVersion)) { throw 'CI build and packaging manifest versions differ' }
    if ($Version -and $Version -ne $buildInfo.version) { throw '-Version differs from the CI build' }
}

if (-not $Version) { $Version = Get-WorkspaceVersion }

# ── the output directory is the release page, and it starts empty ────────────
#
# **Everything left in it is uploaded.** `docs/RELEASING.md` hands
# `gh release create` the whole directory rather than a list of names, which is
# what makes it impossible to leave an asset out — and, until this block, just
# as impossible to leave one behind. On the 0.4.0 run the directory still held
# 0.3.0's archive and 0.3.0's `SHA256SUMS.txt` from the release before it: two
# releases' assets under two version numbers, with nothing between them and the
# page but somebody reading the upload list.
#
# So a run begins by taking the previous one away. Everything here is generated
# — it is a directory under `target\` — and the one generated file written
# *before* this script and needed *by* it is this version's bill of materials,
# which `sbom.ps1` wrote minutes ago and whose line goes into `SHA256SUMS.txt`
# below. That name is kept; the rest of the directory goes, files and folders
# alike, which includes the macOS assets of an earlier release if somebody
# fetched them here. **The macOS half is fetched into this directory after this
# script has run**, and `smoke.ps1` is the second net: it refuses a package
# directory holding a file whose name carries a version other than this one.
#
# Nothing outside `$Output` is reachable from here. The list comes from
# `Get-ChildItem` on that one directory and every entry is removed by its own
# `-LiteralPath`, so there is no pattern for a name to escape through and no
# recursion into anything that was not already inside it.
#
# **And it clears what this lane wrote, rather than whatever is there.** A
# directory somebody else's file is in is a directory this script may not empty:
# `-Output` is a parameter, the default is the only path anybody has ever passed,
# and the day a second one is passed the difference between "empty the previous
# release" and "empty that folder" is somebody's afternoon. So the names are
# written down as shapes — the four assets the Windows lane writes, the three the
# macOS half is fetched into it as, and the two directories this script works in
# — and a name that is none of them stops the run with its own name in the
# message. The version in a shape is left open on purpose: what makes a file the
# previous release's is that this lane wrote it, and the previous release is
# exactly the version that is not this one.
function Test-ReleasePageName {
    param([string] $Name)

    # A version as a file name can carry it: three numbers, and a pre-release or
    # build suffix for the archives of a channel that ever spells one out.
    $v = '\d+\.\d+\.\d+([-+][0-9A-Za-z.]+)?'
    foreach ($shape in @(
            "^folio-$v-windows-x64\.zip$",   # the archive
            '^folio-windows-x64\.zip$',      # the copy under the stable name
            "^folio-$v\.cdx\.json$",         # sbom.ps1's bill of materials
            '^SHA256SUMS\.txt$',
            '^SHA256SUMS-macos\.txt$',
            "^Folio-$v-macos-arm64\.dmg$",   # fetched from the Mac, after this runs
            '^Folio-macos-arm64\.dmg$',
            '^package-work$',                # this script's own working directory
            "^folio-$v$")) {                 # and its staging folder
        if ($Name -match $shape) { return $true }
    }
    return $false
}

$sbomName = "folio-$Version.cdx.json"
[System.IO.Directory]::CreateDirectory($Output) | Out-Null
$strangers = @(
    Get-ChildItem -LiteralPath $Output -Force |
        Where-Object { -not (Test-ReleasePageName -Name $_.Name) } |
        ForEach-Object { $_.Name }
)
if ($strangers.Count -gt 0) {
    Write-Host "$Output holds $($strangers.Count) file(s) this release lane did not write:"
    $strangers | ForEach-Object { Write-Host "  $_" }
    throw ('this directory is emptied before the archive is written, and it is not emptied ' +
           'while it holds something that was not put there by sbom.ps1, by this script or by ' +
           'the macOS fetch. Move those out, or point -Output at a directory of this lane''s own.')
}
foreach ($previous in (Get-ChildItem -LiteralPath $Output -Force)) {
    if ($previous.Name -eq $sbomName) { continue }
    Write-Host "cleared from $([IO.Path]::GetFileName($Output)): $($previous.Name)"
    Remove-Item -LiteralPath $previous.FullName -Recurse -Force
}
if ($Binary) {
    Copy-Item -LiteralPath (Join-Path $Binaries $sbomName) -Destination (Join-Path $Output $sbomName) -Force
}

# **The archive is `archive-members.txt`, in its order** — the program, the two
# files it cannot start without, then what it is under and what that does not
# give them — and nothing else. The list lives in that file rather than here
# because `crates/bt-app/build.rs` reads it too, to build the manifest
# `folio.exe` carries: one list, so the archive and the manifest cannot name
# different files. What each line's source means here:
#
#   * `exe` and `sidecar` come from the build, `$Binaries`: the executable, and
#     the ConPTY files `bt-pty`'s build script wrote beside it;
#   * `msix` is the sparse package that puts "Open in Folio" on the first page of
#     the right-click menu. It is packed further down out of `packaging/msix/`
#     rather than copied from anywhere, which is why it is marked `Packed` — it is
#     the one entry that does not exist yet when the list is checked — and into
#     `$work` rather than `$Output`, because it goes into the archive and nowhere
#     else. It is inert there: nothing registers until a user sets
#     `Settings ▸ General ▸ Explorer context menu` to `On the first page`;
#   * `packaging` comes from `$Packaging`: `folio-here.cmd`, which is one line —
#     `folio.exe --cwd` on the directory it was started in, with `%~dp0` making
#     it a sibling reference, for a program that opens an external terminal with
#     no way to say which folder it means (VS Code's
#     `terminal.external.windowsExec`) — and `uninstall.cmd`;
#   * `documents` comes from `$Documents`: the two licences, the third-party
#     notices and the trademark notice.
#
# **The README is deliberately not on the list.** It is written for a repository
# page: every link in it is relative and every picture it shows is a file under
# `docs/screenshots/` and `assets/readme/`. Dropped into an archive on its own it
# is a page of dead links and broken images, which is worse than no page at all.
$sourceDirectory = @{
    exe       = $Binaries
    sidecar   = $Binaries
    msix      = $work
    packaging = $Packaging
    documents = $Documents
}
$listed = Get-ArchiveMemberList
$members = @(
    foreach ($entry in $listed) {
        $item = @{ Name = $entry.Name; From = $sourceDirectory[$entry.Source]; InManifest = $entry.InManifest }
        if ($entry.Source -ceq 'msix') { $item.Packed = $true }
        $item
    }
)
$exempt = @($listed | Where-Object { -not $_.InManifest } | ForEach-Object { $_.Name })

# What has to be there already, which is everything some other step produced: a
# build, a checkout. The one entry this script packs itself cannot be asked for
# here, because it does not exist yet — makeappx's exit code is what says it was
# made, a few lines further down.
$missing = @()
foreach ($item in $members) {
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

# ── the members, against the manifest the executable carries ────────────────
#
# **Before anything is packed or signed**, because a refusal here is a release
# that must not be made, and the sooner it is said the less there is to undo.
# Signing below changes `folio.exe` and packs `folio.msix`, and the manifest
# names neither; the other eight are not touched by anything after this line, so
# the bytes checked here are the bytes that go into the archive.
#
# The manifest is `crates/bt-app/build.rs`'s, made from `archive-members.txt`
# and the tree the build compiled, so it disagrees with what is here only when
# the build and this packaging run saw different files — a document edited
# after the build, a sidecar from another build, a list changed without a
# rebuild. Every disagreement is printed with its member's name, and the run
# stops: a manifest that does not describe its archive is a release whose
# executable's signature vouches for bytes the archive does not hold.
$exe = Join-Path $Binaries 'folio.exe'
$release = Read-ReleaseManifest -Exe $exe
$expectedHeader = [ordered]@{
    Product     = 'folio'
    Version     = $Version
    Arch        = 'x64'
    ArchiveRoot = "folio-$Version"
}
foreach ($field in $expectedHeader.Keys) {
    if ($release.$field -cne $expectedHeader[$field]) {
        throw "folio.exe's release manifest says $field $($release.$field); this archive is $($expectedHeader[$field])"
    }
}
$found = @(
    foreach ($item in $members) {
        if (-not $item.InManifest) { continue }
        [pscustomobject]@{
            Name   = $item.Name
            Size   = (Get-Item -LiteralPath $item.Path).Length
            Sha256 = (Get-FileHash -LiteralPath $item.Path -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
)
$problems = @(Compare-ReleaseMembers -Manifest $release -Found $found -Exempt $exempt)
if ($problems.Count -gt 0) {
    Write-Host "the members do not match the release manifest folio.exe carries:"
    $problems | ForEach-Object { Write-Host "  $_" }
    throw ("$($problems.Count) member(s) differ from the manifest in folio.exe. Rebuild folio.exe from " +
           'this tree, or package the files it was built with.')
}
Write-Host "release manifest: $($release.Members.Count) members match folio.exe's (protocol $($release.Protocol), min updater $($release.MinUpdater))"

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
if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
$layout = Join-Path $work 'msix-layout'
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
$msix = Join-Path $work 'folio.msix'

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

foreach ($item in $members) {
    $item.Length = (Get-Item -LiteralPath $item.Path).Length
}

$folder = "folio-$Version"
$staging = Join-Path $Output $folder
if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
[System.IO.Directory]::CreateDirectory($staging) | Out-Null
foreach ($item in $members) {
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
    # one folder and not ten loose files — four of which only work while they
    # are beside each other, and a fifth (`folio.msix`) which names the folder
    # the other four are in.
    $true)
Remove-Item -LiteralPath $staging -Recurse -Force

# **Read back what was written**, rather than trusting what was copied. The
# archive is the artefact; the staging directory is not.
$expected = @{}
foreach ($item in $members) { $expected["$folder/$($item.Name)"] = $item.Length }

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

# **A second copy, under a name that is the same in every release.** GitHub
# serves `/releases/latest/download/<asset>`, which is the one download link a
# page outside this repository can carry without it going stale — and it
# resolves an asset by *name*, so it can only find a name no version moves.
# `folio-<version>-windows-x64.zip` is not one of those; `folio-windows-x64.zip`
# is.
#
# It is a copy of the bytes and not a second archive, so there is nothing for the
# two to disagree about, and it is made here rather than by whoever uploads:
# `SHA256SUMS.txt` below hashes this directory, so both names reach the release
# page carrying one hash between them and a reader who fetched either can check
# what they have. The hash is compared on the spot, because a copy nobody read
# back is a copy.
$stable = Join-Path $Output 'folio-windows-x64.zip'
Copy-Item -LiteralPath $archive -Destination $stable -Force
$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
$stableHash = (Get-FileHash -LiteralPath $stable -Algorithm SHA256).Hash
if ($stableHash -ne $archiveHash) {
    throw ("$([IO.Path]::GetFileName($stable)) is not a copy of " +
           "$([IO.Path]::GetFileName($archive)): $stableHash against $archiveHash")
}
Write-Host ("$([IO.Path]::GetFileName($stable)) — the same bytes, under the name " +
            '/releases/latest/download/ resolves')

# **The working directory goes away here**, after the archive has been read
# back and before anything in `$Output` is hashed. `folio.msix` is in the
# archive, which is the only place a recipient can use it from: a package
# registers against the folder its `folio.exe` was extracted into, and a copy
# downloaded on its own names a folder with nothing at it.
Remove-Item -LiteralPath $work -Recurse -Force

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
# Written with LF and a final newline by hand: `Set-Content` on Windows ends every
# line with CRLF, and `sha256sum -c` on Linux and in WSL then looks for a file
# whose name ends in a carriage return and finds none (0.2.5 and 0.3.0 shipped so).
[System.IO.File]::WriteAllText($sums, (($lines -join "`n") + "`n"), [System.Text.Encoding]::ASCII)
Write-Host ''
$lines | Write-Host
