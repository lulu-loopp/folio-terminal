# taskbar-run.ps1 -- run taskbar-probe and photograph its taskbar button through every
# ITaskbarList3 progress state.
#
# This machine's taskbar is set to auto-hide: Shell_TrayWnd reports rect 0,1798-2880,1894 on a
# 2880x1800 primary, i.e. parked one row below the screen. Capturing that rect gets pure black.
# So the cursor is parked on the bottom edge to hold the taskbar revealed, and the capture is
# taken from the revealed position (bottom 96px of the primary monitor) instead.
$sp = "C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\8c51127a-6f8c-4b17-b04c-caef6a7f83fe\scratchpad\win-landing"
$exe = "$sp\probes\target\debug\taskbar-probe.exe"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class TB {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
}
"@
[void][TB]::SetProcessDpiAwarenessContext([IntPtr](-4))

New-Item -ItemType Directory -Force -Path "$sp\taskbar-frames" | Out-Null
Get-ChildItem "$sp\taskbar-frames" -Filter *.png -ErrorAction SilentlyContinue | Remove-Item

$out = "$sp\taskbar-probe-log.txt"
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $out -WindowStyle Minimized

# Revealed taskbar occupies the bottom 96px of the primary monitor, centred icons.
$rx = 1050; $ry = 1704; $rw = 800; $rh = 96

for ($t = 0; $t -lt 22; $t++) {
    # Hold the pointer against the bottom edge so auto-hide keeps the bar up.
    [void][TB]::SetCursorPos(1440, 1799)
    Start-Sleep -Milliseconds 450
    $b = New-Object System.Drawing.Bitmap($rw, $rh)
    $g = [System.Drawing.Graphics]::FromImage($b)
    $g.CopyFromScreen($rx, $ry, 0, 0, (New-Object System.Drawing.Size($rw, $rh)))
    $g.Dispose()
    $b.Save(("{0}\taskbar-frames\t{1:d2}.png" -f $sp, $t), [System.Drawing.Imaging.ImageFormat]::Png)
    $b.Dispose()
    Start-Sleep -Milliseconds 550
}
$p.WaitForExit(15000) | Out-Null
Write-Output "--- probe stdout ---"
Get-Content $out
