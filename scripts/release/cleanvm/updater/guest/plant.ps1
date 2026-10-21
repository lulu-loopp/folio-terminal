<#
.SYNOPSIS
    Guest side of E1-b's planted rows (clean-vm.md 4.4, RW): put a journal the host
    wrote into the installed Folio's home, and the rescue copy the row names.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1, started by `updater/run-row.ps1`
    (not -noWait: the row starts Folio only once this has ended). Folio is not
    running: the row reverted to a snapshot where it never ran.

        H\journal.json              the host's planted bytes, written whole and flushed
        H\<txn>\rescue\folio.exe    -Rescue B or held: the successor's own executable
                                    (`rescue-b.exe` beside this script); A: a copy of the
                                    installed folio.exe (an intentionally substituted older
                                    rescue, the design note's R2-19); none: nothing

    H is `<GuestHome>\folio\.folio-update`; `<txn>` is the journal's own `txn`. The
    hold of the `held` variant is the watcher's (`watch.ps1 -HoldRescueAt * `),
    started by the host before Folio is. Writes updater-plant-<Tag>.txt: what was
    placed, with each file's SHA-256.
#>
param(
  [Parameter(Mandatory)] [ValidateSet('B', 'A', 'none', 'held')] [string] $Rescue,
  [string] $Tag = 'run',
  [string] $GuestHome = 'C:\folio-vm'
)
$ErrorActionPreference = 'Stop'
$install = Join-Path $GuestHome 'folio'
$H = Join-Path $install '.folio-update'
$log = Join-Path $GuestHome "updater-plant-$Tag.txt"
function Say($s) { Add-Content -LiteralPath $log -Value "$((Get-Date).ToString('HH:mm:ss.fff')) $s" -Encoding UTF8 }
function Put($from, $to) {
  [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($to)) | Out-Null
  $bytes = [IO.File]::ReadAllBytes($from)
  $fs = New-Object IO.FileStream($to, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::None)
  try { $fs.Write($bytes, 0, $bytes.Length); $fs.Flush($true) } finally { $fs.Dispose() }
  Say ("placed {0} ({1} bytes, sha256 {2})" -f $to, $bytes.Length, (Get-FileHash -LiteralPath $to -Algorithm SHA256).Hash.ToLowerInvariant())
}
try {
  if (@(Get-Process folio -ErrorAction SilentlyContinue).Count -gt 0) { throw 'a folio.exe is running; the planted row starts from a guest where none is' }
  $planted = Join-Path $PSScriptRoot 'planted-journal.json'
  $txn = [regex]::Match([IO.File]::ReadAllText($planted), '"txn"\s*:\s*"([0-9a-f]{32})"')
  if (-not $txn.Success) { throw 'the planted journal names no txn' }
  $txn = $txn.Groups[1].Value
  Put $planted (Join-Path $H 'journal.json')
  $rescue = Join-Path (Join-Path (Join-Path $H $txn) 'rescue') 'folio.exe'
  switch ($Rescue) {
    'B' { Put (Join-Path $PSScriptRoot 'rescue-b.exe') $rescue }
    'held' { Put (Join-Path $PSScriptRoot 'rescue-b.exe') $rescue }
    'A' { Put (Join-Path $install 'folio.exe') $rescue }
    'none' { [IO.Directory]::CreateDirectory((Join-Path $H $txn)) | Out-Null; Say "no rescue copy at $rescue" }
  }
  Say "PLANT DONE txn=$txn rescue=$Rescue"
} catch { Say "PLANT FAILED: $($_.Exception.Message)"; exit 1 }
