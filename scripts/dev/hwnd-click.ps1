# hwnd-click.ps1 — click (or photograph) ONE named window of a multi-window
# process. ui-probe.ps1 addresses "the process's largest window", which cannot
# tell two same-sized Folio windows apart; this takes the HWND itself.
#
#   .\hwnd-click.ps1 -Hwnd <h> -ProcId <pid> -X 578 -Y 40
#   .\hwnd-click.ps1 -Hwnd <h> -ProcId <pid> -Shot out.png
param(
  [Parameter(Mandatory = $true)][long]$Hwnd,
  [Parameter(Mandatory = $true)][int]$ProcId,
  [int]$X = 0,
  [int]$Y = 0,
  [string]$Shot = ""
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Runtime.InteropServices;
[StructLayout(LayoutKind.Sequential)]
public struct PRECT3 { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)]
public struct WPOINT3 { public int X, Y; }
[StructLayout(LayoutKind.Sequential)]
public struct MOUSEINPUT3 { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
[StructLayout(LayoutKind.Explicit)]
public struct INPUTUNION3 { [FieldOffset(0)] public MOUSEINPUT3 mi; [FieldOffset(0)] public long p1; [FieldOffset(8)] public long p2; [FieldOffset(16)] public long p3; [FieldOffset(24)] public long p4; }
[StructLayout(LayoutKind.Sequential)]
public struct INPUT3 { public uint type; public INPUTUNION3 u; }
public class Click3 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out PRECT3 r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(WPOINT3 p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT3[] i, int size);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  public const uint MOUSEEVENTF_LEFTDOWN = 0x0002, MOUSEEVENTF_LEFTUP = 0x0004;
  public static bool Owns(int x, int y, int wantPid) {
    WPOINT3 p; p.X = x; p.Y = y;
    IntPtr h = WindowFromPoint(p);
    if (h == IntPtr.Zero) return false;
    uint owner; GetWindowThreadProcessId(h, out owner);
    return owner == (uint)wantPid;
  }
  public static uint LeftClick() {
    INPUT3[] i = new INPUT3[2];
    i[0].type = 0; i[0].u.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
    i[1].type = 0; i[1].u.mi.dwFlags = MOUSEEVENTF_LEFTUP;
    return SendInput(2, i, Marshal.SizeOf(typeof(INPUT3)));
  }
}
'@

[Click3]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

$r = New-Object PRECT3
[Click3]::GetWindowRect([IntPtr]$Hwnd, [ref]$r) | Out-Null

if ($Shot) {
  Add-Type -AssemblyName System.Drawing
  $bmp = New-Object System.Drawing.Bitmap(($r.R - $r.L), ($r.B - $r.T))
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($Shot); $g.Dispose(); $bmp.Dispose()
  "shot $($r.R - $r.L)x$($r.B - $r.T) of hwnd=$Hwnd -> $Shot"
  return
}

$px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
$py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
# The pixel-ownership law: a click lands on whoever owns that screen pixel.
if (-not [Click3]::Owns($px, $py, $ProcId)) {
  throw "REFUSED: ($px,$py) is not owned by pid=$ProcId — not clicking blind"
}
[Click3]::SetCursorPos($px, $py) | Out-Null
Start-Sleep -Milliseconds 120
$sent = [Click3]::LeftClick()
if ($sent -eq 0) { throw "SendInput accepted 0 events — nothing was clicked" }
"clicked ($px,$py) = hwnd=$Hwnd +($X,$Y) ($sent events)"
