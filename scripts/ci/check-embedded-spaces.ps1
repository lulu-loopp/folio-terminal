# the_embedded_spaces_list_only_shrinks
#
# **A message literal is one sentence.** A string literal that wraps onto a
# second source line continues with `\` at the end of the line, and Rust drops
# the line break and the indentation after it. When the `\` is lost - an edit
# that joins two lines, a literal re-wrapped by hand - the indentation lands
# inside the string, and the message reads "within the lane" and a dozen spaces
# and "suite's patience". The compiler accepts it, rustfmt leaves it, and a
# test's assertion text carries it unseen. This gate refuses the class: every
# string literal of first-party Rust with a run of three or more spaces
# (defined below) is either repaired or on
# `docs/plans/embedded-spaces-allowed.txt`, the list of literals whose columns
# are deliberate (fixture tables, aligned samples), and that list only shrinks.
# The rule is `docs/CONVENTIONS.md` §五's sentence on message literals.
#
# WHAT IS READ. Every `.rs` file `git ls-files` names under `crates/` - so no
# `vendor/` (not under `crates/`) and no `target/` (not tracked) - with its
# line endings read as `\n`. The file is lexed, not parsed: comments (line,
# block, nested), character literals and lifetimes are skipped as what they
# are, and every string literal - `"…"`, `b"…"`, `c"…"`, and the raw forms
# `r"…"`, `r#"…"#`, `br…`, `cr…` - is read as its source text between the
# quotes. A shape the lexer cannot finish is refused by its file:line, never
# passed and never read to the end of the file: a string, raw string or block
# comment that does not close, and a character literal whose escape or closing
# quote is missing. The rest of that file is not read until it is fixed.
#
# WHAT IS A RUN. Three or more consecutive spaces in a literal's source text,
# in one of two places:
#
#   * inside a source line: a character other than a space before them (a tab
#     counts as such a character, so `a  <tab>   b` holds a run after the tab)
#     and, after them and any further blanks, a character that is not a space
#     or a tab, on the same source line - a run between two words, before a `\`
#     continuation, after an escaped `\n` that was meant to be a continuation;
#   * at the start of a source line of an escaping (non-raw) literal that
#     follows a real line break - one with no `\` continuation before it - when
#     a character that is not a blank follows: the shape a `\` lost at the end
#     of a wrapped line leaves behind.
#
# What is not a run: the blanks right after the opening quote (padding), the
# blanks after a `\` continuation (Rust drops them), the blanks that end a
# source line (before the closing quote or a line break - a lost continuation
# always has words on both sides), and the indentation of a raw literal's
# lines (a raw literal is how a fixture keeps its own line breaks and
# indentation; its mid-line runs are runs all the same).
#
# THE LIST. One row per literal, `path:line reason`: the path from the
# repository root with `/`, the line the literal opens on, and why its columns
# are deliberate. Two literals with runs that open on one line take two rows
# with that line. `#` lines and blank lines are commentary. In the tree the
# list is exact both ways: every line carries as many rows as it opens
# literals with a run - a literal without its row is refused by its file:line
# (and the line of the run, when that is a later one), and a row beyond that
# count is refused as stale.
#
# THE IDENTITY of a row is its path, its literal's own text on the source line
# of its first run (the characters inside the literal on that line, trimmed -
# not the code around it) and its reason; its line is where that literal
# opens today. Rows of one line pair with that line's literals in order. So a
# literal that moves (an edit above it, a rename of the code beside it, a
# rustfmt rewrap) keeps its row with the line corrected, and a row pointed at
# a different literal is a different row. An edit to a listed literal's
# pinned text is a new row, and refused: the list only shrinks, with no
# override and no replacement path. Columns that must evolve leave string
# literals instead - cells in a slice joined at run time, or a fixture file
# read through a listed door.
#
# THE COMPARISON is the dead-code gate's (`check-dead-code.ps1`): against the
# pull request's merge base with `origin/main`, with multiplicity. The base's
# identities are read from the base itself: its list, and each listed file as
# the base holds it (`git show`, decoded as UTF-8), lexed by the same code
# path as the tree; a shape the lexer cannot finish there is reported as
# `base <sha>:<path>:<line>: <shape>` and fails the check (the base is main,
# and main must lex). A row
# whose identity the merge base does not have is refused: a literal is
# repaired, never listed - except by the commit that introduces the list, and
# except a listed literal **moved** to another file (T-MOVED-NOT-ADDED-RULE,
# `moved-rows.ps1`): a row the base does not have whose literal's text and
# reason are a gone base row's, in another file, is that row moved - when that
# text and reason left one file for one other, and no more rows arrived than
# left (a literal names no item, so the count is the whole of "it left"). It
# passes, loudly, when the base has no list - that is how the commit that
# introduces the list passes. It refuses (exit 2) when there is no base at
# all: no `origin/main` in the clone, or no merge base, means the comparison
# did not happen. CI runs it in jobs that check out with `fetch-depth: 0`.
#
# The lexer runs in C# compiled at start-up, as the dead-code gate's does: the
# first-party Rust is some forty-five megabytes, and a character loop in
# PowerShell takes minutes over it.
#
# `-Rows` prints, for the tree as it stands, one `path:line` per literal with a
# run, the line of its first run and that source line, and checks nothing: it
# is how the list was written, and how a triage reads the sites.
#
# Prove it fires before trusting it: `scripts/ci/check-embedded-spaces-tests.ps1`
# runs this script against scratch repositories, one case per rule, and
# `gates-can-fail` in `.github/workflows/ci.yml` runs that file and plants a
# literal in the real tree.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [switch]$Rows
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

