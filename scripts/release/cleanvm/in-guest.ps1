<#
.SYNOPSIS
    The half of the clean-machine gate that runs inside the virtual machine.

.DESCRIPTION
    `run-smoke-in-vm.ps1` on the host copies this in and calls it once per phase.
    It is split into phases rather than written as one run because each phase is
    a separate line on the gate 5 checklist and a separate thing that can fail:
    the host prints which phase it is in, and a failure names itself.

      `unpack`   — extract the release archive and write down what this machine
                   and this binary are, before anything is asked of either.
      `smoke`    — `scripts/release/smoke.ps1`, unchanged, against the extracted
                   `folio.exe`. That script is the gate 2 line; this one does not
                   re-implement any of it.
      `web`      — open a page through `BT_WEB_DEV` and photograph what the seat
                   shows. On a machine with no WebView2 runtime that is the
                   absent-engine card; on one with an engine it is a page or a
                   navigation failure, and the difference between those two cards
                   is the whole point of running this on two machines.
      `notice`   — photograph a message box raised by the program itself, from a
                   launch with no console attached. See the phase for what it can
                   and cannot prove about a crash.
      `explorer` — a folder window showing `folio.exe` with its own icon, and the
                   version resource dumped as text beside it.

    Everything lands under `-Results`, which the host copies back whatever
    happens. Nothing here writes outside `C:\folio-vm`.

.PARAMETER Phase
    Which of the five to run.

.PARAMETER GuestHome
    The working directory in the guest. Defaults to the directory this script is
    in, which is `C:\folio-vm` when the host put it there.

.PARAMETER Zip
    The release archive, for `unpack`. Defaults to the only `.zip` in
    `-GuestHome`.

.PARAMETER TimeoutSeconds
    How long a launch has to put a window on the screen.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('unpack', 'smoke', 'web', 'notice', 'explorer')]
    [string] $Phase,
    [string] $GuestHome,
    [string] $Zip,
    [int] $TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# **The working directory is where this file was put, and it is asserted.** The
# host copies this script to `C:\folio-vm\in-guest.ps1` and passes `-GuestHome
# C:\folio-vm`; when that argument arrives empty — a quoting accident on the way
# through `vmrun`, or a hand-typed re-run that leaves it off — every path below
# silently becomes a relative one and the phase fails somewhere else entirely,
# reporting a `smoke.ps1` that is "not at \scripts\release\smoke.ps1". The
# script's own directory is the same answer by construction, so it is the
# fallback, and an unanswerable location stops the phase here.
if (-not $GuestHome) { $GuestHome = $PSScriptRoot }
if (-not $GuestHome -and $PSCommandPath) { $GuestHome = Split-Path -Parent $PSCommandPath }
if (-not $GuestHome) {
    throw 'in-guest.ps1 has no -GuestHome and cannot tell where it is; pass -GuestHome'
}

$results = Join-Path $GuestHome 'results'
$unpacked = Join-Path $GuestHome 'folio'
$exe = Join-Path $unpacked 'folio.exe'
[IO.Directory]::CreateDirectory($results) | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace Guest -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
[DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
[DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
[DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int index);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
public delegate bool EnumWindowsProc(IntPtr h, IntPtr p);
public struct RECT { public int Left, Top, Right, Bottom; }
'@

# Per-monitor v2 before anything asks a question about pixels — the rule every
# probe process in this repository follows, and the reason `ui-probe` was wrong
# about mouse coordinates for weeks.
[void][Guest.Win32]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))

function Write-Fact {
    param([string] $Name, [string] $Text)
    $path = Join-Path $results $Name
    [IO.File]::WriteAllText($path, $Text, (New-Object Text.UTF8Encoding($false)))
    Write-Host "wrote $path"
}

# **The window a person would Alt-Tab to, and not merely the first one Windows
# lists.** Folio's window is not the only top-level window its process owns:
# winit keeps a permanently visible, unowned 13 x 13 message window of class
# `Winit Thread Event Target`, and that one exists three seconds before the real
# window is created. A search for "visible, unowned, first found" photographs it
# — a 13 x 13 picture, and a `WM_CLOSE` sent to a window that ignores it, which
# is how the `web` phase first came back with a 26 x 26 card and exit code -1.
#
# The discriminator is the one Windows itself uses for the Alt-Tab list rather
# than a size threshold: an application window is a visible unowned top-level
# window without `WS_EX_TOOLWINDOW`. winit's event target carries that bit
# (`ex=0x080800A0`); Folio's window does not (`ex=0x00040110`, and it carries
# `WS_EX_APPWINDOW`).
$script:GwlExStyle = -20
$script:WsExToolWindow = 0x00000080

