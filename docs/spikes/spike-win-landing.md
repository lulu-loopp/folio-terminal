# Spike — the Windows landing block

Five ways Folio should stop being a window that happens to run on Windows and start being a
Windows application: a notification when work finishes, progress on its own taskbar button, a
place in the "Default terminal application" list, a verb in Explorer's context menu, and an honest
answer to "run as administrator".

Everything marked **PROBED** was run on this machine — Windows 11 Pro 26200, unpackaged `.exe`,
`windows` crate 0.62.2, the same binding crate `bt-platform` already depends on. Probe sources are
under `artifacts/win-landing/probes/`, screenshots under `artifacts/win-landing/`. Every registry key any probe wrote has been deleted
again; §7 lists the teardown and shows the machine verified clean.

---

## 0. What the repository already has, and what it does not

Four facts set the size of everything below. Each one moved at least one estimate.

**OSC 9;4 is already parsed.** `bt_term::ProgressState` — `Normal(u8)`, `Error(Option<u8>)`,
`Indeterminate`, `Paused(Option<u8>)` (`crates/bt-term/src/session.rs:672-679`) — reaches
`SessionStatus.progress` and already drives the tab ring. Item 2 is not "implement OSC 9;4"; it is
"give a value that already flows a second consumer".

**The HWND bridge already exists.** `fn window_hwnd(window: &Window) -> Result<NonZeroIsize>`
(`crates/bt-app/src/main.rs:33708-33714`) is called ~18 times and feeds every `bt_platform`
constructor. A taskbar wrapper plugs into the existing pattern with no new plumbing.

**`folio.exe` parses no command line at all.** `fn main()` (`main.rs:34276-34285`) is five lines:
panic hook, event loop, `FolioApp::new(proxy)`, `run_app`. No `clap`, no `std::env::args()`. Runtime
inputs are `BT_*` environment variables read via `env::var_os`. Items 1, 3 and 4 all need to hand a
starting or running process a piece of information. **A command-line front door is the shared
dependency of three of the five items and does not exist yet.** Slice it first, once.

**The app is single-window, deliberately.** `Runtime` holds `tabs: Vec<TabState>` and
`active_tab: usize` (`main.rs:3273-3278`); exactly one `create_window` call exists
(`main.rs:11939`); `winit::window::WindowId` is imported only to satisfy the `ApplicationHandler`
signature and is never stored. `bt-persist` states it as policy: `SessionV1.window` is singular,
"v1 is single-window (the field is named `window`, singular, deliberately not `windows: []`)"
(`crates/bt-persist/src/session.rs:132-134`), and multi-window is a `schema_version` bump.

That last fact is *good news* for this block. "Per-window taskbar progress" is today just "fold
over `self.tabs`", and there is no window-routing problem in a toast click. It is also a constraint:
nothing here should pre-build multi-window machinery, and `CONVENTIONS.md` forbids speculative
persisted fields — a field must arrive in the same change as its reader.

There is also **no single-instance mechanism** (no named mutex, pipe, or `WM_COPYDATA`). A second
`folio.exe` is today an unrelated process.

---

## 1. System notifications

### Verdict

**Fully feasible, unpackaged, with HKCU registry keys only — no Start-menu shortcut, no MSIX.**
Click-back into the *already running* process works. This was the item with the most doubt going in
and the probe settled it in both directions.

### Mechanism

Windows keys notifications on an **AppUserModelID**. An unpackaged process claims one by calling
`SetCurrentProcessExplicitAppUserModelID` and registering the identity under
`HKCU\Software\Classes\AppUserModelId\<AUMID>`. Clicks return through a COM class named by
`CustomActivator`, implemented as `INotificationActivationCallback` and registered as a per-user
`LocalServer32`. One registration serves both cases: while the process runs it is called in-process
(because it registered its class object with `CoRegisterClassObject`); when it is not running, COM
cold-launches the exe from `LocalServer32` with `-Embedding`.

### Exact registry keys

```
HKCU\Software\Classes\AppUserModelId\Folio.Terminal
    DisplayName         REG_SZ  "Folio"                       ; sender name on the toast + Action Center
    IconUri             REG_SZ  "C:\...\folio.ico"            ; absolute path on disk; no URL, no resource id
    IconBackgroundColor REG_SZ  "FF202020"                    ; optional; Settings page tile only
    CustomActivator     REG_SZ  "{9F1B8D21-...-6A2E5C81F704}" ; braces required

HKCU\Software\Classes\CLSID\{9F1B8D21-...-6A2E5C81F704}
    (Default)  REG_SZ  "Folio Toast Activator"
HKCU\Software\Classes\CLSID\{9F1B8D21-...-6A2E5C81F704}\LocalServer32
    (Default)  REG_SZ  "\"C:\...\folio.exe\""                 ; Windows appends -Embedding itself
```

