<#
.SYNOPSIS
    Sign files with the certificate Microsoft's Artifact Signing service issues,
    and refuse to hand back a file it did not actually sign.

.DESCRIPTION
    There is no private key here and there is not going to be one. Artifact
    Signing — the service that used to be called Trusted Signing — keeps the key,
    issues a certificate that lives for **three days**, and signs on request for
    whoever holds the `Artifact Signing Certificate Profile Signer` role on the
    certificate profile. `signtool.exe` reaches it through a signing library —
    `/dlib` — that Microsoft ships in a NuGet package, and tells that library
    which account and which profile to ask through a small JSON file — `/dmdf`.

    Three consequences run through everything below.

      * **The time stamp is not optional.** A certificate that expires in three
        days signs nothing that outlives the week unless the signature carries a
        countersigned time stamp saying it was made while the certificate was
        alive. So `/tr` is always passed, and the verification afterwards refuses
        a signature with no time stamper on it — a signature that verifies today
        and stops verifying on Thursday is the failure this check exists to
        catch, and it cannot be caught on Thursday.

      * **Nothing secret is configured here.** The three things this needs — the
        endpoint, the account, the profile — are names of public resources, and
        they are the defaults below. Who is allowed to use them is decided by
        Azure against a sign-in this script never sees: `DefaultAzureCredential`
        inside the library finds an `az login` session, or the `AZURE_*`
        environment variables of a service principal, on its own. This script
        does not sign anybody in, does not read a token, and does not write one.

      * **The tools are fetched, not committed.** The signing library is 14 MB of
        Microsoft's bytes with Microsoft's own version number on it. It is cached
        under `%LOCALAPPDATA%` and re-fetched by version, so a checkout carries
        the instruction and not the payload.

    What gets signed is decided by the caller. `package.ps1 -Sign` signs the one
    file in the archive that is ours; the two ConPTY files beside it are
    Microsoft's, already signed by Microsoft, and re-signing them would replace
    that fact with a weaker one. They are checked with `-VerifyOnly` instead.

.PARAMETER Files
    The files to sign. Each must exist and must be a PE image — a signature on
    anything else is a request this cannot make and an error the service would
    have to explain from three network hops away.

.PARAMETER OutDir
    Sign copies placed here rather than the files themselves. Nothing that was
    passed in is written to. Use it to sign a build you want to keep an unsigned
    copy of.

.PARAMETER VerifyOnly
    Do not sign; only check the signature each file already carries. Needs no
    Azure sign-in, because it asks Windows and not the service.

.PARAMETER DryRun
    Resolve the tools, write the metadata, say which credential the library is
    going to find, ask that credential for the two tokens a real run needs, print
    the command that would run, and stop before running it. Everything that can
    be wrong before the service is asked for a signature — a wrong endpoint, a
    wrong profile name, the wrong architecture of signing library, an Azure CLI
    that is installed but not on the PATH, a sign-in that has quietly expired —
    is visible in the output of this. A dry run reports the token and does not
    refuse on it: it answers questions, it does not decide anything.

.PARAMETER TimeoutSeconds
    How long `signtool` is given to answer before it is killed and the run fails.
    Defaults to 180. A signature that is going to be made is made in seconds;
    what runs longer is a `signtool` waiting on something it will never be given,
    and that is the failure this bounds. See `Invoke-SignTool`.

.PARAMETER Endpoint
    The regional service endpoint. Defaults to `FOLIO_SIGN_ENDPOINT`, then to
    East US, which is where the account is. The endpoint's region must be the
    account's region: a mismatch is a 403 and not a redirect.

.PARAMETER Account
    The Artifact Signing account. Defaults to `FOLIO_SIGN_ACCOUNT`, then
    `folio-sign`.

.PARAMETER CertificateProfile
    The certificate profile to sign with. Defaults to `FOLIO_SIGN_PROFILE`, then
    `folio-public` — the Public Trust profile, whose certificates chain to a root
    Windows already trusts. Named `CertificateProfile` and not `Profile` because
    `$Profile` is PowerShell's own variable for the file it runs at startup.

.PARAMETER SignTool
    A specific `signtool.exe`. Defaults to the newest x64 one in the Windows
    Kits, which must be at least 10.0.22621.755 — an older one does not fail
    loudly, it looks in the local certificate store instead and reports that it
    found no certificate there.

