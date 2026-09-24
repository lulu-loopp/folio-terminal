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

    **The manifests are edited rather than regenerated.** Each one is read as it
    stands today — from its repository, or from a local copy with `-CaskFile` /
    `-ScoopFile` — and only the lines that carry a version, a URL or a hash are
    replaced, each of which must be there exactly once or nothing is printed at
    all. A cask that has grown a `zap` path and a bucket file that has grown a
    `notes` line keep them; a file this script does not recognise is a file a
    person should look at. That is `cask.sh --file`'s rule, one repository
    wider.

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
    Read the cask from this path instead of from `lulu-loopp/homebrew-folio`.

.PARAMETER ScoopFile
    Read the bucket file from this path instead of from `lulu-loopp/scoop-folio`.

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

.PARAMETER Account
    The `gh` login that `-Apply` must be signed in as.

.EXAMPLE
    ./scripts/release/update-manifests.ps1 -Version 0.4.3

    Print both manifests as they would be after the release, out of
    `target/release-package`'s two checksum files. Nothing is written.

.EXAMPLE
    ./scripts/release/update-manifests.ps1 -Version 0.4.3 -FromRelease -Apply -Account lulu-loopp
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $Version,
    [string] $Tag,
    [string] $PackageDirectory,
    [switch] $FromRelease,
    [string] $CaskFile,
    [string] $ScoopFile,
    [string] $OutDirectory,
    [switch] $Apply,
    [string] $Account,
    [string] $Repository = 'lulu-loopp/folio-terminal',
    [string] $CaskRepository = 'lulu-loopp/homebrew-folio',
    [string] $ScoopRepository = 'lulu-loopp/scoop-folio'
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
    if (-not $PackageDirectory) { $PackageDirectory = Join-Path $root 'target\release-package' }
    $PackageDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PackageDirectory)
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

# ── the two files as they stand ──────────────────────────────────────────────

function Get-Manifest {
    param([string] $Local, [string] $Repo, [string] $Path)

    if ($Local) {
        $Local = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Local)
        if (-not (Test-Path -LiteralPath $Local -PathType Leaf)) { throw "no manifest at $Local" }
        return [pscustomobject]@{
            Text   = [System.IO.File]::ReadAllText($Local)
            Blob   = $null
            Source = $Local
        }
    }
    $encoded = & gh api "repos/$Repo/contents/$Path" --jq '.content'
    if ($LASTEXITCODE -ne 0) { throw "gh api exited $LASTEXITCODE reading $Repo/$Path" }
    $blob = & gh api "repos/$Repo/contents/$Path" --jq '.sha'
    if ($LASTEXITCODE -ne 0) { throw "gh api exited $LASTEXITCODE reading $Repo/$Path" }
    $bytes = [Convert]::FromBase64String(($encoded -join '').Trim())
    return [pscustomobject]@{
        Text   = [System.Text.Encoding]::UTF8.GetString($bytes)
        Blob   = $blob.Trim()
        Source = "$Repo/$Path"
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

$cask = Get-Manifest -Local $CaskFile -Repo $CaskRepository -Path $caskName
$caskText = $cask.Text
$caskText = Set-Once -Text $caskText -Pattern '(?m)^([ \t]*version )"[^"]*"' `
    -Replacement "`${1}`"$Version`"" -What 'version' -Where $cask.Source
$caskText = Set-Once -Text $caskText -Pattern '(?m)^([ \t]*sha256 )"[^"]*"' `
    -Replacement "`${1}`"$imageHash`"" -What 'sha256' -Where $cask.Source

# ── the bucket file: four values, three of them rendered from its own templates

$scoop = Get-Manifest -Local $ScoopFile -Repo $ScoopRepository -Path $scoopName
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

Show-File -Name $caskName -Source $cask.Source -Before $cask.Text -After $caskText
Show-File -Name $scoopName -Source $scoop.Source -Before $scoop.Text -After $scoopText

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
# One commit per repository, over the blob this run read, so a file somebody
# edited in between is a refused write rather than a lost edit — `gh api` is
# handed the `sha` that came back with the contents and GitHub answers 409 if it
# has moved.

$login = (& gh api user --jq '.login')
if ($LASTEXITCODE -ne 0) { throw 'gh is not signed in; -Apply has nothing to write with' }
$login = $login.Trim()
if ($Account -and $login -ne $Account) {
    throw "gh is signed in as $login and -Account names $Account"
}
foreach ($repo in @($CaskRepository, $ScoopRepository)) {
    $push = (& gh api "repos/$repo" --jq '.permissions.push')
    if ($LASTEXITCODE -ne 0) { throw "gh api exited $LASTEXITCODE reading $repo" }
    if ($push.Trim() -ne 'true') {
        throw "$login may not push to $repo, so this would be a write that is refused halfway"
    }
}
Write-Host ''
Write-Host "applying as $login"

function Publish-Manifest {
    param([string] $Repo, [string] $Path, [string] $Text, [string] $Blob)

    if (-not $Blob) {
        throw ("$Repo/$Path was read from a local file, so there is no blob to write over. " +
               'Run without -CaskFile / -ScoopFile to apply.')
    }
    $content = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($Text))
    $arguments = @(
        'api', '--method', 'PUT', "repos/$Repo/contents/$Path",
        '-f', "message=Folio $Version",
        '-f', "content=$content",
        '-f', "sha=$Blob",
        '--jq', '.commit.html_url')
    $url = & gh @arguments
    if ($LASTEXITCODE -ne 0) { throw "gh api exited $LASTEXITCODE writing $Repo/$Path" }
    Write-Host "$Repo/$Path — $url"
}

Publish-Manifest -Repo $CaskRepository -Path $caskName -Text $caskText -Blob $cask.Blob
Publish-Manifest -Repo $ScoopRepository -Path $scoopName -Text $scoopText -Blob $scoop.Blob
Write-Host ''
Write-Host 'check them: brew fetch --cask lulu-loopp/folio/folio, and scoop install from the bucket.'
