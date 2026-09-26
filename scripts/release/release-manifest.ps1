<#
.SYNOPSIS
    The release manifest `folio.exe` carries of its own archive, read out of the
    file and held against what is being packed or was packed. Dot-sourced by
    `package.ps1` and `smoke.ps1`; it runs nothing on its own.

.DESCRIPTION
    **What this release contains** is `crates/bt-app/build.rs`'s fact (0.4.6
    ticket U-9, `docs/plans/design/self-update-2026-09-16.md` revision (b),
    F-4): for a Windows target it embeds an `RCDATA` resource named
    `FOLIO_RELEASE_MANIFEST` listing the name, SHA-256 and size of every archive
    member but `folio.exe` itself and `folio.msix`, so the executable's own
    Authenticode signature signs the rest of the archive. The format is
    `bt_winres::release_manifest`, and the parser below is its grammar spelled a
    second time, in the one language the release machine and a clean Windows
    both have.

    **The resource is read from the file and never by running it.** A flag on
    the executable would start the GUI binary on the packaging machine; this
    maps the file as an image resource with `LoadLibraryExW` and
    `LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE`, which loads no
    code, runs no entry point and resolves no import — the same read the
    updater makes of a never-executed new `folio.exe` (E-14).

    **Which files the archive holds is written once**, in
    `archive-members.txt` beside this file, and `Get-ArchiveMemberList` reads it
    with the grammar `bt_winres::release_manifest::parse_member_list` has.
    `package.ps1` packs from it; `build.rs` builds the manifest from it.

    Written for Windows PowerShell 5.1 as well as 7: `smoke.ps1` dot-sources it,
    and `smoke.ps1` runs on a clean machine that has only 5.1.
#>

Set-StrictMode -Version Latest

$script:ReleaseManifestResource = 'FOLIO_RELEASE_MANIFEST'
$script:ReleaseManifestFormat = 1

if (-not ('FolioRelease.PeResource' -as [type])) {
    Add-Type -Namespace FolioRelease -Name PeResource -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool FreeLibrary(IntPtr module);
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr FindResourceW(IntPtr module, string name, IntPtr type);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern IntPtr LockResource(IntPtr data);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern uint SizeofResource(IntPtr module, IntPtr resource);
'@
}

# `LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE`, and `RT_RCDATA`.
$script:LoadAsResourceOnly = [uint32] (0x00000002 -bor 0x00000020)
$script:RtRcdata = [IntPtr] 10

function Read-ReleaseManifestText {
    <# The manifest's text out of `$Exe`'s resources, or a refusal naming the file. #>
    param([Parameter(Mandatory)] [string] $Exe)

    $module = [FolioRelease.PeResource]::LoadLibraryExW($Exe, [IntPtr]::Zero, $script:LoadAsResourceOnly)
    if ($module -eq [IntPtr]::Zero) {
        $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "$Exe cannot be read as an image's resources (error $code)"
    }
    try {
        $found = [FolioRelease.PeResource]::FindResourceW($module, $script:ReleaseManifestResource, $script:RtRcdata)
        if ($found -eq [IntPtr]::Zero) {
            throw ("$Exe carries no $($script:ReleaseManifestResource) resource, so it names no " +
                   'archive members: it was built before the release manifest existed, or not for Windows')
        }
        $size = [FolioRelease.PeResource]::SizeofResource($module, $found)
        $loaded = [FolioRelease.PeResource]::LoadResource($module, $found)
        $pointer = [FolioRelease.PeResource]::LockResource($loaded)
        if ($pointer -eq [IntPtr]::Zero) { throw "the $($script:ReleaseManifestResource) resource in $Exe cannot be read" }
        $bytes = New-Object byte[] $size
        [Runtime.InteropServices.Marshal]::Copy($pointer, $bytes, 0, [int] $size)
    }
    finally { [void] [FolioRelease.PeResource]::FreeLibrary($module) }

    foreach ($byte in $bytes) {
        if ($byte -gt 0x7E -or ($byte -lt 0x20 -and $byte -ne 0x0A)) {
            throw "the $($script:ReleaseManifestResource) resource in $Exe is not the ASCII text a manifest is"
        }
    }
    return [Text.Encoding]::ASCII.GetString($bytes)
}

