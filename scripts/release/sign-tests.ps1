<#
.SYNOPSIS
    Everything about `sign.ps1` that can be true or false without a network and
    without anybody being signed in to Azure.

.DESCRIPTION
    The one thing `sign.ps1` does that cannot be tested here is the signing
    itself: that needs a person, a sign-in and a service. Everything around it
    can be wrong on its own, and each of those failures is quiet — a metadata
    file naming the wrong profile, an invocation missing `/tr`, an `-OutDir` that
    writes back over what it was given. So each is asserted here rather than
    noticed on a release day.

    **Every case runs `sign.ps1` as a child process and reads its exit code**,
    rather than dot-sourcing it and calling functions. The exit code is what
    `package.ps1` and a workflow read, and a script that prints a refusal and
    exits 0 is exactly the failure worth catching.

    The two fixtures are both already on the machine, so this reaches nothing:

      * a directory holding an empty `Azure.CodeSigning.Dlib.dll`, handed to
        `-DlibDir`. `-DryRun` stops before the library is loaded, so a file of no
        bytes stands in for Microsoft's 14 MB one and these cases do not have to
        reach nuget.org.
      * `signtool.exe` itself, which Microsoft ships signed and time stamped and
        which `sign.ps1` has to find anyway. The verification path is given a
        file whose signature is not in question, and then the same file with a
        byte added to it.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Most cases here are about a non-zero exit code, so a non-zero exit code has to
# be a value this script reads rather than an error PowerShell raises on its own.
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
if (-not $here -and $PSCommandPath) { $here = Split-Path -Parent $PSCommandPath }
if (-not $here) { throw 'sign-tests.ps1 cannot tell where it is; run it as a file' }
$sign = Join-Path $here 'sign.ps1'
if (-not (Test-Path -LiteralPath $sign -PathType Leaf)) { throw "there is no sign.ps1 beside $PSCommandPath" }

$pwsh = @(Get-Command -Name pwsh -CommandType Application -ErrorAction SilentlyContinue)
if ($pwsh.Count -eq 0) { throw 'these cases run sign.ps1 in a child pwsh, and there is no pwsh on the path' }
$pwsh = $pwsh[0].Source

# Whatever is hosting this script: always a real PE image, on both editions of
# PowerShell, and never something a test has to go looking for.
$anImage = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-sign-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null

$failures = New-Object System.Collections.Generic.List[string]
$ran = 0

# **A child `pwsh -Command` and not `pwsh -File`.** `-File` hands every argument
# on as one literal string, so `-Files a b` binds `a` to `-Files` and `b` to
# whatever parameter comes next positionally, and `-Files a,b` binds the single
# string "a,b". Neither is the call `package.ps1` makes. `-Command` is parsed as
# PowerShell, so a list stays a list.
function Invoke-Sign {
    param([hashtable] $Parameters, [hashtable] $Environment = @{})

    function Format-Value {
        param($Value)
        return "'" + ([string] $Value).Replace("'", "''") + "'"
    }

    $parts = @()
    foreach ($name in $Parameters.Keys) {
        $value = $Parameters[$name]
        if ($value -is [bool] -and $value) { $parts += "-$name"; continue }
        $parts += "-$name " + ((@($value) | ForEach-Object { Format-Value $_ }) -join ',')
    }
    $command = "& $(Format-Value $sign) $($parts -join ' ')"

    # Set for the child alone: two cases below are about what `FOLIO_SIGN_*`
    # does, and a test that left them set would decide the ones after it.
    $saved = @{}
    foreach ($name in $Environment.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, $Environment[$name])
    }
    try {
        $output = & $pwsh -NoLogo -NoProfile -Command $command 2>&1
        $text = ($output | Out-String)
        return [pscustomobject]@{
            ExitCode = $LASTEXITCODE
            Text     = $text
            # **The same text put back into one line.** A message inside a
            # PowerShell error report is re-wrapped to the width of whatever
            # console it is printed on, and each continuation is given a `|`
            # gutter — so a sentence this file looks for arrives broken in a
            # different place on every machine, with a bar in the break.
            # Dropping the gutter and flattening the blanks asks about the words
            # rather than about the terminal.
            Flat     = ((($text -split "`n" | ForEach-Object { $_ -replace '^\s*\|\s?', '' }) -join ' ') -replace '\s+', ' ')
        }
    }
    finally {
        foreach ($name in $Environment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $saved[$name])
        }
    }
}

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

