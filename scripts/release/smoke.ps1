<#
.SYNOPSIS
    Start the built `folio.exe` for real and check the seven things a build can
    be green and still be broken about.

.DESCRIPTION
    Every other gate in this repository runs inside `cargo test`, which means it
    runs against a library and never against the executable that ships. This one
    runs the artefact:

      1. `--version` answers on the caller's own stdout — captured through a
         pipe, which is the check that separates "it printed something" from
         "a script can read what it printed" — and exits 0.
      2. `--help` exits 0, and a flag this build has not got exits 2 and names
         itself. A packaging script reads those two numbers.
      3. A cold launch opens a window, builds a device, spawns a shell and gets
         text onto the glass. Read out of `BT_STARTUP_TRACE` rather than
         asserted with a screenshot comparison, because each phase is named
         separately and a failure says which one.
      4. The ConPTY it spawned came from the sidecar beside the exe and not from
         the system — the one fact the archive's whole file list exists to make
         true.
      5. The window agrees with Windows about its own DPI: `GetDpiForWindow` on
         the window Folio opened equals `GetDpiForMonitor` for the monitor it
         opened on. Stated as an agreement rather than as "96", so the same
         check is the 200% check on a machine set to 200%.
      6. It shuts down when asked, rather than being killed.
      7. The file a bug report actually arrives as — `diagnostics.log`, written
         by an ordinary run with no trace variable set — opens with the same
         build line `--version` printed, and says whether this build may update
         itself (`updater on` / `updater off`), which `-Updater` holds it to.

    `-ExpectSigned` adds a check before all seven, over the two artefacts rather
    than over anything running: the executable's signature, and the sparse
    package beside it that carries the same publisher and the same certificate.

    **And the release archive, when there is one, is checked against the
    manifest its own `folio.exe` carries** (0.4.6 ticket U-9): every member but
    `folio.exe` and `folio.msix`, by name, size and SHA-256, and nothing in the
    archive that the manifest does not list. That is before anything runs too —
    the manifest is read out of the executable's resources, never by starting
    it — and `-ArchiveOnly` stops there, which is how the archive is checked on
    a machine where a window may not be opened.

    A picture of the window and every trace file are written to `-Artifacts`
    whatever happens, because the CI run that fails is the one nobody can
    reproduce.

    **The run is sealed off from the machine's own Folio**: `APPDATA` and `TEMP`
    are pointed at a scratch directory, so this reads no settings, restores no
    session and leaves nothing behind.

.PARAMETER Exe
    The `folio.exe` to run. Defaults to `target/release/folio.exe`.

.PARAMETER Artifacts
    Where traces and the screenshot go. Defaults to `target/smoke`.

.PARAMETER TimeoutSeconds
    How long the cold launch has to reach its last phase.

.PARAMETER ExpectSigned
    Also check, before starting anything, that this executable carries a valid,
    time-stamped Authenticode signature naming the holder it says it is
    copyright of, and that `folio.msix` is signed by the same certificate and
    declares that certificate's subject as its `Publisher`. Pass it when the
    artefacts under test came out of `package.ps1 -Sign`; leave it off
    otherwise, because an unsigned build is what an ordinary `cargo build`
    produces and this script has to keep working on one.

.PARAMETER SignerSubject
    Who the certificate has to name, if not the holder named in the executable's
    own `LegalCopyright`. Only read when `-ExpectSigned` is given.

.PARAMETER Msix
    Where the sparse package to check under `-ExpectSigned` is. **Not needed on
    either machine the release is checked on**: the default is `folio.msix`
    beside `-Exe` when there is one — which is where it is once somebody extracts
    the archive, both files in one folder, the package naming the executable at
    the folder it was extracted into — and otherwise the release archive in
    `-PackageDirectory`.

    That second half is the release machine. `package.ps1` leaves the archive,
    the bill of materials and `SHA256SUMS.txt` in `target/release-package` and no
    loose `folio.msix` anywhere, because a package downloaded on its own names a
    folder with no `folio.exe` in it — so a default that only ever looked beside
    the executable named a path that cannot exist on the machine that made the
    release. With neither there, `-ExpectSigned` refuses and names both places.

    **It can be named in the archive rather than as a file of its own**, which is
    what the default does and what `-Msix` may be handed. Given an archive, this
    takes the package out of it into `-Artifacts` and checks that copy, which is
    byte for byte the one a recipient registers. Which of the two a path is is
    settled by opening it rather than by its name: an msix is a zip too, so the
    name could not settle it.

.PARAMETER Updater
    `on` or `off`: what the executable has to say about whether it may update
    itself (`FOLIO_UPDATER`, 0.4.6 ticket U-8), read from the header of the
    `diagnostics.log` check 7 already reads. Only a release build invocation
    sets the flag, so a development or CI build is held to `off` and a release
    to `on`.

    **`-ExpectSigned` alone means `on`**, because a signed release is what that
    switch is for. A signed build that is not a release — a candidate — is
    smoked with `-ExpectSigned -Updater off`, which says so out loud. Neither
    given, the answer is printed and not held to anything: the clean-machine
    run starts whatever archive it was handed.

.PARAMETER Archive
    The release archive to check against the manifest its `folio.exe` carries.
    Defaults to the one release archive in `-PackageDirectory` — the versioned
    one, or the copy under the stable name when that is all there is. With
    neither there the check is skipped and says so, because an ordinary build
    has no archive and is not meant to; `-ArchiveOnly` refuses instead.

