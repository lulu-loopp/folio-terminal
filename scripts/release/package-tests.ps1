<#
.SYNOPSIS
    `package.ps1` and `smoke.ps1 -ArchiveOnly` against the release manifest, on a
    real archive packed from a development build. Nothing is started.

.DESCRIPTION
    0.4.6 ticket U-9 (`docs/plans/design/self-update-2026-09-16.md` revision
    (b), F-4): `folio.exe` carries the manifest of its own archive, `package.ps1`
    refuses to pack a member whose bytes differ from it, and `smoke.ps1` checks
    the packed archive against it. These cases run the real producer end to end:
    the `folio.exe` a `cargo build` wrote (with the manifest `build.rs` embedded),
    the real `package.ps1` packing it with the real `makeappx`, and the real
    `smoke.ps1` reading the archive back — and then the same with one member
    changed, to see each refusal name it.

    **Nothing here runs `folio.exe`.** `package.ps1` is given `-Binaries`, which
    asks the executable nothing (only `-Binary` runs `--version`), and
    `smoke.ps1` is given `-ArchiveOnly`, which stops before its first process.
    The manifest is read out of the file's resources, never by starting it.

    What it needs: a Windows build of `bt-app` (`cargo build -p bt-app`, debug is
    enough) whose directory holds `folio.exe` and the two ConPTY files, and the
    Windows SDK's `makeappx.exe`, which `package.ps1` finds on its own. The
    build must be of this checkout: the manifest names the bytes the tree had
    when it was compiled, which is the point.

    Each script runs as a child `pwsh` and is judged by its exit code and its
    message, for the reason `sign-tests.ps1` gives.

.PARAMETER Binaries
    The build directory to package from. Defaults to `target/debug`.
#>

[CmdletBinding()]
param([string] $Binaries)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path
if (-not $Binaries) { $Binaries = Join-Path $root 'target\debug' }
$Binaries = (Resolve-Path -LiteralPath $Binaries).Path
. (Join-Path $here 'release-manifest.ps1')

$pwsh = (Get-Command -Name pwsh -CommandType Application)[0].Source
Add-Type -AssemblyName System.IO.Compression.FileSystem

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-package-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null

# The build, copied out so that nothing this file does can touch the directory
# a developer's next `cargo` run writes into.
$bin = Join-Path $scratch 'bin'
[IO.Directory]::CreateDirectory($bin) | Out-Null
foreach ($name in @('folio.exe', 'conpty.dll', 'OpenConsole.exe')) {
    $from = Join-Path $Binaries $name
    if (-not (Test-Path -LiteralPath $from -PathType Leaf)) { throw "no $name in $Binaries; build bt-app first" }
    Copy-Item -LiteralPath $from -Destination (Join-Path $bin $name)
}

$failures = New-Object System.Collections.Generic.List[string]
$ran = 0

function Test-Case {
    param([string] $Name, [scriptblock] $Body)
    $script:ran++
    try {
        & $Body
        Write-Host "  ok    $Name"
    }
    catch {
        Write-Host "  FAIL  $Name"
        Write-Host "        $($_.Exception.Message)"
        $script:failures.Add($Name)
    }
}

function Invoke-Child {
    param([string] $Script, [string[]] $Arguments)
    $output = & $pwsh -NoLogo -NoProfile -File (Join-Path $here $Script) @Arguments 2>&1
    $text = ($output | Out-String)
    return [pscustomobject]@{
        ExitCode = $LASTEXITCODE
        Text     = $text
        Flat     = ((($text -split "`n" | ForEach-Object { $_ -replace '^\s*\|\s?', '' }) -join ' ') -replace '\s+', ' ')
    }
}

function Invoke-Package {
    param([string] $Output, [string[]] $More = @())
    return Invoke-Child -Script 'package.ps1' -Arguments (@('-Binaries', $bin, '-Output', $Output) + $More)
}

function Invoke-ArchiveSmoke {
    param([string] $Archive)
    return Invoke-Child -Script 'smoke.ps1' -Arguments @(
        '-Exe', (Join-Path $bin 'folio.exe'), '-Archive', $Archive, '-ArchiveOnly',
        '-Artifacts', (Join-Path $scratch ('smoke-' + [Guid]::NewGuid().ToString('n'))),
        '-PackageDirectory', (Join-Path $scratch 'no-package-directory'))
}

# A copy of `$Archive` with `$Change` applied to it: a scriptblock handed the
# open `ZipArchive` (update mode) and the archive's root folder.
function New-ChangedArchive {
    param([string] $Archive, [string] $Name, [scriptblock] $Change)
    $copy = Join-Path $scratch $Name
    Copy-Item -LiteralPath $Archive -Destination $copy
    $zip = [IO.Compression.ZipFile]::Open($copy, [IO.Compression.ZipArchiveMode]::Update)
    try {
        $rootFolder = ($zip.Entries[0].FullName -split '/')[0]
        & $Change $zip $rootFolder
    }
    finally { $zip.Dispose() }
    return $copy
}

