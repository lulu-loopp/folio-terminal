# the_embedded_spaces_list_only_shrinks
#
# **A message literal is one sentence.** A string literal that wraps onto a
# second source line continues with `\` at the end of the line, and Rust drops
# the line break and the indentation after it. When the `\` is lost - an edit
# that joins two lines, a literal re-wrapped by hand - the indentation lands
# inside the string, and the message reads "within the lane" and a dozen spaces
# and "suite's patience". The compiler accepts it, rustfmt leaves it, and a
# test's assertion text carries it unseen. This gate refuses the class: every
# string literal of first-party Rust with a run of three or more spaces inside
# a source line (defined below) is either repaired or on
# `docs/plans/embedded-spaces-allowed.txt`, the list of literals whose columns
# are deliberate (fixture tables, aligned samples), and that list only shrinks.
# The rule is `docs/CONVENTIONS.md` §五's sentence on message literals.
#
# WHAT IS READ. Every `.rs` file `git ls-files` names under `crates/` - so no
# `vendor/` (not under `crates/`) and no `target/` (not tracked). The file is
# lexed, not parsed: comments (line, block, nested), character literals and
# lifetimes are skipped as what they are, and every string literal - `"…"`,
# `b"…"`, `c"…"`, and the raw forms `r"…"`, `r#"…"#`, `br…`, `cr…` - is read as
# its source text between the quotes.
#
# WHAT IS A RUN. Three or more consecutive spaces in a literal's source text
# with a character other than a space or a tab before them and after them on
# the same source line: a run between two words, before a `\` continuation,
# after an escaped `\n` that was meant to be a continuation. What is not: the
# blanks at the start of a source line of the literal (after a `\`
# continuation Rust drops them; in a literal that holds a line break they are
# the text's own indentation; right after the opening quote they are padding),
# and the blanks that end one (before the closing quote, or before a line
# break the literal holds) - a lost continuation always has words on both
# sides.
#
# THE LIST. One row per literal, `path:line reason`: the path from the
# repository root with `/`, the line the literal opens on (two literals that
# open on one line share its row), and why its columns are deliberate. `#`
# lines and blank lines are commentary. In the tree the list is exact both
# ways: a literal with a run that has no row is refused by its file:line (and
# the line of the run, when that is a later one), and a row whose line opens
# no such literal is refused as stale - the literal was repaired or moved, and
# the row is removed or its line corrected in the same commit.
#
# THE COMPARISON is the dead-code gate's (`check-dead-code.ps1`): against the
# pull request's merge base with `origin/main`, with multiplicity. A row is
# identified by its path and its reason; the line is where the literal opens
# today, so a row whose only change is its line is the same literal moved (an
# edit above it), not an added row. A row the merge base does not have is
# refused: a literal is repaired, never listed - except by the commit that
# introduces the list. It passes, loudly, when the base has no list - that is
# how the commit that introduces the list passes. It refuses (exit 2) when
# there is no base at all: no `origin/main` in the clone, or no merge base,
# means the comparison did not happen. CI runs it in jobs that check out with
# `fetch-depth: 0`.
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
        public string Text;   // that source line, trimmed
    }

    public static class Scanner
    {
        static bool IdentStart(char c) { return c == '_' || char.IsLetter(c); }
        static bool IdentPart(char c) { return c == '_' || char.IsLetterOrDigit(c); }

        // Every string literal of `s` that holds a run, one entry per literal.
        public static List<Run> Runs(string s)
        {
            var runs = new List<Run>();
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
                    int depth = 1; i += 2;
                    while (i < n && depth > 0)
                    {
                        if (s[i] == '\n') { line++; i++; }
                        else if (s[i] == '/' && i + 1 < n && s[i + 1] == '*') { depth++; i += 2; }
                        else if (s[i] == '*' && i + 1 < n && s[i + 1] == '/') { depth--; i += 2; }
                        else i++;
                    }
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
                        int to = Math.Min(i, n);
                        Judge(s, from, to, line, runs);
                        line += Count(s, from, to);
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
                        i = Quoted(s, i, ref line, runs);
                        continue;
                    }
                    if (id == "b" && i < n && s[i] == '\'')
                    {
                        i = SkipChar(s, i);
                        continue;
                    }
                    continue;
                }
                if (c == '"')
                {
                    i = Quoted(s, i, ref line, runs);
                    continue;
                }
                if (c == '\'')
                {
                    // A character literal, or a lifetime / label: `'a'` against `'a`.
                    if (i + 1 < n && s[i + 1] == '\\') { i = SkipChar(s, i); continue; }
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
            return runs;
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

        // An escaping string from its opening quote; returns the index after the closing one.
        static int Quoted(string s, int i, ref int line, List<Run> runs)
        {
            int n = s.Length;
            int from = ++i;
            while (i < n && s[i] != '"')
            {
                if (s[i] == '\\' && i + 1 < n) { i += 2; continue; }
                i++;
            }
            Judge(s, from, i, line, runs);
            line += Count(s, from, i);
            return i + 1;
        }

        static bool Blank(char c) { return c == ' ' || c == '\t'; }

        // The literal's source text is s[from..to), opening on `line`, and each of its source
        // lines is s[k..end). Its first run, if any, is recorded: three spaces or more with a
        // character that is not a space or a tab before it and after it on the same source line.
        static void Judge(string s, int from, int to, int line, List<Run> runs)
        {
            int at = line;
            int k = from;
            while (true)
            {
                int end = s.IndexOf('\n', k, to - k);
                if (end < 0) end = to;
                int p = k;
                while (p < end && Blank(s[p])) p++;
                while (p < end)
                {
                    if (s[p] != ' ') { p++; continue; }
                    int run = p;
                    while (p < end && s[p] == ' ') p++;
                    int width = p - run;
                    while (p < end && Blank(s[p])) p++;
                    if (width >= 3 && p < end)
                    {
                        int head = s.LastIndexOf('\n', run) + 1;
                        int tail = s.IndexOf('\n', run);
                        if (tail < 0) tail = s.Length;
                        runs.Add(new Run { Opens = line, Line = at, Text = s.Substring(head, tail - head).Trim() });
                        return;
                    }
                }
                if (end == to) return;
                at++;
                k = end + 1;
            }
        }

        // A character literal from its opening quote (or the `b` before it has been read).
        static int SkipChar(string s, int i)
        {
            i++;
            if (s[i] == '\\')
            {
                i++;
                if (s[i] == 'u') i = s.IndexOf('}', i) + 1;
                else if (s[i] == 'x') i += 3;
                else i++;
            }
            else
            {
                i += char.IsHighSurrogate(s[i]) ? 2 : 1;
            }
            return i + 1;
        }
    }
}
'@

