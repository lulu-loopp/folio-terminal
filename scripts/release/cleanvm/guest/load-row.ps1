<#
.SYNOPSIS
    Guest side of the load row (clean-vm.md 4.6): Folio open, a person typing and scrolling in
    a pane that prints, before, during and after a machine-wide CPU load at Normal priority.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1, started by `run-load-row.ps1` in the
    logged-on session (hidden console under conhost.exe, so no window stands over Folio).

        launch      <Candidate>\folio.exe with BT_PERF_TRACE=1, its stderr into perf.txt
        settle      the first-launch cards dismissed with Escape (they apply nothing); four
                    boxed rows printed in the first tab, then a new tab opened over it
        baseline    -Seconds of the person's cycle
        hog         cpu-hog.ps1 started (2 x logical processors, Normal priority, -Seconds),
                    and -Seconds of the cycle under it
        after       -Seconds of the cycle once the hog has ended
        close       WM_CLOSE to the window, so the budget's exit summary is written

    **The person's cycle**, every two seconds: type `1..400|%{$_}` and Enter (the keystrokes'
    echo, then 400 lines of output), three wheel notches up into the history and three back.
    Keys and notches go through ui-probe's own class, loaded once into this process (a ui-probe
    run costs seconds on the guest, mostly compiling that class); every send verifies that
    Folio holds the foreground first and sends nothing otherwise.

    **The phases are cut by byte offsets**, because neither the budget lines nor the perf lines
    carry a time: at each boundary the lengths of diagnostics.log and perf.txt are written to
    load-<Tag>.txt beside the UTC instant, and the host reads each phase as the bytes between
    two boundaries.

    Writes, under -GuestHome: load-<Tag>.txt (ends with `load end`), load-<Tag>-perf.txt,
    load-<Tag>-diagnostics.log, load-<Tag>-hog.txt.
#>
param(
  [string]$Candidate = 'C:\folio-vm\candidate',
  [string]$Tag = 'run',
  [int]$Seconds = 60,
  [string]$GuestHome = 'C:\folio-vm'
)
$ErrorActionPreference = 'Continue'
$exe = Join-Path $Candidate 'folio.exe'
$probe = Join-Path $PSScriptRoot 'ui-probe.ps1'
$hog = Join-Path $PSScriptRoot 'cpu-hog.ps1'
$log = Join-Path $GuestHome "load-$Tag.txt"
$perf = Join-Path $GuestHome "load-$Tag-perf.txt"
$hogLog = Join-Path $GuestHome "load-$Tag-hog.txt"
$diag = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio\diagnostics.log'

function Stamp { [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ') }
function Say($s) { Add-Content -LiteralPath $log -Value "$(Stamp) $s" -Encoding UTF8 }
function Length($path) { if (Test-Path -LiteralPath $path) { (Get-Item -LiteralPath $path).Length } else { 0 } }
function Mark($name) { Say ("mark {0} diagnostics={1} perf={2}" -f $name, (Length $diag), (Length $perf)) }

$src = [IO.File]::ReadAllText($probe)
$m = [regex]::Match($src, "(?s)`nAdd-Type @'`r?`n(.*?)`r?`n'@")
Add-Type -TypeDefinition $m.Groups[1].Value
[Probe]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null

function Window($procId) {
  $deadline = (Get-Date).AddSeconds(120)
  while ((Get-Date) -lt $deadline) {
    $h = [Probe]::AppWindow([uint32]$procId)
    if ($h -ne [IntPtr]::Zero) { return $h }
    Start-Sleep -Milliseconds 500
  }
  return [IntPtr]::Zero
}
function Front($h) {
  if ([Probe]::BringToFront($h)) { return $true }
  Say 'no foreground: nothing sent'
  return $false
}
function Cycle($h, $seconds) {
  $end = (Get-Date).AddSeconds($seconds)
  $cycles = 0
  while ((Get-Date) -lt $end) {
    if (Front $h) {
      [void][Probe]::TypeText('1..400|%{$_}')
      [void][Probe]::TapVk([uint16]13)
      Start-Sleep -Milliseconds 600
      $r = New-Object PRECT
      [void][Probe]::GetWindowRect($h, [ref]$r)
      [void][Probe]::SetCursorPos([int](($r.L + $r.R) / 2), [int](($r.T + $r.B) / 2))
      [Probe]::Wheel(3)
      Start-Sleep -Milliseconds 500
      [Probe]::Wheel(-3)
    }
    $cycles++
    Start-Sleep -Milliseconds 900
  }
  Say "cycles $cycles"
}

Say "load start candidate=$Candidate seconds=$Seconds processors=$([Environment]::ProcessorCount)"
$env:BT_PERF_TRACE = '1'
$folio = Start-Process -FilePath $exe -RedirectStandardError $perf -PassThru
Say "started pid=$($folio.Id)"
$h = Window $folio.Id
if ($h -eq [IntPtr]::Zero) { Say 'no Folio window within 120 s'; Say 'load end'; exit 1 }
Start-Sleep -Seconds 8
foreach ($i in 1..2) { if (Front $h) { [void][Probe]::TapVk([uint16]27) }; Start-Sleep -Milliseconds 800 }
Start-Sleep -Seconds 4
# The owner's shape: a pane whose screen holds a boxed frame (an agent's TUI is one) in a tab
# that is not on screen. Four rows with a rule in one column make the capture a frame
# candidate; then a new tab (Ctrl+Shift+N) comes forward and the person works there.
if (Front $h) {
  [void][Probe]::TypeText("1..4|%{'ab $([char]0x2502) cd'}")
  [void][Probe]::TapVk([uint16]13)
  Start-Sleep -Seconds 3
  [void][Probe]::Chord($true, $true, $false, [uint16]0x4E)
  Say 'framed tab printed; a new tab opened over it'
}
Start-Sleep -Seconds 6

Mark 'baseline'
Cycle $h $Seconds
Mark 'hog'
$hogProcess = Start-Process -FilePath 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' `
  -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $hog, '-Seconds', "$Seconds", '-Log', $hogLog) `
  -WindowStyle Hidden -PassThru
Cycle $h $Seconds
$hogProcess.WaitForExit()
Mark 'after'
Cycle $h $Seconds
Mark 'close'
$closed = $folio.CloseMainWindow()
Say "WM_CLOSE -> $closed"
if (-not $folio.WaitForExit(30000)) { Say 'Folio still running 30 s after WM_CLOSE' }
Start-Sleep -Seconds 2
Mark 'end'
Copy-Item -LiteralPath $diag -Destination (Join-Path $GuestHome "load-$Tag-diagnostics.log") -Force
Say 'load end'
