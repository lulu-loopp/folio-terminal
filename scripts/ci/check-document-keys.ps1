# the_document_key_register_is_append_only
#
# `docs/plans/DOCUMENT-KEYS.tsv` pins, per document and version, the shape of every type a
# persisted document's root reaches (0.4.10 plan row 68d, ruling R9 of
# T-DOWNGRADE-SAYS-SO-AND-OFFERS-THE-COPY-BACK, 2026-11-14). The tests are the discovery half:
# bt-persist's `document_keys_tests` and bt-app's `the_integration_records_keep_their_keys` hold
# each document's rows at its current version equal to the source, both ways. This script is the
# history half: it compares the rows with the merge base's, keyed by document, version and type.
#
# **The test alone passes a key added together with an in-place edit of its row**, which is how
# the two unbumped settings.json changes of 0.4.7 and 0.4.8 passed review (the audit's G2). So:
#
#   - a row the merge base has is still here, byte for byte: a row edited in place is refused,
#     and so is a row removed;
#   - a row the merge base does not have holds at a version above its document's highest at the
#     merge base: a key is admitted only by a new version (its migration step and every type's
#     row at that version, in one commit). A document the merge base does not name at all is new
#     and takes any version.
#
# Every row is five non-empty columns under the header, with a reason dated `YYYY-MM-DD: why`,
# and names its (document, version, type) once.

param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Relative = "docs/plans/DOCUMENT-KEYS.tsv",
    [string]$MainRef = "origin/main"
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

# A refusal is said on lines of its own, which no error view wraps, and the script exits 1.
function Refuse([string]$Message) {
    Write-Host "REFUSED: $Message"
    exit 1
}

$list = Join-Path $Repo $Relative
if (-not (Test-Path -LiteralPath $list)) {
    Refuse "$Relative is not in the tree - it is the persisted documents' key-set register"
}

function Read-KeyRows([string[]]$Lines, [string]$Where) {
    $rows = [ordered]@{}
    $header = $false
    foreach ($raw in $Lines) {
        $line = $raw.TrimEnd("`r")
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -cne "document`tversion`ttype`tshape`treason") {
                Refuse "$Where has the wrong column header: $line"
            }
            $header = $true
            continue
        }
        $columns = $line.Split("`t")
        if ($columns.Count -ne 5 -or @($columns | Where-Object { $_.Length -eq 0 }).Count -gt 0) {
            Refuse "$Where row is not five non-empty columns: $line"
        }
        if ($columns[1] -notmatch '^[0-9]+$') {
            Refuse "$Where row's version is not a number: $line"
        }
        if ($columns[4] -notmatch '^\d{4}-\d{2}-\d{2}: \S') {
            Refuse "$Where row's reason is not dated (YYYY-MM-DD: why): $line"
        }
        $key = "$($columns[0])`t$($columns[1])`t$($columns[2])"
        if ($rows.Contains($key)) {
            Refuse "$Where names $($columns[0]) v$($columns[1]) $($columns[2]) more than once"
        }
        $rows[$key] = [pscustomobject]@{
            Line = $line
            Document = $columns[0]
            Version = [long]$columns[1]
            Type = $columns[2]
        }
    }
    if (-not $header) { Refuse "$Where has no column header" }
    return $rows
}

$now = Read-KeyRows ([IO.File]::ReadAllLines($list)) $Relative

Push-Location $Repo
try {
    & git rev-parse --verify --quiet $MainRef *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "no $MainRef in this clone - the document-keys comparison did not run"
        exit 2
    }
    $found = & git merge-base HEAD $MainRef 2>$null
    $status = $LASTEXITCODE
    $base = if ($status -eq 0) { @($found)[0] } else { $null }
    if (-not $base) {
        Write-Host "no merge base with $MainRef in this clone - the document-keys comparison did not run"
        exit 2
    }
    $text = (& git show "${base}:${Relative}" 2>$null) | Out-String
    if ($LASTEXITCODE -ne 0) {
        Write-Host "$Relative is not in $base - no base register exists; the register has $($now.Count) rows. PASS"
        exit 0
    }
} finally {
    Pop-Location
}

$before = Read-KeyRows ($text -split "`r?`n") "$Relative at $($base.Substring(0, 12))"

$highest = @{}
foreach ($row in $before.Values) {
    if (-not $highest.ContainsKey($row.Document) -or $row.Version -gt $highest[$row.Document]) {
        $highest[$row.Document] = $row.Version
    }
}

$problems = @()
foreach ($key in $before.Keys) {
    $old = $before[$key]
    if (-not $now.Contains($key)) {
        $problems += "removed: the merge base's row of $($old.Document) v$($old.Version) $($old.Type) is gone; a row is never removed:`n    $($old.Line)"
        continue
    }
    if ($now[$key].Line -cne $old.Line) {
        $problems += "edited in place: the row of $($old.Document) v$($old.Version) $($old.Type) is not the merge base's; a row is never edited, a new key takes a new version:`n    base: $($old.Line)`n    now:  $($now[$key].Line)"
    }
}
$added = @()
foreach ($key in $now.Keys) {
    if ($before.Contains($key)) { continue }
    $row = $now[$key]
    $added += $row
    if ($highest.ContainsKey($row.Document) -and $row.Version -le $highest[$row.Document]) {
        $problems += "not above the merge base: the new row of $($row.Document) v$($row.Version) $($row.Type) holds at a version the merge base already pins (its highest is v$($highest[$row.Document])); bump the version:`n    $($row.Line)"
    }
}

Write-Host "$Relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($now.Count), $($added.Count) added."
if ($problems.Count -gt 0) {
    Write-Host "REFUSED: the document-key register changed under a version it already pins:"
    foreach ($problem in $problems) { Write-Host $problem }
    Write-Host "Bump the document's version, add its migration step and add the new version's rows (docs/CONVENTIONS.md)."
    exit 1
}
foreach ($row in $added) {
    Write-Host "  admitted at a new version: $($row.Document) v$($row.Version) $($row.Type)"
}
Write-Host "document-key register kept every base row and grew only at new versions. PASS"
exit 0
