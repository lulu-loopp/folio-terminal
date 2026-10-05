# Shared scope for the tracked-text integrity gates. A tracked file is text exactly when its bytes
# are strict UTF-8 and contain no NUL. Every binary is named in tracked-binary-files.tsv, and each
# row is checked against the current bytes so the list can only shrink deliberately.

$script:StrictUtf8 = [Text.UTF8Encoding]::new($false, $true)

function Get-TrackedPaths([string] $Repo) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = 'git'
    $start.WorkingDirectory = $Repo
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.ArgumentList.Add('-c')
    $start.ArgumentList.Add('core.quotepath=false')
    $start.ArgumentList.Add('ls-files')
    $start.ArgumentList.Add('-z')
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'git ls-files could not start' }
        $memory = [IO.MemoryStream]::new()
        $process.StandardOutput.BaseStream.CopyTo($memory)
        $errorText = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "git ls-files failed: $errorText" }
        $decoded = $script:StrictUtf8.GetString($memory.ToArray())
        return @($decoded.Split([char]0, [StringSplitOptions]::RemoveEmptyEntries))
    } finally {
        $process.Dispose()
    }
}

function Read-ReasonList([string] $Path, [string] $Header) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "missing integrity exemption list $Path"
    }
    $rows = @{}
    $lines = @(Get-Content -LiteralPath $Path)
    $first = $lines | Where-Object { $_ -and -not $_.StartsWith('#') } | Select-Object -First 1
    if ($first -ne $Header) { throw "$Path must begin its rows with '$Header'" }
    foreach ($line in $lines) {
        if (-not $line -or $line.StartsWith('#') -or $line -eq $Header) { continue }
        $parts = $line -split "`t", 2
        if ($parts.Count -ne 2 -or -not $parts[0] -or -not $parts[1]) {
            throw "$Path has a row without an exact path and a reason: $line"
        }
        $relative = $parts[0].Replace('\', '/')
        if ($rows.ContainsKey($relative)) { throw "$Path lists $relative twice" }
        $rows[$relative] = $parts[1]
    }
    return $rows
}

function Get-TrackedTextFiles([string] $Repo) {
    $tracked = @(Get-TrackedPaths $Repo)
    $binary = Read-ReasonList (Join-Path $PSScriptRoot 'tracked-binary-files.tsv') "path`treason"
    $seenBinary = @{}
    $out = @()

    foreach ($relative in $tracked) {
        $relative = $relative.Replace('\', '/')
        $full = Join-Path $Repo $relative
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
            throw "$relative is tracked but cannot be read from the working tree"
        }
        $bytes = [IO.File]::ReadAllBytes($full)
        $hasNul = [Array]::IndexOf($bytes, [byte]0) -ge 0
        $validUtf8 = $true
        try { $null = $script:StrictUtf8.GetString($bytes) } catch { $validUtf8 = $false }
        $isText = -not $hasNul -and $validUtf8

        if ($binary.ContainsKey($relative)) {
            $seenBinary[$relative] = $true
            if ($isText) {
                throw "$relative is listed as binary but is now strict UTF-8 text without NUL; remove the stale exemption"
            }
            continue
        }
        if (-not $isText) {
            $reason = if ($hasNul) { 'contains NUL bytes' } else { 'is not strict UTF-8' }
            throw "$relative $reason but has no exact binary exemption"
        }
        $out += [pscustomobject]@{ Relative = $relative; Full = $full; Bytes = $bytes }
    }

    foreach ($relative in $binary.Keys) {
        if (-not $seenBinary.ContainsKey($relative)) {
            throw "$relative is listed as binary but is not a tracked readable file"
        }
    }
    return $out
}