. (Join-Path $PSScriptRoot "moved-rows.ps1")

$repo = $Repo
$relative = "docs/plans/embedded-spaces-allowed.txt"
$list = Join-Path $repo $relative

Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;

namespace FolioEmbeddedSpacesGate
{
    public sealed class Run
    {
        public int Opens;     // the line the literal opens on
        public int Line;      // the line of its first run
        public string Text;   // the literal's own text on that source line, trimmed
    }

    public sealed class Scan
    {
        public List<Run> Runs = new List<Run>();
        public int FaultLine; // 0 when the lexer finished the file
        public string Fault;
    }

    public static class Scanner
    {
        static bool IdentStart(char c) { return c == '_' || char.IsLetter(c); }
        static bool IdentPart(char c) { return c == '_' || char.IsLetterOrDigit(c); }
        static bool Blank(char c) { return c == ' ' || c == '\t'; }

        // Every string literal of `s` that holds a run, one entry per literal, in source order;
        // or, where the lexer cannot finish a shape, the runs before it and that shape's line.
        public static Scan Read(string s)
        {
            var scan = new Scan();
            int i = 0, n = s.Length, line = 1;
            while (i < n)
            {
                char c = s[i];
                if (c == '\n') { line++; i++; continue; }
                if (c == '/' && i + 1 < n && s[i + 1] == '/')
                {
                    while (i < n && s[i] != '\n') i++;
                    continue;
                }
                if (c == '/' && i + 1 < n && s[i + 1] == '*')
                {
                    int opens = line;
                    int depth = 1; i += 2;
                    while (i < n && depth > 0)
                    {
                        if (s[i] == '\n') { line++; i++; }
                        else if (s[i] == '/' && i + 1 < n && s[i + 1] == '*') { depth++; i += 2; }
                        else if (s[i] == '*' && i + 1 < n && s[i + 1] == '/') { depth--; i += 2; }
                        else i++;
                    }
                    if (depth > 0) return Fail(scan, opens, "a block comment that does not close");
                    continue;
                }
                if (IdentStart(c))
                {
                    int start = i;
                    while (i < n && IdentPart(s[i])) i++;
                    string id = s.Substring(start, i - start);
                    if ((id == "r" || id == "br" || id == "cr") && i < n && (s[i] == '"' || (s[i] == '#' && RawAhead(s, i))))
                    {
                        int hashes = 0;
                        while (s[i] == '#') { hashes++; i++; }
                        i++;
                        int from = i;
                        while (i < n && !(s[i] == '"' && Closes(s, i + 1, hashes))) i++;
                        if (i >= n) return Fail(scan, line, "a raw string that does not close");
                        Judge(s, from, i, line, true, scan.Runs);
                        line += Count(s, from, i);
                        i += 1 + hashes;
                        continue;
                    }
                    if (id == "r" && i + 1 < n && s[i] == '#' && IdentStart(s[i + 1]))
                    {
                        i++;
                        while (i < n && IdentPart(s[i])) i++;
                        continue;
                    }
                    if ((id == "b" || id == "c") && i < n && s[i] == '"')
                    {
                        int opens = line;
                        i = Quoted(s, i, ref line, scan.Runs);
                        if (i < 0) return Fail(scan, opens, "a string that does not close");
                        continue;
                    }
                    if (id == "b" && i < n && s[i] == '\'')
                    {
                        i = SkipChar(s, i);
                        if (i < 0) return Fail(scan, line, "a character literal that does not close");
                        continue;
                    }
                    continue;
                }
                if (c == '"')
                {
                    int opens = line;
                    i = Quoted(s, i, ref line, scan.Runs);
                    if (i < 0) return Fail(scan, opens, "a string that does not close");
                    continue;
                }
                if (c == '\'')
                {
                    // A character literal, or a lifetime / label: `'a'` against `'a`.
                    if (i + 1 < n && s[i + 1] == '\\')
                    {
                        i = SkipChar(s, i);
                        if (i < 0) return Fail(scan, line, "a character literal that does not close");
                        continue;
                    }
                    int width = (i + 1 < n && char.IsHighSurrogate(s[i + 1])) ? 2 : 1;
                    if (i + 1 + width < n && s[i + 1 + width] == '\'') { i += 2 + width; continue; }
                    i++;
                    while (i < n && IdentPart(s[i])) i++;
                    continue;
                }
                if (char.IsDigit(c))
                {
                    while (i < n && IdentPart(s[i])) i++;
                    continue;
                }
                i++;
            }
            return scan;
        }

