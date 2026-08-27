# no_public_document_uses_the_words_this_repository_only_says_to_itself
#
# `docs/plans/ui-style/copy-guide.md` forbids a list of words on the screen: they
# are the vocabulary this codebase uses about its own insides, and a reader has no
# use for them. `crates/bt-app/src/i18n.rs` has a test holding that line for every
# string in the window. This script holds the same line for the documents a reader
# arrives at first.
#
# **Prose only.** A path, a file name, a command or a fenced example is the
# reader's own word for a thing they will type or look for, and `vendor/…` names a
# directory that exists whatever we call it in a sentence. So fenced blocks,
# inline code spans and HTML comments are removed before the search, exactly as
# the copy guide exempts a latin token inside a path.
#
# Prove it fires before trusting it: write "the seat is a widget" into any of the
# files below, outside a code span, and this must go red.

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

$files = @(
    "README.md"
    "README.zh-CN.md"
    "CONTRIBUTING.md"
    "CHANGELOG.md"
    "docs/screenshots/README.md"
)

# The copy guide's two tables, verbatim. Latin terms are matched as whole words
# and case is ignored; the Chinese terms and the section sign have no word
# boundary to match on and are searched for as they stand.
$latin = @(
    "chrome", "seat", "seats", "the grid", "the face", "the ground", "slice",
    "mock-up", "mockup", "ruling", "red gate", "this build", "the product",
    "schema", "vendor", "pipeline", "buffer", "widget", "TODO", "FIXME",
    "P1", "P2"
)
$literal = @(
    "裁决", "红线", "本产品", "本软件", "座位", "格子", "字面", "小样", "钉子",
    "切片", "内核", "管线", "缓冲区", "控件", "§"
)

$hits = @()

foreach ($relative in $files) {
    $path = Join-Path $repo $relative
    if (-not (Test-Path $path)) { throw "$relative is missing" }

    $lines = [IO.File]::ReadAllLines($path)
    $inFence = $false
    $inComment = $false
    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]

        # A comment can run over many lines, so where it ends is a state and not
        # something one line can answer on its own.
        $prose = $line
        if ($inComment) {
            $close = $prose.IndexOf("-->")
            if ($close -lt 0) { continue }
            $prose = $prose.Substring($close + 3)
            $inComment = $false
        }

        if ($prose -match '^\s*```') {
            $inFence = -not $inFence
            continue
        }
        if ($inFence) { continue }

        # A code span, an HTML comment and a link target are not prose.
        $prose = $prose -replace '`[^`]*`', ' '
        $prose = $prose -replace '<!--.*?-->', ' '
        $open = $prose.IndexOf("<!--")
        if ($open -ge 0) {
            $prose = $prose.Substring(0, $open)
            $inComment = $true
        }
        $prose = $prose -replace '\]\([^)]*\)', '] '

        foreach ($word in $latin) {
            $escaped = [regex]::Escape($word)
            if ($prose -match "(?i)(?<![\w-])$escaped(?![\w-])") {
                $hits += "{0}:{1}: {2}  <-- '{3}'" -f $relative, ($i + 1), $line.Trim(), $word
            }
        }
        foreach ($word in $literal) {
            if ($prose.Contains($word)) {
                $hits += "{0}:{1}: {2}  <-- '{3}'" -f $relative, ($i + 1), $line.Trim(), $word
            }
        }
    }
}

if ($hits.Count -gt 0) {
    $hits | ForEach-Object { Write-Host $_ }
    throw "$($hits.Count) forbidden word(s) in the public documents - see docs/plans/ui-style/copy-guide.md"
}

Write-Host "the public documents use none of the words this repository only says to itself"
