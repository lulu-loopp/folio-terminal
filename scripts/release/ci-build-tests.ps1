# Standalone child-process harness, like sign-tests.ps1 and smoke-tests.ps1.
# No network, compiler, GUI, credentials or real Folio process is used.
[CmdletBinding()]
param([string] $Case, [string] $Scratch)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Case) {
    . "$PSScriptRoot/ci-build.ps1"
    $sha = '0123456789abcdef0123456789abcdef01234567'
    $payload = Join-Path $Scratch 'payload'
    New-Item -ItemType Directory -Path $payload | Out-Null
    $hashes = [ordered]@{}
    foreach ($name in @('folio.exe', 'folio.pdb', 'conpty.dll', 'OpenConsole.exe', 'folio-0.4.2.cdx.json')) {
        $path = Join-Path $payload $name
        [IO.File]::WriteAllText($path, "fixture $name")
        $hashes[$name] = (Get-FileHash -LiteralPath $path).Hash
    }
    $info = @{
        schema = 1; commit = $sha; version = '0.4.2'; profile = 'release'
        command = 'cargo build --release --locked -p bt-app'; rustc = 'rustc 1.94.1'; files = $hashes
    }
    $script:banner = 'Folio 0.4.2 (0123456789)'
    $script:called = $false
    function Get-CiBuildVersion {
        param([string] $Exe)
        if (-not (Test-Path -LiteralPath $Exe)) { throw 'test version reader got no exe' }
        $script:called = $true
        return $script:banner
    }
    switch ($Case) {
        'hash' { [IO.File]::AppendAllText((Join-Path $payload 'folio.exe'), 'tampered') }
        'pdb' { [IO.File]::AppendAllText((Join-Path $payload 'folio.pdb'), 'tampered') }
        'missing' { Remove-Item -LiteralPath (Join-Path $payload 'conpty.dll') }
        'commit' { $info.commit = 'f' * 40 }
        'profile' { $info.profile = 'dev' }
        'path' { $info.files['../escape.exe'] = 'f' * 64 }
        'extra' { [IO.File]::WriteAllText((Join-Path $payload 'extra.dll'), 'unexpected') }
        'version' { $script:banner = 'Folio 0.4.2 (ffffffffff)' }
        'unknown' { $script:banner = 'Folio 0.4.2 (unknown)' }
        'wrong-version' { $script:banner = 'Folio 0.4.3 (0123456789)' }
        'long-prefix' { $script:banner = 'Folio 0.4.2 (0123456789ab)' }
    }
    [IO.File]::WriteAllText((Join-Path $payload 'BUILDINFO.txt'), ($info | ConvertTo-Json -Depth 5))
    if ($Case.StartsWith('package-')) {
        # Resolve HEAD to the fixture commit; refusal must happen before SDK,
        # signing, SBOM generation or any local cargo invocation is reached.
        function git { $global:LASTEXITCODE = 0; return $sha }
        if ($Case -eq 'package-overlap') {
            & "$PSScriptRoot/package.ps1" -Binary $payload -Output $payload
        } else {
            [IO.File]::AppendAllText((Join-Path $payload 'folio.exe'), 'tampered')
            & "$PSScriptRoot/package.ps1" -Binary $payload -Output (Join-Path $Scratch 'package')
        }
        throw 'package accepted invalid input'
    }
    if ($Case.StartsWith('fetch-')) {
        function gh {
            $global:LASTEXITCODE = 0
            $request = $args -join ' '
            if ($request -eq 'api user') { return '{"login":"fixture-account"}' }
            if ($request -match '/commits/') { return (@{ sha = $sha } | ConvertTo-Json) }
            if ($request -match '/actions/artifacts') {
                $artifacts = @()
                if ($Case -ne 'fetch-missing') {
                    $artifacts = @(@{ id = 12; name = "folio-windows-release-$sha"
                        expired = ($Case -eq 'fetch-expired'); workflow_run = @{ id = 34 } })
                }
                return ConvertTo-Json -InputObject @(@{ artifacts = $artifacts }) -Depth 6
            }
            if ($request -match '/actions/runs/34') {
                return (@{ path = '.github/workflows/build-release.yml'; status = 'completed'
                    conclusion = $(if ($Case -eq 'fetch-failed') { 'failure' } else { 'success' })
                    event = 'workflow_dispatch'; head_sha = ('f' * 40) } | ConvertTo-Json)
            }
            if ($request -match '^run download ') {
                $destination = $args[([array]::IndexOf($args, '--dir') + 1)]
                Get-ChildItem -LiteralPath $payload -File | Copy-Item -Destination $destination
                [IO.File]::AppendAllText((Join-Path $destination 'folio.pdb'), 'tampered')
                return
            }
            throw "unexpected gh request: $request"
        }
        $out = Join-Path $Scratch 'download'
        $account = if ($Case -eq 'fetch-account') { 'wrong-account' } else { 'fixture-account' }
        & "$PSScriptRoot/fetch-ci-build.ps1" -Ref 'feature/test' -Out $out -Account $account -Apply:($Case -eq 'fetch-tampered')
        if (Test-Path -LiteralPath $out) { throw 'dry run wrote output' }
        exit 0
    }
    try {
        Assert-CiBuild -Directory $payload -Commit $sha | Out-Null
        if (-not $script:called) { throw 'version reader was not reached' }
        Write-Host 'VERIFIED fixture'
    } catch {
        if ($Case -in @('hash', 'pdb', 'missing', 'commit', 'profile', 'path', 'extra') -and $script:called) {
            throw 'executed an unverified payload'
        }
        throw
    }
    exit 0
}

