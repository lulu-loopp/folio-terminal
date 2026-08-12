# ui-probe.ps1 — autonomous UI acceptance probe for bt-app
#
# Lets the agent (or a human) drive the real window end to end without manual
# interaction: launch → focus → inject keystrokes → capture physical pixels →
# measure. Born during M0-β round 5, where pixel-measuring the window (8.8px vs
# 17.6px per cell) settled a four-round debugging stalemate in one shot.
#
# Usage:
#   .\ui-probe.ps1 launch [-TraceDpi] [-WaitSeconds 15]   → prints PID + HWND; keeps app running
#   .\ui-probe.ps1 type -Pid <pid> -Text "echo hi"        → focuses window, injects text (Unicode SendInput)
#   .\ui-probe.ps1 key  -Pid <pid> -Name Enter|Backspace|Escape|Shift|Tab|Space|Delete|Insert
#                                        |Left|Up|Right|Down|Home|End|PageUp|PageDown
#                                                      → the navigation cluster carries
#                                                        KEYEVENTF_EXTENDEDKEY, without which
#                                                        an arrow arrives as the numpad key
#                                                        sharing its scancode
#   .\ui-probe.ps1 chord -Pid <pid> -Mods cs -Key n      → Ctrl+Shift+N (c/s/a = ctrl/shift/alt)
#   .\ui-probe.ps1 chord -Pid <pid> -Mods c -Name Tab    → Ctrl+Tab
#   .\ui-probe.ps1 capture -Pid <pid> -Out shot.png [-Margin 400]  → DPI-aware capture; Margin grows the
#                                                                    region beyond the window (IME popups
#                                                                    are separate windows and live outside)
#   .\ui-probe.ps1 click -Pid <pid> -X 100 -Y 20               → left click at window+(X,Y) physical px
#   .\ui-probe.ps1 dblclick -Pid <pid> -X 100 -Y 20 [-GapMs 90] → two presses inside the multi-click
#                                                                interval, from one process — two separate
#                                                                `click` runs are never a double click
#   .\ui-probe.ps1 burst -Pid <pid> -Out run.png [-Frames 8] [-EveryMs 30] [-ClickFirst -X .. -Y ..]
#                                                              → a run of captures from one process, to
#                                                                photograph something that is moving; a
#                                                                180ms transition is over before a second
#                                                                `capture` invocation could start
#   .\ui-probe.ps1 hover -Pid <pid> -X -160 -Y 20              → park the pointer; negative counts from
#                                                                the right/bottom edge (the caption run)
#   .\ui-probe.ps1 drag -Pid <pid> -X 40 -Y 120 -X2 400 -Y2 160 → press at (X,Y), travel, release at
#                                                                (X2,Y2); this is how a text selection
#                                                                is made, and the travel is required
#   .\ui-probe.ps1 close -Pid <pid>
#
# Capture is per-monitor-DPI-aware: pixels are 1:1 physical, so cell width can
# be measured directly (expected: ceil(8.8 × scale) px per ASCII cell).
#
# SOLVED 2026-08-10 — keyboard injection reaches bt-app.
# - The old KNOWN LIMIT ("scancode injection reaches charmap but bt-app/winit
#   does not surface the injected keys") was never about winit at all. The INPUT
#   struct in this file marshalled to 32 bytes instead of the 40 that Win32 x64
#   defines, so every SendInput call was rejected outright and nothing was ever
#   sent. See the note on INPUTUNION below. type/key/chord now drive the real
#   window, and every send path checks the count SendInput returns rather than
#   assuming a call happened.
# - Still true: foreground is VERIFIED before any key is sent (never types blind
#   — the first draft sprayed keystrokes at whatever was foreground; never
#   again). A refusal means the window did not take the foreground, not that the
#   key failed.
# - Still true: BT_PROBE_INPUT is the right tool for rendering-path probes that
#   want bytes rather than keys — bt-app feeds the file straight into Term at
#   startup without starting ConPTY. The M1 fixture is
#   scripts/dev/width-probe-input.vt. IME candidate-window checks stay with a
#   human.
#
# ENVIRONMENT NOTE (measured 2026-08-10, P2-7 acceptance): with a Chinese IME
# loaded, unmodified printable keys arrive as NamedKey::Process — the IME owns
# them and commits through the Ime path, which is correct and is what bt-app
# already handles. Ctrl/Alt chords bypass the IME and arrive as characters. The
# one exception found: `Ctrl+,` has its KEYDOWN swallowed system-wide while its
# keyup still arrives (Ctrl+. / Ctrl+; / Ctrl+/ all arrive normally), which is
# the signature of a global hotkey owned by the IME stack. A chord that "does
# nothing" is worth checking against that before it is called an app bug.

