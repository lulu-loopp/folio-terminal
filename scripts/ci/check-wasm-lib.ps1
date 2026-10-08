# the_library_crates_check_for_wasm32 (gate G1, `wasm-lib-check`)
#
# The library crates below `bt-app` that a browser build will reference compile
# for `wasm32-unknown-unknown`. This runs
#
#   cargo check --locked --lib --target wasm32-unknown-unknown -p <each crate below>
#
# and reads cargo's JSON messages: every crate that reports a compile error is
# named, with the first lines of its first error, and a run that checked fewer
# library crates than the list holds is refused rather than read as a pass.
#
# `--lib` because the claim is about the libraries: a crate's tests, benches and
# `src/bin/` tools are native programs and are not asked to build for a browser.
#
# The list is the one place it is written (`.github/workflows/ci.yml` calls this
# script). A crate joins it in the ticket that makes it pass or creates it; a
# crate leaves it only in the ticket that ends its place in the wasm32 graph.
# `bt-platform` is on it because `bt-term` still depends on it (D-14); it
# leaves when that edge does. `bt-math` and `bt-render` name the admission
# vocabulary and the file-read ledger through `bt-effects` (CC-3).
#
# The target must be installed for the toolchain `rust-toolchain.toml` pins
# (`rustup target add wasm32-unknown-unknown`); CI's toolchain action installs it
# for the jobs that ask.

$Packages = @(
    "bt-unicode", "bt-transcript", "bt-doc", "bt-layout", "bt-viewport",
    "bt-detect", "bt-effects", "bt-platform", "bt-math", "bt-render", "bt-term"
)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$target = "wasm32-unknown-unknown"

$arguments = @("check", "--locked", "--lib", "--target", $target, "--message-format=json")
foreach ($package in $Packages) { $arguments += @("-p", $package) }

$previous = [Console]::OutputEncoding
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
Push-Location $repo
try {
    $output = @(& cargo @arguments 2>&1 | ForEach-Object { "$_" })
    $status = $LASTEXITCODE
} finally {
    Pop-Location
    [Console]::OutputEncoding = $previous
}

# The crate a message belongs to, by the directory of its manifest: a workspace
# crate is its directory's name, a registry crate `<name>-<version>`.
function Get-CrateName([string]$manifest) {
    return Split-Path -Leaf (Split-Path -Parent $manifest)
}

$wanted = @{}
foreach ($package in $Packages) { $wanted[$package] = $true }
$checked = @{}
$errors = [ordered]@{}
$other = @()
foreach ($line in $output) {
    if (-not $line.StartsWith("{")) { $other += $line; continue }
    $message = $line | ConvertFrom-Json
    if ($message.reason -notin @("compiler-artifact", "compiler-message")) { continue }
    $crate = Get-CrateName $message.manifest_path
    if ($message.reason -eq "compiler-artifact") {
        if ($wanted.ContainsKey($crate) -and ($message.target.kind -contains "lib")) { $checked[$crate] = $true }
        continue
    }
    if ($message.reason -ne "compiler-message" -or $message.message.level -ne "error") { continue }
    if (-not $errors.Contains($crate)) { $errors[$crate] = @() }
    $errors[$crate] += $message.message.rendered
}

if ($errors.Count -gt 0) {
    $lines = @("the $target library check failed in $($errors.Count) crate(s):")
    foreach ($crate in $errors.Keys) {
        $first = @(($errors[$crate][0] -split "`r?`n") | Where-Object { $_.Trim().Length -gt 0 } | Select-Object -First 6)
        $lines += "  $crate - $($errors[$crate].Count) error(s); the first:"
        $lines += @($first | ForEach-Object { "      $_" })
    }
    throw ($lines -join [Environment]::NewLine)
}
if ($status -ne 0) {
    $tail = @($other | Select-Object -Last 15)
    throw ("cargo check for $target exited $status without a compile error to name:" +
        [Environment]::NewLine + ($tail -join [Environment]::NewLine))
}
$missing = @($Packages | Where-Object { -not $checked.ContainsKey($_) })
if ($missing.Count -gt 0) {
    throw "cargo check for $target passed without checking the library of: $($missing -join ', ') - a crate this gate did not check is not a crate it passed"
}

Write-Host "$target`: $($checked.Count) of $($Packages.Count) library crates checked ($($Packages -join ', ')). PASS"
exit 0