Those are the probe's values; ship a minted GUID and keep it stable forever — it is the identity
the user's notification settings are filed under.

### Rust surface (`windows` 0.62.2) — all present

- `Win32::UI::Notifications::{INotificationActivationCallback, INotificationActivationCallback_Impl,
  NOTIFICATION_USER_INPUT_DATA}` — feature `Win32_UI_Notifications`.
- `UI::Notifications::{ToastNotification, ToastNotificationManager}`, `Data::Xml::Dom::XmlDocument` —
  features `UI_Notifications`, `Data_Xml_Dom`.
- Three build traps worth recording, all hit during this spike: in 0.62 **`implement` is no longer a
  feature of the `windows` crate** (it is unconditional in `windows-core`), so
  `features = ["implement"]` fails to resolve — add `windows-core` as a direct dependency instead;
  `RegCreateKeyExW` is gated behind `Win32_Security`, not `Win32_System_Registry`; and
  `RegSetValueExW` takes `HKEY`, not `Option<HKEY>`.

### PROBED — what was run and what happened

`probes/toast-probe` (console exe; verbs `register` / `show` / `-Embedding` / `unregister` /
`dumpkeys`), driven by `probes/toast-run.ps1`, which starts the probe and photographs the screen on
a loop — a toast lives about five seconds, shorter than a fresh PowerShell takes to start, which is
why the first attempt caught an empty desktop and briefly looked like a failure.

**The toast appears, with no shortcut anywhere.** `artifacts/win-landing/shot-01-toast.png`: attribution reads
**Folio** with the icon `IconUri` pointed at, both text lines, both action buttons. Nothing but the
HKCU keys above was ever written; no `.lnk` was created.

**The platform accepts the identity.** On first `Show()` Windows itself created
`HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Folio.Terminal` with
`LastNotificationAddedTime` and `PeriodicNotificationCount`. Worth knowing: on the *very first* run
`notifier.Setting()` returned `0x80070490 "Element not found"` because that platform entry did not
exist yet; from the second run it returned `NotificationSetting(0)` = `Enabled`. **Do not treat a
failing `Setting()` as "notifications are off" on first launch** — it is a cold-cache answer and
`Show()` succeeds regardless.

**Action Center persistence.** `artifacts/win-landing/shot-02-actioncenter.png`: grouped under a **Folio** heading
with its icon, beside the machine's other senders.

**Click-back, warm path.** Process running, class object registered, clicking the toast body:

```
class factory: CreateInstance called
*** COM ACTIVATE *** aumid="Folio.Terminal" invokedArgs="action=focusTab&window=3&tab=7" inputCount=0
*** IN-PROCESS Activated *** arguments="action=focusTab&window=3&tab=7"
```

Both fire, COM first. The toast XML's `launch` attribute is what returns as `invokedArgs` — that is
the channel for routing identity.

**Click-back, cold path.** The probe was allowed to exit, leaving the toast in Action Center.
Clicking it there made Windows launch a new process from the HKCU `LocalServer32` value:

```
[pid 70412] launched with -Embedding (COM cold activation)
[pid 70412] *** COM ACTIVATE *** aumid="Folio.Terminal" invokedArgs="action=focusTab&window=3&tab=7"
```

This result does double duty: it is direct evidence that **a plain HKCU `LocalServer32` COM server
is cold-activated out-of-process by the system**, which is the same mechanism item 3 depends on.

### What this settles

Microsoft's API reference for `INotificationActivationCallback::Activate` still says "you also will
need to create a shortcut on the start menu", and the older Win32 toast tutorial says a shortcut is
mandatory outright. **On Windows 11 26200 both statements are stale.** No Rust crate implements
this: `tauri-winrt-notification` and `notify-rust` do no registration at all and their documented
fallback is to borrow *PowerShell's* AUMID (so toasts claim to come from PowerShell);
`win-toast-notify` tells the caller to write the registry by hand; the C++ `WinToast` still defaults
to creating a shortcut. The reference implementation to port from is C#:
`ToastNotificationManagerCompat.cs` in `CommunityToolkit.WinUI.Notifications`.