.PARAMETER ArchiveOnly
    Check the arguments, the signatures under `-ExpectSigned` and the archive
    against its manifest, and stop: **nothing is started**, no `--version`, no
    window. For the machine where a window may not be opened, and for
    `smoke-tests.ps1` and `package-tests.ps1`, which run this script's archive
    check against archives they build. Refused when there is no archive to
    check.

.PARAMETER PackageDirectory
    The directory `package.ps1` writes the release page into. Defaults to
    `target/release-package`, which is `package.ps1 -Output`'s own default.

    Nothing is required to be in it — an ordinary build has never filled it —
    but whatever is in it may not belong to another release. See the door check
    below.
#>

[CmdletBinding()]
param(
    [string] $Exe,
    [string] $Artifacts,
    [int] $TimeoutSeconds = 90,
    [switch] $ExpectSigned,
    [string] $SignerSubject,
    [string] $Msix,
    [string] $PackageDirectory,
    [ValidateSet('on', 'off')]
    [string] $Updater,
    [string] $Archive,
    [switch] $ArchiveOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# **Where this file is, spelled the way both editions of PowerShell read.** The
# machine this script has to run on last is a clean Windows, and a clean Windows
# has Windows PowerShell 5.1 and nothing else: `Join-Path` there takes two paths
# and refuses a third, so `Join-Path $PSScriptRoot '..' '..'` is a parameter
# binding error raised before the first line of work — which is how gate 5's
# first smoke died, with `A positional parameter cannot be found that accepts
# argument '..'`. Nested two-argument calls are the one spelling 5.1 and 7 read
# the same way.
#
# The root is asserted rather than allowed to be empty, too: an empty
# `$PSScriptRoot` turns every path below into a relative one, and the failure
# then arrives later and somewhere else, as a file that is "not at
# \scripts\release\smoke.ps1".
$here = $PSScriptRoot
if (-not $here -and $PSCommandPath) { $here = Split-Path -Parent $PSCommandPath }
if (-not $here) {
    throw 'smoke.ps1 cannot tell where it is; run it as a file (-File, or &), not from a pasted body'
}
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path
# The archive's member list and the manifest `folio.exe` carries of it.
. (Join-Path $here 'release-manifest.ps1')

# **Every path this script was handed is made absolute here, before anything
# reads it.** A relative path has two answers on Windows and they are allowed to
# differ: PowerShell resolves one against `$PWD`, and .NET resolves it against
# the *process's* current directory, which `Set-Location` does not move. A shell
# started in one checkout and pointed at another therefore has `Test-Path` find
# a file that `[System.IO.Compression.ZipFile]::OpenRead` two hundred lines
# later cannot, and the failure names a folder nobody typed. That is exactly how
# a `-Msix` naming something under `target\release-package` — the line
# `docs/RELEASING.md` tells people to run — failed on the 0.2.1 packaging run,
# in a worktree, against the main checkout's path. Resolving once, at the door,
# is the fix that holds for every reader below rather than for the ones somebody
# remembered.
#
# `GetUnresolvedProviderPathFromPSPath` is the resolution `$PWD` implies and it
# answers for a path that does not exist yet — which `$Artifacts` does not, on
# the first run, and which a mistyped `-Exe` never will.
function Resolve-GivenPath {
    param([string] $Path)
    return $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

if (-not $Exe) { $Exe = Join-Path $root 'target\release\folio.exe' }
if (-not $Artifacts) { $Artifacts = Join-Path $root 'target\smoke' }
$Exe = Resolve-GivenPath $Exe
$Artifacts = Resolve-GivenPath $Artifacts
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "no folio.exe at $Exe" }

[System.IO.Directory]::CreateDirectory($Artifacts) | Out-Null

# **What a path names is settled by opening it, not by what it is called.** The
# package ships inside the archive and nowhere else, so `-Msix` is given the
# archive on the machine that packed it and the package itself on a machine
# that has extracted one. An msix is a zip and so is the archive, and both are
# called something ending in a name this script chose, so the only honest
# question is which one holds an `AppxManifest.xml` at its root.
#
# What comes back is always a file on disk: `Get-AuthenticodeSignature` reads a
# file, not an entry in an archive, so a package that arrived in one is written
# out under `-Artifacts` first. Those are the same bytes — an entry is copied
# out of the archive, not repacked — so the signature and the manifest read the
# same as they will on the machine that extracts the release.
function Resolve-PackageFile {
    param([string] $Path, [string] $Into)

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    try { $archive = [System.IO.Compression.ZipFile]::OpenRead($Path) }
    catch {
        throw ("$Path is neither a sparse package nor an archive holding one: it does not open " +
               'as a zip at all.')
    }
    try {
        if ($archive.GetEntry('AppxManifest.xml')) { return $Path }

        $inside = @($archive.Entries | Where-Object { $_.Name -eq 'folio.msix' })
        if ($inside.Count -eq 0) {
            throw ("$Path holds no AppxManifest.xml and no folio.msix, so it is neither the " +
                   'package nor the archive the package ships in.')
        }
        if ($inside.Count -gt 1) {
            throw "$Path holds $($inside.Count) files called folio.msix, and which one is meant is not a guess to make"
        }

        if (Test-Path -LiteralPath $Into) { Remove-Item -LiteralPath $Into -Recurse -Force }
        [System.IO.Directory]::CreateDirectory($Into) | Out-Null
        $extracted = Join-Path $Into 'folio.msix'
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($inside[0], $extracted, $true)
        Write-Host "package: $($inside[0].FullName), out of $([IO.Path]::GetFileName($Path))"
        return $extracted
    }
    finally { $archive.Dispose() }
}

if (-not $PackageDirectory) { $PackageDirectory = Join-Path $root 'target\release-package' }
$PackageDirectory = Resolve-GivenPath $PackageDirectory

if ($Msix) {
    # **A path somebody named has to be there, and is said so at the door.**
    # Naming `-Msix` is asking for that file; whether the checks that read it
    # are switched on is a separate question, and a named path that is quietly
    # carried past this line is one that surfaces much later as a reader failing
    # on a folder the operator never wrote.
    $Msix = Resolve-GivenPath $Msix
    if (-not (Test-Path -LiteralPath $Msix -PathType Leaf)) {
        throw "-Msix names $Msix, and there is no file there"
    }
    # Read at the door, with everything else about the arguments, so that a path
    # naming an archive with nothing usable in it is refused here rather than
    # after a window has been opened.
    $Msix = Resolve-PackageFile -Path $Msix -Into (Join-Path $Artifacts 'package')
}
else {
    # **The default is both arrangements the package is ever in**, because there
    # are exactly two and which one you are standing in is a property of the
    # machine rather than of the command.
    #
    # Beside the executable is the recipient's: one folder holding `folio.exe`
    # and `folio.msix`, which is what extracting the archive produces, and it is
    # resolved from `-Exe` rather than from `$root` for that reason. On the
    # machine that *packed* the release there is no loose copy at all —
    # `package.ps1` packs the msix in a working directory it takes away again,
    # because a package downloaded on its own names a folder with no `folio.exe`
    # in it — so the package is in the archive in `-PackageDirectory` and nowhere
    # else. That is the machine `docs/RELEASING.md`'s signed line is typed on,
    # and defaulting to a path that cannot exist there made `-Msix` a flag the
    # release had to be told about twice: once in the document and once by a
    # failure. The archive is opened rather than read by name, by the same
    # `Resolve-PackageFile` a named `-Msix` goes through, so the bytes checked
    # are the bytes a recipient registers either way.
    #
    # Nothing is asked of either place here — an ordinary unsigned build has no
    # package anywhere and is not meant to. `-ExpectSigned` is what needs one,
    # and its refusal below names both of these.
    # Where it looked, in the order it looked, so that a refusal names both
    # places rather than the one this script happened to prefer.
    $beside = Join-Path (Split-Path -Parent $Exe) 'folio.msix'
    $msixLookedIn = @($beside)
    if (Test-Path -LiteralPath $beside -PathType Leaf) {
        $Msix = $beside
    }
    else {
        # The versioned archive is the release's asset; `folio-windows-x64.zip`
        # beside it is the same bytes under the name `/releases/latest/download/`
        # resolves, so either answers and the versioned one is preferred for
        # being the one a person names. Two versioned archives is the directory
        # holding two releases, which is the door below's question and not a
        # guess to make here.
        $msixLookedIn += $PackageDirectory
        $archives = @()
        $stable = Join-Path $PackageDirectory 'folio-windows-x64.zip'
        if (Test-Path -LiteralPath $PackageDirectory -PathType Container) {
            $archives = @(Get-ChildItem -LiteralPath $PackageDirectory -File -Filter 'folio-*-windows-x64.zip')
        }
        if ($archives.Count -gt 1) {
            throw ("$PackageDirectory holds $($archives.Count) release archives — " +
                   "$(($archives | ForEach-Object { $_.Name }) -join ', ') — so which one holds " +
                   'the package meant here is not a guess to make. Name one with -Msix.')
        }
        if ($archives.Count -eq 1) {
            $Msix = Resolve-PackageFile -Path $archives[0].FullName -Into (Join-Path $Artifacts 'package')
        }
        elseif (Test-Path -LiteralPath $stable -PathType Leaf) {
            $Msix = Resolve-PackageFile -Path $stable -Into (Join-Path $Artifacts 'package')
        }
        else {
            # **Neither place has one**, and under `-ExpectSigned` that is a
            # release with a file missing rather than a check that does not
            # apply. Refused here, at the door, with everything else about the
            # arguments and before a window has been opened — which is where a
            # `-Msix` naming nothing is refused too, so there is one place that
            # answers "which file is the package, and is there one".
            #
            # Without `-ExpectSigned` nothing is asked of it at all: an ordinary
            # unsigned build has no package anywhere and is not meant to. The
            # path kept is the recipient's, because that is the arrangement the
            # signed check describes.
            $Msix = $beside
            if ($ExpectSigned) {
                Write-Host 'no folio.msix was found. These are the places this looked:'
                $msixLookedIn | ForEach-Object { Write-Host "  $_" }
                throw ('there is no folio.msix beside the executable and no release archive ' +
                       'holding one in the package directory. It is what puts "Open in Folio" on ' +
                       'the right-click menu, and it ships in the archive beside the executable, ' +
                       'so a signed release without one is a release missing a file rather than a ' +
                       'check that does not apply. Name the package, or the archive it is in, ' +
                       'with -Msix if it is somewhere else.')
            }
        }
    }
}

# ── the door, last: the package directory holds one release and not two ──────
#
# **The release page is a directory listing.** `docs/RELEASING.md` hands
# `gh release create` everything in `target/release-package`, so a file left
# there by an earlier release is an asset of this one. `package.ps1` empties the
# directory before it writes into it, which settles what it put there; what it
# cannot settle is what arrives afterwards — the macOS image and its checksum
# file are fetched from the Mac into the same directory, and the copy that gets
# fetched is the copy somebody typed a path to.
#
# So this is asked of the directory rather than of any script's memory of it:
# every file whose name carries a version must carry this one. The version is
# read out of `Cargo.toml`, the same single source `package.ps1` reads it from,
# so the two cannot disagree about which release is being made. A file with no
# version in its name says nothing either way and is left alone: the two
# checksum files, and the two copies the release page carries under a name that
# is the same in every release — `folio-windows-x64.zip` and
# `Folio-macos-arm64.dmg`, which are what `/releases/latest/download/` resolves
# by. That is not an exemption written for those four. A name carrying no
# version cannot name another release, which is the whole of what this asks.
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

if (Test-Path -LiteralPath $PackageDirectory -PathType Container) {
    # The three numbers, which is what a file name can carry: `package.ps1`
    # compares `folio.exe`'s `VERSIONINFO` against the same `[-+]`-stripped
    # version, because a pre-release suffix is not part of what a `VERSIONINFO`
    # or a `\d+\.\d+\.\d+` in a file name can say.
    $releasing = ((Get-WorkspaceVersion) -split '[-+]')[0]
    $strangers = @(
        foreach ($file in (Get-ChildItem -LiteralPath $PackageDirectory -File)) {
            if (($file.Name -match '\d+\.\d+\.\d+') -and ($Matches[0] -ne $releasing)) { $file.Name }
        }
    )
    if ($strangers.Count -gt 0) {
        throw ("$PackageDirectory is the release page and this release is $releasing, but it " +
               "also holds $($strangers -join ', '). Run package.ps1, which empties it, and " +
               'fetch the macOS assets again afterwards.')
    }
}

# ── the archive, against the manifest its executable carries ────────────────
#
# **What `package.ps1` refused to pack is checked again on what it packed.** The
# archive is the artefact: the manifest inside its `folio.exe` names every other
# member's size and SHA-256 (`FOLIO_RELEASE_MANIFEST`, 0.4.6 ticket U-9), and the
# executable's signature is what vouches for them. So the check is made of the
# archive's own entries — each read out of the zip and hashed, nothing
# extracted but the executable whose resources are read — and a member missing,
# a member added, or a byte changed after packing is refused here with its name,
# before anything is started.
#
# The manifest is read from the `folio.exe` *in the archive*, which is the copy a
# recipient gets; `-Exe` may be the same bytes or not, and is not what this is
# about. Which two names the manifest leaves out is `archive-members.txt`'s
# answer, the list `package.ps1` packed from.
function Find-ReleaseArchive {
    if ($Archive) {
        $path = Resolve-GivenPath $Archive
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "-Archive names $path, and there is no file there" }
        return $path
    }
    if (-not (Test-Path -LiteralPath $PackageDirectory -PathType Container)) { return $null }
    $versioned = @(Get-ChildItem -LiteralPath $PackageDirectory -File -Filter 'folio-*-windows-x64.zip')
    if ($versioned.Count -gt 1) {
        throw ("$PackageDirectory holds $($versioned.Count) release archives — " +
               "$(($versioned | ForEach-Object { $_.Name }) -join ', ') — so which one to check is not " +
               'a guess to make. Name one with -Archive.')
    }
    if ($versioned.Count -eq 1) { return $versioned[0].FullName }
    $stable = Join-Path $PackageDirectory 'folio-windows-x64.zip'
    if (Test-Path -LiteralPath $stable -PathType Leaf) { return $stable }
    return $null
}

