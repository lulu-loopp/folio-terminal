<#
.SYNOPSIS
    Guest side of the updater rows (clean-vm.md 4.4): start Folio and press the
    update card's keys, the way a person does, through ui-probe.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1, started by
    `updater/run-row.ps1` with `-noWait` in the logged-on session (hidden
    console under conhost.exe, so no window stands over Folio). The pid is
    found in the guest (`Get-Process folio`, the install's folio.exe with a
    window); keys go through `ui-probe.ps1` beside this script (real SendInput;
    the probe verifies the foreground before it sends anything).

    **The ring.** Tab lights the FIRST drawn verb: the offer card draws
    Skip Later Update and the Restart card Later Restart, so Tab lights Skip or
    Later (`update_card.rs`, `enter_presses_nothing_until_the_ring_is_lit`).
    **Update and Restart are Shift+Tab, Enter**; `Tab`, `Enter` would press Skip
    (writing the skipped tag) or Later (U-31, H-4). Every press has a ring shot.

    **A window Windows opened without the foreground** (its taskbar button
    flashes, often after a reboot) makes ui-probe refuse every key. It is taken
    the way a person takes it: a real click at window+(60,-60), whose pixel
    ui-probe checks is Folio's before it clicks, then the key again (H-8).

    Every fresh launch shows *Welcome to Folio* and the PSReadLine invitation
    before the update card; the plans dismiss them with Escape (`later`), which
    applies nothing (H-5).

    -Plan is a comma list of steps:
      launchfeed | launchplain | offer | update | waitverified | restart |
      waitalloc | cancel | later | close | nosleep | preload | fastupdate |
      fastcancel | shot:<name> | sleep:<s> | waitphase:<Phase> |
      answerfirstrun | card:<name> | waithold
    `preload`, `fastupdate` and `fastcancel` press through the probe's own class
    loaded once into this process: a ui-probe run costs seconds on the guest,
    mostly compiling that class, and a Cancel has to land inside a download of a
    few seconds (W13).

    Writes updater-keys-<Tag>.txt (ends with the line `keys end`) and the shots
    in -GuestHome\shots.
#>
param(
  [string]$Plan = 'offer,update',
  [string]$Tag = 'run',
  [int]$WaitSeconds = 600,
  [string]$GuestHome = 'C:\folio-vm',
  [string]$FeedUrl = 'file:///C:/feed/'
)
$ErrorActionPreference = 'Continue'
$install = Join-Path $GuestHome 'folio'
$exe = Join-Path $install 'folio.exe'
$journalPath = Join-Path $install '.folio-update\journal.json'
$probe = Join-Path $PSScriptRoot 'ui-probe.ps1'
$log = Join-Path $GuestHome "updater-keys-$Tag.txt"
$shots = Join-Path $GuestHome 'shots'
[IO.Directory]::CreateDirectory($shots) | Out-Null
$diag = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio\diagnostics.log'

