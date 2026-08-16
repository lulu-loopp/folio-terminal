# screen.ps1 -- full virtual-screen capture, per-monitor DPI aware, for probes whose subject
# (a toast, a taskbar button) is not inside any window we own.
#
#   .\screen.ps1 -Out shot.png                      full virtual screen
#   .\screen.ps1 -Out shot.png -X 1000 -Y 600 -W 900 -H 500   a region in physical px
#   .\screen.ps1 -Click -X 1700 -Y 900              move the cursor there and left-click
#   .\screen.ps1 -Metrics                           print virtual screen bounds
param(
    [string]$Out,
    [int]$X = -1,
    [int]$Y = -1,
    [int]$W = 0,
    [int]$H = 0,
    [switch]$Click,
    [switch]$Metrics
)

Add-Type -AssemblyName System.Drawing, System.Windows.Forms

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Nat {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
"@

# PER_MONITOR_AWARE_V2 = -4. Without this the capture is stretched on a scaled display and
# every coordinate we compute is a lie (the mouse-offset bug from the ui-probe history).
[void][Nat]::SetProcessDpiAwarenessContext([IntPtr](-4))

# SM_XVIRTUALSCREEN 76, SM_YVIRTUALSCREEN 77, SM_CXVIRTUALSCREEN 78, SM_CYVIRTUALSCREEN 79
$vx = [Nat]::GetSystemMetrics(76)
$vy = [Nat]::GetSystemMetrics(77)
$vw = [Nat]::GetSystemMetrics(78)
$vh = [Nat]::GetSystemMetrics(79)

if ($Metrics) {
    Write-Output "virtual screen: x=$vx y=$vy w=$vw h=$vh"
    exit 0
}

if ($Click) {
    [void][Nat]::SetCursorPos($X, $Y)
    Start-Sleep -Milliseconds 120
    [Nat]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 60
    [Nat]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
    Write-Output "clicked at $X,$Y"
    exit 0
}

if ($X -lt 0) { $X = $vx }
if ($Y -lt 0) { $Y = $vy }
if ($W -le 0) { $W = $vw }
if ($H -le 0) { $H = $vh }

$bmp = New-Object System.Drawing.Bitmap($W, $H)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size($W, $H)))
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $Out ($W x $H at $X,$Y)"
