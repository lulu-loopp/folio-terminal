# Scratch-repository tests for check-doc-link-debt.ps1 (T-DOC-LINK-DEBT, 0.4.9 plan row 78): the
# unchanged list passes, and so do a row removed (a link fixed), an owner moved to another module,
# one moved with a link fewer, a changed `file` column and a changed stamp line; an added row, a
# duplicated row, a row of another shape, an owner renamed in place, a moved owner with a link
# more, a move whose old module still lists the owner, an owner arriving at two modules, an
# `(outside any item)` row of a module the base did not list, a malformed row and a missing
# header are each refused for their own reason; a base without the list passes loudly, and a
# clone with no merge base exits 2.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-doc-link-debt.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-doc-link-debt-gate-" + [guid]::NewGuid())
$relative = "docs/plans/DOC-LINK-DEBT.tsv"
$header = "# a comment the gate does not compare`n# measured at 0123abcd`nowner`ttarget`tshape`tmodule`tfile`n"
$scroll = "crate::Runtime::scroll_tab_strip`thit_chrome`tunresolved`tcrate`tmain`n"
$scrollTwo = "crate::Runtime::scroll_tab_strip`tSelf::redraw`tunresolved`tcrate`tmain`n"
$unit = "crate::upright_wheel`tunit`tambiguous`tcrate`tmain`n"
$moduleDoc = "crate::quit (outside any item)`tQUIT_RECOVERY_BY`tunresolved`tcrate::quit`tquit`n"
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
    if ($plain -notmatch [regex]::Escape(($Reason -replace '[\s|]', ''))) {
        throw "$Case was refused for the wrong reason (wanted '$Reason'): $output"
    }
}

# A row of `$Row` moved into the runtime's tabs module.
function Moved([string]$Row) {
    return $Row.Replace("crate::Runtime::", "${tabs}::Runtime::").Replace("`tcrate`t", "`t${tabs}`t").Replace("`tmain`n", "`truntime/tabs`n")
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email doc-link-debt-gate@example.invalid
        & git config user.name doc-link-debt-gate
        & git config core.autocrlf false

        # No merge base at all: not a pass.
        [IO.File]::WriteAllText((Join-Path $root "seed"), "seed`n")
        & git add .
        & git commit -q -m "a commit with no list and no origin/main"
        Write-List ($header + $scroll)
        $output = Run-Gate 2 "no merge base"
        if ($output -notmatch "This is not a pass") { throw "no merge base did not say it was no pass: $output" }

        # A base without the list: the commit that introduces it passes, loudly.
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Passes ($header + $scroll) "the base has no list" "this is the commit that introduces it, and it has 1 rows"

        $all = $header + $scroll + $scroll + $scrollTwo + $unit + $moduleDoc
        Commit-Base $all "the base"
        Passes $all "unchanged" "5 rows -> 5, 0 added, 0 removed, 0 moved"
        Passes ($header + $scroll + $scrollTwo + $unit + $moduleDoc) "a link fixed" "5 rows -> 4, 0 added, 1 removed, 0 moved"
        Passes ($all.Replace("`tmain`n", "`tsomewhere_else`n")) "the file column changed" "5 rows -> 5, 0 added, 0 removed, 0 moved"
        Passes ($all.Replace("# measured at 0123abcd", "# measured at 4567cdef")) "the stamp changed" "0 added"

        $movedAll = $header + (Moved $scroll) + (Moved $scroll) + (Moved $scrollTwo) + $unit + $moduleDoc
        Passes $movedAll "an owner moved" "5 rows -> 5, 0 added, 0 removed, 3 moved"
        Passes ($header + (Moved $scroll) + (Moved $scrollTwo) + $unit + $moduleDoc) "an owner moved with a link fewer" `
            "5 rows -> 4, 0 added, 1 removed, 2 moved"
        Refused ($movedAll + (Moved $scroll)) "a moved owner with a link more" `
            "crate::runtime::tabs::Runtime::scroll_tab_strip hit_chrome unresolved: it moved from crate with more rows than it had there"

        Refused ($all + "crate::Runtime::press_tab`tplanted`tunresolved`tcrate`tmain`n") "an added row" "1 row(s) were added"
        Refused ($all + $unit) "a duplicated row" "1 row(s) were added"
        Refused ($all.Replace("`tunit`tambiguous`t", "`tunit`tunresolved`t")) "a row of another shape" "1 row(s) were added"
        Refused ($all.Replace("scroll_tab_strip", "scroll_strip")) "an owner renamed in place (no rename arm)" "3 row(s) were added"
        Refused ($header + (Moved $scroll) + (Moved $scroll) + $scrollTwo + $unit + $moduleDoc) `
            "a move whose old module still lists the owner" "but crate still lists Runtime::scroll_tab_strip"
        Refused ($header + (Moved $scroll) + (Moved $scroll).Replace("tabs", "panes") + (Moved $scrollTwo) + $unit + $moduleDoc) `
            "an owner arriving at two modules" "and arrived at more than one place"
        Refused ($header + $scroll + $scroll + $scrollTwo + $unit + $moduleDoc.Replace("crate::quit", "crate::runtime::quit")) `
            "a module doc row of a module the base did not list" "1 row(s) were added"
        Refused ($all + "crate::Runtime::x`tplanted`tunresolved`tcrate::other`tmain`n") "a row outside its module" `
            "a row whose owner is not written in its module"
        Refused ($all + "crate::Runtime::x`tplanted`tunresolved`tcrate`n") "a row of four columns" `
            "a row that is not owner, target, shape, module and file"
        Refused ("# only a comment`n" + $scroll) "a list without its header" "has no column header"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-doc-link-debt: no-merge-base, introducing, unchanged, link fixed, file column, stamp, moved, moved-with-fewer, moved-with-more, added, duplicated, other shape, renamed, still-at-its-module, ambiguous move, unlisted module doc, outside-its-module, four-column and headerless cases pass"
exit 0
