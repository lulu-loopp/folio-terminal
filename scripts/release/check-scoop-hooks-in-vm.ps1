<#
.SYNOPSIS
    The scoop hooks, installed: the marker after install and after update, the
    uninstall stopped while Folio runs, the cleanup on uninstall and only there.
    For a Windows test VM with scoop, never for a machine somebody works on.

.DESCRIPTION
    0.4.6 ticket U-2. `packaging/scoop/folio.json` is rendered with a local
    `file:///` URL to the archive given, installed with scoop, and taken through
    the four things the hooks promise (docs/RELEASING.md, "Distribution
    manifests"):

    1. `scoop install`: `post_install` wrote `folio-install.json` into the
       version folder, and a Folio started from it says in `diagnostics.log`
       that it is managed by scoop with an uninstall hook.
    2. `scoop uninstall` while that Folio runs: the cleanup door answers 2, the
       hook throws, and scoop stops — the app folder and a planted update
       entrance are still there (E-16).
    3. `scoop update folio --force` with Folio closed: `pre_uninstall` runs but
       the door does not (the planted entrance is still there), and
       `post_install` wrote the marker into the new version folder.
    4. `scoop uninstall` with Folio closed: the door ran (its lines are in
       scoop's output), the planted entrance is gone, and so is the app.

    The planted entrance is a `HKCU\...\Run` value named as the update's
    entrance is (`FolioUpdate-` and eight hex digits) whose program does not
    exist, which the cleanup door removes as nobody's (`logon_hook::clean`).
    It is the one mark this script makes outside scoop's folders, and step 4
    removes it; if the script stops before then, `folio --uninstall-cleanup` or
    `Remove-ItemProperty` removes it.

    The script starts one Folio and ends it by the process id it recorded. It
    changes `%APPDATA%\Folio` as any Folio run does, and runs the real cleanup,
    which removes what Folio set up outside its folder on this account.

.PARAMETER Archive
    The release archive, `folio-<version>-windows-x64.zip`, as `package.ps1`
    wrote it or the release page has it.

.PARAMETER Shell
    The PowerShell that runs scoop: `powershell` (Windows PowerShell 5.1, the
    default) or `pwsh`.

.EXAMPLE
    pwsh -NoProfile -File scripts\release\check-scoop-hooks-in-vm.ps1 -Archive C:\drop\folio-0.4.6-windows-x64.zip
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $Archive,
    [ValidateSet('powershell', 'pwsh')] [string] $Shell = 'powershell'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Archive = (Resolve-Path -LiteralPath $Archive).Path
$name = Split-Path -Leaf $Archive
if ($name -notmatch '^folio-(\d+\.\d+\.\d+)-windows-x64\.zip$') {
    throw "$name is not named as a release archive (folio-<version>-windows-x64.zip)"
}
$version = $Matches[1]

if (-not (Get-Command scoop -ErrorAction SilentlyContinue)) { throw 'scoop is not on PATH' }
$scoopRoot = if ($env:SCOOP) { $env:SCOOP } else { Join-Path $env:USERPROFILE 'scoop' }
$appRoot = Join-Path $scoopRoot 'apps\folio'
if (Test-Path -LiteralPath $appRoot) {
    throw "$appRoot exists: this check starts from a machine where scoop has no Folio"
}

$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$planted = 'FolioUpdate-00c0ffee'
$diagnostics = Join-Path $env:APPDATA 'Folio\diagnostics.log'
$work = Join-Path ([IO.Path]::GetTempPath()) "folio-scoop-hooks-$PID"
[IO.Directory]::CreateDirectory($work) | Out-Null

$failures = [System.Collections.Generic.List[string]]::new()
function Check([bool] $ok, [string] $what) {
    if ($ok) { Write-Host "ok   $what" } else { Write-Host "FAIL $what"; $failures.Add($what) }
}

# scoop in a process of its own, as a person runs it: a hook's `throw` ends
# that process, not this one.
function Invoke-Scoop([string] $arguments) {
    # Under Windows PowerShell 5.1, `2>&1` turns a native command's stderr into
    # error records, and with `$ErrorActionPreference = 'Stop'` the door's own
    # refusal line ("Folio: refused ...") would end this script instead of being
    # the evidence it is. Collect both streams as text.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $said = (& $Shell -NoProfile -ExecutionPolicy Bypass -Command "scoop $arguments" 2>&1 | ForEach-Object { "$_" }) -join "`n"
    } finally { $ErrorActionPreference = $previous }
    Write-Host $said
    return $said
}

function Test-Planted {
    $null -ne (Get-ItemProperty -LiteralPath $runKey -Name $planted -ErrorAction SilentlyContinue)
}

function Test-Marker([string] $folder) {
    $file = Join-Path $folder 'folio-install.json'
    (Test-Path -LiteralPath $file) -and
        [IO.File]::ReadAllText($file) -ceq '{"v":1,"manager":"scoop","uninstall_hook":true}'
}

# ── the manifest, rendered for this archive ─────────────────────────────────
$manifest = [IO.File]::ReadAllText((Join-Path $repo 'packaging\scoop\folio.json')) | ConvertFrom-Json
$manifest.version = $version
$manifest.architecture.'64bit'.url = ([Uri] $Archive).AbsoluteUri
$manifest.architecture.'64bit'.hash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest.architecture.'64bit'.extract_dir = "folio-$version"
$manifestPath = Join-Path $work 'folio.json'
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 20))
Write-Host "manifest: $manifestPath"