function Assert-ArchiveMatchesManifest {
    param([string] $Path, [string] $Into)

    $listed = Get-ArchiveMemberList
    $exe = @($listed | Where-Object { $_.Source -ceq 'exe' })[0].Name
    $exempt = @($listed | Where-Object { -not $_.InManifest } | ForEach-Object { $_.Name })

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -notmatch '/$' })
        $carriers = @($entries | Where-Object { $_.FullName -cmatch "^[^/]+/$([regex]::Escape($exe))$" })
        if ($carriers.Count -ne 1) {
            throw "$Path holds $($carriers.Count) $exe at its root folder; the manifest is read from exactly one"
        }
        if (Test-Path -LiteralPath $Into) { Remove-Item -LiteralPath $Into -Recurse -Force }
        [System.IO.Directory]::CreateDirectory($Into) | Out-Null
        $carrier = Join-Path $Into $exe
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($carriers[0], $carrier, $true)
        $release = Read-ReleaseManifest -Exe $carrier

        $problems = New-Object System.Collections.Generic.List[string]
        $prefix = "$($release.ArchiveRoot)/"
        $found = @(
            foreach ($entry in $entries) {
                if (-not $entry.FullName.StartsWith($prefix, [StringComparison]::Ordinal)) {
                    $problems.Add("outside   : $($entry.FullName) is not under $prefix, the manifest's root")
                    continue
                }
                $stream = $entry.Open()
                try { $hash = Get-StreamSha256 -Stream $stream } finally { $stream.Dispose() }
                [pscustomobject]@{ Name = $entry.FullName.Substring($prefix.Length); Size = $entry.Length; Sha256 = $hash }
            }
        )
        foreach ($problem in (Compare-ReleaseMembers -Manifest $release -Found $found -Exempt $exempt)) {
            $problems.Add($problem)
        }
    }
    finally { $zip.Dispose() }

    if ($problems.Count -gt 0) {
        Write-Host "$([IO.Path]::GetFileName($Path)) does not match the release manifest its $exe carries:"
        $problems | ForEach-Object { Write-Host "  $_" }
        throw "$($problems.Count) member(s) of $Path differ from the manifest in its $exe"
    }
    Write-Host ("archive: $([IO.Path]::GetFileName($Path)) — $($release.Members.Count) members match the " +
                "manifest in its $exe (protocol $($release.Protocol), min updater $($release.MinUpdater))")
}

