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
  [string]$GuestHome = 'C:\folio-vm',
  [int]$ForegroundSeconds = 180
)
$ErrorActionPreference = 'Continue'
$exe = Join-Path $Candidate 'folio.exe'
$probe = Join-Path $PSScriptRoot 'ui-probe.ps1'
$hog = Join-Path $PSScriptRoot 'cpu-hog.ps1'
$answered = Join-Path $PSScriptRoot 'load-row-settings.json'
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
# Who holds the foreground, and whose window is under a point: the two facts a refused foreground
# is read from (2026-10-21: two runs whose every cycle was refused said nothing about why).
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class LoadRowWindows {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint message, IntPtr w, IntPtr l);
  public const uint WM_CLOSE = 0x0010;
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder text, int max);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder text, int max);
  public static string Describe(IntPtr h) {
    if (h == IntPtr.Zero) { return "none"; }
    uint pid; GetWindowThreadProcessId(h, out pid);
    var title = new StringBuilder(128); GetWindowText(h, title, 128);
    var name = new StringBuilder(128); GetClassName(h, name, 128);
    return String.Format("hwnd={0} pid={1} class={2} title={3}", h, pid, name, title);
  }
  public static string At(int x, int y) { var p = new POINT(); p.X = x; p.Y = y; return Describe(WindowFromPoint(p)); }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
    public int cbSize; public int flags; public IntPtr hwndActive; public IntPtr hwndFocus; public IntPtr hwndCapture;
    public IntPtr hwndMenuOwner; public IntPtr hwndMoveSize; public IntPtr hwndCaret; public RECT rcCaret;
  }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread, ref GUITHREADINFO info);
  // The keyboard's own window: the foreground thread's focus. A key SendInput sends goes there.
  public static IntPtr Focus() {
    uint pid;
    uint thread = GetWindowThreadProcessId(GetForegroundWindow(), out pid);
    var info = new GUITHREADINFO();
    info.cbSize = Marshal.SizeOf(info);
    return GetGUIThreadInfo(thread, ref info) ? info.hwndFocus : IntPtr.Zero;
  }
  public static bool KeyboardIn(uint target) {
    IntPtr focus = Focus();
    if (focus == IntPtr.Zero) { return false; }
    uint pid; GetWindowThreadProcessId(focus, out pid);
    return pid == target;
  }
}
'@

