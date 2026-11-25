<#
.SYNOPSIS
    Make the updater rehearsal's release feed (clean-vm.md 4.4, precondition 3):
    the successor's archive, its release statement and the statement's
    signature, and the releases.json that lists those three.

.DESCRIPTION
    A copy of Folio takes an update only on a release statement signed with a
    key it embeds (0.4.10 plan row 102), so a rehearsal's successor needs one,
    made exactly as a real release's is (row 104's ruling; no key override in
    any binary):

        copy        the successor's archive (and, with -Image, its macOS image)
                    out of the package folder into -Out, a new folder
        compose     release-statement.ps1 of the successor's checkout writes
                    -Out\RELEASE-STATEMENT.txt, of both halves with -Image and
                    of the Windows half alone without, and prints the command
                    that signs it
        sign        the key holder runs that command with the LIVE key; this
                    script takes the signature from -Signature, or waits until
                    -Out\RELEASE-STATEMENT.txt.sig appears (-WaitSeconds)
        verify      release-statement.ps1 -Signature: check-release-statement
                    reads -Out as the updater will
        list        releases.json under the statement's tag, naming the archive
                    the statement lists, the statement and its signature, at
                    file:///C:/feed/ (run-row.ps1 -Feed copies every file there)

    With -Image the statement lists both halves; the image is checked with the
    rest and then removed from -Out, since the Windows feed carries no image.
    The Mac's feed is made from the same statement by `mac/setup.sh`, which
    copies the image, the statement and the signature only.

    **A rehearsal statement names a rehearsal version.** The statement grammar
    takes three numbers and the tags `v<version>` and `v<version>-preview` only,
    so a rehearsal cannot be marked by a word; it is marked by its number
    instead: the successor's patch number is 1000 or more, which no release of
    Folio carries, and anything lower is refused here before a file is written.

    **The statement lives only in the rehearsal folder and only for the run.**
    It is never uploaded. After the last row, `make-feed.ps1 -Out <feed>
    -Remove` deletes it and its signature (and `mac/teardown.sh` the Mac's
    copies, in `<folder>/in/` and `feed/`); the guest's copy goes with the
    snapshot every row reverts to. A failure at any step after the compose -
    the wait running out, a refused check - deletes both at once and names
    -Remove for a signature written after it.

    No private key is read, written or named here.

.PARAMETER Out
    The feed folder to make; it must not exist. With -Remove, the feed folder
    whose statement is deleted.

.PARAMETER Checkout
    The successor's source checkout: its `scripts\release\release-statement.ps1`
    and `Cargo.toml` (the version the statement states) are used. Defaults to
    the checkout this script is in.

.PARAMETER Package
    The successor's package folder, holding one `folio-<version>-windows-x64.zip`
    or `folio-<version>-windows-x64-unsigned.zip`. Defaults to
    `<Checkout>\target\release-package`.

.PARAMETER Image
    The successor's `Folio-<version>-macos-arm64.dmg`, when the rehearsal has a
    macOS half: the statement then states both.

.PARAMETER CheckBinary
    `check-release-statement.exe`, passed to release-statement.ps1 (its default
    is the checkout's release target's).

.PARAMETER Signature
    The signature the key holder made over `-Out\RELEASE-STATEMENT.txt`. Without
    it, the script waits for `-Out\RELEASE-STATEMENT.txt.sig`.

.PARAMETER WaitSeconds
    How long to wait for the signature to appear. 3600 unless given.

.PARAMETER Remove
    Delete the feed's statement and its signature, after the last row.

.EXAMPLE
    ./make-feed.ps1 -Checkout <B's checkout> -Out <rehearsal>\feed-b

    Compose B's statement in <rehearsal>\feed-b, print the signing command,
    wait for the signature, check it, and list the feed.

.EXAMPLE
    ./make-feed.ps1 -Out <rehearsal>\feed-b -Remove
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Out,
    [string] $Checkout,
    [string] $Package,
    [string] $Image,
    [string] $CheckBinary,
    [string] $Signature,
    [int] $WaitSeconds = 3600,
    [switch] $Remove
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$statementName = 'RELEASE-STATEMENT.txt'
$signatureName = 'RELEASE-STATEMENT.txt.sig'
$rehearsalPatchFloor = 1000
$Out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)

# **Delete the feed's statement and its signature** - after the last row
# (-Remove), and on any failure once the statement may exist (the part 3 / 104
# review's P2-10): a rehearsal statement signed with the live key never
# outlives the run in this folder.
function Remove-FeedStatement([string] $Folder) {
    foreach ($name in @($statementName, $signatureName)) {
        $path = Join-Path $Folder $name
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
            Write-Host "deleted $path"
        }
    }
    if ((Test-Path -LiteralPath (Join-Path $Folder $statementName)) -or (Test-Path -LiteralPath (Join-Path $Folder $signatureName))) {
        throw "$Folder still holds a release statement or its signature"
    }
}

if ($Remove) {
    if (-not (Test-Path -LiteralPath $Out -PathType Container)) { throw "there is no feed folder at $Out" }
    Remove-FeedStatement $Out
    Write-Host "FEED_STATEMENT_REMOVED $Out"
    return
}

if (-not $Checkout) { $Checkout = Join-Path $PSScriptRoot '..\..\..\..' }
$Checkout = (Resolve-Path -LiteralPath $Checkout).Path
$statementScript = Join-Path $Checkout 'scripts\release\release-statement.ps1'
if (-not (Test-Path -LiteralPath $statementScript -PathType Leaf)) { throw "$Checkout has no scripts\release\release-statement.ps1" }
if (-not $Package) { $Package = Join-Path $Checkout 'target\release-package' }
if (-not (Test-Path -LiteralPath $Package -PathType Container)) { throw "there is no package folder at $Package" }
if (Test-Path -LiteralPath $Out) { throw "$Out exists; a feed is made in a new folder" }

