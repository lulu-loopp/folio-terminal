# Scratch-repository tests for check-census-unknowns.ps1's move rule (T-MOVED-NOT-ADDED-RULE):
# the unchanged list passes; an unknown moved to another module passes, and so does one moved
# whose `sites` went down; an added unknown, a moved one whose `sites` grew, a move whose old
# module still lists the function, an ambiguous move and a move with a renamed function are each
# refused for their own reason. The gate's growth plants on the real tree stay in `gates-can-fail`.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-census-unknowns.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-census-unknowns-gate-" + [guid]::NewGuid())
$relative = "docs/plans/design/ownership-census-unknowns.tsv"
$annotations = "docs/plans/design/ownership-census-annotations.tsv"
$header = "# a comment the gate does not compare`nsite`tmodule`tfunction`treason`tsites`n"
$settle = "?.attention`tcrate`tsettle`ta closure parameter without a type`t2`n"
$other = "?.attention`tcrate`tsettle`twhat flat_map returns`t1`n"
$rail = "?.files`tcrate::seats`trail_chrome`twhat as_ref returns`t1`n"

function Write-File([string]$Relative, [string]$Text) {
    $path = Join-Path $root $Relative
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
    Write-File $relative $Text
    $output = Run-Gate 0 $Case
    if ($output -notmatch [regex]::Escape($Said)) { throw "$Case passed without saying '$Said': $output" }
}

function Refused([string]$Text, [string]$Case, [string]$Reason) {
    Write-File $relative $Text
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
        & git config user.email census-unknowns-gate@example.invalid
        & git config user.name census-unknowns-gate
        & git config core.autocrlf false
        Write-File $relative ($header + $settle + $rail)
        Write-File $annotations "fact`tpart`tclass`tproposed_owner`tnote`n"
        & git add .
        & git commit -q -m base
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()

        Passes ($header + $settle + $rail) "unchanged" "2 unknowns -> 2, 0 added, 0 grown, 0 shrunk, 0 removed, 0 moved."

        $moved = $settle.Replace("`tcrate`t", "`tcrate::runtime::attention`t")
        Passes ($header + $moved + $rail) "a moved unknown" "0 added, 0 grown, 0 shrunk, 0 removed, 1 moved."
        Passes ($header + $moved.Replace("`t2`n", "`t1`n") + $rail) "a moved unknown whose sites went down" "0 added, 0 grown, 1 shrunk, 0 removed, 1 moved."

        Refused ($header + $settle + $rail + "?.title`tcrate`tprobe`ta planted row`t1`n") `
            "an added unknown" "?.title in crate::probe (a planted row): 1 site(s), not at the merge base"
        Refused ($header + $moved.Replace("`t2`n", "`t3`n") + $rail) `
            "a moved unknown whose sites grew" "3 site(s), moved from crate, where it had 2"
        Refused ($header + $moved.Replace("`tsettle`t", "`tsettled`t") + $rail) `
            "a moved unknown with its function renamed" "?.attention in crate::runtime::attention::settled (a closure parameter without a type): 2 site(s), not at the merge base"
        Refused ($header + $settle.Replace("`tsettle`t", "`tsettled`t") + $rail) `
            "an unknown whose function was renamed in place (no rename arm here)" "?.attention in crate::settled (a closure parameter without a type): 2 site(s), not at the merge base"
        Refused ($header + $moved + $moved.Replace("::attention`t", "::panes`t") + $rail) `
            "one unknown arriving at two modules" "and arrived at more than one place"

        # A base whose function has a second unknown under another reason: moving one key while
        # the other stays says the function is still in its old module.
        Write-File $relative ($header + $settle + $other + $rail)
        & git add .
        & git commit -q -m "a function with two unknowns"
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Refused ($header + $moved + $other + $rail) `
            "a move whose old module still lists the function" "but crate still lists settle"
        Passes ($header + $moved + $other.Replace("`tcrate`t", "`tcrate::runtime::attention`t") + $rail) `
            "both unknowns of the function moved" "0 removed, 2 moved."
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-census-unknowns: unchanged, moved, moved-and-shrunk, added, moved-and-grown, renamed, ambiguous, still-at-its-module and whole-function-moved cases pass"
exit 0
