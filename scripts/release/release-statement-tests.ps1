<#
.SYNOPSIS
    `release-statement.ps1` on a real package directory packed from a
    development build: the statement it writes, and what it refuses. Nothing
    is started and nothing is signed with a real key.

.DESCRIPTION
    0.4.10 plan row 102, part 2. `package.ps1` packs the build (no `-Sign`, so
    the archive is `-unsigned`), a stand-in macOS image is put beside it, and
    `release-statement.ps1` is run over the directory with the
    `check-release-statement` binary this checkout built. The cases read the
    statement back line by line against the files it states, and run each
    refusal: a signature that does not verify, a signed name on unsigned files,
    a missing macOS image, two Windows archives.

    **What these cases cannot show is a signature that verifies**, because the
    only keys the binary trusts are the release keys, which nothing here holds.
    That half rests on `bt_winres::release_statement`'s own tests (a composed
    statement signed with a test key passes the release check; the committed
    fixtures signed with the real keys verify) and on the release machine's
    run before every upload.

    What it needs: a Windows build of `bt-app` (debug is enough) whose directory
    holds `folio.exe` and the two ConPTY files, the Windows SDK's `makeappx.exe`
    and `signtool.exe`, and the check binary, built with
    `cargo build -p bt-winres --features release-key,inflate --bin check-release-statement`.

.PARAMETER Binaries
    The build directory to package from. Defaults to `target/debug`.

.PARAMETER CheckBinary
    The `check-release-statement` executable. Defaults to
    `target/debug/check-release-statement.exe`.
#>

[CmdletBinding()]
param([string] $Binaries, [string] $CheckBinary)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path
if (-not $Binaries) { $Binaries = Join-Path $root 'target\debug' }
$Binaries = (Resolve-Path -LiteralPath $Binaries).Path
if (-not $CheckBinary) { $CheckBinary = Join-Path $root 'target\debug\check-release-statement.exe' }
if (-not (Test-Path -LiteralPath $CheckBinary -PathType Leaf)) {
    throw "no check binary at $CheckBinary; build it with cargo build -p bt-winres --features release-key,inflate --bin check-release-statement"
}
$CheckBinary = (Resolve-Path -LiteralPath $CheckBinary).Path
. (Join-Path $here 'release-manifest.ps1')

$pwsh = (Get-Command -Name pwsh -CommandType Application)[0].Source
Add-Type -AssemblyName System.IO.Compression.FileSystem

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-release-statement-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null

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

function Invoke-Statement {
    param([string] $Directory, [string[]] $More = @())
    return Invoke-Child -Script 'release-statement.ps1' -Arguments (
        @('-PackageDirectory', $Directory, '-CheckBinary', $CheckBinary, '-Date', '2026-11-18') + $More)
}

# A copy of the packed directory, for a case that changes it.
function Copy-Package {
    param([string] $Name)
    $copy = Join-Path $scratch $Name
    Copy-Item -LiteralPath $package -Destination $copy -Recurse
    return $copy
}