# The tree: one `path:line` key per literal with a run.
$found = [ordered]@{}
Push-Location $repo
try {
    $files = @(& git ls-files -- "crates/*.rs")
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed in $repo" }
} finally {
    Pop-Location
}
foreach ($file in ($files | Sort-Object)) {
    $text = [IO.File]::ReadAllText((Join-Path $repo $file)).Replace("`r`n", "`n")
    foreach ($run in [FolioEmbeddedSpacesGate.Scanner]::Runs($text)) {
        $found["${file}:$($run.Opens)"] = $run
    }
}

if ($Rows) {
    foreach ($key in $found.Keys) {
        $run = $found[$key]
        Write-Output "$key`trun on line $($run.Line)`t$($run.Text)"
    }
    exit 0
}

# A list row: `path:line reason`.
function Read-AllowRows([string[]]$lines) {
    $rows = @()
    foreach ($line in $lines) {
        if ($line.Trim().Length -eq 0 -or $line.StartsWith("#")) { continue }
        if ($line -notmatch '^(?<path>[^\s:]+):(?<line>[1-9][0-9]*) (?<reason>\S.*)$') {
            throw "$relative has a row that is not ``path:line reason``: $line"
        }
        $rows += [pscustomobject]@{
            Key      = "$($Matches.path):$($Matches.line)"
            Identity = "$($Matches.path) $($Matches.reason)"
        }
    }
    return , $rows
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - it is the list of literals whose runs of spaces are deliberate"
}
$now = Read-AllowRows ([IO.File]::ReadAllLines($list))

$problems = @()
$listed = @{}
foreach ($row in $now) {
    if ($listed.ContainsKey($row.Key)) {
        $problems += "${relative}: $($row.Key) is listed twice"
        continue
    }
    $listed[$row.Key] = $true
    if (-not $found.Contains($row.Key)) {
        $problems += "${relative}: $($row.Key) opens no string literal with a run of spaces - the row is stale; remove it, or correct its line"
    }
}
foreach ($key in $found.Keys) {
    if (-not $listed.ContainsKey($key)) {
        $run = $found[$key]
        $at = if ($run.Line -ne $run.Opens) { " (the run is on line $($run.Line))" } else { "" }
        $problems += "${key}: a string literal carries a run of three or more spaces${at}: $($run.Text)"
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

    $text = (& git show "${base}:${relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$relative is not in $base - this is the commit that introduces it, and it has $($now.Count) rows."
        exit 0
    }
    $before = Read-AllowRows ($text -split "`r?`n")
} finally {
    Pop-Location
}

# Multiplicity, not membership: two rows of one path and reason are two rows.
$allowed = @{}
foreach ($row in $before) {
    if ($allowed.ContainsKey($row.Identity)) { $allowed[$row.Identity] += 1 } else { $allowed[$row.Identity] = 1 }
}
$added = @()
foreach ($row in $now) {
    if ($allowed.ContainsKey($row.Identity) -and $allowed[$row.Identity] -gt 0) {
        $allowed[$row.Identity] -= 1
    } else {
        $added += $row
    }
}

$removed = $before.Count - ($now.Count - $added.Count)
Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added, $removed removed."

if ($added.Count -gt 0) {
    foreach ($row in $added) { Write-Host "    added: $($row.Key) ($($row.Identity))" }
    throw (
        "$($added.Count) row(s) were added to $relative, and this list only shrinks (listed above). " +
        "Repair the literal instead: a message is one sentence, continued with ``\`` at the end of a line."
    )
}