### Where it hangs off in this codebase

The completion signal already exists. OSC `133;D;<exit_code>` produces
`ShellIntegrationMarker::CommandFinished { exit_code }` (`crates/bt-term/src/session.rs:2857-2867`),
clearing `working` and setting `failure_exit_code`. `SessionFacts::claim` (`main.rs:6896-6908`)
already computes `work_in_flight = working || progress.is_some()` and
`unread = !work_in_flight && has_unseen_output()`. **The toast trigger is the `work_in_flight`
true→false transition while the tab is not active** — the same condition that today only lights a
dot in the tab strip.

One caveat to state in the design record: `cmd.exe`'s profile deliberately emits no OSC 133 at all
(only OSC 7) — see the long justification at `crates/bt-app/src/profiles.rs:410-452`. **`cmd.exe`
tabs therefore have no finish signal and can never raise this notification.** That is a design
consequence, not a bug to fix here.

### Size and risks

**M in `bt-platform` (COM/registry/toast bridge) + S in `bt-app` (trigger and routing).** The COM
plumbing — `IClassFactory`, `CoRegisterClassObject`, the callback — is real but bounded; the probe
is a working ~330-line model. Risks: the activator runs on a COM thread and must not touch renderer
state (push onto the existing `EventLoopProxy<AppEvent>` and return); `DisplayName`/`IconUri`
changes are suspected to be cached by the notification platform (unconfirmed; an explorer restart is
the usual remedy — relevant during development, not in the field); and full uninstall must delete
the platform's own `Notifications\Settings\<AUMID>` key, not just ours.

---

## 2. Taskbar progress

### Verdict

**Fully feasible and the cheapest item in the block.** The value is already in the process, the HWND
accessor already exists, and the aggregation is a fold over one `Vec`.

### Mechanism

`ITaskbarList3` on the top-level HWND: `CoCreateInstance(TaskbarList)` → `HrInit()` →
`SetProgressState(hwnd, TBPF_*)` / `SetProgressValue(hwnd, completed, total)`.
`SetOverlayIcon(hwnd, hicon, desc)` badges the button's corner for an "attention" state.

**The `TaskbarButtonCreated` requirement is real and is the classic silent failure.** The shell
announces each top-level window's button via a message from
`RegisterWindowMessageW(L"TaskbarButtonCreated")`; calls made before it arrives do nothing. It is
**re-sent when `explorer.exe` restarts**, so the handler must *re-apply* current state, not merely
initialise once.

### Mapping — a total function of an existing type

| `bt_term::ProgressState` | `TBPFLAG` | value |
|---|---|---|
| `Normal(p)` | `TBPF_NORMAL` | `p` / 100 |
| `Error(Some(p))` / `Error(None)` | `TBPF_ERROR` | `p` / 100, else leave last |
| `Paused(Some(p))` / `Paused(None)` | `TBPF_PAUSED` | `p` / 100, else leave last |
| `Indeterminate` | `TBPF_INDETERMINATE` | — |
| `None` | `TBPF_NOPROGRESS` | — |

### PROBED — what was run and what happened

`probes/taskbar-probe` creates a real `WS_OVERLAPPEDWINDOW`, handles `TaskbarButtonCreated`, then
walks every state on a timer; `probes/taskbar-run.ps1` photographs the button at each step. Two
incidental findings kept the probe honest: this machine's taskbar is **auto-hidden** (its
`Shell_TrayWnd` rect is `0,1798-2880,1894` — parked a row below an 1800px screen), so the capture
loop parks the cursor on the bottom edge to hold it revealed; and the first draft armed seven
`SetTimer`s of different periods, which *repeat*, so by t=18s four fired in the same second and every
state but the last was overwritten before it could be photographed. One repeating timer driving a
step counter fixed it.

Every call returned `Ok(())`, and each state is visible:

| state | file | appearance |
|---|---|---|
| `TBPF_NORMAL` 40% | `artifacts/win-landing/taskbar-state-normal40.png` | blue bar, ~40% filled |
| `TBPF_INDETERMINATE` | `artifacts/win-landing/taskbar-state-indeterminate.png` | sweeping green |
| `TBPF_ERROR` 70% | `artifacts/win-landing/taskbar-state-error70.png` | red bar |
| `TBPF_PAUSED` 55% | `artifacts/win-landing/taskbar-state-paused55.png` | amber bar |
| `+ SetOverlayIcon` | `artifacts/win-landing/taskbar-state-overlay.png` | yellow warning badge on the corner, bar still blue |
| `TBPF_NOPROGRESS` | `artifacts/win-landing/taskbar-state-noprogress.png` | plain button |