.PARAMETER DlibDir
    A directory already holding `Azure.CodeSigning.Dlib.dll` and the assemblies
    beside it. Defaults to this script's own cache, which it fills from nuget.org
    when it is empty.

.PARAMETER ClientVersion
    The version of Microsoft's `Microsoft.ArtifactSigning.Client` package to
    fetch. Pinned, because the cache is keyed by it and because a signing tool
    that changes underneath a release is a release nobody can reproduce.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string[]] $Files,
    [string] $OutDir,
    [switch] $VerifyOnly,
    [switch] $DryRun,
    [string] $Endpoint,
    [string] $Account,
    [string] $CertificateProfile,
    [string] $SignTool,
    [string] $DlibDir,
    [string] $ClientVersion = '1.0.128',
    [int] $TimeoutSeconds = 180
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# **Exit codes are read here, not raised.** Both native tools this runs —
# `signtool` and `az` — are asked questions whose answer can be a non-zero exit,
# and PowerShell can be set to turn that into a terminating error before the line
# that reads it. Turned off for this script, so that "no, that signature is not
# valid" arrives as a sentence rather than as a stack trace. The setting does not
# exist on Windows PowerShell, where this is the behaviour anyway.
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

# The three names of public resources. Knowing them lets nobody sign anything, so
# they are defaults in the open rather than a file somebody has to be handed. An
# environment variable moves them for a fork or a second account without editing
# a script that is under review.
if (-not $Endpoint) {
    $Endpoint = if ($env:FOLIO_SIGN_ENDPOINT) { $env:FOLIO_SIGN_ENDPOINT } else { 'https://eus.codesigning.azure.net' }
}
if (-not $Account) {
    $Account = if ($env:FOLIO_SIGN_ACCOUNT) { $env:FOLIO_SIGN_ACCOUNT } else { 'folio-sign' }
}
if (-not $CertificateProfile) {
    $CertificateProfile = if ($env:FOLIO_SIGN_PROFILE) { $env:FOLIO_SIGN_PROFILE } else { 'folio-public' }
}

# Microsoft's own RFC 3161 time stamping service. It is the counterpart of the
# three-day certificate and not a preference; see the note at the top.
$TimestampUrl = 'http://timestamp.acs.microsoft.com'

# The oldest `signtool.exe` that can load the signing library at all.
$MinimumSignTool = [version] '10.0.22621.755'

# The two tokens a real run needs, asked for before `signtool` is started rather
# than by `signtool` where nobody can see the answer. See `Assert-AzureToken`.
#
# Two and not one, because they fail apart. The first is what the signing library
# asks for and is the token the signature is actually made with. The second is
# the one an ordinary Azure sign-in is refreshed against, and it is here so that
# an expired sign-in and a missing role read differently: neither token means the
# sign-in is gone, only the first means the sign-in is fine and the role is not.
$TokenScopes = @(
    [pscustomobject]@{ Name = 'the signing service';    Scope = 'https://codesigning.azure.net/.default' }
    [pscustomobject]@{ Name = 'Azure Resource Manager'; Scope = 'https://management.azure.com/.default' }
)

# ── the files ────────────────────────────────────────────────────────────────

# The file extensions of the one container that carries a signature without
# being a PE image.
#
# A package's signature lives in `AppxSignature.p7x` inside the archive rather
# than in a certificate table, so the two-byte test below cannot be the only one:
# an `.msix` opens with `PK`, and so does every other zip on the machine. The
# extension is checked *as well as* the magic, so that a `.zip` handed here by
# mistake is still refused where the mistake was made — which is the whole
# purpose of this function.
$PackageExtensions = @('.msix', '.msixbundle', '.appx', '.appxbundle')

function Test-Signable {
    param([string] $Path)

    $stream = [IO.File]::OpenRead($Path)
    try {
        $head = New-Object byte[] 2
        if ($stream.Read($head, 0, 2) -ne 2) { return $false }
        if ($head[0] -eq 0x4D -and $head[1] -eq 0x5A) { return $true }  # 'MZ'
        if ($head[0] -ne 0x50 -or $head[1] -ne 0x4B) { return $false }  # 'PK'
        return $PackageExtensions -contains [IO.Path]::GetExtension($Path).ToLowerInvariant()
    }
    finally { $stream.Dispose() }
}

