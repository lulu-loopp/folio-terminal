<#
.SYNOPSIS
    Guest side of the planted and overlay rows (clean-vm.md 4.4, E1-b): what card
    Folio's window shows, as text - the window captured, the capture read by the
    operating system's own OCR (Windows.Media.Ocr), the picture kept beside the text.

.DESCRIPTION
    Runs in the guest under Windows PowerShell 5.1, called by `keys.ps1`'s step
    `card:<name>`. Folio draws its text itself (no accessibility tree), so the only
    reading of the card a script can make is of its pixels.

    **The window is Folio's application window, never winit's event target**
    (cleanvm-harness pitfall 3): of every folio.exe's visible top-level windows of
    winit's application class `Window Class`, the one with the largest client area.
    It is captured from the screen (Folio draws through DirectComposition, which a
    PrintWindow does not see), scaled 2x (the OCR reads a 13 px card line poorly
    at 1x) and read. Writes <Out>.png (the capture), <Out>.txt (the recognised
    lines, one per line, after a first line `window <describe>`), or <Out>.txt with
    `window none` when no Folio window stands.
#>
param([Parameter(Mandatory)] [string] $Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CardWindows {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder text, int max);
  [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
  public const string AppClass = "Window Class";
  public static IntPtr AppWindowOf(uint[] pids) {
    IntPtr best = IntPtr.Zero; long bestArea = 0;
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (Array.IndexOf(pids, pid) < 0 || !IsWindowVisible(h)) { return true; }
      var name = new StringBuilder(128); GetClassName(h, name, 128);
      RECT c; GetClientRect(h, out c);
      long area = (long)(c.R - c.L) * (c.B - c.T);
      if (name.ToString() == AppClass && area > bestArea) { bestArea = area; best = h; }
      return true;
    }, IntPtr.Zero);
    return best;
  }
  public static uint PidOf(IntPtr h) { uint pid; GetWindowThreadProcessId(h, out pid); return pid; }
}
'@
[CardWindows]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null
$txt = "$Out.txt"
$pids = @(Get-Process folio -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
$h = if ($pids.Count -gt 0) { [CardWindows]::AppWindowOf([uint32[]]$pids) } else { [IntPtr]::Zero }
if ($h -eq [IntPtr]::Zero) { Set-Content -LiteralPath $txt -Value 'window none' -Encoding UTF8; return }
$owner = [CardWindows]::PidOf($h)
$path = (Get-Process -Id $owner -ErrorAction SilentlyContinue).Path
$r = New-Object CardWindows+RECT
[void][CardWindows]::GetWindowRect($h, [ref]$r)
$w = $r.R - $r.L; $hh = $r.B - $r.T
$shot = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($shot)
$g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size($w, $hh)))
$g.Dispose()
$big = New-Object System.Drawing.Bitmap(($w * 2), ($hh * 2))
$g = [System.Drawing.Graphics]::FromImage($big)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.DrawImage($shot, 0, 0, ($w * 2), ($hh * 2))
$g.Dispose()
$png = "$Out.png"
$shot.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$scaled = "$Out.ocr.png"
$big.Save($scaled, [System.Drawing.Imaging.ImageFormat]::Png)
$shot.Dispose(); $big.Dispose()

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$asTask = @([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, [Type] $type) {
  $task = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
  # bounded: an OCR of one window takes well under a second; 30 s means the engine is gone
  if (-not $task.Wait(30000)) { throw 'the OCR did not answer within 30 s' }
  return $task.Result
}
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($scaled)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
$stream.Dispose()
Remove-Item -LiteralPath $scaled -Force
$lines = @("window pid=$owner path=$path size=${w}x$hh") + @($result.Lines | ForEach-Object { $_.Text })
Set-Content -LiteralPath $txt -Value $lines -Encoding UTF8
