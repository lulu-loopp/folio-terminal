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

param(
    # **Print the `.rs` files the bt-app half of this gate walks, one per line
    # from `crates/bt-app/src`, and do nothing else.**
    #
    # `bt_app::platform_gate_tests::the_gate_and_its_script_walk_the_same_files`
    # is what asks for it (`docs/plans/bt-app-split-prep.md` §6.3, P10): there
    # are two readers of one rule, this one and the Rust twin, and a rule read
    # over two different sets of files is two rules. The twin runs this switch
    # and compares the answer with its own walk, so a file either of them stops
    # seeing is a red test rather than a gate that quietly covers less.
    #
    # It is not a change to the array reader below, which P10 leaves exactly as
    # it is: nothing here reads `main.rs`. It is `-ListSources` rather than
    # `-List` for the same reason — that reader's own answer is held in `$list`,
    # and a parameter of that name would be the variable it assigns to. `-List`
    # still reaches it, being an unambiguous prefix.
    [switch]$ListSources
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

# **The `.rs` files of `bt-app`, walked in one place.** Both the second check
# below and `-ListSources` ask for them here rather than each writing the walk
# out, because two walks is the drift the agreement test exists to catch.
$appSource = Join-Path $repo "crates/bt-app/src"

function Get-AppSourceFiles {
    Get-ChildItem -Path $script:appSource -Recurse -File -Filter *.rs |
        Sort-Object FullName |
        ForEach-Object {
            [pscustomobject]@{
                Path = $_.FullName
                Name = $_.FullName.Substring($script:appSource.Length + 1).Replace("\", "/")
            }
        }
}

if ($ListSources) {
    Get-AppSourceFiles | ForEach-Object { $_.Name }
    exit 0
}

# **The portable core, named one crate at a time.** A list rather than "every
# crate except bt-app", because the difference between the two is the whole
# claim: `bt-platform` is where Win32 belongs and `bt-app` is what has not been
# ported yet, and both of those are facts somebody decided rather than a shape a
# script can infer. A crate that joins this workspace joins this list on purpose
# or it is not part of the promise.
$portable = @(
    "bt-source",
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
    "bt-corpus",
    "bt-workbench"
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

$pin = Join-Path $appSource "main.rs"
if (-not (Test-Path -LiteralPath $pin)) {
    throw "crates/bt-app/src/main.rs is not in the tree - there is no list to read"
}

# ── reading that array as Rust reads it ───────────────────────────────────
#
# The list is read out of `main.rs` and not copied, so the reader has to read
# Rust the way Rust does. A regex that runs to the first `]` ends the array
# inside the first comment that spells `#[cfg(windows)]`, names a type as
# `[&str; 11]` or carries a link in brackets — and then this gate says a file
# that plainly declares the list no longer declares it, which is a sentence
# nobody can act on. These four functions walk the source instead: line
# comments, block comments (nested, as Rust's nest), character literals,
# strings and raw strings are read and stepped over, so that a `[` or a `]`
# counts only where it is one.
#
# The walk crosses eight megabytes of `main.rs` to reach the list, so it jumps
# between the characters that can begin something rather than reading every
# one, and it spells out what a literal *says* only for the entries themselves.

# The characters that can open something that is not code, and the two that can
# end a string. Hoisted because the walk asks for them a hundred thousand
# times; and the newline and the quote are `[char]` rather than one-character
# strings because `IndexOf(string, int)` compares by culture, which costs a
# collation pass over the rest of an eight-megabyte file every time it is
# asked, while `IndexOf(char, int)` is the ordinal search this wants.
$openers = [char[]]@("/", '"', "'")
$quoteOrBackslash = [char[]]@('"', "\")
$newline = [char]"`n"
$quote = [char]'"'

# The escape that starts at the backslash `$index`, as the character it stands
# for and the index just past it. A backslash before a newline is Rust's line
# continuation and stands for nothing at all.
function Read-RustEscape([string]$text, [int]$index) {
    $at = $index + 1
    if ($at -ge $text.Length) { return [pscustomobject]@{ Value = ""; Next = $at } }
    $c = $text[$at]
    if ($c -eq "n") { return [pscustomobject]@{ Value = "`n"; Next = $at + 1 } }
    if ($c -eq "r") { return [pscustomobject]@{ Value = "`r"; Next = $at + 1 } }
    if ($c -eq "t") { return [pscustomobject]@{ Value = "`t"; Next = $at + 1 } }
    if ($c -eq "0") { return [pscustomobject]@{ Value = "`0"; Next = $at + 1 } }
    if ($c -eq "x") {
        $hex = [regex]::Match($text.Substring($at, [Math]::Min(4, $text.Length - $at)),
            '^x([0-9a-fA-F]{2})')
        if ($hex.Success) {
            return [pscustomobject]@{
                Value = [string][char][Convert]::ToInt32($hex.Groups[1].Value, 16)
                Next  = $at + $hex.Length
            }
        }
    }
    if ($c -eq "u") {
        $point = [regex]::Match($text.Substring($at, [Math]::Min(16, $text.Length - $at)),
            '^u\{([0-9a-fA-F_]{1,6})\}')
        if ($point.Success) {
            $digits = $point.Groups[1].Value -replace "_", ""
            return [pscustomobject]@{
                Value = [char]::ConvertFromUtf32([Convert]::ToInt32($digits, 16))
                Next  = $at + $point.Length
            }
        }
    }
    if ($c -eq "`n" -or $c -eq "`r") {
        # A `\` at the end of a line eats the line break and the indentation
        # that follows it.
        while ($at -lt $text.Length -and [char]::IsWhiteSpace($text[$at])) { $at++ }
        return [pscustomobject]@{ Value = ""; Next = $at }
    }
    return [pscustomobject]@{ Value = [string]$c; Next = $at + 1 }
}

# The string literal whose opening quote is at `$index` — `"…"`, `b"…"`,
# `r"…"`, `r#"…"#` and their byte forms, since a raw string wears its prefix in
# front of the quote — as the index just past its closing quote, and, when
# `$withValue`, the text it stands for. `$null` when that quote opens no
# string.
#
# Stepping over a literal is the whole job nearly every time it is asked, and
# reading one character at a time to build a value nobody wants is what makes
# a walk over a large file slow, so without `$withValue` the search jumps quote
# to quote and spells nothing out.
function Read-RustString([string]$text, [int]$index, [bool]$withValue) {
    if ($text[$index] -ne '"') { return $null }

    # The prefix is read backwards: any number of `#`, then `r`, then an
    # optional `b`, and in front of all of it something that is not part of an
    # identifier — otherwise the `"` in `ready"` would open a raw string.
    $hashes = 0
    $before = $index - 1
    while ($before -ge 0 -and $text[$before] -eq "#") { $hashes++; $before-- }
    $raw = $false
    if ($before -ge 0 -and $text[$before] -eq "r") {
        $front = $before - 1
        if ($front -ge 0 -and $text[$front] -eq "b") { $front-- }
        if ($front -lt 0 -or -not ([char]::IsLetterOrDigit($text[$front]) -or $text[$front] -eq "_")) {
            $raw = $true
        }
    }
    if (-not $raw) { $hashes = 0 }

    $at = $index + 1
    if ($raw) {
        # A raw string has no escapes: it ends at the first quote followed by
        # as many `#` as opened it.
        while ($true) {
            $closing = $text.IndexOf($quote, $at)
            if ($closing -lt 0) { break }
            $closes = $true
            for ($h = 1; $h -le $hashes; $h++) {
                if ($closing + $h -ge $text.Length -or $text[$closing + $h] -ne "#") { $closes = $false; break }
            }
            if ($closes) {
                $value = $null
                if ($withValue) { $value = $text.Substring($index + 1, $closing - $index - 1) }
                return [pscustomobject]@{ Value = $value; Next = $closing + 1 + $hashes }
            }
            $at = $closing + 1
        }
        # An unterminated literal is not Rust; the walk stops where the file does.
        return [pscustomobject]@{ Value = $null; Next = $text.Length }
    }

    if (-not $withValue) {
        while ($true) {
            $next = $text.IndexOfAny($quoteOrBackslash, $at)
            if ($next -lt 0) { return [pscustomobject]@{ Value = $null; Next = $text.Length } }
            if ($text[$next] -eq '"') { return [pscustomobject]@{ Value = $null; Next = $next + 1 } }
            # A backslash spends the character after it, whatever it is.
            $at = $next + 2
        }
    }

    $value = New-Object System.Text.StringBuilder
    while ($at -lt $text.Length) {
        $c = $text[$at]
        if ($c -eq "\") {
            $escape = Read-RustEscape $text $at
            [void]$value.Append($escape.Value)
            $at = $escape.Next
            continue
        }
        if ($c -eq '"') { return [pscustomobject]@{ Value = $value.ToString(); Next = $at + 1 } }
        [void]$value.Append($c)
        $at++
    }
    return [pscustomobject]@{ Value = $value.ToString(); Next = $text.Length }
}

# The index just past the character literal that opens at `$index`, or `$null`
# when the quote opens a lifetime (`'static`) rather than a literal.
function Read-RustChar([string]$text, [int]$index) {
    $window = $text.Substring($index, [Math]::Min(24, $text.Length - $index))
    $literal = [regex]::Match($window, "^'(\\(x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^'\\]{1,2})'")
    if (-not $literal.Success) { return $null }
    return $index + $literal.Length
}

# The comment or literal that starts at `$index`, stepped over: the index of
# the first character after it. A `/` that is division and a `'` that opens a
# lifetime are code and carry no span, so they come back as `$index` itself.
function Skip-RustNonCode([string]$text, [int]$index) {
    $c = $text[$index]
    if ($c -eq "/") {
        if ($index + 1 -ge $text.Length) { return $index }
        $second = $text[$index + 1]
        if ($second -eq "/") {
            $end = $text.IndexOf($newline, $index)
            if ($end -lt 0) { return $text.Length }
            return $end + 1
        }
        if ($second -eq "*") {
            # Rust's block comments nest, and a `*/` inside a deeper one does
            # not end the outer.
            $nesting = 1
            $at = $index + 2
            while ($at + 1 -lt $text.Length -and $nesting -gt 0) {
                if ($text[$at] -eq "/" -and $text[$at + 1] -eq "*") { $nesting++; $at += 2; continue }
                if ($text[$at] -eq "*" -and $text[$at + 1] -eq "/") { $nesting--; $at += 2; continue }
                $at++
            }
            if ($nesting -gt 0) { return $text.Length }
            return $at
        }
        return $index
    }
    if ($c -eq '"') {
        $literal = Read-RustString $text $index $false
        if ($null -eq $literal) { return $index }
        return $literal.Next
    }
    if ($c -eq "'") {
        $character = Read-RustChar $text $index
        if ($null -eq $character) { return $index }
        return $character
    }
    return $index
}

# The entries of `const $name: [&str; N] = [ … ];`, read out of `$source`.
#
# `$null` when the source does not declare that constant in code — a comment
# quoting the declaration is not a declaration, which is why the search walks
# from the top of the file rather than trusting the first match. Otherwise
# `Entries` is what the array holds and `Closed` says whether the array the
# declaration opened was closed, so that "there is no such list" and "there is
# one and it is not a closed array of names" stay two different sentences.
function Read-RustStringArray([string]$source, [string]$name) {
    $candidates = [regex]::Matches($source, 'const\s+' + [regex]::Escape($name) + '\s*:')
    if ($candidates.Count -eq 0) { return $null }

    # The first candidate the walk reaches while it is in code is the
    # declaration; one the walk steps over on its way is a comment about it.
    $at = 0
    $start = -1
    foreach ($candidate in $candidates) {
        if ($candidate.Index -lt $at) { continue }
        $buried = $false
        while ($true) {
            $next = $source.IndexOfAny($openers, $at)
            if ($next -lt 0 -or $next -ge $candidate.Index) { break }
            $step = Skip-RustNonCode $source $next
            $at = if ($step -gt $next) { $step } else { $next + 1 }
            if ($at -gt $candidate.Index) { $buried = $true; break }
        }
        if (-not $buried) { $start = $candidate.Index + $candidate.Length; break }
    }
    if ($start -lt 0) { return $null }

    # `type` walks the constant's type to the `=` that ends it, `array` walks
    # the array itself, and `depth` is how many brackets deep that walk is: the
    # entries are the strings lying directly inside the first one.
    $entries = New-Object System.Collections.Generic.List[string]
    $stage = "type"
    $nesting = 0
    $depth = 0
    $at = $start
    while ($at -lt $source.Length) {
        $c = $source[$at]

        if ($c -eq "/" -or $c -eq '"' -or $c -eq "'") {
            if ($stage -eq "array" -and $depth -eq 1 -and $c -eq '"') {
                $literal = Read-RustString $source $at $true
                if ($null -ne $literal) {
                    [void]$entries.Add($literal.Value)
                    $at = $literal.Next
                    continue
                }
            }
            $step = Skip-RustNonCode $source $at
            $at = if ($step -gt $at) { $step } else { $at + 1 }
            continue
        }

        if ($stage -eq "type") {
            # `->` is an arrow in `fn() -> T`, not a bracket coming back.
            if ($c -eq "-" -and $at + 1 -lt $source.Length -and $source[$at + 1] -eq ">") {
                $at += 2
                continue
            }
            if ($c -eq "[" -or $c -eq "(" -or $c -eq "<") { $nesting++; $at++; continue }
            if ($c -eq "]" -or $c -eq ")" -or $c -eq ">") { $nesting--; $at++; continue }
            if ($nesting -le 0 -and $c -eq "=") { $stage = "equals"; $at++; continue }
            if ($nesting -le 0 -and $c -eq ";") {
                return [pscustomobject]@{ Entries = @(); Closed = $false }
            }
            $at++
            continue
        }

        if ($stage -eq "equals") {
            if ($c -eq "[") { $stage = "array"; $depth = 1; $at++; continue }
            if ([char]::IsWhiteSpace($c)) { $at++; continue }
            # The constant is declared and its value is not an array literal.
            return [pscustomobject]@{ Entries = @(); Closed = $false }
        }

        if ($c -eq "[") { $depth++; $at++; continue }
        if ($c -eq "]") {
            $depth--
            $at++
            if ($depth -le 0) {
                return [pscustomobject]@{ Entries = $entries.ToArray(); Closed = $true }
            }
            continue
        }
        $at++
    }
    return [pscustomobject]@{ Entries = $entries.ToArray(); Closed = $false }
}

# ── the reader's own fixture, which runs every time this gate does ─────────
#
# Red against the regex this replaced, which ended the array at the `]` in the
# first line comment and came back with one name. Every bracket and quote below
# is somewhere it must not count: a line comment, a nested block comment, a
# name, the type, and a second constant the walk must never reach.
$fixture = @'
mod platform_gate_tests {
    /// The list, typed `[&str; 4]`, named in a doc link [main.rs] and in an
    /// attribute spelling, `#[cfg(windows)]`.
    const FILES_THAT_MAY_NAME_A_PLATFORM: [&str; 4] = [
        // `#[cfg(windows)]` — a bracket in a line comment.
        "attention_copilot.rs",
        /* a block comment holding `]`, /* a nested one holding "]", */ and an
           unpaired quote: " */
        "cli.rs",
        // A name is whatever it says it is, brackets and quotes included.
        "explorer]\"menu.rs",
        r#"raw]"name.rs"#,
    ];
    const NOT_THIS_ONE: [&str; 1] = ["the walk stopped at the ] above"];
}
'@
$fixtureWanted = @("attention_copilot.rs", "cli.rs", "explorer]`"menu.rs", "raw]`"name.rs")
$fixtureRead = @((Read-RustStringArray $fixture "FILES_THAT_MAY_NAME_A_PLATFORM").Entries)
$fixtureAgrees = $fixtureRead.Count -eq $fixtureWanted.Count
for ($i = 0; $fixtureAgrees -and $i -lt $fixtureWanted.Count; $i++) {
    if ($fixtureRead[$i] -cne $fixtureWanted[$i]) { $fixtureAgrees = $false }
}
if (-not $fixtureAgrees) {
    throw ("the reader of FILES_THAT_MAY_NAME_A_PLATFORM cannot read its own fixture, whose " +
        "comments, names and type all contain a bracket: it should have read " +
        ($fixtureWanted -join ", ") + " and read " + ($fixtureRead -join ", ") +
        ". Nothing this gate says below that line is worth reading until it can.")
}

# The list as `main.rs` declares it. Read rather than repeated: see the note
# above.
$pinText = [IO.File]::ReadAllText($pin)
$list = Read-RustStringArray $pinText "FILES_THAT_MAY_NAME_A_PLATFORM"
if ($null -eq $list) {
    throw ("crates/bt-app/src/main.rs no longer declares FILES_THAT_MAY_NAME_A_PLATFORM, which " +
        "is the list this gate and its Rust twin both read. If the pin moved, move this with it.")
}
if (-not $list.Closed) {
    throw ("crates/bt-app/src/main.rs declares FILES_THAT_MAY_NAME_A_PLATFORM and then does not " +
        "give it a closed array of names, so there is nothing here for this gate to read. Rust " +
        "will not take that source either.")
}
$allowed = @($list.Entries)
if ($allowed.Count -lt 5) {
    throw "the list read out of main.rs has $($allowed.Count) entries, which is not that list"
}

$platformWords = @("windows", "unix", "macos", "target_os", "target_family")
$asking = @{}
$strangers = @()

foreach ($file in Get-AppSourceFiles) {
    $relative = $file.Name
    $lines = [IO.File]::ReadAllLines($file.Path)
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
