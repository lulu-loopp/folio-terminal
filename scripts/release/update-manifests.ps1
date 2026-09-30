<#
.SYNOPSIS
    Print the two distribution manifests for a release — the Homebrew cask and
    the scoop bucket file — and, with `-Apply`, commit them to their taps.

.DESCRIPTION
    Four assets are published from this repository, and two other repositories
    describe them: `lulu-loopp/homebrew-folio` holds `Casks/folio.rb` and
    `lulu-loopp/scoop-folio` holds `bucket/folio.json`. Between them six values
    change at every release and nothing else does, and until 0.4.2 all six were
    typed by hand into the GitHub web editor after the release page went up. A
    hash typed by hand is the one failure mode that cannot be produced by a copy
    and can be produced by a retype, and `brew fetch --cask` and
    `scoop install` are both read by strangers.

    So: one command, and it prints. `-Apply` is the only way anything is
    written, and it is never the default — see **What `-Apply` does**.

    **Every value comes out of the checksum files the release page publishes**,
    which are the hashes taken over the bytes that were uploaded.
    `docs/RELEASING.md` says the same thing about winget's `InstallerSha256` and
    `cask.sh` says it about the cask, for the same reason: a hash recomputed
    from a second download is a hash of that download.

    **The manifests are edited rather than regenerated, and their source is this
    repository.** `packaging/homebrew/folio.rb` and `packaging/scoop/folio.json`
    are the two files as they should be published — the install marker and the
    uninstall hooks live there (0.4.6 ticket U-2) — and only the lines that
    carry a version, a URL or a hash are replaced, each of which must be there
    exactly once or nothing is printed at all. A file this script does not
    recognise is a file a person should look at. That is `cask.sh --file`'s
    rule, one repository wider. The published file is read too, from its
    repository, and what is printed is the difference from it: a line somebody
    changed in the tap or the bucket by hand shows as a line this run takes
    away, and belongs in `packaging/` if it is to stay. `-CaskFile` /
    `-ScoopFile` render some other copy instead, and read nothing from GitHub.

    **And the new values are the manifest's own.** scoop's `autoupdate` block
    already declares how the URL and the extract directory are spelled for a
    version — `.../v$version-preview/folio-$version-windows-x64.zip` — so this
    substitutes into those templates rather than writing a second copy of the
    shape. `checkver -u` does exactly that, and the release machine has no
    scoop. The cask builds its own URL out of `#{version}` and needs no URL
    line changed at all.

.PARAMETER Version
    The version being released — the workspace manifest's, with no `v` and no
    `-preview`. Three numbers, because that is what an asset's name carries.

.PARAMETER Tag
    The tag the release page is under. Defaults to `v<version>-preview`, which
    every release so far has used. It is checked against the URL the scoop
    template renders, so a release tagged some other way is refused here rather
    than published with a URL that answers 404.

.PARAMETER PackageDirectory
    Where `SHA256SUMS.txt` and `SHA256SUMS-macos.txt` are. Defaults to
    `target/release-package`, which is the directory the release page was made
    out of.

.PARAMETER FromRelease
    Fetch those two files from the release page instead, with `gh release
    download`. For a machine that no longer has the package directory — and for
    checking that what is on the page is what this would have written.

.PARAMETER CaskFile
    Render the cask from this path instead of from `packaging/homebrew/folio.rb`,
    and read nothing from `lulu-loopp/homebrew-folio`.

.PARAMETER ScoopFile
    Render the bucket file from this path instead of from
    `packaging/scoop/folio.json`, and read nothing from `lulu-loopp/scoop-folio`.

.PARAMETER OutDirectory
    Also write the two rendered files here, under their own names, so they can
    be diffed or pasted. Nothing is written anywhere else without `-Apply`.

