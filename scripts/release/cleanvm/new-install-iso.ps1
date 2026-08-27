<#
.SYNOPSIS
    Remaster a Microsoft Windows installation ISO so that it boots under EFI
    without waiting for "Press any key to boot from CD or DVD".

.DESCRIPTION
    On 2026-08-27 both clean machines sat for two and a half hours without ever
    reaching Setup. `vmware.log` said what happened, twice over:

        Guest: About to do EFI boot: EFI VMware Virtual SATA CDROM Drive (0.0)
        Guest: Status upon boot failure: Time out
        [msg.Backdoor.OsNotFound] No operating system was found.

    Three seconds between the two lines. That is `cdboot.efi` — the loader inside
    the El Torito boot image `efi\microsoft\boot\efisys.bin` — drawing "Press any
    key to boot from CD or DVD", waiting, and giving up. Nobody was there to press
    a key, the firmware fell through to an empty disk, and the machine announced
    it had no operating system. An unattended install cannot begin with a
    keystroke.

    Microsoft ships the answer on the same disc. Beside `efisys.bin` there is
    **`efisys_noprompt.bin`**, byte for byte the same FAT image with a loader that
    does not ask. This script rebuilds the ISO with that image as the El Torito
    EFI boot entry. Nothing else about the disc changes: the same files, the same
    volume label, and — because the boot image is only *referenced* by the boot
    catalog — both `efisys.bin` and `efisys_noprompt.bin` are still there to look
    at.

    ── Why it is a rebuild and not an edit ──────────────────────────────────────

    The boot catalog records where the boot image lives as a block address. The
    two images are different files at different addresses, so pointing the catalog
    at the other one is a change to a structure the rest of the image is laid out
    around. Rebuilding says what is meant — "this disc, booted from that image" —
    and hands back a file whose every structure was written by one tool that
    agrees with itself.

    ── How ─────────────────────────────────────────────────────────────────────

      1. **Take the contents out.** Not with `Mount-DiskImage`: mounting an image
         wants an elevated session, and a script that needs administrator to
         prepare a *clean machine* test has already lost. So the contents come out
         with an unpacker. `7z` first; failing that Python with `pycdlib`, which
         reads UDF. Neither present is not a guess — it is a message naming both
         and the one command that installs the second.
      2. **Put them back with IMAPI2**, the disc-mastering service that is part of
         Windows, exactly as `new-answer-iso.ps1` builds the answer disc. **UDF,
         not ISO 9660**: `sources\install.wim` on a Windows 11 disc is over six
         gigabytes and ISO 9660 cannot hold a file that size. This is also what
         Microsoft themselves ship — the ISO 9660 tree on their Windows 11 image
         holds one file, a readme saying the disc is UDF.
      3. **Say so in the boot catalog.** One boot entry: platform id `0xEF`, no
         emulation, image `efisys_noprompt.bin`.
      4. **Read the result back and check it**, described under "What is checked"
         below. A disc that boots wrong costs two hours to find out about, so it
         is worth thirty seconds to find out here.

    ── The one ordering that matters ───────────────────────────────────────────

    `IBootOptions::Emulation` has to be set **after** `AssignBootImage`, not
    before. `AssignBootImage` looks at the image's size, sees exactly 1,474,560
    bytes, and sets 1.44 MB floppy emulation on its own; an `Emulation = 0`
    written before it is overwritten and the catalog goes out with media type 2.
    Verified on this host: assigning first and setting emulation second produces
    media type 0 and a 2880-sector entry, matching what Microsoft's own disc
    carries.

    ── What is checked, and what each check is for ─────────────────────────────

      * the El Torito entry is platform `0xEF`, no emulation, and the bytes at the
        block address it names hash equal to `efisys_noprompt.bin`. This is the
        whole point of the exercise, so it is checked against the disc's own copy
        of the file rather than against a number written down here.
      * `sources\install.wim` or `sources\install.esd` is present — the check that
        the contents survived the round trip, aimed at the one file large enough
        to be dropped by a file system that cannot hold it.
      * no `autounattend.xml` anywhere. The answer file rides its own little disc
         (`new-answer-iso.ps1`), and an installation ISO carrying one would be a
        second answer file competing with it in Setup's search order.
      * the file count matches what was taken out, and the image is within a
        sensible fraction of the original's size.

    ── Doing nothing, twice ────────────────────────────────────────────────────

    Two ways this script finishes without building anything, because both are
    situations `new-vm.ps1` walks into on every run:

      * **the ISO already boots without a prompt.** Its boot catalog is read and
        the image it names is compared with the disc's own
        `efisys_noprompt.bin`. If they are the same file, the disc needs nothing,
        and the path handed back is the path handed in.
      * **the remaster is already on disk.** Beside the output ISO is a one-line
        record of the source's name, size and write time. Matching means the
        answer to this question was worked out before and is still true.

    In both cases, and in the case where the work is done, the **only thing this
    script writes to the pipeline is the path of the ISO to boot from**.
    Everything else it has to say goes to the host.

