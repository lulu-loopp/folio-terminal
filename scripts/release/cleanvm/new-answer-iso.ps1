<#
.SYNOPSIS
    Build the little ISO that carries `Autounattend.xml` into Windows Setup.

.DESCRIPTION
    Windows Setup looks for an answer file in eight places before it draws its
    first page. The fifth is "removable read-only media in order of drive letter,
    at the root of the drive", and the name there must be `Autounattend.xml`
    (Microsoft, *Windows Setup Automation Overview*, "Implicit Answer File Search
    Order"). A second CD-ROM device on the VM holding a one-file ISO is therefore
    the whole mechanism: no USB pass-through, no floppy image, and nothing
    written into the installation ISO itself — which stays the bytes Microsoft
    published and can still be checked against their hash.

    The image is written with IMAPI2, the disc-mastering service that is part of
    Windows. That matters more than it sounds: `oscdimg` ships inside the Windows
    ADK, and requiring a several-hundred-megabyte SDK on the machine preparing a
    *clean-machine* test would be its own joke. Where the ADK is installed
    anyway, the equivalent one-liner is

        oscdimg -u2 -m <staging folder> <output .iso>

    Two things this does besides copying a file:

      * **substitutes the placeholders.** The answer files beside this script
        carry `@@PASSWORD@@` rather than a password, so that nothing tracked in
        this repository is a credential. The value is put in here, on the way to
        an ISO that is not tracked.
      * **parses the result as XML.** An answer file with a typo in it does not
        fail loudly during Setup; it is ignored, and the first anyone knows is a
        virtual machine sitting on the language page an hour later.

.PARAMETER AnswerFile
    The template to build from — `autounattend-win11.xml` or
    `autounattend-win10.xml` beside this script. A bare name is looked for here.

.PARAMETER Output
    Where to write the ISO. Defaults to `target/cleanvm/<template name>.iso`
    under the repository root.

.PARAMETER Password
    The local account's password, substituted for `@@PASSWORD@@`.

.PARAMETER ComputerName
    Substituted for `@@COMPUTERNAME@@`. Defaults to `FOLIO-WIN11` / `FOLIO-WIN10`
    after the template's own name.

.PARAMETER TimeZone
    A Windows time-zone id, substituted for `@@TIMEZONE@@`. `tzutil /l` lists
    them.

.EXAMPLE
    ./new-answer-iso.ps1 -AnswerFile autounattend-win11.xml

.EXAMPLE
    ./new-answer-iso.ps1 -AnswerFile autounattend-win10.xml -WhatIf
#>

[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)] [string] $AnswerFile,
    [string] $Output,
    [string] $Password = 'folio',
    [string] $ComputerName,
    [string] $TimeZone = 'China Standard Time'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# ── Writing an ISO with what Windows already has ─────────────────────────────

# IMAPI2 hands back the finished image as a COM `IStream`, and there is no
# PowerShell verb that pours one of those into a file. This is that pour: read
# the stream block by block and write the bytes out. The byte count comes back
# through an `IntPtr` because that is `IStream::Read`'s signature — allocated
# once here rather than taken as the address of a local, which would make the
# whole type `unsafe` and need a compiler switch.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public static class FolioIsoWriter
{
    public static void Write(string path, object imageStream, int blockSize, int totalBlocks)
    {
        IStream source = imageStream as IStream;
        if (source == null) { throw new ArgumentException("not an IStream", "imageStream"); }

        byte[] block = new byte[blockSize];
        IntPtr read = Marshal.AllocHGlobal(sizeof(int));
        try
        {
            using (FileStream file = File.Open(path, FileMode.Create, FileAccess.Write))
            {
                while (totalBlocks-- > 0)
                {
                    source.Read(block, blockSize, read);
                    file.Write(block, 0, Marshal.ReadInt32(read));
                }
                file.Flush();
            }
        }
        finally { Marshal.FreeHGlobal(read); }
    }
}
'@