.PARAMETER Apply
    **Commit the two files to their repositories.** Off by default and there is
    no environment variable that turns it on.

    It refuses unless `gh` reports a signed-in account with push permission on
    both repositories, and `-Account` may name which account that has to be, so
    that a machine signed in as somebody else stops before the first write
    rather than after it. No credential is read from this repository and none is
    written into it: the authorisation is whatever `gh auth status` already has.

    **`-Apply` runs before the release page is published** (`docs/RELEASING.md`,
    the release order), while the draft's assets still answer 404 to everybody
    else, so a write it cannot finish is a write it takes back:

    1. It saves, for both repositories, the exact bytes each file holds now and
       its blob id, to `manifests-before.json` in the package directory, and
       prints the path. That file is the record.
    2. It writes both files, each over the blob id it saved, and reads both
       back and compares them with what it rendered.
    3. If a write or a read-back fails, every write that landed is undone: the
       saved bytes are written over the blob id that write left, and both
       repositories are read back and compared with the saved pair. It then
       exits 1: nothing is published, and both manifests are what they were.
    4. If a write-back fails or is refused (the file moved again), or the
       repositories do not read back as the saved pair, it is a **release
       incident**: exit 3, and one line per repository saying what it holds
       now, and the record to restore from by hand. The page is not published
       either way.

    Run again, it writes only what is not there yet: if both repositories
    already hold this release's files it says so and exits 0 without touching
    the record; if one does, that one counts as a write an earlier run landed,
    and what it held before is kept from the earlier run's record.

.PARAMETER Revert
    **Put both repositories back to what a record says they held**, the same
    write-back and read-back as step 3 of `-Apply`, with the same exit 3 when it
    cannot. For a release whose manifests were applied and whose page was then
    not published — publication failed, or the release was abandoned. The
    argument is the record `-Apply` printed. A repository that already holds
    its saved file is left alone, so running it twice is the same as once.

.PARAMETER Account
    The `gh` login that `-Apply` or `-Revert` must be signed in as.

.EXAMPLE
    ./scripts/release/update-manifests.ps1 -Version 0.4.3

    Print both manifests as they would be after the release, out of
    `target/release-package`'s two checksum files. Nothing is written.

.EXAMPLE
    ./scripts/release/update-manifests.ps1 -Version 0.4.3 -FromRelease -Apply -Account lulu-loopp

.EXAMPLE
    ./scripts/release/update-manifests.ps1 -Revert target/release-package/manifests-before.json

.NOTES
    Exit codes: 0 done; 1 refused, or a failed `-Apply` whose writes were all
    put back; 3 a release incident — a repository that could not be put back.
#>

[CmdletBinding(DefaultParameterSetName = 'Render')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Render')] [string] $Version,
    [Parameter(ParameterSetName = 'Render')] [string] $Tag,
    [Parameter(ParameterSetName = 'Render')] [string] $PackageDirectory,
    [Parameter(ParameterSetName = 'Render')] [switch] $FromRelease,
    [Parameter(ParameterSetName = 'Render')] [string] $CaskFile,
    [Parameter(ParameterSetName = 'Render')] [string] $ScoopFile,
    [Parameter(ParameterSetName = 'Render')] [string] $OutDirectory,
    [Parameter(ParameterSetName = 'Render')] [switch] $Apply,
    [Parameter(Mandatory = $true, ParameterSetName = 'Revert')] [string] $Revert,
    [string] $Account,
    [Parameter(ParameterSetName = 'Render')] [string] $Repository = 'lulu-loopp/folio-terminal',
    [Parameter(ParameterSetName = 'Render')] [string] $CaskRepository = 'lulu-loopp/homebrew-folio',
    [Parameter(ParameterSetName = 'Render')] [string] $ScoopRepository = 'lulu-loopp/scoop-folio'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Two-argument `Join-Path` only: Windows PowerShell 5.1 refuses a third, and
# this script has to run on whatever shell the release machine has open.
$here = $PSScriptRoot
if (-not $here -and $PSCommandPath) { $here = Split-Path -Parent $PSCommandPath }
if (-not $here) { throw 'update-manifests.ps1 cannot tell where it is; run it as a file' }
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path

$caskName = 'Casks/folio.rb'
$scoopName = 'bucket/folio.json'

# A release incident: a repository this run wrote and could not put back.
# Distinct from 1, which every refusal and every failure that was put back ends
# with, so that whatever runs this can tell "stop" from "stop and repair".
$incidentExit = 3

# ── the two repositories, through `gh api` ───────────────────────────────────

# `gh`'s own words on stderr are kept and its exit code decides. `Continue`, in
# this function's scope only, so that a line on stderr is a line to report
# rather than — under Windows PowerShell 5.1 — a terminating error.
function Invoke-Gh {
    param([string[]] $Arguments)

    $ErrorActionPreference = 'Continue'
    $said = @(& gh @Arguments 2>&1)
    $code = $LASTEXITCODE
    $out = @($said | Where-Object { $_ -isnot [System.Management.Automation.ErrorRecord] } |
        ForEach-Object { "$_" })
    $err = @($said | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } |
        ForEach-Object { "$_".Trim() } | Where-Object { $_ })
    return [pscustomobject]@{ Exit = $code; Out = ($out -join "`n"); Error = ($err -join ' ') }
}

