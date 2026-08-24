# f2-drag-probe.ps1 — drive a drag that leaves its window, for multiwindow slice
# F2 (`docs/DESIGN.md` §2.12, `docs/plans/multiwindow-ef/plan.md` F2+F4).
#
# **Why the two probes already in this folder are not enough.** `ui-probe.ps1`
# addresses one window — `MainWindowHandle`, which is whichever the process
# happens to report — and refuses any point that is not over the target pid,
# which is the right law for a press and the wrong one for the gesture this slice
# is about: **a tear-out ends over no window of ours at all**, and a probe that
# cannot aim at the desktop cannot ask the question. `post-probe.ps1` addresses
# any window but presses only; it has no drag, and a posted drag is in one
# respect not the gesture — Windows' own hit-testing is skipped, and the whole of
# F2 is that the *window manager* decides whose glass the hand is over.
#
# So this script keeps the pixel-ownership law and states it per end of the
# travel, because the two ends mean different things:
#
# * the **press** must land on the window it names — the law exactly as
#   `ui-probe.ps1` states it;
# * the **release** must land on whatever the run says it should: another window
#   of this process (`-ToWindow`), or nothing of ours (`-ToDesktop`). Both are
#   checked against `WindowFromPoint`, which is the very question the application
#   asks itself, so the probe and the thing being probed agree about what "over"
#   means or the run fails.
#
# Real input throughout (`SetCursorPos` + `mouse_event`): a posted stream would
# answer "the app followed these numbers" when what is being asked is "the
# desktop put the hand there". If the cursor cannot be parked — the condition
# `post-probe.ps1`'s header records — this refuses rather than pretending.
#
#   .\f2-drag-probe.ps1 list    -ProcId <pid>
#   .\f2-drag-probe.ps1 place   -ProcId <pid> [-W 1100] [-H 820] [-Gap 40]
#   .\f2-drag-probe.ps1 shot    -ProcId <pid> -Out all.png        → the whole desktop
#   .\f2-drag-probe.ps1 drag    -ProcId <pid> -Window 0 -X .. -Y ..
#                               (-ToWindow 1 -X2 .. -Y2 .. | -ToDesktop -ScreenX .. -ScreenY ..)
#                               [-HoldMs 600] [-Steps 24] [-Out mid.png] [-ShotAtMs ..] [-Esc]
#
# X/Y are physical pixels from the named window's own top-left, which for these
# windows is also the client origin (the self-drawn frame makes them one
# rectangle). `-X2/-Y2` are read the same way against `-ToWindow`'s rectangle.
param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("list", "place", "shot", "drag")]
  [string]$Cmd,
  [Parameter(Mandatory = $true)][int]$ProcId,
  [int]$Window = 0,
  [int]$ToWindow = -1,
  [switch]$ToDesktop,
  [int]$X = 0,
  [int]$Y = 0,
  [int]$X2 = 0,
  [int]$Y2 = 0,
  [int]$ScreenX = 0,
  [int]$ScreenY = 0,
  [int]$Steps = 24,
  # How long to REST at the far end with the button still down. The spring
  # (§7.1.6k, and F2's own across a window boundary) is a frame clock: it matures
  # under a pointer sending no events at all, so a drag that arrives and releases
  # in one breath never reaches it.
  [int]$HoldMs = 0,
  [switch]$Esc,
  [int]$W = 1100,
  [int]$H = 820,
  [int]$X0 = 60,
  [int]$Y0 = 60,
  [int]$Gap = 40,
  [int]$ShotAtMs = 0,
  [string]$Out = ""
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public struct F2RECT { public int L,T,R,B; }
public struct F2POINT { public int X,Y; }
public class F2Probe {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out F2RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(F2POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out F2POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr extra);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
  public const uint SWP_NOACTIVATE = 0x0010;
  const uint LEFTDOWN = 0x0002, LEFTUP = 0x0004, KEYUP = 0x0002;
  /* Every visible top-level window this process owns that is big enough to be a
     real one — the compositor helper is a 26x26 square. */
  public static List<IntPtr> Windows(uint want) {
    var found = new List<IntPtr>();
    EnumWindows((h,p) => {
      uint o; GetWindowThreadProcessId(h, out o);
      if (o != want || !IsWindowVisible(h)) return true;
      F2RECT r; GetWindowRect(h, out r);
      if ((long)(r.R-r.L)*(r.B-r.T) >= 40000) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static F2RECT Rect(IntPtr h) { F2RECT r; GetWindowRect(h, out r); return r; }
  /* The application's own question, asked by the probe in the same words:
     `WindowFromPoint` then `GA_ROOT`. */
  public static IntPtr RootAt(int x, int y) {
    F2POINT p; p.X = x; p.Y = y;
    return GetAncestor(WindowFromPoint(p), 2);
  }
  /* Park the hand and prove it went. A desktop that will not take the cursor is
     a desktop this probe reports, never one it presses blind on. */
  public static void Park(int x, int y) {
    SetCursorPos(x, y);
    F2POINT at; GetCursorPos(out at);
    if (Math.Abs(at.X - x) > 1 || Math.Abs(at.Y - y) > 1)
      throw new Exception("SetCursorPos is inert: asked for (" + x + "," + y + "), the cursor is at (" + at.X + "," + at.Y + ")");
  }
  public static void Down() { mouse_event(LEFTDOWN, 0, 0, 0, IntPtr.Zero); }
  public static void Up()   { mouse_event(LEFTUP,   0, 0, 0, IntPtr.Zero); }
  public static void Escape() { keybd_event(0x1B, 0x01, 0, IntPtr.Zero); keybd_event(0x1B, 0x01, KEYUP, IntPtr.Zero); }
}
'@
[void][F2Probe]::SetProcessDpiAwarenessContext([IntPtr](-4))
Get-Process -Id $ProcId -ErrorAction Stop | Out-Null
$ws = [F2Probe]::Windows($ProcId)
if ($ws.Count -eq 0) { throw "process $ProcId has no visible window" }

# `$hwnd` and never `$w`: PowerShell's variables are case-insensitive, so a loop
# over `$w` silently overwrites the `-W` parameter (post-probe.ps1's own note).
function Show-Windows {
  $i = 0
  foreach ($hwnd in $ws) {
    $r = [F2Probe]::Rect($hwnd)
    "$i hwnd=$hwnd rect=$($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T)"
    $i++
  }
}

function Save-DesktopShot([string]$path) {
  $vx = [F2Probe]::GetSystemMetrics(76)
  $vy = [F2Probe]::GetSystemMetrics(77)
  $vw = [F2Probe]::GetSystemMetrics(78)
  $vh = [F2Probe]::GetSystemMetrics(79)
  $bmp = New-Object System.Drawing.Bitmap $vw, $vh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $dst = $g.GetHdc()
  $src = [F2Probe]::GetDC([IntPtr]::Zero)
  [void][F2Probe]::BitBlt($dst, 0, 0, $vw, $vh, $src, $vx, $vy, 0x00CC0020)
  [void][F2Probe]::ReleaseDC([IntPtr]::Zero, $src)
  $g.ReleaseHdc($dst)
  $g.Dispose()
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  "shot ${vw}x${vh} at ($vx,$vy) -> $path"
}

switch ($Cmd) {
  "list" { Show-Windows }
  "place" {
    # Raised so the camera can see them and **never activated**: taking the
    # foreground from whoever holds it is the one thing a probe must not do.
    $i = 0
    foreach ($hwnd in $ws) {
      [void][F2Probe]::SetWindowPos($hwnd, [IntPtr](-1), $X0 + $i * ($W + $Gap), $Y0, $W, $H, [F2Probe]::SWP_NOACTIVATE)
      $i++
    }
    Start-Sleep -Milliseconds 400
    Show-Windows
  }
  "shot" { if ($Out -eq "") { throw "-Out is required" }; Save-DesktopShot $Out }
  "drag" {
    if ($Window -ge $ws.Count) { throw "window index $Window of $($ws.Count)" }
    $from = $ws[$Window]
    $fr = [F2Probe]::Rect($from)
    $px = $fr.L + $X
    $py = $fr.T + $Y
    if ([F2Probe]::RootAt($px, $py) -ne $from) {
      throw "REFUSED: the press at ($px,$py) is not on window $Window — not dragging blind"
    }
    if ($ToDesktop) {
      $qx = $ScreenX
      $qy = $ScreenY
      $at = [F2Probe]::RootAt($qx, $qy)
      if ($ws -contains $at) {
        throw "REFUSED: ($qx,$qy) is over one of this process's windows, so it cannot ask what happens over none"
      }
    } else {
      if ($ToWindow -lt 0 -or $ToWindow -ge $ws.Count) { throw "-ToWindow index $ToWindow of $($ws.Count)" }
      $to = $ws[$ToWindow]
      $tr = [F2Probe]::Rect($to)
      $qx = $tr.L + $X2
      $qy = $tr.T + $Y2
      $at = [F2Probe]::RootAt($qx, $qy)
      if ($at -ne $to) {
        throw "REFUSED: ($qx,$qy) is on $at and not on window $ToWindow ($to) — the hand would not be where the run says"
      }
    }
    [F2Probe]::Park($px, $py)
    Start-Sleep -Milliseconds 120
    [F2Probe]::Down()
    Start-Sleep -Milliseconds 60
    for ($s = 1; $s -le $Steps; $s++) {
      $ix = [int]($px + ($qx - $px) * $s / $Steps)
      $iy = [int]($py + ($qy - $py) * $s / $Steps)
      [F2Probe]::Park($ix, $iy)
      Start-Sleep -Milliseconds 16
    }
    # **The rest, with the hand sending nothing at all.** This is the whole point
    # of `-HoldMs`: F2's spring is a clock the application states to the loop, so
    # a probe that keeps jiggling would be proving something else.
    $rested = 0
    while ($rested -lt $HoldMs) {
      Start-Sleep -Milliseconds 40
      $rested += 40
      if ($Out -ne "" -and $ShotAtMs -gt 0 -and $rested -ge $ShotAtMs) {
        Save-DesktopShot $Out
        $ShotAtMs = 0
      }
    }
    if ($Esc) {
      [F2Probe]::Escape()
      Start-Sleep -Milliseconds 120
      "escaped while still holding the button"
    }
    [F2Probe]::Up()
    Start-Sleep -Milliseconds 200
    "dragged ($px,$py) -> ($qx,$qy), rested ${HoldMs}ms" + $(if ($Esc) { ", cancelled" } else { "" })
  }
}