function Window($procId) {
  $deadline = (Get-Date).AddSeconds(120)
  while ((Get-Date) -lt $deadline) {
    $h = [Probe]::AppWindow([uint32]$procId)
    if ($h -ne [IntPtr]::Zero) { return $h }
    Start-Sleep -Milliseconds 500
  }
  return [IntPtr]::Zero
}
# Foreground first, as ui-probe's own law asks before a key is sent. A window Windows opened
# without the foreground (the driver runs from a hidden console the user never touched) refuses
# the request; it is taken the way a person takes it — a real click at window+(60,-60), a point
# inside the pane, made only when that pixel is Folio's (keys.ps1's H-8) — and asked again.
# **And the keyboard, not only the foreground** (2026-10-21: two rows whose driver reported the
# foreground taken sent 60 cycles that Folio never saw — `file_reads … input_seen=false` all run
# long, Folio's own focus lost a second after it was gained). A key goes to the foreground
# thread's focus window, so the check is that this window is Folio's (`GetGUIThreadInfo`); a
# foreground without it is taken the way a person takes it, a click inside the pane.
function Rect($h) {
  $r = New-Object PRECT
  [void][Probe]::GetWindowRect($h, [ref]$r)
  return $r
}
# A point inside Folio's window to click, verified as Folio's pixel: the window's centre, else a
# point 60 px in from its bottom-left corner (inside the pane on every layout). $null when neither
# is Folio's (the window covered, or its rectangle not where the screen is).
function ClickPoint($h) {
  $r = Rect $h
  foreach ($point in @(@([int](($r.L + $r.R) / 2), [int](($r.T + $r.B) / 2)), @(($r.L + 60), ($r.B - 60)))) {
    if ([Probe]::PointBelongsTo($point[0], $point[1], $folio.Id)) { return $point }
  }
  return $null
}
function Front($h) {
  for ($try = 1; $try -le 4; $try++) {
    # The road that worked on before3: the foreground asked for, with the probe's thread attached
    # to the foreground thread's input.
    [void][Probe]::BringToFront($h)
    if ([LoadRowWindows]::KeyboardIn([uint32]$folio.Id)) { return $true }
    # Windows grants the foreground only to the process that had the last input: a tapped Alt
    # makes it this one (the desktop, Progman, held it on 2026-10-21's late boots), then ask again.
    [void][Probe]::TapVk([uint16]0x12)
    [void][Probe]::BringToFront($h)
    if ([LoadRowWindows]::KeyboardIn([uint32]$folio.Id)) { return $true }
    # And the person's way: a click inside the window, on a pixel that is Folio's.
    $point = ClickPoint $h
    if ($point) {
      [Probe]::Click($point[0], $point[1])
      Start-Sleep -Milliseconds 400
      if ([LoadRowWindows]::KeyboardIn([uint32]$folio.Id)) { return $true }
    }
  }
  $r = Rect $h
  $cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
  Say ("no keyboard: nothing sent (foreground {0}; focus {1}; window rect {2},{3},{4},{5}; under its centre {6}; folio pid={7})" -f `
      [LoadRowWindows]::Describe([LoadRowWindows]::GetForegroundWindow()), [LoadRowWindows]::Describe([LoadRowWindows]::Focus()),
      $r.L, $r.T, $r.R, $r.B, [LoadRowWindows]::At($cx, $cy), $folio.Id)
  return $false
}

# **The close, on every road** (2026-10-21: the row that never got the keyboard closed Folio and
# recorded nothing about it — and a close with the first run answered and no key sent is the
# datum the close-hang ticket needs). WM_CLOSE, then 30 s for Folio to exit; a Folio still
# running is the row's failure, with the processes still standing and the hang reports kept.
function CloseFolio {
  Mark 'close'
  # **WM_CLOSE to Folio's window, posted** (2026-10-21, T-CLOSE-HANG-ON-MAIN's finding):
  # `Process.CloseMainWindow()` picks the process's first visible unowned top-level window, which
  # is winit's thread event-target window (`WS_VISIBLE | WS_POPUP`, class `Winit Thread Event
  # Target`); its procedure hands WM_CLOSE to DefWindowProc, which destroys it and closes
  # nothing, and Folio's window never heard the close. The row posts the close to the window it
  # drives ($h, the largest visible window of Folio's process), and says which window .NET
  # would have picked.
  $folio.Refresh()
  Say ("close target {0}; Process.MainWindowHandle {1}" -f [LoadRowWindows]::Describe($h), [LoadRowWindows]::Describe($folio.MainWindowHandle))
  $closed = [LoadRowWindows]::PostMessage($h, [LoadRowWindows]::WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
  Say "WM_CLOSE posted -> $closed"
  $asked = Get-Date
  if ($folio.WaitForExit(30000)) {
    Say ("close ok: Folio exited with {0} after {1:N1} s" -f $folio.ExitCode, ((Get-Date) - $asked).TotalSeconds)
    return
  }
  Say 'FAILED close: Folio still running 30 s after WM_CLOSE'
  foreach ($name in @('folio', 'OpenConsole', 'powershell', 'pwsh', 'conhost')) {
    foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
      Say ("  still running: {0} pid={1} started={2:HH:mm:ss}" -f $process.ProcessName, $process.Id, $process.StartTime)
    }
  }
  $reports = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio\hang-reports'
  if (Test-Path -LiteralPath $reports) {
    $kept = Join-Path $GuestHome "load-$Tag-hang-reports.zip"
    Compress-Archive -Path (Join-Path $reports '*') -DestinationPath $kept -Force
    Say "  hang reports kept: $kept"
  } else {
    Say '  no hang report was written'
  }
}
function EndRow {
  Start-Sleep -Seconds 2
  Mark 'end'
  Copy-Item -LiteralPath $diag -Destination (Join-Path $GuestHome "load-$Tag-diagnostics.log") -Force
  Say 'load end'
  Set-Content -LiteralPath (Join-Path $GuestHome "load-$Tag-done.txt") -Value 'done' -Encoding UTF8
}

# **The row starts only once Folio can be given the keys** (2026-10-21: on boots whose logon came
# late, every cycle of a row was refused and its three phases measured nothing). Asked every few
# seconds for up to -ForegroundSeconds; a Folio that never takes it ends the row as a failure
# rather than as numbers.
function FrontWithin($h, $seconds) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) {
    if (Front $h) { return $true }
    Start-Sleep -Seconds 3
  }
  return $false
}
function Cycle($h, $seconds) {
  $end = (Get-Date).AddSeconds($seconds)
  $cycles = 0
  $sent = 0
  while ((Get-Date) -lt $end) {
    if (Front $h) {
      $sent++
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
  # How many cycles reached Folio: a phase whose keys were all refused measured nothing.
  Say "cycles $cycles sent $sent"
}

Say "load start candidate=$Candidate seconds=$Seconds processors=$([Environment]::ProcessorCount)"
# **The row runs with the first-run card already answered** (2026-10-21: the card stood over the
# pane through two rows whose keys never reached it, and through their WM_CLOSE). The guest is a
# fresh `clean` account with no Folio data; the row gives it a settings document from before the
# card existed (schema 25, every value the shipped default but the PowerShell invitation, which is
# answered too), and the candidate's own migrations carry it to its version — the v31 -> v32 step
# writes `first_run_card: Shown`, the product's rule that a returning reader never meets the card.
# A guest that already holds a settings.json is left as it is. The card itself is its own row.
$data = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio'
$settings = Join-Path $data 'settings.json'
if (Test-Path -LiteralPath $settings) {
  Say "settings.json already present: left as it is"
} else {
  [IO.Directory]::CreateDirectory($data) | Out-Null
  Copy-Item -LiteralPath $answered -Destination $settings
  Say "first run answered: a schema-25 settings.json placed for the candidate to migrate"
}
$env:BT_PERF_TRACE = '1'
$folio = Start-Process -FilePath $exe -RedirectStandardError $perf -PassThru
Say "started pid=$($folio.Id)"
$h = Window $folio.Id
if ($h -eq [IntPtr]::Zero) { Say 'no Folio window within 120 s'; Say 'load end'; exit 1 }
Start-Sleep -Seconds 8
if (-not (FrontWithin $h $ForegroundSeconds)) {
  Say "FAILED foreground: Folio never took the keyboard within $ForegroundSeconds s; the row measures nothing"
  CloseFolio
  EndRow
  exit 1
}
Say ("keyboard taken (focus {0})" -f [LoadRowWindows]::Describe([LoadRowWindows]::Focus()))
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
CloseFolio
EndRow
