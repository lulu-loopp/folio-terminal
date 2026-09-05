<#
.SYNOPSIS
    Start the built `folio.exe` for real and check the six things a build can be
    green and still be broken about.

.DESCRIPTION
    Every other gate in this repository runs inside `cargo test`, which means it
    runs against a library and never against the executable that ships. This one
    runs the artefact:

      1. `--version` answers on the caller's own stdout — captured through a
         pipe, which is the check that separates "it printed something" from
         "a script can read what it printed" — and exits 0.
      2. `--help` exits 0, and a flag this build has not got exits 2 and names
         itself. A packaging script reads those two numbers.
      3. A cold launch opens a window, builds a device, spawns a shell and gets
         text onto the glass. Read out of `BT_STARTUP_TRACE` rather than
         asserted with a screenshot comparison, because each phase is named
         separately and a failure says which one.
      4. The ConPTY it spawned came from the sidecar beside the exe and not from
         the system — the one fact the archive's whole file list exists to make
         true.
      5. The window agrees with Windows about its own DPI: `GetDpiForWindow` on
         the window Folio opened equals `GetDpiForMonitor` for the monitor it
         opened on. Stated as an agreement rather than as "96", so the same
         check is the 200% check on a machine set to 200%.
      6. It shuts down when asked, rather than being killed.

    A picture of the window and every trace file are written to `-Artifacts`
    whatever happens, because the CI run that fails is the one nobody can
    reproduce.

    **The run is sealed off from the machine's own Folio**: `APPDATA` and `TEMP`
    are pointed at a scratch directory, so this reads no settings, restores no
    session and leaves nothing behind.

.PARAMETER Exe
    The `folio.exe` to run. Defaults to `target/release/folio.exe`.

.PARAMETER Artifacts
    Where traces and the screenshot go. Defaults to `target/smoke`.

.PARAMETER TimeoutSeconds
    How long the cold launch has to reach its last phase.

.PARAMETER ExpectSigned
    Also check, before starting anything, that this executable carries a valid,
    time-stamped Authenticode signature naming the holder it says it is
    copyright of. Pass it when the exe under test came out of
    `package.ps1 -Sign`; leave it off otherwise, because an unsigned build is
    what an ordinary `cargo build` produces and this script has to keep working
    on one.

.PARAMETER SignerSubject
    Who the certificate has to name, if not the holder named in the executable's
    own `LegalCopyright`. Only read when `-ExpectSigned` is given.
#>

[CmdletBinding()]
param(
    [string] $Exe,
    [string] $Artifacts,
    [int] $TimeoutSeconds = 90,
    [switch] $ExpectSigned,
    [string] $SignerSubject
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# **Where this file is, spelled the way both editions of PowerShell read.** The
# machine this script has to run on last is a clean Windows, and a clean Windows
# has Windows PowerShell 5.1 and nothing else: `Join-Path` there takes two paths
# and refuses a third, so `Join-Path $PSScriptRoot '..' '..'` is a parameter
# binding error raised before the first line of work — which is how gate 5's
# first smoke died, with `A positional parameter cannot be found that accepts
# argument '..'`. Nested two-argument calls are the one spelling 5.1 and 7 read
# the same way.
#
# The root is asserted rather than allowed to be empty, too: an empty
# `$PSScriptRoot` turns every path below into a relative one, and the failure
# then arrives later and somewhere else, as a file that is "not at
# \scripts\release\smoke.ps1".
$here = $PSScriptRoot
if (-not $here -and $PSCommandPath) { $here = Split-Path -Parent $PSCommandPath }
if (-not $here) {
    throw 'smoke.ps1 cannot tell where it is; run it as a file (-File, or &), not from a pasted body'
}
$root = (Resolve-Path (Join-Path (Join-Path $here '..') '..')).Path
if (-not $Exe) { $Exe = Join-Path $root 'target\release\folio.exe' }
if (-not $Artifacts) { $Artifacts = Join-Path $root 'target\smoke' }
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "no folio.exe at $Exe" }

[System.IO.Directory]::CreateDirectory($Artifacts) | Out-Null

Add-Type -Namespace Smoke -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int index);
[DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int t, out uint x, out uint y);
public delegate bool EnumWindowsProc(IntPtr h, IntPtr p);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

# Every probe process in this repository declares per-monitor v2 before it asks
# a question about pixels. A probe left at the default awareness is handed
# virtualised coordinates by Windows and reports them confidently.
[void][Smoke.Win32]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))

