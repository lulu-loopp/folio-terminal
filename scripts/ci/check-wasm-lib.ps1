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
# `bt-platform` left it with CC-4: no crate on the list depends on it (the host
# installs what `bt-term` used to ask it, D-14 repaid). `bt-math` and
# `bt-render` name the admission vocabulary and the file-read ledger through
# `bt-effects` (CC-3). `bt-compose` joined with CC-6a, the ticket that created it.
#
# **The font bytes a browser build carries (CC-9).** `bt-math` takes typst-assets'
# seventeen faces (9,683,068 bytes) on the desktop and none on wasm32, where the host
# hands its faces to `MathEngine::with_faces`. There is no wasm artefact for this
# gate to weigh, so the bound is asked of the graph that decides it: after the check,
# `cargo tree -e normal,build,features -i typst-assets` (the shipped graph: a
# dev-dependency is not in a browser build) must show the `fonts` feature for
# `bt-math` on the host target (so the question can be answered at all), and must
# not show it for wasm32 in the graph of any crate on the list — a crate that
# turned the fonts on itself would carry them as surely as `bt-math` would. Every
# crate whose wasm32 graph has them on is refused by name.
#
# The target must be installed for the toolchain `rust-toolchain.toml` pins
# (`rustup target add wasm32-unknown-unknown`); CI's toolchain action installs it
# for the jobs that ask.

$Packages = @(
    "bt-unicode", "bt-transcript", "bt-doc", "bt-layout", "bt-viewport",
    "bt-detect", "bt-effects", "bt-math", "bt-render", "bt-term", "bt-compose"
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

# The bound on embedded font bytes: typst-assets' `fonts` feature, asked of one
# crate's shipped graph for one target. A crate whose graph does not reach
# typst-assets at all answers with no line.
function Get-TypstAssetsFeatures([string]$package, [string[]]$targetArguments) {
    Push-Location $repo
    try {
        $tree = @(& cargo tree --locked -p $package @targetArguments -e normal,build,features -i typst-assets --prefix none 2>&1 | ForEach-Object { "$_" })
        $treeStatus = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    # `cargo tree -i` names a package that is not in this crate's graph for this
    # target as unmatched; that is the answer "no line", not a failure.
    if ($treeStatus -ne 0 -and ($tree | Where-Object { $_.Contains("package ID specification ``typst-assets`` did not match any packages") })) {
        return @()
    }
    if ($treeStatus -ne 0) {
        throw ("cargo tree for $package's typst-assets features exited $treeStatus" + [Environment]::NewLine + (($tree | Select-Object -Last 10) -join [Environment]::NewLine))
    }
    return $tree
}
$fontsFeature = 'typst-assets feature "fonts"'
$hostTree = Get-TypstAssetsFeatures "bt-math" @()
if (-not ($hostTree | Where-Object { $_.Contains($fontsFeature) })) {
    throw "bt-math's host graph does not turn on typst-assets' fonts, so the $target question below would be answered by nothing - the desktop engine embeds them (MathEngine::new)"
}
$withFonts = [ordered]@{}
foreach ($package in $Packages) {
    $wasmTree = @(Get-TypstAssetsFeatures $package @("--target", $target))
    if ($wasmTree | Where-Object { $_.Contains($fontsFeature) }) { $withFonts[$package] = $wasmTree }
}
if ($withFonts.Count -gt 0) {
    $lines = @("the $target graph of $($withFonts.Count) crate(s) turns on typst-assets' fonts (9,683,068 bytes of faces a browser build would carry); a browser host hands its faces to MathEngine::with_faces:")
    foreach ($package in $withFonts.Keys) {
        $lines += "  $package - the edges that turn it on:"
        $lines += @($withFonts[$package] | Where-Object { $_.Contains(" feature ") -or $_.StartsWith("bt-") } | Select-Object -First 12 | ForEach-Object { "      $_" })
    }
    throw ($lines -join [Environment]::NewLine)
}

Write-Host "$target`: $($checked.Count) of $($Packages.Count) library crates checked ($($Packages -join ', ')); none of them embeds a typst-assets face for $target. PASS"
exit 0