param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("launch", "type", "key", "chord", "capture", "close", "wheel", "resize", "click", "rightclick", "dblclick", "hover", "drag", "burst")]
  [string]$Cmd,
  [int]$ProcId = 0,
  [string]$Text = "",
  [string]$Name = "",
  # chord: modifiers as a string containing any of c(trl) s(hift) a(lt), and the
  # base key named by the character printed on it (-Key "n", -Key "-", -Key "1").
  [string]$Mods = "",
  [string]$Key = "",
  [string]$Out = "$env:TEMP\ui-probe.png",
  [int]$Margin = 0,
  [int]$WaitSeconds = 15,
  [int]$Delta = 3,
  [int]$W = 0,
  [int]$H = 0,
  # click/hover: physical pixels from the window's own top-left. Negative values
  # count back from its right/bottom edge, which is how the caption run and the
  # gear are addressed without knowing the window's width.
  [int]$X = 0,
  [int]$Y = 0,
  # drag: the far end of the travel, in the same coordinate system as X/Y.
  [int]$X2 = 0,
  [int]$Y2 = 0,
  [int]$Steps = 12,
  # dblclick: the gap between the two releases. Must stay under the app's own
  # multi-click interval; the OS default is 500ms and bt-app follows it.
  [int]$GapMs = 90,
  # burst: how many frames to grab, and how far apart. The default window spans
  # a 180ms transition with frames either side of it.
  [int]$Frames = 8,
  [int]$EveryMs = 30,
  # burst: press at X/Y and start the run immediately. A separate `click`
  # invocation cannot be used to start an animation, because the burst's own
  # PowerShell start-up would land after the transition had finished.
  [switch]$ClickFirst,
  [switch]$TraceDpi
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Runtime.InteropServices;
public struct PRECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)]
public struct WPOINT { public int X, Y; }
[StructLayout(LayoutKind.Sequential)]
public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
/* The union must be as big as its LARGEST member, and that is MOUSEINPUT, not
   KEYBDINPUT: on x64 MOUSEINPUT is 4+4+4+4+4+8 = 28 bytes padded to 32, so the
   union is 32 and `INPUT` is 4 + 4 of alignment + 32 = 40.

   Diagnosed 2026-08-10, and it is the whole of the "injected keys do not reach
   winit" mystery this file has carried as a KNOWN LIMIT since 2026-07-17: with
   only three pads the union measured 24, `Marshal.SizeOf(INPUT)` handed
   SendInput a cbSize of 32, and SendInput rejects any cbSize that is not
   exactly sizeof(INPUT) — it returned 0 and sent nothing, every time. Nothing
   was ever being swallowed by winit; nothing was ever being sent. The fourth
   pad is the fix, and callers should check the count SendInput returns rather
   than trust that a call happened. */
