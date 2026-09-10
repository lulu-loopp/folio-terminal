# the_portable_core_names_no_win32_outside_a_cfg
#
# **The rule, and it is one sentence: platform-specific code lives behind
# `bt-platform`'s interface, and no crate below `bt-app` calls Win32 directly.**
#
# `scripts/check-adapter-boundary.ps1` is the other half of the same discipline —
# it says the vendor seam may not import a policy crate — and the two are kept
# apart because they answer different questions about different files. This one
# is about a *platform*: `docs/plans/port/macos-spike-2026-09-07.md` measured
# that 90% of this workspace already compiles on macOS, and the way that stops
# being true is not a decision anybody takes, it is a `windows::` import that
# nobody notices because every machine in CI is a Windows machine.
#
# The `core-macos` and `core-linux` jobs in `.github/workflows/ci.yml` are the
# other guard on the same drift and they are the stronger one: they compile.
# This gate exists beside them because a compile failure names a symbol and a
# line, while a rule names the rule — and because a `#[cfg(windows)]` block that
# is *correct* still hides the decision to write one, which is a thing a
# reviewer should see in a diff.
#
# WHAT IS REFUSED: a reference to `windows::`, `windows_sys::`, `winapi`,
# `webview2` or `std::os::windows` in one of the crates listed below, unless the
# item carrying it is inside a `#[cfg(windows)]` / `#[cfg(target_os = "windows")]`
# / `#[cfg(all(..., windows))]` gate.
#
# HOW THE GATE READS "GATED": the file is walked once, tracking brace depth. A
# `#[cfg(...)]` attribute naming windows arms the next item; the item's extent is
# the block it opens, and everything inside that block is gated. An attribute on
# a `use`, a `const`, an `fn` signature or a `mod` all work the same way, and a
# `#![cfg(...)]` inner attribute gates the whole file. This is deliberately a
# *lexical* reading and not a parse: a gate that needed `syn` would be a gate
# that needs a build, and this one has to be able to run in five seconds on a
# tree that does not compile.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

# **The portable core, named one crate at a time.** A list rather than "every
# crate except bt-app", because the difference between the two is the whole
# claim: `bt-platform` is where Win32 belongs and `bt-app` is what has not been
# ported yet, and both of those are facts somebody decided rather than a shape a
# script can infer. A crate that joins this workspace joins this list on purpose
# or it is not part of the promise.
$portable = @(
    "bt-unicode",
    "bt-doc",
    "bt-detect",
    "bt-layout",
    "bt-persist",
    "bt-winres",
    "bt-math",
    "bt-transcript",
    "bt-viewport",
    "bt-render",
    "bt-term",
    "bt-pty",
    "bt-corpus"
)

# Each is a whole-word match against the source, so `windows_sys` does not answer
# for `windows` and a comment about "the Windows build" is not a call into it.
$forbidden = @(
    '\bwindows\s*::',
    '\bwindows_sys\s*::',
    '\bwinapi\s*::',
    '\bwebview2\w*\s*::',
    '\bstd\s*::\s*os\s*::\s*windows\b',
    '^\s*use\s+windows\s*;',
    '^\s*extern\s+crate\s+(windows|windows_sys|winapi)\b'
)

# A `#[cfg(...)]` (or `#![cfg(...)]`) whose predicate mentions this platform. The
# spellings that reach it are `windows`, `target_os = "windows"`, and either of
# those inside `all(...)` / `any(...)` / `not(not(...))`; `not(windows)` is
# deliberately NOT one of them, which is why the word is looked for without a
# `not(` in front of it.
$windowsCfg = '#!?\[\s*cfg\s*\('

function Test-GatesWindows([string]$attribute) {
    # `not(windows)` and `not(target_os = "windows")` gate the *other* platform,
    # so an item under one of them naming Win32 is exactly the mistake this looks
    # for. Strip every `not(...)` group first, then ask whether what is left
    # still names Windows.
    $stripped = $attribute
    for ($i = 0; $i -lt 8; $i++) {
        $next = [regex]::Replace($stripped, 'not\s*\([^()]*\)', '')
        if ($next -eq $stripped) { break }
        $stripped = $next
    }
    return ($stripped -match '(^|[^\w"])windows([^\w"]|$)') -or
           ($stripped -match 'target_os\s*=\s*"windows"') -or
           ($stripped -match 'target_family\s*=\s*"windows"')
}