.PARAMETER Iso
    The Microsoft installation ISO.

.PARAMETER Output
    Where the remastered image goes. Defaults to
    `target/cleanvm/<source name>-noprompt.iso` under the repository root.

.PARAMETER BootImage
    The boot image inside the ISO, as a path relative to its root.
    `efi\microsoft\boot\efisys_noprompt.bin` on every Windows installation disc
    since Windows 7.

.PARAMETER VolumeName
    The volume label. Defaults to the source image's own, read out of its primary
    volume descriptor.

.PARAMETER Staging
    Where the contents are unpacked to. Defaults to a directory beside the output,
    removed afterwards. Needs as much room as the ISO, and the output needs that
    much again.

.PARAMETER KeepStaging
    Leave the unpacked contents behind — for looking at what was in the disc, or
    for building twice without unpacking twice.

.PARAMETER Force
    Rebuild even though a remaster of this source is already there.

.PARAMETER SevenZipPath
.PARAMETER PythonPath
    The two unpackers, when they are not where this script looks.

.EXAMPLE
    ./new-install-iso.ps1 -Iso D:\iso\Win10_22H2.iso

.EXAMPLE
    ./new-install-iso.ps1 -Iso D:\iso\Win11.iso -WhatIf

    Prints what it would unpack, build and check, and writes nothing.
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $Iso,
    [string] $Output,
    [string] $BootImage = 'efi\microsoft\boot\efisys_noprompt.bin',
    [string] $VolumeName,
    [string] $Staging,
    [switch] $KeepStaging,
    [switch] $Force,
    [string] $SevenZipPath,
    [string] $PythonPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# `-WhatIf` means "tell me what you would do". Held once, here, so that the plan
# a dry run prints is the run a real one performs.
$planning = [bool] $WhatIfPreference
# A native program's exit code is read here, deliberately, at every call. Newer
# PowerShell can be configured to turn a non-zero one into a terminating error
# before this script ever sees it, which would take the program's output — the
# only useful half of a failure — with it.
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')).Path

# ── Pouring a COM image stream into a file ───────────────────────────────────
#
# IMAPI2 hands the finished image back as an `IStream` and there is no PowerShell
# verb that pours one of those into a file. Two differences from the same routine
# in `new-answer-iso.ps1`, both of them about size: this one reads a thousand
# blocks at a time rather than one — three and a half million cross-process calls
# is not how seven gigabytes should be moved — and it says how far along it is,
# because a build that prints nothing for four minutes looks like a build that has
# stopped.
#
# `SHCreateStreamOnFileEx` is here for the other direction: `AssignBootImage`
# wants the boot image as an `IStream` too.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public static class FolioInstallIso
{
    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode, ExactSpelling = true, PreserveSig = false)]
    private static extern IStream SHCreateStreamOnFileEx(
        string file, uint grfMode, uint dwAttributes, bool fCreate, IStream reserved);

    public static object OpenRead(string path)
    {
        // STGM_READ | STGM_SHARE_DENY_WRITE
        return SHCreateStreamOnFileEx(path, 0x20, 0, false, null);
    }

    public static long Write(string path, object imageStream, int blockSize, int totalBlocks)
    {
        IStream source = imageStream as IStream;
        if (source == null) { throw new ArgumentException("not an IStream", "imageStream"); }

        long expected = (long)blockSize * (long)totalBlocks;
        int chunk = blockSize * 1024;
        byte[] buffer = new byte[chunk];
        IntPtr read = Marshal.AllocHGlobal(sizeof(int));
        long done = 0;
        long announced = 0;
        try
        {
            using (FileStream file = File.Open(path, FileMode.Create, FileAccess.Write))
            {
                while (done < expected)
                {
                    int want = (int)Math.Min((long)chunk, expected - done);
                    source.Read(buffer, want, read);
                    int got = Marshal.ReadInt32(read);
                    if (got <= 0) { break; }
                    file.Write(buffer, 0, got);
                    done += got;
                    if (done - announced >= 512L * 1024 * 1024)
                    {
                        announced = done;
                        Console.Out.WriteLine(string.Format(
                            "             {0,5:N1} GB of {1,5:N1} GB written",
                            done / 1073741824.0, expected / 1073741824.0));
                        Console.Out.Flush();
                    }
                }
                file.Flush();
            }
        }
        finally { Marshal.FreeHGlobal(read); }
        return done;
    }
}
'@