# **Folio's window is not the only top-level window its process owns.** winit
# keeps a permanently visible, unowned 13 x 13 message window of class `Winit
# Thread Event Target`, created seconds before the real one. A search for
# "visible, unowned" alone finds whichever of the two is higher in the Z-order,
# which is a coin this script has been winning rather than a rule it follows.
# The rule is the one Windows uses for the Alt-Tab list: an application window
# has no `WS_EX_TOOLWINDOW`. winit's event target has it; Folio's window has
# `WS_EX_APPWINDOW` instead.
$GwlExStyle = -20
$WsExToolWindow = 0x00000080

function Invoke-Folio {
    param([string[]] $Arguments)

    $out = Join-Path $Artifacts ('cli-' + ($Arguments -join '_' -replace '[^\w.-]', '') + '.out')
    $err = "$out.err"
    $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError $err
    # **`-Encoding UTF8` on every read of something Folio wrote.** Folio writes
    # UTF-8 without a byte-order mark; PowerShell 7 assumes that and Windows
    # PowerShell 5.1 assumes the machine's ANSI code page, so the same file read
    # by the same line comes back as mojibake on the clean machine. It was the
    # `diagnostics.log` header, quoted into the evidence transcript as
    # `â”€â”€ Folio 0.1.0`, that showed it.
    return [pscustomobject]@{
        ExitCode = $process.ExitCode
        StdOut   = (Get-Content -LiteralPath $out -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
        StdErr   = (Get-Content -LiteralPath $err -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
    }
}

$expectedVersion = (Get-Item -LiteralPath $Exe).VersionInfo.ProductVersion.Trim()

# ── 0: it is signed, and signed by whoever it says it belongs to ─────────────
#
# **Asked of the file rather than of a script's memory of what it signed.** The
# name to expect is read out of the executable's own `LegalCopyright` — the
# holder the two licences and `crates/bt-app/build.rs` already agree on — so this
# check has no second copy of that name to drift away from, and a certificate
# issued to somebody else fails it whatever the signing script believed.
#
# The time stamp is checked as hard as the signature: an Artifact Signing
# certificate is valid for three days, and a signature made without `/tr` passes
# every check for three days and then stops passing them on a machine nobody here
# is sitting at.
if ($ExpectSigned) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Exe
    if ($signature.Status -ne 'Valid') {
        throw "the signature on $Exe is $($signature.Status): $($signature.StatusMessage)"
    }
    if (-not $signature.TimeStamperCertificate) {
        throw 'the signature carries no time stamp, so it expires with the certificate that made it'
    }

    $holder = $SignerSubject
    if (-not $holder) {
        $copyright = (Get-Item -LiteralPath $Exe).VersionInfo.LegalCopyright
        if ($copyright -notmatch '(?i)copyright\s*\(c\)\s*\d{4}\s+(.+?)\s+and\b') {
            throw "the executable's LegalCopyright is '$copyright'; no holder can be read out of it"
        }
        $holder = $Matches[1]
    }
    $subject = $signature.SignerCertificate.Subject
    if ($subject -notlike "*$holder*") {
        throw "the certificate says $subject; the executable says it is copyright $holder"
    }
    Write-Host "signed by: $subject"
    Write-Host "stamped by: $($signature.TimeStamperCertificate.Subject)"
}

# ── 1 and 2: the front door ──────────────────────────────────────────────────

$version = Invoke-Folio @('--version')
if ($version.ExitCode -ne 0) { throw "--version exited $($version.ExitCode)" }
$line = ($version.StdOut | Out-String).Trim()
if (-not $line) {
    throw '--version wrote nothing a caller could capture (it went to the console screen, not to stdout)'
}
if ($line -notmatch "^Folio\s+$([regex]::Escape($expectedVersion))\s+\(") {
    throw "--version said '$line'; the executable's own resources say $expectedVersion"
}
Write-Host "--version: $line"

$help = Invoke-Folio @('--help')
if ($help.ExitCode -ne 0) { throw "--help exited $($help.ExitCode)" }
if (($help.StdOut | Out-String) -notmatch '--cwd') { throw '--help printed no usage block' }

$refused = Invoke-Folio @('--nope')
if ($refused.ExitCode -ne 2) { throw "an unknown flag exited $($refused.ExitCode), expected 2" }
if (($refused.StdOut | Out-String) -notmatch '--nope') { throw 'a refusal did not name the flag' }
Write-Host 'the front door answers --help and refuses what it has not got.'

# ── 3 and 4: a cold launch, in a home of its own ─────────────────────────────

$home_ = Join-Path $Artifacts 'home'
if (Test-Path -LiteralPath $home_) { Remove-Item -LiteralPath $home_ -Recurse -Force }
[System.IO.Directory]::CreateDirectory((Join-Path $home_ 'temp')) | Out-Null

$trace = Join-Path $Artifacts 'startup.trace'
$stdout = Join-Path $Artifacts 'startup.out'
$environment = @{
    APPDATA         = $home_
    TEMP            = (Join-Path $home_ 'temp')
    TMP             = (Join-Path $home_ 'temp')
    BT_STARTUP_TRACE = '1'
    # The project's own test windows always carry this, and here it is also the
    # artefact: what the shell actually said, for a failure that has to be
    # diagnosed from a log.
    BT_PTY_DUMP     = (Join-Path $Artifacts 'pty.dump')
}
foreach ($name in $environment.Keys) {
    Set-Item -Path "Env:$name" -Value $environment[$name]
}

$folio = Start-Process -FilePath $Exe -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $trace
# **Touching `.Handle` is what makes `.ExitCode` answerable later.** Windows
# PowerShell 5.1 hands back a `Process` for a redirected `-PassThru` start that
# has never opened the process handle, and a handle first asked for after the
# process has gone cannot be had: `.ExitCode` then answers `$null` for the rest
# of the run, and check 6 below reads "folio exited " with nothing after it.
# PowerShell 7 caches it on its own; asking here costs nothing there.
[void] $folio.Handle

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$window = [IntPtr]::Zero
$seen = ''
while ((Get-Date) -lt $deadline) {
    if ($folio.HasExited) { break }
    $seen = (Get-Content -LiteralPath $trace -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
    if ($seen -and $seen -match 'BT_STARTUP first_text_present=') { break }
    Start-Sleep -Milliseconds 250
}

# The picture and the window facts, while it is still up.
[Smoke.Win32]::EnumWindows({
        param([IntPtr] $handle, [IntPtr] $unused)
        $pid_ = 0
        [void][Smoke.Win32]::GetWindowThreadProcessId($handle, [ref] $pid_)
        if ($pid_ -eq $folio.Id -and [Smoke.Win32]::IsWindowVisible($handle) -and
            [Smoke.Win32]::GetWindow($handle, 4) -eq [IntPtr]::Zero -and
            ([Smoke.Win32]::GetWindowLongW($handle, $script:GwlExStyle) -band $script:WsExToolWindow) -eq 0) {
            $script:window = $handle
            return $false
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null

# **Bringing a window to the front from a process that is not in front.**
# Windows refuses a bare `SetForegroundWindow` from a process that does not own
# the foreground, and answers `false` rather than raising: the capture below
# then photographs the rectangle where the window is, showing whatever sits on
# top of it. Joining the foreground thread's input queue for the length of the
# call is the documented way round it, and the result is checked rather than
# assumed — a picture of the wrong window is worse than no picture.
function Set-WindowInFront {
    param([IntPtr] $Window)

    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        $foreground = [Smoke.Win32]::GetForegroundWindow()
        if ($foreground -eq $Window) { return $true }

        $owner = 0
        $theirs = [Smoke.Win32]::GetWindowThreadProcessId($foreground, [ref] $owner)
        $mine = [Smoke.Win32]::GetCurrentThreadId()
        $attached = $false
        if ($theirs -ne 0 -and $theirs -ne $mine) {
            $attached = [Smoke.Win32]::AttachThreadInput($mine, $theirs, $true)
        }
        [void][Smoke.Win32]::BringWindowToTop($Window)
        [void][Smoke.Win32]::SetForegroundWindow($Window)
        if ($attached) { [void][Smoke.Win32]::AttachThreadInput($mine, $theirs, $false) }

        Start-Sleep -Milliseconds 400
        if ([Smoke.Win32]::GetForegroundWindow() -eq $Window) { return $true }
    }
    return $false
}

$picture = Join-Path $Artifacts 'window.png'
if ($window -ne [IntPtr]::Zero) {
    if (-not (Set-WindowInFront -Window $window)) {
        Write-Host 'WARNING: window.png was taken while the window was not in front; read it before believing it'
    }
    Start-Sleep -Milliseconds 600
    $rect = New-Object Smoke.Win32+RECT
    [void][Smoke.Win32]::GetWindowRect($window, [ref] $rect)
    Add-Type -AssemblyName System.Drawing
    $bitmap = New-Object System.Drawing.Bitmap(
        ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    # Off the screen and not `PrintWindow`: this window's pixels are composed by
    # the GPU into a swap chain, and a window that is asked to paint itself into
    # a device context hands back the blank one it never draws into.
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($picture, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
    Write-Host "window picture: $picture"
}

# **Asked while the window is alive.** `GetDpiForWindow` answers `0` for a
# handle that has gone, so a check made after the shutdown below would be two
# zeroes agreeing with each other for the rest of this product's life.
$windowDpi = 0
$monitorDpi = 0
if ($window -ne [IntPtr]::Zero) {
    $windowDpi = [Smoke.Win32]::GetDpiForWindow($window)
    $monitor = [Smoke.Win32]::MonitorFromWindow($window, 2) # MONITOR_DEFAULTTONEAREST
    $y = 0
    [void][Smoke.Win32]::GetDpiForMonitor($monitor, 0, [ref] $monitorDpi, [ref] $y) # MDT_EFFECTIVE_DPI
}

# Shut it the way a person does, and give it the time a clean quit takes.
if (-not $folio.HasExited -and $window -ne [IntPtr]::Zero) {
    [void][Smoke.Win32]::PostMessageW($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
}
$closed = $folio.WaitForExit(20000)

$seen = (Get-Content -LiteralPath $trace -Raw -Encoding UTF8 -ErrorAction SilentlyContinue)
if (-not $seen) { throw "the launch wrote no startup trace; see $trace" }
Write-Host $seen

foreach ($phase in @('runtime_ready=', 'background_visible=', 'first_text_present=')) {
    if ($seen -notmatch [regex]::Escape($phase)) {
        throw "the launch never reached $phase — see $trace"
    }
}
# `source=sidecar`, and never `source=system`: falling back to the inbox ConPTY
# is precisely the failure the archive's file list exists to prevent, and it is
# invisible — the shell starts either way.
if ($seen -notmatch 'conpty_sources=\["source=sidecar') {
    throw "the shell did not start on the sidecar ConPTY beside the exe — see $trace"
}
Write-Host 'a cold launch reached first text, on the sidecar ConPTY.'

# ── 5: the window and Windows agree about the DPI ────────────────────────────

if ($window -eq [IntPtr]::Zero) { throw 'the launch opened no visible top-level window' }
# Zero is what a dead handle and an unaware process both answer, so it is
# refused before the comparison rather than compared.
if ($windowDpi -le 0 -or $monitorDpi -le 0) {
    throw "no dpi was read: window $windowDpi, monitor $monitorDpi"
}
if ($windowDpi -ne $monitorDpi) {
    throw "the window reports $windowDpi dpi on a monitor Windows calls $monitorDpi"
}
Write-Host ("dpi: window and monitor agree at {0} ({1:P0} scaling)." -f $windowDpi, ($windowDpi / 96))

# ── 6 ────────────────────────────────────────────────────────────────────────

if (-not $closed) { throw 'the window did not close when it was asked to' }
if ($folio.ExitCode -ne 0) { throw "folio exited $($folio.ExitCode)" }
Write-Host 'it closed when asked, and exited 0.'

# ── 7: the log an ordinary run leaves says which build left it ───────────────
#
# A second launch, and a second one is the only way: `BT_STARTUP_TRACE` keeps
# the console, which is exactly the case in which `diagnostics.log` is not the
# destination. The run above proved the phases; this one proves the file a bug
# report will actually arrive as.

Remove-Item Env:BT_STARTUP_TRACE
$plain = Start-Process -FilePath $Exe -PassThru `
    -RedirectStandardOutput (Join-Path $Artifacts 'plain.out') `
    -RedirectStandardError (Join-Path $Artifacts 'plain.err')
[void] $plain.Handle

$log = Join-Path $home_ 'Folio\diagnostics.log'
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
while ((Get-Date) -lt $deadline -and -not (Test-Path -LiteralPath $log)) {
    if ($plain.HasExited) { break }
    Start-Sleep -Milliseconds 250
}
$second = [IntPtr]::Zero
[Smoke.Win32]::EnumWindows({
        param([IntPtr] $handle, [IntPtr] $unused)
        $pid_ = 0
        [void][Smoke.Win32]::GetWindowThreadProcessId($handle, [ref] $pid_)
        if ($pid_ -eq $plain.Id -and [Smoke.Win32]::IsWindowVisible($handle) -and
            [Smoke.Win32]::GetWindow($handle, 4) -eq [IntPtr]::Zero -and
            ([Smoke.Win32]::GetWindowLongW($handle, $script:GwlExStyle) -band $script:WsExToolWindow) -eq 0) {
            $script:second = $handle
            return $false
        }
        return $true
    }, [IntPtr]::Zero) | Out-Null
if ($second -ne [IntPtr]::Zero) {
    [void][Smoke.Win32]::PostMessageW($second, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
}
[void]$plain.WaitForExit(20000)
if (-not $plain.HasExited) { $plain.Kill() }

if (-not (Test-Path -LiteralPath $log)) { throw "no diagnostics.log under $home_" }
Copy-Item -LiteralPath $log -Destination (Join-Path $Artifacts 'diagnostics.log') -Force
$header = (Get-Content -LiteralPath $log -TotalCount 1 -Encoding UTF8)
if (-not ($header.Contains($line) -and $header.Contains('run started'))) {
    throw "diagnostics.log opens with '$header'; expected the same build line --version printed"
}
Write-Host "diagnostics.log: $header"