        static Scan Fail(Scan scan, int line, string what)
        {
            scan.FaultLine = line;
            scan.Fault = what;
            return scan;
        }

        static bool RawAhead(string s, int i)
        {
            while (i < s.Length && s[i] == '#') i++;
            return i < s.Length && s[i] == '"';
        }

        static bool Closes(string s, int i, int hashes)
        {
            for (int k = 0; k < hashes; k++)
                if (i + k >= s.Length || s[i + k] != '#') return false;
            return true;
        }

        static int Count(string s, int from, int to)
        {
            int lines = 0;
            for (int k = from; k < to; k++) if (s[k] == '\n') lines++;
            return lines;
        }

        // An escaping string from its opening quote; returns the index after the closing one,
        // or -1 when the file ends first.
        static int Quoted(string s, int i, ref int line, List<Run> runs)
        {
            int n = s.Length;
            int from = ++i;
            while (i < n && s[i] != '"')
            {
                if (s[i] == '\\' && i + 1 < n) { i += 2; continue; }
                i++;
            }
            if (i >= n) return -1;
            Judge(s, from, i, line, false, runs);
            line += Count(s, from, i);
            return i + 1;
        }

        // Whether the source line that ends at `end` (a line break inside an escaping literal)
        // ends in a `\` continuation: an odd number of backslashes right before the break.
        static bool Continues(string s, int lineStart, int end)
        {
            int k = end, slashes = 0;
            while (k > lineStart && s[k - 1] == '\\') { slashes++; k--; }
            return slashes % 2 == 1;
        }

