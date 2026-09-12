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

# ── the second check: bt-app knows what platform it is on in a named list of
#    files, and nowhere else ────────────────────────────────────────────────
#
# The first check is about the thirteen crates that must not name a platform at
# all. This one is about the one crate that may, and it asks the opposite
# question: *where*. `docs/plans/port/macos-plan-2026-09-12.md` §4.3 states the
# design as a ratio — `bt-app` names `bt_platform::` at several hundred call
# sites with no gate, and asks what machine it is on in a short list of files —
# and a ratio that nothing enforces is a sentence about a day in September.
#
# **This is the twin of `bt_app::platform_gate_tests::
# only_the_named_files_decide_what_platform_this_is`, and it reads that test's
# own array out of `main.rs` rather than keeping a second copy.** One list, two
# readers, exactly as `core-macos` and `core-linux` read one crate list: the
# Rust pin runs on three platforms in CI and names the file and the line; this
# runs in five seconds on a tree that does not compile and can be run before a
# commit. A list that lived in both files would drift, and drift in a gate is a
# gate that is decoration.
#
# The rule counts both spellings of the question, because they are one question:
# the attribute (`#[cfg(...)]`, `#![cfg(...)]`, `#[cfg_attr(...)]`) compiles one
# arm or the other, and the macro (`cfg!(...)`) answers it as a `bool`. `test`,
# `debug_assertions` and `feature = "..."` are not statements about a machine and
# are not counted.

$appSource = Join-Path $repo "crates/bt-app/src"
$pin = Join-Path $appSource "main.rs"
if (-not (Test-Path -LiteralPath $pin)) {
    throw "crates/bt-app/src/main.rs is not in the tree - there is no list to read"
}

# The array as `main.rs` writes it, from the opening bracket to the `];` that
# closes it. Read rather than repeated: see the note above.
$pinText = [IO.File]::ReadAllText($pin)
$match = [regex]::Match(
    $pinText,
    'const\s+FILES_THAT_MAY_NAME_A_PLATFORM\s*:\s*\[&str;\s*\d+\]\s*=\s*\[(?<body>[^\]]*)\]\s*;'
)
if (-not $match.Success) {
    throw ("crates/bt-app/src/main.rs no longer declares FILES_THAT_MAY_NAME_A_PLATFORM, which " +
        "is the list this gate and its Rust twin both read. If the pin moved, move this with it.")
}
# Every entry carries a comment saying why it is on the list, and those comments
# contain quoted English. The names are the strings in the *code*, so the
# comments go first - the same reading every other walk in this file takes.
$body = ($match.Groups["body"].Value -split "`n" |
    ForEach-Object { $_ -replace '//.*$', '' }) -join "`n"
$allowed = [regex]::Matches($body, '"(?<name>[^"]+)"') |
    ForEach-Object { $_.Groups["name"].Value }
if ($allowed.Count -lt 5) {
    throw "the list read out of main.rs has $($allowed.Count) entries, which is not that list"
}

$platformWords = @("windows", "unix", "macos", "target_os", "target_family")
$asking = @{}
$strangers = @()

foreach ($file in Get-ChildItem -Path $appSource -Recurse -File -Filter *.rs) {
    $relative = $file.FullName.Substring($appSource.Length + 1).Replace("\", "/")
    $lines = [IO.File]::ReadAllLines($file.FullName)
    for ($index = 0; $index -lt $lines.Length; $index++) {
        # A comment is prose about a rule and not a use of it - the same reading
        # the walk above takes, and the same one the Rust twin takes.
        $code = $lines[$index] -replace '//.*$', ''
        if ($code -notmatch 'cfg\(|cfg!\(|cfg_attr\(') { continue }
        $named = $false
        foreach ($word in $platformWords) {
            if ($code.Contains($word)) { $named = $true; break }
        }
        if (-not $named) { continue }
        if (-not $asking.ContainsKey($relative)) {
            $asking[$relative] = $index + 1
            if ($allowed -notcontains $relative) {
                $strangers += "${relative}:$($index + 1): $($lines[$index].Trim())"
            }
        }
    }
}

if ($strangers.Count -gt 0) {
    $details = ($strangers | Sort-Object) -join [Environment]::NewLine
    throw ("bt-app decides what platform it is on outside the list main.rs keeps:" +
        [Environment]::NewLine + $details + [Environment]::NewLine +
        "Platform code lives behind bt-platform's interface. If the call belongs there, move " +
        "it; if this file really has to ask, add it to FILES_THAT_MAY_NAME_A_PLATFORM with the " +
        "reason, which admits it to this gate and to its Rust twin at once.")
}

$silent = @($allowed | Where-Object { -not $asking.ContainsKey($_) })
if ($silent.Count -gt 0) {
    throw ("these names are on bt-app's list and no longer name a platform, so the list is " +
        "promising less than it says: " + ($silent -join ", "))
}

Write-Host "bt-app names a platform in the $($allowed.Count) files its own list admits"