Add-Type -Namespace Smoke -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int index);
[DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int t, out uint x, out uint y);
public delegate bool EnumWindowsProc(IntPtr h, IntPtr p);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

# Every probe process in this repository declares per-monitor v2 before it asks
# a question about pixels. A probe left at the default awareness is handed
# virtualised coordinates by Windows and reports them confidently.
[void][Smoke.Win32]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))

# **Folio's window is not the only top-level window its process owns.** winit
# keeps a permanently visible, unowned 13 x 13 message window of class `Winit
# Thread Event Target`, created seconds before the real one. A search for
# "visible, unowned" alone finds whichever of the two is higher in the Z-order,
# which is a coin this script has been winning rather than a rule it follows.
# The rule is the one Windows uses for the Alt-Tab list: an application window
# has no `WS_EX_TOOLWINDOW`. winit's event target has it; Folio's window has
# `WS_EX_APPWINDOW` instead.
$GwlExStyle = -20
$WsExToolWindow = 0x00000080

function Invoke-Folio {
    param([string[]] $Arguments)

    $out = Join-Path $Artifacts ('cli-' + ($Arguments -join '_' -replace '[^\w.-]', '') + '.out')
    $err = "$out.err"
    $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError $err
    # **`-Encoding UTF8` on every read of something Folio wrote.** Folio writes
    # UTF-8 without a byte-order mark; PowerShell 7 assumes that and Windows
    # PowerShell 5.1 assumes the machine's ANSI code page, so the same file read
    # by the same line comes back as mojibake on the clean machine. It was the
    # `diagnostics.log` header, quoted into the evidence transcript as
    # `â”€â”€ Folio 0.1.0`, that showed it.
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        StdOut   = (Get-Content -LiteralPath $out -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
        StdErr   = (Get-Content -LiteralPath $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
    }
}

$expectedVersion = (Get-Item -LiteralPath $Exe).VersionInfo.ProductVersion.Trim()

# ── 0: it is signed, and signed by whoever it says it belongs to ─────────────
#
# **Asked of the file rather than of a script's memory of what it signed.** The
# name to expect is read out of the executable's own `LegalCopyright` — the
# holder the two licences and `crates/bt-app/build.rs` already agree on — so this
# check has no second copy of that name to drift away from, and a certificate
# issued to somebody else fails it whatever the signing script believed.
#
# The time stamp is checked as hard as the signature: an Artifact Signing
# certificate is valid for three days, and a signature made without `/tr` passes
# every check for three days and then stops passing them on a machine nobody here
# is sitting at.
if ($ExpectSigned) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Exe
    if ($signature.Status -ne 'Valid') {
        throw "the signature on $Exe is $($signature.Status): $($signature.StatusMessage)"
    }
    if (-not $signature.TimeStamperCertificate) {
        throw 'the signature carries no time stamp, so it expires with the certificate that made it'
    }

    $holder = $SignerSubject
    if (-not $holder) {
        $copyright = (Get-Item -LiteralPath $Exe).VersionInfo.LegalCopyright
        if ($copyright -notmatch '(?i)copyright\s*\(c\)\s*\d{4}\s+(.+?)\s+and\b') {
            throw "the executable's LegalCopyright is '$copyright'; no holder can be read out of it"
        }
        $holder = $Matches[1]
    }
    $subject = $signature.SignerCertificate.Subject
    if ($subject -notlike "*$holder*") {
        throw "the certificate says $subject; the executable says it is copyright $holder"
    }
    Write-Host "signed by: $subject"
    Write-Host "stamped by: $($signature.TimeStamperCertificate.Subject)"
}

