<#
.SYNOPSIS
    What `smoke.ps1` does with the paths it is given, asserted without building,
    signing or starting anything.

.DESCRIPTION
    `smoke.ps1`'s real work needs a `folio.exe` that was built, and on a signed
    run one that was signed; none of that is here. What is here is the step
    before all of it — turning `-Exe`, `-Artifacts` and `-Msix` into paths the
    rest of the script can agree about — because that step failed silently once
    and cost a packaging run.

    **The failure this file exists for.** A relative path has two answers on
    Windows. PowerShell resolves one against `$PWD`; .NET resolves it against
    the process's own current directory, which `Set-Location` never moves. A
    shell started in one checkout and moved into another therefore has
    `Test-Path` find a file that `[System.IO.Compression.ZipFile]::OpenRead`
    later cannot, and the message names a folder nobody typed. That is what
    `-Msix target\release-package\folio.msix` — the line `docs/RELEASING.md`
    tells people to run — did on the 0.2.1 packaging run.

    So every case here reproduces that divergence rather than describing it: the
    child is started with the repository as its process directory and then
    `Set-Location`s into a scratch folder, which is the arrangement the bug
    needs. A case passes only when the path `smoke.ps1` names is the one the
    shell was standing in. A child simply launched in the scratch folder would
    have the two agree, and would pass whether or not anything was resolved.

    Each case runs `smoke.ps1` as a child process and reads its exit code and
    its message, for the reason `sign-tests.ps1` gives: the exit code is what a
    workflow reads, and a script that prints a refusal and exits 0 is the
    failure worth catching. Nothing here starts `folio.exe` — every case is
    stopped by an argument check, before the first window.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Most cases are about a non-zero exit code, so a non-zero exit code has to be a
# value this script reads rather than an error PowerShell raises on its own.
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
if (-not $here -and $PSCommandPath) { $here = Split-Path -Parent $PSCommandPath }
if (-not $here) { throw 'smoke-tests.ps1 cannot tell where it is; run it as a file' }
$smoke = Join-Path $here 'smoke.ps1'
if (-not (Test-Path -LiteralPath $smoke -PathType Leaf)) { throw "there is no smoke.ps1 beside $PSCommandPath" }
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path

$pwsh = @(Get-Command -Name pwsh -CommandType Application -ErrorAction SilentlyContinue)
if ($pwsh.Count -eq 0) { throw 'these cases run smoke.ps1 in a child pwsh, and there is no pwsh on the path' }
$pwsh = $pwsh[0].Source

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-smoke-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
[IO.Directory]::CreateDirectory((Join-Path $scratch 'pkg')) | Out-Null

# A file under the name `smoke.ps1` is looking for, so the cases that have to
# get past the "is it there" gate can. It is never started: each of them is
# refused by a later argument check first.
#
# **It is a real PE image with a byte added to it**, and not a text file with an
# `.exe` on the end, for the same reason `sign-tests.ps1` reaches for the
# hosting process's own image: `Get-AuthenticodeSignature` handed something that
# is not a PE at all comes back with nothing to read, and a case that then
# asserts on the refusal is asserting on a stumble rather than on an answer. A
# copy of this shell with one byte on the end is a file Windows will read and
# report as tampered with, which is an answer, and it is still never run.
$stubExe = Join-Path $scratch 'pkg\folio.exe'
Copy-Item -LiteralPath ([Diagnostics.Process]::GetCurrentProcess().MainModule.FileName) -Destination $stubExe
[IO.File]::AppendAllText($stubExe, '.')

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

