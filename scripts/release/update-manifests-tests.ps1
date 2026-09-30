<#
.SYNOPSIS
    `update-manifests.ps1 -Apply` and `-Revert`, run against a stand-in for
    `gh` that keeps two repositories in a folder and fails where a case tells
    it to. Nothing reaches GitHub.

.DESCRIPTION
    The stand-in is a `gh` first on `PATH` for the child `pwsh` that runs the
    real script: it answers `gh api user`, `gh api repos/<repo>` and
    `gh api [--method PUT] repos/<repo>/contents/<path>` the way GitHub's
    contents API does — a blob id that is the git blob hash of the bytes, the
    content as wrapped base64, **409 for a write over a blob id the file no
    longer has** — and records every call. A case's faults are scripted per
    method, file and call number: `fail` (the call fails and nothing lands),
    `stale` (a read answers the given bytes instead of the file's), `tamper`
    (somebody else writes the given bytes just before the call). The
    repositories are named `example/…`, and `GH_HOST` points nowhere, so a
    real `gh` reached by mistake writes nothing either.

    **the_second_write_fails_after_the_first_lands** (U-41e0) — the tap is
    written, the bucket's write fails; the tap is put back over the blob id its
    write left, both read back as the saved pair, exit 1.
    MUTATION: in `update-manifests.ps1` step 4, `exit 1` without calling
    `Restore-Manifests`.

    **a_read_back_that_differs_puts_both_back** — both writes land and the
    tap's read-back answers something else; both are put back, exit 1.
    MUTATION: in step 3, drop the `$failed = $true` of the read-back's `else`.

    **revert_puts_back_an_apply_whose_page_never_went_out** — `-Apply`
    succeeds, the page is never published, `-Revert <record>` puts both back
    over the blob ids the record names; a second `-Revert` writes nothing.
    MUTATION: in the `-Revert` branch, `exit 0` before `Restore-Manifests`.

    **a_write_back_answered_409_is_a_release_incident** — the bucket's write
    fails, and somebody writes the tap before it is put back: the write-back
    answers 409, the script exits 3, names what both repositories hold, and
    the record still holds both saved files.
    MUTATION: in step 4, `exit 1` in place of `exit $incidentExit`.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') { $PSNativeCommandUseErrorActionPreference = $false }

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$script = Join-Path $root 'scripts\release\update-manifests.ps1'
$pwsh = (Get-Process -Id $PID).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('folio-update-manifests-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$failures = [System.Collections.Generic.List[string]]::new()
function Check([bool] $ok, [string] $what) {
    if ($ok) { Write-Host "ok   $what" } else { Write-Host "FAIL $what"; $failures.Add($what) }
}

$tap = 'example/homebrew-tap/Casks/folio.rb'
$bucket = 'example/scoop-bucket/bucket/folio.json'

# ── the stand-in ─────────────────────────────────────────────────────────────

$standIn = @'
# A stand-in for `gh`: the calls update-manifests.ps1 makes, answered out of
# the folder $env:FOLIO_GH_STANDIN names (repos.json, faults.json, counts.json)
# and appended to calls.log there.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$state = $env:FOLIO_GH_STANDIN
function Read-Json([string] $name) { Get-Content -Raw -LiteralPath (Join-Path $state $name) | ConvertFrom-Json }
function Write-Json([string] $name, $value) {
    [IO.File]::WriteAllText((Join-Path $state $name), ($value | ConvertTo-Json -Depth 5))
}
function Get-BlobId([byte[]] $bytes) {
    $head = [Text.Encoding]::ASCII.GetBytes("blob $($bytes.Length)`0")
    $sha = [Security.Cryptography.SHA1]::Create()
    return (($sha.ComputeHash([byte[]]($head + $bytes)) | ForEach-Object { $_.ToString('x2') }) -join '')
}
function Say($value, [string] $jq) {
    if ($jq) {
        foreach ($step in $jq.TrimStart('.').Split('.')) { $value = $value.$step }
        if ($value -is [bool]) { $value = "$value".ToLowerInvariant() }
        [Console]::Out.WriteLine("$value")
    }
    else { [Console]::Out.WriteLine(($value | ConvertTo-Json -Depth 5 -Compress)) }
}
function Fail([string] $message) { [Console]::Error.WriteLine($message); exit 1 }

$method = 'GET'; $jq = $null; $fields = @{}; $endpoint = $null
for ($i = 1; $i -lt $args.Count; $i++) {
    switch ($args[$i]) {
        '--method' { $method = $args[++$i] }
        '--jq' { $jq = $args[++$i] }
        '-f' { $kv = $args[++$i]; $at = $kv.IndexOf('='); $fields[$kv.Substring(0, $at)] = $kv.Substring($at + 1) }
        default { $endpoint = $args[$i] }
    }
}
$log = Join-Path $state 'calls.log'

if ($endpoint -eq 'user') { Add-Content $log 'USER'; Say ([pscustomobject]@{ login = 'release-bot' }) $jq; exit 0 }
if ($endpoint -match '^repos/[^/]+/[^/]+$') {
    Add-Content $log "REPO $($endpoint.Substring(6))"
    Say ([pscustomobject]@{ permissions = [pscustomobject]@{ push = $true } }) $jq; exit 0
}
if ($endpoint -notmatch '^repos/([^/]+/[^/]+)/contents/(.+)$') { Fail "stand-in: no answer for $($args -join ' ')" }
$target = "$($Matches[1])/$($Matches[2])"

$counts = Read-Json 'counts.json'
$key = "$method $target"
$n = 1 + [int]$(if ($counts.PSObject.Properties[$key]) { $counts.$key } else { 0 })
$counts | Add-Member -NotePropertyName $key -NotePropertyValue $n -Force
Write-Json 'counts.json' $counts
$fault = @(Read-Json 'faults.json') | Where-Object { $_ -and $_.Method -eq $method -and $_.Target -eq $target -and $_.N -eq $n } |
    Select-Object -First 1

$repos = Read-Json 'repos.json'
if ($fault -and $fault.Action -eq 'tamper') {
    $repos.$target = $fault.Content
    Write-Json 'repos.json' $repos
    Add-Content $log "TAMPER $target blob=$(Get-BlobId ([Convert]::FromBase64String($fault.Content)))"
}
$bytes = [Convert]::FromBase64String($repos.$target)
$blob = Get-BlobId $bytes

if ($method -eq 'GET') {
    Add-Content $log "GET $target"
    if ($fault -and $fault.Action -eq 'fail') { Fail 'gh: Server Error (HTTP 502)' }
    if ($fault -and $fault.Action -eq 'stale') { $bytes = [Convert]::FromBase64String($fault.Content); $blob = Get-BlobId $bytes }
    $wrapped = ([regex]::Matches([Convert]::ToBase64String($bytes), '.{1,60}') | ForEach-Object { $_.Value }) -join "`n"
    Say ([pscustomobject]@{ sha = $blob; content = "$wrapped`n"; encoding = 'base64' }) $jq
    exit 0
}

$new = [Convert]::FromBase64String($fields['content'])
Add-Content $log "PUT $target over=$($fields['sha']) with=$(Get-BlobId $new)"
if ($fault -and $fault.Action -eq 'fail') { Fail 'gh: Server Error (HTTP 502)' }
if ($fields['sha'] -ne $blob) { Fail "gh: $($Matches[2]) does not match $($fields['sha']) (HTTP 409)" }
$repos.$target = $fields['content']
Write-Json 'repos.json' $repos
$newBlob = Get-BlobId $new
Say ([pscustomobject]@{
        content = [pscustomobject]@{ sha = $newBlob; path = $Matches[2] }
        commit  = [pscustomobject]@{ html_url = "https://example.invalid/$target/$newBlob" }
    }) $jq
exit 0
'@

function Get-BlobId([byte[]] $bytes) {
    $head = [Text.Encoding]::ASCII.GetBytes("blob $($bytes.Length)`0")
    $sha = [Security.Cryptography.SHA1]::Create()
    return (($sha.ComputeHash([byte[]]($head + $bytes)) | ForEach-Object { $_.ToString('x2') }) -join '')
}
function To64([string] $text) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($text)) }