Log: `probes/taskbar-probe-log.txt`.

### Where it lands in this codebase

`TabState::fleet_progress` (`main.rs:10025-10029`) already reduces one tab's panes to a single
`Option<ProgressState>`, and its doc comment is explicit that the rule is **"the first one reported,
in seat order"** — a deterministic pick, deliberately not a mean, sum, or max
(`main.rs:10013-10024`). The window-level aggregate is the missing sibling and should obey the same
discipline rather than invent a new one: a `Runtime::window_progress(&self) -> Option<ProgressState>`
folding `self.tabs.iter().filter_map(TabState::fleet_progress)` with the same first-in-order pick.
Because the app is single-window, "per window" is simply "all tabs", with no routing.

If the design record wants severity to win over order (an errored tab outranking an earlier normal
one), that is a legitimate choice — but it is a *new* rule that contradicts `fleet_progress`'s
stated one, so it belongs in DESIGN.md with a reason, not quietly in the fold.

### Size and risks

**S in `bt-platform`, S in `bt-app`.** Follow the crate's existing shape: a `pub struct TaskbarProgress`
inside `mod windows_impl` with `pub fn new(hwnd: NonZeroIsize) -> Result<Self, String>`, added to the
`pub use windows_impl::{ ... }` re-export list, with `Win32_UI_Shell` (already present) in
`Cargo.toml`. Risks: `TaskbarButtonCreated` ordering (handled); COM apartment discipline
(`ITaskbarList3` is apartment-threaded — keep it on the UI thread that created it); re-applying state
after an explorer restart. Note that `bt-app` does not run a raw wndproc of its own for this, so
receiving `TaskbarButtonCreated` needs the same subclassing route `CustomWindowFrame` already uses.

---

## 3. Register as the default terminal

### Verdict

**Split.** The delegation itself is reachable unpackaged. Appearing in the Settings dropdown is
**not** — that list is built exclusively from MSIX app-extension declarations. A third obstacle,
below, is the real cost.

### The registry contract

```
HKCU\Console\%%Startup            ; literal doubled percent in the key name
    DelegationConsole   REG_SZ  "{CLSID}"   ; the console (conhost replacement) side
    DelegationTerminal  REG_SZ  "{CLSID}"   ; the terminal side
```

Written as `StringFromCLSID` output (braces, 38 chars + NUL), read back with `IIDFromString`.
Per-user only; no HKLM or policy equivalent. `{00000000-...}` means "let Windows decide";
`{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}` forces legacy conhost.

**Confirmed on this machine.** These keys hold exactly the Windows Terminal Stable pair —
`DelegationConsole = {2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}`,
`DelegationTerminal = {E12CFF52-A866-4C77-9A90-F570A7AA2C6B}` — matching values read independently
out of the `microsoft/terminal` sources. The contract is real and current.

| channel | `DelegationConsole` | `DelegationTerminal` |
|---|---|---|
| Stable | `{2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}` | `{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}` |
| Preview | `{06EC847C-C0A5-46B8-92CB-7C92F6E35CD5}` | `{86633F1F-6454-40EC-89CE-DA4EBA977EE2}` |
| Canary | `{A854D02A-F2FE-44A5-BB24-D03F4CF830D4}` | `{1706609C-A4CE-4C0D-B7D2-C19BF66398A5}` |

### The COM interfaces

```
IConsoleHandoff        {E686C757-9A35-4A1C-B3CE-0BCC8B5C69F4}
ITerminalHandoff       {59D55CCE-FC8A-48B4-ACE8-0A9286C6557F}   deprecated
ITerminalHandoff2      {AA6B364F-4A50-4176-9002-0AE755E7B5EF}   adds TERMINAL_STARTUP_INFO by value
ITerminalHandoff3      {6F23DA90-15C5-4203-9DB0-64E73F1B1B00}   current — the only one to implement
IDefaultTerminalMarker {746E6BC0-AB05-4E38-AB14-71E86763141F}   marker, no methods
OpenConsoleProxy       {3171DE52-6EFA-4AEF-8A9F-D02BD67E7A4F}   the proxy/stub CLSID
```