function Get-Sha256 {
    param([string] $Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

Write-Host ''
Write-Host 'release-statement.ps1, on a package directory packed from this build:'

$manifest = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\](.*?)^\[') { throw 'Cargo.toml has no [workspace.package] table' }
if ($Matches[1] -notmatch '(?m)^\s*version\s*=\s*"(\d+\.\d+\.\d+)"') { throw '[workspace.package] declares no version' }
$version = $Matches[1]

$package = Join-Path $scratch 'package'
$packed = Invoke-Child -Script 'package.ps1' -Arguments @('-Binaries', $bin, '-Output', $package)
if ($packed.ExitCode -ne 0) { throw "package.ps1 exited $($packed.ExitCode): $($packed.Text)" }
$archiveName = "folio-$version-windows-x64-unsigned.zip"
$archive = Join-Path $package $archiveName
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw "package.ps1 left no $archiveName" }
$imageName = "Folio-$version-macos-arm64.dmg"
[IO.File]::WriteAllText((Join-Path $package $imageName), 'a stand-in for the macOS image')
$minUpdater = (Read-ReleaseManifest -Exe (Join-Path $bin 'folio.exe')).MinUpdater

$written = Invoke-Statement -Directory $package
$statementPath = Join-Path $package 'RELEASE-STATEMENT.txt'

# RED (0.4.10 plan row 102, part 2) — **the statement states the release in the
# directory**: this version and its tag, the `min_updater` the archive's own
# `folio.exe` carries, `authenticode false` for an `-unsigned` archive, both
# assets with their SHA-256 and size, and every member of the archive under its
# root folder, in LF lines that end in one. And the command it prints signs
# that file with `-rawin`, which is what Ed25519 in OpenSSL needs.
#
# MUTATION: hand `compose` `true` for the archive's `authenticode`
# (`plant-102-2-statement-claims-signed`): the line reads `authenticode true`.
Test-Case 'the_statement_states_the_packed_release' {
    if ($written.ExitCode -ne 0) { throw "release-statement.ps1 exited $($written.ExitCode): $($written.Text)" }
    $bytes = [IO.File]::ReadAllBytes($statementPath)
    if ($bytes -contains 13) { throw 'the statement holds a CR' }
    if ($bytes[-1] -ne 10) { throw 'the statement does not end in a newline' }
    $lines = [Text.Encoding]::ASCII.GetString($bytes).TrimEnd("`n") -split "`n"
    foreach ($expected in @('folio-release-statement 1', 'product folio', "version $version",
            "tag v$version-preview", 'date 2026-11-18', "min_from $minUpdater", 'authenticode false',
            'retires 0', 'assets 2', 'members windows-x64 ' )) {
        if (-not @($lines | Where-Object { $_.StartsWith($expected, [StringComparison]::Ordinal) })) {
            throw "the statement has no line '$expected': $($lines -join ' | ')"
        }
    }
    if (-not @($lines | Where-Object { $_ -cmatch '^key [0-9a-f]{16}$' })) { throw 'the statement names no key' }
    foreach ($asset in @(@('windows-x64', $archiveName), @('macos-arm64', $imageName))) {
        $path = Join-Path $package $asset[1]
        $line = "$(Get-Sha256 $path) $((Get-Item -LiteralPath $path).Length) $($asset[0]) $($asset[1])"
        if ($lines -cnotcontains $line) { throw "the statement has no line '$line'" }
    }
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $entries = @($zip.Entries | Where-Object { $_.FullName -notmatch '/$' })
    }
    finally { $zip.Dispose() }
    $count = [int](@($lines | Where-Object { $_ -cmatch '^members windows-x64 \d+$' })[0] -split ' ')[2]
    if ($count -ne $entries.Count) { throw "the statement lists $count members and the archive holds $($entries.Count)" }
    foreach ($entry in $entries) {
        $name = $entry.FullName.Substring("folio-$version/".Length)
        if (-not @($lines | Where-Object { $_ -cmatch "^[0-9a-f]{64} $($entry.Length) $([regex]::Escape($name))$" })) {
            throw "the statement has no member line for $name"
        }
    }
    if ($written.Flat -notmatch 'openssl pkeyutl -sign -inkey <the live key file> -rawin') {
        throw "the signing command was not printed: $($written.Text)"
    }
}

# RED (part 2, ruling R10) — **a signature that does not verify stops the
# release**: the check is the product's own, over the directory as it will be
# published, and a refusal is exit 1 with the check's words.
#
# MUTATION: ignore the check binary's exit code in `-Signature` mode
# (`plant-102-2-statement-check-ignored`): the bad signature is accepted.
Test-Case 'a_signature_that_does_not_verify_is_refused' {
    $copy = Copy-Package 'bad-signature'
    $signature = Join-Path $scratch 'bad.sig'
    [IO.File]::WriteAllBytes($signature, [byte[]]::new(64))
    $result = Invoke-Statement -Directory $copy -More @('-Signature', $signature)
    if ($result.ExitCode -eq 0) { throw "a signature of zeros was accepted: $($result.Text)" }
    if ($result.Flat -notmatch 'refused the release') { throw "the refusal did not say so: $($result.Text)" }
    if ($result.Flat -notmatch '(?i)signature') { throw "the check's words did not name the signature: $($result.Text)" }
}

