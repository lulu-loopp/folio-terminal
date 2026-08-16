# menu-invoke.ps1 -- open the CLASSIC context menu directly (Shift+F10 bypasses the Win11
# primary menu and its "Show more options" hop, which is where two runs of coordinate-chasing
# went wrong) and capture it. With -ClickY the same flow clicks that row, so the verb runs and
# shell-probe.log records what %V expanded to.
param([int]$ClickY = 0)

$sp = "C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\8c51127a-6f8c-4b17-b04c-caef6a7f83fe\scratchpad\win-landing"
$target = "$sp\ctx-target"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class K {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,IntPtr e);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
 public delegate bool EnumProc(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
 public static IntPtr ByTitle(string needle) {
   IntPtr found = IntPtr.Zero;
   EnumWindows((h,l) => {
     if (!IsWindowVisible(h)) return true;
     var t = new StringBuilder(300); GetWindowTextW(h,t,300);
     if (t.ToString().Contains(needle)) { found = h; return false; }
     return true;
   }, IntPtr.Zero);
   return found;
 }
}
"@
[void][K]::SetProcessDpiAwarenessContext([IntPtr](-4))
function Shot($n,$x,$y,$w,$h){
  $b=New-Object System.Drawing.Bitmap($w,$h);$g=[System.Drawing.Graphics]::FromImage($b)
  $g.CopyFromScreen($x,$y,0,0,(New-Object System.Drawing.Size($w,$h)));$g.Dispose()
  $b.Save("$sp\$n",[System.Drawing.Imaging.ImageFormat]::Png);$b.Dispose(); Write-Output "saved $n"
}

$h = [K]::ByTitle("ctx-target")
if ($h -eq [IntPtr]::Zero) { Start-Process explorer.exe $target; Start-Sleep -Seconds 3; $h = [K]::ByTitle("ctx-target") }
[void][K]::SetForegroundWindow($h); Start-Sleep -Milliseconds 900
$r = New-Object K+RECT; [void][K]::GetWindowRect($h,[ref]$r)

# Left-click empty list space first: Shift+F10 acts on the focused pane, and without this the
# focus may still be on the address bar or the tree.
$cx = $r.L + [int](($r.R - $r.L) * 0.45)
$cy = $r.T + [int](($r.B - $r.T) * 0.45)
[void][K]::SetCursorPos($cx,$cy); Start-Sleep -Milliseconds 250
[K]::mouse_event(0x0002,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 70
[K]::mouse_event(0x0004,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 600
Write-Output "explorer rect=$($r.L),$($r.T)-$($r.R),$($r.B)  anchor=$cx,$cy"

# Shift+F10 = the classic menu, opened at the current selection/cursor.
[K]::keybd_event(0x10,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 60      # SHIFT down
[K]::keybd_event(0x79,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 60      # F10 down
[K]::keybd_event(0x79,0,2,[IntPtr]::Zero); Start-Sleep -Milliseconds 40      # F10 up
[K]::keybd_event(0x10,0,2,[IntPtr]::Zero)                                    # SHIFT up
Start-Sleep -Milliseconds 1500

Shot "shot-09-classic-menu.png" $cx ($cy - 40) 700 1000

if ($ClickY -gt 0) {
    $ix = $cx + 200
    $iy = $cy - 40 + $ClickY
    [void][K]::SetCursorPos($ix,$iy); Start-Sleep -Milliseconds 400
    Shot "shot-10-hover.png" ($ix - 300) ($iy - 30) 700 60
    [K]::mouse_event(0x0002,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 80
    [K]::mouse_event(0x0004,0,0,0,[IntPtr]::Zero)
    Write-Output "clicked row at $ix,$iy"
    Start-Sleep -Seconds 2
    Write-Output "--- shell-probe.log tail ---"
    Get-Content "$sp\probes\target\debug\shell-probe.log" -ErrorAction SilentlyContinue | Select-String "ARGV" | Select-Object -Last 4
}