[StructLayout(LayoutKind.Explicit)]
public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad1; [FieldOffset(8)] public long pad2; [FieldOffset(16)] public long pad3; [FieldOffset(24)] public long pad4; }
[StructLayout(LayoutKind.Sequential)]
public struct INPUT { public uint type; public INPUTUNION u; }
public class Probe {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out PRECT r);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  /* The foreground lock denies SetForegroundWindow from background processes,
     and SendInput goes to WHATEVER is foreground — a blind probe can spray
     keystrokes into the user's active app. So: attach to the foreground
     thread's input queue (the classic legal workaround), request foreground,
     and let the caller VERIFY before a single key is sent. */
  public static bool BringToFront(IntPtr target) {
    uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    uint myThread = GetCurrentThreadId();
    AttachThreadInput(myThread, fgThread, true);
    bool ok = SetForegroundWindow(target);
    AttachThreadInput(myThread, fgThread, false);
    System.Threading.Thread.Sleep(300);
    return ok && GetForegroundWindow() == target;
  }
  /* The safety law for a POINTER, which is a different law from the one for a
     keystroke. A keystroke has no address — it lands wherever the foreground is,
     so foreground is the only thing that can be verified. A click has an address:
     it lands on whatever window owns that screen pixel. So the honest check is
     ownership of the point, not who currently holds foreground — and it is the
     stricter one, because it is what actually decides where the press goes.
     This also matches what a human does: clicking a background window is how you
     bring it to the front, and refusing to click until it is already in front
     makes the probe unable to do the one thing a user does first. */
  /* WindowFromPoint takes POINT BY VALUE, and on x64 that is one 8-byte
     register — declaring it as two ints puts y in a register the callee never
     reads and leaves the high half of x as garbage, so it answers for a point
     nobody asked about. (Measured: it named `explorer` for a pixel the screen
     capture proves belongs to the app.) The struct is the signature. */
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(WPOINT p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static bool PointBelongsTo(int x, int y, int targetPid) {
    WPOINT p; p.X = x; p.Y = y;
    IntPtr under = WindowFromPoint(p);
    if (under == IntPtr.Zero) return false;
    uint owner; GetWindowThreadProcessId(under, out owner);
    return owner == (uint)targetPid;
  }
  public const uint KEYEVENTF_UNICODE = 0x0004;
  public const uint KEYEVENTF_KEYUP = 0x0002;
  public const uint KEYEVENTF_EXTENDEDKEY = 0x0001;
  /* The navigation cluster shares its scancodes with the numeric keypad, and the
     only thing that tells the two apart is this flag. MapVirtualKeyW hands back
     the NUMPAD scancode for VK_LEFT (0x4B = numpad 4), so an arrow sent without
     it arrives as a keypad key — which is how "the arrow keys do nothing"
     looks from inside an app that decodes scancodes, as winit does.

     Added 2026-08-11, for the files tree's keyboard contract: it is the first
     thing in this product whose whole surface IS the arrow keys. */
  public static bool IsExtendedVk(ushort vk) {
    switch (vk) {
      case 0x21: case 0x22:            // PageUp / PageDown
      case 0x23: case 0x24:            // End / Home
      case 0x25: case 0x26:            // Left / Up
      case 0x27: case 0x28:            // Right / Down
      case 0x2D: case 0x2E:            // Insert / Delete
      /* VK_APPS, the Menu key. Extended like the navigation island around it,
         and missing here until 2026-08-11: without the flag Windows reports
         scancode 0x5D instead of 0xE05D, and winit's scancode table — which it
         consults before the virtual key — has no 0x5D, so the press arrived as
         some other key entirely and a context menu bound to the Menu key looked
         like it was not bound at all. The app was innocent; this line was the
         bug. */
      case 0x5D:                       // Menu (VK_APPS)
      case 0x90:                       // NumLock
      case 0xA3: case 0xA5:            // RightCtrl / RightAlt
        return true;
      default:
        return false;
    }
  }
  [DllImport("user32.dll")] public static extern short VkKeyScanW(char c);
  [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint mapType);
  /* Scancode-level typing: winit's keyboard pipeline reconstructs keys from the
     hardware scancode stream and quietly drops KEYEVENTF_UNICODE/VK_PACKET
     synthetics (measured: unicode injection with verified foreground produced
     zero characters in the app). This path is what a physical keyboard emits,
     so it exercises the app's REAL input path — which for an input-probe is a
     feature, not a workaround. */
  /* Returns how many events SendInput actually accepted, so a caller can tell
     "sent" from "silently rejected" — see the INPUT struct note above. */
  public static uint TypeText(string text) {
    uint accepted = 0;
    foreach (char c in text) {
      short vks = VkKeyScanW(c);
      if (vks == -1) continue;                    // not typeable on this layout
      ushort vk = (ushort)(vks & 0xFF);
      bool shift = (vks & 0x100) != 0;
      ushort sc = (ushort)MapVirtualKeyW(vk, 0);  // MAPVK_VK_TO_VSC
      var seq = new System.Collections.Generic.List<INPUT>();
      if (shift) { var s = new INPUT { type = 1 }; s.u.ki = new KEYBDINPUT { wVk = 0x10, wScan = 0x2A, dwFlags = 0 }; seq.Add(s); }
      var down = new INPUT { type = 1 }; down.u.ki = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = 0 }; seq.Add(down);
      var up   = new INPUT { type = 1 }; up.u.ki   = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = KEYEVENTF_KEYUP }; seq.Add(up);
      if (shift) { var s = new INPUT { type = 1 }; s.u.ki = new KEYBDINPUT { wVk = 0x10, wScan = 0x2A, dwFlags = KEYEVENTF_KEYUP }; seq.Add(s); }
      accepted += SendInput((uint)seq.Count, seq.ToArray(), Marshal.SizeOf(typeof(INPUT)));
      System.Threading.Thread.Sleep(24);   // real-ish cadence; IMEs dislike zero-interval streams
    }
    return accepted;
  }
  public static uint TapVk(ushort vk) {
    ushort sc = (ushort)MapVirtualKeyW(vk, 0);
    uint ext = IsExtendedVk(vk) ? KEYEVENTF_EXTENDEDKEY : 0;
    var down = new INPUT { type = 1 }; down.u.ki = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = ext };
    var up   = new INPUT { type = 1 }; up.u.ki   = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = ext | KEYEVENTF_KEYUP };
    return SendInput(2, new INPUT[] { down, up }, Marshal.SizeOf(typeof(INPUT)));
  }
  /* A modifier chord, at the same scancode level TypeText uses — hold the
     modifiers, tap the base key, release in reverse. Added 2026-08-10 for the
     P2-7 shortcut audit, which needs Ctrl+Shift+N, Ctrl+Tab and Alt+Shift+= to
     arrive as the app's own dispatcher sees them rather than as text.

     The base key is named by the character printed on it, and its VK comes from
     VkKeyScanW on the CURRENT layout: only the low byte is taken, and the shift
     bit VkKeyScanW reports is deliberately discarded, because the caller is
     supplying its own Shift. That is what makes `-Key "-"` mean "the key that
     types a minus here" rather than a hard-coded US scancode. */
  public static uint Chord(bool ctrl, bool shift, bool alt, ushort vk) {
    ushort sc = (ushort)MapVirtualKeyW(vk, 0);
    var seq = new System.Collections.Generic.List<INPUT>();
    if (ctrl)  { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x11, wScan = 0x1D, dwFlags = 0 }; seq.Add(m); }
    if (shift) { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x10, wScan = 0x2A, dwFlags = 0 }; seq.Add(m); }
    if (alt)   { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x12, wScan = 0x38, dwFlags = 0 }; seq.Add(m); }
    uint ext = IsExtendedVk(vk) ? KEYEVENTF_EXTENDEDKEY : 0;
    var down = new INPUT { type = 1 }; down.u.ki = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = ext }; seq.Add(down);
    var up   = new INPUT { type = 1 }; up.u.ki   = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = ext | KEYEVENTF_KEYUP }; seq.Add(up);
    if (alt)   { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x12, wScan = 0x38, dwFlags = KEYEVENTF_KEYUP }; seq.Add(m); }
    if (shift) { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x10, wScan = 0x2A, dwFlags = KEYEVENTF_KEYUP }; seq.Add(m); }
    if (ctrl)  { var m = new INPUT { type = 1 }; m.u.ki = new KEYBDINPUT { wVk = 0x11, wScan = 0x1D, dwFlags = KEYEVENTF_KEYUP }; seq.Add(m); }
    return SendInput((uint)seq.Count, seq.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  /* The VK of the key that prints this character on the current layout. */
  public static ushort VkForChar(char c) {
    short vks = VkKeyScanW(c);
    if (vks == -1) return 0;
    return (ushort)(vks & 0xFF);
  }
  /* Discovered 2026-07-17: unlike synthetic KEYBOARD events, synthetic mouse
     WHEEL events DO reach winit — scrollback can be driven autonomously. Same
     safety law as typing: foreground-verified before anything is sent. */
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public static void Wheel(int notches) {
    int dir = notches < 0 ? -120 : 120;
    for (int i = 0; i < System.Math.Abs(notches); i++) {
      mouse_event(0x0800, 0, 0, dir, UIntPtr.Zero);
      System.Threading.Thread.Sleep(60);
    }
  }
  /* Buttons travel the same road the wheel does, and reach winit for the same
     reason. The pointer is parked first and given a beat to land: bt-app routes
     a press through the position its last CursorMoved reported, so a click sent
     in the same tick as the move would be tested against the old point. */
  public static void Click(int x, int y) {
    ClickNoSettle(x, y);
    System.Threading.Thread.Sleep(120);
  }
  /* The same press with the settling sleep left off, for the one caller that
     must not have it: a burst photographing a transition. The 120ms `Click`
     waits out so the *next* command sees a finished window is longer than the
     180ms transition itself, so a burst started after it would only ever
     photograph the end state and report that nothing moved. */
  public static void ClickNoSettle(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(120);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    System.Threading.Thread.Sleep(40);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
  }
  /* The secondary button, which is what raises a context menu. Its own method
     rather than a flag on `Click`, so the existing callers keep the signature
     they were written against and nothing has to be re-read to be sure it still
     presses the left one. Same park-then-press shape and the same reason. */
  public static void RightClick(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(120);
    mouse_event(0x0008, 0, 0, 0, UIntPtr.Zero);   // RIGHTDOWN
    System.Threading.Thread.Sleep(40);
    mouse_event(0x0010, 0, 0, 0, UIntPtr.Zero);   // RIGHTUP
    System.Threading.Thread.Sleep(120);
  }
  public static void MoveTo(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(150);
  }
  /* A press, a *travelled* path, and a release — the shape a text selection is
     made of. The intermediate moves are the point: bt-app extends a selection
     from CursorMoved events while the button is down, so a press at one end and
     a release at the other with nothing in between selects nothing at all. */
  public static void Drag(int x1, int y1, int x2, int y2, int steps) {
    SetCursorPos(x1, y1);
    System.Threading.Thread.Sleep(150);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    System.Threading.Thread.Sleep(80);
    if (steps < 1) steps = 1;
    for (int i = 1; i <= steps; i++) {
      SetCursorPos(x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps);
      System.Threading.Thread.Sleep(40);
    }
    System.Threading.Thread.Sleep(80);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
    System.Threading.Thread.Sleep(150);
  }
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
}
'@