$violations = @()

foreach ($crate in $portable) {
    $root = Join-Path $repo "crates/$crate/src"
    if (-not (Test-Path -LiteralPath $root)) {
        throw "crates/$crate/src is not in the tree - the portable-core list names a crate that is not here"
    }
    foreach ($file in Get-ChildItem -Path $root -Recurse -File -Filter *.rs) {
        $lines = [IO.File]::ReadAllLines($file.FullName)
        $relative = $file.FullName.Substring($repo.Length + 1).Replace("\", "/")

        $depth = 0
        # Depths at which a windows gate is open. An item gated at depth d covers
        # everything until the brace depth falls back to d.
        $gatedFrom = @()
        $fileIsGated = $false
        # Set when a `#[cfg(windows)]` has been read and the item it belongs to
        # has not opened its block yet.
        $pending = $false
        $pendingDepth = 0

        for ($index = 0; $index -lt $lines.Length; $index++) {
            $line = $lines[$index]
            $code = $line -replace '//.*$', ''

            if ($code -match $windowsCfg) {
                # The attribute may wrap over several lines; join until the
                # brackets balance, so `#[cfg(all(\n test,\n windows\n ))]` reads
                # as one predicate.
                $attribute = $code
                $scan = $index
                while (($attribute.ToCharArray() | Where-Object { $_ -eq '(' }).Count -ne
                       ($attribute.ToCharArray() | Where-Object { $_ -eq ')' }).Count) {
                    $scan++
                    if ($scan -ge $lines.Length) { break }
                    $attribute += " " + ($lines[$scan] -replace '//.*$', '')
                }
                if (Test-GatesWindows $attribute) {
                    if ($code -match '#!\[') {
                        $fileIsGated = $true
                    } else {
                        $pending = $true
                        $pendingDepth = $depth
                    }
                }
                # An attribute line carries no code of its own worth scanning.
                $depth += ($code.ToCharArray() | Where-Object { $_ -eq '{' }).Count
                $depth -= ($code.ToCharArray() | Where-Object { $_ -eq '}' }).Count
                continue
            }

            $gated = $fileIsGated -or $pending -or ($gatedFrom.Count -gt 0)

            if (-not $gated) {
                foreach ($pattern in $forbidden) {
                    if ($code -match $pattern) {
                        $violations += "${relative}:$($index + 1): $($line.Trim())"
                        break
                    }
                }
            }

            $opens = ($code.ToCharArray() | Where-Object { $_ -eq '{' }).Count
            $closes = ($code.ToCharArray() | Where-Object { $_ -eq '}' }).Count

            if ($pending) {
                if ($opens -gt 0) {
                    # The gated item opened a block: it stays gated until the
                    # depth comes back.
                    $gatedFrom += $pendingDepth
                    $pending = $false
                } elseif ($code -match ';\s*$') {
                    # A one-line gated item (`use ...;`, `const X: u32 = 1;`).
                    $pending = $false
                }
            }

            $depth += $opens - $closes
            while ($gatedFrom.Count -gt 0 -and $depth -le $gatedFrom[-1]) {
                $gatedFrom = @($gatedFrom[0..($gatedFrom.Count - 1)] | Select-Object -SkipLast 1)
            }
        }
    }
}

if ($violations.Count -gt 0) {
    $details = ($violations | Sort-Object) -join [Environment]::NewLine
    throw ("the portable core calls Win32 outside a #[cfg(windows)] gate:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Platform-specific code lives behind bt-platform's interface. If this really is " +
        "Windows-only, gate the item; if it is not, it belongs in bt-platform.")
}

Write-Host "the $($portable.Count) portable crates name no Win32 outside a #[cfg(windows)] gate"