function New-AnswerImage {
    param(
        [Parameter(Mandatory)] [string] $Staging,
        [Parameter(Mandatory)] [string] $Output,
        [Parameter(Mandatory)] [string] $VolumeName
    )

    $image = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
    try {
        # ISO9660 | Joliet. Joliet is what lets the name keep its case: Setup
        # matches case-insensitively, but a disc that spells the file
        # `AUTOUNA~1.XML` is a disc nobody can look at and recognise.
        $image.FileSystemsToCreate = 3
        $image.VolumeName = $VolumeName
        $image.Root.AddTree($Staging, $false)

        $result = $image.CreateResultImage()
        [FolioIsoWriter]::Write($Output, $result.ImageStream, $result.BlockSize, $result.TotalBlocks)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ReleaseComObject($image) | Out-Null
    }
}

# ── The template, and where its ISO goes ─────────────────────────────────────

$root = (Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')).Path

$template = if (Test-Path -LiteralPath $AnswerFile -PathType Leaf) {
    (Resolve-Path -LiteralPath $AnswerFile).Path
}
else {
    $beside = Join-Path $PSScriptRoot $AnswerFile
    if (-not (Test-Path -LiteralPath $beside -PathType Leaf)) {
        throw "no answer file at $AnswerFile, and none by that name beside this script"
    }
    (Resolve-Path -LiteralPath $beside).Path
}

$stem = [IO.Path]::GetFileNameWithoutExtension($template)
if (-not $ComputerName) {
    # `autounattend-win11` becomes `FOLIO-WIN11`. A NetBIOS name is fifteen
    # characters at most and this shape stays inside it.
    $ComputerName = 'FOLIO-' + ($stem -replace '^autounattend-', '').ToUpperInvariant()
}
if (-not $Output) { $Output = Join-Path $root "target\cleanvm\$stem.iso" }

# ── The answer file, with the placeholders filled in ─────────────────────────

$text = [IO.File]::ReadAllText($template)
$substitutions = [ordered]@{
    '@@PASSWORD@@'     = $Password
    '@@COMPUTERNAME@@' = $ComputerName
    '@@TIMEZONE@@'     = $TimeZone
}
foreach ($placeholder in $substitutions.Keys) {
    $text = $text.Replace($placeholder, $substitutions[$placeholder])
}
# Every placeholder this script knows about is gone, and none of the *shape* is
# left either: a template that grew a fourth one would otherwise carry it to
# Setup verbatim and be ignored there.
if ($text -match '@@[A-Za-z_]+@@') {
    throw "$stem still carries the placeholder $($Matches[0]); this script does not know it"
}

# **Read it back as XML before it becomes an image.** Windows Setup does not
# report a malformed answer file. It declines to use it, draws its own first
# page, and waits for a person who is not there.
try { [xml] $text | Out-Null }
catch { throw "$stem is not well-formed XML after substitution: $($_.Exception.Message)" }

Write-Host "$stem : computer $ComputerName, time zone $TimeZone, account folio"

$staging = Join-Path ([IO.Path]::GetTempPath()) ('folio-answer-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($staging) | Out-Null
try {
    # The name Setup looks for, at the root, and nothing else on the disc.
    # UTF-8 with no byte-order mark: a BOM in front of the XML declaration is
    # the classic reason an answer file is skipped without a word.
    [IO.File]::WriteAllText(
        (Join-Path $staging 'Autounattend.xml'),
        $text,
        (New-Object Text.UTF8Encoding($false)))

    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Output)) | Out-Null
    if (-not $PSCmdlet.ShouldProcess($Output, 'write answer-file ISO')) {
        Write-Host "would write $Output"
        return
    }
    New-AnswerImage -Staging $staging -Output $Output -VolumeName 'FOLIOANSWER'
}
finally {
    # `-WhatIf:$false` because this one is not part of what the caller asked
    # about: a dry run that leaves a staging directory in the temp folder is a
    # dry run that litters.
    Remove-Item -LiteralPath $staging -Recurse -Force -WhatIf:$false -ErrorAction SilentlyContinue
}

$iso = Get-Item -LiteralPath $Output
Write-Host ('{0} — {1:N0} bytes' -f $iso.FullName, $iso.Length)
Write-Host 'attach it to the VM as a second CD/DVD device, connected at power on.'
