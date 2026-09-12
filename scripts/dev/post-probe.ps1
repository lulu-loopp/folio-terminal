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
# Two more, learned by measuring a pan with `drag` on 2026-09-10:
#
# * **A drag is a run of moves and never one jump.** `-Steps 1` posts a single
#   move to the far end, and what came out was a fraction of the distance asked
#   for; eight to fifteen steps came out exact, four times running, on both
#   axes. So measure a gesture with the steps a hand would have made.
# * **Park the real cursor outside the target window.** Windows keeps sending
#   that window real `WM_MOUSEMOVE` messages while the pointer rests over it,
#   and they interleave with the posted ones — a pan then measures its delta
#   against wherever the hand actually is and the result is noise. `place` the
#   window somewhere the cursor is not before driving it.
#
#   .\post-probe.ps1 list  -ProcId <pid>                       → the process's windows, in z-order
#   .\post-probe.ps1 place -ProcId <pid> [-W 1200] [-H 1000]   → side by side and raised, without activating
#   .\post-probe.ps1 left  -ProcId <pid> -X .. -Y .. [-Window 0]
#   .\post-probe.ps1 right -ProcId <pid> -X .. -Y ..           → the press that raises a context menu
#   .\post-probe.ps1 drag  -ProcId <pid> -X .. -Y .. -ToX .. -ToY .. [-Steps 8]
#                                                              → press, travel, release: the gesture a
#                                                                click cannot stand in for
#   .\post-probe.ps1 menu  -ProcId <pid> -X .. -Y .. -HoverX .. -HoverY ..
#                          [-PressX .. -PressY ..] [-Out shot.png]
#                                                              → open a menu, hold it open, photograph
#                                                                it, and press one of its rows
#   .\post-probe.ps1 wheel -ProcId <pid> -X .. -Y .. [-Steps 3] [-Delta -120] [-Mods a]
#                                                              → notches over a point, with the
#                                                                modifiers a hand would be holding
#   .\post-probe.ps1 sizemove -ProcId <pid> -Name enter|exit    → the OS's modal move/size loop,
#                                                                opened and closed by hand
#
# **`sizemove`, and the one thing it is for.** A window dragged between two
# monitors of different scale does not merely change DPI: every message of that
# change arrives *inside* the modal move/size loop, and §7.50 defers the
# expensive half of the answer until the hand lets go. `place` moves a window
# with `SetWindowPos`, which runs in no such loop, so it exercises the immediate
# path and can never reproduce a defect that lives in the deferred one. Posting
# `WM_ENTERSIZEMOVE`, moving, and posting `WM_EXITSIZEMOVE` is that loop's own
# message sequence, and it is the only way to reach it without taking the real
# mouse away from whoever is holding it. Always close what you open: a window
# left believing a hand is on its frame defers everything for ever.
#
# X/Y are **client** pixels, which for this window are also window pixels: the
# self-drawn frame makes the client area the whole outer rectangle, so a
# coordinate read off a `ui-probe capture` is a coordinate this script can press.
param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("list", "place", "left", "right", "drag", "menu", "type", "key", "shot", "wheel", "chord", "sizemove")]
  [string]$Cmd,
  [Parameter(Mandatory = $true)][int]$ProcId,
  [int]$Window = 0,
  [int]$X = 0,
  [int]$Y = 0,
  [int]$HoverX = 0,
  [int]$HoverY = 0,
  [int]$PressX = -1,
  [int]$PressY = -1,
  # drag: where the hand lets go, the press being at -X/-Y.
  [int]$ToX = 0,
  [int]$ToY = 0,
  [int]$PumpMs = 1500,
  [int]$ShotAtMs = 500,
  [int]$W = 1200,
  [int]$H = 1000,
  [int]$X0 = 40,
  [int]$Y0 = 60,
  [int]$Gap = 20,
  [string]$Out = "",
  # type: the characters to post. key: one of the names in $POSTED_KEYS.
  # sizemove: `enter` or `exit`.
  [string]$Text = "",
  [string]$Name = "",
  # chord: modifiers as any of c(trl) s(hift) a(lt); -Name is the base key, either
  # one of $POSTED_KEYS or the single character printed on it. wheel reads the
  # same letters and holds them down across its notches.
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
  /* A press, a hand travelling, and a release — the one gesture `Press` cannot
     stand in for, because a pan is measured from move to move and a click that
     goes down and up on one pixel travels nowhere. `MK_LBUTTON` rides in the
     wParam of every move between the two, which is what a real drag carries and
     what tells the app the button is still down. */
  public static void Drag(IntPtr h, int x, int y, int toX, int toY, int steps) {
    Move(h, x, y);
    PostMessage(h, 0x0201, (IntPtr)1, LP(x,y));
    if (steps < 1) steps = 1;
    for (int i = 1; i <= steps; i++) {
      int mx = x + (toX - x) * i / steps;
      int my = y + (toY - y) * i / steps;
      PostMessage(h, 0x0200, (IntPtr)1, LP(mx,my));
      System.Threading.Thread.Sleep(30);
    }
    PostMessage(h, 0x0202, (IntPtr)0, LP(toX,toY));
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
    HoldMods(h, ctrl, shift, alt);
    PostMessage(h, down, (IntPtr)vk, Down(vk));
    PostMessage(h, up,   (IntPtr)vk, Up(vk));
    /* The hand does not let go before the window has read the chord: the key
       table is shared state and `DropMods` clears it, so releasing it in the
       same breath would have winit read the base key with nothing held. */
    System.Threading.Thread.Sleep(150);
    DropMods(h, ctrl, shift, alt);
  }
  /* `Chord`'s two halves on their own, because a chord's base is not always a
     key: `Alt`+wheel is a modifier held across a run of mouse messages, and
     `WM_MOUSEWHEEL`'s own wParam has no bit for `Alt` to ride in.

     **A posted modifier is not a held modifier, and this is the third thing
     learned the hard way** (measured 2026-09-12). winit answers "is Alt down"
     with `GetKeyState`, and a thread's key-state table is moved only by
     messages the system itself put in the queue — a `PostMessage`d
     `WM_SYSKEYDOWN` leaves it at zero. Posting the key alone therefore looked
     exactly like a chord and was read as a bare press: `Ctrl+Shift+Z` did not
     toggle the mode and `Alt`+wheel scrolled the list. So the state is *set*
     as well as posted, through the one door Windows has for it:
     `AttachThreadInput` makes this thread and the window's thread share one
     input state, and `SetKeyboardState` then writes the table winit is about
     to read. The posted key message is still sent, because it is what makes
     winit *look* — it calls `update_modifiers` out of its key handlers and
     nowhere else. Attach, set, post; and undo all three in the reverse order,
     because a table left with Alt down is a modifier stuck on a window nobody
     is holding. */
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool on);
  [DllImport("user32.dll")] public static extern bool GetKeyboardState(byte[] state);
  [DllImport("user32.dll")] public static extern bool SetKeyboardState(byte[] state);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  static uint ThreadOf(IntPtr h) { uint pid; return GetWindowThreadProcessId(h, out pid); }
  /* VK_MENU and VK_LMENU together, and the pair for each of the other two: the
     agnostic key is what `GetKeyState` is asked for and the sided one is what a
     real keyboard would also have down. */
  static void Table(bool ctrl, bool shift, bool alt, bool down) {
    var state = new byte[256];
    if (!GetKeyboardState(state)) return;
    byte bit = down ? (byte)0x80 : (byte)0x00;
    if (ctrl)  { state[0x11] = bit; state[0xA2] = bit; }
    if (shift) { state[0x10] = bit; state[0xA0] = bit; }
    if (alt)   { state[0x12] = bit; state[0xA4] = bit; }
    SetKeyboardState(state);
  }
  public static void HoldMods(IntPtr h, bool ctrl, bool shift, bool alt) {
    AttachThreadInput(GetCurrentThreadId(), ThreadOf(h), true);
    Table(ctrl, shift, alt, true);
    if (ctrl)  PostMessage(h, 0x0100, (IntPtr)0x11, Down(0x11));
    if (shift) PostMessage(h, 0x0100, (IntPtr)0x10, Down(0x10));
    if (alt)   PostMessage(h, 0x0104, (IntPtr)0x12, Down(0x12));
  }
  public static void DropMods(IntPtr h, bool ctrl, bool shift, bool alt) {
    Table(ctrl, shift, alt, false);
    if (alt)   PostMessage(h, 0x0105, (IntPtr)0x12, Up(0x12));
    if (shift) PostMessage(h, 0x0101, (IntPtr)0x10, Up(0x10));
    if (ctrl)  PostMessage(h, 0x0101, (IntPtr)0x11, Up(0x11));
    AttachThreadInput(GetCurrentThreadId(), ThreadOf(h), false);
  }
  /* `WM_ENTERSIZEMOVE` / `WM_EXITSIZEMOVE` — the modal move/size loop's own
     brackets. See this file's header for why a `place` cannot stand in for
     them. */
  public static void SizeMove(IntPtr h, bool entering) {
    PostMessage(h, entering ? 0x0231u : 0x0232u, IntPtr.Zero, IntPtr.Zero);
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
    $ctrl = $Mods -match "c"; $shift = $Mods -match "s"; $alt = $Mods -match "a"
    if ($ctrl -or $shift -or $alt) { [PostProbe]::HoldMods($hwnd, $ctrl, $shift, $alt); Start-Sleep -Milliseconds 150 }
    for ($i = 0; $i -lt $Steps; $i++) { [PostProbe]::Wheel($hwnd, $X, $Y, $Delta); Start-Sleep -Milliseconds 60 }
    if ($ctrl -or $shift -or $alt) { Start-Sleep -Milliseconds 150; [PostProbe]::DropMods($hwnd, $ctrl, $shift, $alt) }
    "posted $Steps notches of $Delta at ($X,$Y)$(if ($Mods) { " under $Mods" }) to window $Window"
  }
  "sizemove" {
    if ($Name -ne "enter" -and $Name -ne "exit") { throw "sizemove needs -Name enter or -Name exit" }
    [PostProbe]::SizeMove((Get-Target), ($Name -eq "enter"))
    "posted size-move $Name to window $Window"
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
  "drag" {
    [PostProbe]::Drag((Get-Target), $X, $Y, $ToX, $ToY, $Steps)
    "posted drag ($X,$Y) -> ($ToX,$ToY) in $Steps steps to window $Window"
  }
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