$sources = @()
foreach ($file in $Files) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "there is no file at $file" }
    $path = (Resolve-Path -LiteralPath $file).Path
    # A signature goes into a PE image's own certificate table, or into an
    # `AppxSignature.p7x` inside a package. Handing this a `.md` is a mistake
    # refused where it was made.
    if (-not (Test-Signable -Path $path)) {
        throw "$path is not a PE image (no MZ header) and not an MSIX package; only executables, libraries and packages can carry a signature"
    }
    $sources += $path
}

$targets = $sources
if ($OutDir) {
    [IO.Directory]::CreateDirectory($OutDir) | Out-Null
    $out = (Resolve-Path -LiteralPath $OutDir).Path
    # Two files of the same name would sign one copy twice and report both.
    $names = @($sources | ForEach-Object { [IO.Path]::GetFileName($_) })
    $clashes = @($names | Group-Object | Where-Object { $_.Count -gt 1 })
    if ($clashes.Count -gt 0) {
        throw "two of the files given are named $($clashes[0].Name); they cannot both be copied into $out"
    }
    $targets = @()
    foreach ($source in $sources) {
        $copy = Join-Path $out ([IO.Path]::GetFileName($source))
        if ($copy -eq $source) { throw "$source is already in $out; signing it there would not be signing a copy" }
        Copy-Item -LiteralPath $source -Destination $copy -Force
        $targets += $copy
    }
    Write-Host "signing copies in $out; the files given are untouched."
}

# ── verification, which is also the whole of -VerifyOnly ─────────────────────

function Assert-Signature {
    param([string] $Path, [string] $Tool)

    # Windows' own answer first, because it is the one that distinguishes "no
    # signature" from "a signature that does not chain" from "a signature with no
    # time stamp on it", and an exit code does not.
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid') {
        throw "$([IO.Path]::GetFileName($Path)): Windows calls the signature $($signature.Status) — $($signature.StatusMessage)"
    }
    if (-not $signature.TimeStamperCertificate) {
        throw ("$([IO.Path]::GetFileName($Path)): the signature carries no time stamp. " +
               'An Artifact Signing certificate is valid for three days, so an untimestamped ' +
               'signature stops verifying three days from now.')
    }

    # And then the tool that decides at install time, with `/pa` — the
    # Authenticode policy an ordinary program is judged by, rather than the
    # driver policy `verify` defaults to.
    $output = & $Tool verify /pa /v /tw $Path 2>&1
    if ($LASTEXITCODE -ne 0) {
        $output | ForEach-Object { Write-Host "  $_" }
        throw "$([IO.Path]::GetFileName($Path)): signtool verify exited $LASTEXITCODE"
    }

    Write-Host ("{0}`n    signed by  {1}`n    stamped by {2}" -f
        [IO.Path]::GetFileName($Path),
        $signature.SignerCertificate.Subject,
        $signature.TimeStamperCertificate.Subject)
}

# ── signtool ─────────────────────────────────────────────────────────────────

function Get-FileVersionNumber {
    param([string] $Path)

    $info = (Get-Item -LiteralPath $Path).VersionInfo
    return [version] ('{0}.{1}.{2}.{3}' -f
        $info.FileMajorPart, $info.FileMinorPart, $info.FileBuildPart, $info.FilePrivatePart)
}

function Find-SignTool {
    if ($SignTool) {
        if (-not (Test-Path -LiteralPath $SignTool -PathType Leaf)) { throw "there is no signtool.exe at $SignTool" }
        return (Resolve-Path -LiteralPath $SignTool).Path
    }

    # The x64 one: the signing library is loaded into signtool's own process, and
    # the library and the process have to be the same architecture.
    $roots = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "$env:ProgramFiles\Windows Kits\10\bin")
    $candidates = @()
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        foreach ($sdk in (Get-ChildItem -LiteralPath $root -Directory)) {
            $exe = Join-Path (Join-Path $sdk.FullName 'x64') 'signtool.exe'
            if (Test-Path -LiteralPath $exe -PathType Leaf) { $candidates += $exe }
        }
    }
    if ($candidates.Count -eq 0) {
        throw ('no x64 signtool.exe under any Windows Kit. Install the Windows SDK — the ' +
               'signing tools feature is enough — or pass -SignTool.')
    }

    $newest = $candidates |
        Sort-Object -Property @{ Expression = { Get-FileVersionNumber -Path $_ } } -Descending |
        Select-Object -First 1
    $version = Get-FileVersionNumber -Path $newest
    if ($version -lt $MinimumSignTool) {
        throw ("the newest signtool.exe here is $version ($newest); the signing library needs " +
               "$MinimumSignTool or newer. An older one does not fail — it looks in the local " +
               'certificate store instead and reports that it found no certificate there.')
    }
    Write-Host "signtool: $newest ($version)"
    return $newest
}

