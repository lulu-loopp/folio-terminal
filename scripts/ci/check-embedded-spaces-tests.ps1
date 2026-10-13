# Scratch-repository tests for check-embedded-spaces.ps1: a string literal with a run of three or
# more spaces between two characters of one source line is refused by its file:line unless the
# allow-list has its row, the list is exact (a stale row is refused), and it only shrinks against
# the merge base (a row is its path and its reason, so a row whose line moved is the same row).

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-embedded-spaces.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-embedded-spaces-" + [guid]::NewGuid())
$relative = "docs/plans/embedded-spaces-allowed.txt"
$source = "crates/bt-a/src/lib.rs"
$header = "# the deliberate runs`n#`n"

# Everything here that is not a run, each for its own reason: a `\` continuation's indentation,
# the indentation inside a literal that holds a line break, a run at the start of the literal and
# one before its closing quote, a run in a comment, a run in a doc comment, an escaped quote (which
# must not close its string), a quote in a character literal (which must not open a string), a
# lifetime, and a raw identifier.
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

pub const SAMPLE: &str = "fn main() {
    println!();
}";
pub const PADDED: &str = "   indented and trailing   ";
pub const QUOTED: &str = "an escaped quote \"";
// a lexer that ended that string at its escaped quote would read this run   as a string's: "
'@

# The fixture whose columns are deliberate: line 3 of this file, as a raw string.
$fixture = @'
pub const ONE: u8 = 1;

pub const TABLE: &str = r"name   value";
'@
$fixtureRow = "${source}:3 a table fixture`n"

function Write-Tree([string]$Code, [string]$List) {
    $path = Join-Path $root $source
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Code.Replace("`r`n", "`n"))
    $listPath = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($listPath)) | Out-Null
    [IO.File]::WriteAllText($listPath, $List)
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root 2>&1 | Out-String
    $got = $LASTEXITCODE
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

        # A row whose line opens no literal with a run is stale.
        Write-Tree ($clean + "`n" + $fixture) ($header + $row + "${source}:1 a row for nothing`n")
        $output = Run-Gate 1 "a stale row"
        if ($output -notmatch [regex]::Escape("${source}:1 opens no string literal with a run of spaces")) {
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

Write-Host "check-embedded-spaces: no-base, clean, planted, escaped break, continuation, byte string, removed row, moved row, stale row, added row, reworded row, shrink and malformed-row cases pass"
exit 0