[Probe]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null   # per-monitor v2: physical pixels everywhere

# The keys that have names rather than printed characters, shared by `key` and
# `chord` so the two can never know different halves of the keyboard. The
# navigation cluster is here for the files tree's contract (↑↓←→, Home/End,
# Enter/Space); `TapVk` and `Chord` add the extended-key flag for whichever of
# these need it, so a caller never has to know which ones those are.
$NAMED_KEYS = @{
  Enter = 0x0D; Backspace = 0x08; Escape = 0x1B; Shift = 0x10; Tab = 0x09
  Space = 0x20; Delete = 0x2E; Insert = 0x2D
  Left = 0x25; Up = 0x26; Right = 0x27; Down = 0x28
  Home = 0x24; End = 0x23; PageUp = 0x21; PageDown = 0x22
  F9 = 0x78; F10 = 0x79
  # VK_APPS — the Menu key, which is how a keyboard raises a context menu on
  # every Windows application. Named `Menu` here rather than `Apps` because that
  # is what is printed on the keycap.
  Menu = 0x5D
}

function Get-AppWindow([int]$targetPid) {
  $p = Get-Process -Id $targetPid -ErrorAction Stop
  if ($p.MainWindowHandle -eq [IntPtr]::Zero) { throw "process $targetPid has no visible window (yet)" }
  return $p.MainWindowHandle
}

