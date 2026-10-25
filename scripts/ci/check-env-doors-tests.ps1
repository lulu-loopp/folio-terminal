# Scratch-repository tests for check-env-doors.ps1: the no-base and unchanged
# passes, a shrink and a row that reads less pass, and each of the gate's three
# refusals fails for its own reason (an added row, a row reading a name its base
# row did not, a row holding more refused shapes than its base row). And the move
# rule (T-MOVED-NOT-ADDED-RULE): against a list without the `module` column no
# row is paired as moved; against one with it, a row moved to another file and
# module passes, and so does one moved that reads less, while a moved row reading
# a new name or holding more refused shapes, a row arriving at two places, a row
# moved to another package and a moved row renamed are each refused. (A move
# whose old place still lists the item cannot be written here: the key is the
# file and the item, so such a row matches by key.)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-env-doors.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-env-doors-gate-" + [guid]::NewGuid())
$relative = "docs/plans/ENV-DOORS.tsv"
$header = "# a comment the gate does not compare`nfile`titem`tcategory`tnames`trefused`tticket`treason`n"
$door = "crates/x/src/lib.rs`tcrate::door`tdoor`tBT_A,BT_B`t1`tT-X`ta door not moved yet`n"
$entry = "crates/x/src/main.rs`tcrate::main`tentry`tBT_C`t0`tT-X`tthe entry`n"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
}

function Run-Gate([int]$Want, [string]$Case) {
    $output = & pwsh -NoProfile -File $gate -Repo $root -Relative $relative 2>&1 | Out-String
    $got = $LASTEXITCODE
    if ($got -ne $Want) { throw "$Case returned $got, wanted $Want`n$output" }
    return $output
}

