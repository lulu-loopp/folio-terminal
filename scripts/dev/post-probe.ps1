# post-probe.ps1 — drive Folio's chrome by POSTING mouse messages to one named
# window, for the desktop where `ui-probe.ps1` cannot.
#
# **Why this exists** (multiwindow slice F1c, 2026-08-23). `ui-probe.ps1` injects
# real input: it parks the cursor with `SetCursorPos` and presses with
# `mouse_event`, and it refuses to press unless the target owns the pixel — the
# right law for input that goes to whoever the desktop says is under the
# pointer. On a machine several agents are working on at once that law can stop
# being satisfiable: measured on 2026-08-23, `SetCursorPos` was **inert** (five
# calls in a row, the cursor never moved off the pixel another session had left
# it on) and every `SetForegroundWindow` was denied, because the foreground
# belonged to the ghost window of somebody else's hung process. Nothing could be
# clicked and nothing could be typed.
#
# So this file presses by `PostMessage` to one HWND. It is **narrower** than the
# pixel-ownership law rather than a way around it: a posted message cannot reach
# another process's window, so a press can never land somewhere it was not aimed.
# What it gives up is faithfulness — Windows' own hit-testing and z-order are not
# consulted — so a result measured here says "the app answered this press", not
# "a hand could reach this control". Reach for `ui-probe.ps1` first; this is the
# fallback for a desktop that will not let it work.
#
# Two things had to be learned the hard way, and both are load-bearing:
#
# * **winit drops a `WM_MOUSEMOVE` whose position equals the last one it saw.**
#   A second press on the same control therefore arrives with no pointer at all
#   (`pointer=none` in `BT_MOUSE_TRACE`, and every hit test answers nothing). So
#   every press posts a move one pixel away first.
# * **A posted move makes winit call `TrackMouseEvent`, and the real cursor is
#   not over the window**, so Windows posts `WM_MOUSELEAVE` straight back and the
#   app's pointer goes away. For a control that is just pressed this does not
#   matter — the button messages are already queued ahead of it — but a menu
#   raised by §7.1.6e's chevron closes 150ms after the pointer leaves it, which
#   is faster than any camera in a second process. `menu` therefore re-posts the
#   move every 30ms, which is what a hand resting on a menu does anyway.
#
#   .\post-probe.ps1 list  -ProcId <pid>                       → the process's windows, in z-order
#   .\post-probe.ps1 place -ProcId <pid> [-W 1200] [-H 1000]   → side by side and raised, without activating
#   .\post-probe.ps1 left  -ProcId <pid> -X .. -Y .. [-Window 0]
#   .\post-probe.ps1 right -ProcId <pid> -X .. -Y ..           → the press that raises a context menu
#   .\post-probe.ps1 menu  -ProcId <pid> -X .. -Y .. -HoverX .. -HoverY ..
#                          [-PressX .. -PressY ..] [-Out shot.png]
#                                                              → open a menu, hold it open, photograph
#                                                                it, and press one of its rows
#
# X/Y are **client** pixels, which for this window are also window pixels: the
# self-drawn frame makes the client area the whole outer rectangle, so a
# coordinate read off a `ui-probe capture` is a coordinate this script can press.
param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("list", "place", "left", "right", "menu")]
  [string]$Cmd,
  [Parameter(Mandatory = $true)][int]$ProcId,
  [int]$Window = 0,
  [int]$X = 0,
  [int]$Y = 0,
  [int]$HoverX = 0,
  [int]$HoverY = 0,
  [int]$PressX = -1,
  [int]$PressY = -1,
  [int]$PumpMs = 1500,
  [int]$ShotAtMs = 500,
  [int]$W = 1200,
  [int]$H = 1000,
  [int]$X0 = 40,
  [int]$Y0 = 60,
  [int]$Gap = 20,
  [string]$Out = ""
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public struct PPRECT { public int L,T,R,B; }
public class PostProbe {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out PPRECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
  /* HWND_TOPMOST with SWP_NOACTIVATE: raised so the camera's own
     pixel-ownership check can pass, and never activated, because taking the
     foreground from whoever holds it is the one thing a probe must not do. */
  public const uint SWP_NOACTIVATE = 0x0010;
  /* Every visible top-level window the process owns that is big enough to be a
     real one — the compositor helper is a 26x26 square. */
  public static List<IntPtr> Windows(uint want) {
    var found = new List<IntPtr>();
    EnumWindows((h,p) => {
      uint o; GetWindowThreadProcessId(h, out o);
      if (o != want || !IsWindowVisible(h)) return true;
      PPRECT r; GetWindowRect(h, out r);
      if ((long)(r.R-r.L)*(r.B-r.T) >= 40000) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static PPRECT Rect(IntPtr h) { PPRECT r; GetWindowRect(h, out r); return r; }
  static IntPtr LP(int x, int y) { return (IntPtr)((y << 16) | (x & 0xFFFF)); }
  /* One pixel away and then the target: see this file's header for why the pair
     is not one message. */
  public static void Move(IntPtr h, int x, int y) {
    PostMessage(h, 0x0200, (IntPtr)0, LP(x-1,y));
    PostMessage(h, 0x0200, (IntPtr)0, LP(x,y));
  }
  public static void Press(IntPtr h, bool right, int x, int y) {
    Move(h, x, y);
    PostMessage(h, right ? 0x0204u : 0x0201u, right ? (IntPtr)2 : (IntPtr)1, LP(x,y));
    PostMessage(h, right ? 0x0205u : 0x0202u, (IntPtr)0, LP(x,y));
  }
}
'@
[void][PostProbe]::SetProcessDpiAwarenessContext([IntPtr](-4))
Get-Process -Id $ProcId -ErrorAction Stop | Out-Null
$ws = [PostProbe]::Windows($ProcId)
if ($ws.Count -eq 0) { throw "process $ProcId has no visible window" }

# **`$hwnd` and never `$w`**, which is `two-window-shot.ps1`'s own warning:
# PowerShell's variables are case-insensitive, so a loop over `$w` silently
# overwrites the `-W` parameter — measured here as a window asked to be 3738620
# pixels wide and clamped by Windows to 65535.
function Show-Windows {
  $i = 0
  foreach ($hwnd in $ws) {
    $r = [PostProbe]::Rect($hwnd)
    "$i hwnd=$hwnd rect=$($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T)"
    $i++
  }
}

function Get-Target {
  if ($Window -ge $ws.Count) { throw "window index $Window of $($ws.Count)" }
  return $ws[$Window]
}

function Save-WindowShot([IntPtr]$hwnd, [string]$path) {
  $r = [PostProbe]::Rect($hwnd)
  $wide = $r.R - $r.L
  $high = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $wide, $high
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $dst = $g.GetHdc()
  $src = [PostProbe]::GetDC([IntPtr]::Zero)
  [void][PostProbe]::BitBlt($dst, 0, 0, $wide, $high, $src, $r.L, $r.T, 0x00CC0020)
  [void][PostProbe]::ReleaseDC([IntPtr]::Zero, $src)
  $g.ReleaseHdc($dst)
  $g.Dispose()
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  "shot ${wide}x${high} -> $path"
}

switch ($Cmd) {
  "list" { Show-Windows }
  "place" {
    $i = 0
    foreach ($hwnd in $ws) {
      [void][PostProbe]::SetWindowPos($hwnd, [IntPtr](-1), $X0 + $i * ($W + $Gap), $Y0, $W, $H, [PostProbe]::SWP_NOACTIVATE)
      $i++
    }
    Start-Sleep -Milliseconds 400
    Show-Windows
  }
  "left"  { [PostProbe]::Press((Get-Target), $false, $X, $Y); "posted left at ($X,$Y) to window $Window" }
  "right" { [PostProbe]::Press((Get-Target), $true,  $X, $Y); "posted right at ($X,$Y) to window $Window" }
  "menu" {
    # `$hwnd` again, and here it would have been `-H`: see Show-Windows' note.
    $hwnd = Get-Target
    [PostProbe]::Press($hwnd, $false, $X, $Y)
    $elapsed = 0
    $shot = $false
    $pressed = $false
    while ($elapsed -lt $PumpMs) {
      [PostProbe]::Move($hwnd, $HoverX, $HoverY)
      Start-Sleep -Milliseconds 30
      $elapsed += 30
      if (-not $shot -and $elapsed -ge $ShotAtMs -and $Out -ne "") {
        Save-WindowShot $hwnd $Out
        $shot = $true
      }
      # After the photograph, so one run can both show the menu and spend it.
      if (-not $pressed -and $PressX -ge 0 -and $elapsed -ge ($ShotAtMs + 200)) {
        [PostProbe]::Press($hwnd, $false, $PressX, $PressY)
        $pressed = $true
        "pressed ($PressX,$PressY)"
      }
    }
  }
}