# ── 0, continued: the package beside it names the same publisher ─────────────
#
# **The failure this catches is invisible everywhere else.** Windows compares the
# `Publisher` in a package's `<Identity>` against the subject of the certificate
# the package was signed with, at the moment somebody registers it — and it
# refuses a mismatch with a message that names neither of the two strings it
# compared. Nothing in the build fails. Nothing in a smoke test that only starts
# the executable fails. The first person to turn the Explorer menu row on gets a
# registration error about a package they cannot inspect, on a machine nobody
# here is sitting at. It costs one file read to know before the release leaves.
#
# It is asked of the package that shipped rather than of `packaging/msix/`: the
# file in the tree is an input, the copy in the package is what a user registers,
# and the version injected between them proves the two are not the same bytes.

# **A distinguished name is compared as a name and not as a string.**
# `CN=Weiyi Shi, O=Weiyi Shi` and `CN=Weiyi Shi,O=Weiyi Shi` are the same name
# written twice: the space after a separating comma is spelling and not content,
# and which of the two a certificate authority, a Windows API and a person
# editing XML each produce is not something this check gets to decide. Compared
# as text they differ, and the failure sends somebody to edit a file that was
# already correct — which is worse than not checking at all, because it teaches
# people to distrust the check.
#
# So each side is cut into its relative names at the commas that separate them,
# and only at those: a comma inside a value is written `\,` or sits inside a
# quoted value, and neither of those separates anything. Attribute types are
# compared without case, because `cn` and `CN` are one attribute. Values are
# compared with it, because `CN=Weiyi Shi` and `CN=WEIYI SHI` are two things a
# certificate authority issued on purpose and this is not the place to decide
# they are one person. Order is kept, because the order of the relative names is
# part of the name.
function Split-DistinguishedName {
    param([string] $Name)

    $parts = @()
    $current = New-Object System.Text.StringBuilder
    $quoted = $false
    for ($i = 0; $i -lt $Name.Length; $i++) {
        $character = $Name[$i]
        # A backslash spells the next character literally. It is how a comma, a
        # plus or a quotation mark appears inside a value, and the character it
        # protects is kept while the backslash itself is not — the two sides are
        # then comparable however each of them chose to write it.
        if ($character -eq '\' -and $i + 1 -lt $Name.Length) {
            [void] $current.Append($Name[$i + 1])
            $i++
            continue
        }
        if ($character -eq '"') { $quoted = -not $quoted; continue }
        if ($character -eq ',' -and -not $quoted) {
            $parts += $current.ToString()
            [void] $current.Clear()
            continue
        }
        [void] $current.Append($character)
    }
    $parts += $current.ToString()

    return @($parts | ForEach-Object {
        $rdn = $_.Trim()
        $equals = $rdn.IndexOf('=')
        if ($equals -lt 0) { return $rdn }
        '{0}={1}' -f $rdn.Substring(0, $equals).Trim().ToUpperInvariant(), $rdn.Substring($equals + 1).Trim()
    })
}