# RED (part 2) — **a signature left from an earlier statement is removed when a
# statement is written**: it signs other bytes, and left beside the new one it
# would be uploaded as this release's.
Test-Case 'writing_a_statement_removes_an_earlier_signature' {
    $copy = Copy-Package 'stale-signature'
    [IO.File]::WriteAllBytes((Join-Path $copy 'RELEASE-STATEMENT.txt.sig'), [byte[]]::new(64))
    $result = Invoke-Statement -Directory $copy
    if ($result.ExitCode -ne 0) { throw "release-statement.ps1 exited $($result.ExitCode): $($result.Text)" }
    if (Test-Path -LiteralPath (Join-Path $copy 'RELEASE-STATEMENT.txt.sig')) { throw 'the earlier signature is still there' }
}

# RED (part 2, rulings R4 and R6) — **a signed name on files that are not signed
# is refused**: `authenticode true` is written only for an archive whose
# `folio.exe` and `folio.msix` `sign.ps1 -VerifyOnly` accepts, and no statement
# is written for one it does not.
#
# MUTATION: take the name's word for it in `release-statement.ps1`
# (`plant-102-2-statement-trusts-the-name`): the statement is written.
Test-Case 'a_signed_name_on_unsigned_files_is_refused' {
    $copy = Copy-Package 'signed-name'
    Remove-Item -LiteralPath (Join-Path $copy 'RELEASE-STATEMENT.txt')
    Rename-Item -LiteralPath (Join-Path $copy $archiveName) -NewName "folio-$version-windows-x64.zip"
    $result = Invoke-Statement -Directory $copy
    if ($result.ExitCode -eq 0) { throw "a signed name on unsigned files was stated: $($result.Text)" }
    if ($result.Flat -notmatch 'folio\.exe') { throw "the refusal did not name folio.exe: $($result.Text)" }
    if (Test-Path -LiteralPath (Join-Path $copy 'RELEASE-STATEMENT.txt')) { throw 'a statement was written anyway' }
}

# RED (part 2) — **a release states both halves**: without the macOS image the
# statement is refused, unless `-WindowsOnly` says the feed carries none, and
# then the statement lists the Windows archive alone.
Test-Case 'a_missing_macos_image_is_refused_unless_windows_only' {
    $copy = Copy-Package 'windows-only'
    Remove-Item -LiteralPath (Join-Path $copy $imageName)
    $result = Invoke-Statement -Directory $copy
    if ($result.ExitCode -eq 0) { throw "a release without its macOS image was stated: $($result.Text)" }
    if ($result.Flat -notmatch [regex]::Escape($imageName)) { throw "the refusal did not name ${imageName}: $($result.Text)" }
    $result = Invoke-Statement -Directory $copy -More @('-WindowsOnly')
    if ($result.ExitCode -ne 0) { throw "-WindowsOnly exited $($result.ExitCode): $($result.Text)" }
    $lines = [IO.File]::ReadAllText((Join-Path $copy 'RELEASE-STATEMENT.txt')) -split "`n"
    if ($lines -cnotcontains 'assets 1') { throw "-WindowsOnly did not state one asset: $($lines -join ' | ')" }
}

# RED (part 2) — **one Windows archive, or no statement**: a directory holding
# this version's archive under both spellings is a directory whose release is a
# guess.
Test-Case 'two_windows_archives_are_refused' {
    $copy = Copy-Package 'two-archives'
    Copy-Item -LiteralPath (Join-Path $copy $archiveName) -Destination (Join-Path $copy "folio-$version-windows-x64.zip")
    $result = Invoke-Statement -Directory $copy
    if ($result.ExitCode -eq 0) { throw "two archives were stated: $($result.Text)" }
    if ($result.Flat -notmatch 'holds 2 Windows archives') { throw "the refusal did not count them: $($result.Text)" }
}

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
exit 0
