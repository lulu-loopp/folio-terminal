# Scratch-repository tests for check-pointer-debt.ps1 (T-MOVED-NOT-ADDED-RULE): the list before
# its `module` column matches whole and pairs no move; the list before its `needle` column
# matches row for row and pairs moves; then the unchanged list passes, a reader moved to another
# module passes, and so does one moved that reads less; an added row, a moved reader that reads
# more, a move whose old module still lists the owner and an ambiguous move are each refused; and
# an import row of the module a reader moved into is accepted once per needle the owners moved in
# with its door read (ruled in K2 batch 23, 2026-10-27), and refused for a second row of a needle,
# for a needle no moved owner reads, without a needle column, or with no reader moved.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-pointer-debt.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-pointer-debt-gate-" + [guid]::NewGuid())
$relative = "docs/plans/POINTER-DEBT.tsv"
$header = "# a comment the gate does not compare`nowner`tdoor`tmodule`tneedle`n"
$scroll = "crate::Runtime::scroll_tab_strip`tN-B`tcrate`tMouseButton`n"
$scrollE = "crate::Runtime::scroll_tab_strip`tN-E`tcrate`tPhysicalPosition`n"
$wheel = "crate::upright_wheel`tN-B`tcrate`tMouseScrollDelta`n"
$import = "crate (outside any item)`tN-B`tcrate`tMouseButton`n"
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
    return $Row.Replace("crate::Runtime::", "${tabs}::Runtime::").Replace("`tcrate`t", "`t${tabs}`t")
}