function Test-SameDistinguishedName {
    param([string] $Left, [string] $Right)

    # Named apart from the parameters on purpose: PowerShell's variable names do
    # not distinguish case, so a `$left` here would be the `$Left` above, and the
    # second line would be splitting an array it had already replaced.
    $first = Split-DistinguishedName -Name $Left
    $second = Split-DistinguishedName -Name $Right
    if ($first.Count -ne $second.Count) { return $false }
    for ($i = 0; $i -lt $first.Count; $i++) {
        if ($first[$i] -cne $second[$i]) { return $false }
    }
    return $true
}

if ($ExpectSigned) {
    # **That there is a package here at all was settled at the door**, for
    # either road it came down: a `-Msix` somebody typed and a default that
    # looked in the two places the package is ever in are both refused up there,
    # by the block that decides which file this is. A second existence check
    # here would be a second owner of that question and could only ever
    # disagree with the first.

    # The package's own signature, held to what the executable's is held to. A
    # package signed without a time stamp registers for three days and then
    # stops, and it stops on somebody else's machine.
    $packageSignature = Get-AuthenticodeSignature -LiteralPath $Msix
    if ($packageSignature.Status -ne 'Valid') {
        throw "the signature on $Msix is $($packageSignature.Status): $($packageSignature.StatusMessage)"
    }
    if (-not $packageSignature.TimeStamperCertificate) {
        throw 'the package signature carries no time stamp, so it stops verifying with the certificate that made it'
    }

    # **An msix is a zip, and `MakeAppx` leaves `AppxManifest.xml` in it as plain
    # XML.** Read that way rather than through the packaging API, because this
    # has to run on a clean Windows with nothing installed on it and because the
    # question — what does one attribute in one file say — does not need a
    # package reader to answer.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $package = [System.IO.Compression.ZipFile]::OpenRead($Msix)
    try {
        $entry = $package.GetEntry('AppxManifest.xml')
        if (-not $entry) { throw "$Msix holds no AppxManifest.xml; it is not a package" }
        $reader = New-Object System.IO.StreamReader($entry.Open())
        try { $manifestText = $reader.ReadToEnd() } finally { $reader.Dispose() }
    }
    finally { $package.Dispose() }

    $publisher = ([xml] $manifestText).Package.Identity.Publisher
    $packageSubject = $packageSignature.SignerCertificate.Subject
    if (-not (Test-SameDistinguishedName -Left $publisher -Right $packageSubject)) {
        throw ("the package declares Publisher=""$publisher"" and is signed by ""$packageSubject"". " +
               'Windows compares exactly those two when the package is registered and refuses ' +
               'without naming either; packaging/msix/AppxManifest.xml has to carry the ' +
               'certificate subject character for character.')
    }

    # And the same certificate as the executable, which is what makes the two
    # files one release rather than two things that happen to be in one folder.
    if (-not (Test-SameDistinguishedName -Left $packageSubject -Right $subject)) {
        throw "folio.msix is signed by $packageSubject and folio.exe by $subject"
    }

    Write-Host "package publisher: $publisher"
    Write-Host "package stamped by: $($packageSignature.TimeStamperCertificate.Subject)"
}

# The archive check itself, after the signatures: both read files and start
# nothing, and a signature refused is the more basic answer.
$releaseArchive = Find-ReleaseArchive
if ($releaseArchive) {
    Assert-ArchiveMatchesManifest -Path $releaseArchive -Into (Join-Path $Artifacts 'manifest')
}
elseif ($ArchiveOnly) {
    throw ("-ArchiveOnly checks a release archive against its manifest, and there is none: no " +
           "-Archive, and no release archive in $PackageDirectory")
}
else {
    Write-Host "archive: none in $PackageDirectory, so the manifest check has nothing to read"
}

# **`-ArchiveOnly` ends here**, before the first process is started: everything
# above reads files, and everything below runs one.
if ($ArchiveOnly) {
    Write-Host 'archive only: nothing was started.'
    return
}

# ── 1 and 2: the front door ──────────────────────────────────────────────────

$version = Invoke-Folio @('--version')
if ($version.ExitCode -ne 0) { throw "--version exited $($version.ExitCode)" }
$line = ($version.StdOut | Out-String).Trim()
if (-not $line) {
    throw '--version wrote nothing a caller could capture (it went to the console screen, not to stdout)'
}
if ($line -notmatch "^Folio\s+$([regex]::Escape($expectedVersion))\s+\(") {
    throw "--version said '$line'; the executable's own resources say $expectedVersion"
}
Write-Host "--version: $line"

$help = Invoke-Folio @('--help')
if ($help.ExitCode -ne 0) { throw "--help exited $($help.ExitCode)" }
if (($help.StdOut | Out-String) -notmatch '--cwd') { throw '--help printed no usage block' }