function Get-Metadata {
    param([string] $Text)
    if ($Text -notmatch '(?s)metadata\.json \([^)]+\):\s*(\{.*?\})') {
        throw 'the output holds no metadata.json body'
    }
    return ($Matches[1] | ConvertFrom-Json)
}

function Get-Command-Line {
    param([string] $Text)
    if ($Text -notmatch '(?m)^\s+"[^"]+signtool\.exe" (sign .+)$') {
        throw 'the output holds no signtool command line'
    }
    return $Matches[1]
}

# ── fixtures ─────────────────────────────────────────────────────────────────

$stubDlib = Join-Path $scratch 'dlib'
[IO.Directory]::CreateDirectory($stubDlib) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $stubDlib 'Azure.CodeSigning.Dlib.dll'), (New-Object byte[] 0))

$notAnImage = Join-Path $scratch 'notes.md'
[IO.File]::WriteAllText($notAnImage, "not a program`n")

Write-Host 'sign.ps1'

Test-Case 'a file that is not there is refused, and the exit code says so' {
    $result = Invoke-Sign @{ DlibDir = $stubDlib; Files = (Join-Path $scratch 'absent.exe') }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'there is no file at') { throw "the refusal did not name the reason: $($result.Text)" }
}

Test-Case 'a file that is not a PE image is refused before any tool is asked' {
    $result = Invoke-Sign @{ DlibDir = $stubDlib; Files = $notAnImage }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'is not a PE image') { throw "the refusal did not say what was wrong: $($result.Text)" }
}

# The signtool `sign.ps1` chose, read out of its own output. The verification
# cases below judge that same file, so they are asking about the tool that is
# about to be used rather than one that happens to be lying around.
$signTool = $null
Test-Case 'the plan names the signtool.exe it chose, and it is a new enough one' {
    $result = Invoke-Sign @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode): $($result.Text)" }
    if ($result.Text -notmatch '(?m)^signtool: (.+) \(([0-9.]+)\)\s*$') {
        throw "the output does not name a signtool.exe and its version: $($result.Text)"
    }
    $script:signTool = $Matches[1]
    if ([version] $Matches[2] -lt [version] '10.0.22621.755') {
        throw "it chose $($Matches[2]), which cannot load the signing library"
    }
}

Test-Case 'the metadata is the three keys the service reads, with this account and profile' {
    $result = Invoke-Sign @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode)" }
    $metadata = Get-Metadata -Text $result.Text
    $keys = @($metadata.PSObject.Properties.Name | Sort-Object)
    $wanted = @('CertificateProfileName', 'CodeSigningAccountName', 'Endpoint')
    if (@(Compare-Object $wanted $keys).Count -ne 0) { throw "the metadata's keys are $($keys -join ', ')" }
    if ($metadata.Endpoint -ne 'https://eus.codesigning.azure.net') { throw "endpoint is $($metadata.Endpoint)" }
    if ($metadata.CodeSigningAccountName -ne 'folio-sign') { throw "account is $($metadata.CodeSigningAccountName)" }
    if ($metadata.CertificateProfileName -ne 'folio-public') { throw "profile is $($metadata.CertificateProfileName)" }
}

Test-Case 'the command carries SHA-256 both ways, a time stamp, the library and the metadata' {
    $result = Invoke-Sign @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode)" }
    $command = Get-Command-Line -Text $result.Text
    foreach ($flag in @('/fd SHA256', '/td SHA256', '/tr http://timestamp.acs.microsoft.com', '/dlib ', '/dmdf ')) {
        if ($command -notmatch [regex]::Escape($flag)) { throw "the command has no $flag : $command" }
    }
    if ($command -notmatch 'Azure\.CodeSigning\.Dlib\.dll') { throw 'the command does not name the signing library' }
    if ($command -notmatch 'metadata\.json') { throw 'the command does not name a metadata file' }
}

Test-Case 'the three names move with FOLIO_SIGN_ENDPOINT, _ACCOUNT and _PROFILE' {
    $result = Invoke-Sign -Parameters @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage } -Environment @{
        FOLIO_SIGN_ENDPOINT = 'https://weu.codesigning.azure.net'
        FOLIO_SIGN_ACCOUNT  = 'another-account'
        FOLIO_SIGN_PROFILE  = 'another-profile'
    }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode)" }
    $metadata = Get-Metadata -Text $result.Text
    if ($metadata.Endpoint -ne 'https://weu.codesigning.azure.net') { throw "endpoint is $($metadata.Endpoint)" }
    if ($metadata.CodeSigningAccountName -ne 'another-account') { throw "account is $($metadata.CodeSigningAccountName)" }
    if ($metadata.CertificateProfileName -ne 'another-profile') { throw "profile is $($metadata.CertificateProfileName)" }
}