function Get-TopLevelWindow {
    param([int] $ProcessId)
    # **Script scope, both times.** The callback runs in a scope of its own, so a
    # local assigned inside it is a local of the callback; the answer has to be
    # written and read at the same scope or every search returns zero.
    $script:foundWindow = [IntPtr]::Zero
    $script:wantedProcess = $ProcessId
    [Guest.Win32]::EnumWindows({
            param([IntPtr] $handle, [IntPtr] $unused)
            $owner = 0
            [void][Guest.Win32]::GetWindowThreadProcessId($handle, [ref] $owner)
            if ($owner -eq $script:wantedProcess -and [Guest.Win32]::IsWindowVisible($handle) -and
                [Guest.Win32]::GetWindow($handle, 4) -eq [IntPtr]::Zero -and
                ([Guest.Win32]::GetWindowLongW($handle, $script:GwlExStyle) -band $script:WsExToolWindow) -eq 0) {
                $script:foundWindow = $handle
                return $false
            }
            return $true
        }, [IntPtr]::Zero) | Out-Null
    return $script:foundWindow
}

function Wait-ForWindow {
    param([System.Diagnostics.Process] $Process, [int] $Seconds)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) { break }
        $window = Get-TopLevelWindow -ProcessId $Process.Id
        if ($window -ne [IntPtr]::Zero) { return $window }
        Start-Sleep -Milliseconds 250
    }
    return [IntPtr]::Zero
}

# **Bringing a window to the front from a process that is not in front.**
# Windows refuses a bare `SetForegroundWindow` from a process that does not own
# the foreground, and answers `false` rather than raising anything: the capture
# below then photographs the screen rectangle where the window is, showing
# whatever is on top of it. The documented way round it is to join the input
# queue of the thread that does own the foreground for the length of the call.
# The result is checked rather than assumed, because a photograph of the wrong
# window is worse evidence than no photograph.
function Set-WindowInFront {
    param([IntPtr] $Window)

    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        $foreground = [Guest.Win32]::GetForegroundWindow()
        if ($foreground -eq $Window) { return $true }

        $owner = 0
        $theirs = [Guest.Win32]::GetWindowThreadProcessId($foreground, [ref] $owner)
        $mine = [Guest.Win32]::GetCurrentThreadId()
        $attached = $false
        if ($theirs -ne 0 -and $theirs -ne $mine) {
            $attached = [Guest.Win32]::AttachThreadInput($mine, $theirs, $true)
        }
        [void][Guest.Win32]::BringWindowToTop($Window)
        [void][Guest.Win32]::SetForegroundWindow($Window)
        if ($attached) { [void][Guest.Win32]::AttachThreadInput($mine, $theirs, $false) }

        Start-Sleep -Milliseconds 400
        if ([Guest.Win32]::GetForegroundWindow() -eq $Window) { return $true }
    }
    return $false
}

function Save-Screen {
    param([string] $Name, [IntPtr] $Window = [IntPtr]::Zero)

    # Off the screen, never `PrintWindow`: Folio's pixels are composed by the GPU
    # into a swap chain, and a window asked to paint itself into a device context
    # hands back the blank one it never draws into. `smoke.ps1` says the same
    # thing at its own capture.
    if ($Window -ne [IntPtr]::Zero) {
        if (-not (Set-WindowInFront -Window $Window)) {
            Write-Host "WARNING: $Name was taken while the window was not in front; read the picture before believing it"
        }
        Start-Sleep -Milliseconds 700
        $rect = New-Object Guest.Win32+RECT
        [void][Guest.Win32]::GetWindowRect($Window, [ref] $rect)
        $x = $rect.Left; $y = $rect.Top
        $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    }
    else {
        $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
        $x = $screen.X; $y = $screen.Y; $width = $screen.Width; $height = $screen.Height
    }

    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($x, $y, 0, 0, $bitmap.Size)
    $path = Join-Path $results $Name
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
    Write-Host "wrote $path ($width x $height)"
    return $path
}