$refused = Invoke-Folio @('--nope')
if ($refused.ExitCode -ne 2) { throw "an unknown flag exited $($refused.ExitCode), expected 2" }
if (($refused.StdOut | Out-String) -notmatch '--nope') { throw 'a refusal did not name the flag' }
Write-Host 'the front door answers --help and refuses what it has not got.'

# ── 3 and 4: a cold launch, in a home of its own ─────────────────────────────

$home_ = Join-Path $Artifacts 'home'
if (Test-Path -LiteralPath $home_) { Remove-Item -LiteralPath $home_ -Recurse -Force }
[System.IO.Directory]::CreateDirectory((Join-Path $home_ 'temp')) | Out-Null

$trace = Join-Path $Artifacts 'startup.trace'
$stdout = Join-Path $Artifacts 'startup.out'
$environment = @{
    APPDATA         = $home_
    TEMP            = (Join-Path $home_ 'temp')
    TMP             = (Join-Path $home_ 'temp')
    BT_STARTUP_TRACE = '1'
    # The project's own test windows always carry this, and here it is also the
    # artefact: what the shell actually said, for a failure that has to be
    # diagnosed from a log.
    BT_PTY_DUMP     = (Join-Path $Artifacts 'pty.dump')
}
foreach ($name in $environment.Keys) {
    Set-Item -Path "Env:$name" -Value $environment[$name]
}

