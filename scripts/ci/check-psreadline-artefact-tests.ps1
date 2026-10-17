# Scratch-copy tests for check-psreadline-artefact.ps1 (T-INTEGRATION-INJECT-2-BUILD).
#
# The gate holds three places a pin is written to one another and Folio's manifest to the
# binary-only rule. Each case moves one of them alone in a scratch copy of the four files the gate
# reads and requires the gate to refuse it, naming the file that moved; the untouched copy passes.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-psreadline-artefact.ps1"
$repository = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
# The gate names the files it reads; each case below names one of them by its label.
$inputs = @{}
foreach ($line in (& pwsh -NoProfile -File $gate -ListInputs)) {
    $label, $path = $line -split "`t", 2
    $inputs[$label] = $path
}
if ($inputs.Count -ne 4) { throw "the gate listed $($inputs.Count) inputs, wanted four" }

function New-Copy {
    $root = Join-Path ([IO.Path]::GetTempPath()) ("folio-psreadline-gate-" + [guid]::NewGuid())
    foreach ($file in $inputs.Values) {
        $target = Join-Path $root $file
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
        Copy-Item -LiteralPath (Join-Path $repository $file) -Destination $target
    }
    return $root
}

function Edit([string]$Root, [string]$Label, [string]$From, [string]$To) {
    $File = $inputs[$Label]
    $path = Join-Path $Root $File
    $text = [IO.File]::ReadAllText($path)
    if (-not $text.Contains($From)) { throw "the case's text is not in $File : $From" }
    [IO.File]::WriteAllText($path, $text.Replace($From, $To))
}

function Run-Gate([string]$Root, [int]$Want, [string]$Case, [string]$Names) {
    $output = & pwsh -NoProfile -File $gate -Root $Root 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    if ($Names -and $output -notmatch [regex]::Escape($Names)) {
        throw "$Case was refused without naming $Names`n$output"
    }
}

$pin = "cb9390e9733208456c234a7971d1ec4a917886c239502aab68f4b71aa4bba235"
$moved = "0" * 64
$cases = @(
    @{ Case = "the README's package hash moved alone"; Label = "readme";
       From = "Package SHA-256: ``$pin``"; To = "Package SHA-256: ``$moved``"; Names = "readme" },
    @{ Case = "the table's package hash moved alone"; Label = "table";
       From = "PACKAGE_SHA256: &str = `"$pin`""; To = "PACKAGE_SHA256: &str = `"$moved`""; Names = "table" },
    @{ Case = "the manifest's ModuleVersion moved alone"; Label = "manifest";
       From = "ModuleVersion = '2.4.5'"; To = "ModuleVersion = '2.4.6'"; Names = "manifest" },
    @{ Case = "a .psm1 added to the manifest"; Label = "manifest";
       From = "RootModule = 'Microsoft.PowerShell.PSReadLine.dll'"; To = "RootModule = 'PSReadLine.psm1'"; Names = "manifest" },
    @{ Case = "a FormatsToProcess line added to the manifest"; Label = "manifest";
       From = "PowerShellVersion = '5.1'"; To = "PowerShellVersion = '5.1'`nFormatsToProcess = 'PSReadLine.format.ps1xml'"; Names = "FormatsToProcess" },
    @{ Case = "an entry's pin moved alone"; Label = "table";
       From = "451994c3d3e38d939b4fa5f8594d0207840f45e7a210aa75f81aef85aa17c592"; To = $moved; Names = "Microsoft.PowerShell.Pager.dll" },
    @{ Case = "an entry the table no longer names"; Label = "table";
       From = "        `"SamplePSReadLineProfile.ps1`",`n"; To = ""; Names = "SamplePSReadLineProfile.ps1" }
)

$untouched = New-Copy
try {
    Run-Gate $untouched 0 "the untouched copy" ""
} finally {
    Remove-Item -LiteralPath $untouched -Recurse -Force
}
foreach ($case in $cases) {
    $root = New-Copy
    try {
        Edit $root $case.Label $case.From $case.To
        # A label names the file the refusal must name; anything else is a word it must say.
        $names = if ($inputs.ContainsKey($case.Names)) { Split-Path -Leaf $inputs[$case.Names] } else { $case.Names }
        Run-Gate $root 1 $case.Case $names
    } finally {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}

Write-Host "check-psreadline-artefact: the untouched copy passes and $($cases.Count) moved pins are refused by name"
exit 0