# ── the signing library ──────────────────────────────────────────────────────

function Find-Dlib {
    if ($DlibDir) {
        $named = Join-Path $DlibDir 'Azure.CodeSigning.Dlib.dll'
        if (-not (Test-Path -LiteralPath $named -PathType Leaf)) {
            throw "there is no Azure.CodeSigning.Dlib.dll in $DlibDir"
        }
        return (Resolve-Path -LiteralPath $named).Path
    }

    $cache = Join-Path (Join-Path (Join-Path $env:LOCALAPPDATA 'Folio') 'artifact-signing') $ClientVersion
    $bin = Join-Path (Join-Path $cache 'bin') 'x64'
    $dll = Join-Path $bin 'Azure.CodeSigning.Dlib.dll'
    if (Test-Path -LiteralPath $dll -PathType Leaf) {
        Write-Host "signing library: $dll (Microsoft.ArtifactSigning.Client $ClientVersion, cached)"
        return $dll
    }

    # Fetched into a directory of its own and moved into place whole, so that an
    # interrupted download leaves a directory nothing looks in rather than a
    # cache that answers "present" with half a package in it.
    $url = 'https://api.nuget.org/v3-flatcontainer/microsoft.artifactsigning.client/' +
           "$ClientVersion/microsoft.artifactsigning.client.$ClientVersion.nupkg"
    Write-Host "fetching Microsoft.ArtifactSigning.Client $ClientVersion from nuget.org"
    $staging = "$cache.incoming"
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    [IO.Directory]::CreateDirectory($staging) | Out-Null
    $nupkg = Join-Path $staging 'package.nupkg'
    Invoke-WebRequest -Uri $url -OutFile $nupkg -UseBasicParsing

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($nupkg)
    try {
        # Only `bin/x64/`. The rest of the package is the x86 copy of the same
        # thing, a licence and a picture, and the x86 copy is 7 MB this cannot
        # load.
        $wanted = @($zip.Entries |
            Where-Object { $_.FullName.StartsWith('bin/x64/') -and -not $_.FullName.EndsWith('/') })
        if ($wanted.Count -eq 0) {
            throw "the package has no bin/x64/ directory; is $ClientVersion a version that exists?"
        }
        $into = Join-Path (Join-Path $staging 'bin') 'x64'
        [IO.Directory]::CreateDirectory($into) | Out-Null
        foreach ($entry in $wanted) {
            [IO.Compression.ZipFileExtensions]::ExtractToFile(
                $entry, (Join-Path $into ([IO.Path]::GetFileName($entry.FullName))), $true)
        }
    }
    finally { $zip.Dispose() }
    Remove-Item -LiteralPath $nupkg -Force

    if (Test-Path -LiteralPath $cache) { Remove-Item -LiteralPath $cache -Recurse -Force }
    [IO.Directory]::CreateDirectory((Split-Path -Parent $cache)) | Out-Null
    Move-Item -LiteralPath $staging -Destination $cache

    if (-not (Test-Path -LiteralPath $dll -PathType Leaf)) { throw "the package did not contain $dll" }
    Write-Host "signing library: $dll (Microsoft.ArtifactSigning.Client $ClientVersion, fetched)"
    return $dll
}

