# Scratch-repository tests for check-document-keys.ps1 (0.4.10 plan row 68d, D12): each refusal
# planted and required to fail for its own reason, while the no-base, unchanged, new-version and
# new-document cases pass.

$ErrorActionPreference = "Stop"
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$gate = Join-Path $PSScriptRoot "check-document-keys.ps1"
$root = Join-Path ([IO.Path]::GetTempPath()) ("folio-document-keys-gate-" + [guid]::NewGuid())
$relative = "docs/plans/DOCUMENT-KEYS.tsv"
$reason = "2026-11-14: T-PLANT (row 68d); a planted row"
$header = "# a comment line`ndocument`tversion`ttype`tshape`treason`n"
$rootV1 = "settings`t1`tSettingsV1`tpubstructSettingsV1{pubschema_version:u32,}`t$reason`n"
$leafV1 = "settings`t1`tThemeV1`tpubenumThemeV1{Dark,}`t$reason`n"
$pinsV2 = "pins`t2`tPinsV1`tpubstructPinsV1{pubschema_version:u32,}`t$reason`n"
$baseText = $header + $rootV1 + $leafV1 + $pinsV2

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

function Expect-Refusal([string]$Text, [string]$Reason, [string]$Case) {
    Write-List $Text
    $output = Run-Gate 1 $Case
    if ($output -notmatch $Reason) { throw "$Case was refused for the wrong reason: $output" }
}

try {
    [IO.Directory]::CreateDirectory($root) | Out-Null
    Push-Location $root
    try {
        & git init -q
        & git config user.email document-keys-gate@example.invalid
        & git config user.name document-keys-gate
        [IO.File]::WriteAllText((Join-Path $root ".keep"), "base")
        & git add .
        & git commit -q -m base
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()

        Write-List $baseText
        $output = Run-Gate 0 "no base register"
        if ($output -notmatch "no base register exists" -or $output -notmatch "PASS") {
            throw "the no-base-register pass was not loud: $output"
        }

        & git add .
        & git commit -q -m register
        & git update-ref refs/remotes/origin/main (& git rev-parse HEAD).Trim()
        Run-Gate 0 "unchanged" | Out-Null

        # A new version of a document the base pins: every type's row again, at v2.
        $rootV2 = "settings`t2`tSettingsV1`tpubstructSettingsV1{pubschema_version:u32,pubnew:bool,}`t$reason`n"
        $leafV2 = "settings`t2`tThemeV1`tpubenumThemeV1{Dark,}`t$reason`n"
        Write-List ($baseText + $rootV2 + $leafV2)
        $output = Run-Gate 0 "a new version's rows"
        if ($output -notmatch "admitted at a new version: settings v2 SettingsV1" -or $output -notmatch "PASS") {
            throw "the new-version pass was not loud: $output"
        }

        Write-List ($baseText + "update-homes`t1`tUpdateHomesRecord`tpubstructUpdateHomesRecord{version:u32,}`t$reason`n")
        Run-Gate 0 "a document the base does not name" | Out-Null

        Expect-Refusal ($header + $rootV1.Replace("u32,}", "u32,pubnew:bool,}") + $leafV1 + $pinsV2) "edited in place" "a row's shape edited in place"
        Expect-Refusal ($header + $rootV1 + $leafV1.Replace("a planted row", "another reason") + $pinsV2) "edited in place" "a row's reason edited in place"
        Expect-Refusal ($header + $rootV1 + $pinsV2) "removed: the merge base's row of settings v1 ThemeV1" "a row removed"
        Expect-Refusal ($baseText + "settings`t1`tNewTypeV1`tpubstructNewTypeV1{}`t$reason`n") "not above the merge base" "a new type's row at the base's version"
        Expect-Refusal ($baseText + "pins`t1`tPinsV1`tpubstructPinsV1{}`t$reason`n") "not above the merge base" "a new row below the base's highest"
        Expect-Refusal ($baseText + "settings`t3`tSettingsV1`tx`tno date`n") "not dated" "an undated reason"
        Expect-Refusal ($baseText + "settings`t3`tSettingsV1`t`t$reason`n") "five non-empty columns" "an empty shape"
        Expect-Refusal ($baseText + "settings`tthree`tSettingsV1`tx`t$reason`n") "not a number" "a version that is no number"
        Expect-Refusal ($baseText + $rootV1) "more than once" "a duplicate row"
        Expect-Refusal ($baseText.Replace("document`tversion", "doc`tversion")) "wrong column header" "a wrong header"
    } finally {
        Pop-Location
    }
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

Write-Host "check-document-keys: no-base, unchanged, new version, new document, shape edited, reason edited, row removed, new type at the base's version, row below the base's highest, undated, empty column, version no number, duplicate and header cases pass"
exit 0