# A file's bytes and its blob id, out of **one** read: the two are a pair, and
# the blob id is what a later write is made over. The bytes are kept as base64,
# re-encoded, so that two reads of the same bytes compare equal as strings.
function Read-Remote {
    param([string] $Repo, [string] $Path)

    $said = Invoke-Gh @('api', "repos/$Repo/contents/$Path")
    if ($said.Exit -ne 0) { throw "gh api exited $($said.Exit) reading $Repo/${Path}: $($said.Error)" }
    $doc = $said.Out | ConvertFrom-Json
    $bytes = [Convert]::FromBase64String(($doc.content -replace '\s', ''))
    return [pscustomobject]@{ Blob = "$($doc.sha)"; Content = [Convert]::ToBase64String($bytes); Bytes = $bytes }
}

# One commit, over the blob id `$Over`: GitHub answers 409 if the file has
# moved from it, so a file somebody changed in between is a refused write rather
# than a lost edit. Answers the blob id the write left and the commit's page.
function Write-Remote {
    param([string] $Repo, [string] $Path, [string] $Content, [string] $Over, [string] $Message)

    $said = Invoke-Gh @('api', '--method', 'PUT', "repos/$Repo/contents/$Path",
        '-f', "message=$Message",
        '-f', "content=$Content",
        '-f', "sha=$Over")
    if ($said.Exit -ne 0) { throw "gh api exited $($said.Exit) writing $Repo/${Path}: $($said.Error)" }
    $doc = $said.Out | ConvertFrom-Json
    return [pscustomobject]@{ Blob = "$($doc.content.sha)"; Url = "$($doc.commit.html_url)" }
}

# The account, and its push permission on every repository, before the first
# write rather than after it.
function Assert-Writer {
    param([string[]] $Repositories)

    $said = Invoke-Gh @('api', 'user', '--jq', '.login')
    if ($said.Exit -ne 0) { throw 'gh is not signed in; there is nothing to write with' }
    $login = $said.Out.Trim()
    if ($Account -and $login -ne $Account) {
        throw "gh is signed in as $login and -Account names $Account"
    }
    foreach ($repo in $Repositories) {
        $said = Invoke-Gh @('api', "repos/$repo", '--jq', '.permissions.push')
        if ($said.Exit -ne 0) { throw "gh api exited $($said.Exit) reading ${repo}: $($said.Error)" }
        if ($said.Out.Trim() -ne 'true') {
            throw "$login may not push to $repo, so this would be a write that is refused halfway"
        }
    }
    Write-Host ''
    Write-Host "writing as $login"
}

function Save-Record {
    param($Record, [string] $Path)

    [System.IO.File]::WriteAllText($Path, ($Record | ConvertTo-Json -Depth 6) + "`n",
        [System.Text.UTF8Encoding]::new($false))
}