# The library is a .NET 8 assembly hosted inside signtool's native process, and a
# missing runtime is the failure Microsoft's troubleshooting page describes as
# "no error code, signing just fails". Said here instead.
function Assert-DotnetRuntime {
    $shared = Join-Path (Join-Path $env:ProgramFiles 'dotnet') 'shared\Microsoft.NETCore.App'
    $installed = @()
    if (Test-Path -LiteralPath $shared -PathType Container) {
        $installed = @(Get-ChildItem -LiteralPath $shared -Directory | ForEach-Object {
            $parsed = $null
            if ([version]::TryParse($_.Name.Split('-')[0], [ref] $parsed)) { $parsed }
        })
    }
    if (-not ($installed | Where-Object { $_.Major -ge 8 })) {
        throw ('the signing library is a .NET 8 assembly and no .NET runtime of 8.0 or newer is ' +
               "installed under $shared. The x64 .NET Runtime is at " +
               'https://dotnet.microsoft.com/download/dotnet .')
    }
}

# ── who Azure will think is asking ───────────────────────────────────────────

# **`az` has to be on the PATH, not merely on the disk.** The credential the
# library ends up using runs `az` as a program by name, from inside `signtool`'s
# process — so a machine where the Azure CLI is installed but its directory is
# not on this shell's PATH is a machine where the sign-in exists, this script
# could find it by looking, and `signtool` cannot. What happens then is not an
# error: the run stops on a prompt nobody is watching, and the release waits until
# somebody kills it. It happened on the first real signature this script made,
# minutes after the CLI was installed and before any shell had a PATH with it in.
#
# So the directory is put in front of this process's PATH, which every child
# inherits, and the CLI is found the same way twice rather than probed at one
# path and called from another.
function Resolve-AzureCli {
    $onPath = @(Get-Command -Name az -CommandType Application, ExternalScript -ErrorAction SilentlyContinue)
    if ($onPath.Count -gt 0) { return $onPath[0].Source }

    $roots = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ }
    foreach ($root in $roots) {
        $directory = Join-Path $root 'Microsoft SDKs\Azure\CLI2\wbin'
        $az = Join-Path $directory 'az.cmd'
        if (-not (Test-Path -LiteralPath $az -PathType Leaf)) { continue }
        $env:PATH = $directory + [IO.Path]::PathSeparator + $env:PATH

        # **Asked again by name, rather than assumed to have worked.** The whole
        # point of the line above is that a child process can find `az` without
        # being told where it is, and the only statement of that is the name
        # resolving. A repair that set a variable and changed nothing would
        # otherwise be reported as a repair.
        $byName = @(Get-Command -Name az -CommandType Application, ExternalScript -ErrorAction SilentlyContinue)
        if ($byName.Count -eq 0) {
            throw "$az is on the disk, but az still does not resolve by name after $directory was put in front of PATH"
        }
        Write-Host "azure cli: $az"
        Write-Host '  its directory was not on PATH; it is in front of PATH for this run, because'
        Write-Host '  the signing library runs az by name and waits forever when it cannot find it.'
        Write-Host "  az now resolves by name to $($byName[0].Source)"
        return $byName[0].Source
    }
    return $null
}

# What `DefaultAzureCredential` is going to find, decided before `signtool` is
# started rather than discovered by watching it not finish. The chain also tries
# managed identity, Visual Studio and others; those are not asserted, because a
# machine that has one of them signs successfully and a check for them would only
# be in the way.
function Get-CredentialPlan {
    if ($env:AZURE_TENANT_ID -and $env:AZURE_CLIENT_ID -and
        ($env:AZURE_CLIENT_SECRET -or $env:AZURE_CLIENT_CERTIFICATE_PATH)) {
        return [pscustomobject]@{ Kind = 'service principal'; Az = $null }
    }
    return [pscustomobject]@{ Kind = 'azure cli'; Az = (Resolve-AzureCli) }
}

function Show-CredentialPlan {
    param($Plan)

    if ($Plan.Kind -eq 'service principal') {
        Write-Host 'credential: the AZURE_* environment variables of a service principal'
        return
    }
    if (-not $Plan.Az) {
        Write-Host 'credential: no Azure CLI on PATH or in its standard install location'
        return
    }
    Write-Host "credential: the Azure CLI at $($Plan.Az)"
}

