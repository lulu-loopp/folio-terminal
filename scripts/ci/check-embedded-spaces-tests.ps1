# Scratch-repository tests for check-embedded-spaces.ps1: a string literal with a run of three or
# more spaces inside a source line, or at the start of an escaping literal's line after a real line
# break, is refused by its file:line unless the allow-list has its row; the list is exact (a stale
# row is refused, a second literal on a listed line needs its own row); it only shrinks against the
# merge base (a row is its path, its literal's text and its reason, so a row whose line moved is the
# same row and a row pointed at another literal is not); a listed literal moved to another file
# keeps its row, re-keyed, while a move that brings more rows than left, a move whose reason was
# reworded and a literal arriving in two files are refused (T-MOVED-NOT-ADDED-RULE); and a shape
# the lexer cannot finish is refused by its file:line rather than read to the end of the file.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-embedded-spaces.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-embedded-spaces-" + [guid]::NewGuid())
$relative = "docs/plans/embedded-spaces-allowed.txt"
$source = [IO.Path]::ChangeExtension('crates/bt-a/src/lib', 'rs')
$header = "# the deliberate runs`n#`n"

# Everything here that is not a run, each for its own reason: a `\` continuation's indentation,
# the indentation inside a raw literal that holds a line break, a run at the start of the literal
# and one before its closing quote, a run in a comment, a run in a doc comment, an escaped quote
# (which must not close its string), an escaped backslash before a closing quote, a quote in a
# character literal and in a byte character literal (neither opens a string), an inner quote in a
# `r#"…"#`, a lifetime, a raw identifier, and an escaped backslash followed by a continuation
# (`\\\` at the end of a line: three backslashes, so the line does continue and the indentation
# after it is dropped). Each trap comment holds a run after a quote, so a lexer that misreads the
# shape before it reads that run as a string's.
$clean = @'
//! A doc comment with a run:   here.
/// Another   one.
pub fn message<'a>(name: &'a str) -> String {
    // a comment   with a run
    /* a block comment /* nested */   with a run */
    let quote = '"';
    let r#type = 1;
    let _ = (quote, r#type);
    format!(
        "the first half of the sentence \
         and its second half: {name}"
    )
}

pub const SAMPLE: &str = r"fn main() {
    println!();
}";
pub const PADDED: &str = "   indented and trailing   ";
pub const QUOTED: &str = "an escaped quote \"";
// a lexer that ended that string at its escaped quote would read this run   as a string's: "
pub const SLASH: &str = "\\"; // a lexer that read the backslash as escaping the quote:   a " b
pub const BQUOTE: u8 = b'"'; // a lexer that read the quote as opening a string:   a " b
pub const INNER: &str = r#"a "quoted" word"#;
pub const BACKSLASH: &str = "an escaped backslash, then a continuation \\\
     and the indentation it drops";
'@

# The fixture whose columns are deliberate: line 3 of this file, as a raw string.
$fixture = @'
pub const ONE: u8 = 1;

pub const TABLE: &str = r"name   value";
'@
$fixtureRow = "${source}:3 a table fixture`n"

function Write-Tree([string]$Code, [string]$List, [switch]$Crlf) {
    $path = Join-Path $root $source
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    $text = $Code.Replace("`r`n", "`n")
    if ($Crlf) { $text = $text.Replace("`n", "`r`n") }
    [IO.File]::WriteAllText($path, $text)
    $listPath = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($listPath)) | Out-Null
    [IO.File]::WriteAllText($listPath, $List)
}