Test-Case 'a parameter beats the environment variable for the same name' {
    $result = Invoke-Sign -Parameters @{
        DryRun = $true; DlibDir = $stubDlib; Files = $anImage; CertificateProfile = 'from-the-parameter'
    } -Environment @{ FOLIO_SIGN_PROFILE = 'from-the-environment' }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode)" }
    $metadata = Get-Metadata -Text $result.Text
    if ($metadata.CertificateProfileName -ne 'from-the-parameter') {
        throw "profile is $($metadata.CertificateProfileName)"
    }
}

Test-Case '-OutDir signs a copy and leaves the file it was given exactly as it was' {
    $original = Join-Path $scratch 'original.exe'
    Copy-Item -LiteralPath $anImage -Destination $original -Force
    $before = (Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash

    $out = Join-Path $scratch 'signed'
    $result = Invoke-Sign @{ DryRun = $true; DlibDir = $stubDlib; OutDir = $out; Files = $original }
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode): $($result.Text)" }

    $copy = Join-Path $out 'original.exe'
    if (-not (Test-Path -LiteralPath $copy -PathType Leaf)) { throw "nothing was copied into $out" }
    if ((Get-FileHash -LiteralPath $original -Algorithm SHA256).Hash -ne $before) {
        throw 'the file that was passed in was written to'
    }
    $command = Get-Command-Line -Text $result.Text
    if ($command -notmatch [regex]::Escape($copy)) { throw 'the command does not name the copy' }
    if ($command -match [regex]::Escape($original)) { throw 'the command names the original' }
}

Test-Case 'two files of the same name cannot both be signed into one -OutDir' {
    $one = Join-Path $scratch 'a'
    $two = Join-Path $scratch 'b'
    [IO.Directory]::CreateDirectory($one) | Out-Null
    [IO.Directory]::CreateDirectory($two) | Out-Null
    Copy-Item -LiteralPath $anImage -Destination (Join-Path $one 'same.exe') -Force
    Copy-Item -LiteralPath $anImage -Destination (Join-Path $two 'same.exe') -Force

    $result = Invoke-Sign @{
        DryRun = $true; DlibDir = $stubDlib; OutDir = (Join-Path $scratch 'both')
        Files  = @((Join-Path $one 'same.exe'), (Join-Path $two 'same.exe'))
    }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'are named same\.exe') { throw "the refusal did not name the clash: $($result.Text)" }
}

Test-Case '-VerifyOnly passes a file that is signed and time stamped, and says by whom' {
    if (-not $signTool) { throw 'the earlier case found no signtool.exe to judge' }
    $result = Invoke-Sign @{ VerifyOnly = $true; Files = $signTool }
    if ($result.ExitCode -ne 0) { throw "verifying signtool.exe exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'signed by\s+CN=Microsoft Corporation') { throw 'it did not print the signer' }
    if ($result.Flat -notmatch 'stamped by') { throw 'it did not print the time stamper' }
}

Test-Case '-VerifyOnly refuses a file whose bytes no longer match its signature' {
    if (-not $signTool) { throw 'the earlier case found no signtool.exe to copy' }
    $tampered = Join-Path $scratch 'tampered.exe'
    Copy-Item -LiteralPath $signTool -Destination $tampered -Force
    $stream = [IO.File]::Open($tampered, [IO.FileMode]::Append, [IO.FileAccess]::Write)
    try { $stream.WriteByte(0) } finally { $stream.Dispose() }

    $result = Invoke-Sign @{ VerifyOnly = $true; Files = $tampered }
    if ($result.ExitCode -eq 0) { throw 'it called a broken signature valid' }
}

# The Azure CLI's own directory, found the way `sign.ps1` finds it when the PATH
# does not name it. The three cases after this need it; on a machine without the
# CLI installed they say so instead of pretending.
$cliDirectory = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } |
    ForEach-Object { Join-Path $_ 'Microsoft SDKs\Azure\CLI2\wbin' } |
    Where-Object { Test-Path -LiteralPath (Join-Path $_ 'az.cmd') -PathType Leaf } |
    Select-Object -First 1

