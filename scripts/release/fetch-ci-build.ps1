<#
.SYNOPSIS
    Resolve a ref and find its successful Windows release build. Print-only
    unless -Apply is supplied; -Account checks gh's login without switching it.
.DESCRIPTION
    Requires PowerShell 7 and gh. Downloads into a new directory and verifies
    every payload hash before executing --version. Never signs or builds.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateNotNullOrEmpty()] [string] $Ref,
    [Parameter(Mandatory)] [ValidateNotNullOrEmpty()] [string] $Out,
    [string] $Account,
    [ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')]
    [string] $Repository = 'lulu-loopp/folio-terminal',
    [switch] $Apply
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/ci-build.ps1"

function Invoke-GhJson {
    param([string[]] $Arguments)
    $raw = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'gh request failed (check gh auth status)' }
    return ($raw -join "`n") | ConvertFrom-Json
}

$login = (Invoke-GhJson @('api', 'user')).login
if ($Account -and $login -ne $Account) { throw "gh is signed in as $login and -Account names $Account" }
$encoded = [Uri]::EscapeDataString($Ref)
$commit = (Invoke-GhJson @('api', "repos/$Repository/commits/$encoded")).sha
if ($commit -notmatch '^[0-9a-f]{40}$') { throw 'GitHub did not resolve a full commit SHA' }
$name = "folio-windows-release-$commit"
# A dispatch's head_sha describes the workflow ref, which may differ from its
# input ref. Locate the payload by resolved SHA, then validate its producing run
# and BUILDINFO, rather than trusting that misleading head_sha.
$pages = Invoke-GhJson @('api', '--paginate', '--slurp', "repos/$Repository/actions/artifacts?name=$name&per_page=100")
$selected = $null
foreach ($artifact in @($pages | ForEach-Object { $_.artifacts } | Sort-Object id -Descending)) {
    if ($artifact.name -cne $name -or $artifact.expired) { continue }
    $run = Invoke-GhJson @('api', "repos/$Repository/actions/runs/$($artifact.workflow_run.id)")
    if ($run.path -ne '.github/workflows/build-release.yml' -or $run.status -ne 'completed' -or
        $run.conclusion -ne 'success' -or $run.event -notin @('push', 'workflow_dispatch')) { continue }
    $selected = $artifact
    break
}
if (-not $selected) { throw "No unexpired successful build-release.yml artifact for $commit; ask the coordinator to run it." }
$Out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)
if (Test-Path -LiteralPath $Out) { throw "Output already exists: $Out; use a new directory" }
Write-Host "Account: $login; commit: $commit; run: $($selected.workflow_run.id); artifact: $($selected.id)"
Write-Host "Download $name to $Out, verify SHA-256 and folio.exe --version."
if (-not $Apply) { Write-Host 'Print-only. Pass -Apply to download and verify.'; return }
# Do not replace an existing directory. Failed downloads remain for inspection,
# explicitly unverified; no receipt or success message is written on failure.
New-Item -ItemType Directory -Path $Out | Out-Null
& gh run download $selected.workflow_run.id --repo $Repository --name $name --dir $Out
if ($LASTEXITCODE -ne 0) { throw 'gh run download failed; output is UNVERIFIED' }
Assert-CiBuild -Directory $Out -Commit $commit | Out-Null
Write-Host "VERIFIED $commit in $Out (unsigned; ready for local signing and packaging)."