        // The literal's source text is s[from..to), opening on `line`, and each of its source
        // lines is s[k..end). Its first run, if any, is recorded (the header says what a run is).
        static void Judge(string s, int from, int to, int line, bool raw, List<Run> runs)
        {
            int at = line;
            int k = from;
            bool first = true;
            bool continued = false;
            while (true)
            {
                int end = s.IndexOf('\n', k, to - k);
                if (end < 0) end = to;
                int p = k;
                int spaces = 0, widest = 0;
                while (p < end && Blank(s[p]))
                {
                    spaces = s[p] == ' ' ? spaces + 1 : 0;
                    if (spaces > widest) widest = spaces;
                    p++;
                }
                bool found = !first && !raw && !continued && widest >= 3 && p < end;
                while (!found && p < end)
                {
                    if (s[p] != ' ') { p++; continue; }
                    int run = p;
                    while (p < end && s[p] == ' ') p++;
                    if (p - run < 3) continue; // a tab after a short run is the next run's boundary
                    while (p < end && Blank(s[p])) p++;
                    found = p < end;
                }
                if (found)
                {
                    // The literal's own text on this source line: what stands between the quotes
                    // (or the line's ends), not the code around it.
                    runs.Add(new Run { Opens = line, Line = at, Text = s.Substring(k, end - k).Trim() });
                    return;
                }
                if (end == to) return;
                continued = !raw && Continues(s, k, end);
                first = false;
                at++;
                k = end + 1;
            }
        }

        // A character literal from its opening quote (or the `b` before it has been read);
        // returns the index after its closing quote, or -1 when the escape or the quote is missing.
        static int SkipChar(string s, int i)
        {
            int n = s.Length;
            i++;
            if (i >= n) return -1;
            if (s[i] == '\\')
            {
                i++;
                if (i >= n) return -1;
                if (s[i] == 'u')
                {
                    int close = s.IndexOf('}', i);
                    int stop = s.IndexOf('\n', i);
                    if (close < 0 || (stop >= 0 && stop < close)) return -1;
                    i = close + 1;
                }
                else if (s[i] == 'x') i += 3;
                else i++;
            }
            else
            {
                i += char.IsHighSurrogate(s[i]) ? 2 : 1;
            }
            if (i >= n || s[i] != '\'') return -1;
            return i + 1;
        }
    }
}
'@

# One file's literals with a run, in source order, each with its `path:line` key and its text;
# a shape the lexer cannot finish is a problem named by its file:line.
function Read-Literals([string]$File, [string]$Text, [System.Collections.Generic.List[string]]$Problems) {
    $scan = [FolioEmbeddedSpacesGate.Scanner]::Read($Text.Replace("`r`n", "`n"))
    if ($scan.FaultLine -gt 0) {
        $Problems.Add("${File}:$($scan.FaultLine): $($scan.Fault) - the lexer cannot read on, and the rest of the file is not checked")
    }
    foreach ($run in $scan.Runs) {
        [pscustomobject]@{ Key = "${File}:$($run.Opens)"; Line = $run.Line; Text = $run.Text; Path = $File }
    }
}

$problems = [System.Collections.Generic.List[string]]::new()
$found = [System.Collections.Generic.List[object]]::new()
Push-Location $repo
try {
    $files = @(& git ls-files -- "crates/*.rs")
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed in $repo" }
} finally {
    Pop-Location
}
foreach ($file in ($files | Sort-Object)) {
    foreach ($literal in (Read-Literals $file ([IO.File]::ReadAllText((Join-Path $repo $file))) $problems)) {
        $literal | Add-Member -NotePropertyName Ordinal -NotePropertyValue $found.Count
        $found.Add($literal)
    }
}

if ($Rows) {
    foreach ($literal in $found) {
        Write-Output "$($literal.Key)`trun on line $($literal.Line)`t$($literal.Text)"
    }
    foreach ($problem in $problems) { Write-Output "PROBLEM`t$problem" }
    exit 0
}

