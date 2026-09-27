<#
.SYNOPSIS
    The guest-side half of the updater hard-reset test: watch the journal phase
    and write a marker file when the target row's state is reached.

.DESCRIPTION
    `hard-reset-in-vm.ps1` copies this into the guest and runs it. It does not
    drive Folio's UI — it watches the journal file that the updater writes during
    its transaction, and when the journal's phase matches the target row, it writes
    a marker file that the host waits for.

    The host then hard-stops the guest (`vmrun stop <vmx> hard`), which is the
    whole point: the marker says "the durable state is now at row X", and the hard
    stop says "the power went out at that instant".

    This script exits after writing the marker. It does not survive the hard stop,
    and it does not need to.

.PARAMETER Row
    The W/M row to watch for: W1..W13 or M1..M11.

.PARAMETER GuestHome
    The working directory in the guest.

.PARAMETER PollIntervalSeconds
    How often to check the journal.

.PARAMETER TimeoutSeconds
    How long to wait before giving up.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^[WM]\d{1,2}$')]
    [string] $Row,
    [string] $GuestHome = 'C:\folio-vm',
    [int] $PollIntervalSeconds = 2,
    [int] $TimeoutSeconds = 600
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# The journal lives inside the install folder's `.folio-update` directory.
$installDir = Join-Path $GuestHome 'folio'
$updateHome = Join-Path $installDir '.folio-update'

# Map each row to the journal phase it represents.
# The phase names match the `Phase` enum in `update_txn.rs`.
$rowToPhase = @{
    'W1'  = 'Allocated'
    'W2'  = 'Prepared'
    'W3'  = 'Handoff'
    'W4'  = 'Handoff'       # entrance written, Armed not yet durable
    'W5'  = 'Armed'
    'W6'  = 'Moving'
    'W7'  = 'Trial'         # no receipt
    'W8'  = 'Trial'         # with receipt
    'W9'  = 'RollbackIntent'
    'W10' = 'Stuck'
    'W11' = 'RolledBack'
    'W12' = 'Committed'
    'W13' = 'Abandoned'
    'M1'  = 'Allocated'
    'M2'  = 'Prepared'
    'M3'  = 'Handoff'
    'M4'  = 'Armed'
    'M5'  = 'Exchanging'
    'M6'  = 'Exchanging'
    'M7'  = 'Trial'
    'M8'  = 'Trial'
    'M9'  = 'RollbackIntent'
    'M10' = 'Stuck'
    'M11' = 'RolledBack'    # or Abandoned or Committed-with-debt
}

$targetPhase = $rowToPhase[$Row]
if (-not $targetPhase) {
    throw "unknown row: $Row"
}

$markerFile = Join-Path $GuestHome "updater-marker-$Row.txt"

# Find the journal file. The transaction directory is the first subdirectory of
# the update home that contains a journal.json.
function Find-Journal {
    if (-not (Test-Path -LiteralPath $updateHome)) { return $null }
    $dirs = Get-ChildItem -LiteralPath $updateHome -Directory -ErrorAction SilentlyContinue
    foreach ($d in $dirs) {
        $j = Join-Path $d.FullName 'journal.json'
        if (Test-Path -LiteralPath $j -PathType Leaf) { return $j }
    }
    return $null
}

# Read the journal's current phase. The journal is a JSON file; the phase is in
# the `phase` field (or nested under `body.phase` depending on schema version).
# We do a simple text search for reliability with PowerShell 5.1.
function Get-JournalPhase {
    param([string] $Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    $text = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 -ErrorAction SilentlyContinue
    if (-not $text) { return $null }
    # Look for "phase":"<value>" — the journal writes it as a JSON string.
    if ($text -match '"phase"\s*:\s*"([^"]+)"') {
        return $Matches[1]
    }
    # Also try a bare word after "phase": (some serialisations).
    if ($text -match '"phase"\s*:\s*\{?\s*"(\w+)"') {
        return $Matches[1]
    }
    return $null
}

# For W8 vs W7: W8 needs a receipt file to exist.
function Test-Receipt {
    if (-not (Test-Path -LiteralPath $updateHome)) { return $false }
    $dirs = Get-ChildItem -LiteralPath $updateHome -Directory -ErrorAction SilentlyContinue
    foreach ($d in $dirs) {
        $receipts = Get-ChildItem -LiteralPath $d.FullName -Filter 'health-*' -File -ErrorAction SilentlyContinue
        if ($receipts -and $receipts.Count -gt 0) { return $true }
    }
    return $false
}

# For W4 vs W3/W5: W4 needs the Run value written but Armed not durable.
# We check whether a FolioUpdate-* value exists in the Run key.
function Test-RunValue {
    try {
        $runKey = Get-ItemProperty -LiteralPath 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue
        if (-not $runKey) { return $false }
        $names = $runKey.PSObject.Properties | Where-Object { $_.Name -match '^FolioUpdate-' }
        return ($names -and @($names).Count -gt 0)
    } catch {
        return $false
    }
}

# Decide whether the current state matches the target row. Some rows share a
# phase but are distinguished by other evidence (receipt, Run value).
function Test-RowReached {
    param([string] $Phase)
    if (-not $Phase) { return $false }

    # Normalise: the phase string might be a Rust enum variant name.
    $p = $Phase

    switch ($Row) {
        'W4' {
            # Handoff + Run value present + not yet Armed.
            return ($p -eq 'Handoff' -and (Test-RunValue))
        }
        'W8' {
            # Trial + receipt exists.
            return ($p -eq 'Trial' -and (Test-Receipt))
        }
        'M8' {
            return ($p -eq 'Trial' -and (Test-Receipt))
        }
        'M6' {
            # Exchanging + the live identity is new. We cannot easily check
            # the identity from PowerShell 5.1, so we accept Exchanging and
            # let the harness operator verify which identity is live.
            return ($p -eq 'Exchanging')
        }
        'M5' {
            # Exchanging + live identity is old. Same caveat as M6.
            return ($p -eq 'Exchanging')
        }
        default {
            return ($p -eq $targetPhase)
        }
    }
}

# ── Poll ─────────────────────────────────────────────────────────────────────

$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$reached = $false

while ([DateTime]::UtcNow -lt $deadline) {
    $journal = Find-Journal
    if ($journal) {
        $phase = Get-JournalPhase -Path $journal
        if ($phase -and (Test-RowReached -Phase $phase)) {
            $reached = $true
            break
        }
    }
    Start-Sleep -Seconds $PollIntervalSeconds
}

# ── Write the marker ─────────────────────────────────────────────────────────

$now = Get-Date -Format 'yyyy-MM-ddTHH:mm:ss'
if ($reached) {
    $journal = Find-Journal
    $phase = if ($journal) { Get-JournalPhase -Path $journal } else { '(unknown)' }
    Set-Content -LiteralPath $markerFile -Value "row=$Row phase=$phase at=$now" -Encoding UTF8
} else {
    # Write a marker anyway so the host knows the watcher ran to completion
    # without reaching the target. The host decides whether to hard-stop.
    Set-Content -LiteralPath $markerFile -Value "row=$Row phase=NOT_REACHED at=$now timeout=${TimeoutSeconds}s" -Encoding UTF8
}