# The list as it was written before a column: every data line and the column header cut back to
# its first `$Keep` columns.
function Before-Column([string]$Text, [int]$Keep) {
    $lines = foreach ($line in ($Text -split "`n")) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { $line } else { ($line.Split("`t")[0..($Keep - 1)]) -join "`t" }
    }
    return $lines -join "`n"
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email pointer-debt-gate@example.invalid
        & git config user.name pointer-debt-gate
        & git config core.autocrlf false

        # The list before the module column: its rows match whole, and none of them is paired as moved.
        Commit-Base (Before-Column ($header + $scroll + $scroll + $wheel) 2) "the list before its module column"
        Passes ($header + $scroll + $scroll + $wheel) "the columns added" "3 rows -> 3, 0 added, 0 removed, 0 moved"
        Refused ($header + (Moved $scroll) + (Moved $scroll) + $wheel) "a move against a base without the column" `
            "2 row(s) were added"

        # The list before the needle column: its rows match row for row, and a move pairs.
        Commit-Base (Before-Column ($header + $scroll + $scroll + $scrollE + $wheel + $import) 3) "the list before its needle column"
        Passes ($header + $scroll + $scroll + $scrollE + $wheel + $import) "the needle column added" `
            "5 rows -> 5, 0 added, 0 removed, 0 moved"
        Passes ($header + (Moved $scroll) + (Moved $scroll) + (Moved $scrollE) + $wheel + $import) `
            "a move against a base without the needle column" "5 rows -> 5, 0 added, 0 removed, 3 moved"

        Commit-Base ($header + $scroll + $scroll + $scrollE + $wheel + $import) "the base"
        $all = $header + $scroll + $scroll + $scrollE + $wheel + $import
        Passes $all "unchanged" "5 rows -> 5, 0 added, 0 removed, 0 moved"

        $movedAll = $header + (Moved $scroll) + (Moved $scroll) + (Moved $scrollE) + $wheel + $import
        Passes $movedAll "a reader moved" "5 rows -> 5, 0 added, 0 removed, 3 moved"
        Passes ($header + (Moved $scroll) + (Moved $scrollE) + $wheel + $import) "a moved reader that reads less" `
            "5 rows -> 4, 0 added, 1 removed, 2 moved"
        Refused ($movedAll + (Moved $scroll)) "a moved reader that reads more" `
            "crate::runtime::tabs::Runtime::scroll_tab_strip N-B: it moved from crate with more rows than it had there"
        Refused ($all + "crate::Runtime::press_tab`tN-B`tcrate`tMouseButton`n") "an added row" "1 row(s) were added"
        Refused ($header + $scroll.Replace("scroll_tab_strip", "scroll_strip") + $scroll.Replace("scroll_tab_strip", "scroll_strip") + $scrollE.Replace("scroll_tab_strip", "scroll_strip") + $wheel + $import) `
            "an owner renamed in place (no rename arm here)" "3 row(s) were added"
        Refused ($header + (Moved $scroll) + (Moved $scroll) + $scrollE + $wheel + $import) `
            "a move whose old module still lists the owner" "but crate still lists Runtime::scroll_tab_strip"
        Refused ($header + (Moved $scroll) + (Moved $scroll).Replace("tabs", "panes") + (Moved $scrollE) + $wheel + $import) `
            "one owner arriving at two modules" "and arrived at more than one place"

        # Imports of a moved reader, one per needle a moved owner reads (ruled in K2 batch 23).
        $tabsImport = "${tabs} (outside any item)`tN-B`t${tabs}`tMouseButton`n"
        Passes ($movedAll + $tabsImport) "an import row for a reader moved in" "1 import(s) of moved readers"
        Refused ($movedAll + $tabsImport + $tabsImport) "two import rows of one needle" `
            "a second import row of MouseButton for crate::runtime::tabs and N-B: one import row per needle a moved owner reads"
        Refused ($all + $tabsImport) "an import row with no reader moved" "1 row(s) were added"
        Refused ($movedAll + $tabsImport.Replace("`tN-B`t", "`tN-W`t")) "an import row of another door" `
            "1 row(s) were added"
        Refused ($movedAll + (Before-Column $tabsImport 3)) "an import row without its needle" `
            "an import row names its needle (the list's needle column), and this one names none"

        # Batch 23's shape: two owners moved in that read three button and wheel types between them,
        # and the module's three imports, one per type: accepted. One of the three imports naming a
        # needle neither owner reads stays refused, by name.
        $settings = "crate::runtime::settings"
        $mouse = "crate::Runtime::settings_mouse_input"
        $wheelOwner = "crate::Runtime::scroll_settings"
        Commit-Base ($header + "$mouse`tN-B`tcrate`tElementState`n" + "$mouse`tN-B`tcrate`tMouseButton`n" +
            "$wheelOwner`tN-B`tcrate`tMouseScrollDelta`n" + $import) "a base with batch 23's two owners"
        $two = $header + "${settings}::Runtime::settings_mouse_input`tN-B`t${settings}`tElementState`n" +
            "${settings}::Runtime::settings_mouse_input`tN-B`t${settings}`tMouseButton`n" +
            "${settings}::Runtime::scroll_settings`tN-B`t${settings}`tMouseScrollDelta`n" + $import
        $settingsImport = { param([string]$Needle) "${settings} (outside any item)`tN-B`t${settings}`t$Needle`n" }
        Passes ($two + (& $settingsImport "ElementState") + (& $settingsImport "MouseButton") + (& $settingsImport "MouseScrollDelta")) `
            "two owners, three needles, three import rows" "3 import(s) of moved readers"
        Refused ($two + (& $settingsImport "ElementState") + (& $settingsImport "MouseButton") + (& $settingsImport "PanStep")) `
            "an import row of a needle no moved owner reads" `
            "it imports PanStep, which no owner moved into crate::runtime::settings with N-B reads (they read: ElementState, MouseButton, MouseScrollDelta)"

        # A row whose owner is not written in its module is malformed, not read as something else.
        Commit-Base ($header + $scroll + $scroll + $scrollE + $wheel + $import) "the base again"
        Refused ($all + "crate::Runtime::x`tN-B`tcrate::other`tMouseButton`n") "a row outside its module" `
            "a row whose owner is not written in its module"

        # A move never lands where its owner was already listed at the base (review N1): with
        # crate::f and crate::runtime::tabs::f both at the base, the tabs owner doubling while
        # crate::f leaves is the tabs key grown, not crate::f moved.
        $f = "crate::f`tN-B`tcrate`tMouseButton`n"
        $tabsF = "${tabs}::f`tN-B`t${tabs}`tMouseButton`n"
        Commit-Base ($header + $f + $tabsF) "a base listing one owner name in two modules"
        Refused ($header + $tabsF + $tabsF) "a move onto a module that already listed the owner" `
            "1 row(s) were added"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-pointer-debt: the two columns' transitions, unchanged, moved, moved-and-read-less, moved-and-read-more, added, still-at-its-module, ambiguous, import, a second import of a needle, unowed import, other-door import, needle-less import, batch 23's three-needle import, unread-needle import, malformed-row and destination-already-listed cases pass"
exit 0