$root = (Resolve-Path "$PSScriptRoot/../..").Path
$tempRoot = Join-Path $root 'target/ci-build-tests'
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
$temp = Join-Path $tempRoot ([Guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $temp | Out-Null
$cases = [ordered]@{
    valid = 'VERIFIED fixture'; 'long-prefix' = 'VERIFIED fixture'
    hash = 'SHA-256 mismatch'; pdb = 'SHA-256 mismatch'; missing = 'missing conpty.dll'
    commit = 'BUILDINFO commit mismatch'; profile = 'BUILDINFO profile mismatch'
    path = 'BUILDINFO file list mismatch'; extra = 'unexpected CI payload contents'
    version = '--version commit mismatch'; unknown = '--version format/version mismatch'
    'wrong-version' = '--version format/version mismatch'
    'fetch-dry' = 'Print-only'; 'fetch-account' = '-Account names wrong-account'
    'fetch-missing' = 'No unexpired successful'; 'fetch-expired' = 'No unexpired successful'
    'fetch-failed' = 'No unexpired successful'; 'fetch-tampered' = 'SHA-256 mismatch'
    'package-hash' = 'SHA-256 mismatch'; 'package-overlap' = 'must be separate, non-nested'
}
$count = 0
try {
    foreach ($item in $cases.GetEnumerator()) {
        $dir = Join-Path $temp $item.Key
        New-Item -ItemType Directory -Path $dir | Out-Null
        $output = & pwsh -NoLogo -NoProfile -File $PSCommandPath -Case $item.Key -Scratch $dir 2>&1 | Out-String
        $code = $LASTEXITCODE
        $success = $item.Key -in @('valid', 'long-prefix', 'fetch-dry')
        if (($success -and $code -ne 0) -or (-not $success -and $code -eq 0) -or
            -not $output.Contains($item.Value) -or $output.Contains('executed an unverified payload')) {
            throw "$($item.Key): exit $code`n$output"
        }
        $count++
        Write-Host "PASS $($item.Key)"
    }
    Write-Host "$count cases passed. No network or Cargo invoked."
} finally {
    # Only this invocation's UUID directory, resolved inside the worktree.
    $resolved = (Resolve-Path -LiteralPath $temp).Path
    if (-not $resolved.StartsWith((Resolve-Path $tempRoot).Path + [IO.Path]::DirectorySeparatorChar)) {
        throw 'test cleanup escaped its scratch root'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
# The last negative child leaves a nonzero native exit code. GitHub's pwsh
# wrapper must see the harness's success, not that deliberately refused child.
exit 0
