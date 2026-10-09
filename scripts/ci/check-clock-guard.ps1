# no_clock_of_the_standard_library_is_named_where_a_browser_build_reads
# (gate G3, `clock-guard`)
#
# `std::time::Instant::now()` and `SystemTime::now()` panic on
# `wasm32-unknown-unknown`, which is `panic=abort`. The crates a browser build
# reads name their clock through `web_time`, which is `std::time` on every native
# target. This gate refuses `std::time::Instant` and `std::time::SystemTime` in
# every spelling - a qualified path (inside a macro's arguments too), a flat
# `use std::time::{...}`, a nested `use std::{..., time::{...}}`, a glob, an alias
# of `std::time` or of `std` - in the product code of an explicit source set.
#
# THE SOURCE SET (the test's `SOURCE_SET` is where it is written):
#   crates/{bt-unicode,bt-transcript,bt-doc,bt-layout,bt-viewport,bt-detect,
#           bt-effects,bt-math,bt-render,bt-term,bt-compose}/src
#   vendor/vte/src
#   vendor/alacritty_terminal/src minus `crate::event_loop` and `crate::tty`
#
# OUT OF SCOPE, BY NAME: bt-platform (its `http`, `install_txn` and `instance`
# modules read std::time today; its admission vocabulary is bt-effects' since
# CC-3), bt-app, bt-pty, bt-persist,
# bt-corpus, bt-winres, bt-workbench, bt-source, bt-lint-probe, vendor/mitex and
# vendor/mitex-parser. None is in the wasm32 graph; each joins the set in the
# ticket that brings it into that graph. `gates-can-fail` plants a clock in
# bt-platform's `http` module and requires this gate to stay green, so nothing
# outside the set joins it silently.
#
# "Product" is bt-source's production view (`Occurrence::in_the_product`): test
# files, `#[cfg(test)]` items and inline `#[cfg(test)]` modules are not read. The
# reading itself is `crates/bt-source/tests/clock_guard.rs`, which also runs in
# the workspace test; this script runs it alone, relays what it refused, and
# refuses a run that did not report reading the set (a filtered-out or renamed
# test passes with nothing read, which is not a pass).

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$test = "no_clock_of_the_standard_library_is_named_where_a_browser_build_reads"

$previous = [Console]::OutputEncoding
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
Push-Location $repo
try {
    $output = @(& cargo test -p bt-source --locked --color never --test clock_guard -- --exact $test --nocapture 2>&1 |
        ForEach-Object { "$_" })
    $status = $LASTEXITCODE
} finally {
    Pop-Location
    [Console]::OutputEncoding = $previous
}

$counts = @($output | Where-Object { $_ -match '^clock-guard: ' })
$summary = '^clock-guard: (\d+) file\(s\) read over (\d+) package'
$read = $output | Where-Object { $_ -match $summary } | Select-Object -First 1
$refused = @($output | Where-Object { $_ -match '^  (crates|vendor)/\S+:\d+:\d+: ' })

if ($status -ne 0) {
    if ($refused.Count -gt 0) {
        throw ("the clock guard refused $($refused.Count) site(s) where a wasm32 build reads; use web_time::Instant / web_time::SystemTime:" +
            [Environment]::NewLine + ($refused -join [Environment]::NewLine))
    }
    throw ("the clock guard did not pass and named no site (cargo exited $status):" + [Environment]::NewLine +
        (($output | Select-Object -Last 25) -join [Environment]::NewLine))
}
if (-not $read -or -not ($read -match $summary) -or [int]$Matches[1] -eq 0) {
    throw "the clock guard passed without reporting the files it read - a guard that read nothing has proved nothing"
}

$counts | ForEach-Object { Write-Host $_ }
Write-Host "the clock guard's source set names no clock of the standard library. PASS"
exit 0