# A list row: `path:line reason`, in file order.
function Read-AllowRows([string[]]$lines) {
    $rows = @()
    foreach ($line in $lines) {
        if ($line.Trim().Length -eq 0 -or $line.StartsWith("#")) { continue }
        if ($line -notmatch '^(?<path>[^\s:]+):(?<line>[1-9][0-9]*) (?<reason>\S.*)$') {
            throw "$relative has a row that is not ``path:line reason``: $line"
        }
        $rows += [pscustomobject]@{
            Key    = "$($Matches.path):$($Matches.line)"
            Path   = $Matches.path
            Reason = $Matches.reason
        }
    }
    return , $rows
}

# The identities of `rows` against `literals`: the rows of one key pair in order with that key's
# literals in source order, and a row past its key's literals has none (it is stale).
function Get-Identities($rows, $literals) {
    $byKey = @{}
    foreach ($literal in $literals) {
        if (-not $byKey.ContainsKey($literal.Key)) { $byKey[$literal.Key] = [System.Collections.Generic.List[object]]::new() }
        $byKey[$literal.Key].Add($literal)
    }
    $taken = @{}
    foreach ($row in $rows) {
        $index = if ($taken.ContainsKey($row.Key)) { $taken[$row.Key] } else { 0 }
        $taken[$row.Key] = $index + 1
        $literal = if ($byKey.ContainsKey($row.Key) -and $index -lt $byKey[$row.Key].Count) { $byKey[$row.Key][$index] } else { $null }
        [pscustomobject]@{
            Row      = $row
            Literal  = $literal
            Identity = if ($literal) { "$($row.Path)`t$($literal.Text)`t$($row.Reason)" } else { $null }
        }
    }
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - it is the list of literals whose runs of spaces are deliberate"
}
$now = Read-AllowRows ([IO.File]::ReadAllLines($list))
$paired = @(Get-Identities $now $found)

$listed = @{}
foreach ($pair in $paired) {
    if ($pair.Literal) {
        $listed[$pair.Literal.Ordinal] = $true
    } else {
        $problems.Add("${relative}: $($pair.Row.Key) is stale - no further string literal with a run of spaces opens on that line; remove the row (a moved literal keeps its row with the line corrected; an edited one is a new row, refused - take deliberate columns out of string literals)")
    }
}
foreach ($literal in $found) {
    if (-not $listed.ContainsKey($literal.Ordinal)) {
        $at = if ("$($literal.Path):$($literal.Line)" -ne $literal.Key) { " (the run is on line $($literal.Line))" } else { "" }
        $problems.Add("$($literal.Key): a string literal carries a run of three or more spaces${at}: $($literal.Text)")
    }
}

Write-Host "${relative}: $($found.Count) literal(s) with a run in the tree, $($now.Count) row(s)."

if ($problems.Count -gt 0) {
    # One line each, on the output stream: a thrown message is wrapped to the console's width,
    # and a file:line broken across two lines is not one a reader (or a test) can find.
    foreach ($problem in $problems) { Write-Host "    $problem" }
    throw (
        "$($problems.Count) embedded-spaces problem(s), listed above. A run of spaces inside a message " +
        "is a lost line continuation: end the line with ``\`` (Rust drops the break and the next " +
        "line's indentation). Deliberate columns stay only on the rows the list already has (it only shrinks)."
    )
}

# `git show` of a blob at the base, decoded as UTF-8 (the reason is in check-dead-code.ps1: the
# console's code page would turn non-ASCII text into different strings); $null when absent.
function Show-AtBase([string]$Base, [string]$Path) {
    $consoleEncoding = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = [Text.Encoding]::UTF8
        $text = (& git show "${Base}:${Path}" 2>$null) | Out-String
        if ($LASTEXITCODE -ne 0) { return $null }
        return $text
    } finally {
        [Console]::OutputEncoding = $consoleEncoding
    }
}

