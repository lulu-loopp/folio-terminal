# The gate a PSReadLine pin change passes (T-INTEGRATION-INJECT-2-BUILD; the design note
# docs/plans/design/integration-inject-2-2026-10-15.md, section 3.5).
#
# Folio's Windows PowerShell panes load Microsoft's PSReadLine 2.4.5, taken out of the vendored
# PowerShell Gallery package by bt-app's build script. The build script refuses a package or an
# entry whose SHA-256 is not its pin, which is the gate every build passes. This script holds the
# three places a pin is written to one another, and asserts what only Windows can say:
#
#   1. The package file's SHA-256 equals vendor/psreadline/README.md's and the Rust table's
#      (crates/bt-app/src/psreadline_artefact.rs, PACKAGE_SHA256).
#   2. Every entry the table takes is in the package with its pinned SHA-256 and in the README's
#      table with the same one, and every entry of the package is either taken or named as not
#      taken (NOT_TAKEN in the same file's tests).
#   3. On Windows, each DLL the table takes has a Valid Authenticode signature whose signer subject
#      is Microsoft Corporation.
#   4. Folio's manifest (assets/psreadline/PSReadLine.psd1) names the table's VERSION as its
#      ModuleVersion and the assembly as its RootModule, and has no NestedModules,
#      FormatsToProcess, ScriptsToProcess or TypesToProcess and no .psm1 anywhere: the module is
#      binary-only, which is what lets it load under every execution policy.
#   5. The licence the notices reproduce (vendor/psreadline/LICENSE-PSREADLINE.txt) is the
#      package's own License.txt, byte for byte.
#
# Every refusal names the file it is about. It reads; it writes nothing but a temporary copy of
# the DLLs it asks Windows about, which it removes. scripts/ci/check-psreadline-artefact-tests.ps1
# plants each refusal in a scratch copy.

#
# -ListInputs prints the files it reads, one `label<TAB>path` line each, and reads none of them:
# the tests script copies them by label, so the gate is the one place their names are written.

param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path,
    [switch]$ListInputs
)

$ErrorActionPreference = "Stop"

$inputs = [ordered]@{
    table    = "crates/bt-app/src/psreadline_artefact.rs"
    readme   = "vendor/psreadline/README.md"
    package  = "vendor/psreadline/PSReadLine.2.4.5.nupkg"
    manifest = "assets/psreadline/PSReadLine.psd1"
    licence  = "vendor/psreadline/LICENSE-PSREADLINE.txt"
}
if ($ListInputs) {
    foreach ($label in $inputs.Keys) { Write-Output "$label`t$($inputs[$label])" }
    exit 0
}

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$failures = [System.Collections.Generic.List[string]]::new()
function Refuse([string]$Line) { $failures.Add($Line) | Out-Null }

$tableFile = Join-Path $Root $inputs.table
$readmeFile = Join-Path $Root $inputs.readme
$manifestFile = Join-Path $Root $inputs.manifest
$licenceFile = Join-Path $Root $inputs.licence
foreach ($file in @($tableFile, $readmeFile, $manifestFile, $licenceFile)) {
    if (-not (Test-Path -LiteralPath $file)) { throw "check-psreadline-artefact: $file is missing" }
}
$table = [IO.File]::ReadAllText($tableFile)
$readme = [IO.File]::ReadAllText($readmeFile)

function One([string]$Text, [string]$Pattern, [string]$What) {
    $found = [regex]::Matches($Text, $Pattern)
    if ($found.Count -ne 1) {
        throw "check-psreadline-artefact: $What is written $($found.Count) times, wanted once"
    }
    return $found[0].Groups[1].Value
}

$package = One $table 'pub const PACKAGE: &str = "([^"]+)";' "the table's PACKAGE"
$pinned = One $table 'pub const PACKAGE_SHA256: &str = "([0-9a-f]{64})";' "the table's PACKAGE_SHA256"
$version = One $table 'pub const VERSION: &str = "([^"]+)";' "the table's VERSION"
$taken = @([regex]::Matches($table, 'entry: "([^"]+)",\s*sha256: "([0-9a-f]{64})"') |
    ForEach-Object { [pscustomobject]@{ Entry = $_.Groups[1].Value; Sha256 = $_.Groups[2].Value } })
if ($taken.Count -eq 0) { throw "check-psreadline-artefact: the table in $tableFile takes no entry" }
$notTakenBlock = One $table '(?s)const NOT_TAKEN: \[&str; \d+\] = \[(.*?)\];' "the table's NOT_TAKEN"
$notTaken = @([regex]::Matches($notTakenBlock, '"([^"]+)"') | ForEach-Object { $_.Groups[1].Value })

$packageFile = Join-Path $Root $inputs.package
if ((Split-Path -Leaf $packageFile) -ne $package) {
    Refuse "${tableFile}: PACKAGE is $package, but the vendored package is $(Split-Path -Leaf $packageFile)"
}
if (-not (Test-Path -LiteralPath $packageFile)) { throw "check-psreadline-artefact: $packageFile is missing" }
$actual = (Get-FileHash -LiteralPath $packageFile -Algorithm SHA256).Hash.ToLowerInvariant()

