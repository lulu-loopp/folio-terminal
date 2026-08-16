# toast-run.ps1 -- start toast-probe `show` and photograph the toast while it is on screen.
# A toast lives ~5s, which is shorter than a fresh PowerShell can start, so the capture loop
# has to be running before the toast is raised, not launched after it.
param([int]$Frames = 10, [int]$EveryMs = 600, [switch]$ClickToast, [int]$ClickFrame = 3)

$sp = "C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\8c51127a-6f8c-4b17-b04c-caef6a7f83fe\scratchpad\win-landing"
$exe = "$sp\probes\target\debug\toast-probe.exe"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class T { [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,IntPtr e); }
"@
[void][T]::SetProcessDpiAwarenessContext([IntPtr](-4))

# The toast pops at the bottom-right of the primary monitor (2880x1800 here).
$rx = 1880; $ry = 1380; $rw = 1000; $rh = 420

Remove-Item "$sp\probes\target\debug\toast-probe.log" -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $exe -ArgumentList "show", "40" -PassThru -WindowStyle Minimized
Write-Output "started toast-probe pid=$($p.Id)"

for ($i = 0; $i -lt $Frames; $i++) {
    $bmp = New-Object System.Drawing.Bitmap($rw, $rh)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($rx, $ry, 0, 0, (New-Object System.Drawing.Size($rw, $rh)))
    $g.Dispose()
    $bmp.Save(("{0}\toast-frame-{1:d2}.png" -f $sp, $i), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    if ($ClickToast -and $i -eq $ClickFrame) {
        # Click the middle of the toast body. Coordinates come from reading an earlier frame.
        [void][T]::SetCursorPos(2500, 1580)
        Start-Sleep -Milliseconds 150
        [T]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [T]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
        Write-Output "clicked toast body at 2500,1580 on frame $i"
    }
    Start-Sleep -Milliseconds $EveryMs
}
Write-Output "frames done; waiting for probe"
$p.WaitForExit(45000) | Out-Null
Write-Output "--- log ---"
Get-Content "$sp\probes\target\debug\toast-probe.log"
