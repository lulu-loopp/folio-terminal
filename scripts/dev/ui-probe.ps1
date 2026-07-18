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
#   .\ui-probe.ps1 key  -Pid <pid> -Name Enter|Backspace|Escape|Shift
#   .\ui-probe.ps1 capture -Pid <pid> -Out shot.png [-Margin 400]  → DPI-aware capture; Margin grows the
#                                                                    region beyond the window (IME popups
#                                                                    are separate windows and live outside)
#   .\ui-probe.ps1 close -Pid <pid>
#
# Capture is per-monitor-DPI-aware: pixels are 1:1 physical, so cell width can
# be measured directly (expected: ceil(8.8 × scale) px per ASCII cell).
#
# KNOWN LIMITS (2026-07-17):
# - launch/capture/close: proven (settled the M0-β presentation-shrink case).
# - type/key: foreground-VERIFIED before any key is sent (never types blind —
#   the first draft sprayed keys at whatever was foreground; never again), and
#   scancode-level injection reaches classic Win32 apps (charmap), but bt-app
#   (winit) does not surface the injected keys — under investigation. Until
#   solved, rendering-path probes should set BT_PROBE_INPUT to a raw byte file;
#   bt-app feeds it directly into Term at startup without starting ConPTY. The
#   M1 fixture is scripts/dev/width-probe-input.vt. IME candidate-window checks
#   stay with a human.

param(
  [Parameter(Position = 0, Mandatory = $true)]
  [ValidateSet("launch", "type", "key", "capture", "close", "wheel", "resize")]
  [string]$Cmd,
  [int]$ProcId = 0,
  [string]$Text = "",
  [string]$Name = "",
  [string]$Out = "$env:TEMP\ui-probe.png",
  [int]$Margin = 0,
  [int]$WaitSeconds = 15,
  [int]$Delta = 3,
  [int]$W = 0,
  [int]$H = 0,
  [switch]$TraceDpi
)

$ErrorActionPreference = "Stop"

Add-Type @'
using System;
using System.Runtime.InteropServices;
public struct PRECT { public int L, T, R, B; }
[StructLayout(LayoutKind.Sequential)]
public struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
[StructLayout(LayoutKind.Explicit)]
public struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad1; [FieldOffset(8)] public long pad2; [FieldOffset(16)] public long pad3; }
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
  public const uint KEYEVENTF_UNICODE = 0x0004;
  public const uint KEYEVENTF_KEYUP = 0x0002;
  [DllImport("user32.dll")] public static extern short VkKeyScanW(char c);
  [DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint mapType);
  /* Scancode-level typing: winit's keyboard pipeline reconstructs keys from the
     hardware scancode stream and quietly drops KEYEVENTF_UNICODE/VK_PACKET
     synthetics (measured: unicode injection with verified foreground produced
     zero characters in the app). This path is what a physical keyboard emits,
     so it exercises the app's REAL input path — which for an input-probe is a
     feature, not a workaround. */
  public static void TypeText(string text) {
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
      SendInput((uint)seq.Count, seq.ToArray(), Marshal.SizeOf(typeof(INPUT)));
      System.Threading.Thread.Sleep(24);   // real-ish cadence; IMEs dislike zero-interval streams
    }
  }
  public static void TapVk(ushort vk) {
    var down = new INPUT { type = 1 }; down.u.ki = new KEYBDINPUT { wVk = vk, wScan = 0, dwFlags = 0 };
    var up   = new INPUT { type = 1 }; up.u.ki   = new KEYBDINPUT { wVk = vk, wScan = 0, dwFlags = KEYEVENTF_KEYUP };
    SendInput(2, new INPUT[] { down, up }, Marshal.SizeOf(typeof(INPUT)));
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
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
}
'@

[Probe]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null   # per-monitor v2: physical pixels everywhere

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
    [Probe]::TypeText($Text)
    "typed $($Text.Length) chars into pid=$ProcId (foreground verified)"
  }
  "key" {
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not sending keys blind" }
    $vk = @{ Enter = 0x0D; Backspace = 0x08; Escape = 0x1B; Shift = 0x10 }[$Name]
    if (-not $vk) { throw "unknown key: $Name" }
    [Probe]::TapVk([uint16]$vk)   # [ushort] accelerator only exists in PS 7; this runs on 5.1
    "sent $Name (foreground verified)"
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
    $h = Get-AppWindow $ProcId
    if (-not [Probe]::BringToFront($h)) { throw "REFUSED: target window did not take foreground — not scrolling blind" }
    $r = New-Object PRECT
    [Probe]::GetWindowRect($h, [ref]$r) | Out-Null
    [Probe]::SetCursorPos([int](($r.L + $r.R) / 2), [int](($r.T + $r.B) / 2)) | Out-Null
    Start-Sleep -Milliseconds 150
    [Probe]::Wheel($Delta)   # positive = scroll up (into history), negative = down
    "wheeled $Delta notches on pid=$ProcId (foreground verified)"
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
