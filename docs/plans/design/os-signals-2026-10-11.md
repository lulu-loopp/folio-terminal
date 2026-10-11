# OS-level change signals and the deep palette refresh: T-OS-SIGNALS and T-PALETTE-DEEP (0.4.8 B5), design note, 2026-10-11

Status: **draft, 2026-10-11**, for review before any ticket below is dispatched.
Plan row B5 ("OS-level change signals; deep palette refresh"), the last in the chain
B1 → B2 → B3 → B4, all four landed. Read at `main` = `ae933eb7` on
`design/os-signals`. No product code, no cargo run, no window. Facts about
Folio come from the code, cited `file:line` at `ae933eb7` with the symbol beside
the line so a later reader can find it again. Facts about Windows and macOS come
from their documentation (§9 Sources, one URL per claim). A claim no source
settles is an experiment `E-<n>` (§6), and the ticket step that rests on it
waits for it. Every cost is either a measurement from the B2/B3/B4 reports
(cited by table) or an **estimate**, marked as one.

**Section signs.** A bare `§n` is a section of this note. `A§n` is a section of
`docs/ARCHITECTURE.md`, and `TD§n` a section of `docs/plans/design/thread-door-2026-09-26.md`.
`B2§n`, `B3§n` and `B4§n` are sections of those tickets' reports (the 0.4.8 trace,
2026-10-08), which give the measurements this note relies on.

**The rulings this answers**