# A PATH with the CLI's directory taken out of it, and one with it put in.
$withoutCli = (($env:PATH -split ';' | Where-Object { $_ -and $_ -ne $cliDirectory }) -join ';')
$noCredentials = @{
    AZURE_TENANT_ID = ''; AZURE_CLIENT_ID = ''
    AZURE_CLIENT_SECRET = ''; AZURE_CLIENT_CERTIFICATE_PATH = ''
}

Test-Case 'the plan names the Azure CLI the signing library will run' {
    if (-not $cliDirectory) { Write-Host '        (no Azure CLI on this machine)'; return }
    $environment = $noCredentials.Clone()
    $environment['PATH'] = $cliDirectory + ';' + $withoutCli
    $result = Invoke-Sign -Parameters @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage } -Environment $environment
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'credential: the Azure CLI at ') { throw "it named no CLI: $($result.Text)" }
    if ($result.Flat -match 'not on PATH') { throw 'it repaired a PATH that did not need repairing' }
}

Test-Case 'an Azure CLI that is installed but not on PATH is put in front of PATH' {
    # The failure this is about is not an error message: the library runs `az` by
    # name from inside signtool, and a signtool that cannot find it stops on a
    # prompt nobody is watching. Finding it on the disk is not enough — the
    # directory has to reach the child process.
    if (-not $cliDirectory) { Write-Host '        (no Azure CLI on this machine)'; return }
    $environment = $noCredentials.Clone()
    $environment['PATH'] = $withoutCli
    $result = Invoke-Sign -Parameters @{ DryRun = $true; DlibDir = $stubDlib; Files = $anImage } -Environment $environment
    if ($result.ExitCode -ne 0) { throw "a dry run exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch [regex]::Escape((Join-Path $cliDirectory 'az.cmd'))) {
        throw "it did not find the CLI in its install location: $($result.Text)"
    }
    if ($result.Flat -notmatch 'its directory was not on PATH') { throw 'it did not say it repaired the PATH' }
    # The claim that matters is not that a variable was set but that the name
    # resolves afterwards, which is what a child of signtool will do.
    if ($result.Flat -notmatch ('az now resolves by name to ' +
            [regex]::Escape((Join-Path $cliDirectory 'az.cmd')))) {
        throw "az did not become resolvable by name: $($result.Text)"
    }
}

Test-Case 'a CLI put on PATH is then run, and a signed-out one stops the run before signtool' {
    if (-not $cliDirectory) { Write-Host '        (no Azure CLI on this machine)'; return }
    # **The repaired PATH is asked to do the thing the repair is for.** The case
    # above proves the message; this one proves the CLI is then reachable by
    # name, because `sign.ps1` goes on to run it and comes back with what it
    # said rather than with "there is no Azure CLI here".
    #
    # `AZURE_CONFIG_DIR` at an empty directory is a signed-out machine, so this
    # reads the same on a machine somebody has signed in on. Reaching for the
    # real profile instead would let the case pass by being unable to run.
    $environment = $noCredentials.Clone()
    $environment['PATH'] = $withoutCli
    $environment['AZURE_CONFIG_DIR'] = Join-Path $scratch 'empty-azure-config'
    [IO.Directory]::CreateDirectory($environment['AZURE_CONFIG_DIR']) | Out-Null

    # A copy, so that a regression which signs where it should refuse damages a
    # file in a temporary directory rather than the PowerShell this runs on.
    $subject = Join-Path $scratch 'unsigned-subject.exe'
    Copy-Item -LiteralPath $anImage -Destination $subject -Force
    $before = (Get-FileHash -LiteralPath $subject -Algorithm SHA256).Hash

    $result = Invoke-Sign -Parameters @{ DlibDir = $stubDlib; Files = $subject } -Environment $environment
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'its directory was not on PATH') { throw 'the PATH was never repaired' }
    if ($result.Flat -notmatch 'az login --use-device-code') { throw 'the refusal did not say what to run' }
    if ($result.Flat -notmatch 'not signed in to Azure') {
        throw "it did not get an answer out of the CLI it had just put on PATH: $($result.Text)"
    }
    if ((Get-FileHash -LiteralPath $subject -Algorithm SHA256).Hash -ne $before) {
        throw 'it wrote to the file it was refusing to sign'
    }
}

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
