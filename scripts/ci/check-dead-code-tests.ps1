<#
.SYNOPSIS
    What `check-dead-code.ps1` decides, asserted by running it against scratch
    trees, one case per rule.

.DESCRIPTION
    Each case builds a small git repository: a crate `crates/demo` with its
    `src`, a `docs/plans/DEAD-CODE.tsv`, and a copy of the gate under
    `scripts/ci/`, where the gate looks for the repository it belongs to. The
    tree as first written is committed and `refs/remotes/origin/main` is
    pointed at that commit, so the merge base is the stub it compares against;
    then the case edits the working tree and runs the gate as a child `pwsh`,
    reading its exit code and its output. Nothing here stands in for the gate:
    every verdict below is the real script's, over real files and a real merge
    base.

    The repositories are made under `-Scratch` (the system temporary folder
    unless told otherwise) and deleted at the end, never inside the checkout
    this file lives in.
#>

[CmdletBinding()]
param(
    [string]$Scratch = [IO.Path]::GetTempPath()
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Most cases are about a non-zero exit code, so a non-zero exit code has to be a
# value this script reads rather than an error PowerShell raises on its own.
if (Test-Path -LiteralPath 'Variable:PSNativeCommandUseErrorActionPreference') {
    $PSNativeCommandUseErrorActionPreference = $false
}

$here = $PSScriptRoot
if (-not $here) { throw 'check-dead-code-tests.ps1 cannot tell where it is; run it as a file' }
$gate = Join-Path $here 'check-dead-code.ps1'
if (-not (Test-Path -LiteralPath $gate -PathType Leaf)) { throw "there is no check-dead-code.ps1 beside $PSCommandPath" }

$pwsh = @(Get-Command -Name pwsh -CommandType Application -ErrorAction SilentlyContinue)
if ($pwsh.Count -eq 0) { throw 'these cases run the gate in a child pwsh, and there is no pwsh on the path' }
$pwsh = $pwsh[0].Source

$work = Join-Path ([IO.Path]::GetFullPath($Scratch)) ('folio-dead-code-tests-' + [Guid]::NewGuid().ToString('n'))
[IO.Directory]::CreateDirectory($work) | Out-Null

$columns = "crate`tfile`titem`tform`treason"
$utf8 = [Text.UTF8Encoding]::new($false)

function Get-List([string[]]$rows) {
    $text = "# the list of a scratch tree`n$columns`n"
    foreach ($row in $rows) { $text += "$row`n" }
    return $text
}

function Write-Files([string]$root, [hashtable]$files) {
    foreach ($name in $files.Keys) {
        $path = Join-Path $root $name
        if ($null -eq $files[$name]) {
            Remove-Item -LiteralPath $path -Force
            continue
        }
        [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
        [IO.File]::WriteAllText($path, $files[$name], $utf8)
    }
}

function Invoke-Git([string]$root) {
    $output = & git -C $root -c core.autocrlf=false -c user.name=gate-tests -c user.email=gate-tests@example.invalid @args 2>&1
    if ($LASTEXITCODE -ne 0) { throw "git $args failed in the scratch tree: $output" }
}

# A scratch repository: `Base` committed as the stub merge base, then `Now` written over it.
function New-Tree {
    param([string]$Name, [hashtable]$Base, [hashtable]$Now = @{}, [switch]$NoOrigin)

    $root = Join-Path $work $Name
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Invoke-Git $root init -q -b main
    [IO.Directory]::CreateDirectory((Join-Path $root 'scripts/ci')) | Out-Null
    Copy-Item -LiteralPath $gate -Destination (Join-Path $root 'scripts/ci/check-dead-code.ps1')
    Write-Files $root $Base
    Invoke-Git $root add -A
    Invoke-Git $root commit -q -m base
    if (-not $NoOrigin) { Invoke-Git $root update-ref refs/remotes/origin/main HEAD }
    Write-Files $root $Now
    return $root
}

function Invoke-Gate([string]$root) {
    $output = & $pwsh -NoLogo -NoProfile -File (Join-Path $root 'scripts/ci/check-dead-code.ps1') 2>&1
    $code = $LASTEXITCODE
    $text = ($output | ForEach-Object { "$_" }) -join "`n"
    return [pscustomobject]@{ ExitCode = $code; Text = $text; Flat = ($text -replace '\s+', ' ') }
}

$failures = @()
$ran = 0
function Test-Case([string]$name, [scriptblock]$body) {
    $script:ran++
    try {
        & $body
        Write-Host "ok    $name"
    } catch {
        Write-Host "FAIL  $name"
        Write-Host "      $($_.Exception.Message)"
        $script:failures += $name
    }
}

# The tree every case starts from: one listed site, and a doc comment quoting an
# attribute, which is not a site.
$lib = @'
//! A crate for the gate's cases. A doc comment quoting `#[allow(dead_code)]` is not a site.

pub struct Kept;

#[allow(dead_code)]
fn listed_helper() {}
'@
$listedRow = "demo`tsrc/lib.rs`tlisted_helper`tallow`t"
$base = @{
    'crates/demo/src/lib.rs' = $lib
    'docs/plans/DEAD-CODE.tsv' = (Get-List @($listedRow))
}

Test-Case 'the tree exactly as the list has it passes, and says how many sites there are' {
    $result = Invoke-Gate (New-Tree -Name 'as-listed' -Base $base)
    if ($result.ExitCode -ne 0) { throw "it exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'the dead-code list only shrinks: 1 sites, 0 dated\.') { throw "no summary line: $($result.Text)" }
}

Test-Case 'a new site with no dated reason fails, naming the site and the rule' {
    $now = @{ 'crates/demo/src/lib.rs' = $lib + "`n#[expect(dead_code, reason = `"somebody will call it`")]`nfn fresh() {}`n" }
    $result = Invoke-Gate (New-Tree -Name 'undated' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'src/lib\.rs:\d+ fresh \(expect\)') { throw "the site was not named: $($result.Text)" }
    if ($result.Flat -notmatch [regex]::Escape('<TICKET-ID> until YYYY-MM-DD: <why>')) { throw "the rule was not named: $($result.Text)" }
}

Test-Case 'a new site dated to a day that has not come passes' {
    $now = @{ 'crates/demo/src/lib.rs' = $lib + "`n#[expect(dead_code, reason = `"U-41 until 2999-12-31: the card calls it`")]`nfn fresh() {}`n" }
    $result = Invoke-Gate (New-Tree -Name 'dated' -Base $base -Now $now)
    if ($result.ExitCode -ne 0) { throw "it exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'dated .*fresh \(expect\) - U-41 until 2999-12-31') { throw "the dated site was not reported: $($result.Text)" }
    if ($result.Flat -notmatch '2 sites, 1 dated\.') { throw "the summary did not count it: $($result.Text)" }
}

Test-Case 'a site whose date has passed fails as expired' {
    $now = @{ 'crates/demo/src/lib.rs' = $lib + "`n#[expect(dead_code, reason = `"T-KEYBOARD-RECORDS until 2000-01-01: the card calls it`")]`nfn stale_door() {}`n" }
    $result = Invoke-Gate (New-Tree -Name 'expired' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'stale_door \(expect\) is past its date \(T-KEYBOARD-RECORDS until 2000-01-01\): expired: wire it, delete it, or re-ticket it') {
        throw "the expiry was not reported: $($result.Text)"
    }
}

Test-Case 'sites inside a #[cfg(test)] module, a test-named file and a file reached only through a test module are not product code' {
    $now = @{
        'crates/demo/src/lib.rs' = $lib + @'

#[cfg(test)]
mod tests_here {
    #[allow(dead_code)]
    fn helper() {}
}

#[cfg(all(test, windows))]
mod on_windows {
    #[allow(dead_code)]
    struct Holder(u8);
}

#[cfg(test)]
mod harness;

mod thing_tests;
'@
        'crates/demo/src/harness.rs' = "mod inner;`n#[allow(dead_code)]`npub fn in_harness() {}`n"
        'crates/demo/src/harness/inner.rs' = "#[allow(dead_code)]`npub fn under_harness() {}`n"
        'crates/demo/src/thing_tests.rs' = "#[allow(dead_code)]`npub fn in_a_test_file() {}`n"
    }
    $result = Invoke-Gate (New-Tree -Name 'test-code' -Base $base -Now $now)
    if ($result.ExitCode -ne 0) { throw "it exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch '1 sites, 0 dated\.') { throw "a test site was counted: $($result.Text)" }
}

Test-Case 'a file a crate root reaches through an ordinary declaration is product code' {
    $now = @{
        'crates/demo/src/lib.rs' = $lib + "`nmod shipped;`n"
        'crates/demo/src/shipped.rs' = "#[allow(dead_code)]`npub fn in_shipped() {}`n"
    }
    $result = Invoke-Gate (New-Tree -Name 'shipped' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'src/shipped\.rs:\d+ in_shipped \(allow\)') { throw "the site was not named: $($result.Text)" }
}

Test-Case 'an allow(dead_code) inside a cfg_attr is found' {
    $now = @{ 'crates/demo/src/lib.rs' = $lib + "`n#[cfg_attr(`n    not(test),`n    allow(`n        dead_code`n    )`n)]`npub(crate) fn gated() {}`n" }
    $result = Invoke-Gate (New-Tree -Name 'cfg-attr' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'src/lib\.rs:\d+ gated \(allow\)') { throw "the cfg_attr site was not named: $($result.Text)" }
}

Test-Case 'a site that went, with its row, passes and is reported as removed' {
    $now = @{
        'crates/demo/src/lib.rs' = "pub struct Kept;`n"
        'docs/plans/DEAD-CODE.tsv' = (Get-List @())
    }
    $result = Invoke-Gate (New-Tree -Name 'removed' -Base $base -Now $now)
    if ($result.ExitCode -ne 0) { throw "it exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'removed demo \| src/lib\.rs \| listed_helper \| allow') { throw "the removal was not reported: $($result.Text)" }
    if ($result.Flat -notmatch '1 rows -> 0, 0 added, 1 removed\.') { throw "the comparison line is wrong: $($result.Text)" }
}

Test-Case 'a site that went while its row stayed fails as stale' {
    $now = @{ 'crates/demo/src/lib.rs' = "pub struct Kept;`n" }
    $result = Invoke-Gate (New-Tree -Name 'stale' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'has a row whose site is not in the tree: demo \| src/lib\.rs \| listed_helper \| allow') {
        throw "the stale row was not named: $($result.Text)"
    }
}

Test-Case 'a listed site whose reason changed fails' {
    $now = @{ 'crates/demo/src/lib.rs' = $lib.Replace('#[allow(dead_code)]', '#[allow(dead_code, reason = "a reason written later")]') }
    $result = Invoke-Gate (New-Tree -Name 'changed' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch "listed_helper \(allow\) is on docs/plans/DEAD-CODE\.tsv with the reason '' and now says 'a reason written later'") {
        throw "the changed reason was not reported: $($result.Text)"
    }
}

Test-Case 'a row added to the list together with its site fails against the merge base' {
    $now = @{
        'crates/demo/src/lib.rs' = $lib + "`n#[allow(dead_code)]`nfn smuggled() {}`n"
        'docs/plans/DEAD-CODE.tsv' = (Get-List @($listedRow, "demo`tsrc/lib.rs`tsmuggled`tallow`t"))
    }
    $result = Invoke-Gate (New-Tree -Name 'added' -Base $base -Now $now)
    if ($result.ExitCode -eq 0) { throw 'it exited 0' }
    if ($result.Flat -notmatch 'gained a row the merge base does not have: demo \| src/lib\.rs \| smuggled \| allow') {
        throw "the added row was not named: $($result.Text)"
    }
}

Test-Case 'with no merge base the gate refuses rather than passes' {
    $result = Invoke-Gate (New-Tree -Name 'no-base' -Base $base -NoOrigin)
    if ($result.ExitCode -ne 2) { throw "it exited $($result.ExitCode), not 2: $($result.Text)" }
    if ($result.Flat -notmatch 'This is not a pass: the comparison did not happen\.') { throw "the refusal was something else: $($result.Text)" }
}

Test-Case 'a merge base without the list passes, and says it is the commit that introduces it' {
    $bare = @{ 'crates/demo/src/lib.rs' = $lib }
    $now = @{ 'docs/plans/DEAD-CODE.tsv' = (Get-List @($listedRow)) }
    $result = Invoke-Gate (New-Tree -Name 'introduced' -Base $bare -Now $now)
    if ($result.ExitCode -ne 0) { throw "it exited $($result.ExitCode): $($result.Text)" }
    if ($result.Flat -notmatch 'this is the commit that introduces it, and it has 1 rows\.') { throw "it did not say so: $($result.Text)" }
}

Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($failures.Count -gt 0) {
    throw "$($failures.Count) of $ran case(s) failed: $($failures -join '; ')"
}
Write-Host "$ran cases, all green."
# Several cases run the gate in a child that is meant to refuse, so the last thing
# `$LASTEXITCODE` holds may be one of those refusals.
exit 0