switch ($Cmd) {
  "launch" {
    if ($TraceDpi) { $env:BT_STARTUP_TRACE = "1" }
    $err = "$env:TEMP\ui-probe-stderr.txt"
    $p = Start-Process -FilePath "D:\Developer\BetterTerminal\target\release\bt-app.exe" -RedirectStandardError $err -PassThru
    Start-Sleep -Seconds $WaitSeconds
    $h = Get-AppWindow $p.Id
    "pid=$($p.Id) hwnd=$h stderr=$err"
    Get-Content $err -ErrorAction SilentlyContinue | Select-String "BT_DPI" | Select-Object -First 3
  }
  "type" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not typing blind" }
    $sent = [Probe]::TypeText($Text)
    if ($sent -eq 0) { throw "SendInput accepted 0 events — nothing was typed" }
    "typed $($Text.Length) chars into pid=$ProcId ($sent events accepted, foreground verified)"
  }
  "key" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not sending keys blind" }
    $vk = $NAMED_KEYS[$Name]
    if (-not $vk) { throw "unknown key: $Name (known: $(($NAMED_KEYS.Keys | Sort-Object) -join ', '))" }
    $sent = [Probe]::TapVk([uint16]$vk)   # [ushort] accelerator only exists in PS 7; this runs on 5.1
    if ($sent -eq 0) { throw "SendInput accepted 0 events — $Name was not sent" }
    "sent $Name ($sent events accepted, foreground verified)"
  }
  "chord" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not sending keys blind" }
    $ctrl  = $Mods -match "c"
    $shift = $Mods -match "s"
    $alt   = $Mods -match "a"
    # A named key wins; otherwise the base key is the one printing $Key here.
    if ($Name -and $NAMED_KEYS.ContainsKey($Name)) {
      $vk = $NAMED_KEYS[$Name]
    } elseif ($Key) {
      $vk = [Probe]::VkForChar([char]$Key)
      if (-not $vk) { throw "'$Key' is not typeable on the current layout" }
    } else {
      throw "chord needs -Key <char> or -Name <Tab|Enter|Escape|F9>"
    }
    $sent = [Probe]::Chord($ctrl, $shift, $alt, [uint16]$vk)
    if ($sent -eq 0) { throw "SendInput accepted 0 events — the chord was not sent" }
    "sent chord mods=$Mods key=$(if ($Name) { $Name } else { $Key }) vk=0x$('{0:X2}' -f $vk) ($sent events accepted, foreground verified)"
  }
  "capture" {
    $h = Get-AppWindow $ProcId
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    Add-Type -AssemblyName System.Drawing
    $x = $r.L - $Margin; $y = $r.T - $Margin
    $w = ($r.R - $r.L) + 2 * $Margin; $hh = ($r.B - $r.T) + 2 * $Margin
    $bmp = New-Object System.Drawing.Bitmap($w, $hh)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $bmp.Save($Out); $g.Dispose(); $bmp.Dispose()
    "captured ${w}x${hh} (margin $Margin) -> $Out"
  }
  "wheel" {
    # A notch belongs to whatever the pointer is over, so where the pointer is
    # put is the whole of what this command means. -X/-Y address it exactly as
    # `click` and `hover` do — including the negative-counts-from-the-far-edge
    # rule — and omitting both keeps the old behaviour of the window's centre,
    # which is what every existing caller wants for the terminal's own history.
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not scrolling blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    if ($X -eq 0 -and $Y -eq 0) {
      $px = [int](($r.L + $r.R) / 2); $py = [int](($r.T + $r.B) / 2)
    } else {
      $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
      $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
      # The same ownership check a click makes, and for the same reason: a notch
      # has an address, and one aimed past the window's edge scrolls whatever is
      # behind it.
      if (-not [Probe]::PointBelongsTo($px, $py, $ProcId)) {
        throw "REFUSED: ($px, $py) does not belong to pid=$ProcId — not scrolling someone else's window"
      }
    }
    [Probe]::SetCursorPos($px, $py) | Out-Null
    Start-Sleep -Milliseconds 150
    [Probe]::Wheel($Delta)   # positive = scroll up (into history), negative = down
    "wheeled $Delta notches at ($px, $py) on pid=$ProcId (foreground verified)"
  }
  "click" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not clicking blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
    $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
    [Probe]::Click($px, $py)
    "clicked ($px, $py) = window+($X, $Y) on pid=$ProcId (foreground verified)"
  }
  "rightclick" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not clicking blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
    $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
    [Probe]::RightClick($px, $py)
    "right-clicked ($px, $py) = window+($X, $Y) on pid=$ProcId (foreground verified)"
  }
  "dblclick" {
    # Two presses inside the multi-click interval, from *one* process. Two
    # separate `click` invocations cannot be a double click: each pays for its
    # own PowerShell start-up, and the app's pairing window closes long before
    # the second one arrives.
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not clicking blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
    $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
    if (-not [Probe]::PointBelongsTo($px, $py, $ProcId)) { throw "REFUSED: ($px, $py) is not over pid=$ProcId — not clicking blind" }
    [Probe]::Click($px, $py)
    Start-Sleep -Milliseconds $GapMs
    [Probe]::Click($px, $py)
    "double-clicked ($px, $py) = window+($X, $Y) on pid=$ProcId, ${GapMs}ms apart (foreground verified)"
  }
  "burst" {
    # A run of captures from one process, to catch something that is moving.
    # A transition is over in 180ms and a fresh `capture` invocation costs more
    # than that, so an animation can only ever be photographed from inside a
    # loop that is already running. `-Out shot.png` writes shot-00.png … .
    # `$hwnd`, not the `$h` every other block uses: `$h` *is* the `-H` parameter
    # (PowerShell variables are case-insensitive), so the handle would overwrite
    # the requested crop height with a window handle.
    $hwnd = Get-AppWindow $ProcId
    $r = New-Object PRECT
    [Probe]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    Add-Type -AssemblyName System.Drawing
    # `-W`/`-H` crop the grab to a sub-rect of the window's top-left corner. A
    # full 2060x1138 CopyFromScreen costs ~50ms, which is a third of the
    # transition being photographed — the frames end up further apart than the
    # thing they are meant to sample. A strip wide enough to hold the rail is
    # four times cheaper and turns 2 usable frames into 6.
    # Named apart from `$W`/`$H`: PowerShell variables are case-insensitive, so a
    # local `$w` *is* the `-W` parameter and assigning to it destroys the value
    # being read on the same line.
    $capW = if ($W -gt 0) { [int]$W } else { [int]($r.R - $r.L) }
    $capH = if ($H -gt 0) { [int]$H } else { [int]($r.B - $r.T) }
    $stem = [System.IO.Path]::Combine(
      [System.IO.Path]::GetDirectoryName($Out),
      [System.IO.Path]::GetFileNameWithoutExtension($Out))
    if ($ClickFirst) {
      if (-not [Probe]::BringToFront($hwnd)) { throw "REFUSED: target window did not take foreground — not clicking blind" }
      $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
      $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
      if (-not [Probe]::PointBelongsTo($px, $py, $ProcId)) { throw "REFUSED: ($px, $py) is not over pid=$ProcId — not clicking blind" }
      # The first CopyFromScreen pays for GDI warm-up, so take one throwaway
      # frame *before* the press. Paid here, that cost lands before the clock
      # rather than inside the transition being photographed.
      $warm = New-Object -TypeName System.Drawing.Bitmap -ArgumentList @(8, 8)
      $wg = [System.Drawing.Graphics]::FromImage($warm)
      $wg.CopyFromScreen($r.L, $r.T, 0, 0, $warm.Size)
      $wg.Dispose(); $warm.Dispose()
      [Probe]::ClickNoSettle($px, $py)
    }
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $shots = @()
    for ($i = 0; $i -lt $Frames; $i++) {
      $bmp = New-Object -TypeName System.Drawing.Bitmap -ArgumentList @($capW, $capH)
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
      $at = $clock.ElapsedMilliseconds
      $path = "{0}-{1:d2}.png" -f $stem, $i
      $bmp.Save($path); $g.Dispose(); $bmp.Dispose()
      $shots += "{0} @ {1}ms" -f $path, $at
      if ($i -lt $Frames - 1) { Start-Sleep -Milliseconds $EveryMs }
    }
    $shots -join "`n"
  }
  "drag" {
    $h = Get-AppWindow $ProcId
    [Probe]::BringToFront($h) | Out-Null
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
    $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
    $qx = if ($X2 -lt 0) { $r.R + $X2 } else { $r.L + $X2 }
    $qy = if ($Y2 -lt 0) { $r.B + $Y2 } else { $r.T + $Y2 }
    if (-not [Probe]::PointBelongsTo($px, $py, $ProcId)) { throw "REFUSED: ($px, $py) is not over pid=$ProcId — not dragging blind" }
    if (-not [Probe]::PointBelongsTo($qx, $qy, $ProcId)) { throw "REFUSED: ($qx, $qy) is not over pid=$ProcId — not dragging blind" }
    [Probe]::Drag($px, $py, $qx, $qy, $Steps)
    "dragged ($px, $py) -> ($qx, $qy) = window+($X, $Y) -> window+($X2, $Y2) on pid=$ProcId (foreground verified)"
  }
  "hover" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not moving the pointer blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    $px = if ($X -lt 0) { $r.R + $X } else { $r.L + $X }
    $py = if ($Y -lt 0) { $r.B + $Y } else { $r.T + $Y }
    [Probe]::MoveTo($px, $py)
    "pointer at ($px, $py) = window+($X, $Y) on pid=$ProcId (foreground verified)"
  }
  "resize" {
    # NOT $h: PowerShell variables are case-INsensitive, so $h would overwrite
    # the -H height parameter with the HWND (real incident 2026-07-18: resized
    # a window to 65595px tall and took the app down with it).
    $hwndR = Get-AppWindow $ProcId
    if ($W -le 0 -or $H -le 0) { throw "resize needs -W and -H" }
    [Probe]::SetWindowPos($hwndR, [IntPtr]::Zero, 60, 60, $W, $H, 0x0004) | Out-Null
    "resized pid=$ProcId to ${W}x${H}"
  }
  "close" {
    try { Stop-Process -Id $ProcId -Force -ErrorAction Stop } catch {}
    "closed pid=$ProcId"
  }
}
