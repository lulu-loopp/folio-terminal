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
        return [pscustomobject]@{
            ExitCode = $LASTEXITCODE
            Text     = ($output | Out-String)
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
    if ($result.Text -notmatch 'there is no file at') { throw "the refusal did not name the reason: $($result.Text)" }
}

Test-Case 'a file that is not a PE image is refused before any tool is asked' {
    $result = Invoke-Sign @{ DlibDir = $stubDlib; Files = $notAnImage }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Text -notmatch 'is not a PE image') { throw "the refusal did not say what was wrong: $($result.Text)" }
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
    if ($result.Text -notmatch 'are named same\.exe') { throw "the refusal did not name the clash: $($result.Text)" }
}

Test-Case '-VerifyOnly passes a file that is signed and time stamped, and says by whom' {
    if (-not $signTool) { throw 'the earlier case found no signtool.exe to judge' }
    $result = Invoke-Sign @{ VerifyOnly = $true; Files = $signTool }
    if ($result.ExitCode -ne 0) { throw "verifying signtool.exe exited $($result.ExitCode): $($result.Text)" }
    if ($result.Text -notmatch 'signed by\s+CN=Microsoft Corporation') { throw 'it did not print the signer' }
    if ($result.Text -notmatch 'stamped by') { throw 'it did not print the time stamper' }
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

Test-Case 'signing with nobody signed in refuses early and names the command to run' {
    # Its counterpart is the case above: verification runs with no credential at
    # all. Skipped rather than passed on a machine that *is* signed in — this is
    # about the message, and there is no message when the credential is there.
    $result = Invoke-Sign @{ DlibDir = $stubDlib; Files = $anImage }
    if ($result.ExitCode -eq 0) {
        Write-Host '        (this machine is signed in to Azure; the refusal could not be provoked)'
        return
    }
    if ($result.Text -notmatch 'az login --use-device-code') { throw 'the refusal did not say what to run' }
    if ($result.Text -notmatch 'not signed in to Azure') { throw 'the refusal did not name itself' }
}

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