# 1. The package against the README and the table.
$readmePin = One $readme 'Package SHA-256: `([0-9a-f]{64})`' "the README's package SHA-256"
if ($actual -ne $pinned) {
    Refuse "${tableFile}: PACKAGE_SHA256 is $pinned, but $packageFile hashes to $actual"
}
if ($readmePin -ne $actual) {
    Refuse "${readmeFile}: the package SHA-256 is $readmePin, but $packageFile hashes to $actual"
}

# 2. The entries against the package and the README's table.
$archive = [IO.Compression.ZipFile]::OpenRead($packageFile)
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("folio-psreadline-artefact-" + [guid]::NewGuid())
try {
    $names = @($archive.Entries | ForEach-Object { $_.FullName })
    foreach ($file in $taken) {
        $entry = $archive.GetEntry($file.Entry)
        if ($null -eq $entry) {
            Refuse "${packageFile}: the table takes $($file.Entry), which the package does not hold"
            continue
        }
        $stream = $entry.Open()
        try {
            $sha = [Security.Cryptography.SHA256]::Create()
            $hash = ([BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLowerInvariant()
        } finally {
            $stream.Dispose()
        }
        if ($hash -ne $file.Sha256) {
            Refuse "${tableFile}: $($file.Entry) is pinned to $($file.Sha256), but the package's hashes to $hash"
        }
        $row = [regex]::Matches($readme, '\| `' + [regex]::Escape($file.Entry) + '` \| `([0-9a-f]{64})` \|')
        if ($file.Entry -eq "License.txt") {
            $licence = (Get-FileHash -LiteralPath $licenceFile -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($licence -ne $hash) {
                Refuse "${licenceFile}: hashes to $licence, but the package's License.txt hashes to $hash"
            }
        }
        if ($row.Count -ne 1) {
            Refuse "${readmeFile}: $($file.Entry) has $($row.Count) rows in the entry table, wanted one"
        } elseif ($row[0].Groups[1].Value -ne $file.Sha256) {
            Refuse "${readmeFile}: $($file.Entry) is $($row[0].Groups[1].Value) there and $($file.Sha256) in the table"
        }
        if ($file.Entry.EndsWith(".dll") -and $IsWindows -ne $false) {
            $target = Join-Path $scratch ($file.Entry -replace '/', '\')
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    }
    foreach ($name in $names) {
        $isTaken = @($taken | Where-Object { $_.Entry -ceq $name }).Count -gt 0
        $isNamed = $notTaken -ccontains $name
        if ($isTaken -and $isNamed) {
            Refuse "${tableFile}: $name is both taken and named as not taken"
        } elseif (-not $isTaken -and -not $isNamed) {
            Refuse "${packageFile}: $name is in the package and the table neither takes it nor names it as not taken"
        }
    }
    foreach ($name in $notTaken) {
        if ($names -cnotcontains $name) {
            Refuse "${tableFile}: NOT_TAKEN names $name, which the package does not hold"
        }
    }

    # 3. The signatures, on Windows.
    if ($IsWindows -ne $false) {
        foreach ($file in @($taken | Where-Object { $_.Entry.EndsWith(".dll") })) {
            $target = Join-Path $scratch ($file.Entry -replace '/', '\')
            if (-not (Test-Path -LiteralPath $target)) { continue }
            $signature = Get-AuthenticodeSignature -LiteralPath $target
            $subject = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { "" }
            if ($signature.Status -ne "Valid") {
                Refuse "${packageFile}: $($file.Entry)'s Authenticode signature is $($signature.Status), wanted Valid"
            }
            if ($subject -notmatch '^CN=Microsoft Corporation,') {
                Refuse "${packageFile}: $($file.Entry) is signed by '$subject', wanted Microsoft Corporation"
            }
        }
    } else {
        Write-Host "check-psreadline-artefact: not Windows - the Authenticode signatures were not asked"
    }
} finally {
    $archive.Dispose()
    if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}

# 4. Folio's manifest. Read as data (Import-PowerShellDataFile runs nothing), and as text for the
#    one property a data read cannot see: a .psm1 named anywhere.
$manifest = Import-PowerShellDataFile -LiteralPath $manifestFile
if ($manifest.ModuleVersion -ne $version) {
    Refuse "${manifestFile}: ModuleVersion is '$($manifest.ModuleVersion)', but the table's VERSION is '$version'"
}
$assembly = $taken[0].Entry
if ($manifest.RootModule -ne $assembly) {
    Refuse "${manifestFile}: RootModule is '$($manifest.RootModule)', wanted the assembly '$assembly'"
}
foreach ($key in @("NestedModules", "FormatsToProcess", "ScriptsToProcess", "TypesToProcess")) {
    if ($manifest.ContainsKey($key)) {
        Refuse "${manifestFile}: names $key, which a binary-only module does not have"
    }
}
$code = ([IO.File]::ReadAllLines($manifestFile) | Where-Object { -not $_.TrimStart().StartsWith("#") }) -join "`n"
if ($code -match '\.psm1') {
    Refuse "${manifestFile}: names a .psm1, which execution policy governs"
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { Write-Host "REFUSED $failure" }
    Write-Host "check-psreadline-artefact: $($failures.Count) refusal(s)"
    exit 1
}
Write-Host "check-psreadline-artefact: PASS - $package $actual, $($taken.Count) entries pinned, version $version"
exit 0
