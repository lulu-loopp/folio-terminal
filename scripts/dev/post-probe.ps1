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
  [ValidateSet("list", "place", "left", "right", "menu", "type", "key", "shot", "wheel", "chord")]
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
  [string]$Out = "",
  # type: the characters to post. key: one of the names in $POSTED_KEYS.
  [string]$Text = "",
  [string]$Name = "",
  # chord: modifiers as any of c(trl) s(hift) a(lt); -Name is the base key, either
  # one of $POSTED_KEYS or the single character printed on it.
  [string]$Mods = "",
  # wheel: how many notches, and the WHEEL_DELTA each one carries (negative
  # scrolls down, which is Win32's own sign).
  [int]$Steps = 3,
  [int]$Delta = -120
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
  [DllImport("user32.dll")] public static extern short VkKeyScanW(char c);
  [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint type);
  /* WM_KEYDOWN, then the WM_CHAR winit peeks for while it is handling that
     keydown, then WM_KEYUP. The WM_CHAR is what carries the *character*: a
     posted key cannot move the real keyboard state, so the ToUnicode path
     would answer for a Shift nobody is holding and every capital would arrive
     lowered. The pair is therefore not a belt-and-braces — the second half is
     the half that is read. */
  static IntPtr Down(ushort vk) { return (IntPtr)(1 | (int)(MapVirtualKeyW(vk, 0) << 16)); }
  static IntPtr Up(ushort vk) { return (IntPtr)(unchecked((int)0xC0000001) | (int)(MapVirtualKeyW(vk, 0) << 16)); }
  public static void Type(IntPtr h, string text) {
    foreach (char c in text) {
      short scan = VkKeyScanW(c);
      ushort vk = (ushort)(scan & 0xFF);
      if (scan == -1) vk = 0;
      PostMessage(h, 0x0100, (IntPtr)vk, Down(vk));
      PostMessage(h, 0x0102, (IntPtr)c,  Down(vk));
      PostMessage(h, 0x0101, (IntPtr)vk, Up(vk));
    }
  }
  /* WM_MOUSEWHEEL carries SCREEN coordinates in its lParam, unlike every
     button message above — so the caller's client point is offset by the
     window's own rectangle here rather than at the call site. */
  public static void Wheel(IntPtr h, int x, int y, int delta) {
    PPRECT r; GetWindowRect(h, out r);
    Move(h, x, y);
    PostMessage(h, 0x020A, (IntPtr)(delta << 16), LP(r.L + x, r.T + y));
  }
  public static void Tap(IntPtr h, ushort vk) {
    PostMessage(h, 0x0100, (IntPtr)vk, Down(vk));
    PostMessage(h, 0x0101, (IntPtr)vk, Up(vk));
  }
  /* A chord, held the way a hand holds one: the modifiers go down first and come
     up last, because winit builds its `ModifiersState` from the key events it is
     handed and a base key posted between two of them is the only thing that
     carries the chord. WM_SYSKEYDOWN for the Alt-held pair, which is the message
     Windows itself would send. */
  public static void Chord(IntPtr h, bool ctrl, bool shift, bool alt, ushort vk) {
    uint down = alt ? 0x0104u : 0x0100u, up = alt ? 0x0105u : 0x0101u;
    if (ctrl)  PostMessage(h, 0x0100, (IntPtr)0x11, Down(0x11));
    if (shift) PostMessage(h, 0x0100, (IntPtr)0x10, Down(0x10));
    if (alt)   PostMessage(h, 0x0104, (IntPtr)0x12, Down(0x12));
    PostMessage(h, down, (IntPtr)vk, Down(vk));
    PostMessage(h, up,   (IntPtr)vk, Up(vk));
    if (alt)   PostMessage(h, 0x0105, (IntPtr)0x12, Up(0x12));
    if (shift) PostMessage(h, 0x0101, (IntPtr)0x10, Up(0x10));
    if (ctrl)  PostMessage(h, 0x0101, (IntPtr)0x11, Up(0x11));
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

$POSTED_KEYS = @{
  enter = 0x0D; tab = 0x09; esc = 0x1B; back = 0x08; space = 0x20
  up = 0x26; down = 0x28; left = 0x25; right = 0x27
  home = 0x24; end = 0x23; pageup = 0x21; pagedown = 0x22
  f1 = 0x70; f2 = 0x71; f3 = 0x72; f4 = 0x73; f5 = 0x74; f6 = 0x75
  f7 = 0x76; f8 = 0x77; f9 = 0x78; f10 = 0x79; f11 = 0x7A; f12 = 0x7B
}

switch ($Cmd) {
  "list" { Show-Windows }
  # Typing by posted message, for the desktop this file exists for — and for a
  # run beside somebody who is using their keyboard, where `ui-probe type` would
  # take the foreground and swallow what they are typing. Same narrowing as the
  # presses above: a posted key cannot reach another process's window, and it
  # says "the app answered these keys", not "a hand could type them".
  "type" { [PostProbe]::Type((Get-Target), $Text); "posted $($Text.Length) chars to window $Window" }
  "key" {
    $vk = $POSTED_KEYS[$Name]
    if (-not $vk) { throw "unknown key: $Name (known: $(($POSTED_KEYS.Keys | Sort-Object) -join ', '))" }
    [PostProbe]::Tap((Get-Target), [uint16]$vk)
    "posted $Name to window $Window"
  }
  "wheel" {
    $hwnd = Get-Target
    for ($i = 0; $i -lt $Steps; $i++) { [PostProbe]::Wheel($hwnd, $X, $Y, $Delta); Start-Sleep -Milliseconds 40 }
    "posted $Steps notches of $Delta at ($X,$Y) to window $Window"
  }
  "chord" {
    $base = if ($POSTED_KEYS.ContainsKey($Name)) { $POSTED_KEYS[$Name] } else { [PostProbe]::VkKeyScanW($Name[0]) -band 0xFF }
    [PostProbe]::Chord((Get-Target), ($Mods -match "c"), ($Mods -match "s"), ($Mods -match "a"), [uint16]$base)
    "posted $Mods+$Name to window $Window"
  }
  "shot" {
    if ($Out -eq "") { throw "shot needs -Out" }
    Save-WindowShot (Get-Target) $Out
  }
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
