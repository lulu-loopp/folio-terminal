# two-window-shot.ps1 — place a process's two Folio windows side by side and
# photograph both in one frame.
#
# NOTE: `$h` is `$H` in PowerShell — the loop variable is `$hwnd` so that a
# window handle can never be passed where a height belongs.
#
# ui-probe.ps1's own `capture` aims at the process's LARGEST window, which with
# two open windows is a picture of one of them. This is the same camera with the
# same pixel-ownership law, pointed at the union of both rectangles.
#
#   .\two-window-shot.ps1 -ProcId <pid> -Place        → side by side, no activation
#   .\two-window-shot.ps1 -ProcId <pid> -Out shot.png → capture the union
param(
  [Parameter(Mandatory = $true)][int]$ProcId,
  [string]$Out = "$env:TEMP\two-window-shot.png",
  [switch]$Place,
  [int]$W = 900,
  [int]$H = 700,
  [int]$X0 = 40,
  [int]$Y0 = 60,
  [int]$Gap = 20
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public struct PRECT2 { public int L, T, R, B; }
public class Shot {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out PRECT2 r);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr param);
  public delegate bool EnumProc(IntPtr h, IntPtr param);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(WPOINT2 p);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [StructLayout(LayoutKind.Sequential)] public struct WPOINT2 { public int X, Y; }
  public const uint SWP_NOACTIVATE = 0x0010;
  public const uint SWP_NOZORDER = 0x0004;
  /* Every visible top-level window the process owns whose area is large enough
     to be a real window — the compositor helper is a 26x26 square, and the
     windows a person is looking at are not. */
  public static List<IntPtr> AppWindows(uint targetPid, long minArea) {
    List<IntPtr> found = new List<IntPtr>();
    EnumWindows((h, p) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner != targetPid || !IsWindowVisible(h)) return true;
      PRECT2 r; if (!GetWindowRect(h, out r)) return true;
      long area = (long)(r.R - r.L) * (r.B - r.T);
      if (area >= minArea) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static bool PointBelongsTo(int x, int y, int wantPid) {
    WPOINT2 p; p.X = x; p.Y = y;
    IntPtr h = WindowFromPoint(p);
    if (h == IntPtr.Zero) return false;
    uint owner; GetWindowThreadProcessId(h, out owner);
    return owner == (uint)wantPid;
  }
}
'@

# Physical pixels, 1:1, exactly as ui-probe.ps1 does it.
[Shot]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

$windows = [Shot]::AppWindows([uint32]$ProcId, 100000)
if ($windows.Count -lt 2) { throw "pid=$ProcId has $($windows.Count) real window(s); this shot is about two" }

if ($Place) {
  # **No activation.** The user's foreground is theirs; a probe that steals it
  # to take a picture has already broken the run it is documenting.
  $i = 0
  foreach ($hwnd in $windows) {
    $x = $X0 + $i * ($W + $Gap)
    [Shot]::SetWindowPos($hwnd, [IntPtr]::Zero, $x, $Y0, $W, $H, ([Shot]::SWP_NOACTIVATE -bor [Shot]::SWP_NOZORDER)) | Out-Null
    $i++
  }
  Start-Sleep -Milliseconds 700
}

# `$R` and `$r` are one variable in PowerShell — the accumulators are spelled
# out so that the union cannot be clobbered by the rectangle being read into.
$unionL = [int]::MaxValue; $unionT = [int]::MaxValue
$unionR = [int]::MinValue; $unionB = [int]::MinValue
foreach ($hwnd in $windows) {
  $r = New-Object PRECT2
  [Shot]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $unionL = [Math]::Min($unionL, $r.L); $unionT = [Math]::Min($unionT, $r.T)
  $unionR = [Math]::Max($unionR, $r.R); $unionB = [Math]::Max($unionB, $r.B)
  # The pixel-ownership law: each window's own centre must belong to the process
  # under test, or the picture is of whatever is standing over it.
  $cx = [int](($r.L + $r.R) / 2); $cy = [int](($r.T + $r.B) / 2)
  if (-not [Shot]::PointBelongsTo($cx, $cy, $ProcId)) {
    throw "REFUSED: ($cx,$cy) is not owned by pid=$ProcId — something is standing over a window under test"
  }
  "window $hwnd rect=($($r.L),$($r.T))-($($r.R),$($r.B))"
}

Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap(($unionR - $unionL), ($unionB - $unionT))
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($unionL, $unionT, 0, 0, $bmp.Size)
$bmp.Save($Out); $g.Dispose(); $bmp.Dispose()
"captured $($unionR - $unionL)x$($unionB - $unionT) of $($windows.Count) windows -> $Out"