$folio = Start-Process -FilePath $Exe -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $trace
# **Touching `.Handle` is what makes `.ExitCode` answerable later.** Windows
# PowerShell 5.1 hands back a `Process` for a redirected `-PassThru` start that
# has never opened the process handle, and a handle first asked for after the
# process has gone cannot be had: `.ExitCode` then answers `$null` for the rest
# of the run, and check 6 below reads "folio exited " with nothing after it.
# PowerShell 7 caches it on its own; asking here costs nothing there.
[void] $folio.Handle

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$window = [IntPtr]::Zero
$seen = ''
while ((Get-Date) -lt $deadline) {
    if ($folio.HasExited) { break }
    $seen = (Get-Content -LiteralPath $trace -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
    if ($seen -and $seen -match 'BT_STARTUP first_text_present=') { break }
    Start-Sleep -Milliseconds 250
}

# The picture and the window facts, while it is still up.
[Smoke.Win32]::EnumWindows({
        param([IntPtr] $handle, [IntPtr] $unused)
        $pid_ = 0
        [void][Smoke.Win32]::GetWindowThreadProcessId($handle, [ref] $pid_)
        if ($pid_ -eq $folio.Id -and [Smoke.Win32]::IsWindowVisible($handle) -and
            [Smoke.Win32]::GetWindow($handle, 4) -eq [IntPtr]::Zero -and
            ([Smoke.Win32]::GetWindowLongW($handle, $script:GwlExStyle) -band $script:WsExToolWindow) -eq 0) {
            $script:window = $handle
            return $false
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null

# **Bringing a window to the front from a process that is not in front.**
# Windows refuses a bare `SetForegroundWindow` from a process that does not own
# the foreground, and answers `false` rather than raising: the capture below
# then photographs the rectangle where the window is, showing whatever sits on
# top of it. Joining the foreground thread's input queue for the length of the
# call is the documented way round it, and the result is checked rather than
# assumed — a picture of the wrong window is worse than no picture.
function Set-WindowInFront {
    param([IntPtr] $Window)

    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        $foreground = [Smoke.Win32]::GetForegroundWindow()
        if ($foreground -eq $Window) { return $true }

        $owner = 0
        $theirs = [Smoke.Win32]::GetWindowThreadProcessId($foreground, [ref] $owner)
        $mine = [Smoke.Win32]::GetCurrentThreadId()
        $attached = $false
        if ($theirs -ne 0 -and $theirs -ne $mine) {
            $attached = [Smoke.Win32]::AttachThreadInput($mine, $theirs, $true)
        }
        [void][Smoke.Win32]::BringWindowToTop($Window)
        [void][Smoke.Win32]::SetForegroundWindow($Window)
        if ($attached) { [void][Smoke.Win32]::AttachThreadInput($mine, $theirs, $false) }

        Start-Sleep -Milliseconds 400
        if ([Smoke.Win32]::GetForegroundWindow() -eq $Window) { return $true }
    }
    return $false
}

$picture = Join-Path $Artifacts 'window.png'
if ($window -ne [IntPtr]::Zero) {
    if (-not (Set-WindowInFront -Window $window)) {
        Write-Host 'WARNING: window.png was taken while the window was not in front; read it before believing it'
    }
    Start-Sleep -Milliseconds 600
    $rect = New-Object Smoke.Win32+RECT
    [void][Smoke.Win32]::GetWindowRect($window, [ref] $rect)
    Add-Type -AssemblyName System.Drawing
    $bitmap = New-Object System.Drawing.Bitmap(
        ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    # Off the screen and not `PrintWindow`: this window's pixels are composed by
    # the GPU into a swap chain, and a window that is asked to paint itself into
    # a device context hands back the blank one it never draws into.
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($picture, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
    Write-Host "window picture: $picture"
}

# **Asked while the window is alive.** `GetDpiForWindow` answers `0` for a
# handle that has gone, so a check made after the shutdown below would be two
# zeroes agreeing with each other for the rest of this product's life.
$windowDpi = 0
$monitorDpi = 0
if ($window -ne [IntPtr]::Zero) {
    $windowDpi = [Smoke.Win32]::GetDpiForWindow($window)
    $monitor = [Smoke.Win32]::MonitorFromWindow($window, 2) # MONITOR_DEFAULTTONEAREST
    $y = 0
    [void][Smoke.Win32]::GetDpiForMonitor($monitor, 0, [ref] $monitorDpi, [ref] $y) # MDT_EFFECTIVE_DPI
}

# Shut it the way a person does, and give it the time a clean quit takes.
if (-not $folio.HasExited -and $window -ne [IntPtr]::Zero) {
    [void][Smoke.Win32]::PostMessageW($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
}
$closed = $folio.WaitForExit(20000)

$seen = (Get-Content -LiteralPath $trace -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
if (-not $seen) { throw "the launch wrote no startup trace; see $trace" }
Write-Host $seen

foreach ($phase in @('runtime_ready=', 'background_visible=', 'first_text_present=')) {
    if ($seen -notmatch [regex]::Escape($phase)) {
        throw "the launch never reached $phase — see $trace"
    }
}
# `source=sidecar`, and never `source=system`: falling back to the inbox ConPTY
# is precisely the failure the archive's file list exists to prevent, and it is
# invisible — the shell starts either way.
if ($seen -notmatch 'conpty_sources=\["source=sidecar') {
    throw "the shell did not start on the sidecar ConPTY beside the exe — see $trace"
}
Write-Host 'a cold launch reached first text, on the sidecar ConPTY.'

# ── 5: the window and Windows agree about the DPI ────────────────────────────

if ($window -eq [IntPtr]::Zero) { throw 'the launch opened no visible top-level window' }
# Zero is what a dead handle and an unaware process both answer, so it is
# refused before the comparison rather than compared.
if ($windowDpi -le 0 -or $monitorDpi -le 0) {
    throw "no dpi was read: window $windowDpi, monitor $monitorDpi"
}
if ($windowDpi -ne $monitorDpi) {
    throw "the window reports $windowDpi dpi on a monitor Windows calls $monitorDpi"
}
Write-Host ("dpi: window and monitor agree at {0} ({1:P0} scaling)." -f $windowDpi, ($windowDpi / 96))

# ── 6 ────────────────────────────────────────────────────────────────────────

if (-not $closed) { throw 'the window did not close when it was asked to' }
if ($folio.ExitCode -ne 0) { throw "folio exited $($folio.ExitCode)" }
Write-Host 'it closed when asked, and exited 0.'

# ── 7: the log an ordinary run leaves says which build left it ───────────────
#
# A second launch, and a second one is the only way: `BT_STARTUP_TRACE` keeps
# the console, which is exactly the case in which `diagnostics.log` is not the
# destination. The run above proved the phases; this one proves the file a bug
# report will actually arrive as.

Remove-Item Env:BT_STARTUP_TRACE
# The traced run above kept its console, and a console run writes exactly one
# kind of line into `diagnostics.log`: its watchdog's, when a cold first turn on
# a slow machine runs past the hang threshold. That line is that run's and not
# this one's, so the file is taken away before the ordinary run is asked to
# leave its own.
$log = Join-Path $home_ 'Folio\diagnostics.log'
Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue
$plain = Start-Process -FilePath $Exe -PassThru `
    -RedirectStandardOutput (Join-Path $Artifacts 'plain.out') `
    -RedirectStandardError (Join-Path $Artifacts 'plain.err')
[void] $plain.Handle

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline -and -not (Test-Path -LiteralPath $log)) {
    if ($plain.HasExited) { break }
    Start-Sleep -Milliseconds 250
}
$second = [IntPtr]::Zero
[Smoke.Win32]::EnumWindows({
        param([IntPtr] $handle, [IntPtr] $unused)
        $pid_ = 0
        [void][Smoke.Win32]::GetWindowThreadProcessId($handle, [ref] $pid_)
        if ($pid_ -eq $plain.Id -and [Smoke.Win32]::IsWindowVisible($handle) -and
            [Smoke.Win32]::GetWindow($handle, 4) -eq [IntPtr]::Zero -and
            ([Smoke.Win32]::GetWindowLongW($handle, $script:GwlExStyle) -band $script:WsExToolWindow) -eq 0) {
            $script:second = $handle
            return $false
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null
if ($second -ne [IntPtr]::Zero) {
    [void][Smoke.Win32]::PostMessageW($second, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
}
[void]$plain.WaitForExit(20000)
if (-not $plain.HasExited) { $plain.Kill() }

if (-not (Test-Path -LiteralPath $log)) { throw "no diagnostics.log under $home_" }
Copy-Item -LiteralPath $log -Destination (Join-Path $Artifacts 'diagnostics.log') -Force
$header = (Get-Content -LiteralPath $log -TotalCount 1 -Encoding UTF8)
if (-not ($header.Contains($line) -and $header.Contains('run started'))) {
    throw "diagnostics.log opens with '$header'; expected the same build line --version printed"
}
Write-Host "diagnostics.log: $header"

# ── 7, continued: whether this build may update itself ───────────────────────
#
# A build fact, like the line above, and read from the same header rather than
# from `--version`: three release scripts compare `--version` byte for byte, and
# this is the line an ordinary run already writes. The flag is set by the release
# build invocation and nothing else, so what is required is the caller's word for
# which of the two this artefact is — `-Updater`, or `on` under `-ExpectSigned`.
if ($header -notmatch ', updater (on|off) ──$') {
    throw "diagnostics.log opens with '$header'; it does not say whether this build may update itself"
}
$said = $Matches[1]
$required = if ($Updater) { $Updater } elseif ($ExpectSigned) { 'on' } else { $null }
if ($required -and $said -ne $required) {
    throw "this build says updater $said; it was expected to say updater $required"
}
if ($required) {
    Write-Host "updater: $said, as required."
}
else {
    Write-Host "updater: $said (not held to either answer: no -Updater, no -ExpectSigned)."
}
