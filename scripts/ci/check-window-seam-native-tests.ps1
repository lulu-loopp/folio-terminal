# Scratch-repository tests for check-window-seam-native.ps1's move rule (T-MOVED-NOT-ADDED-RULE):
# the unchanged list passes; a function moved to another module and a const moved to another file
# pass; an added row, a method whose owner (a type) changed, a function moved onto a type's impl
# and a function arriving at two modules are each refused. (A move whose old place still lists the
# row cannot be written here: a row's identity is its kind and name, so such a row matches whole.)

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-window-seam-native.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-window-seam-native-gate-" + [guid]::NewGuid())
$relative = "docs/plans/window-seam-native.tsv"
$header = "# a comment the gate does not compare`nkind`towner`tname`treason`n"
$create = "method`tRuntime`tcreate`tbuilds the GPU device`n"
$rebuild = "function`tcrate`trebuild_every_native_surface`tthe native backend's rebuild`n"
$check = "const`tmain.rs`t_`ta compile-time check`n"

function Write-List([string]$Text) {
    $path = Join-Path $root $relative
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $Text)
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

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email window-seam-native-gate@example.invalid
        & git config user.name window-seam-native-gate
        & git config core.autocrlf false
        Write-List ($header + $create + $rebuild + $check)
        & git add .
        & git commit -q -m base
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()

        Passes ($header + $create + $rebuild + $check) "unchanged" "3 rows -> 3, 0 added, 0 moved, 0 renamed."
        $movedFunction = $rebuild.Replace("`tcrate`t", "`tcrate::runtime::frame`t")
        Passes ($header + $create + $movedFunction + $check) "a function moved to another module" "3 rows -> 3, 0 added, 1 moved, 0 renamed."
        Passes ($header + $create + $movedFunction + $check.Replace("`tmain.rs`t", "`truntime/frame.rs`t")) `
            "a function and a const moved" "3 rows -> 3, 0 added, 2 moved, 0 renamed."
        Passes ($header + $create + $movedFunction.Replace("rebuild`n", "rebuild, reworded`n") + $check) `
            "a moved function whose reason is reworded" "1 moved, 0 renamed."

        Refused ($header + $create + $rebuild + $check + "method`tRuntime`tplanted_probe`ta planted row`n") `
            "an added row" "only shrinks: 1 row(s) not at the merge base"
        Refused ($header + $create.Replace("`tRuntime`t", "`tWindowRuntime`t") + $rebuild + $check) `
            "a method whose owner changed" "method WindowRuntime create"
        Refused ($header + $create + $rebuild.Replace("`tcrate`t", "`tRuntime`t") + $check) `
            "a function moved onto a type's impl" "function Runtime rebuild_every_native_surface"
        Refused ($header + $create + $movedFunction + $rebuild.Replace("`tcrate`t", "`tcrate::runtime::panes`t") + $check) `
            "one function arriving at two modules" "and arrived at more than one place"
        Refused ($header + $create + $movedFunction.Replace("rebuild_every", "rebuild_each") + $check) `
            "a moved function renamed" "function crate::runtime::frame rebuild_each_native_surface"

        # The rename arm: a row renamed in place is accepted only with a dated reason naming its
        # ticket; without one, or two at once, it is refused. (No row has a count to grow.)
        $renamed = "method`tRuntime`tcreate_native`t2026-10-24: T-X the builder renamed`n"
        Passes ($header + $renamed + $rebuild + $check) "a method renamed with a dated reason" "0 added, 0 moved, 1 renamed."
        Refused ($header + $create.Replace("`tcreate`t", "`tcreate_native`t") + $rebuild + $check) "a method renamed without a dated reason" `
            "method Runtime create_native: it would be method Runtime create renamed, and a rename carries a dated reason naming its ticket"
        Refused ($header + $renamed + $renamed.Replace("create_native", "create_headless") + $rebuild + $check) "two renames with one identity" `
            "more than one row of its kind and owner left or arrived, so no rename can be paired"

        # A base row whose reason is already dated: a rename that keeps that reason carries no new
        # one, and is refused (review N2).
        $dated = "method`tRuntime`tcreate`t2026-10-24: T-X built`n"
        Write-List ($header + $dated + $rebuild + $check)
        & git add .
        & git commit -q -m "a base whose reason is dated"
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Refused ($header + $dated.Replace("`tcreate`t", "`tcreate_native`t") + $rebuild + $check) `
            "a rename that keeps the base row's dated reason" "a rename carries a new dated reason, and this one is the base row's"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-window-seam-native: unchanged, moved function, moved const, reworded reason, added, method owner, onto an impl, ambiguous and renamed cases pass; and the rename arm's dated, undated, two-at-once and kept-reason cases pass"
exit 0