function Invoke-Smoke {
    param([string] $StandingIn, [hashtable] $Parameters)

    function Format-Value {
        param($Value)
        return "'" + ([string] $Value).Replace("'", "''") + "'"
    }

    $parts = @()
    foreach ($name in $Parameters.Keys) {
        $value = $Parameters[$name]
        if ($value -is [bool] -and $value) { $parts += "-$name"; continue }
        $parts += "-$name " + (Format-Value $value)
    }
    $command = "Set-Location $(Format-Value $StandingIn); & $(Format-Value $smoke) $($parts -join ' ')"

    # The child inherits the *process* directory of this one, which is what the
    # fixture needs to be the repository rather than the scratch folder.
    $saved = [Environment]::CurrentDirectory
    [Environment]::CurrentDirectory = $root
    try {
        $output = & $pwsh -NoLogo -NoProfile -Command $command 2>&1
        $text = ($output | Out-String)
        return [pscustomobject]@{
            ExitCode = $LASTEXITCODE
            Text     = $text
            # The same text put back into one line: a message inside a
            # PowerShell error report is re-wrapped to the width of whatever
            # console printed it, with a `|` gutter on each continuation, so a
            # sentence this file looks for arrives broken in a different place
            # on every machine.
            Flat     = ((($text -split "`n" | ForEach-Object { $_ -replace '^\s*\|\s?', '' }) -join ' ') -replace '\s+', ' ')
        }
    }
    finally { [Environment]::CurrentDirectory = $saved }
}

Write-Host ''
Write-Host 'smoke.ps1, on the paths it is handed:'

Test-Case 'a relative -Exe is read from where the shell is standing' {
    $result = Invoke-Smoke -StandingIn $scratch -Parameters @{ Exe = 'nowhere\folio.exe' }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    $wanted = Join-Path $scratch 'nowhere\folio.exe'
    if ($result.Flat -notmatch [regex]::Escape($wanted)) {
        throw "it did not name $wanted : $($result.Text)"
    }
    if ($result.Flat -match [regex]::Escape((Join-Path $root 'nowhere'))) {
        throw 'it looked in the repository, which is where the process started and not where the shell was'
    }
}

Test-Case 'a relative -Exe that is there is made absolute too' {
    # This one gets past "is it there" and is refused by the signature instead,
    # which is the message that then has to carry the resolved path.
    #
    # Naming the absolute path is not enough to ask for on its own: a reader
    # further down resolves the path itself and can put an absolute spelling
    # into its own error. So what is asserted is that the *only* spelling in
    # the report is the absolute one — with every occurrence of it struck out,
    # nothing relative is left to have been carried this far.
    $result = Invoke-Smoke -StandingIn $scratch -Parameters @{ Exe = 'pkg\folio.exe'; ExpectSigned = $true }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch [regex]::Escape($stubExe)) {
        throw "the refusal named something other than $stubExe : $($result.Text)"
    }
    $withoutAbsolute = $result.Flat -replace [regex]::Escape($stubExe), ''
    if ($withoutAbsolute -match [regex]::Escape('pkg\folio.exe')) {
        throw "the path was carried on as it was typed: $($result.Text)"
    }
}

Test-Case 'a relative -Msix is read from where the shell is standing' {
    $result = Invoke-Smoke -StandingIn $scratch -Parameters @{ Exe = $stubExe; Msix = 'nowhere\folio.msix' }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    $wanted = Join-Path $scratch 'nowhere\folio.msix'
    if ($result.Flat -notmatch [regex]::Escape($wanted)) {
        throw "it did not name $wanted : $($result.Text)"
    }
    if ($result.Flat -match [regex]::Escape((Join-Path $root 'nowhere'))) {
        throw 'it looked for the package in the repository rather than where the shell was'
    }
}

Test-Case 'a -Msix that was named and is not there stops the run at the door' {
    # Whether the checks that read the package are switched on is a separate
    # question: a path somebody typed either exists or the run stops, and it
    # stops before a window is opened rather than after.
    $result = Invoke-Smoke -StandingIn $scratch -Parameters @{ Exe = $stubExe; Msix = 'nowhere\folio.msix' }
    if ($result.Flat -notmatch '-Msix names') { throw "the refusal was something else: $($result.Text)" }
}

Test-Case 'an absolute path is passed through as it was written' {
    $absent = Join-Path $scratch 'absent\folio.exe'
    $result = Invoke-Smoke -StandingIn $scratch -Parameters @{ Exe = $absent }
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch [regex]::Escape($absent)) {
        throw "an absolute path came back changed: $($result.Text)"
    }
}

Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
