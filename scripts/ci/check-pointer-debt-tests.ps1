# Scratch-repository tests for check-pointer-debt.ps1 (T-MOVED-NOT-ADDED-RULE): the list before
# its `module` column matches whole and pairs no move; then the unchanged list passes, a reader
# moved to another module passes, and so does one moved that reads less; an added row, a moved
# reader that reads more, a move whose old module still holds the owner and an ambiguous move are
# each refused; and an import row of the module a reader moved into is accepted up to the number of
# owners moved in with its door (ruling Q1 (b)), and refused beyond it or with no reader moved.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-pointer-debt.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-pointer-debt-gate-" + [guid]::NewGuid())
$relative = "docs/plans/POINTER-DEBT.tsv"
$header = "# a comment the gate does not compare`nowner`tdoor`tmodule`n"
$scroll = "crate::Runtime::scroll_tab_strip`tN-B`tcrate`n"
$scrollE = "crate::Runtime::scroll_tab_strip`tN-E`tcrate`n"
$wheel = "crate::upright_wheel`tN-B`tcrate`n"
$import = "crate (outside any item)`tN-B`tcrate`n"
$tabs = "crate::runtime::tabs"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Commit-Base([string]$Text, [string]$Message) {
    Write-List $Text
    & git add .
    & git commit -q -m $Message
    & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

function Passes([string]$Text, [string]$Case, [string]$Said) {
    Write-List $Text
    $output = Run-Gate 0 $Case
    if ($output -notmatch [regex]::Escape($Said)) { throw "$Case passed without saying '$Said': $output" }
}

function Refused([string]$Text, [string]$Case, [string]$Reason) {
    Write-List $Text
    $output = Run-Gate 1 $Case
    $plain = ($output -replace '\x1b\[[0-9;]*m', '') -replace '[\s|]', ''
    if ($plain -notmatch [regex]::Escape(($Reason -replace '\s', ''))) {
        throw "$Case was refused for the wrong reason (wanted '$Reason'): $output"
    }
}

# A row of `$Row` moved into the runtime's tabs module.
function Moved([string]$Row) {
    return $Row.Replace("crate::Runtime::", "${tabs}::Runtime::").Replace("`tcrate`n", "`t${tabs}`n")
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email pointer-debt-gate@example.invalid
        & git config user.name pointer-debt-gate
        & git config core.autocrlf false

        # The list before the column: its rows match whole, and none of them is paired as moved.
        $two = { param([string]$Text) $Text.Replace("`tcrate`n", "`n").Replace("`tmodule`n", "`n") }
        Commit-Base (& $two ($header + $scroll + $scroll + $wheel)) "the list before its module column"
        Passes ($header + $scroll + $scroll + $wheel) "the column added" "3 rows -> 3, 0 added, 0 removed, 0 moved"
        Refused ($header + (Moved $scroll) + (Moved $scroll) + $wheel) "a move against a base without the column" `
            "2 row(s) were added"

        Commit-Base ($header + $scroll + $scroll + $scrollE + $wheel + $import) "the base"
        $all = $header + $scroll + $scroll + $scrollE + $wheel + $import
        Passes $all "unchanged" "5 rows -> 5, 0 added, 0 removed, 0 moved"

        $movedAll = $header + (Moved $scroll) + (Moved $scroll) + (Moved $scrollE) + $wheel + $import
        Passes $movedAll "a reader moved" "5 rows -> 5, 0 added, 0 removed, 3 moved"
        Passes ($header + (Moved $scroll) + (Moved $scrollE) + $wheel + $import) "a moved reader that reads less" `
            "5 rows -> 4, 0 added, 1 removed, 2 moved"
        Refused ($movedAll + (Moved $scroll)) "a moved reader that reads more" `
            "crate::runtime::tabs::Runtime::scroll_tab_strip N-B: it moved from crate with more rows than it had there"
        Refused ($all + "crate::Runtime::press_tab`tN-B`tcrate`n") "an added row" "1 row(s) were added"
        Refused ($header + $scroll.Replace("scroll_tab_strip", "scroll_strip") + $scroll.Replace("scroll_tab_strip", "scroll_strip") + $scrollE.Replace("scroll_tab_strip", "scroll_strip") + $wheel + $import) `
            "an owner renamed in place (no rename arm here)" "3 row(s) were added"
        Refused ($header + (Moved $scroll) + (Moved $scroll) + $scrollE + $wheel + $import) `
            "a move whose old module still holds the owner" "but crate still holds Runtime::scroll_tab_strip"
        Refused ($header + (Moved $scroll) + (Moved $scroll).Replace("tabs", "panes") + (Moved $scrollE) + $wheel + $import) `
            "one owner arriving at two modules" "and arrived at more than one place"

        # Imports of a moved reader (ruling Q1 (b)).
        $tabsImport = "${tabs} (outside any item)`tN-B`t${tabs}`n"
        Passes ($movedAll + $tabsImport) "an import row for a reader moved in" "1 import(s) of moved readers"
        Refused ($movedAll + $tabsImport + $tabsImport) "two import rows for one owner moved in" `
            "more import rows for crate::runtime::tabs and N-B than the 1 owner(s) moved into it with that door"
        Refused ($all + $tabsImport) "an import row with no reader moved" "1 row(s) were added"
        Refused ($movedAll + $tabsImport.Replace("`tN-B`t", "`tN-W`t")) "an import row of another door" `
            "1 row(s) were added"

        # A row whose owner is not written in its module is malformed, not read as something else.
        Refused ($all + "crate::Runtime::x`tN-B`tcrate::other`n") "a row outside its module" `
            "a row whose owner is not written in its module"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-pointer-debt: the column's transition, unchanged, moved, moved-and-read-less, moved-and-read-more, added, still-at-its-module, ambiguous, import, two imports, unowed import, other-door import and malformed-row cases pass"
exit 0
