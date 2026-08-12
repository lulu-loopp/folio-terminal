# mw-probe.ps1 — multi-window spike probe.
#
# Enumerates every top-level window a process owns (ui-probe's MainWindowHandle
# cannot see a second one), prints each HWND's rect and GetDpiForWindow, and
# captures the whole virtual desktop so both windows land in one image.
param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("list", "capture", "move", "close")]
  [string]$Cmd,
  [int]$ProcId = 0,
  [string]$Out = "$env:TEMP\mw-probe.png",
  [long]$Hwnd = 0,
  [int]$X = 0,
  [int]$Y = 0,
  [int]$W = 0,
  [int]$H = 0
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public struct MRECT { public int L, T, R, B; }
public static class MW {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lparam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder s, int max);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hwnd, StringBuilder s, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out MRECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out MRECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  public static List<IntPtr> TopLevel(uint pid) {
    var found = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint owner; GetWindowThreadProcessId(h, out owner);
      // Owned popups (tooltips, IME candidates) are excluded: a real top-level
      // window has no owner. GW_OWNER = 4.
      if (owner == pid && IsWindowVisible(h) && GetWindow(h, 4) == IntPtr.Zero) found.Add(h);
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static string Text(IntPtr h) { var s = new StringBuilder(512); GetWindowTextW(h, s, 512); return s.ToString(); }
  public static string Cls(IntPtr h) { var s = new StringBuilder(512); GetClassNameW(h, s, 512); return s.ToString(); }
}
'@ -ReferencedAssemblies System.Drawing, System.Collections

# Physical pixels everywhere, on every monitor, whatever their scaling.
[MW]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null

switch ($Cmd) {
  "list" {
    if ($ProcId -eq 0) { throw "-ProcId required" }
    $windows = [MW]::TopLevel([uint32]$ProcId)
    foreach ($h in $windows) {
      $r = New-Object MRECT; [MW]::GetWindowRect($h, [ref]$r) | Out-Null
      $c = New-Object MRECT; [MW]::GetClientRect($h, [ref]$c) | Out-Null
      $dpi = [MW]::GetDpiForWindow($h)
      $scale = [math]::Round($dpi / 96.0, 4)
      "hwnd=0x{0:X} title='{1}' class={2} rect={3},{4},{5},{6} outer={7}x{8} client={9}x{10} dpi={11} scale={12}" -f `
        [int64]$h, [MW]::Text($h), [MW]::Cls($h), $r.L, $r.T, $r.R, $r.B, ($r.R - $r.L), ($r.B - $r.T), ($c.R - $c.L), ($c.B - $c.T), $dpi, $scale
    }
    "count=$($windows.Count)"
  }
  "move" {
    if ($Hwnd -eq 0) { throw "-Hwnd required" }
    # SWP_NOZORDER|SWP_NOACTIVATE = 0x4 | 0x10
    $flags = 0x4 -bor 0x10
    if ($W -eq 0 -or $H -eq 0) { $flags = $flags -bor 0x1 }  # SWP_NOSIZE
    [MW]::SetWindowPos([IntPtr]::new($Hwnd), [IntPtr]::Zero, $X, $Y, $W, $H, [uint32]$flags) | Out-Null
    "moved 0x{0:X} to {1},{2} {3}x{4}" -f $Hwnd, $X, $Y, $W, $H
  }
  "close" {
    if ($Hwnd -eq 0) { throw "-Hwnd required" }
    [MW]::SendMessageW([IntPtr]::new($Hwnd), 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null   # WM_CLOSE
    "closed 0x{0:X}" -f $Hwnd
  }
  "capture" {
    Add-Type -AssemblyName System.Drawing
    # The whole virtual desktop: both windows, wherever they are, at 1:1.
    $vx = [MW]::GetSystemMetrics(76); $vy = [MW]::GetSystemMetrics(77)
    $vw = [MW]::GetSystemMetrics(78); $vh = [MW]::GetSystemMetrics(79)
    $bmp = New-Object System.Drawing.Bitmap($vw, $vh)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($vx, $vy, 0, 0, (New-Object System.Drawing.Size($vw, $vh)))
    $g.Dispose()
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    "captured virtual={0},{1} {2}x{3} -> {4}" -f $vx, $vy, $vw, $vh, $Out
  }
}
