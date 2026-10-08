# Scratch-repository tests for check-crate-edges.ps1's merge-base half: the exemption list only
# shrinks. Each case runs the gate on a scratch tree with a metadata file written here, so the
# history it compares against is the one the case builds.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-crate-edges.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-crate-edges-" + [guid]::NewGuid())
$metadata = Join-Path $root "metadata.json"
$exemptionHeader = "from`tto`tkind`tledger`treason`n"
$aToB = "bt-a`tbt-b`tnormal`tD-0`tsame layer, recorded`n"
$aToC = "bt-a`tbt-c`tnormal`tD-0`tsame layer, planted`n"

function Write-File([string]$Relative, [string]$Text) {
    $path = Join-Path $root $Relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

# Three crates on one layer; bt-a depends on each crate named.
function Write-Metadata([string[]]$From) {
    $packages = foreach ($name in @("bt-a", "bt-b", "bt-c")) {
        $dependencies = @()
        if ($name -eq "bt-a") {
            $dependencies = @($From | ForEach-Object {
                [ordered]@{ name = $_; kind = $null; target = $null; rename = $null; path = (Join-Path $root "crates/$_") }
            })
        }
        [ordered]@{
            name = $name
            id = "path+file:///$name#0.0.0"
            manifest_path = (Join-Path $root "crates/$name/Cargo.toml")
            dependencies = $dependencies
        }
    }
    $document = [ordered]@{
        packages = @($packages)
        workspace_members = @("path+file:///bt-a#0.0.0", "path+file:///bt-b#0.0.0", "path+file:///bt-c#0.0.0")
        workspace_root = $root
    }
    [IO.File]::WriteAllText($metadata, ($document | ConvertTo-Json -Depth 8))
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root -Metadata $metadata 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

# The way a `shell: pwsh` step runs a script on GitHub Actions: `pwsh -command ". '<step file>'"`,
# where the step file is the step's text between `$ErrorActionPreference = 'stop'` and a last line
# that exits with `$LASTEXITCODE`. A native command the gate ran last sets that variable, so this
# road sees an exit status that `-File` does not.
function Run-GateAsAStep([int]$Want, [string]$Case) {
    $step = Join-Path $root "step.ps1"
    [IO.File]::WriteAllText($step, (
        "`$ErrorActionPreference = 'stop'`n" +
        "& '$gate' -Repo '$root' -Metadata '$metadata'`n" +
        "if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit `$LASTEXITCODE }`n"))
    $output = & pwsh -NoProfile -command ". '$step'" 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case as a CI step returned $got, wanted $Want`n$output" }
    return $output
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email crate-edges@example.invalid
        & git config user.name crate-edges
        & git config core.autocrlf false
        Write-File "scripts/ci/crate-layers.tsv" "crate`tlayer`nbt-a`t1`nbt-b`t1`nbt-c`t1`n"
        Write-File ".gitignore" "metadata.json`nstep.ps1`n"
        & git add .
        & git commit -q -m layers
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()

        Write-Metadata @("bt-b")
        Write-File "scripts/ci/crate-edge-exemptions.tsv" ($exemptionHeader + $aToB)
        $output = Run-Gate 0 "no base list"
        if ($output -notmatch "introduces it") { throw "the no-base-list pass was not loud: $output" }
        $output = Run-GateAsAStep 0 "no base list"
        if ($output -notmatch "introduces it") { throw "the no-base-list pass as a CI step was not loud: $output" }

        & git add .
        & git commit -q -m exemptions
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Run-Gate 0 "unchanged" | Out-Null

        Write-Metadata @("bt-b", "bt-c")
        Write-File "scripts/ci/crate-edge-exemptions.tsv" ($exemptionHeader + $aToB + $aToC)
        $output = Run-Gate 1 "added row"
        if ($output -notmatch "bt-a -> bt-c" -or $output -notmatch "only shrinks") {
            throw "an added row was refused for the wrong reason: $output"
        }

        Write-Metadata @()
        Write-File "scripts/ci/crate-edge-exemptions.tsv" $exemptionHeader
        Run-Gate 0 "shrunk" | Out-Null

        Write-Metadata @("bt-b")
        Write-File "scripts/ci/crate-edge-exemptions.tsv" ($exemptionHeader + $aToB)
        & git update-ref -d refs/remotes/origin/main
        $output = Run-Gate 2 "no merge base"
        if ($output -notmatch "not a pass") { throw "the no-merge-base refusal did not say so: $output" }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-crate-edges: no-base-list (also as a CI step), unchanged, growth, shrink and no-merge-base cases pass"
exit 0