$before = @{
    $tap    = [Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $root 'packaging\homebrew\folio.rb')))
    $bucket = [Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $root 'packaging\scoop\folio.json')))
}
$beforeBlob = @{}
foreach ($t in @($tap, $bucket)) { $beforeBlob[$t] = Get-BlobId ([Convert]::FromBase64String($before[$t])) }

# The package directory a release would have: the two checksum files, for a
# version no release has.
function New-Case([string] $name, [object[]] $faults) {
    $case = Join-Path $scratch $name
    $bin = Join-Path $case 'bin'
    $state = Join-Path $case 'gh'
    $package = Join-Path $case 'release-package'
    foreach ($d in @($bin, $state, $package)) { [IO.Directory]::CreateDirectory($d) | Out-Null }
    [IO.File]::WriteAllText((Join-Path $bin 'gh-standin.ps1'), $standIn, [Text.UTF8Encoding]::new($true))
    # One launcher per system, and only the one: an extensionless `gh` beside
    # a `gh.cmd` is a file Windows would try to open rather than run.
    if ([IO.Path]::DirectorySeparatorChar -eq '\') {
        [IO.File]::WriteAllText((Join-Path $bin 'gh.cmd'),
            "@`"$pwsh`" -NoProfile -NonInteractive -File `"%~dp0gh-standin.ps1`" %*`r`n@exit /b %ERRORLEVEL%`r`n")
    }
    else {
        $sh = Join-Path $bin 'gh'
        [IO.File]::WriteAllText($sh, "#!/bin/sh`nexec `"$pwsh`" -NoProfile -NonInteractive -File `"`$(dirname `"`$0`")/gh-standin.ps1`" `"`$@`"`n")
        & chmod +x $sh
    }
    [IO.File]::WriteAllText((Join-Path $state 'repos.json'), ([pscustomobject]@{ $tap = $before[$tap]; $bucket = $before[$bucket] } | ConvertTo-Json))
    [IO.File]::WriteAllText((Join-Path $state 'faults.json'), (ConvertTo-Json -InputObject @($faults) -Depth 4))
    [IO.File]::WriteAllText((Join-Path $state 'counts.json'), '{}')
    [IO.File]::WriteAllText((Join-Path $state 'calls.log'), '')
    [IO.File]::WriteAllText((Join-Path $package 'SHA256SUMS.txt'), ('ab' * 32) + "  folio-9.8.7-windows-x64.zip`n")
    [IO.File]::WriteAllText((Join-Path $package 'SHA256SUMS-macos.txt'), ('cd' * 32) + "  Folio-9.8.7-macos-arm64.dmg`n")
    return [pscustomobject]@{ Bin = $bin; State = $state; Package = $package; Record = (Join-Path $package 'manifests-before.json') }
}

function Invoke-Script($case, [string[]] $arguments) {
    $saved = @{ PATH = $env:PATH; FOLIO_GH_STANDIN = $env:FOLIO_GH_STANDIN; GH_HOST = $env:GH_HOST; GH_TOKEN = $env:GH_TOKEN }
    try {
        $env:PATH = $case.Bin + [IO.Path]::PathSeparator + $env:PATH
        $env:FOLIO_GH_STANDIN = $case.State
        $env:GH_HOST = 'stand-in.invalid'
        $env:GH_TOKEN = 'stand-in'
        $said = & $pwsh -NoProfile -NonInteractive -File $script @arguments 2>&1 | Out-String
        return [pscustomobject]@{ Exit = $LASTEXITCODE; Text = $said }
    }
    finally {
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
}
function Invoke-Apply($case) {
    return Invoke-Script $case @('-Version', '9.8.7', '-PackageDirectory', $case.Package, '-Apply', '-Account', 'release-bot',
        '-CaskRepository', 'example/homebrew-tap', '-ScoopRepository', 'example/scoop-bucket')
}
function Get-Held($case, [string] $target) { (Get-Content -Raw -LiteralPath (Join-Path $case.State 'repos.json') | ConvertFrom-Json).$target }
function Get-Calls($case) { @(Get-Content -LiteralPath (Join-Path $case.State 'calls.log')) }

# Every case: the stand-in answered, nothing named a real repository.
function Check-StandIn($case, [string] $label) {
    $calls = Get-Calls $case
    Check ($calls -contains 'USER') "${label}: the stand-in answered gh api user"
    Check (@($calls | Where-Object { $_ -match 'lulu-loopp' }).Count -eq 0) "${label}: no call named a published repository"
}
function Check-BothBack($case, [string] $label) {
    Check ((Get-Held $case $tap) -ceq $before[$tap]) "${label}: the tap holds the bytes it held before"
    Check ((Get-Held $case $bucket) -ceq $before[$bucket]) "${label}: the bucket holds the bytes it held before"
}
function Check-Record($case, [string] $label) {
    Check (Test-Path -LiteralPath $case.Record -PathType Leaf) "${label}: the record is in the package directory"
    if (-not (Test-Path -LiteralPath $case.Record -PathType Leaf)) { return }
    $record = Get-Content -Raw -LiteralPath $case.Record | ConvertFrom-Json
    foreach ($t in @($tap, $bucket)) {
        $entry = @($record.Repositories | Where-Object { "$($_.Repository)/$($_.Path)" -eq $t })
        Check ($entry.Count -eq 1 -and $entry[0].Before.Content -ceq $before[$t] -and $entry[0].Before.Blob -eq $beforeBlob[$t]) `
            "${label}: the record holds $t's saved bytes and blob id"
    }
}

try {
    # ── the_second_write_fails_after_the_first_lands ─────────────────────────
    $case = New-Case 'second-write-fails' @([pscustomobject]@{ Method = 'PUT'; Target = $bucket; N = 1; Action = 'fail' })
    $run = Invoke-Apply $case
    Check-StandIn $case 'second write fails'
    Check ($run.Exit -eq 1) "second write fails: exit 1 ($($run.Exit))"
    Check ($run.Text -match 'nothing published; both manifests are back to what they were') 'second write fails: says nothing is published and both are back'
    Check-BothBack $case 'second write fails'
    Check-Record $case 'second write fails'
    $puts = @(Get-Calls $case | Where-Object { $_ -like 'PUT *' })
    $tapPuts = @($puts | Where-Object { $_ -like "PUT $tap *" })
    Check ($tapPuts.Count -eq 2) "second write fails: the tap is written twice, forward and back ($($tapPuts.Count))"
    if ($tapPuts.Count -eq 2) {
        $forward = [regex]::Match($tapPuts[0], 'with=(\w+)').Groups[1].Value
        Check ($tapPuts[1] -eq "PUT $tap over=$forward with=$($beforeBlob[$tap])") `
            'second write fails: the tap is put back with its saved bytes, over the blob id its write left'
    }

    # ── a_read_back_that_differs_puts_both_back ──────────────────────────────
    # The tap's second read (the first is the one the difference is printed
    # against) answers the old file, as a lagging read would.
    $case = New-Case 'read-back-differs' @([pscustomobject]@{ Method = 'GET'; Target = $tap; N = 2; Action = 'stale'; Content = $before[$tap] })
    $run = Invoke-Apply $case
    Check-StandIn $case 'read-back differs'
    Check ($run.Exit -eq 1) "read-back differs: exit 1 ($($run.Exit))"
    Check ($run.Text -match 'Casks/folio\.rb — reads back as the file it held before .*, not as written') 'read-back differs: says the tap does not read back as written'
    Check ($run.Text -match 'nothing published; both manifests are back to what they were') 'read-back differs: says nothing is published and both are back'
    Check-BothBack $case 'read-back differs'
    $back = @(Get-Calls $case | Where-Object { $_ -like 'PUT *' -and $_ -like '* with=*' } | Where-Object {
            $_ -eq "PUT $tap over=$([regex]::Match($_, 'over=(\w+)').Groups[1].Value) with=$($beforeBlob[$tap])" -or
            $_ -eq "PUT $bucket over=$([regex]::Match($_, 'over=(\w+)').Groups[1].Value) with=$($beforeBlob[$bucket])" })
    Check ($back.Count -eq 2) "read-back differs: both are written back with their saved bytes ($($back.Count))"

    # ── revert_puts_back_an_apply_whose_page_never_went_out ───────────────────
    $case = New-Case 'revert' @()
    $run = Invoke-Apply $case
    Check-StandIn $case 'revert'
    Check ($run.Exit -eq 0) "revert: -Apply exits 0 ($($run.Exit))"
    Check ($run.Text -match [regex]::Escape("-Revert $($case.Record)")) 'revert: -Apply names -Revert and the record'
    Check ((Get-Held $case $tap) -cne $before[$tap] -and (Get-Held $case $bucket) -cne $before[$bucket]) 'revert: -Apply wrote both'
    $applied = @{ $tap = Get-BlobId ([Convert]::FromBase64String((Get-Held $case $tap)))
                  $bucket = Get-BlobId ([Convert]::FromBase64String((Get-Held $case $bucket))) }
    $run = Invoke-Script $case @('-Revert', $case.Record, '-Account', 'release-bot')
    Check ($run.Exit -eq 0) "revert: -Revert exits 0 ($($run.Exit))"
    Check ($run.Text -match 'both manifests are back to what they were') 'revert: says both are back'
    Check-BothBack $case 'revert'
    Check-Record $case 'revert'
    foreach ($t in @($tap, $bucket)) {
        Check ((Get-Calls $case) -contains "PUT $t over=$($applied[$t]) with=$($beforeBlob[$t])") "revert: $t is written back over the blob id -Apply left"
    }
    $count = @(Get-Calls $case | Where-Object { $_ -like 'PUT *' }).Count
    $run = Invoke-Script $case @('-Revert', $case.Record)
    Check ($run.Exit -eq 0 -and @(Get-Calls $case | Where-Object { $_ -like 'PUT *' }).Count -eq $count) `
        "revert: a second -Revert exits 0 and writes nothing ($($run.Exit))"

    # ── a_write_back_answered_409_is_a_release_incident ──────────────────────
    $third = To64 "cask `"folio`" do`n  # somebody else's edit`nend`n"
    $case = New-Case 'write-back-409' @(
        [pscustomobject]@{ Method = 'PUT'; Target = $bucket; N = 1; Action = 'fail' },
        [pscustomobject]@{ Method = 'PUT'; Target = $tap; N = 2; Action = 'tamper'; Content = $third })
    $run = Invoke-Apply $case
    Check-StandIn $case 'write-back 409'
    Check ($run.Exit -eq 3) "write-back 409: exit 3, the release incident ($($run.Exit))"
    Check ($run.Text -match 'HTTP 409') 'write-back 409: the 409 is reported'
    Check ($run.Text -match 'RELEASE INCIDENT') 'write-back 409: says release incident'
    Check ($run.Text -match "example/homebrew-tap/Casks/folio\.rb holds a file that is neither the saved one nor Folio 9\.8\.7's \(blob $(Get-BlobId ([Convert]::FromBase64String($third)))\)") `
        'write-back 409: names what the tap holds now'
    Check ($run.Text -match "example/scoop-bucket/bucket/folio\.json holds the file it held before \(blob $($beforeBlob[$bucket])\)") `
        'write-back 409: names what the bucket holds now'
    Check ($run.Text -match [regex]::Escape("the record is $($case.Record)")) 'write-back 409: names the record'
    Check ($run.Text -notmatch 'nothing published; both manifests are back') 'write-back 409: does not say both are back'
    Check ((Get-Held $case $tap) -ceq $third) 'write-back 409: the other edit to the tap is not written over'
    Check-Record $case 'write-back 409'
}
finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) check(s) failed"
    exit 1
}
Write-Host 'update-manifests.ps1 puts back every write that landed, and names an incident when it cannot'
# Explicit: the cases leave the last child's exit code (3, the incident case) in
# $LASTEXITCODE, and CI's pwsh shell ends a step with it.
exit 0