# ── Reading an El Torito boot catalog ────────────────────────────────────────
#
# Enough of the structure to answer one question: which image does this disc boot
# under EFI, and is it the one that does not ask for a keystroke. The volume
# descriptors start at block 16 and are read until the terminator; the boot record
# among them holds the block address of the boot catalog at offset 71; the catalog
# is a validation entry, a default entry, and then section headers each followed
# by their entries.

function Read-BootCatalog {
    param([Parameter(Mandatory)] [string] $Path)

    $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try {
        $block = New-Object byte[] 2048
        $catalogBlock = $null
        for ($lba = 16; $lba -lt 64; $lba++) {
            $stream.Position = [int64] $lba * 2048
            if ($stream.Read($block, 0, 2048) -ne 2048) { break }
            if ([Text.Encoding]::ASCII.GetString($block, 1, 5) -ne 'CD001') { continue }
            if ($block[0] -eq 0) { $catalogBlock = [BitConverter]::ToUInt32($block, 71) }
            if ($block[0] -eq 255) { break }
        }
        if ($null -eq $catalogBlock) { return $null }

        $stream.Position = [int64] $catalogBlock * 2048
        $catalog = New-Object byte[] 2048
        if ($stream.Read($catalog, 0, 2048) -ne 2048) { return $null }

        # The validation entry names the platform of the *default* entry that
        # follows it; a section header names the platform of the entries after
        # that. An EFI image can be in either place, and this reads both.
        $found = $null
        if ($catalog[1] -eq 0xEF -and $catalog[32] -eq 0x88) {
            $found = @{ Where = 'default entry'; Offset = 32 }
        }
        $offset = 64
        while ($null -eq $found -and $offset -lt 1984) {
            $indicator = $catalog[$offset]
            if ($indicator -eq 0x90 -or $indicator -eq 0x91) {
                if ($catalog[$offset + 1] -eq 0xEF -and $catalog[$offset + 32] -eq 0x88) {
                    $found = @{ Where = 'section entry'; Offset = $offset + 32 }
                }
                $offset += 64
            }
            elseif ($indicator -eq 0) { break }
            else { $offset += 32 }
        }
        if ($null -eq $found) { return $null }

        $at = $found.Offset
        return [ordered]@{
            Where              = $found.Where
            ValidationPlatform = $catalog[1]
            Emulation          = $catalog[$at + 1]
            Sectors            = [BitConverter]::ToUInt16($catalog, $at + 6)
            Block              = [BitConverter]::ToUInt32($catalog, $at + 8)
            CatalogBlock       = $catalogBlock
        }
    }
    finally { $stream.Dispose() }
}

function Get-BootImageHash {
    # The boot image is a file on the disc, laid down contiguously, so the whole
    # of it can be read from the block address the catalog names — however many
    # sectors the catalog claims. Microsoft's own discs say one sector for an
    # image of 2880, and reading only what they claim would compare 512 bytes of
    # FAT boot record that both candidate images share.
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [uint32] $Block,
        [Parameter(Mandatory)] [int] $Length
    )

    $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try {
        $stream.Position = [int64] $Block * 2048
        $bytes = New-Object byte[] $Length
        $got = 0
        while ($got -lt $Length) {
            $step = $stream.Read($bytes, $got, $Length - $got)
            if ($step -le 0) { break }
            $got += $step
        }
        if ($got -ne $Length) { return $null }
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha.ComputeHash($bytes)) -replace '-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
    }
    finally { $stream.Dispose() }
}