```idl
interface ITerminalHandoff3 : IUnknown {
    HRESULT EstablishPtyHandoff(
        [out, system_handle(sh_pipe)]    HANDLE* in,        // terminal creates these two, so it
        [out, system_handle(sh_pipe)]    HANDLE* out,       //   owns pipe mode and buffer size
        [in,  system_handle(sh_pipe)]    HANDLE  signal,    // ConPTY signal channel (resize, ctrl)
        [in,  system_handle(sh_file)]    HANDLE  reference, // lifetime handle for the session
        [in,  system_handle(sh_process)] HANDLE  server,    // the console server process
        [in,  system_handle(sh_process)] HANDLE  client,    // the app that was launched
        [in]  const TERMINAL_STARTUP_INFO* startupInfo);    // title, icon, geometry, show cmd
};
```

The delegated terminal **receives** the ConPTY handles — it does not spawn its own ConPTY. The inbox
conhost stays resident for the session's life because it alone holds the OS-granted signal privilege.

### Why unpackaged cannot appear in Settings

`DelegationConfig::s_GetAvailablePackages` does not enumerate `HKCR\CLSID`. It opens a WinRT
`Windows.ApplicationModel.AppExtensions.AppExtensionCatalog` for two hardcoded categories —
`com.microsoft.windows.console.host` and `com.microsoft.windows.terminal.host` — and reads each
candidate's CLSID from a `<uap3:Properties><Clsid>` element in its **appx manifest**.
`AppExtensionCatalog` only enumerates installed packages with package identity. No code path would
ever see an unpackaged exe; the design spec says as much — WinRT cannot be exposed outside the
package context, so the in-box conhost cannot find it.

**So Folio can make itself the default by writing the keys, but can never be an option the user
picks in Settings.** Setting it from Folio's own settings UI is the only route, and it must be
honest about being a switch the user reverses inside Folio, not in Windows.

### The real cost: marshalling

Activation is ordinary COM — `srvinit.cpp` calls plain
`CoCreateInstance(clsid, nullptr, CLSCTX_LOCAL_SERVER, IID_PPV_ARGS(&handoff))`, resolved through the
standard `LocalServer32` lookup, which works identically for a hand-written HKCU key. **Item 1's cold
activation is a live demonstration of exactly that mechanism working unpackaged.**

But `EstablishPtyHandoff`'s parameters are `system_handle`-attributed, requiring an NDR proxy/stub —
`OpenConsoleProxy.dll`, CLSID `{3171DE52-...}`. **On this machine that registration does not exist in
the plain registry at all**: both `HKCU\Software\Classes` and `HKLM\SOFTWARE\Classes` were checked for
`Interface\{6F23DA90-...}` and `CLSID\{3171DE52-...}` and both are absent, while Windows Terminal
1.24.11911.0 is installed and is the active delegate. Its proxy/stub lives in the MSIX package's
virtualised registry.

Folio would have to ship and register its own copy (the IDL is MIT-licensed in `microsoft/terminal`)
under `HKCU\Software\Classes\CLSID\{3171DE52-...}\InprocServer32` plus `Interface\{IID}\ProxyStubClsid32`
for each interface — or hand-roll a marshaller. **This is what turns the item from "a day" into "a
project"**, and it is undocumented territory: these interfaces are not a public contract and can
change between Windows builds.

### NOT PROBED, deliberately

A live handoff probe means pointing `DelegationTerminal` at a Folio CLSID. If anything is wrong,
**every console application the user launches** goes to a broken handler. That is not a risk to take
unattended on a working machine. The safe half — unpackaged HKCU `LocalServer32` cold activation — is
already proven by probe 1. The unproven remainder is exactly the marshalling question, and the way to
answer it is a throwaway VM.

### Size and risks

**L, and it should not be in this block.** An out-of-process COM server implementing
`ITerminalHandoff3`; a proxy/stub to build and register; ConPTY handle *adoption*, which bypasses
`bt-pty`'s spawn path entirely and needs a session whose PTY was created by someone else. Recommend a
separate spike on a VM after the rest of the block ships.

---

## 4. Explorer context menu

### Verdict

**Feasible, trivial, and it lands in the second-level menu.** That is not a bug to fix; it is the
price of not being packaged, and it is where every other unpackaged tool sits.

### Exact keys

Three trees, because three different things get right-clicked:

```
HKCU\Software\Classes\Directory\Background\shell\Folio    ; empty space inside a folder
HKCU\Software\Classes\Directory\shell\Folio               ; a folder icon
HKCU\Software\Classes\Drive\shell\Folio                   ; a drive root, which is not a Directory
```

each carrying:

```
    (Default)  REG_SZ  "Open in Folio here"          ; the menu label
    Icon       REG_SZ  "C:\...\folio.exe,0"          ; "path,index"
  \command
    (Default)  REG_SZ  "\"C:\...\folio.exe\" --cwd \"%V\""
```

`%V` is correct for all three (`%1` also works for `Directory` and `Drive` but **not** for
`Background`, so `%V` everywhere is the uniform choice). Optional values: `Extended` (empty REG_SZ)
hides the verb behind Shift+right-click; `MUIVerb` points at a localised string resource instead of a
literal label — the route to use when the bilingual plan lands; `Position` takes `Top`/`Bottom`.

### PROBED — what was run and what happened

`probes/shell-probe` with `register-menu` / `dump-menu` / `unregister-menu` / `argv` / `invoke-verb`.
The registered command runs the probe's own `argv` verb, which appends its full argv and cwd to a log
— so the log is direct evidence of what `%V` expanded to.

**The verb appears, in the classic menu.** `artifacts/win-landing/shot-06-ctx-primary.png` is the Win11 primary menu:
**"Open in Folio here" is not in it**. `artifacts/win-landing/shot-07-ctx-classic.png` is the same menu after "Show
more options", and there it is, between "释放C盘空间" and "Open Git GUI here".

The primary menu on this machine *does* carry third-party entries — "Open with Code", "TortoiseSVN",
"在终端中打开" (Windows Terminal), "AMD Software" — and that is the point: each ships an
`IExplorerCommand` registered through a **sparse MSIX package**. Classic `shell\<verb>` registrations
are not promoted. The choice is: classic verb in the second-level menu with zero packaging, or a
sparse package to reach the primary menu. **Recommend the classic verb**, matching Git's placement;
revisit only if Folio acquires a package identity for other reasons.

**`%V` expands correctly.** Driving the Win11 menu by synthesised mouse clicks proved flaky (the menu
opens up or down depending on room, and its geometry shifted between runs), so the verb was invoked
the way Explorer invokes it — `ShellExecuteExW` with `lpVerb = "Folio"` on a directory, which reads
the very same `command` value and performs the same substitution:

```
ARGV argv=["...\shell-probe.exe", "argv", "--cwd", "D:\\Developer\\BetterTerminal\\crates"]
     cwd="...\probes\target\debug"   elevated=false
```

**Note the second half of that line.** `%V` arrived intact as an argument, but the launched process's
**working directory is the exe's own directory, not the clicked folder**. Folio must take the path
from `--cwd` and must not read `current_dir()`.

### Size and risks

**S in `bt-platform` (registry writer) + S in `bt-app` (`--cwd`), gated on the CLI front door.**
`bt-platform` already has precedent for this shape — `shell_execute`, `reveal_in_explorer`, and
`reveal_arguments`, the last being a pure `#[must_use]` string builder with tests, which is exactly
the model a `context_menu_command(exe, cwd)` builder should follow.

The open question is product, not technical: should the verb open a new window or a tab in the
existing one? The app is single-window today, so **"launch a new process" is the only coherent answer
now**, and it needs nothing extra. Revisit alongside item 1's cold path, which wants the same
single-instance machinery.

---

## 5. Run as administrator

### Verdict

**Feasible as a whole-window action. An elevated *tab* inside a non-elevated window is impossible
without a broker, and the reason is structural, not a missing API.**

### Why a tab cannot be elevated

A process has exactly one token, fixed at `CreateProcess` time. There is no flag that elevates a
child: `CreateProcessW` cannot do it at all, and elevation happens only through the shell's `runas`
verb, which starts a *new process* under the Consent UI. Folio's tabs are PTY children of one
`folio.exe`, all inheriting its token. An elevated tab in a non-elevated window would need a separate
elevated broker process hosting that PTY and streaming bytes back over IPC — and that channel is a
non-elevated process's handle on an elevated one, i.e. a privilege-escalation surface that must be
designed as security-critical, not as a convenience. Windows Terminal does not do it either; it
elevates the whole window.

### Mechanism