Push-Location $repo
try {
    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        # git's own status, read before any other stage can stand on it (the reason is in
        # `check-migration-debt.ps1`: a native command piped into `Select-Object -First 1`
        # loses its exit code under Windows PowerShell 5.1).
        $merge = & git merge-base HEAD origin/main 2>$null
        $status = $LASTEXITCODE
        if ($status -eq 0) { $base = @($merge)[0] }
    }

    if (-not $base) {
        Write-Host "no merge base with origin/main in this clone - $relative has $($now.Count) rows and nothing to compare them against."
        Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
        exit 2
    }

    $text = Show-AtBase $base $relative
    if ($null -eq $text) {
        Write-Host "$relative is not in $base - this is the commit that introduces it, and it has $($now.Count) rows."
        exit 0
    }
    $before = Read-AllowRows ($text -split "`r?`n")
    # The base's literals, read from the base's own copy of each file its list names.
    $baseProblems = [System.Collections.Generic.List[string]]::new()
    $baseLiterals = [System.Collections.Generic.List[object]]::new()
    foreach ($path in ($before | ForEach-Object { $_.Path } | Sort-Object -Unique)) {
        $blob = Show-AtBase $base $path
        if ($null -eq $blob) { continue }
        foreach ($literal in (Read-Literals $path $blob $baseProblems)) { $baseLiterals.Add($literal) }
    }
} finally {
    Pop-Location
}

if ($baseProblems.Count -gt 0) {
    foreach ($problem in $baseProblems) { Write-Host "    base ${base}:$problem" }
    throw "$($baseProblems.Count) file(s) the merge base's list names do not lex at the base (listed above); the base is main, and main must lex."
}

# Multiplicity, not membership: two rows of one identity are two rows.
$allowed = @{}
foreach ($pair in (Get-Identities $before $baseLiterals)) {
    if (-not $pair.Identity) { continue }
    if (-not $allowed.ContainsKey($pair.Identity)) { $allowed[$pair.Identity] = [System.Collections.Generic.List[object]]::new() }
    $allowed[$pair.Identity].Add($pair)
}
$unmatched = @()
foreach ($pair in $paired) {
    if ($allowed.ContainsKey($pair.Identity) -and $allowed[$pair.Identity].Count -gt 0) {
        $allowed[$pair.Identity].RemoveAt(0)
    } else {
        $unmatched += $pair
    }
}

# A pair as the move rule reads it: located by its file, the same literal when its text and reason
# are.
function Get-MoveRow($pair) {
    return [pscustomobject]@{
        Identity = "$($pair.Literal.Text)`t$($pair.Row.Reason)"
        Location = $pair.Row.Path
        Tail     = $null
        Row      = $pair
    }
}
$leftover = @(foreach ($pairs in $allowed.Values) { foreach ($pair in $pairs) { Get-MoveRow $pair } })
$added = @()
$moves = 0
foreach ($result in (Find-MovedRows -Unmatched @($unmatched | ForEach-Object { Get-MoveRow $_ }) -Leftover $leftover -Tree @() -NoItem)) {
    if ($result.Verdict -eq "moved") {
        $moves += 1
        Write-Host "moved     $($result.Row.Row.Key) ($($result.Row.Row.Reason)) from $($result.Was)"
    } else {
        $added += [pscustomobject]@{ Pair = $result.Row; Why = (Format-MoveRefusal $result) }
    }
}

$removed = $leftover.Count - $moves
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed, $moves moved."

if ($added.Count -gt 0) {
    foreach ($one in $added) {
        $pair = $one.Pair
        $why = if ($one.Why) { " - $($one.Why)" } else { "" }
        Write-Host "    added: $($pair.Row.Key) ($($pair.Row.Reason)) for the literal text: $($pair.Literal.Text)$why"
    }
    throw (
        "$($added.Count) row(s) were added to $relative, and this list only shrinks (listed above): a row is its path, " +
        "its literal's own text and its reason, so a row pointed at another literal or a listed literal whose text was " +
        "edited is a new row - repair a message as one sentence continued with ``\``, and take deliberate columns out " +
        "of string literals (cells joined at run time, or a fixture file read through a listed door)."
    )
}