$folio = $null
try {
    # ── 1. install ────────────────────────────────────────────────────────────
    Invoke-Scoop "install `"$manifestPath`"" | Out-Null
    $versionFolder = Join-Path $appRoot $version
    Check (Test-Marker $versionFolder) "install: post_install wrote the marker into $versionFolder"

    New-ItemProperty -LiteralPath $runKey -Name $planted -PropertyType String `
        -Value '"C:\folio-u2-check-absent\folio.exe" --update-recover' -Force | Out-Null

    $before = if (Test-Path -LiteralPath $diagnostics) { (Get-Item -LiteralPath $diagnostics).Length } else { 0 }
    $folio = Start-Process -FilePath (Join-Path $appRoot 'current\folio.exe') -PassThru
    Write-Host "started Folio, pid $($folio.Id)"
    $line = $null
    $deadline = (Get-Date).AddSeconds(60)
    while (-not $line -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        if (Test-Path -LiteralPath $diagnostics) {
            $stream = [IO.File]::Open($diagnostics, 'Open', 'Read', 'ReadWrite')
            try {
                $null = $stream.Seek([Math]::Min($before, $stream.Length), 'Begin')
                $text = [IO.StreamReader]::new($stream).ReadToEnd()
            }
            finally { $stream.Dispose() }
            $line = ($text -split "`r?`n") | Where-Object { $_ -like '*install channel*' } | Select-Object -First 1
        }
    }
    if ([Version] $version -lt [Version] '0.4.6') {
        # the install-channel line is 0.4.6 work (U-1); an older archive cannot write it
        Write-Host "skip install: diagnostics.log's install-channel line needs a 0.4.6 build (this archive is $version)"
    } else {
        Check ($null -ne $line -and $line -like '*install channel managed by scoop with an uninstall hook*') `
            "install: diagnostics.log says '$line'"
    }
    # A moment more, for the data folder's claim the cleanup door asks about.
    Start-Sleep -Seconds 2

    # ── 2. uninstall while Folio runs ─────────────────────────────────────────
    $said = Invoke-Scoop 'uninstall folio'
    Check ((Test-Path -LiteralPath (Join-Path $versionFolder 'folio.exe')) -and (Test-Planted)) `
        'uninstall while Folio runs: scoop stopped, the app and the planted entrance are intact'
    Check ($said -match 'A Folio instance is running') 'uninstall while Folio runs: the door said why'

    Stop-Process -Id $folio.Id
    $folio.WaitForExit(30000) | Out-Null
    $folio = $null

    # ── 3. update ─────────────────────────────────────────────────────────────
    Invoke-Scoop 'update folio --force' | Out-Null
    Check (Test-Marker $versionFolder) 'update: post_install wrote the marker into the new version folder'
    Check (Test-Marker (Join-Path $appRoot 'current')) "update: 'current' carries the marker"
    Check (Test-Planted) 'update: the cleanup door did not run (the planted entrance is still there)'

    # ── 4. uninstall ──────────────────────────────────────────────────────────
    $said = Invoke-Scoop 'uninstall folio'
    if ([Version] $version -lt [Version] '0.4.6') {
        # the door learns the update entrance (FolioUpdate-*) in 0.4.6 (U-22); an older archive's door cannot remove it
        Write-Host "skip uninstall: the door's entrance lines and the planted entrance need a 0.4.6 build (this archive is $version)"
        Remove-ItemProperty -LiteralPath $runKey -Name $planted -ErrorAction SilentlyContinue
    } else {
        Check ($said -match 'Update entrance \(per-copy\)') 'uninstall: the door ran and scoop printed its lines'
        Check (-not (Test-Planted)) 'uninstall: the planted entrance is gone'
    }
    Check (-not (Test-Path -LiteralPath $versionFolder)) 'uninstall: the app is gone'
}
finally {
    if ($folio -and -not $folio.HasExited) { Stop-Process -Id $folio.Id }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host "$($failures.Count) check(s) failed:"
    $failures | ForEach-Object { Write-Host "  $_" }
    exit 1
}
Write-Host ''
Write-Host 'the scoop hooks hold, installed'
exit 0