$archivePattern = '^folio-(\d+)\.(\d+)\.(\d+)-windows-x64(-unsigned)?\.zip$'
$archives = @(Get-ChildItem -LiteralPath $Package -File | Where-Object { $_.Name -cmatch $archivePattern })
if ($archives.Count -ne 1) {
    throw "$Package holds $($archives.Count) versioned Windows archives ($(($archives | ForEach-Object { $_.Name }) -join ', ')); a feed carries one"
}
$archive = $archives[0]
$null = $archive.Name -cmatch $archivePattern
$version = '{0}.{1}.{2}' -f $Matches[1], $Matches[2], $Matches[3]
if ([int64] $Matches[3] -lt $rehearsalPatchFloor) {
    throw ("$($archive.Name) is $version, which a release may carry; a rehearsal successor's patch number is " +
           "$rehearsalPatchFloor or more, so its statement can never be taken for a release's")
}
if ($Image) {
    if (-not (Test-Path -LiteralPath $Image -PathType Leaf)) { throw "there is no image at $Image" }
    $imageName = [IO.Path]::GetFileName($Image)
    if ($imageName -cne "Folio-$version-macos-arm64.dmg") { throw "the image is $imageName; the archive's version names Folio-$version-macos-arm64.dmg" }
}

[IO.Directory]::CreateDirectory($Out) | Out-Null
Copy-Item -LiteralPath $archive.FullName -Destination (Join-Path $Out $archive.Name)
if ($Image) { Copy-Item -LiteralPath $Image -Destination (Join-Path $Out $imageName) }

$passed = @{ PackageDirectory = $Out }
if ($CheckBinary) { $passed.CheckBinary = $CheckBinary }
$composeArguments = $passed.Clone()
if (-not $Image) { $composeArguments.WindowsOnly = $true }
try {
    & $statementScript @composeArguments
    $statementPath = Join-Path $Out $statementName
    if (-not (Test-Path -LiteralPath $statementPath -PathType Leaf)) { throw "release-statement.ps1 wrote no $statementName in $Out" }

    $signaturePath = Join-Path $Out $signatureName
    if ($Signature) {
        if (-not (Test-Path -LiteralPath $Signature -PathType Leaf)) { throw "there is no signature at $Signature" }
        Copy-Item -LiteralPath $Signature -Destination $signaturePath
    }
    else {
        Write-Host ''
        Write-Host "waiting up to $WaitSeconds s for $signaturePath (the key holder signs with the live key, by the command above)"
        $deadline = [DateTime]::UtcNow.AddSeconds($WaitSeconds)
        while (-not ((Test-Path -LiteralPath $signaturePath -PathType Leaf) -and (Get-Item -LiteralPath $signaturePath).Length -eq 64)) {
            if ([DateTime]::UtcNow -ge $deadline) { throw "no 64-byte $signatureName appeared in $Out within $WaitSeconds s" }
            Start-Sleep -Milliseconds 500
        }
    }
    & $statementScript @passed -Signature $signaturePath

    # The archive is the one the statement lists, under the statement's tag.
    $lines = [IO.File]::ReadAllLines($statementPath)
    $tags = @($lines | Where-Object { $_ -cmatch '^tag ' } | ForEach-Object { $_.Substring(4) })
    if ($tags.Count -ne 1) { throw "$statementName has $($tags.Count) tag lines" }
    $listed = @($lines | Where-Object { $words = $_ -split ' '; $words.Count -eq 4 -and $words[2] -ceq 'windows-x64' } | ForEach-Object { ($_ -split ' ')[3] })
    if ($listed.Count -ne 1) { throw "$statementName has $($listed.Count) windows-x64 lines" }
    $archiveName = $listed[0]
    if (-not (Test-Path -LiteralPath (Join-Path $Out $archiveName) -PathType Leaf)) { throw "$statementName lists $archiveName, which is not in $Out" }
    if ($Image) { Remove-Item -LiteralPath (Join-Path $Out $imageName) -Force }

    $assets = foreach ($name in @($archiveName, $statementName, $signatureName)) {
        [ordered]@{
            name                 = $name
            browser_download_url = "file:///C:/feed/$name"
            size                 = (Get-Item -LiteralPath (Join-Path $Out $name)).Length
        }
    }
    $list = @([ordered]@{
            tag_name   = $tags[0]
            name       = "Folio $version"
            draft      = $false
            prerelease = $false
            assets     = @($assets)
        })
    $json = ConvertTo-Json -InputObject $list -Depth 5
    [IO.File]::WriteAllText((Join-Path $Out 'releases.json'), ($json -replace "`r`n", "`n") + "`n", (New-Object Text.UTF8Encoding $false))
}
catch {
    $failure = $_
    Remove-FeedStatement $Out
    throw ("$($failure.Exception.Message) - the statement and its signature are deleted from $Out; " +
           "a signature written there after this is deleted by make-feed.ps1 -Out `"$Out`" -Remove, " +
           'and a new feed is made in a new folder')
}

Get-ChildItem -LiteralPath $Out | ForEach-Object { '{0}  {1,10}  {2}' -f (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant(), $_.Length, $_.Name }
Get-Content -LiteralPath (Join-Path $Out 'releases.json')
if ($Image) {
    Write-Host "the Mac's feed: scp $Image, $statementPath and $signaturePath to <folder>/in/ on the Mac, then"
    Write-Host "  setup.sh <A.dmg> <folder>/in/$imageName <folder>/in/$statementName <folder>/in/$signatureName"
}
Write-Host "after the last row: make-feed.ps1 -Out `"$Out`" -Remove (the statement is never uploaded)"
Write-Host "FEED_DONE $Out"