```rust
SHELLEXECUTEINFOW {
    cbSize:       size_of::<SHELLEXECUTEINFOW>() as u32,
    fMask:        SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
    lpVerb:       w!("runas"),
    lpFile:       <full path to folio.exe>,
    lpParameters: <--profile <id> --cwd <path>>,
    nShow:        SW_SHOWNORMAL.0,
    ..Default::default()
}
ShellExecuteExW(&mut info)
```

Declining is not an error condition — it arrives as `ERROR_CANCELLED` (1223) wrapped in an `HRESULT`,
and Folio should treat it as "the user said no" and do nothing, not surface a failure. Reading the
process's own state uses `GetTokenInformation(.., TokenElevation, ..)`.

### PROBED — partially, and the rest deliberately not

`probes/shell-probe amiadmin` reports `elevated=false` via `TokenElevation`, so detection is verified.
`runas` itself was **not fired**: this machine has `EnableLUA=1`, `ConsentPromptBehaviorAdmin=5` and
**`PromptOnSecureDesktop=1`**. A consent dialog on the secure desktop cannot be dismissed by injected
input, so running it would have parked a modal prompt in front of the user with no way for the probe
to clear it. The API path is compiled and the cancel-path handling written; only the human keystroke
is unverified, and it is the best-documented part of the block.

### Design — and a correction the survey forced

The obvious design is "a per-profile *Run as administrator* flag". **The repository does not support
that shape today.** `PROFILES` is a fixed compile-time `[Profile; 5]` of `Copy` structs with
`&'static str` fields (`crates/bt-app/src/profiles.rs:207-267, 455`), and the fixed set was a
deliberate user ruling, not an accident ("Four fixed entries rather than a discovery pass",
`profiles.rs:196-205`). Users cannot add or edit profiles, so a per-profile boolean would be a
constant the user cannot reach — a flag with no way to set it.

Two honest options:

1. **An action, not a profile flag** — a "New elevated window" command in the command surface and the
   tab-strip `+` menu, which spawns `folio.exe --profile <current> --cwd <current>` under `runas`.
   This needs no profile-schema change and no persisted setting, so it is the smaller and more
   truthful choice while `PROFILES` stays fixed.
2. **A profile field** — add `elevated: bool` to `Profile` and a sixth entry. Cheap mechanically, but
   it doubles the fixed list per shell and asks the fixed-set ruling to be revisited.

**Recommend option 1.** Revisit option 2 only if user-defined profiles ever land.

Two consequences for the design record either way:

- **Session restore must exclude elevated windows.** Once multi-window exists, an elevated window is a
  separate process with a separate token; letting it into the persisted set would make restore try to
  re-elevate at startup — a UAC storm and a poor security posture. Today this is free, since
  `SessionV1.window` is singular and an elevated window is simply a second process nothing tracks.
- **Drag-and-drop and clipboard** between elevated and non-elevated windows are restricted by UIPI, so
  tab tear-out between them can never work; the tab strip must not offer it.

### Size and risks

**S in `bt-platform` (extend the existing `shell_execute` bridge) + S in `bt-app` (the action and its
spawn), gated on the CLI front door.** Risk is entirely in the design decisions above, not the API.

---

## 6. Which items need a register step, and how each is undone

| item | registration? | written where | undone by |
|---|---|---|---|
| 1 notifications | **yes**, before the first toast | `HKCU\Software\Classes\AppUserModelId\<AUMID>`, `HKCU\Software\Classes\CLSID\{activator}` | delete both trees **plus** `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>`, which Windows creates itself and which otherwise leaves Folio listed in Settings forever |
| 2 taskbar progress | no | — | — |
| 3 default terminal | **yes**, and it changes system behaviour | `HKCU\Console\%%Startup` + CLSID/Interface/proxy-stub trees | restore the previous `DelegationConsole`/`DelegationTerminal` (**save them before overwriting**), delete the CLSID trees |
| 4 context menu | **yes** | the three `HKCU\Software\Classes\...\shell\Folio` trees | `RegDeleteTree` on each |
| 5 run as admin | no | — | — |

Everything is `HKCU` and needs no elevation to write or remove; all of it yields to `RegDeleteTreeW`,
which is what the probes used.

The shipping shape should be **a first-run "Integrate with Windows" step and a matching "Remove
integration"**, both idempotent, both in Settings — not an installer, since Folio ships as a bare exe.
Registration must be re-asserted on launch whenever the exe path has changed: `LocalServer32` and the
`command` values embed absolute paths, and a moved `folio.exe` silently breaks both.