function Set-ZipEntryText {
    param($Zip, [string] $FullName, [string] $Text)
    $old = $Zip.GetEntry($FullName)
    if ($old) { $old.Delete() }
    $entry = $Zip.CreateEntry($FullName)
    $writer = New-Object IO.StreamWriter($entry.Open())
    try { $writer.Write($Text) } finally { $writer.Dispose() }
}

Write-Host ''
Write-Host 'package.ps1 and smoke.ps1 -ArchiveOnly, against the release manifest:'

$goodOutput = Join-Path $scratch 'out-good'
$good = Invoke-Package -Output $goodOutput
$goodArchive = @(Get-ChildItem -LiteralPath $goodOutput -File -Filter 'folio-*-windows-x64.zip' -ErrorAction SilentlyContinue)

# RED (U-9) — **the manifest lists every member but the two signed ones.**
#
# The archive `package.ps1` wrote from this build, read back entry by entry,
# against the manifest read out of that build's `folio.exe`: every member but
# `folio.exe` and `folio.msix` is listed, with the entry's own size and SHA-256,
# and nothing else is. Then `smoke.ps1 -ArchiveOnly` reads the same archive and
# agrees. On BASE there is no manifest in the executable and the read refuses.
#
# MUTATION: make `build.rs`'s `release_manifest` skip `Source::Documents` and
# the four documents are unlisted; make `package.ps1` pack a file the list does
# not name and it is unlisted too.
Test-Case 'the_manifest_lists_every_member_but_the_two_signed_ones' {
    if ($good.ExitCode -ne 0) { throw "package.ps1 exited $($good.ExitCode): $($good.Text)" }
    if ($good.Flat -notmatch 'release manifest: \d+ members match') { throw "package.ps1 did not say it checked the manifest: $($good.Text)" }
    if ($goodArchive.Count -ne 1) { throw "package.ps1 left $($goodArchive.Count) archives" }

    $release = Read-ReleaseManifest -Exe (Join-Path $bin 'folio.exe')
    $zip = [IO.Compression.ZipFile]::OpenRead($goodArchive[0].FullName)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -notmatch '/$' })
        $prefix = "$($release.ArchiveRoot)/"
        $inArchive = @($entries | ForEach-Object {
                if (-not $_.FullName.StartsWith($prefix)) { throw "$($_.FullName) is outside $prefix" }
                $_.FullName.Substring($prefix.Length)
            })
        $signed = @('folio.exe', 'folio.msix')
        foreach ($name in $signed) {
            if ($inArchive -cnotcontains $name) { throw "the archive has no $name" }
            if (@($release.Members | Where-Object { $_.Name -ceq $name }).Count) { throw "the manifest lists $name" }
        }
        $expected = @($inArchive | Where-Object { $signed -cnotcontains $_ } | Sort-Object)
        $listed = @($release.Members | ForEach-Object { $_.Name } | Sort-Object)
        if (($expected -join '|') -cne ($listed -join '|')) {
            throw "the archive holds [$($expected -join ', ')] and the manifest lists [$($listed -join ', ')]"
        }
        foreach ($member in $release.Members) {
            $entry = $zip.GetEntry($prefix + $member.Name)
            $stream = $entry.Open()
            try { $hash = Get-StreamSha256 -Stream $stream } finally { $stream.Dispose() }
            if ($entry.Length -ne $member.Size -or $hash -cne $member.Sha256) {
                throw "$($member.Name) in the archive is $($entry.Length) bytes $hash; the manifest says $($member.Size) $($member.Sha256)"
            }
        }
    }
    finally { $zip.Dispose() }

    $smoked = Invoke-ArchiveSmoke -Archive $goodArchive[0].FullName
    if ($smoked.ExitCode -ne 0) { throw "smoke.ps1 -ArchiveOnly exited $($smoked.ExitCode): $($smoked.Text)" }
    if ($smoked.Flat -notmatch 'members match the manifest' -or $smoked.Flat -notmatch 'nothing was started') {
        throw "smoke.ps1 did not say it checked the archive and started nothing: $($smoked.Text)"
    }
}