# What a repository holds, in the record's terms.
function Get-Holding {
    param($Entry, $Now, [string] $Version)

    if ($Now.Content -ceq $Entry.Before.Content) { return "the file it held before (blob $($Now.Blob))" }
    if ($Now.Content -ceq $Entry.Rendered) { return "Folio $Version's file (blob $($Now.Blob))" }
    return "a file that is neither the saved one nor Folio $Version's (blob $($Now.Blob))"
}

# **Every write that landed is undone**, and then both repositories are read
# back and compared with the saved pair. A write is undone over the blob id it
# left — the one its answer named, or, when the answer was lost, the one a read
# finds holding this release's bytes — so a file that moved again since is a
# 409 and an incident, never somebody else's edit written over. A repository
# that already holds its saved bytes is not written at all.
# True when both read back as the saved pair; otherwise the incident is printed.
function Restore-Manifests {
    param($Record, [string] $RecordPath)

    foreach ($entry in $Record.Repositories) {
        $where = "$($entry.Repository)/$($entry.Path)"
        try {
            $now = Read-Remote -Repo $entry.Repository -Path $entry.Path
            if ($now.Content -ceq $entry.Before.Content) {
                Write-Host "$where — holds what it held before, nothing to put back"
                continue
            }
            $over = if ($entry.Applied) { $entry.Applied }
                    elseif ($now.Content -ceq $entry.Rendered) { $now.Blob }
                    else { $null }
            if (-not $over) {
                Write-Host "$where — not put back: it holds $(Get-Holding $entry $now $Record.Version), which this run did not write"
                continue
            }
            $done = Write-Remote -Repo $entry.Repository -Path $entry.Path -Content $entry.Before.Content `
                -Over $over -Message "Folio $($Record.Version): put back, the release did not go out"
            Write-Host "$where — put back, $($done.Url)"
        }
        catch {
            Write-Host "$where — not put back: $($_.Exception.Message)"
        }
    }

    $restored = $true
    $lines = @()
    foreach ($entry in $Record.Repositories) {
        $where = "$($entry.Repository)/$($entry.Path)"
        try {
            $now = Read-Remote -Repo $entry.Repository -Path $entry.Path
            if ($now.Content -cne $entry.Before.Content) { $restored = $false }
            $lines += "  $where holds $(Get-Holding $entry $now $Record.Version)"
        }
        catch {
            $restored = $false
            $lines += "  $where could not be read: $($_.Exception.Message)"
        }
    }
    if ($restored) {
        foreach ($entry in $Record.Repositories) {
            Write-Host "$($entry.Repository)/$($entry.Path) — reads back as it was"
        }
        return $true
    }
    Write-Host ''
    Write-Host 'RELEASE INCIDENT: the manifests are not what they were, and the page must not be published.'
    foreach ($line in $lines) { Write-Host $line }
    Write-Host "  the record is ${RecordPath}: each repository's Before.Content is the file it held, as base64,"
    Write-Host '  to be written back by hand over the blob it holds now, once whatever moved it is understood.'
    return $false
}

# ── -Revert: a release whose manifests were applied and whose page was not ────

if ($PSCmdlet.ParameterSetName -eq 'Revert') {
    $recordPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Revert)
    if (-not (Test-Path -LiteralPath $recordPath -PathType Leaf)) { throw "there is no record at $recordPath" }
    $record = [System.IO.File]::ReadAllText($recordPath) | ConvertFrom-Json
    Write-Host "putting back Folio $($record.Version)'s manifests, from $recordPath"
    Assert-Writer -Repositories @($record.Repositories | ForEach-Object { $_.Repository })
    if (Restore-Manifests -Record $record -RecordPath $recordPath) {
        Write-Host ''
        Write-Host 'both manifests are back to what they were'
        exit 0
    }
    exit $incidentExit
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw ("'$Version' is not a version. It is the one in [workspace.package], with no 'v' and " +
           "no '-preview': the suffix is a release channel and lives on the tag.")
}
if (-not $Tag) { $Tag = "v$Version-preview" }

# ── the hashes, out of the files the release page publishes ──────────────────

function Get-Sum {
    param([string] $Path, [string] $Name)

    # The format both halves of a release write: the hash, two spaces, a bare
    # file name. Read as lines rather than with a regex over the whole file, so
    # a name that is a suffix of another cannot answer for it.
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].Trim() -eq $Name) {
            $hash = $parts[0].Trim().ToLowerInvariant()
            if ($hash -notmatch '^[0-9a-f]{64}$') {
                throw "$Path says the hash of $Name is '$hash', which is not a SHA-256"
            }
            return $hash
        }
    }
    throw ("$Path has no line for $Name. That file is the release page's own list, so a name " +
           'missing from it is an asset that was not published under that name.')
}

# The package directory is also where `-Apply` keeps its record, so it has a
# value under `-FromRelease` too.
if (-not $PackageDirectory) { $PackageDirectory = Join-Path $root 'target\release-package' }
$PackageDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PackageDirectory)

$sums = $null
$macSums = $null
if ($FromRelease) {
    # The two checksum *files*, fetched from the page. Not the assets and not
    # their digests: what is wanted is the claim the release already makes,
    # which is these two files, and they are a few hundred bytes each.
    $into = Join-Path ([IO.Path]::GetTempPath()) "folio-manifests-$Tag-$PID"
    [System.IO.Directory]::CreateDirectory($into) | Out-Null
    Write-Host "reading the checksum files from $Repository at $Tag"
    & gh release download $Tag --repo $Repository --dir $into --clobber `
        --pattern 'SHA256SUMS.txt' --pattern 'SHA256SUMS-macos.txt'
    if ($LASTEXITCODE -ne 0) { throw "gh release download exited $LASTEXITCODE" }
    $sums = Join-Path $into 'SHA256SUMS.txt'
    $macSums = Join-Path $into 'SHA256SUMS-macos.txt'
}
else {
    $sums = Join-Path $PackageDirectory 'SHA256SUMS.txt'
    $macSums = Join-Path $PackageDirectory 'SHA256SUMS-macos.txt'
}
foreach ($file in @($sums, $macSums)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
        throw ("there is no $file. Both halves of the release write one, and both are on the " +
               'release page; -FromRelease fetches them from there.')
    }
}

$archiveName = "folio-$Version-windows-x64.zip"
$imageName = "Folio-$Version-macos-arm64.dmg"
$archiveHash = Get-Sum -Path $sums -Name $archiveName
$imageHash = Get-Sum -Path $macSums -Name $imageName

Write-Host ''
Write-Host "Folio $Version, tagged $Tag"
Write-Host "  $archiveName  $archiveHash"
Write-Host "  $imageName  $imageHash"

# ── the two files: this repository's source, and what is published ────────────

# The text rendered is `packaging/`'s, or a local copy's. The published file is
# read beside it, for two things only: the difference this run prints, and the
# blob `-Apply` writes over. A local copy reads nothing published, so it can
# be printed and diffed but not applied.
function Get-Manifest {
    param([string] $Local, [string] $Packaged, [string] $Repo, [string] $Path)

    if ($Local) {
        $Local = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Local)
        if (-not (Test-Path -LiteralPath $Local -PathType Leaf)) { throw "no manifest at $Local" }
        return [pscustomobject]@{
            Text      = [System.IO.File]::ReadAllText($Local)
            Published = $null
            Content   = $null
            Blob      = $null
            Source    = $Local
        }
    }
    $published = Read-Remote -Repo $Repo -Path $Path
    return [pscustomobject]@{
        Text      = [System.IO.File]::ReadAllText($Packaged)
        Published = [System.Text.Encoding]::UTF8.GetString($published.Bytes)
        Content   = $published.Content
        Blob      = $published.Blob
        Source    = "$Packaged, published as $Repo/$Path"
    }
}