function Close-Window {
    param([System.Diagnostics.Process] $Process, [IntPtr] $Window)
    if ($Window -ne [IntPtr]::Zero) {
        [void][Guest.Win32]::PostMessageW($Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
    }
    if (-not $Process.WaitForExit(20000)) { $Process.Kill() }
}

# A run of Folio that reads none of this machine's settings and leaves nothing
# behind — the same seal `smoke.ps1` puts on its own launches.
function Set-SealedEnvironment {
    param([string] $Under)
    $temp = Join-Path $Under 'temp'
    [IO.Directory]::CreateDirectory($temp) | Out-Null
    $env:APPDATA = $Under
    $env:TEMP = $temp
    $env:TMP = $temp
}

switch ($Phase) {

    # ── What this machine is, and what this binary says it is ────────────────
    'unpack' {
        if (-not $Zip) {
            $candidates = @(Get-ChildItem -LiteralPath $GuestHome -Filter *.zip -File)
            if ($candidates.Count -ne 1) {
                throw "expected exactly one .zip in $GuestHome, found $($candidates.Count)"
            }
            $Zip = $candidates[0].FullName
        }
        if (Test-Path -LiteralPath $unpacked) { Remove-Item -LiteralPath $unpacked -Recurse -Force }
        Expand-Archive -LiteralPath $Zip -DestinationPath $unpacked -Force

        # The archive holds one folder holding seven files (`package.ps1`). Lift
        # them so `folio.exe` and its ConPTY sidecar sit where the rest of this
        # script expects, and so the folder photographed in Explorer is the one a
        # person actually gets.
        $inner = @(Get-ChildItem -LiteralPath $unpacked -Directory)
        if ($inner.Count -eq 1 -and -not (Test-Path -LiteralPath $exe)) {
            Get-ChildItem -LiteralPath $inner[0].FullName -Force |
                Move-Item -Destination $unpacked -Force
            Remove-Item -LiteralPath $inner[0].FullName -Recurse -Force
        }
        if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
            throw "the archive did not contain folio.exe: $Zip"
        }

        $os = Get-CimInstance Win32_OperatingSystem
        $info = (Get-Item -LiteralPath $exe).VersionInfo

        # **What engine this machine has, asked before Folio is asked anything.**
        # The registry is indicative and not authoritative — `webhost.rs` records
        # that gate 7 caught the registry claiming a runtime the loader would not
        # produce — so this is written down as evidence and the verdict is left
        # to the card the `web` phase photographs.
        $clients = @(
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
            'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
            'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
        )
        $runtimeVersions = @(
            foreach ($key in $clients) {
                $value = Get-ItemProperty -LiteralPath $key -Name pv -ErrorAction SilentlyContinue
                if ($value) { "$key -> pv=$($value.pv)" }
            }
        )
        $runtimeFolders = @(
            foreach ($base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, $env:LOCALAPPDATA)) {
                if (-not $base) { continue }
                $folder = Join-Path $base 'Microsoft\EdgeWebView\Application'
                if (Test-Path -LiteralPath $folder) {
                    "$folder -> " + (@(Get-ChildItem -LiteralPath $folder -Directory |
                            ForEach-Object Name) -join ', ')
                }
            }
        )

        $lines = @(
            "captured        : $((Get-Date).ToString('yyyy-MM-dd HH:mm:ss zzz'))"
            "machine         : $env:COMPUTERNAME"
            "windows         : $($os.Caption) $($os.Version) build $($os.BuildNumber)"
            "architecture    : $($os.OSArchitecture)"
            "installed       : $($os.InstallDate)"
            ''
            "archive         : $Zip"
            "archive sha256  : $((Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant())"
            "archive bytes   : $((Get-Item -LiteralPath $Zip).Length)"
            ''
            'the archive, as extracted:'
        ) + @(
            Get-ChildItem -LiteralPath $unpacked -File |
                Sort-Object Name |
                ForEach-Object { '  {0,12:N0}  {1}' -f $_.Length, $_.Name }
        ) + @(
            ''
            'folio.exe version resource (what Explorer shows on the Properties page):'
            "  ProductName     : $($info.ProductName)"
            "  ProductVersion  : $($info.ProductVersion)"
            "  FileVersion     : $($info.FileVersion)"
            "  FileDescription : $($info.FileDescription)"
            "  CompanyName     : $($info.CompanyName)"
            "  InternalName    : $($info.InternalName)"
            "  OriginalFilename: $($info.OriginalFilename)"
            "  LegalCopyright  : $($info.LegalCopyright)"
            ''
            'WebView2 runtime, as the registry and the disk describe it:'
        ) + @(
            if ($runtimeVersions.Count -eq 0 -and $runtimeFolders.Count -eq 0) {
                '  none — no EdgeUpdate client key, no EdgeWebView application folder'
            }
            else {
                $runtimeVersions + $runtimeFolders | ForEach-Object { "  $_" }
            }
        )
        Write-Fact -Name 'machine.txt' -Text (($lines -join "`r`n") + "`r`n")
    }

    # ── Gate 2's own script, on the artefact, on this machine ────────────────
    'smoke' {
        $script = Join-Path $GuestHome 'scripts\release\smoke.ps1'
        if (-not (Test-Path -LiteralPath $script -PathType Leaf)) {
            throw "smoke.ps1 is not at $script"
        }
        # Under its own transcript, because the host reads the exception text out
        # of a file when `vmrun` hands back nothing but an exit code.
        $log = Join-Path $results 'smoke.log'
        Start-Transcript -LiteralPath $log -Force | Out-Null
        try {
            & $script -Exe $exe -Artifacts (Join-Path $results 'smoke') -TimeoutSeconds $TimeoutSeconds
        }
        finally { Stop-Transcript | Out-Null }
    }

    # ── The web seat, and whichever card this machine earns ──────────────────
    'web' {
        # `BT_WEB_DEV=<url>` opens a preview seat on that page at launch
        # (`webhost::development_target`). It is the only door into a page that
        # does not need a hand on a mouse, which is what makes this phase
        # scriptable at all.
        #
        # The address is deliberately an ordinary `https` one: it passes the
        # navigation door by syntax, so whatever appears on the seat is about the
        # *engine* and not about a refusal.
        Set-SealedEnvironment -Under (Join-Path $GuestHome 'home-web')
        $env:BT_WEB_DEV = 'https://example.com/'
        $env:BT_PTY_DUMP = Join-Path $results 'web-pty.dump'

        $folio = Start-Process -FilePath $exe -PassThru `
            -RedirectStandardOutput (Join-Path $results 'web.out') `
            -RedirectStandardError (Join-Path $results 'web.err')
        # Windows PowerShell 5.1 never opens the process handle for a redirected
        # `-PassThru` start, and a handle first asked for after the process has
        # gone cannot be had — `.ExitCode` would be blank in `web.txt` below.
        [void] $folio.Handle
        $window = Wait-ForWindow -Process $folio -Seconds $TimeoutSeconds
        if ($window -eq [IntPtr]::Zero) { throw 'the launch opened no visible window' }

        # A card is drawn after the engine has been asked for and has answered.
        # Two seconds is not a guess at how long that takes; it is longer than
        # the answer takes in either direction, because both the loader saying
        # "no runtime" and an environment coming up are local calls.
        Start-Sleep -Seconds 3
        Save-Screen -Name 'webview2-card.png' -Window $window | Out-Null
        Close-Window -Process $folio -Window $window

        Remove-Item Env:BT_WEB_DEV
        Write-Fact -Name 'web.txt' -Text @"
BT_WEB_DEV      : https://example.com/
exit code       : $($folio.ExitCode)
what to read    : which card the seat is showing.
  "Microsoft Edge WebView2 Runtime is not installed." with a "Download the
  runtime" button is the absent-engine card — the one the Windows 10 machine
  must produce.
  A page, or a card naming a host and a WebErrorStatus, means the engine came
  up: that is the Windows 11 machine's answer, and an offline machine gets the
  second of the two.
  A card that says the engine did not start, on a machine that has a runtime,
  is a failure of this gate and not of the network.
"@
    }

    # ── The message box a launch with no console raises ──────────────────────
    'notice' {
        # **What this phase can prove, and what it cannot.**
        #
        # It cannot make a release build panic. `BT_HANG_SELFTEST` is compiled
        # out of release (`hang_watch.rs`: "Release builds do not read
        # BT_HANG_SELFTEST"), and `cli::parse` accepts no flag that raises one —
        # which the first check below states as a fact rather than assuming:
        # `--panic-selftest` must be refused as an unknown flag, and the day
        # somebody adds that entry this line goes red and is rewritten.
        #
        # What it does prove is the *channel*. `announce_panic` and
        # `report_at_the_front_door` are the same two destinations in the same
        # order — a console if this run has one, and `bt_platform::message_box`
        # when it has not — so a refusal raised from a launch with no console
        # attached puts up the same kind of box, from the same call, that a crash
        # would. A double-clicked Folio that dies is not silent.
        $refusal = Start-Process -FilePath $exe -ArgumentList '--panic-selftest' -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput (Join-Path $results 'panic-selftest.out') `
            -RedirectStandardError (Join-Path $results 'panic-selftest.err')
        $entry = switch ($refusal.ExitCode) {
            2 { 'refused as an unknown flag — this build has no controlled panic entry' }
            0 { 'ACCEPTED — this build has grown a panic entry; rewrite this phase' }
            default { "answered $($refusal.ExitCode) — neither a refusal nor an acceptance; look at it" }
        }

        # No console for the child to attach to, which is the whole trick: a
        # shortcut started through Explorer has no parent console, and
        # `write_to_console` answers false exactly there.
        $shortcut = Join-Path $GuestHome 'refusal.lnk'
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        $link.TargetPath = $exe
        $link.Arguments = '--panic-selftest'
        $link.WorkingDirectory = $unpacked
        $link.Save()
        [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null

        Start-Process -FilePath 'explorer.exe' -ArgumentList "`"$shortcut`""
        Start-Sleep -Seconds 6
        Save-Screen -Name 'no-console-notice.png' | Out-Null

        # The box belongs to a process that is waiting to be dismissed. Close it
        # the way a person would, so the phase leaves nothing running.
        $stragglers = @(Get-Process -Name 'folio' -ErrorAction SilentlyContinue)
        foreach ($process in $stragglers) {
            $box = Get-TopLevelWindow -ProcessId $process.Id
            Close-Window -Process $process -Window $box
        }

        Write-Fact -Name 'notice.txt' -Text @"
folio.exe --panic-selftest : exit $($refusal.ExitCode)
  $entry

no-console-notice.png      : the same message box a crash raises, raised here by
  a refusal from a launch Explorer started, which has no console to write to.
  Read in the picture: the box carries the product's name and the refusal names
  the flag. There is no build line in it -- the box says what was refused and how
  to call the program, and the build identity is diagnostics.log's first line.

what is NOT proved here    : a real panic. A release build has no entry that
  raises one. The gate 5 line in docs/plans/release/clean-vm.md carries the
  recommendation that closes this hole (a hidden --panic-selftest flag) and the
  manual trigger to use until it exists.
"@
    }

    # ── The icon and the version resource, as Explorer draws them ────────────
    'explorer' {
        # An icon view, so the shot shows the icon `build.rs` compiled into the
        # PE at a size a reader can judge, rather than a 16-pixel version of it.
        # View mode is a per-folder setting and is asked for through the shell.
        $shell = New-Object -ComObject Shell.Application
        $shell.Open($unpacked)
        Start-Sleep -Seconds 3
        foreach ($window in @($shell.Windows())) {
            # 1 is the icon view in the shell's own numbering. The icon *size*
            # within that view is not part of the automation surface on every
            # Windows, so both of these are attempted and neither is required:
            # a Details-view shot still shows the icon, only smaller, and a
            # failure here must not cost the whole phase its picture.
            try { $window.Document.CurrentViewMode = 1 } catch { }
            try { $window.Document.IconSize = 96 } catch { }
        }
        Start-Sleep -Seconds 2
        Save-Screen -Name 'explorer-icon.png' | Out-Null
        [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null

        # The Properties page cannot be photographed without driving a modal
        # dialog, and a screenshot of it would say no more than this does: the
        # same resource, read from the same file, by the same API Explorer uses.
        $info = (Get-Item -LiteralPath $exe).VersionInfo
        Write-Fact -Name 'explorer.txt' -Text @"
folio.exe in a folder window, extra-large icons: explorer-icon.png
version resource, read from the file Explorer is drawing:
$($info | Format-List * | Out-String)
"@
    }
}

Write-Host "phase $Phase finished"