# Hands back what `az account show` said, so that the token check after it can
# name this machine's tenant in the command somebody has to run, rather than
# printing a placeholder for them to go and look up.
function Assert-AzureCredential {
    param($Plan)

    if ($Plan.Kind -eq 'service principal') {
        Write-Host 'credential: the AZURE_* environment variables of a service principal'
        return $null
    }

    if (-not $Plan.Az) {
        Write-Host ''
        Write-Host 'There is no Azure CLI on PATH or in its standard install location, and no'
        Write-Host 'service principal in the environment, so nothing here can authorise a signature.'
        Write-Host 'Refused now rather than started: signtool asks for a credential it cannot get and'
        Write-Host 'then waits, and a run that waits forever is worse than one that stops.'
        Write-Host ''
        Write-Host '    winget install -e --id Microsoft.AzureCLI'
        Write-Host ''
        Write-Host 'Open a new shell afterwards, or this one will still not have it on PATH.'
        Write-Host ''
        throw 'no Azure CLI and no service principal'
    }

    # Not `$account`: `$Account` is this script's own parameter, the name of the
    # Artifact Signing account, and it is declared `[string]`. PowerShell's
    # variables are case-insensitive and a type constraint outlives an
    # assignment, so a sign-in put into a variable of that name reaches the
    # caller as the string a sign-in prints as, and every property on it is gone.
    $shown = & $Plan.Az account show --output json 2>$null
    if ($LASTEXITCODE -eq 0) {
        $signIn = ($shown | Out-String | ConvertFrom-Json)
        Write-Host "credential: an Azure CLI sign-in (subscription '$($signIn.name)')"
        return $signIn
    }

    Write-Host ''
    Write-Host 'The Azure CLI is here but nothing is signed in, so the signing service has nobody'
    Write-Host 'to authorise. Sign in with the account holding the Artifact Signing Certificate'
    Write-Host "Profile Signer role on the $CertificateProfile profile, and run this again:"
    Write-Host ''
    Write-Host '    az login --scope "https://management.core.windows.net//.default"'
    Write-Host ''
    Write-Host 'That opens a browser and asks for the second factor. `--use-device-code` is not'
    Write-Host 'an alternative here: this tenant refuses the device code flow, and what it gives'
    Write-Host 'back is a sign-in that cannot get a token. Add `--tenant <id>` if the account can'
    Write-Host 'see more than one tenant.'
    Write-Host ''
    Write-Host 'If that account can see more than one subscription, select the one the'
    Write-Host "$Account account is in as well:"
    Write-Host ''
    Write-Host '    az account set --subscription <id>'
    Write-Host ''
    Write-Host 'A build with no person at it uses a service principal instead: AZURE_TENANT_ID,'
    Write-Host 'AZURE_CLIENT_ID and AZURE_CLIENT_SECRET are read as a sign-in too.'
    Write-Host ''
    throw 'not signed in to Azure'
}

# ── and whether that credential can still get a token ────────────────────────

# **A sign-in that exists is not a sign-in that still works.** The Azure CLI
# keeps its profile on disk, so `az account show` goes on naming a subscription
# and a tenant long after the refresh token behind it has expired — which means
# the check above passes on a machine that cannot sign anything.
#
# What happens then has no error message in it. `signtool` asks the signing
# library for a credential, the library runs this same CLI, the CLI cannot
# refresh, and the tenant this project signs into refuses the device code flow —
# so what is waited on is a browser sign-in that nobody opened. `signtool` prints
# nothing at all while that goes on. On the 0.2.1 release it went on for twenty
# minutes before somebody killed it.
#
# So the token is asked for here, by this script, before `signtool` is started,
# where the answer is a sentence somebody reads.

function Get-TokenProbe {
    param($Plan)

    # A service principal is not asked. There is no CLI holding its sign-in —
    # the library reads `AZURE_*` itself — and a wrong secret there is refused in
    # milliseconds rather than waited on, because there is no prompt a build
    # could be shown.
    if ($Plan.Kind -ne 'azure cli' -or -not $Plan.Az) { return @() }

    $results = @()
    foreach ($scope in $TokenScopes) {
        # `--output none`, always: the answer to this question is a bearer token,
        # and a bearer token belongs in neither a console nor a workflow log.
        # What is read is the exit code, and what is kept is whatever the CLI
        # said on its way out.
        $said = & $Plan.Az account get-access-token --scope $scope.Scope --output none 2>&1
        $results += [pscustomobject]@{
            Name  = $scope.Name
            Scope = $scope.Scope
            Ok    = ($LASTEXITCODE -eq 0)
            Said  = (($said | Out-String).Trim())
        }
    }
    return $results
}