# The gate runs as a child process with a deadline, so a lexer that loops (the `'\u` case once
# did) fails its case instead of hanging the suite; on the deadline the child this function
# started, and only that one, is ended by its recorded process id.
$patience = [TimeSpan]::FromSeconds(120)
function Run-Gate([int]$Want, [string]$Case) {
    $out = Join-Path $root ".gate-out.txt"
    $err = Join-Path $root ".gate-err.txt"
    $child = Start-Process -FilePath (Get-Process -Id $PID).Path -PassThru -NoNewWindow `
        -ArgumentList @("-NoProfile", "-File", "`"$gate`"", "-Repo", "`"$root`"") `
        -RedirectStandardOutput $out -RedirectStandardError $err
    $null = $child.Handle # keeps the exit code readable after the process ends
    if (-not $child.WaitForExit([int]$patience.TotalMilliseconds)) {
        Stop-Process -Id $child.Id -Force
        throw "$Case did not finish within $patience (the gate's process $($child.Id) was ended)"
    }
    $child.WaitForExit()
    $got = $child.ExitCode
    $output = [IO.File]::ReadAllText($out) + [IO.File]::ReadAllText($err)
    Remove-Item -LiteralPath $out, $err
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

function Commit-Base([string]$Message) {
    & git add -A
    & git commit -q -m $Message
    $base = (& git rev-parse HEAD).Trim()
    & git update-ref refs/remotes/origin/main $base
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email embedded-spaces@example.invalid
        & git config user.name embedded-spaces
        & git config core.autocrlf false

        # No merge base: the comparison did not happen, and that is not a pass.
        Write-Tree $clean $header
        & git add -A
        & git commit -q -m "no base yet"
        $output = Run-Gate 2 "no merge base"
        if ($output -notmatch "not a pass") { throw "the missing base was not said: $output" }

        # The base: the clean source and the fixture beside it, with the fixture's row.
        Write-Tree ($clean + "`n" + $fixture) ""
        $fixtureLine = 0
        $lines = [IO.File]::ReadAllLines((Join-Path $root $source))
        for ($at = 0; $at -lt $lines.Count; $at++) {
            if ($lines[$at].Contains('r"name   value"')) { $fixtureLine = $at + 1 }
        }
        $row = "${source}:$fixtureLine a table fixture`n"
        Write-Tree ($clean + "`n" + $fixture) ($header + $row)
        Commit-Base "base"

        $output = Run-Gate 0 "clean, with an allow-listed fixture"
        if ($output -notmatch "1 literal\(s\) with a run in the tree, 1 row\(s\)" -or
            $output -notmatch "1 rows -> 1, 0 added, 0 removed") {
            throw "the clean tree was not read and compared: $output"
        }

        # A lost continuation: a run between two words of one source line.
        $planted = $clean + "`n" + $fixture + "`npub const LOST: &str = `"the first half          and the second`";`n"
        $plantedLine = $fixtureLine + 1
        Write-Tree $planted ($header + $row)
        $output = Run-Gate 1 "a planted run"
        if ($output -notmatch [regex]::Escape("${source}:${plantedLine}: a string literal carries a run")) {
            throw "the planted run was not refused by its file:line: $output"
        }

        # A lost continuation spelled as an escaped line break, on a later line of a literal: the
        # row names the line the literal opens on and the line of the run.
        $escaped = $clean + "`n" + $fixture + "`npub const ESCAPED: &str = `"the first half \`n         and \`n          the second \n          half`";`n"
        $opens = $fixtureLine + 1
        Write-Tree $escaped ($header + $row)
        $output = Run-Gate 1 "an escaped line break before a run"
        if ($output -notmatch [regex]::Escape("${source}:${opens}: a string literal carries a run of three or more spaces (the run is on line $($opens + 2))")) {
            throw "the escaped line break was not refused by its file:line: $output"
        }

        # A run before a `\` continuation is inside the sentence too, and so is one in a byte string.
        foreach ($case in @(
                @{ Name = "a run before a continuation"; Text = "pub const C: &str = `"one   \`n    two`";" },
                @{ Name = "a run in a byte string"; Text = "pub const B: &[u8] = b`"one   two`";" })) {
            Write-Tree ($clean + "`n" + $fixture + "`n" + $case.Text + "`n") ($header + $row)
            $output = Run-Gate 1 $case.Name
            if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): a string literal carries a run")) {
                throw "$($case.Name) was not refused by its file:line: $output"
            }
        }

        # A row removed while its literal is still there: the list only shrinks by repairing.
        Write-Tree ($clean + "`n" + $fixture) $header
        $output = Run-Gate 1 "a needed row removed"
        if ($output -notmatch [regex]::Escape("${source}:${fixtureLine}: a string literal carries a run")) {
            throw "the removed row was not refused by its literal's file:line: $output"
        }

        # The literal moved down by an edit above it: the row's line follows it, and that is no
        # added row.
        Write-Tree ("// a line added above`n" + $clean + "`n" + $fixture) ($header + "${source}:$($fixtureLine + 1) a table fixture`n")
        $output = Run-Gate 0 "a moved literal"
        if ($output -notmatch "1 rows -> 1, 0 added, 0 removed") { throw "the moved row was counted as added: $output" }

        # The literal moved to another file with its item: the row is re-keyed to the new file and
        # line, and it is that row moved (T-MOVED-NOT-ADDED-RULE).
        $other = [IO.Path]::ChangeExtension('crates/bt-a/src/table', 'rs')
        $otherPath = Join-Path $root $other
        [IO.File]::WriteAllText($otherPath, $fixture.Replace("`r`n", "`n"))
        & git add -- $other
        Write-Tree $clean ($header + "${other}:3 a table fixture`n")
        $output = Run-Gate 0 "a literal moved to another file"
        if ($output -notmatch "1 rows -> 1, 0 added, 0 removed, 1 moved") { throw "the literal moved to another file was not counted as moved: $output" }
        # More rows arrived than left: the second is added.
        [IO.File]::WriteAllText($otherPath, ($fixture + "`npub const AGAIN: &str = r`"name   value`";`n").Replace("`r`n", "`n"))
        Write-Tree $clean ($header + "${other}:3 a table fixture`n" + "${other}:4 a table fixture`n")
        $output = Run-Gate 1 "a move that brings more rows than left"
        if ($output -notmatch "with more rows than it had there") { throw "the grown move was refused for the wrong reason: $output" }
        # A move whose reason was reworded is a new row.
        [IO.File]::WriteAllText($otherPath, $fixture.Replace("`r`n", "`n"))
        Write-Tree $clean ($header + "${other}:3 a reworded table fixture`n")
        $output = Run-Gate 1 "a moved literal whose reason was reworded"
        if ($output -notmatch "only shrinks" -or $output -notmatch "1 added") { throw "the reworded move was refused for the wrong reason: $output" }
        # One literal arriving in two files cannot be paired.
        $third = [IO.Path]::ChangeExtension('crates/bt-a/src/columns', 'rs')
        [IO.File]::WriteAllText((Join-Path $root $third), $fixture.Replace("`r`n", "`n"))
        & git add -- $third
        Write-Tree $clean ($header + "${other}:3 a table fixture`n" + "${third}:3 a table fixture`n")
        $output = Run-Gate 1 "one literal arriving in two files"
        if ($output -notmatch "and arrived at more than one place") { throw "the ambiguous move was refused for the wrong reason: $output" }
        & git rm -q -f -- $other $third

        # A row whose line opens no literal with a run is stale.
        Write-Tree ($clean + "`n" + $fixture) ($header + $row + "${source}:1 a row for nothing`n")
        $output = Run-Gate 1 "a stale row"
        if ($output -notmatch [regex]::Escape("${source}:1 is stale")) {
            throw "the stale row was refused for the wrong reason: $output"
        }

        # A literal with a run listed by a row the merge base does not have: the list only shrinks.
        $second = $clean + "`n" + $fixture + "`npub const OTHER: &str = r`"a   b`";`n"
        Write-Tree $second ($header + $row + "${source}:$($fixtureLine + 1) another table fixture`n")
        $output = Run-Gate 1 "an added row"
        if ($output -notmatch "only shrinks" -or $output -notmatch "another table fixture") {
            throw "the added row was refused for the wrong reason: $output"
        }

        # The same row with its reason rewritten is an added row as well.
        Write-Tree ($clean + "`n" + $fixture) ($header + "${source}:$fixtureLine a reworded reason`n")
        $output = Run-Gate 1 "a reworded row"
        if ($output -notmatch "only shrinks") { throw "the reworded row was refused for the wrong reason: $output" }

        # The repair shrinks the list.
        Write-Tree ($clean + "`n" + $fixture.Replace('r"name   value"', 'r"name value"')) $header
        $output = Run-Gate 0 "shrunk"
        if ($output -notmatch "1 rows -> 0, 0 added, 1 removed") { throw "the removal was not counted: $output" }

        # A second literal with a run on a line that is already listed needs a row of its own, and
        # a second row for a line that opens one literal is stale.
        $shared = $clean + "`n" + $fixture.Replace('r"name   value";', '[r"name   value", r"one   two"];').Replace('TABLE: &str', 'TABLE: [&str; 2]')
        Write-Tree $shared ($header + $row)
        $output = Run-Gate 1 "a second literal on a listed line"
        if ($output -notmatch [regex]::Escape("${source}:${fixtureLine}: a string literal carries a run")) {
            throw "the second literal on a listed line was not refused by its file:line: $output"
        }
        Write-Tree ($clean + "`n" + $fixture) ($header + $row + $row)
        $output = Run-Gate 1 "a second row for a line with one literal"
        if ($output -notmatch [regex]::Escape("${source}:${fixtureLine} is stale")) {
            throw "the extra row was refused for the wrong reason: $output"
        }

        # A row pointed at another literal of the same file, with the same reason, is a new row:
        # the fixture it listed was repaired, and its row now names a literal the base never listed.
        $repointed = $clean + "`n" + $fixture.Replace('r"name   value"', 'r"name value"') + "`npub const OTHER: &str = r`"key   word`";`n"
        Write-Tree $repointed ($header + "${source}:$($fixtureLine + 1) a table fixture`n")
        $output = Run-Gate 1 "a row repointed at another literal"
        if ($output -notmatch "only shrinks" -or $output -notmatch [regex]::Escape('key   word')) {
            throw "the repointed row was not refused as an added row: $output"
        }

        # A lost `\` at the end of a wrapped line: the next line's indentation follows a real line
        # break inside an escaping literal; an escaped backslash before the break is no
        # continuation. And the other string forms: a C string, a raw byte string, and a raw string
        # whose inner `"#` does not close it (it opened with two hashes).
        foreach ($case in @(
                @{ Name = "a lost continuation before a real line break"; Text = "pub const WRAPPED: &str = `"the first half of the sentence`n     and the second half`";" },
                @{ Name = "an escaped backslash before a real line break"; Text = 'pub const SLASHES: &str = "a path that ends in \\' + "`n" + '     and this line was meant to continue";' },
                @{ Name = "a run in a C string"; Text = "pub const C: &core::ffi::CStr = c`"one   two`";" },
                @{ Name = "a run in a raw byte string"; Text = "pub const BR: &[u8] = br`"one   two`";" },
                @{ Name = "a run in a raw string whose inner quote and hash do not close it"; Text = 'pub const HASHES: &str = r##"a "#   " b"##;' })) {
            Write-Tree ($clean + "`n" + $fixture + "`n" + $case.Text + "`n") ($header + $row)
            $output = Run-Gate 1 $case.Name
            if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): a string literal carries a run")) {
                throw "$($case.Name) was not refused by its file:line: $output"
            }
        }

        # CRLF line endings read as LF: the clean tree passes, a planted run is named by its line.
        Write-Tree ($clean + "`n" + $fixture) ($header + $row) -Crlf
        $output = Run-Gate 0 "the clean tree with CRLF line endings"
        if ($output -notmatch "1 rows -> 1, 0 added, 0 removed") { throw "the CRLF tree was not read as the LF one: $output" }
        Write-Tree ($clean + "`n" + $fixture + "`npub const LOST: &str = `"the first half          and the second`";`n") ($header + $row) -Crlf
        $output = Run-Gate 1 "a planted run with CRLF line endings"
        if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): a string literal carries a run")) {
            throw "the CRLF planted run was not refused by its file:line: $output"
        }

        # A shape the lexer cannot finish is refused by its file:line, and the run ends.
        foreach ($case in @(
                @{ Name = "a string that does not close"; Text = 'pub const OPEN: &str = "never closed;'; Said = "a string that does not close" },
                @{ Name = "a raw string that does not close"; Text = 'pub const RAW: &str = r#"never closed";'; Said = "a raw string that does not close" },
                @{ Name = "a block comment that does not close"; Text = "/* never closed"; Said = "a block comment that does not close" },
                @{ Name = "a character escape that does not close"; Text = "pub const U: char = '\u{41';"; Said = "a character literal that does not close" })) {
            Write-Tree ($clean + "`n" + $fixture + "`n" + $case.Text + "`n") ($header + $row)
            $output = Run-Gate 1 $case.Name
            if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): $($case.Said)")) {
                throw "$($case.Name) was not refused by its file:line: $output"
            }
        }

        # A rename of the code beside a listed literal, on its own line, keeps the row: the identity
        # is the literal's own text, not the line around it.
        Write-Tree ($clean + "`n" + $fixture.Replace('pub const TABLE: &str', 'pub(crate) const RENAMED_TABLE: &str')) ($header + $row)
        $output = Run-Gate 0 "a rename beside a listed literal"
        if ($output -notmatch "1 rows -> 1, 0 added, 0 removed") { throw "the rename made the row look added: $output" }

        # A tab is the character before a run: two spaces, a tab, then three spaces hold a run.
        Write-Tree ($clean + "`n" + $fixture + "`npub const TAB: &str = `"a  `t   b`";`n") ($header + $row)
        $output = Run-Gate 1 "a run after a tab that follows a short run"
        if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): a string literal carries a run")) {
            throw "the run after the tab was not refused by its file:line: $output"
        }

        # A `'\u{` whose brace closes only on the next line is refused by its line, mid-file.
        Write-Tree ($clean + "`n" + $fixture + "`npub const U2: char = '\u{41`n}';`npub const AFTER: u8 = 2;`n") ($header + $row)
        $output = Run-Gate 1 "a character escape whose brace is on the next line"
        if ($output -notmatch [regex]::Escape("${source}:$($fixtureLine + 1): a character literal that does not close")) {
            throw "the split escape was not refused by its file:line: $output"
        }

        # A literal whose listed text is not ASCII is read at the base through `git show` and
        # matched: the same row is no added row.
        $wide = $clean + "`n" + $fixture + "`npub const NAMES: &str = r`"名字   值`";`n"
        $wideRows = $header + $row + "${source}:$($fixtureLine + 1) a table fixture with Chinese cells`n"
        Write-Tree $wide $wideRows
        Commit-Base "a base whose list names a Chinese literal"
        $output = Run-Gate 0 "a listed literal with Chinese text"
        if ($output -notmatch "2 rows -> 2, 0 added, 0 removed") { throw "the Chinese literal's row was not matched at the base: $output" }

        # A base whose listed file does not lex is a failure, said as the base's own file:line.
        $broken = $clean + "`n" + $fixture + "`n/* never closed`n"
        Write-Tree $broken ($header + $row)
        Commit-Base "a base that does not lex"
        $brokenBase = (& git rev-parse HEAD).Trim()
        Write-Tree ($clean + "`n" + $fixture) ($header + $row)
        $output = Run-Gate 1 "a base that does not lex"
        if ($output -notmatch [regex]::Escape("base ${brokenBase}:${source}:$($fixtureLine + 1): a block comment that does not close")) {
            throw "the base's lexer fault was not reported: $output"
        }

        # A malformed row is refused rather than read as something else.
        Write-Tree ($clean + "`n" + $fixture) ($header + "${source} a table fixture`n")
        $output = Run-Gate 1 "a malformed row"
        if ($output -notmatch "not ``path:line reason``") { throw "the malformed row was refused for the wrong reason: $output" }
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-embedded-spaces: no-base, clean, planted, escaped break, continuation, byte string, removed row, moved row, moved to another file, moved and grown, moved and reworded, moved to two files, stale row, added row, reworded row, shrink, shared line, extra row, repointed row, lost continuation at a line break, escaped backslash, C string, raw byte string, hash count, CRLF, four unfinished shapes, rename beside a literal, tab boundary, split escape, Chinese identity, base fault and malformed-row cases pass"
exit 0