function Test-ManifestWord {
    param([string] $Text)
    return ($Text -cmatch '^[\x21-\x7E]+$')
}

function Test-MemberName {
    param([string] $Name)
    return ((Test-ManifestWord $Name) -and ($Name -cnotmatch '[/\\:*?"<>|]') -and -not $Name.EndsWith('.'))
}

function ConvertFrom-ReleaseManifest {
    <#
        The manifest in `$Text`, as an object with `Product`, `Version`, `Arch`,
        `ArchiveRoot`, `Protocol`, `MinUpdater` and `Members` (each `Name`,
        `Sha256`, `Size`), or a refusal. The grammar is
        `bt_winres::release_manifest::Manifest::parse`'s, refusal for refusal:
        an unknown format by name, a cut text as truncated.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Text)

    if (-not $Text.EndsWith("`n")) {
        throw 'truncated manifest: the text does not end in a newline, so its last line may be cut'
    }
    $lines = $Text.Substring(0, $Text.Length - 1).Split([char] "`n")
    $at = 0

    if ($lines[0] -cnotmatch '^folio-release-manifest (.*)$') {
        throw "malformed manifest: the first line is '$($lines[0])', not 'folio-release-manifest <format>'"
    }
    if ($Matches[1] -cne [string] $script:ReleaseManifestFormat) {
        throw "unknown manifest format: '$($Matches[1])'; this script reads format $($script:ReleaseManifestFormat)"
    }
    $at = 1

    $values = @{}
    foreach ($key in @('product', 'version', 'arch', 'archive_root', 'protocol', 'min_updater', 'members')) {
        if ($at -ge $lines.Count) { throw "truncated manifest: it ends before its '$key' line" }
        $line = $lines[$at]
        $at++
        $prefix = "$key "
        if (-not $line.StartsWith($prefix, [StringComparison]::Ordinal)) {
            throw "malformed manifest: expected a '$key' line, found '$line'"
        }
        $value = $line.Substring($prefix.Length)
        if (-not (Test-ManifestWord $value)) {
            throw "malformed manifest: '$key' is '$value', which is not one printable word"
        }
        $values[$key] = $value
    }
    foreach ($key in @('protocol', 'members')) {
        if ($values[$key] -cnotmatch '^[0-9]+$') { throw "malformed manifest: $key is '$($values[$key])', not a number" }
    }
    $count = [uint64] $values['members']

    $members = New-Object System.Collections.Generic.List[object]
    $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    for (; $at -lt $lines.Count; $at++) {
        $line = $lines[$at]
        if ($line -cnotmatch '^([0-9a-f]{64}) ([0-9]+) (.+)$') {
            throw "malformed manifest: '$line' is not '<sha256> <size> <name>'"
        }
        $member = [pscustomobject]@{ Name = $Matches[3]; Sha256 = $Matches[1]; Size = [uint64] $Matches[2] }
        if (-not (Test-MemberName $member.Name)) { throw "malformed manifest: '$($member.Name)' is not a member name" }
        if (-not $seen.Add($member.Name)) { throw "malformed manifest: $($member.Name) is listed twice" }
        $members.Add($member)
    }
    if ([uint64] $members.Count -lt $count) {
        throw "truncated manifest: it declares $count members and holds $($members.Count)"
    }
    if ([uint64] $members.Count -gt $count) {
        throw "malformed manifest: it declares $count members and holds $($members.Count)"
    }

    return [pscustomobject]@{
        Product     = $values['product']
        Version     = $values['version']
        Arch        = $values['arch']
        ArchiveRoot = $values['archive_root']
        Protocol    = [uint32] $values['protocol']
        MinUpdater  = $values['min_updater']
        Members     = $members.ToArray()
    }
}