function Show-TokenProbe {
    param($Plan, $Results)

    if ($Plan.Kind -eq 'service principal') {
        Write-Host 'token: not asked for; the library reads the service principal itself'
        return
    }
    if (-not $Plan.Az) {
        Write-Host 'token: not asked for; there is no Azure CLI here to ask'
        return
    }
    foreach ($result in @($Results)) {
        if ($result.Ok) {
            Write-Host "token: the sign-in can still get one for $($result.Name)"
            continue
        }
        Write-Host "token: the sign-in CANNOT get one for $($result.Name) ($($result.Scope))"
        if ($result.Said) {
            $result.Said -split "`n" | ForEach-Object { Write-Host "  $($_.TrimEnd())" }
        }
    }
}

function Assert-AzureToken {
    param($Plan, $SignIn)

    $results = Get-TokenProbe -Plan $Plan
    Show-TokenProbe -Plan $Plan -Results $results

    $refused = @($results | Where-Object { -not $_.Ok })
    if ($refused.Count -eq 0) { return }

    # The tenant is read off this machine's own sign-in rather than written down
    # here, because it is the one part of that command line nobody can guess and
    # because a tenant identifier in a public script is a fact about a person.
    $tenant = if ($SignIn) { $SignIn.tenantId } else { '<the tenantId az account show prints>' }
    $names = @($refused | ForEach-Object { $_.Name }) -join ' or '

    Write-Host ''
    Write-Host 'The Azure CLI still has a profile on disk, but it can no longer get a token for'
    Write-Host "$names. The sign-in has expired."
    Write-Host ''
    Write-Host 'Refused here rather than started. signtool does not report this: it asks the'
    Write-Host 'signing library for a credential, the library asks this same CLI, and the run'
    Write-Host 'then waits on a sign-in nobody opened, printing nothing while it waits.'
    Write-Host ''
    Write-Host 'This tenant refuses the device code flow, so `az login --use-device-code` is not'
    Write-Host 'the line that fixes it. Sign in through a browser, with the second factor, and'
    Write-Host 'name the tenant and the scope:'
    Write-Host ''
    Write-Host '    az logout'
    Write-Host "    az login --tenant $tenant --scope `"https://management.core.windows.net//.default`""
    Write-Host ''
    Write-Host 'Then run this again. If the token comes back and the service still refuses, the'
    Write-Host 'sign-in is fine and the role is not: see the 403 row in docs/RELEASING.md.'
    Write-Host ''
    throw 'the Azure sign-in cannot get a token'
}

# ── running signtool, with an end to it ──────────────────────────────────────