function Get-VolumeLabel {
    param([Parameter(Mandatory)] [string] $Path)

    $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try {
        $block = New-Object byte[] 2048
        for ($lba = 16; $lba -lt 64; $lba++) {
            $stream.Position = [int64] $lba * 2048
            if ($stream.Read($block, 0, 2048) -ne 2048) { break }
            if ([Text.Encoding]::ASCII.GetString($block, 1, 5) -ne 'CD001') { continue }
            if ($block[0] -eq 1) { return [Text.Encoding]::ASCII.GetString($block, 40, 32).Trim() }
            if ($block[0] -eq 255) { break }
        }
        return $null
    }
    finally { $stream.Dispose() }
}

function Get-FileHashHex {
    param([Parameter(Mandatory)] [string] $Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

# ── The unpacker ─────────────────────────────────────────────────────────────
#
# `Mount-DiskImage` is not used, and the reason is in the description: it wants an
# elevated session on this host, and needing administrator to prepare a
# clean-machine test would be its own joke. Two unpackers that do not:
#
#   7z        reads UDF and ISO 9660 and is the ordinary answer.
#   pycdlib   a Python module that reads UDF, for a host with no 7-Zip.
#             **UNVERIFIED** — 7-Zip was present on the host this was written on,
#             so this half has never selected itself. Its shape is small on
#             purpose.

function Resolve-Unpacker {
    if ($PythonPath -and $SevenZipPath) {
        throw 'pass one of -SevenZipPath and -PythonPath, not both'
    }

    if (-not $PythonPath) {
        $candidates = @()
        if ($SevenZipPath) { $candidates += $SevenZipPath }
        else {
            $onPath = Get-Command '7z' -ErrorAction SilentlyContinue
            if ($onPath) { $candidates += $onPath.Source }
            $candidates += (Join-Path $env:ProgramFiles '7-Zip\7z.exe')
            $candidates += (Join-Path ${env:ProgramFiles(x86)} '7-Zip\7z.exe')
        }
        foreach ($candidate in $candidates) {
            if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
                return @{ Kind = '7z'; Path = (Resolve-Path -LiteralPath $candidate).Path }
            }
        }
        if ($SevenZipPath) { throw "no 7z.exe at $SevenZipPath" }
    }

    $pythons = @()
    if ($PythonPath) { $pythons += $PythonPath }
    else {
        foreach ($name in @('python', 'python3', 'py')) {
            $onPath = Get-Command $name -ErrorAction SilentlyContinue
            if ($onPath) { $pythons += $onPath.Source }
        }
    }
    foreach ($python in $pythons) {
        if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { continue }
        & $python -c 'import pycdlib' 2>&1 | Out-Null
        if ($LASTEXITCODE -eq 0) {
            return @{ Kind = 'pycdlib'; Path = (Resolve-Path -LiteralPath $python).Path }
        }
    }

    throw @"
nothing on this host can read an ISO without administrator rights.

  * 7-Zip is the ordinary answer. <https://www.7-zip.org/>, or pass -SevenZipPath
    if it is installed somewhere this script does not look.
  * Python with pycdlib is the other one:
        $(if ($pythons.Count) { $pythons[0] } else { 'python' }) -m pip install pycdlib

By hand, on a host where an elevated session is available, the whole of what this
script does can be had from the Windows ADK's oscdimg:

  Mount-DiskImage -ImagePath <source .iso>
  robocopy <mounted drive>:\ <staging> /E
  Dismount-DiskImage -ImagePath <source .iso>
  oscdimg -u2 -bootdata:1#pEF,e,b<staging>\efi\microsoft\boot\efisys_noprompt.bin ``
      <staging> <output .iso>
"@
}

function Expand-Iso {
    param(
        [Parameter(Mandatory)] [hashtable] $Unpacker,
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $Destination
    )

    if ($Unpacker.Kind -eq '7z') {
        $output = (& $Unpacker.Path 'x' $Path "-o$Destination" '-y' '-bso0' '-bsp0' 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { throw "7z exited $LASTEXITCODE`n$output" }
        return
    }

    $script = Join-Path ([IO.Path]::GetTempPath()) ('folio-unpack-' + [Guid]::NewGuid().ToString('n') + '.py')
    [IO.File]::WriteAllText($script, @'
import os, sys
import pycdlib

source, destination = sys.argv[1], sys.argv[2]
iso = pycdlib.PyCdlib()
iso.open(source)
for directory, _, files in iso.walk(udf_path='/'):
    local = os.path.join(destination, directory.strip('/').replace('/', os.sep))
    if not os.path.isdir(local):
        os.makedirs(local)
    for name in files:
        remote = directory.rstrip('/') + '/' + name
        with open(os.path.join(local, name), 'wb') as handle:
            iso.get_file_from_iso_fp(handle, udf_path=remote)
iso.close()
'@, (New-Object Text.UTF8Encoding($false)))
    try {
        $output = (& $Unpacker.Path $script $Path $Destination 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { throw "pycdlib exited $LASTEXITCODE`n$output" }
    }
    finally { Remove-Item -LiteralPath $script -Force -WhatIf:$false -ErrorAction SilentlyContinue }
}

function Expand-OneFile {
    # One file out of a seven-gigabyte image, for the question asked before any
    # work starts: does this disc already boot without a prompt.
    param(
        [Parameter(Mandatory)] [hashtable] $Unpacker,
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $Entry,
        [Parameter(Mandatory)] [string] $Destination
    )

    [IO.Directory]::CreateDirectory($Destination) | Out-Null
    $leaf = Join-Path $Destination ([IO.Path]::GetFileName($Entry))
    Remove-Item -LiteralPath $leaf -Force -WhatIf:$false -ErrorAction SilentlyContinue

    if ($Unpacker.Kind -eq '7z') {
        & $Unpacker.Path 'e' $Path "-o$Destination" $Entry '-y' '-bso0' '-bsp0' 2>&1 | Out-Null
    }
    else {
        $script = Join-Path ([IO.Path]::GetTempPath()) ('folio-one-' + [Guid]::NewGuid().ToString('n') + '.py')
        [IO.File]::WriteAllText($script, @'
import sys
import pycdlib

source, entry, output = sys.argv[1], sys.argv[2], sys.argv[3]
iso = pycdlib.PyCdlib()
iso.open(source)
with open(output, 'wb') as handle:
    iso.get_file_from_iso_fp(handle, udf_path='/' + entry.replace('\\', '/'))
iso.close()
'@, (New-Object Text.UTF8Encoding($false)))
        try { & $Unpacker.Path $script $Path $Entry $leaf 2>&1 | Out-Null }
        finally { Remove-Item -LiteralPath $script -Force -WhatIf:$false -ErrorAction SilentlyContinue }
    }

    if (Test-Path -LiteralPath $leaf -PathType Leaf) { return (Resolve-Path -LiteralPath $leaf).Path }
    return $null
}

function Get-IsoEntryList {
    param(
        [Parameter(Mandatory)] [hashtable] $Unpacker,
        [Parameter(Mandatory)] [string] $Path
    )

    if ($Unpacker.Kind -eq '7z') {
        # `-slt` prints one field per line, so a name and whether it is a
        # directory arrive as two separate lines and have to be paired. The
        # column-aligned default listing would be shorter to read and wrong the
        # first time a name has two spaces in it.
        $lines = & $Unpacker.Path 'l' '-ba' '-slt' $Path 2>&1
        if ($LASTEXITCODE -ne 0) { throw "7z exited $LASTEXITCODE listing $Path" }
        $files = New-Object Collections.Generic.List[string]
        $name = $null
        foreach ($line in $lines) {
            if ($line -match '^Path = (.*)$') { $name = $Matches[1] }
            elseif ($line -eq 'Folder = -' -and $name) { $files.Add($name); $name = $null }
            elseif ($line -eq 'Folder = +') { $name = $null }
        }
        return @($files)
    }

    $script = Join-Path ([IO.Path]::GetTempPath()) ('folio-list-' + [Guid]::NewGuid().ToString('n') + '.py')
    [IO.File]::WriteAllText($script, @'
import sys
import pycdlib

iso = pycdlib.PyCdlib()
iso.open(sys.argv[1])
for directory, _, files in iso.walk(udf_path='/'):
    for name in files:
        print((directory.rstrip('/') + '/' + name).lstrip('/').replace('/', '\\'))
iso.close()
'@, (New-Object Text.UTF8Encoding($false)))
    try {
        $lines = & $Unpacker.Path $script $Path 2>&1
        if ($LASTEXITCODE -ne 0) { throw "pycdlib exited $LASTEXITCODE listing $Path" }
        return @($lines)
    }
    finally { Remove-Item -LiteralPath $script -Force -WhatIf:$false -ErrorAction SilentlyContinue }
}

# ── Where everything goes ────────────────────────────────────────────────────

if (-not (Test-Path -LiteralPath $Iso -PathType Leaf)) { throw "no installation ISO at $Iso" }
$Iso = (Resolve-Path -LiteralPath $Iso).Path
$source = Get-Item -LiteralPath $Iso
$stem = [IO.Path]::GetFileNameWithoutExtension($Iso)

if (-not $Output) { $Output = Join-Path $root "target\cleanvm\$stem-noprompt.iso" }
$Output = [IO.Path]::GetFullPath($Output)
$outputDirectory = [IO.Path]::GetDirectoryName($Output)
# The record beside the output that says which source it was made from. Three
# facts, because "the same file" is what is being asked and a path alone does not
# answer it.
$record = "$Output.source"
$stamp = '{0}|{1}|{2:o}' -f $source.Name, $source.Length, $source.LastWriteTimeUtc

if (-not $Staging) {
    # **Short on purpose.** IMAPI2 is COM from before long paths and gives up on
    # anything over 259 characters with "the system cannot find the path
    # specified", naming nothing. A Windows 11 disc carries relative paths of 148
    # characters under `sources\replacementmanifests\`, so a staging directory
    # named after an ISO whose own name is 91 characters long is over the line
    # before a single file is added. Eight hex digits of the source's path keep
    # two sources from colliding without spending the room.
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $token = [BitConverter]::ToString(
            $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Iso.ToLowerInvariant()))
        ).Replace('-', '').Substring(0, 8).ToLowerInvariant()
    }
    finally { $sha.Dispose() }
    $Staging = Join-Path $outputDirectory "staging-$token"
}
$Staging = [IO.Path]::GetFullPath($Staging)

Write-Host "source   : $Iso ($('{0:N0}' -f $source.Length) bytes)"
Write-Host "output   : $Output"
Write-Host "boot     : $BootImage"
if ($planning) { Write-Host 'MODE     : planning only — nothing is unpacked, built or written.' }

$unpacker = Resolve-Unpacker
Write-Host "unpacker : $($unpacker.Kind) — $($unpacker.Path)"

# ── Does the source already boot without a prompt ────────────────────────────

$probe = Join-Path ([IO.Path]::GetTempPath()) ('folio-noprompt-' + [Guid]::NewGuid().ToString('n'))
$reference = $null
try {
    $reference = Expand-OneFile -Unpacker $unpacker -Path $Iso -Entry $BootImage -Destination $probe
    if (-not $reference) { throw "$([IO.Path]::GetFileName($Iso)) does not carry $BootImage" }
    $referenceHash = Get-FileHashHex $reference
    $referenceLength = (Get-Item -LiteralPath $reference).Length

    $catalog = Read-BootCatalog -Path $Iso
    if ($null -eq $catalog) { throw "$([IO.Path]::GetFileName($Iso)) has no EFI entry in its El Torito boot catalog" }
    $current = Get-BootImageHash -Path $Iso -Block $catalog.Block -Length $referenceLength
    Write-Host ("source   : boots from the image at block {0} ({1}), {2}" -f
        $catalog.Block, $catalog.Where,
        $(if ($current -eq $referenceHash) { 'which is the one that does not prompt' } else { 'which prompts' }))

    if ($current -eq $referenceHash) {
        Write-Host ''
        Write-Host 'nothing to do: this ISO already boots without waiting for a key.'
        Write-Output $Iso
        return
    }
}
finally { Remove-Item -LiteralPath $probe -Recurse -Force -WhatIf:$false -ErrorAction SilentlyContinue }

# ── Is the remaster already there ────────────────────────────────────────────

if ((Test-Path -LiteralPath $Output -PathType Leaf) -and -not $Force) {
    $was = if (Test-Path -LiteralPath $record -PathType Leaf) { [IO.File]::ReadAllText($record).Trim() } else { '' }
    if ($was -eq $stamp) {
        Write-Host ''
        Write-Host ('reusing the remaster made earlier from this same source ({0:N0} bytes).' -f
            (Get-Item -LiteralPath $Output).Length)
        Write-Output $Output
        return
    }
    if ($was) { Write-Host "output   : an older remaster is there, from $($was.Split('|')[0]) — rebuilding" }
    else { Write-Host 'output   : something is there with no record of its source — rebuilding' }
}

if (-not $VolumeName) {
    $VolumeName = Get-VolumeLabel -Path $Iso
    if (-not $VolumeName) { $VolumeName = $stem.ToUpperInvariant() }
}
Write-Host "label    : $VolumeName"

# ── Take it apart ────────────────────────────────────────────────────────────

Write-Host ''
Write-Host 'unpack'
Write-Host "  into       $Staging"
$began = Get-Date

if (-not $PSCmdlet.ShouldProcess($Output, "remaster $([IO.Path]::GetFileName($Iso)) to boot without a prompt")) {
    $planning = $true
}

$unpacked = 0
if (-not $planning) {
    Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
    [IO.Directory]::CreateDirectory($Staging) | Out-Null
    Expand-Iso -Unpacker $unpacker -Path $Iso -Destination $Staging
    $files = @(Get-ChildItem -LiteralPath $Staging -Recurse -File)
    $unpacked = $files.Count
    Write-Host ("             {0:N0} files in {1:N0} s" -f $unpacked, ((Get-Date) - $began).TotalSeconds)

    # IMAPI2 is COM from before long paths: over 259 characters it answers "the
    # system cannot find the path specified" and names nothing at all. Asked here,
    # where the answer can name the file and the fix.
    $longest = $files | Sort-Object { $_.FullName.Length } -Descending | Select-Object -First 1
    if ($longest -and $longest.FullName.Length -gt 259) {
        throw @"
$Staging is too long a place to unpack this disc into.
$($longest.FullName.Length) characters, and IMAPI2 stops at 259:
  $($longest.FullName)
Pass a shorter -Staging, or a shorter -Output for it to sit beside.
"@
    }

    $bootImagePath = Join-Path $Staging $BootImage
    if (-not (Test-Path -LiteralPath $bootImagePath -PathType Leaf)) {
        throw "the unpacked contents have no $BootImage"
    }
    if ((Get-FileHashHex $bootImagePath) -ne $referenceHash) {
        throw "$BootImage came out of the image differently the second time; the unpacker is not reading it consistently"
    }
}

# ── Put it back together ─────────────────────────────────────────────────────

Write-Host 'build'
Write-Host '  image      IMAPI2FS.MsftFileSystemImage, UDF 1.02'
Write-Host "  boot       platform 0xEF, no emulation, $BootImage"
if ($planning) {
    Write-Host ''
    Write-Host 'planning only — nothing above was unpacked, built or written.'
    Write-Output $Output
    return
}

[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$image = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
try {
    # UDF alone. `sources\install.wim` on a Windows 11 disc is over six gigabytes
    # and no ISO 9660 tree can hold a file that size; Microsoft's own image puts
    # nothing but a readme in its ISO 9660 tree for the same reason.
    $image.FileSystemsToCreate = 4
    $image.UDFRevision = 0x102
    # Without a disc recorder attached IMAPI2 sizes the image for a blank CD.
    # A negative value is how it is told there is no such limit.
    $image.FreeMediaBlocks = -1
    # Read the files where they are rather than copying every one of them into
    # the temp directory first, which for seven gigabytes is a second copy of the
    # whole disc for no purpose.
    $image.StageFiles = $false
    $image.VolumeName = $VolumeName

    $boot = New-Object -ComObject IMAPI2FS.BootOptions
    $boot.PlatformId = 0xEF
    $boot.AssignBootImage([FolioInstallIso]::OpenRead((Join-Path $Staging $BootImage)))
    # **After the assignment, and this is the whole reason the two lines are in
    # this order.** `AssignBootImage` sees an image of exactly 1,474,560 bytes and
    # sets 1.44 MB floppy emulation by itself; an emulation written before it is
    # overwritten, and the disc goes out with media type 2 in its catalog.
    $boot.Emulation = 0
    $image.BootImageOptions = $boot

    $image.Root.AddTree($Staging, $false) | Out-Null

    $built = Get-Date
    $result = $image.CreateResultImage()
    Write-Host ("  layout     {0:N0} blocks of {1:N0} bytes" -f $result.TotalBlocks, $result.BlockSize)
    $written = [FolioInstallIso]::Write($Output, $result.ImageStream, $result.BlockSize, $result.TotalBlocks)
    Write-Host ("  wrote      {0:N0} bytes in {1:N0} s" -f $written, ((Get-Date) - $built).TotalSeconds)
}
finally {
    [Runtime.InteropServices.Marshal]::ReleaseComObject($image) | Out-Null
}

# ── Read it back ─────────────────────────────────────────────────────────────

Write-Host 'check'
$problems = New-Object Collections.Generic.List[string]

$catalog = Read-BootCatalog -Path $Output
if ($null -eq $catalog) { $problems.Add('the image has no EFI entry in its El Torito boot catalog') }
else {
    Write-Host ("  catalog    {0}, platform 0x{1:X2}, emulation {2}, {3} sectors at block {4}" -f
        $catalog.Where, $catalog.ValidationPlatform, $catalog.Emulation, $catalog.Sectors, $catalog.Block)
    if ($catalog.Emulation -ne 0) { $problems.Add("the boot entry says emulation $($catalog.Emulation); it has to be 0") }
    $expectedSectors = [math]::Ceiling($referenceLength / 512)
    if ($catalog.Sectors -ne $expectedSectors) {
        $problems.Add("the boot entry loads $($catalog.Sectors) sectors; the image is $expectedSectors")
    }
    $hash = Get-BootImageHash -Path $Output -Block $catalog.Block -Length $referenceLength
    if ($hash -eq $referenceHash) { Write-Host "  boot       the bytes at that block are $BootImage" }
    else { $problems.Add("the bytes the boot entry names are not $BootImage") }
}

$entries = Get-IsoEntryList -Unpacker $unpacker -Path $Output
$normalised = @($entries | ForEach-Object { $_.Replace('/', '\').ToLowerInvariant() })
$install = @($normalised | Where-Object { $_ -eq 'sources\install.wim' -or $_ -eq 'sources\install.esd' })
if ($install.Count -eq 1) { Write-Host "  contents   $($install[0]) is on the disc" }
else { $problems.Add('neither sources\install.wim nor sources\install.esd is on the disc') }

$answers = @($normalised | Where-Object { [IO.Path]::GetFileName($_) -eq 'autounattend.xml' })
if ($answers.Count -eq 0) { Write-Host '  answer     no autounattend.xml — it rides its own disc' }
else { $problems.Add("the disc carries an answer file: $($answers -join ', ')") }

if ($entries.Count -eq $unpacked) { Write-Host "  files      $($entries.Count), the number that came out" }
else { $problems.Add("$($entries.Count) files went in and $unpacked came out of the source") }

$grew = (Get-Item -LiteralPath $Output).Length
Write-Host ("  size       {0:N0} bytes, {1:P1} of the source" -f $grew, ($grew / $source.Length))
if ($grew -lt $source.Length * 0.9 -or $grew -gt $source.Length * 1.2) {
    $problems.Add("the image is $('{0:N0}' -f $grew) bytes against the source's $('{0:N0}' -f $source.Length)")
}

if ($problems.Count -gt 0) {
    Remove-Item -LiteralPath $record -Force -ErrorAction SilentlyContinue
    throw @"
$([IO.Path]::GetFileName($Output)) is not right and has been left in place to look at:
$($problems | ForEach-Object { "  $_" } | Out-String)
"@
}

[IO.File]::WriteAllText($record, $stamp, (New-Object Text.UTF8Encoding($false)))
if (-not $KeepStaging) {
    Write-Host "  staging    removing $Staging"
    Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host ("done in {0:N0} s." -f ((Get-Date) - $began).TotalSeconds)
Write-Output $Output