# **One occurrence, or nothing is printed.** A value that is in the file twice
# is a file this script does not understand, and half of an edit is worse than
# none: it would publish a manifest whose URL and whose hash are about two
# different releases.
# The one match is replaced in place rather than with `-replace`, because
# `-replace` rewrites every occurrence and the whole point of this function is
# that there is exactly one. `$LiteralReplacement` is the difference between a
# replacement that may name the groups it matched (`${1}`, for the indentation
# and the key an anchored edit keeps) and one that is a value — a URL or a hash
# — in which a `$` would otherwise be read as a group reference.
function Set-Once {
    param(
        [string] $Text,
        [string] $Pattern,
        [string] $Replacement,
        [string] $What,
        [string] $Where,
        [switch] $LiteralReplacement)

    $found = [regex]::Matches($Text, $Pattern)
    if ($found.Count -ne 1) {
        throw "$Where has $($found.Count) places declaring $What, and this edit needs exactly one"
    }
    $with = if ($LiteralReplacement) { $Replacement } else { $found[0].Result($Replacement) }
    return $Text.Remove($found[0].Index, $found[0].Length).Insert($found[0].Index, $with)
}

function Set-Literal {
    param([string] $Text, [string] $Old, [string] $New, [string] $What, [string] $Where)

    return Set-Once -Text $Text -Pattern ([regex]::Escape($Old)) -Replacement $New `
        -What $What -Where $Where -LiteralReplacement
}

# ── the cask: two lines, and the URL builds itself out of one of them ─────────

$cask = Get-Manifest -Local $CaskFile -Packaged (Join-Path $root 'packaging/homebrew/folio.rb') `
    -Repo $CaskRepository -Path $caskName
$caskText = $cask.Text
$caskText = Set-Once -Text $caskText -Pattern '(?m)^([ \t]*version )"[^"]*"' `
    -Replacement "`${1}`"$Version`"" -What 'version' -Where $cask.Source
$caskText = Set-Once -Text $caskText -Pattern '(?m)^([ \t]*sha256 )"[^"]*"' `
    -Replacement "`${1}`"$imageHash`"" -What 'sha256' -Where $cask.Source

# ── the bucket file: four values, three of them rendered from its own templates

$scoop = Get-Manifest -Local $ScoopFile -Packaged (Join-Path $root 'packaging/scoop/folio.json') `
    -Repo $ScoopRepository -Path $scoopName
$scoopText = $scoop.Text
$current = $scoopText | ConvertFrom-Json
$architecture = $current.architecture.'64bit'
$template = $current.autoupdate.architecture.'64bit'

# `$version` is scoop's own placeholder and is substituted literally, not by
# PowerShell: these strings come out of a JSON document and nothing in them is
# an expression.
$newUrl = $template.url.Replace('$version', $Version)
$newExtract = $template.extract_dir.Replace('$version', $Version)

# The tag is the one thing the templates cannot check about themselves: it is
# written into the URL as `v$version-preview`, and a release tagged otherwise
# would be described by a manifest pointing at a page that is not there.
if ($newUrl -notmatch [regex]::Escape("/download/$Tag/")) {
    throw ("the bucket file's autoupdate URL renders as $newUrl, which does not name the tag " +
           "$Tag. The tag and the manifest are one claim; change the template in " +
           "$($scoop.Source), once, rather than this release's copy of it.")
}
if ($newUrl -notmatch [regex]::Escape("/$archiveName")) {
    throw "the bucket file's autoupdate URL renders as $newUrl, which does not name $archiveName"
}

$scoopText = Set-Once -Text $scoopText -Pattern '(?m)^([ \t]*"version":[ \t]*)"[^"]*"' `
    -Replacement "`${1}`"$Version`"" -What 'version' -Where $scoop.Source
# The URL first and the extract directory second: the old directory name is a
# prefix of the old archive's name, and the quotes are what tell them apart.
$scoopText = Set-Literal -Text $scoopText -Old $architecture.url -New $newUrl `
    -What 'the 64-bit download URL' -Where $scoop.Source
$scoopText = Set-Literal -Text $scoopText -Old $architecture.hash -New $archiveHash `
    -What 'the 64-bit hash' -Where $scoop.Source
$scoopText = Set-Literal -Text $scoopText -Old "`"$($architecture.extract_dir)`"" `
    -New "`"$newExtract`"" -What 'the 64-bit extract_dir' -Where $scoop.Source

# Read back as a document, because a manifest that no longer parses is the one
# failure a line-by-line edit can produce.
$rendered = $scoopText | ConvertFrom-Json
if ($rendered.version -ne $Version -or
    $rendered.architecture.'64bit'.hash -ne $archiveHash -or
    $rendered.architecture.'64bit'.url -ne $newUrl -or
    $rendered.architecture.'64bit'.extract_dir -ne $newExtract) {
    throw 'the bucket file was edited into something that does not read back as this release'
}

# ── what it prints ───────────────────────────────────────────────────────────

function Show-File {
    param([string] $Name, [string] $Source, [string] $Before, [string] $After)

    Write-Host ''
    Write-Host "══ $Name — from $Source"
    if ($Before -ceq $After) {
        Write-Host '   (unchanged: this release is already what it says)'
    }
    else {
        $old = $Before -split "`r?`n"
        $new = $After -split "`r?`n"
        for ($i = 0; $i -lt [Math]::Max($old.Count, $new.Count); $i++) {
            $a = if ($i -lt $old.Count) { $old[$i] } else { $null }
            $b = if ($i -lt $new.Count) { $new[$i] } else { $null }
            if ($a -cne $b) {
                if ($null -ne $a) { Write-Host "   - $a" }
                if ($null -ne $b) { Write-Host "   + $b" }
            }
        }
    }
    Write-Host ''
    Write-Host $After
}

# The difference printed is from what is published, where it was read: that is
# what the release changes for a reader of the tap or the bucket.
foreach ($shown in @(
        @($caskName, $cask, $caskText),
        @($scoopName, $scoop, $scoopText))) {
    $before = if ($null -ne $shown[1].Published) { $shown[1].Published } else { $shown[1].Text }
    Show-File -Name $shown[0] -Source $shown[1].Source -Before $before -After $shown[2]
}

if ($OutDirectory) {
    $OutDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutDirectory)
    [System.IO.Directory]::CreateDirectory($OutDirectory) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $OutDirectory 'folio.rb'), $caskText)
    [System.IO.File]::WriteAllText((Join-Path $OutDirectory 'folio.json'), $scoopText)
    Write-Host ''
    Write-Host "written to $OutDirectory"
}

if (-not $Apply) {
    Write-Host ''
    Write-Host 'nothing was written to either repository. -Apply, from an account that may push to'
    Write-Host 'both, is the only thing that commits this.'
    return
}

# ── what `-Apply` does ───────────────────────────────────────────────────────
#
# Save, write, read back; and on any failure, put back. See `-Apply` above and
# `docs/RELEASING.md`'s release order: this runs before the page is published,
# so a public state it changes is one it must be able to return.

foreach ($read in @($cask, $scoop)) {
    if (-not $read.Blob) {
        throw ("$($read.Source) is a local file, so there is no published file to write over or to " +
               'put back. Run without -CaskFile / -ScoopFile to apply.')
    }
}
[System.IO.Directory]::CreateDirectory($PackageDirectory) | Out-Null
$recordPath = Join-Path $PackageDirectory 'manifests-before.json'

# **Running `-Apply` again is safe.** A repository that already holds this
# release's file holds a write an earlier run landed. Both: nothing to do, and
# the record — the only place the files from before this release are kept — is
# not touched. One: that one is recorded as landed, with the file from before
# taken from the earlier run's record, so `-Revert` still puts back what the
# repository held before the release rather than this release's own file.
$caskRendered = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($caskText))
$scoopRendered = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($scoopText))
if ($cask.Content -ceq $caskRendered -and $scoop.Content -ceq $scoopRendered) {
    Write-Host ''
    Write-Host "both repositories already hold Folio $Version's manifests: already applied; the record is left as it is"
    exit 0
}
Assert-Writer -Repositories @($CaskRepository, $ScoopRepository)

$earlier = $null
if (Test-Path -LiteralPath $recordPath -PathType Leaf) {
    $earlier = [System.IO.File]::ReadAllText($recordPath) | ConvertFrom-Json
    if ($earlier.Version -ne $Version) { $earlier = $null }
}

function New-Entry {
    param([string] $Repo, [string] $Path, $Read, [string] $Rendered)

    $entry = [pscustomobject]@{
        Repository = $Repo
        Path       = $Path
        Before     = [pscustomobject]@{ Blob = $Read.Blob; Content = $Read.Content }
        Rendered   = $Rendered
        Applied    = $null
    }
    if ($Read.Content -cne $Rendered) { return $entry }

    $entry.Applied = $Read.Blob
    $kept = $null
    if ($earlier) {
        $kept = @($earlier.Repositories | Where-Object { $_.Repository -eq $Repo -and $_.Path -eq $Path }) |
            Select-Object -First 1
    }
    if ($kept) {
        $entry.Before = [pscustomobject]@{ Blob = $kept.Before.Blob; Content = $kept.Before.Content }
        Write-Host "$Repo/$Path — already holds Folio $Version's file (blob $($Read.Blob)); what it held before is kept from the earlier record"
    }
    else {
        Write-Host "$Repo/$Path — already holds Folio $Version's file (blob $($Read.Blob)), and no earlier record says what it held before"
    }
    return $entry
}

# 1. Save: the bytes and blob ids the difference above was printed against —
# one read each — before either write.
$record = [pscustomobject]@{
    Version      = $Version
    Tag          = $Tag
    Repositories = @(
        (New-Entry -Repo $CaskRepository -Path $caskName -Read $cask -Rendered $caskRendered),
        (New-Entry -Repo $ScoopRepository -Path $scoopName -Read $scoop -Rendered $scoopRendered))
}
Save-Record -Record $record -Path $recordPath
Write-Host "saved what both repositories hold to $recordPath"

# 2. Write both, each over its saved blob id. The blob id each write leaves
# goes into the record as it lands, so a write-back is made over exactly it.
$failed = $false
foreach ($entry in $record.Repositories) {
    $where = "$($entry.Repository)/$($entry.Path)"
    if ($entry.Applied) { continue }
    try {
        $written = Write-Remote -Repo $entry.Repository -Path $entry.Path -Content $entry.Rendered `
            -Over $entry.Before.Blob -Message "Folio $Version"
        $entry.Applied = $written.Blob
        Save-Record -Record $record -Path $recordPath
        Write-Host "$where — written, $($written.Url)"
    }
    catch {
        Write-Host "$where — not written: $($_.Exception.Message)"
        $failed = $true
        break
    }
}

# 3. Read both back.
if (-not $failed) {
    foreach ($entry in $record.Repositories) {
        $where = "$($entry.Repository)/$($entry.Path)"
        try {
            $now = Read-Remote -Repo $entry.Repository -Path $entry.Path
            if ($now.Content -ceq $entry.Rendered) {
                Write-Host "$where — reads back as written"
            }
            else {
                Write-Host "$where — reads back as $(Get-Holding $entry $now $Version), not as written"
                $failed = $true
            }
        }
        catch {
            Write-Host "$where — could not be read back: $($_.Exception.Message)"
            $failed = $true
        }
    }
}

# 4. On any failure, put back every write that landed.
if ($failed) {
    Write-Host ''
    Write-Host 'putting back every write that landed'
    if (Restore-Manifests -Record $record -RecordPath $recordPath) {
        Write-Host ''
        Write-Host 'nothing published; both manifests are back to what they were'
        exit 1
    }
    exit $incidentExit
}

Write-Host ''
Write-Host "both repositories hold Folio $Version's manifests. Publish the page next; if it is not"
Write-Host "published, put them back: update-manifests.ps1 -Revert $recordPath"