On the Settings surface itself, note the existing shape before designing the panel: rows are the
`SettingsRow` enum grouped by `SettingsGroup` (`crates/bt-app/src/settings.rs:241-263, 297-355`), and
**there is no checkbox widget** — booleans render as a two-item combo picker reading "On"/"Off"
(`settings.rs:184, 210-213`). A new Windows-only group would follow how `Files` was added. Any
persisted boolean bumps `SETTINGS_SCHEMA_VERSION` (currently 5) with one structural migration step in
`bt-persist/src/migrate.rs`, and `CONVENTIONS.md` requires the field to arrive **in the same change as
its reader** — no placeholder fields.

---

## 7. This spike left nothing behind

Verified after teardown: the AUMID tree, the activator CLSID tree, the platform's
`Notifications\Settings\Folio.Terminal` entry, and all three context-menu trees all read
`exists=False`; `HKCU\Console\%%Startup` still holds the Windows Terminal Stable pair it held before
the spike began. The seven Explorer windows the menu probe opened were closed by title; the user's own
Explorer windows and `explorer.exe` were untouched, `dist/` was never written to, and no `folio.exe`
was ever signalled.

---

## 8. Recommended slicing

**Slice 0 — the front door.** Command-line parsing in `bt-app`: `--cwd <path>`, `--profile <id>`, and
a reserved `-Embedding`. It goes between `install_panic_log_hook()` and `EventLoop::build()`
(`main.rs:34277`), threaded into `Runtime::create` (`main.rs:11875`) where profile and restore
decisions already happen. Three of five items need it, and nothing else in the block is honest without
it. Decide the single-instance question here too, because items 1 and 4 both bend around it.

**Slice 1 — taskbar progress.** Smallest, self-contained, no registration, no CLI; gives an
already-parsed value a second consumer and reuses the existing `window_hwnd` accessor. Ship first. The
one new design decision is the aggregation rule, which should either follow `fleet_progress`'s
first-in-order discipline or overturn it explicitly in DESIGN.md.

**Slice 2 — context menu.** Registry only, plus `--cwd` from slice 0. Ships with the
register/unregister pair and the Settings group that slice 3 reuses.

**Slice 3 — notifications.** The COM activator is the block's real engineering. Depends on slice 0 for
the cold path and shares slice 2's registration UI. Hang the trigger off the existing `work_in_flight`
true→false transition rather than inventing a completion signal, and record that `cmd.exe` tabs cannot
raise it.

**Slice 4 — run as administrator.** Small once slice 0 exists: an action plus a spawn. Sequence after
the multiwindow block, since it adds a window that must be excluded from session restore.

**Not in this block — default terminal.** Separate spike, on a VM, with the proxy/stub question
answered first.

---

## 9. Probe inventory

```
probes/toast-probe/      Rust — AUMID registration, toast, INotificationActivationCallback,
                         IClassFactory + CoRegisterClassObject, cold -Embedding path
probes/taskbar-probe/    Rust — real HWND, TaskbarButtonCreated, every TBPF_* state, SetOverlayIcon
probes/shell-probe/      Rust — context-menu key trees, argv/%V logging, ShellExecuteExW verb invoke,
                         runas path, TokenElevation
probes/screen.ps1        DPI-aware full/region screen capture and click. Needed because toasts and
                         taskbar buttons are not inside any window we own, so ui-probe.ps1's
                         per-window capture cannot reach them.
probes/toast-run.ps1     starts the toast probe and photographs the ~5s toast on a loop
probes/taskbar-run.ps1   holds the auto-hidden taskbar revealed and photographs each state
probes/menu-run.ps1      drives Explorer's Win11 menu and its "Show more options" hop
probes/menu-invoke.ps1   Shift+F10 route to the classic menu
artifacts/win-landing/                   the evidence referenced above
```

Build with `CARGO_TARGET_DIR` pointed at `probes/target` and
`PSModulePath="C:\Program Files\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules"`.
For anyone extending these: `windows` 0.62 has **no `implement` feature** (add `windows-core` as a
direct dependency), `RegCreateKeyExW` needs `Win32_Security`, `RegSetValueExW` takes `HKEY` not
`Option<HKEY>`, and `ITaskbarList3::SetOverlayIcon` takes a bare `HICON` not an `Option<HICON>`.