function Read-ReleaseManifest {
    <# The manifest `$Exe` carries, parsed. #>
    param([Parameter(Mandatory)] [string] $Exe)
    $text = Read-ReleaseManifestText -Exe $Exe
    try { return ConvertFrom-ReleaseManifest -Text $text }
    catch { throw "the release manifest in ${Exe}: $($_.Exception.Message)" }
}

function Get-ArchiveMemberList {
    <#
        The archive's members, in order, out of `archive-members.txt` beside
        this script (or `$Path`): each `Name`, `Source` and `InManifest`. The
        grammar is `bt_winres::release_manifest::parse_member_list`'s.
    #>
    param([string] $Path)

    if (-not $Path) { $Path = Join-Path $PSScriptRoot 'archive-members.txt' }
    $sources = @('exe', 'msix', 'sidecar', 'packaging', 'documents')
    $listed = New-Object System.Collections.Generic.List[object]
    $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    $number = 0
    foreach ($raw in [IO.File]::ReadAllLines($Path)) {
        $number++
        $line = $raw.Trim()
        if (-not $line -or $line.StartsWith('#')) { continue }
        $fields = @($line -split '\s+')
        if ($fields.Count -ne 2) { throw "$Path line ${number}: '$line' is not '<name> <source>'" }
        $name = $fields[0]
        $source = $fields[1]
        if (-not (Test-MemberName $name)) { throw "$Path line ${number}: '$name' is not a member name" }
        if ($sources -cnotcontains $source) { throw "$Path line ${number}: '$source' is not a source" }
        if (-not $seen.Add($name)) { throw "${Path}: $name is listed twice" }
        $listed.Add([pscustomobject]@{
                Name       = $name
                Source     = $source
                InManifest = ($source -cne 'exe' -and $source -cne 'msix')
            })
    }
    foreach ($only in @('exe', 'msix')) {
        $count = @($listed | Where-Object { $_.Source -ceq $only }).Count
        if ($count -ne 1) { throw "$Path lists $count members from '$only'; the archive has one" }
    }
    return $listed.ToArray()
}

function Compare-ReleaseMembers {
    <#
        **The refusals, in one place for both scripts.** `$Manifest` is what the
        executable says the archive holds; `$Found` is what is actually there —
        each `Name`, `Size` and `Sha256` (lower-case hex) — for every file that
        is not one of the two the manifest leaves out (`$Exempt`). Returns one
        line per disagreement, naming the member: listed and missing, present
        and unlisted, or a size or hash that differs. An empty answer is
        agreement.
    #>
    param(
        [Parameter(Mandatory)] $Manifest,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [object[]] $Found,
        [Parameter(Mandatory)] [string[]] $Exempt
    )

    $problems = New-Object System.Collections.Generic.List[string]
    $byName = @{}
    foreach ($file in $Found) { $byName[$file.Name] = $file }
    foreach ($member in $Manifest.Members) {
        if (-not $byName.ContainsKey($member.Name)) {
            $problems.Add("missing   : $($member.Name) is in the manifest and not here")
            continue
        }
        $file = $byName[$member.Name]
        if ([uint64] $file.Size -ne $member.Size) {
            $problems.Add("different : $($member.Name) is $($file.Size) bytes; the manifest says $($member.Size)")
        }
        elseif ($file.Sha256 -cne $member.Sha256) {
            $problems.Add("different : $($member.Name) hashes to $($file.Sha256); the manifest says $($member.Sha256)")
        }
    }
    $listed = @($Manifest.Members | ForEach-Object { $_.Name })
    foreach ($file in $Found) {
        if ($Exempt -ccontains $file.Name) { continue }
        if ($listed -cnotcontains $file.Name) {
            $problems.Add("unlisted  : $($file.Name) is here and not in the manifest")
        }
    }
    return $problems.ToArray()
}

function Get-StreamSha256 {
    param([Parameter(Mandatory)] [IO.Stream] $Stream)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return (-join ($hasher.ComputeHash($Stream) | ForEach-Object { $_.ToString('x2') })) }
    finally { $hasher.Dispose() }
}