function Now { (Get-Date).ToString('HH:mm:ss.fff') }
function Say($s) { Add-Content -LiteralPath $log -Value "$(Now) $s" -Encoding UTF8 }
# The phase, read with every sharing flag and closed at once (watch.ps1 says why that matters).
function Phase {
  try {
    $fs = [IO.File]::Open($journalPath, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
    try { $t = (New-Object IO.StreamReader($fs)).ReadToEnd() } finally { $fs.Dispose() }
    $m = [regex]::Match($t, '"body"\s*:\s*\{\s*"phase"\s*:\s*\{\s*"phase"\s*:\s*"(\w+)"')
    if ($m.Success) { return $m.Groups[1].Value } else { return '?' }
  } catch { return $null }
}
# Any folio.exe with a window: the installed one, or (E1-b) the rescue copy
# in the transaction's rescue folder that an exit guard opened. The installed one first.
function WindowPid {
  $p = @(Get-Process folio -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } |
      Sort-Object @{ Expression = { $_.Path -ne $exe } })
  if ($p.Count -gt 0) { return $p[0].Id } else { return $null }
}
# **Folio's own window, for the close** (cleanvm-harness pitfall 3): `Process.CloseMainWindow()`
# picks winit's thread event-target popup, whose procedure destroys it and closes nothing. The
# close is posted to the process's visible window of winit's application class.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class KeysWindows {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder text, int max);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint message, IntPtr w, IntPtr l);
  public const uint WM_CLOSE = 0x0010;
  public static IntPtr AppWindowOf(uint target) {
    IntPtr best = IntPtr.Zero; long bestArea = 0;
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid != target || !IsWindowVisible(h)) { return true; }
      var name = new StringBuilder(128); GetClassName(h, name, 128);
      RECT c; GetClientRect(h, out c);
      long area = (long)(c.R - c.L) * (c.B - c.T);
      if (name.ToString() == "Window Class" && area > bestArea) { bestArea = area; best = h; }
      return true;
    }, IntPtr.Zero);
    return best;
  }
}
'@
function CloseFolio {
  $w = WindowPid
  if (-not $w) { Say 'close: no Folio window'; return }
  $h = [KeysWindows]::AppWindowOf([uint32]$w)
  if ($h -eq [IntPtr]::Zero) { Say "close: pid=$w has no window of winit's application class"; return }
  $ok = [KeysWindows]::PostMessage($h, [KeysWindows]::WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
  $p = Get-Process -Id $w -ErrorAction SilentlyContinue
  # bounded: Folio's own close budget is seconds; 30 s is load-row's bound for the same close
  $gone = (-not $p) -or $p.WaitForExit(30000)
  Say "WM_CLOSE posted to hwnd=$h of pid=$w -> $ok; exited within 30 s: $gone"
}
function WaitWindow {
  $deadline = (Get-Date).AddSeconds($WaitSeconds)
  while ((Get-Date) -lt $deadline) { $w = WindowPid; if ($w) { return $w }; Start-Sleep -Milliseconds 500 }
  Say "no Folio window within $WaitSeconds s"; return $null
}
function Probe([string[]]$a) {
  # A foreground refusal sent nothing (the probe checks before it sends), so asking again is safe.
  for ($try = 1; $try -le 6; $try++) {
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $probe @a 2>&1 | Out-String
    $refused = $out -match 'did not take foreground'
    Say ("ui-probe " + ($a -join ' ') + " (try $try) -> " + $(if ($refused) { 'REFUSED: no foreground' } else { $out.Trim() }))
    if (-not $refused) { return }
    if ($a[0] -ne 'click' -and $a.Count -ge 3) {
      $cl = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $probe click -ProcId $a[2] -X 60 -Y -60 2>&1 | Out-String
      Say ("  focus click -> " + $cl.Trim())
    }
    Start-Sleep -Milliseconds 1500
  }
}
function Shot($name) { $w = WindowPid; if ($w) { Probe @('capture', '-ProcId', "$w", '-Out', (Join-Path $shots "$Tag-$name.png")) } else { Say "shot ${name}: no window" } }
function WaitPhase($want) {
  $deadline = (Get-Date).AddSeconds($WaitSeconds)
  while ((Get-Date) -lt $deadline) { if ((Phase) -eq $want) { Say "phase $want seen"; return $true }; Start-Sleep -Milliseconds 100 }
  Say "phase $want not seen within $WaitSeconds s (now $(Phase))"; return $false
}

$script:probeLoaded = $false
function Load-Probe {
  if ($script:probeLoaded) { return }
  $src = [IO.File]::ReadAllText($probe)
  $m = [regex]::Match($src, "(?s)`nAdd-Type @'`r?`n(.*?)`r?`n'@")
  Add-Type -TypeDefinition $m.Groups[1].Value
  [Probe]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null
  $script:probeLoaded = $true
}
function Fast($keys) {
  Load-Probe
  $w = WindowPid
  $h = [Probe]::AppWindow([uint32]$w)
  for ($try = 1; $try -le 4; $try++) {
    if ([Probe]::BringToFront($h)) { break }
    Say "  fast: no foreground (try $try); focus click"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $probe click -ProcId $w -X 60 -Y -60 2>&1 | Out-Null
  }
  if ([Probe]::GetForegroundWindow() -ne $h) { Say '  fast: REFUSED, Folio is not foreground; nothing sent'; return }
  foreach ($k in $keys) {
    $n = try { switch ($k) { 'Tab' { [Probe]::TapVk([uint16]9) } 'Enter' { [Probe]::TapVk([uint16]13) } 'ShiftTab' { [Probe]::Chord($false, $true, $false, [uint16]9) } 'Escape' { [Probe]::TapVk([uint16]27) } } } catch { "ERR $($_.Exception.Message)" }
    Say "  fast: $k sent ($n events, foreground verified)"
  }
}

Say "keys start plan=$Plan"
foreach ($step in ($Plan -split ',')) {
  $step = $step.Trim()
  Say "step $step"
  switch -Regex ($step) {
    '^launchfeed$' { $p = Start-Process -FilePath $exe -ArgumentList @('--update-feed', $FeedUrl) -PassThru; Say "started pid=$($p.Id) with --update-feed $FeedUrl" }
    '^launchplain$' { $p = Start-Process -FilePath $exe -PassThru; Say "started pid=$($p.Id) plain" }
    '^offer$' {
      $w = WaitWindow; Say "window pid=$w"
      $deadline = (Get-Date).AddSeconds($WaitSeconds)
      while ((Get-Date) -lt $deadline) {
        if ((Test-Path -LiteralPath $diag) -and (Select-String -LiteralPath $diag -Pattern 'is offered' -SimpleMatch -Quiet)) { break }
        Start-Sleep -Milliseconds 500
      }
      $lines = @(Select-String -LiteralPath $diag -Pattern 'update' -ErrorAction SilentlyContinue | ForEach-Object { $_.Line })
      Say "diagnostics update lines: $($lines -join ' || ')"
      Start-Sleep -Seconds 3
      Shot 'offer-card'
    }
    '^update$' { $w = WindowPid; Probe @('chord', '-ProcId', "$w", '-Mods', 's', '-Name', 'Tab'); Start-Sleep -Milliseconds 700; Shot 'offer-ring'; Probe @('key', '-ProcId', "$w", '-Name', 'Enter') }
    '^waitverified$' { [void](WaitPhase 'Prepared'); Start-Sleep -Seconds 3; Shot 'restart-card' }
    '^restart$' { $w = WindowPid; Probe @('chord', '-ProcId', "$w", '-Mods', 's', '-Name', 'Tab'); Start-Sleep -Milliseconds 700; Shot 'restart-ring'; Probe @('key', '-ProcId', "$w", '-Name', 'Enter') }
    '^waitalloc$' { [void](WaitPhase 'Allocated') }
    '^fastupdate$' { Fast @('ShiftTab', 'Enter') }
    '^fastcancel$' { Fast @('Tab', 'Enter') }
    '^preload$' { Load-Probe; Say 'probe class loaded' }
    '^cancel$' { $w = WindowPid; Probe @('key', '-ProcId', "$w", '-Name', 'Tab'); Probe @('key', '-ProcId', "$w", '-Name', 'Enter') }
    '^later$' { $w = WindowPid; Probe @('key', '-ProcId', "$w", '-Name', 'Escape') }
    '^nosleep$' { & powercfg.exe /change monitor-timeout-ac 0; & powercfg.exe /change standby-timeout-ac 0; & powercfg.exe /change hibernate-timeout-ac 0; Say 'display, standby and hibernate timeouts off (guest power plan)' }
    '^close$' { CloseFolio }
    # E1-b's held rescue: the watcher's hold taken before Folio starts (bounded: the watcher polls
    # every 25 ms once its 1-2 s of startup are done; 60 s means it never will).
    '^waithold$' {
      $deadline = (Get-Date).AddSeconds(60); $held = $false
      while (-not $held -and (Get-Date) -lt $deadline) {
        $held = @(Get-ChildItem -LiteralPath $GuestHome -Filter 'updater-watch-*.txt' -File | Where-Object { Select-String -LiteralPath $_.FullName -Pattern 'RESCUE HOLD taken' -SimpleMatch -Quiet }).Count -gt 0
        if (-not $held) { Start-Sleep -Milliseconds 250 }
      }
      Say "rescue hold taken: $held"
    }
    # The first-run card answered before Folio's first start (cleanvm-harness pitfall 4): an
    # old-schema settings document the candidate's own migrations carry to `first_run_card: Shown`
    # (the same document load-row.ps1 places). A guest that holds one already is left as it is.
    '^answerfirstrun$' {
      $data = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Folio'
      $settings = Join-Path $data 'settings.json'
      if (Test-Path -LiteralPath $settings) { Say 'settings.json already present: left as it is' }
      else {
        [IO.Directory]::CreateDirectory($data) | Out-Null
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'first-run-settings.json') -Destination $settings
        Say 'first run answered: a schema-25 settings.json placed for the candidate to migrate'
      }
    }
    # The card Folio's window shows, read by the OCR (card.ps1), the capture beside the text.
    '^card:(.+)$' {
      $o = Join-Path $shots "$Tag-card-$($Matches[1])"
      $r = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'card.ps1') -Out $o 2>&1 | Out-String
      $t = if (Test-Path -LiteralPath "$o.txt") { (Get-Content -LiteralPath "$o.txt") -join ' | ' } else { "no text: $($r.Trim())" }
      Say "card $($Matches[1]): $t"
    }
    '^shot:(.+)$' { Shot $Matches[1] }
    '^sleep:(\d+)$' { Start-Sleep -Seconds ([int]$Matches[1]) }
    '^waitphase:(\w+)$' { [void](WaitPhase $Matches[1]) }
    default { Say "unknown step $step" }
  }
}
Say 'keys end'
