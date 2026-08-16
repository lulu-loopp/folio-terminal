# menu-run.ps1 -- open an Explorer window on a known folder, right-click the folder background,
# photograph the Win11 primary context menu, then open "Show more options" and photograph the
# classic menu. The question this answers is which of the two menus a classic `shell\<verb>`
# registration lands in on Windows 11.
$sp = "C:\Users\Weiyi\AppData\Local\Temp\claude\D--Developer-BetterTerminal\8c51127a-6f8c-4b17-b04c-caef6a7f83fe\scratchpad\win-landing"
$target = "$sp\ctx-target"
New-Item -ItemType Directory -Force -Path $target | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class M {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,IntPtr e);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
 public delegate bool EnumProc(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 // FindWindowW("CabinetWClass", null) returned 0 here even with the window plainly present,
 // so the window is located by its title instead -- which also picks the RIGHT explorer
 // window when the user already has others open.
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
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
}
"@
[void][M]::SetProcessDpiAwarenessContext([IntPtr](-4))
function Shot($n,$x,$y,$w,$h){
  $b=New-Object System.Drawing.Bitmap($w,$h);$g=[System.Drawing.Graphics]::FromImage($b)
  $g.CopyFromScreen($x,$y,0,0,(New-Object System.Drawing.Size($w,$h)));$g.Dispose()
  $b.Save("$sp\$n",[System.Drawing.Imaging.ImageFormat]::Png);$b.Dispose(); "saved $n"
}
function RClick($x,$y){
  [void][M]::SetCursorPos($x,$y); Start-Sleep -Milliseconds 250
  [M]::mouse_event(0x0008,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 80   # RIGHTDOWN
  [M]::mouse_event(0x0010,0,0,0,[IntPtr]::Zero)                                 # RIGHTUP
}
function LClick($x,$y){
  [void][M]::SetCursorPos($x,$y); Start-Sleep -Milliseconds 250
  [M]::mouse_event(0x0002,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 80
  [M]::mouse_event(0x0004,0,0,0,[IntPtr]::Zero)
}

Start-Process explorer.exe $target
Start-Sleep -Seconds 3
# Bring the freshly opened window forward and find where it is.
$h = [M]::ByTitle("ctx-target")
[void][M]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 800
$r = New-Object M+RECT; [void][M]::GetWindowRect($h,[ref]$r)
Write-Output "explorer hwnd=$h rect=$($r.L),$($r.T)-$($r.R),$($r.B)"

# Right-click well inside the (empty) file list area.
$cx = $r.L + [int](($r.R - $r.L) * 0.55)
$cy = $r.T + [int](($r.B - $r.T) * 0.40)
RClick $cx $cy
Start-Sleep -Milliseconds 1400
Shot "shot-06-ctx-primary.png" ($cx - 80) ($cy - 80) 950 800

Write-Output "primary menu captured at click point $cx,$cy"

# "Show more options" sits at the foot of the Win11 menu; clicking it opens the classic
# (Windows 10) menu, which is where legacy `shell\<verb>` registrations that did not get
# promoted are supposed to appear.
$smoX = $cx + 206
$smoY = $cy + 664
LClick $smoX $smoY
Start-Sleep -Milliseconds 1600
Shot "shot-07-ctx-classic.png" ($smoX - 120) ($smoY - 700) 950 900
Write-Output "clicked Show more options at $smoX,$smoY"

# Finally invoke the verb itself, so the probe's own log records what %V expanded to.
# "Open in Folio here" sits 486px below the top of the classic menu capture.
$itemX = $smoX + 120
$itemY = $smoY - 700 + 486
# Photograph the strip the cursor is about to land on, so a miss is diagnosable rather
# than silent -- the first attempt clicked into empty space and nothing was logged.
[void][M]::SetCursorPos($itemX, $itemY); Start-Sleep -Milliseconds 500
Shot "shot-08-ctx-hover.png" ($itemX - 400) ($itemY - 60) 900 120
LClick $itemX $itemY
Write-Output "clicked 'Open in Folio here' at $itemX,$itemY"
Start-Sleep -Seconds 2
Write-Output "--- shell-probe.log ---"
Get-Content "$sp\probes\target\debug\shell-probe.log" -ErrorAction SilentlyContinue | Select-Object -Last 6