# **One command line, quoted once.** The line that is printed and the line that
# is run are the same string, built by the rule `CommandLineToArgvW` reads back:
# a run of backslashes is doubled only where a quote follows it, and an argument
# is wrapped only when it holds a space or a quote. So a path with a space in it
# arrives as one argument, and the command printed above the run is one that can
# be pasted.
function ConvertTo-CommandLineArgument {
    param([string] $Value)

    if ($Value -ne '' -and $Value -notmatch '[\s"]') { return $Value }

    $quoted = New-Object Text.StringBuilder
    [void] $quoted.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq [char] '\') { $backslashes++; continue }
        if ($character -eq [char] '"') {
            [void] $quoted.Append([char] '\', $backslashes * 2 + 1)
            [void] $quoted.Append([char] '"')
        }
        else {
            [void] $quoted.Append([char] '\', $backslashes)
            [void] $quoted.Append($character)
        }
        $backslashes = 0
    }
    [void] $quoted.Append([char] '\', $backslashes * 2)
    [void] $quoted.Append('"')
    return $quoted.ToString()
}

function ConvertTo-CommandLine {
    param([string[]] $Arguments)
    return (@($Arguments | ForEach-Object { ConvertTo-CommandLineArgument -Value $_ }) -join ' ')
}

# **A request that is going to be answered is answered in seconds.** What takes
# longer than that is not a slow service; it is a `signtool` stopped on something
# it is never going to be given — the sign-in prompt above, or a connection that
# was accepted and then said nothing. Neither of those ends on its own, and a
# release step whose failure looks exactly like its success-in-progress is one
# nobody can read. So it is given an end.
#
# `Process.Start` and `WaitForExit(ms)` rather than the call operator, because
# the call operator waits for as long as the child cares to take and there is no
# argument that changes that. The child is handed this process's own console
# instead of a redirected pipe, so `signtool`'s output still appears as it
# happens and still reaches a workflow log.
function Invoke-SignTool {
    param([string] $Tool, [string[]] $Arguments, [int] $Seconds)

    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Tool
    $info.Arguments = ConvertTo-CommandLine -Arguments $Arguments
    $info.WorkingDirectory = $PWD.ProviderPath
    $info.UseShellExecute = $false
    $process = [Diagnostics.Process]::Start($info)

    if ($process.WaitForExit($Seconds * 1000)) { return $process.ExitCode }

    # `az` runs as a child of `signtool`, and in the failure this exists for it
    # is the child that is holding the prompt. So the tree goes, on a runtime
    # that can kill a tree.
    $killTree = $process.GetType().GetMethod('Kill', [type[]] @([bool]))
    if ($killTree) { $killTree.Invoke($process, @($true)) } else { $process.Kill() }
    $process.WaitForExit()

    throw ("the signing service did not respond: signtool was still running $Seconds seconds after " +
           'it was started, and has been killed. Nothing was signed. A signature that is going to ' +
           'be made is made in seconds, so this is a run that was waiting for something it was ' +
           'never going to get — check that az account get-access-token still answers, and pass ' +
           '-TimeoutSeconds if a slow link really does need longer.')
}

# ── run ──────────────────────────────────────────────────────────────────────

$tool = Find-SignTool

if ($VerifyOnly) {
    foreach ($target in $targets) { Assert-Signature -Path $target -Tool $tool }
    Write-Host ''
    Write-Host "$($targets.Count) file(s) carry a valid, time-stamped signature."
    return
}

Assert-DotnetRuntime
$dlib = Find-Dlib

# Resolved with the other two tools, and before the metadata is written, because
# repairing the PATH is part of getting the tools ready rather than part of
# asking for a signature. `-DryRun` reports what it found and stops; the signing
# path below refuses on it.
$credential = Get-CredentialPlan

# The metadata file names somebody's account, so it goes to a directory of its
# own under TEMP and is removed when the run ends rather than being left in the
# tree it was run from.
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-sign-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
try {
    $metadataPath = Join-Path $scratch 'metadata.json'
    $metadata = [ordered] @{
        Endpoint               = $Endpoint
        CodeSigningAccountName = $Account
        CertificateProfileName = $CertificateProfile
    }
    $json = $metadata | ConvertTo-Json
    [IO.File]::WriteAllText($metadataPath, $json, (New-Object Text.UTF8Encoding $false))

    $arguments = @(
        'sign', '/v',
        '/fd', 'SHA256',
        '/tr', $TimestampUrl,
        '/td', 'SHA256',
        '/dlib', $dlib,
        '/dmdf', $metadataPath
    ) + $targets

    Write-Host ''
    Write-Host "metadata.json ($metadataPath):"
    $json -split "`n" | ForEach-Object { Write-Host "  $($_.TrimEnd())" }
    Write-Host ''
    # Printed so it can be read, and so it can be pasted. It is the same string
    # the run below hands to the process, quoted by the same function, so what is
    # printed is not a rendering of the command — it is the command.
    Write-Host 'command:'
    Write-Host "  `"$tool`" $(ConvertTo-CommandLine -Arguments $arguments)"
    Write-Host ''

    if ($DryRun) {
        Show-CredentialPlan -Plan $credential
        # The token as well, because an expired sign-in is the one thing on this
        # list that costs a run rather than a message, and a dry run that
        # reported everything except it would have had nothing to say about the
        # twenty minutes of silence on the 0.2.1 release. Reported, not enforced:
        # a dry run answers questions and decides nothing.
        Show-TokenProbe -Plan $credential -Results (Get-TokenProbe -Plan $credential)
        Write-Host 'dry run: nothing was signed.'
        return
    }

    $signIn = Assert-AzureCredential -Plan $credential
    Assert-AzureToken -Plan $credential -SignIn $signIn

    $code = Invoke-SignTool -Tool $tool -Arguments $arguments -Seconds $TimeoutSeconds
    if ($code -ne 0) { throw "signtool sign exited $code" }
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
foreach ($target in $targets) { Assert-Signature -Path $target -Tool $tool }
Write-Host ''
Write-Host "$($targets.Count) file(s) signed and verified."