function Refused([string]$Text, [string]$Case, [string]$Reason) {
    Write-List $Text
    $output = Run-Gate 1 $Case
    # The refusal is a thrown error, which the host wraps at its width and decorates: compare
    # with the colours, the decorations and every blank taken out.
    $plain = ($output -replace '\x1b\[[0-9;]*m', '') -replace '[\s|]', ''
    if ($plain -notmatch [regex]::Escape(($Reason -replace '\s', ''))) {
        throw "$Case was refused for the wrong reason (wanted '$Reason'): $output"
    }
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email env-doors-gate@example.invalid
        & git config user.name env-doors-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        & git add .
        & git commit -q -m base
        $emptyBase = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $emptyBase

        Write-List ($header + $door + $entry)
        $output = Run-Gate 0 "no base list"
        if ($output -notmatch "the commit that introduces it") {
            throw "the no-base-list pass was not loud: $output"
        }

        & git add .
        & git commit -q -m list
        $base = (& git rev-parse HEAD).Trim()
        & git update-ref refs/remotes/origin/main $base
        $output = Run-Gate 0 "unchanged"
        if ($output -notmatch "2 rows -> 2, 0 removed, 0 grown") { throw "the unchanged pass said: $output" }

        Write-List ($header + $door)
        Run-Gate 0 "a row removed" | Out-Null

        Write-List ($header + $door.Replace("BT_A,BT_B`t1", "BT_A`t0").Replace("`tdoor`t", "`tentry`t") + $entry)
        Run-Gate 0 "a row that reads less, its category rewritten" | Out-Null

        Refused ($header + $door + $entry + "crates/x/src/new.rs`tcrate::new`tdoor`tBT_D`t0`tT-X`ta new reader`n") `
            "an added row" "crates/x/src/new.rs crate::new is a row the base does not have"
        Refused ($header + $door.Replace("BT_A,BT_B", "BT_A,BT_B,BT_E") + $entry) `
            "a row reading a new name" "crates/x/src/lib.rs crate::door reads BT_E, which its base row does not"
        Refused ($header + $door.Replace("BT_A,BT_B`t1", "BT_A,BT_B`t2") + $entry) `
            "a row holding more refusals" "crates/x/src/lib.rs crate::door holds 2 refused shape(s), its base row 1"

        # The move rule. The base so far has no `module` column: no row is paired as moved.
        $header8 = $header.Replace("`treason`n", "`treason`tmodule`n")
        $door8 = $door.Replace("`n", "`tcrate`n")
        $entry8 = $entry.Replace("`n", "`tcrate`n")
        $moved = "crates/x/src/runtime/web.rs`tcrate::runtime::web::door`tdoor`tBT_A,BT_B`t1`tT-X`ta door not moved yet`tcrate::runtime::web`n"
        Write-List ($header8 + $door8 + $entry8)
        $output = Run-Gate 0 "the column added"
        if ($output -notmatch "2 rows -> 2, 0 removed, 0 grown, 0 moved") { throw "the column's transition said: $output" }
        Refused ($header8 + $moved + $entry8) "a move against a base without the column" `
            "crates/x/src/runtime/web.rs crate::runtime::web::door is a row the base does not have"

        Write-List ($header8 + $door8 + $entry8)
        & git add .
        & git commit -q -m "the list with its module column"
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Write-List ($header8 + $moved + $entry8)
        $output = Run-Gate 0 "a row moved"
        if ($output -notmatch "2 rows -> 2, 0 removed, 0 grown, 1 moved") { throw "the moved row was not counted as moved: $output" }
        Write-List ($header8 + $moved.Replace("BT_A,BT_B`t1", "BT_A`t0") + $entry8)
        Run-Gate 0 "a moved row that reads less" | Out-Null
        Refused ($header8 + $moved.Replace("BT_A,BT_B", "BT_A,BT_B,BT_E") + $entry8) "a moved row reading a new name" `
            "crates/x/src/runtime/web.rs crate::runtime::web::door reads BT_E, which its base row (moved from crates/x/src/lib.rs (crate)) does not"
        Refused ($header8 + $moved.Replace("BT_A,BT_B`t1", "BT_A,BT_B`t2") + $entry8) "a moved row holding more refusals" `
            "crates/x/src/runtime/web.rs crate::runtime::web::door holds 2 refused shape(s), its base row (moved from crates/x/src/lib.rs (crate)) 1"
        Refused ($header8 + $moved + $moved.Replace("web", "panes") + $entry8) "one row arriving at two places" `
            "and arrived at more than one place"
        Refused ($header8 + $moved.Replace("crates/x/", "crates/y/") + $entry8) "a row moved to another package" `
            "crates/y/src/runtime/web.rs crate::runtime::web::door is a row the base does not have"
        Refused ($header8 + $moved.Replace("::door`t", "::doors`t") + $entry8) "a moved row renamed" `
            "crates/x/src/runtime/web.rs crate::runtime::web::doors is a row the base does not have"
        Refused ($header8 + $moved.Replace("`tcrate::runtime::web`n", "`tcrate::runtime`n").Replace("crate::runtime::web::door", "crate::runtimes::door") + $entry8) `
            "a row whose item is not written in its module" "a row whose item is not written in its module"

        # The rename arm: an item renamed in place is accepted only with a dated reason naming its
        # ticket; without one, with a grown count, or two at once, it is refused.
        $renamed = $door8.Replace("crate::door`t", "crate::opened_door`t").Replace("a door not moved yet", "2026-10-24: T-X the read moved into its own function")
        Write-List ($header8 + $renamed + $entry8)
        $output = Run-Gate 0 "a row renamed with a dated reason"
        if ($output -notmatch "2 rows -> 2, 0 removed, 0 grown, 0 moved, 1 renamed") { throw "the renamed row was not counted as renamed: $output" }
        Refused ($header8 + $door8.Replace("crate::door`t", "crate::opened_door`t") + $entry8) "a row renamed without a dated reason" `
            "crates/x/src/lib.rs crate::opened_door is a row the base does not have: it would be crates/x/src/lib.rs crate::door renamed, and a rename carries a dated reason naming its ticket"
        Refused ($header8 + $renamed.Replace("2026-10-24: T-X", "2026-13-40: T-X") + $entry8) "a row renamed with a reason dated on no real day" `
            "is not one"
        Refused ($header8 + $renamed.Replace("BT_A,BT_B`t1", "BT_A,BT_B`t2") + $entry8) "a renamed row whose count grew" `
            "crates/x/src/lib.rs crate::opened_door holds 2 refused shape(s), its base row (renamed from crates/x/src/lib.rs crate::door) 1"
        Refused ($header8 + $renamed + $renamed.Replace("crate::opened_door", "crate::other_door") + $entry8) "two renames with one identity" `
            "more than one row of its file, module and names left or arrived, so no rename can be paired"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-env-doors: no-base, unchanged, shrink, read-less, added-row, new-name and more-refusals cases pass; and the move rule's column transition, moved, moved-and-read-less, moved-with-a-new-name, moved-with-more-refusals, ambiguous, another-package, renamed and malformed-module cases pass; and the rename arm's dated, undated, no-real-day, grown and two-at-once cases pass"
exit 0
