# Installing Folio

[`README.md`](../README.md) has the three lines most people need. This is the
rest: what each download contains, what a machine has to be, what the first run
asks, and what to do if the system puts a panel in front of you.

## Windows

Take `folio-<version>-windows-x64.zip` from the
[releases page](https://github.com/lulu-loopp/folio-terminal/releases), unpack it
wherever you keep programs, and run `folio.exe`. There is no installer; keep the
extracted files together in one folder. `SHA256SUMS.txt` is the hash of what you
downloaded. Needs **Windows 10 1809 or newer, or Windows 11, 64-bit**.

The archive holds ten files that belong together: `folio.exe`, the two console
libraries it needs to start a shell, `folio.msix` for the Explorer menu,
`folio-here.cmd` for VS Code, `uninstall.cmd` for cleanup, and the licences and notices. `folio.msix` is not
an installer and is not run: it is the sparse package that lets Windows 11 put
"Open in Folio" on the first page of a folder's right-click menu, and it names
the folder it was unpacked into — which is why moving `folio.exe` out on its own
takes that entry with it.

The web preview needs the **WebView2 Runtime**. Windows 11 has it; Windows 10
usually does, and if it does not, the Evergreen Runtime is
[here](https://developer.microsoft.com/microsoft-edge/webview2/). Without it
everything except the web preview works, and the preview says what is missing.

<!-- winget: add when live -->

## macOS

Take `Folio-<version>-macos-arm64.dmg` from the same
[releases page](https://github.com/lulu-loopp/folio-terminal/releases), open it,
and drag **Folio** to Applications. Needs an **Apple silicon Mac running macOS 14
or newer**; there is no Intel build in this preview. `SHA256SUMS-macos.txt` is the hash
of what you downloaded.

Or, with [Homebrew](https://brew.sh):

```sh
brew install --cask lulu-loopp/folio/folio
```

The web preview uses the WebKit already on the machine. There is nothing to
install.

## If Windows or macOS shows a warning

Every release is signed, and the macOS one is notarized by Apple as well;
[`RELEASING.md`](RELEASING.md) says what is signed on each platform and who
holds the certificate. A machine that has not seen a signature before still puts
one panel in front of you the first time.

- **Windows** — "Windows protected your PC". **More info** names the publisher,
  and **Run anyway** starts it. Check that the name there is the holder
  `RELEASING.md` names.
- **macOS** — a panel naming the developer, with **Open** in it. If it opens
  without offering **Open**, right-click the application and choose **Open**
  instead. It asks once and not again.

What must **not** appear on a Mac is a panel saying the developer **cannot be
verified**, or that Folio is **damaged and can't be opened**. Either means what
you have is not what was published — an interrupted download, or a copy altered
after it was signed. Check it against `SHA256SUMS.txt` and take it from the
releases page again. A build you made yourself and signed ad-hoc for your own
machine is refused in the same words; [`BUILDING.md`](BUILDING.md) says what to
do with one.

## First run

A machine that has never run Folio gets one card, once. It says **Welcome to
Folio** and asks whether to check for updates, add Folio to the folder
right-click menu, enable the PowerShell integration, and mark the tab for each
of Claude Code, Codex and Copilot CLI this machine has. Update checks arrive on;
the rest arrive off. Nothing about theme, font, size, language or layout — those
are one click away and cost nothing while they are wrong.

**The card offers only the rows the machine can honour.** The folder right-click
menu and the PowerShell integration are Windows facilities, so a Mac is asked
about the update check and the agents and nothing else.

<picture>
  <source media="(prefers-color-scheme: dark)"
          srcset="screenshots/first-run-dark.png">
  <img src="screenshots/first-run-light.png" width="100%"
       alt="The card over a window that has just started: Folio's mark beside
       Welcome to Folio, then the rows this machine was offered, one line each
       with a switch at the right of every one. Check for Folio updates is on;
       Open any folder in Folio from its right-click menu and Jump between
       commands in PowerShell are off; after a wider gap, Mark the tab when
       Claude Code is waiting, when a Codex turn ends, and when Copilot CLI is
       waiting, all three off. At the foot, a faint line reading You can change
       these options in Settings, then Not now and Done. Behind the card, one
       tab and a prompt.">
</picture>

**Rest the pointer on a row and it says how** — including which of your own
files the switch writes, and that the file is copied to a dated backup first.

**Every row on the card is also a row in Settings**, so nothing on it is a last
chance. **Done** applies the rows that are on. **Not now** and `Esc` keep the
shipped values: the update check on, the rest off. Either way the card does not
come back, and if you were already using Folio you never see it.

The first tab opens the first shell your machine actually has. On Windows the
five shipped profiles are looked for in order — PowerShell 7, Windows
PowerShell, WSL, Git Bash, Command Prompt; on a Mac it is the shell your account
already uses, then zsh, then bash, then `/bin/sh`. One whose program is not
installed does not appear in the menus that start a shell; it stays on the
Profiles page in Settings, greyed out and naming the program that was looked
for. The seven agent profiles are found the same way, on the `PATH`, and that is
the same lookup the card's agent rows use.

**On Windows**, the PowerShell integration adds one line —
`. "$env:APPDATA\Folio\shell-integration\folio.ps1"` — to the `$PROFILE` a
PowerShell names for itself, after copying the file as it stood to a dated backup
beside it; delete that line to undo it. Command marks and inline `$…$` formulas
run on that integration. Git Bash and WSL need none of it, and neither do zsh
and bash on a Mac: Folio hands those their own integration as it starts them,
out of its own directory, and writes nothing of yours.

Where that file is comes from the shell, so a row left on when the card is
answered takes effect in the next PowerShell session, and Settings > Terminal
says so until it does.

If you were not asked on the card, a strip offers the same thing the first time
a PowerShell pane prints something: **Add to `$PROFILE`** does it, **Don't show
again** ends the asking, and closing the strip decides nothing, so the next
PowerShell asks once more.

**On a Mac**, what Folio remembers lives in
`~/Library/Application Support/Folio`.

The first time a waiting agent's mark has to leave the window, macOS asks
whether Folio may send notifications. Answer it once. Say no and the Agent page
in Settings says so rather than going quiet, the dot on the tab and the Dock
icon go on working, and Folio does not ask again.

Finder's right-click menu gets **Open in Folio**, under **Services** — Folio
registers it the first time it runs, so there is nothing to switch on and no
need to sign out. On a folder it opens a tab standing in that folder; on a file,
a tab standing in the folder the file is in. Folio also has a menu bar of its
own, and every item in it carries the same key as the row it has in
[Shortcuts](shortcuts.md).

The three rows on the Agent page read the tool's own configuration file and
report what is in it. On a new machine all three files are absent, so all three
read Off.

## Uninstalling

Close Folio, then run one command from another terminal:

- **Windows:** `folio.exe --uninstall-cleanup` from the extracted folder, or
  double-click its `uninstall.cmd`.
- **macOS:** `/Applications/Folio.app/Contents/MacOS/folio --uninstall-cleanup`.

The command opens no window. It removes Folio's integrations, keeps hooks and
Explorer entries belonging to another existing copy, and reports each result.
Exit `0` means cleanup succeeded; `1` names a refusal to fix and retry; `2`
means Folio is running or, with purge, a process still holds its data.

Add `--purge` to the same command to also delete settings, sessions and browser
data, including both Windows data roots and legacy data, or all six macOS data
locations. Without it, your data stays. Dated recovery copies beside user
configuration files are kept. The application folder is never deleted.
After cleanup succeeds, delete the extracted folder or move Folio.app to the Bin;
for a managed installation, use its package manager to remove the application.

macOS notification permission and cached Services entries are managed by the OS;
removing the bundle may take time to be reflected. Windows notification history
and package-manager records are also managed by their owners.

Already deleted Folio? See [recovery after deleting Folio](recovery-after-deleting-folio.md).

## Known issues

- **A window was once reported drawing its top half black** after a move to a
  second monitor, unreproduced. Attach `%APPDATA%\Folio\diagnostics.log` — or
  `~/Library/Application Support/Folio/diagnostics.log` on a Mac — if you hit
  it.
- **Windows: `.webm` needs the VP9 or AV1 Video Extension** from the Microsoft
  Store. A stock Windows has neither, and without one there is no still and no
  playback.
- **macOS: a run that ends in a crash** leaves its report where the system puts
  every one, `~/Library/Logs/DiagnosticReports`; the next Folio names the file
  in its own log rather than copying it anywhere.
- The rest are in [`CHANGELOG.md`](../CHANGELOG.md).