| date | ruling | where |
|---|---|---|
| ARCHITECTURE §4; B3's brief | No timer anywhere. The window thread waits on nothing new (`window_waits.tsv` does not grow). One owner per fact. A stale answer never replaces a fresh one | every row of §2; §4 |
| 2026-10-04 (owner) | Menus that list programs update whenever a new answer lands, and rows follow their items by identity | §3.2, §4.3 |
| R31 (DESIGN §7.1.3g ②) | A directory is not read because time passed; a change notification is not time passing | §4.2 (b) |
| TD§3.3; `bt_platform::admission` | An OS-owned callback is entered with `enter_callback(name)` and only nudges the loop | §3.5, §4.3 |
| 2026-10-05 (owner) | A pane takes the account's current environment, not the launcher's | §3.2 (the PATH ear reads the walk's logon block), §8 Q4 |

---

## 0. In one page

- **What already listens** (§1). Each window has one ear, `bt_platform::SystemSettingsWatch`
  (on Windows one subclass, `crates/bt-platform/src/lib.rs:10934-11053`; on macOS three
  observers, `crates/bt-platform/src/macos_impl.rs:1934-2091`). It tells the loop one
  of three `SystemNews` (`lib.rs:17457-17471`): `Preferences` (every `WM_SETTINGCHANGE`
  and `WM_THEMECHANGED`, unfiltered, the `Environment` broadcast among them), and
  `Display` and `InputLanguage` (the two arms B3 added). `Preferences` drives B2's program walk
  (`main.rs:67163-67185`). Directory watches (`DirWatch`, one `bt-dir-watch` thread per
  watch, `lib.rs:16416-16420`) serve the git page, previews, schemes, storage and the files
  column. The files column's watches are shallow and cover only the unfolded folders
  on the tab in front (`files_watch.rs:24-47`). The palette's index is walked again on every
  palette open (B3, `palette_index.rs:593-604`), and the files watch marks it dirty
  (`runtime/files.rs:1218-1238`). **Linux has no ear at all**: `SystemSettingsWatch`
  subscribes to nothing and `DirWatch` refuses (`portable_impl.rs:513-531`, `:826-866`).
- **The inventory** (§2) lists 30 OS signals on the two shipping platforms (15 on
  Windows, 15 on macOS). One rule sorts them. **A fact read when something opens**
  (a menu, a page, the palette) **is already as fresh as that opening** (B2, B3). An
  ear is only worth adding for a fact read **without** an opening, or where the opening
  shows a stale answer long enough to be seen. By that rule this note adopts seven
  signals in four families, finds nine already heard, and gives the other fourteen no
  ear, each with its reason (most because nothing in Folio reads them).
- **Adopted** (§3):
  1. **Volumes.** Windows sends `WM_DEVICECHANGE` with `DBT_DEVICEARRIVAL` or
     `DBT_DEVICEREMOVECOMPLETE` and `DBT_DEVTYP_VOLUME` to every top-level window
     without registration; macOS posts `NSWorkspace.didMountNotification` and
     `didUnmountNotification`. Both become a fourth `SystemNews` arm on the **existing**
     ear. The arm asks again for the folders a files column could not read or could not
     watch, and for the palette roots that failed. Today a column over a drive that was
     absent says "Folder not found" until the folder is folded and unfolded, because
     nothing asks again (`files_watch.rs:72-81`).
  2. **The PATH directories.** A shallow, named watch on each `PATH` directory of the
     logon block feeds B2's lane through a new `Trigger::PathDirectory`. The
     `Environment` broadcast does **not** fire for the most common agent installs:
     `npm i -g`, `cargo install`, winget's portable links, a Store app's execution alias
     and Homebrew each write a file into a directory that is **already** on `PATH`.
     Today such a program reaches "+" and the default profile only after a menu or a
     page opens.
  3. **The macOS locale.** `NSLocale.currentLocaleDidChangeNotification` makes Folio ask
     `system_locale_declaration` again. That is the `LANG` and `LC_CTYPE` a new pane gets,
     today a `OnceLock` read once per process by two child processes (`lib.rs:7807-7810`).
  4. **A watch lets go of a volume that is being removed** (Windows). An open
     `ReadDirectoryChangesW` handle is an open handle on the volume. The documented
     way to stop blocking "Safely remove" is a notification on that handle
     (`CM_Register_Notification` with `CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE`), on which the
     watch closes itself. Experiment E-2 decides whether today's watches block an eject.
- **Not adopted, each with its reason** (§3.4):
  - `PackageCatalog`: redundant with the PATH ear for every fact the walk reads. It does
    **not** need package identity, so identity is not the reason.
  - Start-menu folders and `/Applications`: nothing reads them, because programs come
    from `PATH` and the install roots.
  - Time zone and `intl`: Folio shows no local wall time.
  - App launch, terminate and activate: Folio reads these fresh when it needs them.
  - Finder labels: Folio does not show them.
  - WSL's registry key: every surface that reads it walks when it opens.
- **The deep palette refresh** (§4) is one design. **Each palette root gets a named tree
  ear.** The watcher thread filters what it hears by the index's own definition
  (`SKIPPED_DIRECTORIES`, `INDEX_MAX_DEPTH`), so a `cargo build` writing into `target\`
  wakes nothing. What gets through marks the root dirty. After `WatchClock`'s quiet
  window, the root is **walked in the background** on the existing index worker. A
  palette open then walks only a root that is dirty, missing or **unheard**: a network
  share, `\\wsl$`, a watch that failed. B3's walk on every open stays for exactly those
  roots. This adds no timer (the clock starts only on a disk event, R31) and no
  window-thread wait. Memory stays bounded by the existing `INDEX_MAX_ENTRIES` for each
  retained root. **What a person sees: nothing.** The rows are already current when
  the palette opens. If the palette is open when a walk lands, the new rows arrive and
  the selection follows its item (B3).
- **Costs** (§5). An existing mechanism coalesces every adopted signal:
  - B2's lane: one walk out and one waiting, 2–7 ms a walk (B2§2);
  - `WatchClock`: a 300 ms quiet window and a 2 s floor (`watch_clock.rs:33`, `:51`);
  - the files lane's queue, which keeps the newest request per folder;
  - the index register's epoch.

  Some signals lie, and the note names them:
  - `DBT_DEVNODES_CHANGED` fires for any device, a phone or a mouse. It is ignored.
  - A phone connected over MTP is not a volume, so it sends no volume message.
  - On macOS, Folio's own update DMG mount is heard. It costs one more read of the
    folders that failed.

  Package identity changes nothing. The run drops its identity at start (the ledger's
  2026-10-10 correction), and every ear here is a file-system watch or a window
  message, not a package API.
- **Tickets** (§7), in dependency order: **T-WATCH-LETS-GO** (S, Windows; E-2 first) →
  **T-OS-SIGNALS-WIN** (M) → **T-OS-SIGNALS-MAC** (M) → **T-PALETTE-DEEP** (M). Each
  names its tests, and the mutation each test must be seen failing under.
- **Four owner questions** (§8), each with a recommendation:
  - Should Folio watch the PATH directories? (Recommended: yes.)
  - Should the palette walk in the background? (Recommended: yes.)
  - Should a watched drive that Windows is ejecting be let go silently? (Recommended: yes.)
  - Should the macOS locale re-read apply to new panes only? (Recommended: yes.)

---

## 1. What stands on `ae933eb7`

### 1.1 The one ear per window

Every window installs `bt_platform::SystemSettingsWatch` once, and carries on without it
if the install fails (`crates/bt-app/src/main.rs:43023-43031`). Its wake is one
`EventLoopProxy::send_event(AppEvent::of_system_news(news))` (`main.rs:833-839`).

- **Windows** (`crates/bt-platform/src/lib.rs:10934-11053`): one `SetWindowSubclass`
  procedure. `system_news_of` (`lib.rs:11025-11032`) maps `WM_SETTINGCHANGE` and
  `WM_THEMECHANGED` to `Preferences`, `WM_DISPLAYCHANGE` to `Display` and
  `WM_INPUTLANGCHANGE` to `InputLanguage`. Every other message is forwarded untouched.
  The `wParam` and `lParam` are **not** read. The doc comment (`lib.rs:10937-10944`)
  explains why: the section string is best-effort. B2§2 also measured that filtering
  on it would save nothing. The procedure runs inside the sender's dispatch; it calls
  the wake and returns.
- **macOS** (`crates/bt-platform/src/macos_impl.rs:1934-2091`): key-value observing on
  `NSApplication.effectiveAppearance` and `NSWorkspace`'s accessibility-display
  notification (both `Preferences`), and `NSApplicationDidChangeScreenParametersNotification`
  (`Display`). The observer only calls the wake (`macos_impl.rs:2064-2087`).
- **Linux and every other target** (`crates/bt-platform/src/portable_impl.rs:513-531`):
  the watch is constructed but subscribes to nothing and never wakes.

The loop's arms are at `main.rs:67163-67199`:
- `SystemPreferencesChanged` reads motion and the system canvas again, asks the taskbar
  lane, and requests a program walk (`programs_lane::Trigger::Environment`).
- `DisplayChanged` runs `follow_the_display` in every window.
- `InputLanguageChanged` calls `Quake::layout_changed`.

**One ear per window means one wake per window for each message.** With three windows
open, one broadcast becomes three `SystemPreferencesChanged` turns and three walk
requests. The lane merges them (§5).

### 1.2 The program walk (B2) and what asks for it

The walk lives in `crates/bt-app/src/programs_lane.rs:1-50`:
- One `program-walk` worker serves numbered requests.
- Requests made while a walk is out are answered by the next walk.
- Answers arrive row by row. The newest answer per row is kept and an older one is
  refused (`ProfilePrograms::adopt`).
- The walk reads the **current logon block**
  (`bt_platform::environment::fresh_logon_environment`), never the launch environment.

Its triggers (`programs_lane.rs:70-96`) are: the launch, a table edit, a program menu
opening, the Profiles, Agents and Git pages opening, the broadcast (`Environment`), a
pane born on an unknown row, and a walk asked again after one died. One walk answers the
program rows, WSL's facts, git's location and the agent folders (B3 row 6). B2§2
measured it (the logon-block rows of its table): 2–3 ms idle and at most 7 ms with every
hardware thread busy, over 23 `PATH` entries and 291 `is_file` calls.

### 1.3 Directory watches

**`DirWatch` on Windows** (`lib.rs:15981-16022`):
- It runs `ReadDirectoryChangesW` on a handle opened with `FILE_LIST_DIRECTORY` and all
  three share modes (`lib.rs:16368-16381`).
- Each watch has its own thread, spawned through the thread door at `Normal` priority
  (`lib.rs:16416-16420`). The thread waits on the completion and a stop event
  (`lib.rs:16558`).
- The notification filter leaves out `LAST_ACCESS` (`lib.rs:16059-16067`), and the
  buffer is 64 KiB (`lib.rs:16052`).
- There are three constructors: tree, shallow, and shallow named (`lib.rs:16311-16355`).
  **Only the shallow named one reads names.** The tree watcher deliberately reads
  nothing (`lib.rs:15989-16010`). This followed a measurement: under a repository root,
  655 ms of writes into `target\` cost the window thread 188 ms of CPU.

**`DirWatch` on macOS** is one FSEvents stream on its own run-loop thread per watch
(`lib.rs:18112-18134`, `macos_watch.rs`).

**The files column's watch** (`crates/bt-app/src/files_watch.rs`):
- It is shallow, with one watch per folder on the glass (`files_watch.rs:24-47`).
- A folder that cannot be watched keeps an entry with no handle. Folio tries it again
  only **each time it comes on screen** (`files_watch.rs:72-81`).
- A watch that fails later is dropped silently (`files_watch.rs:194-199`).
- Notifications go through `WatchClock` (`watch_clock.rs:33` `WATCH_QUIET` 300 ms,
  `:51` `WATCH_FLOOR` 2 s).
- A folder that cannot be read answers `DirFault::NotFound` or `Unreadable`
  (`files.rs:200-216`).

### 1.4 The palette's file index

The index lives in `crates/bt-app/src/palette_index.rs`. Its walk is breadth-first and
bounded by:
- `INDEX_MAX_DEPTH` = 6 (`:93`);
- `INDEX_MAX_ENTRIES` = 30 000 (`:103`);
- the `SKIPPED_DIRECTORIES` names (`:119-133`).

It runs on one `bt-index-worker` thread at `BelowNormal` priority (`:403-438`).

The register, `FileIndexes`, belongs to the application and holds one entry per root
(`:516-631`):
- `claim` (`:552-570`) asks for one walk per epoch.
- `mark_dirty` (`:579-583`) marks a root without walking it.
- `reopened` (`:593-604`, B3) walks on every open, unless a walk is already out. In that
  case the open is merged into it, with one `diagnostics.log` line
  (`runtime/files.rs:2769-2783`).
- `accept` (`:611-621`) drops an answer from a superseded epoch and keeps `dirty`.
- `retain` (`:629-631`) forgets the roots no window names.

The roots are the files columns of the **tab in front** (`runtime/files.rs:2758-2764`).
`retain_file_indexes` keeps the union over all windows (`main.rs:63092-63101`). The files
watch marks a root dirty only for a folder it hears, and it hears only **unfolded**
folders (`runtime/files.rs:1218-1238`, `:1263`).

B3§2 (row 8) measured the walk of a repository with 1 883 entries at 39–77 ms idle and
**4.8–9.8 s with 32 busy normal-priority threads**, because the below-normal worker waits
for Windows' anti-starvation boost. A note on the brief: B3's "5 s" walks are the
**loaded** figure, not a cold-cache one. Nobody has measured a walk on a cold disk after
boot (E-5).

### 1.5 The window thread's budget

`crates/bt-app/src/window_waits.tsv` (232 lines at `ae933eb7`), checked by
`scripts/ci/check-window-waits.ps1`, lists every bare wait on the window thread. Row 7
(`window_waits.tsv:61`) is still open. It is the macOS locale probe's two child
processes, "blocking, no timeout, on the pane-birth road"
(`bt_platform::read_system_locale_declaration`). Nothing in this note adds a row, and
§3.3 is written so that its only possible effect on the file is to remove row 7.

---

## 2. The signal inventory

The tables use these columns:
- **mechanism**, with its source (§9);
- **thread**: where the signal arrives;
- **reliability**;
- **Folio fact**: the fact it bears on, and who reads that fact;
- **ear**: the existing `SystemNews` arm, a new arm on the existing ear, a new ear, or none;
- **refresh**: whether it runs on the signal itself, or waits behind an epoch or a lane.

"No reader" means a search of the tree finds nothing Folio shows that the signal could
make stale. The search used is named in the row.

### 2.1 Windows

| # | signal | mechanism, source | thread | reliability | Folio fact (reader) | ear | refresh |
|---|---|---|---|---|---|---|---|
| W1 | `WM_SETTINGCHANGE` "Environment" | Sent with `SendMessageTimeout(HWND_BROADCAST)` by whoever edits the environment [S1] | The window thread, inside the sender's dispatch | An installer may or may not send it, and nothing requires it to [S1] | Program rows, WSL, git, agent folders, and Copilot through the walk (B2; B3 row 6) | **existing** `Preferences` (`lib.rs:11027`) | Lane (B2): one walk out, one waiting |
| W2 | `WM_SETTINGCHANGE` for other areas: "Policy", "intl", `SystemParametersInfo` areas, and undocumented strings such as the colour mode's | As W1 [S1]. The `lParam` "does not usually indicate which specific system parameter changed" [S1] | As W1 | Installers and Settings send bursts | Motion, the system canvas (dark or light), taskbar auto-hide (`main.rs:67163-67175`) | **existing** `Preferences` | Synchronous reads (µs), plus the taskbar lane |
| W3 | `WM_THEMECHANGED` | Window message [S2] | Window thread | — | As W2 | **existing** `Preferences` (`lib.rs:11027`) | As W2 |
| W4 | `WM_DISPLAYCHANGE` | Sent to every top-level window on a mode change [S3] | Window thread | — | Each window's frame rate (B3 row 4) | **existing** `Display` | Synchronous, 28 µs median under load (B3§2) |
| W5 | `WM_DPICHANGED` | Sent to each window [S4]. winit surfaces it as `ScaleFactorChanged` | Window thread | — | Scale and layout (`runtime/dpi.rs`) | winit, not ours | Synchronous (already exists) |
| W6 | `WM_INPUTLANGCHANGE` | Sent to the focused top-level window [S5] | Window thread | Reaches only the window with focus | The summon's virtual key (B3 row 5) | **existing** `InputLanguage` | Synchronous, 12 µs median under load (B3§2) |
| W7 | `WM_DEVICECHANGE` with `DBT_DEVICEARRIVAL` or `DBT_DEVICEREMOVECOMPLETE`, where `dbch_devicetype == DBT_DEVTYP_VOLUME` | "Volume notifications are also broadcast to top-level windows", with no registration; `RegisterDeviceNotification` refuses the volume type [S6][S7]. `dbcv_flags` is `DBTF_MEDIA` for media and `DBTF_NET` for a network drive [S8] | Window thread | A forced removal may skip the query, but `REMOVECOMPLETE` still arrives [S6]. Whether a VHD attach or a volume mounted on a folder is broadcast is not documented (E-3) | Files-column folders that answered `NotFound` or `Unreadable`, or could not be watched (`files.rs:200-216`, `files_watch.rs:72-81`); palette roots with no index; `PATH` entries on that drive (the walk) | **new arm**, `SystemNews::Volumes`, on the existing subclass | Re-asks through the files lane (newest request per folder) and the index register (epoch); one walk request |
| W8 | `WM_DEVICECHANGE` with `DBT_DEVNODES_CHANGED` | "A device has been added to or removed from the system": any device, no `lParam` [S6] | Window thread | **Misleading for us**: a mouse, a phone or a headset sends it | None | **none** (`system_news_of` ignores it) | — |
| W9 | `DBT_DEVICEQUERYREMOVE` on a **handle** | Only after `RegisterDeviceNotification(DBT_DEVTYP_HANDLE)` or `CM_Register_Notification(CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE)` on the open handle [S9][S10] | With `CM_Register_Notification`, a system thread, no window [S10] | "If the system forcibly removes a device, it may not send" the query [S6] | No fact Folio shows. The issue is that **our open watch handles may refuse an eject** (E-2) | **new, inside `DirWatch`** (§3.5) | On the signal: the watch closes its handle and reports that it has ended |
| W10 | Store and App Installer package events | `PackageCatalog.OpenForCurrentUser()` events `PackageInstalling`, `Updating`, `Uninstalling` and `StatusChanged`. It needs `packageQuery` or `packageManagement` **or medium integrity** [S11]; no package identity is required | A WinRT callback thread | — | A Store program's execution alias in `%LOCALAPPDATA%\Microsoft\WindowsApps` (on `PATH` by default), which the walk's rows read | **none**: W12 hears the alias itself (§3.2) | — |
| W11 | `ReadDirectoryChangesW` on Start-menu folders | [S12] | A watcher thread | — | **No reader**: no surface lists Start-menu shortcuts. Programs come from the profile table and `PATH` (`profiles.rs`) | none | — |
| W12 | `ReadDirectoryChangesW`, shallow and named, on each `PATH` directory of the logon block (`WindowsApps`, winget's `Links`, npm's global prefix, Cargo's `bin`, Python's user `Scripts`, …) | [S12]. A zero-length completion means the buffer overflowed [S12] (`DirChange::Unknown`, `lib.rs:16011-16015`) | A watcher thread | A network directory caps the buffer at 64 KiB [S12]. Whether creating an alias is heard is E-1 | The program rows behind "+", the default profile, `Split with`, the Agents rows, and the Copilot probe (through `ProgramNews::copilot`) | **new ear** (§3.2) | Lane (B2), after `WatchClock` |
| W13 | `WM_TIMECHANGE` | Broadcast after the system time changes [S13] | Window thread | — | **No reader**: Folio prints no local wall time. Git dates use the commit's own time zone (`git.rs:1556`). A search for `GetTimeZoneInformation`, `localtime` and `chrono::Local` finds no reader in product code | none | — |
| W14 | `WM_SETTINGCHANGE` with "intl" | Locale settings changed; `wParam` is 0 [S1] | Window thread | — | **No reader on Windows.** The UI language's "System" choice is the *display* language (`os_ui_language`, `lib.rs:14290-14311`), read at start and when the Language row is pressed (`main.rs:69469`). Number and date formats are not used. A display-language change takes effect at the next sign-in (**estimate**, from Windows' own Settings text, not a documented API contract) | Already heard as `Preferences`; nothing to read again | — |
| W15 | `RegNotifyChangeKeyValue` on `HKCU\…\Lxss` (WSL distributions) | [S14] | A waiting thread | — | WSL rows. Every surface that reads them walks when it opens (menus, the Profiles page; B2) | **none** (§3.4) | — |

### 2.2 macOS

| # | signal | mechanism, source | thread | reliability | Folio fact (reader) | ear | refresh |
|---|---|---|---|---|---|---|---|
| M1 | Appearance | Key-value observing on `NSApplication.effectiveAppearance` (`macos_impl.rs:1995`) | Main | — | Canvas | **existing** `Preferences` | Synchronous |
| M2 | Accessibility display options | `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` (`macos_impl.rs:2010`) | Main | — | Motion | **existing** `Preferences` | Synchronous |
| M3 | Screen parameters | `NSApplicationDidChangeScreenParametersNotification` (`macos_impl.rs:2020`) | Main | — | Frame rate (B3 row 4) | **existing** `Display` | Synchronous |
| M4 | `NSWorkspace.didMountNotification` | "posts when a new device mounts"; the object is the shared workspace [S15] | Main (the workspace centre, as for M2) | — | As W7 | **new arm**, `Volumes`, on the existing observer | As W7 |
| M5 | `NSWorkspace.didUnmountNotification` | "posts even if a volume becomes forcibly and immediately unavailable"; `userInfo` carries the mount path [S16] | Main | Reliable even for forced removal [S16] | As W7 | **new arm**, `Volumes` | As W7 |
| M6 | `NSWorkspace.willUnmountNotification` | "doesn't post if a volume becomes forcibly … unavailable" [S17] | Main | Not sent on forced removal | The macOS counterpart of W9. But an FSEvents stream holds no descriptor on the watched tree, so it does not block an unmount (**estimate**; the Mac half of E-2) | **none**, unless E-2 shows otherwise | — |
| M7 | `didLaunchApplication`, `didTerminateApplication` | Not posted for background apps [S18][S19] | Main | Incomplete by design | **No stale reader**: the summon asks which app to hand back to at that moment (`hotkey.rs:1002`, `frontmostApplication`), and the hand-off runs a fresh query instead of reading the cached list (`handoff.rs:1756-1758`) | none | — |
| M8 | `didActivateApplication` | [S20] | Main | — | As M7 | none | — |
| M9 | FSEvents on `/Applications` and `~/Applications` | [S21] | Stream thread | — | **No reader**: programs come from `PATH` and git's install roots (the walk); an app bundle is not a profile | none | — |
| M10 | FSEvents on the `PATH` directories of the logon environment (`/opt/homebrew/bin`, `/usr/local/bin`, Cargo's `bin`, …) | [S21]. One stream accepts many paths (`FSEventStreamCreate` takes an array of paths) [S21] | Stream thread | The stream's latency coalesces events | As W12. `brew install` links into Homebrew's `bin`, which is a `PATH` directory | **new ear** (§3.2) | Lane, after `WatchClock` |
| M11 | A Homebrew distributed notification | **None exists.** Homebrew's documentation names none, and `brew` posts nothing to `NSDistributedNotificationCenter`. This is our reading of the docs [S22]; there is nothing to cite *for* it. M10 is the ear | — | — | — | none | — |
| M12 | `NSLocale.currentLocaleDidChangeNotification` | "the user's locale changed … register … if your app displays content that is affected by the locale" [S23] | The posting thread (Foundation); the main thread for a change the user makes (**estimate**) | — | `system_locale_declaration` (`lib.rs:7807-7810`, a `OnceLock`): the `LANG` and `LC_CTYPE` every new pane is born with (`main.rs:39240`, `shell_integration.rs:636`, `:709`) | **new arm**, `SystemNews::Locale`, on the existing observer | A probe cell on a worker, run as a lane (two child processes, §3.3) |
| M13 | `NSSystemTimeZoneDidChange` | "posted when the time zone changes" [S24] | — | — | **No reader** (as W13) | none | — |
| M14 | `NSWorkspace.didChangeFileLabelsNotification` | Finder labels or colours changed; no `userInfo` [S25] | Main | — | **No reader**: the files column draws no Finder tags | none | — |
| M15 | The UI language (`NSLocale.preferredLanguages`) | Read at start and when the Language row is pressed (`macos_impl.rs:1911-1930`, `main.rs:69469`) | — | macOS applies a language change to an app at its next launch (**estimate**: a platform convention, not an API contract) | The Language row's "System" choice | none: not followed live. This is not raised in §8, because the Windows counterpart needs a new sign-in anyway | — |

### 2.3 Linux

The Linux port has **no ear and no watch**. `SystemSettingsWatch::install` subscribes
to nothing (`portable_impl.rs:522-531`), and every `DirWatch` constructor refuses
(`portable_impl.rs:845-866`). So nothing exists for `inotify` on
`~/.local/share/applications` or `/usr/share/applications` [S26], or for the desktop
portal's settings interface [S27], to plug into. Nothing Folio shows on Linux reads a
desktop-entry directory either: programs come from `PATH`, as on the other platforms.
**This note adopts no Linux signal.** When the port gets a `DirWatch` built on inotify,
it gets this note's PATH ear and palette ear for free, because both are written against
`DirWatch`'s contract rather than a platform.

---

## 3. What is adopted, signal by signal

### 3.1 Volumes (W7, M4, M5)

**The defect.** A files column rooted on a drive that is not there answers
`DirFault::NotFound` (`files.rs:200-216`) and holds no watch. Folio tries to watch it
again only "each time it comes on screen" (`files_watch.rs:72-81`). After the person
plugs the drive in, maps the share or mounts the image, the column still says "Folder
not found" until they fold and unfold the folder or switch tabs. The palette root on
that drive has no index, and the open's walk finds nothing. A `PATH` entry on that
drive answered "absent" for its programs.

**The ear.** The **existing** ear gains a fourth `SystemNews` variant, `Volumes`.

- **Windows.** `system_news_of` answers `Volumes` for `WM_DEVICECHANGE` **only** when
  `wParam` is `DBT_DEVICEARRIVAL` or `DBT_DEVICEREMOVECOMPLETE` **and** the `lParam`
  header's `dbch_devicetype` is `DBT_DEVTYP_VOLUME` [S6][S7][S8]. The pure function
  therefore takes the message, the `wparam` and the `lparam` header. Reading the header
  means reading one `u32` at a pointer the system owns for the length of the call,
  inside the subclass, before the wake. Every other device event answers `None`:
  `DBT_DEVNODES_CHANGED`, device-interface arrivals, ports.
- **macOS.** The observer adds two registrations, `didMount` and `didUnmount`
  [S15][S16], on the workspace centre it already uses (`macos_impl.rs:2004-2013`). Both
  map to `Volumes`.

**The arm.** `AppEvent::VolumesChanged` runs in every window (`for_each_window`):

1. Every folder on the glass whose node is `DirNode::Failed` is asked for again
   (`ask_files_dir`). Every folder whose watch entry holds no handle is dropped from
   `FilesWatch`, so the next `sync` tries it again. Both go through the files lane's
   existing queue, which keeps the newest request per folder.
2. Every palette root in `FileIndexes` that is `Missing`, or whose last index is empty
   because its root could not be read, is marked dirty. Nothing is walked now: the next
   open, or §4's ear, walks it.
3. The application makes one `programs_lane::request(Trigger::Volumes, …)` per event,
   not one per window (the lane would merge them anyway).

Removal needs no separate unwatch step. A watch on a removed volume fails, and
`FilesWatch::due` drops it (`files_watch.rs:194-199`). Step 1's re-ask then shows
`NotFound`, which is the honest answer.

**Cost.** Receiving the message is a match on three integers on the window thread. The
re-asks are one `read_dir` per failed folder on the files worker (**estimate**: µs to ms
per folder; in practice a column has at most one failed folder per root), plus one walk
(2–7 ms, B2§2). The window thread does nothing but bookkeeping.

### 3.2 The PATH directories (W12, M10)

**The defect.** B2 hung the walk on the broadcast, and the broadcast reports a **changed
variable**. Most installs an agent user makes change no variable. They drop a file into
a directory that is already on `PATH`:
- `npm i -g @github/copilot` writes `copilot.cmd` into npm's global prefix;
- `cargo install` writes into Cargo's `bin`;
- `winget install` of a portable package writes into winget's `Links`;
- a Store app writes an execution alias into `WindowsApps` (the Store PowerShell 7 of the
  2026-10-10 incidents is one);
- `brew install` writes into Homebrew's `bin`.

Today the walk then runs only when a menu or page opens. The menus show the right answer
because they walk when they open and the rows arrive (owner ruling 2026-10-04). **"+",
Ctrl+Shift+T, the summon's new window and the default profile do not**, because they
read the answer already held and have no opening to trigger a walk (B2§1.3, "default
profile: read per use").

**The ear.** `ProgramsEar` belongs to the application. After every walk lands it is
synced, as `FilesWatch::sync` is. Its wanted set is the walk's own `PATH` list from the
logon block (the walk already holds that list; it is published with `MachineFacts`).
Three kinds of entry are left out:
- Entries that are not absolute or do not exist. They cannot be watched, and the walk
  already answers them as absent.
- **Remote** entries: `GetDriveTypeW` answers `DRIVE_REMOTE`, or the path is UNC; on
  macOS, a path not on a local volume. The 64 KiB network buffer cap [S12] and a dead
  share's timeouts are not worth paying for a list of programs.
- **Removable** entries (`DRIVE_REMOVABLE`), until T-WATCH-LETS-GO has landed, so that the
  ear itself never blocks an eject.

Each directory kept is watched **shallow and named**.
- On Windows the watcher thread passes only names whose extension is in the logon
  block's `PATHEXT` (case-insensitive) and names that are reparse points. An execution
  alias needs nothing special beyond `.exe` (**estimate**, E-1).
- On macOS every name passes, because any file can be executable.
- A `DirChange::Unknown` (the buffer overflowed) always passes (`lib.rs:16011-16015`).

A name that passes stamps the ear's mailbox and wakes the loop. The loop runs one
`WatchClock` per directory. When a clock is due it makes **one**
`programs_lane::request(Trigger::PathDirectory, …)`.

**Threads.** Today's `DirWatch` uses one thread per directory, so 23 `PATH` directories
(the logon block of B2§2) would mean 23 `bt-dir-watch` threads idle in
`WaitForMultipleObjects`. Instead, the ticket adds **one** door with a set contract,
`DirWatchSet`:
- **Windows:** one thread waits on a stop event plus up to 63 directory completion
  events per `WaitForMultipleObjects` call (`MAXIMUM_WAIT_OBJECTS` is 64 [S28]). Each
  further 63 directories get a second thread. There is no cap that drops directories.
- **macOS:** one `FSEventStream` over the whole path array [S21]. Nothing on macOS ever
  required one stream per path.

The contract for each member is `start_shallow_named`'s. Its thread is a bare platform
thread entered through `spawn_at_priority`, like `bt-dir-watch` (`lib.rs:16416-16420`),
so A§0.1's count of spawn sites goes up by one.

**Cost** (estimates; the ticket's step 1 measures them, E-4):
- One 64 KiB buffer per watched directory (the existing `DIR_WATCH_BUFFER_BYTES`,
  `lib.rs:16052`). That is about 23 × 64 KiB ≈ 1.5 MiB committed on the machine B2
  measured, plus one thread per 63 directories.
- One walk per burst of installs: 2–7 ms (B2§2).
- The walk cannot hear itself. It only reads metadata and writes nothing, and
  `FILE_NOTIFY_CHANGE_LAST_ACCESS` is not in the filter (`lib.rs:16059-16067`).

**Why not `PackageCatalog` (W10).** It reports packages, but the walk reads files. A
Store app matters to Folio exactly when its alias appears in a `PATH` directory, and
the ear hears that. A package event would be a second road to the same walk, through
WinRT activation, for no fact the ear misses. Its documented requirement is
`packageQuery`, `packageManagement` **or medium integrity** [S11], so identity was never
the obstacle. Redundancy is.

### 3.3 The macOS locale (M12)

**The defect.** `system_locale_declaration` (`lib.rs:7807-7810`) is a `OnceLock`. Two
child processes fill it (`defaults read -g AppleLocale` and `locale -a`,
`lib.rs:7821-7833`) when the first pane is born. Window-waits row 7 still names that as
blocking, with no timeout, on the birth road (`window_waits.tsv:61`). A region change in
System Settings never reaches a new pane's `LANG` until Folio restarts.

**The ear.** `SystemNews::Locale`, from one more registration on the existing
observer's default centre: `NSCurrentLocaleDidChangeNotification` [S23].

**The owner.** The `OnceLock` takes B4's probe shape (B4§3; the model is
`attention_copilot`'s `CopilotProbe`):
- It holds the last answer and a request number.
- One probe at a time runs on a worker, and requests made meanwhile are merged.
- A failure answers "unknown" (no declaration, the `None` of today) and is not kept.
- An older answer never replaces a newer one.

Folio asks at launch and on `Locale`. A birth reads whatever answer is held and **does
not wait**, which is what retires row 7. If L3 (T-WINDOW-WAITS-048, plan line 162) moves
the probe off the birth road first, this ticket adds only the trigger. Whichever lands
second rebases onto the other.

**Scope:** new panes only. A running shell keeps its own environment (owner question Q4).

### 3.4 Considered and not adopted

| signal | why not |
|---|---|
| W10 `PackageCatalog` | Redundant with the PATH ear for every fact the walk reads (§3.2) |
| W11 Start-menu folders, M9 `/Applications` | Nothing reads them (§2) |
| W13 `WM_TIMECHANGE`, M13 time zone | Nothing reads local wall time (§2; the search is named there) |
| W14 "intl", M15 preferred languages | The Language row's "System" is the display language, which neither platform changes live (§2) |
| W15 WSL's `Lxss` key | Every surface that reads it walks when it opens (B2: menus, the Profiles page). A distribution installed while no menu opens changes nothing on screen |
| M6 `willUnmount` | FSEvents holds no descriptor (estimate; the Mac half of E-2), and `didUnmount` covers forced removal |
| M7, M8 app launch, terminate, activate | Read fresh at the moment they are used (§2) |
| M14 file labels | Folio does not draw them |
| W8 `DBT_DEVNODES_CHANGED` | Says nothing about volumes; it is §5's misleading signal |

### 3.5 A watch lets go of a volume that is being removed (W9; Windows)

**The risk.** A `DirWatch` keeps `CreateFileW(FILE_LIST_DIRECTORY)` open for as long as
its folder is on screen (`lib.rs:16368-16381`). The git watch keeps the repository
root's handle open for as long as a Git page or pane stands in it. The documented
contract for a program holding a handle is to register for that handle's removal events
and close the handle when asked [S9]. Folio does not. If E-2 confirms that an armed watch
makes "Safely remove" answer "in use", then every files column over a USB stick blocks
its eject today, and §3.1 and §4 would add more such handles.

**The design.** It lives inside `DirWatch` and `DirWatchSet`, on Windows only.
1. Once the handle is open, the watch calls `CM_Register_Notification` with
   `CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE` on it [S10]. This needs no window and is
   available from Windows 8.
2. The callback is an OS-owned callback, entered as
   `admission::enter_callback("dir-watch-removal")` (TD§3.3).
3. On `CM_NOTIFY_ACTION_DEVICEQUERYREMOVE` and `…REMOVEPENDING`, the callback signals the
   watch's stop event and waits until the watcher thread has closed the directory
   handle. That wait lasts at most one turn of the thread's `WaitForMultipleObjects`.
   The callback then returns `ERROR_SUCCESS`, never `ERROR_CANCELLED`: Folio never
   refuses an eject.
4. The watch reports that it has ended through the existing `take_failure`
   (`files_watch.rs:194-199`). The column keeps its last listing, and the next `Volumes`
   arrival (§3.1) arms it again.
5. `DirWatch::drop` calls `CM_Unregister_Notification`; the callback never does,
   because it must return quickly [S10].
6. On `…QUERYREMOVEFAILED` nothing reopens. The next `sync` arms the folder again,
   which is the files watch's rule 3 (`files_watch.rs:40-46`).

The callbacks run on a system thread. The only state they share is the stop event and
an acknowledgement event, and no window-thread wait appears.

---

## 4. The deep palette refresh

### 4.1 What "deep" has to mean after B3

B3 made every palette open walk its roots (`palette_index.rs:593-604`). So "the next
open is instant" is already true for **the rows that existed at the last walk**. The
held index shows at once, and the open's walk replaces it 40–80 ms later when the
machine is idle, or **5–10 s later with every core busy** (B3§2). The gap between those
two moments is what is still missing. A file made under a folded folder since the last
walk appears only when the open's own walk lands, and under load that is seconds after
the person has started typing. "Deep" therefore means two things: **the index is current
before the palette opens, and an open costs nothing when nothing has changed.**

### 4.2 The three candidates of the brief

- **(a) Walk again on an OS signal.** Adopted in the narrow form that is honest. The
  signal is the root's own disk changing (§4.3), plus `Volumes` for a root that was not
  there before (§3.1). A package install, or a drive elsewhere, says nothing about a root.
- **(b) A periodic walk.** **Rejected.**
  - For a watched local root, nothing changes without a notification, so a timer would
    only read again what is already known.
  - For an unheard root (a network share, `\\wsl$`), a timer is exactly the poll of a
    share that R31 forbids. It would also stall the index worker on a schedule for
    someone on a slow VPN.
  - No exception exists. The only case a timer would serve is an unheard root, and B3's
    walk on open already serves it by reading at the moment someone asks.
- **(c) Walk the folded subfolders ahead of time.** By design the files watch does not
  cover them: it watches what is on the glass (`files_watch.rs:24-32`). Widening it to
  every folder of a root would mean hundreds of handles. One **tree** watch per root
  covers them with one handle. But the existing tree door reads no names, so a
  repository's `target\` would wake the index on every compiler write (the 188 ms
  finding, `lib.rs:15996-16004`). So (c) is adopted only with a **named tree** contract
  that filters by the index's own definition.

### 4.3 The one design

**`DirWatch::start_named`** is the platform half of T-PALETTE-DEEP, on both platforms. It
is the tree contract with names. On Windows it calls
`ReadDirectoryChangesW(bWatchSubtree = TRUE)` and reads the `FILE_NOTIFY_INFORMATION`
names, which are relative to the root [S12]. On macOS it uses the FSEvents paths the
stream already delivers [S21]. The wake receives `DirChange::Named(relative)` or
`DirChange::Unknown`.

**The filter** runs on the watcher thread and is pure. `palette_index::bears_on_index(relative)`
answers `false` when any component of the name is in `SKIPPED_DIRECTORIES`, or when the
name is deeper than `INDEX_MAX_DEPTH`. Otherwise it answers `true`. `Unknown` (overflow)
answers `true`. This is the walk's own definition (`palette_index.rs:93`, `:119-133`), so
a name the walk could never hold never wakes anything. `cargo build` writing into
`target\`, `npm` writing into `node_modules\` and git writing into `.git\` are all silent.

**The ear, `IndexEar`,** belongs to the application, beside `FileIndexes`. Its wanted set
is exactly the set `retain_file_indexes` already computes (`main.rs:63092-63101`). So it
follows the columns of every window's front tab, and it is synced on the same turn. A
root whose watch cannot be opened, or fails later, is **unheard**, and the register
records that beside the entry.

**What a heard change does.**
1. The watcher stamps the mailbox and sends `AppEvent::FileIndexHeard`.
2. On the loop, `mark_dirty(root)` runs and the root's `WatchClock` notes the change.
3. When the clock is due (after 300 ms of quiet, and at most once every 2 s while writes
   continue, `watch_clock.rs:33`, `:51`), `claim(root)` runs and the request goes to the
   existing `bt-index-worker`.
4. `accept` keeps `dirty`, as it does today (`palette_index.rs:606-621`). After an
   accept, a root that is still dirty and whose clock is due is claimed once more.

So each root has **one walk out and at most one owed.**

**What the open does.** `reopened` walks a root only when it is **missing, dirty or
unheard**. A ready root that is heard and clean is current, and is not walked. The merged
case and its diagnostics line stay. For an unheard root, this is B3's behaviour
unchanged: the honest answer where no signal exists.

**Memory.** No new cap is needed and none is added. The register holds only retained
roots, and `INDEX_MAX_ENTRIES` bounds each one. **Estimate** of that bound:
- An `IndexedFile` is three heap strings (`palette_index.rs:146-158`): about 72 B of
  headers plus about 130 B of text for a typical path, so about 200 B in all.
- At the cap of 30 000 entries that is about 6 MB per root. It doubles while a walk's
  answer and the held index exist at the same time.
- A repository of 1 883 entries takes about 0.4 MB.

The ear adds one handle, one thread and one 64 KiB buffer per root.

**How it yields to the window thread.** The walk stays on the `BelowNormal` index worker
(`palette_index.rs:403-410`). Under load the worker waits, and that is how it yields.

On `FileIndexReady` the window thread does what it does today: an `accept` that moves a
`Vec`, and a palette refill only in windows that have a palette up
(`main.rs:63067-63082`). Beyond that it adds the ear's `sync` (a set comparison, in the
shape of `FilesWatch::sync`) and a `WatchClock` per root. It waits on nothing:
`window_waits.tsv` is unchanged and `check-window-waits` stays green.

The watcher thread follows the callback rule: stamp, wake, nothing else (TD§3.3; the
model is `files_watch.rs:255-261`).

**What a person sees.** In the common case, nothing: the rows are already current when
the palette opens. If the palette is up when a walk lands, the new rows arrive and the
selection follows its item (B3's `same_item` refill and `show_selected`). No
"refreshing" row is added. `Indexing…` stays what it is today: the note for a root that
has **no** index yet (`runtime/files.rs:2813-2827`).

One change is worth naming. Today every open marks a root as `Building` for the length of
its walk, so `Indexing…` can show when the held index matched nothing. After this design,
an open over a heard root does not.

**The storm bound, worked through.**
- Someone saves a file in a watched repository every second for a minute: at most 30
  walks (one per 2 s floor). Each takes 40–80 ms below normal priority when idle (B3§2),
  about 2–4 % of one core.
- A `cargo build`: no walks, because the filter drops its writes.
- A `git checkout` of a large branch: one overflow or one burst, so one walk after 300 ms
  of quiet.

### 4.4 Why not reuse the git watch

The git watch already holds a tree watch on a repository root, but its contract reads no
names and must not (`lib.rs:15989-16000`). A palette root is also often not a repository
root. Two tree handles on one root are the price of two contracts, and each subject keeps
its own registration.

### 4.5 The volumes link

A palette root on a drive that appears is `Missing`, because it never answered. §3.1 step
2 marks it dirty. When the ear's `sync` then arms the root, the files watch's rule 3
applies here too: **arming is a reason to walk**, since a root whose watch has just opened
was not being heard a moment ago. So the arm claims it.

---

## 5. Cost and risk, per signal

| signal | storm | coalescing | a misleading signal | identity |
|---|---|---|---|---|
| W1, W2, W3: `WM_SETTINGCHANGE`, `WM_THEMECHANGED` | Installers broadcast `Environment` several times; Settings pages broadcast for each toggle. There is one wake **per window** (§1.1) | The walk lane: one out, one waiting (B2§2). Preference reads take µs; the taskbar lane merges | A broadcast for an unrelated area reaches the arm: one wasted walk, at most 7 ms on a worker (B2§2) | None: a window message |
| W1, on the sender's side | The broadcaster waits in `SendMessageTimeout` for each top-level window [S1] | The subclass only sends an event and returns (`lib.rs:11034-11053`) | — | — |
| W4 `WM_DISPLAYCHANGE` | A dock with two panels sends a few messages | Each is a 28 µs read per window (B3§2) | A resolution change with the same rate: one wasted read | None |
| W6 `WM_INPUTLANGCHANGE` | Cycling through layouts | 12 µs each; the hotkey is claimed again only when its key moved (B3) | — | None |
| W7 volumes | A hub with four drives: four arrivals, times the number of windows | The files lane's queue (newest per folder), the register's epoch, the walk lane | `DBT_DEVNODES_CHANGED` (a phone, a mouse) is **not** heard (§3.1). A phone over MTP is no volume, so silence is correct. A card reader without media sends `DBTF_MEDIA` only when a card goes in [S8] | None |
| W9 removal query | One per handle on the volume | Each callback closes its own watch | A forced removal skips the query [S6]; `REMOVECOMPLETE` and the watch's own failure cover it | None |
| W12, M10: the PATH ear | `npm i -g` writes a few shims into its prefix (its `node_modules` is below the shallow watch). Windows Update writes into `System32` | The `PATHEXT` name filter (Windows), a `WatchClock` per directory, the walk lane | A log file written into a `PATH` directory: filtered by extension on Windows; at worst one walk of up to 7 ms every 2 s on macOS | **None**: a file-system watch. The Store alias appears in the user's `WindowsApps` whether or not Folio has package identity. In any case the run drops its identity at start (the ledger's 2026-10-10 correction: the process with identity lived 12 s, and the run itself had none) |
| M4, M5 mount | Time Machine snapshot mounts; **Folio's own update DMG** (`macos_update.rs`) | As W7 | The DMG mount is heard: one more read of the folders that failed, harmless | Not applicable |
| M12 locale | Rare | One probe out, merged (B4's shape) | A locale change that does not move the declaration: one probe, two child processes, on a worker | Not applicable |
| The palette ear | Builds, checkouts, editors | The index filter, `WatchClock`, one walk out and one owed per root | An overflow (`Unknown`) causes one walk | None |

**Why Store events would have been the risky choice.** The 2026-10-10 incidents
(`00-LEDGER-048.md`) concern the Store PowerShell's *activation*, not its install, and
they belong to T-STORE-PWSH. A `PackageCatalog` subscription would have been a WinRT
callback in a process that starts with identity and then relaunches without it, so one
run would see the catalog from two different points of view. The PATH ear has no such
split.

---

## 6. Experiments owed (each before the ticket step that rests on it)

- **E-1 (T-OS-SIGNALS-WIN step 1).** In the clean VM, arm a shallow named watch on
  `WindowsApps` and on `Links`. Install a Store app with an execution alias and a winget
  portable package. Record the names and actions delivered. This settles §3.2's
  Windows name filter.
- **E-2 (T-WATCH-LETS-GO step 1).** On a USB stick or an attached VHD, arm (1) a shallow
  `DirWatch` and (2) a tree `DirWatch`, then ask Windows to eject. Record whether the
  eject is refused while the watch is armed, and allowed once it is dropped. The Mac half:
  an FSEvents stream on a mounted DMG, then `diskutil eject`. If neither platform
  refuses, T-WATCH-LETS-GO is closed with this evidence and §3.2 stops excluding
  removable entries.
- **E-3 (T-OS-SIGNALS-WIN step 1).** Find which volume events reach a subclassed winit
  top-level window for: a USB stick, a VHD attach, `subst`, a mapped network drive
  (`DBTF_NET`), and a volume mounted on an NTFS folder. Any case that sends no broadcast
  is listed as unheard in the ticket's report, where folding and unfolding the folder
  stays the way to refresh it. It is not patched over.
- **E-4 (T-OS-SIGNALS-WIN step 1).** Measure the idle cost of `DirWatchSet` over the
  logon block's `PATH`: threads, committed bytes, and wakes per hour on an idle machine
  and during a Windows Update. Count walks per `npm i -g`.
- **E-5 (T-PALETTE-DEEP step 1).** Measure a palette walk on a cold disk after boot (B3
  measured only warm and loaded walks). Count walks per minute during an editing session
  and during a `cargo build` with the filter on.

---

## 7. Tickets, in dependency order

Each ticket starts from a worktree off `main` and runs the usual gates:
- rustfmt, and workspace clippy with `-D warnings`;
- the whole bt-app suite, plainly and under `env -u TERM_PROGRAM …`;
- the crate suites;
- every `scripts/ci/check-*.ps1`, including `check-window-waits`, with
  `window_waits.tsv` unchanged;
- the Mac gate for any `cfg(macos)` arm.

Each test below must be seen failing under its named mutation, after which the mutation
is reverted.

### 7.1 T-WATCH-LETS-GO (S, Windows; first)

E-2 runs first. If it confirms the risk:
- All three `DirWatch` constructors call `CM_Register_Notification(DEVICEHANDLE)` on
  their handle.
- The callback, entered as `Callback("dir-watch-removal")`, stops the watch, waits for the
  handle to close, and returns `ERROR_SUCCESS`.
- `Drop` unregisters.
- The watch reports that it has ended through `take_failure`.

Tests and the mutation each must fail under:
- `bt-platform` `dir_watch_removal_tests::a_query_remove_closes_the_watch_before_the_callback_returns`
  (the callback body is driven directly with a fake action). MUTATION: the callback
  signals stop without waiting for the close.
- `…::a_removal_is_never_refused`. MUTATION: answer `ERROR_CANCELLED` on query-remove.
- `…::a_watch_ended_by_its_volume_reports_itself_ended`. MUTATION: the thread exits
  without filing the failure.
- `files_watch::tests::a_watch_ended_by_removal_is_dropped_and_rearmed_on_the_next_sync`.
  MUTATION: `due` keeps an ended watch.
- A source guard: `CM_Unregister_Notification(` appears only in `Drop`. MUTATION: call it
  from the callback.

### 7.2 T-OS-SIGNALS-WIN (M; after 7.1)

E-1, E-3 and E-4 run first. The work, in three parts:
1. `SystemNews::Volumes`; `system_news_of` takes the device header;
   `AppEvent::VolumesChanged` and its arm (§3.1).
2. `DirWatchSet`, with its Windows arm. The portable arm refuses, as `DirWatch` does.
3. `ProgramsEar`; `MachineFacts` carries the walk's `PATH` list;
   `Trigger::PathDirectory`; the `PATHEXT` filter.

Docs: a DESIGN entry; A§0.1 (the `AppEvent` count and one more spawn site); A§4.4 rows;
A§5.1 (the lane's triggers); and one CHANGELOG line: "A program installed while Folio
runs — by npm, cargo, winget or the Store — is in the new-tab button and the default
profile without opening a menu; a drive plugged in is read by the files column".

Tests and the mutation each must fail under:
- `bt-platform` `system_preference_message_tests::a_volume_arriving_or_leaving_is_heard_and_no_other_device_is`,
  a table of volume arrival, volume remove-complete, `DBT_DEVNODES_CHANGED`, a
  device-interface arrival and a port. MUTATION: answer `Volumes` for every
  `WM_DEVICECHANGE`.
- `app_windows_tests::each_system_news_wakes_the_loop_for_its_own_fact`, extended with
  `Volumes`. MUTATION: `of_system_news(Volumes)` answers `SystemPreferencesChanged`.
- `runtime::files` `volumes_tests::a_folder_that_was_not_found_is_asked_again_when_a_volume_arrives`.
  MUTATION: the arm skips `Failed` nodes.
- `…::an_unwatched_folder_is_attempted_again_after_a_volume_arrives`. MUTATION: the arm
  leaves `FilesWatch`'s entries that hold no handle.
- `programs_ear::tests::a_program_written_into_a_path_directory_is_walked_for`, with a
  fake set door and a fake clock. MUTATION: the ear never requests.
- `…::the_watched_set_follows_the_walks_path`. MUTATION: sync from the launch environment
  instead of the walk's block.
- `…::remote_and_missing_path_entries_are_not_watched`. MUTATION: drop the drive-type
  filter.
- `…::a_name_outside_pathext_does_not_wake_and_an_overflow_does`. MUTATION: filter
  `Unknown` out.
- `bt-platform` `dir_watch_set_tests::one_thread_watches_many_directories_and_names_each`
  and `…::a_sixty_fourth_directory_gets_a_second_thread`. MUTATIONS: one thread per
  directory (caught by the thread count); a hard cap at 63 (the 64th goes unheard).
- `programs_lane::tests::a_burst_of_path_news_is_one_walk_out_and_one_waiting`.
  MUTATION: `request` starts a walk for every call.

### 7.3 T-OS-SIGNALS-MAC (M; after 7.2, whose app-side arms it shares)

The work, in three parts:
1. `didMount` and `didUnmount` map to `Volumes` on the existing observer.
2. `DirWatchSet`'s macOS arm: one `FSEventStream` over the path array.
3. `SystemNews::Locale` from `NSCurrentLocaleDidChangeNotification`, and
   `AppEvent::LocaleChanged`. The locale declaration becomes a cell holding the last
   answer and a request number, answered on a worker (B4's shape). A birth reads the
   held answer. Window-waits row 7 is retired, unless L3 has already retired it: the
   diff of the TSV must show the row leaving and nothing entering.

Run the Mac gate.

Tests and the mutation each must fail under:
- macOS `system_settings_watch_tests::mount_and_unmount_are_heard_as_volumes`, which
  posts the two notifications on the workspace centre. MUTATION: register only
  `didMount`.
- A macOS source pin: the observer's `Drop` removes all five registrations. MUTATION:
  drop one of the removals.
- `locale_declaration::tests::a_changed_locale_is_asked_again_and_an_older_answer_never_lands_over_a_newer`.
  MUTATION: `publish` files every answer.
- `…::a_failed_probe_answers_none_and_is_asked_again`. MUTATION: keep the failure.
- `…::a_birth_never_waits_for_the_probe`, with a fake probe held by a gate; the birth
  must complete. MUTATION: the birth blocks on the cell.
- macOS `dir_watch_set_tests::one_stream_hears_every_member`. MUTATION: create the stream
  with only the first path.

### 7.4 T-PALETTE-DEEP (M; after 7.1, so the tree ear never blocks an eject, and after 7.2, for `Volumes`)

E-5 runs first. The work, in four parts:
1. `DirWatch::start_named`, with both platform arms.
2. `palette_index::bears_on_index`.
3. `IndexEar`, synced from the retained set; an `unheard` flag beside each entry;
   `AppEvent::FileIndexHeard`; a `WatchClock` per root; a claim when the clock is due and
   after an accept.
4. `reopened` walks only roots that are missing, dirty or unheard.

Docs: a DESIGN entry; A§4.4 (the owner line in the palette row); an amendment to DESIGN
§7.55 ④; and one CHANGELOG line: "Files made anywhere under a folder in the files column
are in the command palette when you open it".

Tests and the mutation each must fail under:
- `palette_index::tests::a_name_under_a_skipped_folder_or_too_deep_does_not_bear_on_the_index`,
  a table including `target/x`, `a/node_modules/b`, a name at depth 7, `src/new.rs` and
  `Unknown`. MUTATION: check only the first component.
- `…::a_heard_change_walks_the_root_before_the_next_open`, with a fake ear and a fake
  clock. MUTATION: the due clock only marks the root dirty.
- `…::an_open_over_a_heard_clean_root_does_not_walk`. MUTATION: `reopened` behaves as in
  B3 (always walks).
- `…::an_open_over_an_unheard_root_walks`. MUTATION: treat unheard roots as heard.
- `…::changes_during_a_walk_are_one_more_walk_and_no_more`. MUTATION: claim on every
  heard stamp.
- `…::a_root_whose_watch_just_armed_is_walked`. MUTATION: arming does not count as news.
- `index_ear::tests::the_ear_follows_the_retained_roots`. MUTATION: never drop a root.
- `bt-platform` `dir_watch_named_tree_tests::a_file_two_levels_down_is_named_relative_to_the_root`,
  on Windows and macOS. MUTATION: `bWatchSubtree = FALSE`, or names read from the shallow
  road.
- `hang_watch::window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site` stays
  green with the TSV unchanged. This one has no mutation; it is the census itself.

---

## 8. Questions for the owner (recommendation first)

**Q1. Should Folio watch the `PATH` directories, so that a program installed by npm,
cargo, winget's portable packages, the Store or Homebrew appears in "+" and the default
profile without opening a menu?** **Recommendation: yes.** The cost is one watcher
thread per 63 local directories (one, in practice) and a 2–7 ms walk per install. Remote
and removable `PATH` entries are not watched. Without this, those installs keep needing
a menu to open before "+" uses them.

**Q2. Should the palette keep its index current in the background, so that an open finds
new files at once and an unchanged root is not walked at all?** This means a filtered
tree watch per palette root, at most one walk every 2 s while files change, and build
folders ignored. **Recommendation: yes.** The cost is one handle and one thread per root:
about 2–4 % of a core while a file is saved every second (§4.3), and nothing during a
build.

**Q3. When Windows asks to eject a drive Folio is watching, should Folio let it go
silently and never refuse?** The column would keep its last listing and read the folder
again when the drive is back. **Recommendation: yes, silently, with no toast.** The person
asked Windows for the eject, and a refusal or a message would be Folio arguing with them.
This applies only if E-2 shows that the watches block an eject today.

**Q4. On macOS, after a region or locale change, should new panes get the new `LANG`
while running panes keep theirs?** **Recommendation: yes, new panes only, with no notice.**
This is the same rule as the 2026-10-05 environment ruling: a pane is born with the
account's current state, and a running shell is not rewritten.

---

## 9. Sources

Windows (Microsoft Learn, read 2026-10-11):

- [S1] `WM_SETTINGCHANGE`: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-settingchange. Sent with `SendMessageTimeout(HWND_BROADCAST)`; `lParam` "Environment", "intl", "Policy"; the `lParam` "does not usually indicate which specific system parameter changed".
- [S2] `WM_THEMECHANGED`: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-themechanged
- [S3] `WM_DISPLAYCHANGE`: https://learn.microsoft.com/en-us/windows/win32/gdi/wm-displaychange
- [S4] `WM_DPICHANGED`: https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged
- [S5] `WM_INPUTLANGCHANGE`: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-inputlangchange
- [S6] `WM_DEVICECHANGE`: https://learn.microsoft.com/en-us/windows/win32/devio/wm-devicechange. The event table, including `DBT_DEVNODES_CHANGED`; "If the system forcibly removes a device, it may not send a DBT_DEVICEQUERYREMOVE message".
- [S7] `RegisterDeviceNotificationW`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerdevicenotificationw. "Volume notifications are also broadcast to top-level windows, so the function fails if dbch_devicetype is DBT_DEVTYP_VOLUME"; handle events quickly.
- [S8] `DBT_DEVICEARRIVAL`: https://learn.microsoft.com/en-us/windows/win32/devio/dbt-devicearrival. Media insertion is `DBT_DEVTYP_VOLUME` with `DBTF_MEDIA`. `DEV_BROADCAST_VOLUME` (`DBTF_NET`): https://learn.microsoft.com/en-us/windows/win32/api/dbt/ns-dbt-dev_broadcast_volume
- [S9] Processing a request to remove a device: https://learn.microsoft.com/en-us/windows/win32/devio/processing-a-request-to-remove-a-device. Register `DBT_DEVTYP_HANDLE` on the open handle, and close it on `DBT_DEVICEQUERYREMOVE`.
- [S10] `CM_Register_Notification`: https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_register_notification. `CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE`; a query-remove is answered with `ERROR_SUCCESS` or `ERROR_CANCELLED`; handle events quickly; no window handle needed.
- [S11] `PackageCatalog.OpenForCurrentUser`: https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.packagecatalog.openforcurrentuser. It "requires that the caller has either the `packageQuery` or the `packageManagement` capability; or else that the caller has integrity level (IL) greater than or equal to Medium".
- [S12] `ReadDirectoryChangesW`: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw. `bWatchSubtree`; a zero-byte completion on overflow; the 64 KB limit for a network directory.
- [S13] `WM_TIMECHANGE`: https://learn.microsoft.com/en-us/windows/win32/sysinfo/wm-timechange
- [S14] `RegNotifyChangeKeyValue`: https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regnotifychangekeyvalue
- [S28] `WaitForMultipleObjects` (`MAXIMUM_WAIT_OBJECTS`): https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects

macOS (Apple Developer Documentation, read 2026-10-11 through its JSON data):

- [S15] `NSWorkspace.didMountNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didmountnotification. "posts when a new device mounts".
- [S16] `NSWorkspace.didUnmountNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didunmountnotification. "posts even if a volume becomes forcibly and immediately unavailable".
- [S17] `NSWorkspace.willUnmountNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/willunmountnotification. "doesn't post if a volume becomes forcibly and immediately unavailable".
- [S18] `didLaunchApplicationNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didlaunchapplicationnotification. Not posted for background apps.
- [S19] `didTerminateApplicationNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didterminateapplicationnotification
- [S20] `didActivateApplicationNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didactivateapplicationnotification
- [S21] File System Events: https://developer.apple.com/documentation/coreservices/file_system_events. `FSEventStreamCreate` takes an array of paths.
- [S22] Homebrew documentation: https://docs.brew.sh/. No notification mechanism is documented (the absence is our reading).
- [S23] `NSLocale.currentLocaleDidChangeNotification`: https://developer.apple.com/documentation/foundation/nslocale/currentlocaledidchangenotification. "the user's locale changed … Register for this notification if your app displays content … affected by the locale".
- [S24] `NSSystemTimeZoneDidChange`: https://developer.apple.com/documentation/foundation/nsnotification/name-swift.struct/nssystemtimezonedidchange
- [S25] `didChangeFileLabelsNotification`: https://developer.apple.com/documentation/appkit/nsworkspace/didchangefilelabelsnotification

Linux:

- [S26] `inotify(7)`: https://man7.org/linux/man-pages/man7/inotify.7.html
- [S27] XDG Desktop Portal, Settings: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html

Folio (at `ae933eb7`):
- every `file:line` above;
- B2§1–§2, B3§1–§2 and B4§1–§2;
- the DESIGN entries dated 2026-10-08: "The program list follows the machine…"
  (T-PROGRAMS-REFRESH), "The machine facts frozen at launch…" (T-FRESH-FACTS) and "The
  one-shot PowerShell probes stop keeping a failure…" (T-PROBE-NO-CACHED-FAILURE);
- A§4.4's T-FRESH-FACTS row;
- TD§3.3.

---

## 10. Architecture impact (for the tickets' briefs)

- `bt_platform::SystemNews` gains `Volumes` (Windows and macOS) and `Locale` (macOS).
  `AppEvent` gains `VolumesChanged`, `LocaleChanged` and `FileIndexHeard`, which changes
  A§0.1's count.
- Two new platform doors: `DirWatchSet` (a set contract, shallow and named for each
  member) and `DirWatch::start_named` (tree, named). Each adds one spawn site through the
  thread door (A§0.1, TD§5's census).
- New owners (A§4.4):
  - `ProgramsEar`, owned by the application; its writer is the window thread's sync after
    a walk lands.
  - `IndexEar`, owned by the application; its writer runs on the same turn as
    `retain_file_indexes`.
  - The locale declaration cell, with one writer: its probe worker.
- `programs_lane::Trigger` gains `Volumes` and `PathDirectory` (A§5.1).
- `window_waits.tsv` is unchanged by every ticket except T-OS-SIGNALS-MAC, which may only
  **remove** row 7.
- There is no timer anywhere. Every clock added is a `WatchClock` that a notification
  starts (R31).