# RED (U-9) — **package.ps1 refuses a member that differs from the manifest.**
#
# The review's counterexample (F-4): `uninstall.cmd` changed after the build.
# `SHA256SUMS.txt` would have hashed the changed file and every earlier check
# passed; now the manifest in `folio.exe` names the bytes the build saw, and the
# run stops before anything is packed, naming the member.
#
# MUTATION: make `Compare-ReleaseMembers` skip the SHA-256 comparison and a
# same-length change packs; drop the call in `package.ps1` and this packs.
Test-Case 'package_refuses_a_member_that_differs_from_the_manifest' {
    $packaging = Join-Path $scratch 'packaging-changed'
    [IO.Directory]::CreateDirectory($packaging) | Out-Null
    foreach ($name in @('folio-here.cmd', 'uninstall.cmd')) {
        Copy-Item -LiteralPath (Join-Path (Join-Path $root 'packaging') $name) -Destination (Join-Path $packaging $name)
    }
    # One byte changed and none added, so the size agrees and only the hash
    # can tell.
    $cmd = Join-Path $packaging 'uninstall.cmd'
    $bytes = [IO.File]::ReadAllBytes($cmd)
    $bytes[0] = if ($bytes[0] -eq 0x40) { 0x41 } else { 0x40 }
    [IO.File]::WriteAllBytes($cmd, $bytes)

    $output = Join-Path $scratch 'out-changed'
    $result = Invoke-Package -Output $output -More @('-Packaging', $packaging)
    if ($result.ExitCode -eq 0) { throw "package.ps1 packed a changed uninstall.cmd: $($result.Text)" }
    if ($result.Flat -notmatch 'different : uninstall\.cmd hashes to') { throw "the refusal did not name uninstall.cmd's hash: $($result.Text)" }
    if (@(Get-ChildItem -LiteralPath $output -Filter '*.zip' -ErrorAction SilentlyContinue).Count) {
        throw 'an archive was written although the run was refused'
    }
}

Test-Case 'package refuses a member missing from where the list says it is' {
    $documents = Join-Path $scratch 'documents-short'
    [IO.Directory]::CreateDirectory($documents) | Out-Null
    foreach ($name in @('LICENSE-MIT', 'LICENSE-APACHE', 'THIRD-PARTY-NOTICES.md')) {
        Copy-Item -LiteralPath (Join-Path $root $name) -Destination (Join-Path $documents $name)
    }
    $result = Invoke-Package -Output (Join-Path $scratch 'out-short') -More @('-Documents', $documents)
    if ($result.ExitCode -eq 0) { throw 'package.ps1 packed without TRADEMARK.md' }
    if ($result.Flat -notmatch 'TRADEMARK\.md') { throw "the refusal did not name TRADEMARK.md: $($result.Text)" }
}

Test-Case 'smoke refuses an archive whose member changed after packing' {
    if ($goodArchive.Count -ne 1) { throw 'no good archive to change' }
    $changed = New-ChangedArchive -Archive $goodArchive[0].FullName -Name 'changed.zip' -Change {
        param($zip, $rootFolder)
        Set-ZipEntryText -Zip $zip -FullName "$rootFolder/uninstall.cmd" -Text "@echo off`r`necho changed`r`n"
    }
    $result = Invoke-ArchiveSmoke -Archive $changed
    if ($result.ExitCode -eq 0) { throw 'smoke.ps1 passed an archive with a changed uninstall.cmd' }
    if ($result.Flat -notmatch 'different : uninstall\.cmd') { throw "the refusal did not name uninstall.cmd: $($result.Text)" }
}

Test-Case 'smoke refuses an archive with a member the manifest does not list' {
    if ($goodArchive.Count -ne 1) { throw 'no good archive to change' }
    $added = New-ChangedArchive -Archive $goodArchive[0].FullName -Name 'added.zip' -Change {
        param($zip, $rootFolder)
        Set-ZipEntryText -Zip $zip -FullName "$rootFolder/version.dll" -Text 'not ours'
    }
    $result = Invoke-ArchiveSmoke -Archive $added
    if ($result.ExitCode -eq 0) { throw 'smoke.ps1 passed an archive with an added DLL' }
    if ($result.Flat -notmatch 'unlisted : version\.dll') { throw "the refusal did not name version.dll: $($result.Text)" }
}

Test-Case 'smoke refuses an archive missing a member the manifest lists' {
    if ($goodArchive.Count -ne 1) { throw 'no good archive to change' }
    $short = New-ChangedArchive -Archive $goodArchive[0].FullName -Name 'short.zip' -Change {
        param($zip, $rootFolder)
        $zip.GetEntry("$rootFolder/TRADEMARK.md").Delete()
    }
    $result = Invoke-ArchiveSmoke -Archive $short
    if ($result.ExitCode -eq 0) { throw 'smoke.ps1 passed an archive without TRADEMARK.md' }
    if ($result.Flat -notmatch 'missing : TRADEMARK\.md') { throw "the refusal did not name TRADEMARK.md: $($result.Text)" }
}

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
exit 0
