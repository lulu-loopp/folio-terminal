# Shared, offline validation of the unsigned CI payload. BUILDINFO.txt is JSON
# so filenames and rustc's multiline output have an unambiguous representation.
Set-StrictMode -Version Latest

function Get-CiBuildVersion {
    param([string] $Exe)
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Exe
    $start.Arguments = '--version'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) {
            $process.Kill()
            throw 'folio.exe --version timed out'
        }
        if ($process.ExitCode -ne 0) { throw 'folio.exe --version failed' }
        $stderr.GetAwaiter().GetResult() | Out-Null
        return $stdout.GetAwaiter().GetResult().Trim()
    } finally { $process.Dispose() }
}

function Assert-CiBuild {
    param([string] $Directory, [string] $Commit)
    if ($Commit -notmatch '^[0-9a-f]{40}$') { throw 'expected a full commit SHA' }
    $info = Get-Content -LiteralPath (Join-Path $Directory 'BUILDINFO.txt') -Raw | ConvertFrom-Json
    if ($info.schema -ne 1 -or $info.commit -cne $Commit) { throw 'BUILDINFO commit mismatch' }
    if ($info.profile -cne 'release' -or $info.command -cne 'cargo build --release --locked -p bt-app') {
        throw 'BUILDINFO profile mismatch'
    }
    if ($info.rustc -notmatch '^rustc ' -or $info.version -notmatch '^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') {
        throw 'invalid compiler or version metadata'
    }
    $names = @('folio.exe', 'folio.pdb', 'conpty.dll', 'OpenConsole.exe', "folio-$($info.version).cdx.json")
    $entries = @($info.files.PSObject.Properties)
    if ($entries.Count -ne $names.Count) { throw 'BUILDINFO file list mismatch' }
    foreach ($entry in $entries) {
        if ($entry.Name -cnotin $names -or $entry.Value -notmatch '^[0-9a-fA-F]{64}$') {
            throw 'invalid BUILDINFO file or SHA-256'
        }
        $path = Join-Path $Directory $entry.Name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "missing $($entry.Name)" }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $entry.Value) {
            throw "SHA-256 mismatch: $($entry.Name)"
        }
    }
    $actual = @(Get-ChildItem -LiteralPath $Directory -Force)
    if ($actual.Count -ne ($names.Count + 1) -or @($actual | Where-Object { $_.PSIsContainer -or $_.Name -cnotin ($names + 'BUILDINFO.txt') }).Count) {
        throw 'unexpected CI payload contents'
    }
    # Git may lengthen --short=10 to disambiguate. Accept only a >=10-character
    # prefix of the exact resolved SHA, never a substring or unknown commit.
    $banner = Get-CiBuildVersion -Exe (Join-Path $Directory 'folio.exe')
    $pattern = '^Folio ' + [regex]::Escape($info.version) + ' \(([0-9a-f]{10,40})\)$'
    if ($banner -cnotmatch $pattern) { throw '--version format/version mismatch' }
    if (-not $Commit.StartsWith($Matches[1], [StringComparison]::Ordinal)) { throw '--version commit mismatch' }
    return $info
}
