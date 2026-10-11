# OS-level change signals and the deep palette refresh: T-OS-SIGNALS and T-PALETTE-DEEP (0.4.8 B5), design note, 2026-10-11

Status: **revision (b), 2026-10-11**, after the Codex review of `3de187a4`
(`B5-os-signals-design.review-codex.md`, verdict *adopt with changes*, twelve required
changes). Revision (b) is the dated section at the end. **Where revision (a)'s text and
revision (b) differ, revision (b) stands**: in particular §2's totals, §3.1's forced-removal
sentence, §3.5's procedure, §4.3's queue, filter and memory sentences, §6, §7 and §8 are
replaced by (b)1–(b)14. **Revision (c)**, after the Codex confirmation round
(`B5-os-signals-design.review-codex-b.md`), follows (b) and stands wherever the two differ:
(c)1–(c)10 replace (b)3's callback, (b)4's tests, (b)6's rerun, (b)11's probe door and
retry rule, (b)13's ticket sizes and (b)14's Q2, Q3 and Q5. **Revision (d)**, after the
scoped confirmation (`B5-os-signals-design.review-codex-c.md`), replaces (c)1's slot protocol
(a close-request bit; one closer in every interleaving) and stands over (c) there. Revision (a) was the draft for review, written before any ticket
below was dispatched.
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

---

## Revision (b), 2026-10-11, after the Codex review

Codex reviewed `3de187a4` (`B5-os-signals-design.review-codex.md`). Its verdict was
**adopt with changes**: the direction is sound, but the note was not ready to generate
build tickets. The review lists twelve required changes. **All twelve are adopted**;
none is argued against, so the Departures heading below records no departure. Each
change is checked against the code at `ae933eb7`. Where the review cites
`crates/bt-model/src/palette_index.rs`, the file at this base is
`crates/bt-app/src/palette_index.rs`; the functions it names are the same.

Two of the review's findings are facts that revision (a) stated wrongly. They are
corrected here, not just qualified:

- **§1.3, "a watch that fails later is dropped silently".** At this base, `DirWatch::is_armed`
  answers `true` on both platforms, and `take_failure` answers `None` on both
  (`lib.rs:16274-16281`, `macos_watch.rs:365-372`). So `FilesWatch::due`'s failure arm
  (`files_watch.rs:194-199`) never fires. A watcher thread that ends on an error
  ends silently, and its folder stays counted as watched. (b)5 fixes this.
- **§4.3, "one walk out and at most one owed".** `FileIndexes::claim`
  (`palette_index.rs:552-570`) issues a new epoch for a dirty root **even while its walk
  is out**, and the worker serves a plain FIFO queue (`palette_index.rs:403-438`). A
  walk that lasts longer than the 2 s floor can therefore queue several superseded walks.
  Each runs in full and then has its answer dropped by `accept`. (b)6 fixes this.

### (b)1 · The inventory's classification and totals: adopted

The sorting rule of §0 (a fact read at an opening is as fresh as that opening) is a
freshness rule, and it does not classify every row. The review names three rows that
fall outside it. The inventory now uses eight classes and two decision rules:
- **freshness**: does a reader go stale without an ear?
- **redundancy**: does another adopted ear already deliver the same mutation?

| class | meaning | rows | count |
|---|---|---|---|
| **A** adopted for freshness | A reader goes stale without it, and no other ear covers it | W7, W12, M4, M5, M10, M12 | 6 |
| **L** lifecycle prerequisite | It refreshes no fact; another adopted ear needs it to be safe | W9 (the removal handshake that the volume and PATH ears need) | 1 |
| **H** already heard and used | The existing ear delivers it to a reader | W1–W6, M1–M3 | 9 |
| **U** already heard, no applicable reader | The existing ear delivers it, and no reader needs it | W14 ("intl" arrives as `Preferences`; the Language row's "System" is the display language) | 1 |
| **I** heard and deliberately ignored | It reaches the subclass and is filtered out as misleading | W8 (`DBT_DEVNODES_CHANGED`) | 1 |
| **R** passes the freshness rule but redundant | A displayed fact can go stale through it, but an adopted ear hears the same mutation more directly | W10 (`PackageCatalog`: W12 hears the alias appear) | 1 |
| **O** covered by an opening | Every reader walks when it opens | W15 (WSL's `Lxss` key) | 1 |
| **N** not subscribed | No reader, or the fact is read fresh when used, or the signal does not exist | W11, W13, M6 (pending E-2's Mac half), M7, M8, M9, M11, M13, M14, M15 | 10 |

The total is 30 (15 on Windows, 15 on macOS). §0's sentence "seven signals in four
families, nine already heard, fourteen no ear" is replaced by this table.

**Linux** (§2.3) is recorded as **deferred implementation debt**, not as a platform with
nothing to hear. The port has no settings ear and no `DirWatch` (`portable_impl.rs:513-531`,
`:826-866`). An inotify-backed `DirWatch` and the desktop portal's settings interface
[S26][S27] are port work that this note does not schedule. When the port gets a
`DirWatch` with this note's contract, the PATH ear and the palette ear can be enabled for
Linux by a ticket that runs their tests there. Nothing arrives on Linux automatically.

### (b)2 · `WM_DEVICECHANGE` payload validation, and the forced-removal sentence: adopted

`system_news_of` becomes `system_news_of(message, wparam, header: Option<DeviceHeader>)`,
and the subclass builds `DeviceHeader` with one pure, separately tested reader,
`device_header(lparam) -> Option<DeviceHeader>`. That reader:
- answers `None` for a zero `lParam`;
- reads the `DEV_BROADCAST_HDR` with `ptr::read_unaligned`, never through a reference,
  because the sender owns the memory and its alignment is not promised;
- answers `None` when `dbch_size` is smaller than `size_of::<DEV_BROADCAST_HDR>()`;
- reads `dbch_devicetype`, and nothing past the header. `Volumes` needs only the type, so
  `dbcv_unitmask` and `dbcv_flags` are not read. A later reader of those fields must check
  `dbch_size >= size_of::<DEV_BROADCAST_VOLUME>()` first.

`Volumes` is answered only for `DBT_DEVICEARRIVAL` or `DBT_DEVICEREMOVECOMPLETE`, with a
valid header whose type is `DBT_DEVTYP_VOLUME`. **`DBT_DEVICEQUERYREMOVE`,
`…QUERYREMOVEFAILED` and `…REMOVEPENDING` never become `SystemNews`.** They belong to
(b)3's handshake, which runs on the handle notification and not on the window.

**The forced-removal sentence is softened.** Revision (a) said, in §2 W7 and §5, that a
forced removal "still" delivers `DBT_DEVICEREMOVECOMPLETE`. The documentation says only
that the system "may not send" the query on a forced removal [S6]. It allows removal
events without an earlier query, and it promises no particular message for every forced
loss. The design therefore does not rely on one. **The fallback for a volume that
vanishes without a message** is the watch's terminal failure ((b)5), which makes the
folder or root unheard, plus the next read, which answers `NotFound`.

### (b)3 · The query-remove procedure is a state machine; the callback blocks on nothing: adopted

Revision (a)'s callback waited for the watcher thread to close the handle. That wait is
bounded only by the scheduling of a `Normal` thread, and the callback must return quickly
[S10]. The handle also closes today only in `DirWatch::drop`, after the join
(`lib.rs:16407-16430`), so the watcher thread has no road to close it itself. And an
ordinary `sync` could reopen a stopped watch before the removal completed, holding the
same eject up again.

**The handle moves into a slot that both sides can close.** Each watched directory, in a
`DirWatch` or a `DirWatchSet` member, owns a `HandleSlot`: a mutex around
`{ state, handle }`. The lock is held only across nonblocking calls:
- by the watcher thread, while it issues `ReadDirectoryChangesW` (overlapped) and reads
  `GetOverlappedResult` without waiting;
- by the callback, while it runs `CancelIoEx` and `CloseHandle`.

No wait, I/O completion or join happens under it. The overlapped buffer stays owned by
the watcher thread, which stays alive, so a cancelled read completes into valid memory.

**The states, per watched directory:**

```text
Active     — handle open, read outstanding, registration on that handle
  QUERYREMOVE (callback) ── lock; CancelIoEx; CloseHandle; state = Suspended; unlock;
                            set the thread's "suspended" event; return ERROR_SUCCESS
Suspended  — no handle; the registration is KEPT, so QUERYREMOVEFAILED can arrive
  QUERYREMOVEFAILED (callback) ── state = Rearm; set the event; return ERROR_SUCCESS
      → the watcher thread reopens by path, registers the new handle, and asks the
        steward ((b)4) to unregister the old registration; state = Active
  REMOVEPENDING / REMOVECOMPLETE (callback) ── state = Absent; set the event; return
      → the steward retires the registration (never from inside the callback)
  ordinary sync ── no effect: a Suspended member is not reopened by a set comparison
Absent     — no handle, no registration
  Volumes arrival whose volume holds the path, or the member leaving and re-entering
  the wanted set ── eligible to arm again
REMOVEPENDING / REMOVECOMPLETE while Active (no query first) ── as from Suspended
```

The callback's work is a short critical section plus an event signal. It never waits for
a thread, never joins, and never unregisters [S10]. It always answers `ERROR_SUCCESS`:
Folio never refuses an eject (owner question Q3). Each handle has its own registration,
so several watches on one volume each close their own handle. Whether any of them may
reopen is decided per member, so that one member's `QUERYREMOVEFAILED` cannot rearm
another member that is Absent.

**No premature rearm.** `FilesWatch::sync`, `ProgramsEar` and `IndexEar` treat a
Suspended or Absent member as *present in the set but not armed*. A set comparison never
reopens it. Only `QUERYREMOVEFAILED` (back to Active) or a `Volumes` arrival covering the
path (Absent becomes eligible) opens a handle again. While Suspended or Absent, the folder
or root is **unheard**, with (b)5's consequences.

**The notification itself** is the `CM_NOTIFY_ACTION` set of [S29]
(`DEVICEQUERYREMOVE`, `DEVICEQUERYREMOVEFAILED`, `DEVICEREMOVEPENDING`,
`DEVICEREMOVECOMPLETE`), and the ordering is the one in [S9].

### (b)4 · Watch start, reconfiguration and retirement leave the window thread: adopted

Revision (a) argued from an unchanged `window_waits.tsv`, and the review is right that
this proves nothing about new *executions* of an existing row. Row 8
(`window_waits.tsv:62`) already records that `DirWatch::start_scoped` waits on
`listening.recv()` with no deadline, and that each `Drop` runs `SetEvent` followed by an
unbounded `join()`. Every `sync` that arms or drops a watch on the window thread runs that
wait. The PATH ear and the index ear would add more such executions, through new callers
of a row that is already open.

**The steward.** A new `bt-app` lane, `watch_steward`, takes this over. It is one worker
through `spawn_at_priority` at `BelowNormal`, started by its first request, and it owns
every `DirWatch` and `DirWatchSet` value of the **two new ears**:
- The window thread sends it the wanted set (an owned `BTreeSet<PathBuf>` per ear, newest
  set per ear kept) with a nonblocking channel send.
- The steward starts, reconfigures and drops watches on its own thread, where the start
  handshake and the join happen.
- It reports per member `Armed`, `Unheard(cause)` or `Ended(cause)` into a mailbox, then
  wakes the loop (the lane contract: an answer is published before its wake).
- It also retires the handle registrations of (b)3.

The window thread never holds a `DirWatch` of these ears and never drops one.

**The existing users of row 8** (the files watch, the git watch, the scheme, storage and
preview watches) are **not** moved by these tickets. They stay row 8's debt, to be repaid
by row 8's own ticket. This note neither adds to that debt nor depends on its repayment.

**The evidence is a test, not the census:** `watch_steward::tests::a_sync_returns_while_a_watch_start_and_a_watch_drop_are_held`.
It uses a fake watch door whose `start` and `drop` block on a gate, and a `sync` from the
test thread that stands for the window thread. With the gate shut, the sync must return
and the loop turn must complete. MUTATION: arm on the calling thread. The test fails by
not returning within the suite's patience. The census
(`hang_watch::window_waits_tests`) still runs, and both ears' sync functions are named in
the hang watch's station list, so a watch call that crept back onto the window thread
would show as a station.

### (b)5 · `DirWatch` reports initial and terminal failure, and either makes the member unheard: adopted

**Initial failure** is already an error from `start_*` (`files_watch.rs:255-268`). Each ear
records it as `Unheard(cause)`.

**Terminal failure is made real**, on both platforms:
- `watch_loop` on Windows (`lib.rs:16489`) and the FSEvents callback path on macOS store
  the error that ended them in a shared `Option<io::Error>`, set `armed = false`, and call
  the wake once with `DirChange::Ended`.
- `is_armed` answers that flag, and `take_failure` takes the stored error. Today they
  answer `true` and `None` (`lib.rs:16274-16281`, `macos_watch.rs:365-372`).
- On macOS, an FSEvents `kFSEventStreamEventFlagRootChanged` or `…Unmount` on the
  watched root counts as terminal [S21].
- A stop asked for by `Drop` is not a failure.

**The transition to unheard:**
- A **PATH directory** that is unheard is simply not heard. The walk's other triggers
  (B2) still cover it, and §5's one-line `diagnostics.log` names it once per transition.
- A **palette root** that is unheard (`IndexEar`) makes `reopened` walk it at every open,
  which is B3's behaviour. A root that goes from heard to unheard is also marked dirty,
  because news may have been lost between the last notification and the failure.
- A **files-column folder** is outside this note's tickets, but it benefits as a matter of
  course: `FilesWatch::due`'s existing failure arm starts firing, and the folder is
  attempted again on its next arrival on screen.

Tests: an unavailable root (a path that does not exist, and a UNC path to a host that
does not exist), and a watch that fails **after** arming (the watched directory removed,
or on macOS the root renamed). Each must end unheard, and the next open must walk.
MUTATIONS: `is_armed` answers `true` again; the ear ignores `Ended`.

### (b)6 · One active walk and one owed rerun per root, nothing else queued: adopted

The register's entry becomes `{ epoch, running: bool, owed: bool, index, unheard }`,
replacing `building` and `dirty`:
- `claim(root)`: when `running` is true, set `owed = true` and answer `None`. Otherwise
  answer a request with a new epoch, set `running = true` and `owed = false`.
- `mark_dirty(root)`: when running, set `owed = true`; when idle, the next claim walks.
  A separate `stale` flag records "the index is not known to be current".
- `accept(response)`: drop a superseded epoch, as today. For the current epoch, file the
  index and set `running = false`. If `owed`, the caller claims again at once, with no
  clock needed: the debounce has already run for the news that set it.
- `reopened`: a running root answers `Merged`, as today. A heard, clean, ready root is
  not walked ((a) §4.3). Otherwise it claims.

So per root **at most one walk is in the worker's queue or running, and at most one more
is owed as a Boolean**. No epoch is issued while a walk is out, so the FIFO queue can
never hold a superseded walk. This changes B3's "a dirty root supersedes its own walk"
(`palette_index.rs:557-559`). The rerun now waits for the running walk instead of
overtaking it: the running walk's answer is filed (still useful), and one more walk
follows. Across roots, the worker serves one root at a time, and the queue's length is
bounded by the number of retained roots.

Tests: `palette_index::tests::claims_while_a_walk_is_out_are_one_owed_rerun_and_no_queued_walk`
(ten `mark_dirty` + due claims during one walk give one request out, `owed`, and exactly
one more request after `accept`). MUTATION: `claim` issues an epoch while running, as
today. Also `…::an_owed_rerun_is_claimed_when_the_running_walk_lands`. MUTATION:
`accept` clears `owed`.

### (b)7 · The event filter follows the walk's directory-only skip: adopted

`walk_bounded` refuses a `SKIPPED_DIRECTORIES` name only for a **directory**
(`palette_index.rs:290-306`: a file named `build` at the root is indexed). A filter that
dropped any component named `build` would leave that file stale. **`bears_on_index(relative)`
is now:**
- answer `true` for `Unknown`;
- answer `false` when any **non-final** component is in `SKIPPED_DIRECTORIES`, since a
  non-final component is a directory by construction;
- answer `false` when the name's depth (components) is greater than `INDEX_MAX_DEPTH`;
- otherwise answer `true`, **including a final component named `build`, `target`,
  `node_modules` and so on.** A delete or rename event no longer carries the entry's type,
  and the walk indexes such a name when it is a file, so passing it is the conservative
  choice. The cost is one walk when a whole `target` directory is created or deleted at an
  indexable depth: the event names the directory itself as a final component. Its contents
  are non-final and stay silent.

Tests (table): `build` (a final file at the root) passes; `target` (final) passes;
`target/debug/x.o` is refused; `a/node_modules/b/c.js` is refused;
`src/build/notes.md` is refused, because `build` is a non-final directory there, which
matches the walk's own skip of a `build` directory at any level; a name at depth 7 is
refused; `Unknown` passes. MUTATIONS: check every component (the root-level `build` file
fails); check only the first component (`a/node_modules/b/c.js` fails).

### (b)8 · Memory: an enforceable traversal bound, and a qualified retained-index estimate: adopted

**The enforceable part.** `INDEX_MAX_ENTRIES` bounds the files retained, and it bounds
the entries read in one directory (`palette_index.rs:256-266`). It does **not** bound the
number of directories queued: a tree of many directories holding few files is traversed
in full, to depth 6, with each queued `Pending` carrying its ancestor paths
(`palette_index.rs:209-221`). T-PALETTE-DEEP adds `INDEX_MAX_DIRECTORIES`. Once that many
directories have been enqueued, nothing more is enqueued and the index records
`truncated`, said as today's cap is said (`PaletteFilesTruncated`). The value is chosen
by E-5's 200 000-entry tree. **Estimate** for E-5 to confirm or replace: 30 000, matching
the entry cap, so that both bounds describe "a large repository read whole".

**The estimate, now labelled as one.** §4.3's "≈ 6 MB per root at the cap" is an
**expected retained-index size**, not a cap. It assumes:
- three owned buffers per `IndexedFile`;
- allocator overhead and string capacity slack, which are not counted precisely;
- typical path lengths.

Long paths raise it in proportion. A walk's transient memory (the queue, the ancestors,
the per-directory `Vec`, and the replacement index while the old one is held) comes on
top of it. **No byte cap is introduced.** The two count bounds are enforceable and
testable, and a byte cap would need an allocator measurement Folio does not have. E-5
measures peak and retained bytes on its trees, and the ticket's DESIGN entry states the
measured figures.

Tests: `palette_index::tests::a_tree_of_many_folders_and_few_files_stops_at_the_folder_bound`
(a scratch tree with more directories than the bound, under a bounded test parameter).
MUTATION: the bound is never checked.

### (b)9 · Watcher costs per platform, and one removable-PATH policy: adopted

**Costs, corrected** (estimates, E-4 measures them):
- **Windows `DirWatchSet`:** one thread per 63 members, and per member a directory
  handle, an event, an `OVERLAPPED` and the 64 KiB read buffer (`lib.rs:16052`). About
  23 × 64 KiB ≈ 1.5 MiB committed for B2's 23-entry `PATH`. Each `IndexEar` root on
  Windows: one handle and one 64 KiB buffer, and one thread unless roots share a set (they
  can: a root is a member of a tree-contract set, one set per contract).
- **macOS:** no application read buffer. FSEvents delivers paths through the stream's
  callback. The cost is one stream and its run-loop thread per set, and the stream's
  kernel-side queue, which the application does not size. Revision (a)'s "64 KiB per
  root" applied only to Windows.

**One removable-PATH policy.** Revision (a) excluded removable `PATH` entries "until
T-WATCH-LETS-GO has landed" in §3.2, but called the exclusion permanent in §8 Q1. **The
policy is conditional, everywhere.** Removable entries (`DRIVE_REMOVABLE`) and entries on
a volume whose watches E-2 shows can block an eject are excluded **until T-WATCH-LETS-GO
has landed**. From then on they are watched like any local entry, because (b)3's
handshake lets them go. If E-2 shows that no watch blocks an eject, the exclusion is
dropped at once and T-WATCH-LETS-GO is closed with the evidence. Remote entries stay
excluded for (a) §3.2's own reason (dead shares, the 64 KiB network limit), not for the
eject. The same rule applies to `IndexEar` roots on removable volumes.

### (b)10 · PATH and PATHEXT are the walk's exact snapshot; E-1 to E-5 expanded: adopted

**The snapshot.** The walk publishes, with its generation, the exact `PATH` and `PATHEXT`
values it walked: the `Environment` it read. On Windows that is the logon block
(`fresh_logon_environment`). **On macOS and every other target it is the process's
inherited environment**, because `fresh_logon_environment` answers `None` off Windows
(`bt-platform/src/environment.rs:20-24`). Revision (a) wrongly called the macOS source a
"logon environment". `ProgramsEar`'s wanted set is derived from that published snapshot
only, and changes only when a newer walk's snapshot lands, so the ear and the walk always
agree on which directories are "the PATH". An older snapshot never replaces a newer one
(the walk's own generation rule).

**The name filter is a pure decision on the event's name.** Revision (a)'s "or which are
reparse points" needed I/O at event time, which fails after a delete or rename, and is
removed. The Windows filter is: the name's extension, case-insensitive, is in the
snapshot's `PATHEXT` list, or the event is `Unknown`. An execution alias is an `.exe`
name, so it passes as a name. On macOS every name passes. The npm storm behaviour of §5 is
**an expectation for E-4 to test, not a design fact.**

**E-1 to E-5, expanded** (each runs before the ticket step that rests on it, in the clean
VM or on the Mac, never on a real profile):
- **E-1 (PATH ear, Windows).** For a Store app with an alias, a winget portable package,
  `npm i -g`, and `cargo install`, run install, update, uninstall, and an atomic replace
  (write to a temporary name, then rename over the target). Record each delivered
  name and action. Check PATHEXT-positive names (`.exe`, `.cmd`) and negative ones (a
  `.log`, a `.md`), and force an overflow (more than 64 KiB of names in one burst into a
  watched directory). Check the alias and link behaviour: an `IO_REPARSE_TAG_APPEXECLINK`
  entry and a symlink. Mutations to observe with E-1's recorded events replayed through
  the pure filter: drop the `Unknown` pass; filter by extension case-sensitively.
- **E-2 (removal handshake).** Run query-remove followed by a successful eject;
  query-remove followed by query-remove-failed (another process holds a file open);
  remove-pending; remove-complete with no query (pull the device, or detach the VHD by
  force); several watched handles on one volume (a shallow files-column folder, a tree
  root, a PATH entry). Measure the callback latency, which must stay within microseconds
  for the critical section. Prove there is **no rearm while Suspended**: a `sync` during
  the suspension must open nothing. Do the Mac half too: an FSEvents stream on a mounted
  DMG and `diskutil eject`.
- **E-3 (volumes and locale).** The (a) volume cases (USB stick, VHD attach, `subst`, a
  mapped network drive, a volume mounted on a folder). Add for the locale: a cold first
  pane (Folio started with no answer held); a launch probe that fails (`defaults` made
  unavailable) followed by a retry at the next birth; stale completion ordering (two
  probes, the older one answering last, must be refused); and on `Drop`, removing the
  observer from each **exact** centre it registered with (workspace centre, default
  centre).
- **E-4 (PATH ear cost).** Idle cost (threads, committed bytes, wakes per hour); a short
  burst (one `npm i -g`); and **long mutation storms**: a Windows Update that writes
  `System32` for minutes, and an `npm i -g` of a large package. Count walks per minute
  against the 2 s floor's ceiling of 30 per minute per directory.
- **E-5 (palette ear).** Roots with 200 000 mixed entries and with long paths (near
  `MAX_PATH` and beyond, with long-path support on); a network root and an unwatchable
  root (`\\wsl$`); a terminal failure after arming; continuous changes during a slow walk
  (the owed rerun of (b)6); several roots at once; final files named `build` and `target`
  ((b)7); watcher start and drop measured on the steward, with the window thread's turn
  time recorded alongside; a cold-disk walk after boot.

### (b)11 · The cold first pane's locale, retries without an OS event, and row 7: adopted, with an owner question

**Today**, the first pane's birth calls `system_locale_declaration()` on the window thread
(product call sites: `shell_integration.rs:636` in `shell_command`, and `:709` in
`derive_environment_for_birth`; `main.rs:39240` is a test). It blocks until the two
children answer, so the first pane always has the probe's result, success or `None`.
Revision (a)'s "a birth reads whatever answer is held and does not wait" would give a cold
first pane no declaration, which is a behaviour change nobody ruled on.

**The first pane's semantics are preserved without a window-thread wait.**
- The launch requests the probe at the top of `Runtime::create`, as B2's launch walk is
  requested.
- A pane whose birth needs the declaration and finds **no outcome yet** (neither an answer
  nor a failure) is born **in birth**, waiting with a new `PaneBirth::waiting` reason,
  `Locale`. This is B2's `Programs` and T-BIRTH-OFF-WINDOW's `Shell` road: what is typed
  into it is held, and its birth completes when the probe's outcome lands.
- The probe gets B3's 10 s deadline (`probe_output_with_raw_tail`). An overrun is a
  failure. So the wait is bounded, and a failure gives exactly today's failure outcome:
  no declaration.
- Only a pane on a platform with a declaration to make (macOS) and with no locale variable
  of its own waits. On Windows `system_locale_declaration` answers `None` without a probe
  (`lib.rs:7836-7839`).

**A failure is retried without waiting for an OS locale event.** The cell keeps "last
success" and "last outcome" separately. A birth that finds no success held and no probe
out **requests** a probe and does not wait for it, unless no outcome has ever existed (the
cold case above). Merged requests and one probe out follow B4's shape, with one
`diagnostics.log` line per merge and per failure.

**Row 7 is retired only when every product call site is nonblocking.** Both call sites
above read the cell, never the probe. The ticket's diff must show `system_locale_declaration`
no longer reachable from any `OnceLock` initialiser on the window thread
(a source guard: `get_or_init(read_system_locale_declaration` absent), and then row 7
leaves the TSV. If L3 changes the call sites first, this ticket rebases onto L3's shape.

**Owner question Q5**, added: may a cold first pane be born without the locale
declaration? The recommendation, which is also the review's: **no; preserve it through
the in-birth wait above.**

### (b)12 · `WatchClock` is an event-armed debounce, and long mutations are qualified: adopted

Revision (a)'s "no timer" (§0, §4.3, §10) is replaced throughout by **"no periodic timer
and no polling loop"**. `WatchClock` keeps a deadline (`due_at`, `watch_clock.rs:89`), and
the loop wakes for it. That deadline exists only after a notification has armed it, and
it disarms when its news is taken (`watch_clock.rs:69-110`). It is an event-armed
debounce, which is what R31 allows. It is not a schedule.

§4.3's "a `git checkout` of a large branch: one walk after 300 ms of quiet" holds only for
a checkout that **finishes within the 2 s floor**. A mutation that lasts longer (a large
checkout, a `git clean`, an unpacked archive, a sync client catching up) gives one walk
per 2 s floor for as long as it lasts, plus one owed rerun after it ends ((b)6). That is
at most 30 walks a minute per root while it lasts, each at below-normal priority. E-4
and E-5 measure the real counts.

### (b)13 · The tickets, revised

The order is now five tickets. The new first ticket carries (b)4 and (b)5, which every ear
needs:

1. **T-DIRWATCH-LIFECYCLE (M; first).** Terminal failure reporting on both platforms
   ((b)5); the `watch_steward` lane ((b)4); the `HandleSlot` refactor of `DirWatch` that
   (b)3 builds on (no notification yet). Tests: (b)4's held-gate test; (b)5's
   unavailable-root and failure-after-arming tests on Windows and the Mac; a source guard
   that neither ear's module names `DirWatch::start` or drops a watch (only the steward
   does). Mutations as named in (b)4 and (b)5. Mac gate.
2. **T-WATCH-LETS-GO (M, Windows; was S).** E-2 first. (b)3's state machine with its
   notifications. Tests:
   - `dir_watch_removal_tests::a_query_remove_closes_the_handle_inside_the_callback_and_returns`
     (the callback driven directly). MUTATION: the callback only signals the thread.
   - `…::a_suspended_member_is_not_reopened_by_a_sync`. MUTATION: `sync` treats Suspended
     as absent from the set.
   - `…::query_remove_failed_rearms_and_registers_the_new_handle`. MUTATION: rearm on the
     old registration.
   - `…::remove_complete_without_a_query_ends_the_member_absent`. MUTATION: that action
     ignored when Active.
   - `…::each_handle_on_one_volume_lets_go_of_itself`. MUTATION: one registration per
     volume.
   - `…::a_removal_is_never_refused`. MUTATION: answer `ERROR_CANCELLED`.
   - Source guard: `CM_Unregister_Notification(` only in the steward and `Drop`. MUTATION:
     call it from the callback.
3. **T-OS-SIGNALS-WIN (M).** As (a) §7.2, with these changes:
   - `device_header` and its tests ((b)2: null `lParam`, short `dbch_size`, a header at
     an odd offset in a byte buffer, a query-remove, a non-volume type). MUTATIONS:
     remove the size check (the short-size case fails); answer `Volumes` for
     `DBT_DEVICEQUERYREMOVE` (the query-remove case fails). The unaligned read is pinned
     by a source guard: `read_unaligned` present, and no `&*` on the header pointer.
   - The ear's set comes from the walk's snapshot ((b)10).
   - The pure `PATHEXT` filter ((b)10). MUTATION: case-sensitive comparison.
   - Removable entries conditional on ticket 2 ((b)9).
   - Arming through the steward ((b)4).
4. **T-OS-SIGNALS-MAC (M).** As (a) §7.3, with these changes:
   - The locale cell with "last success" and "last outcome"; the `PaneBirth::waiting`
     `Locale` reason; the deadline; the retry on a birth ((b)11).
   - Tests: `…::a_cold_first_pane_waits_in_birth_and_gets_the_declaration` (MUTATION: the
     birth proceeds with nothing held); `…::a_failed_launch_probe_is_retried_by_the_next_birth_without_an_os_event`
     (MUTATION: retry only on `Locale`); `…::an_older_probe_answer_is_refused`
     (MUTATION: `publish` files every answer); the source guard of (b)11 (MUTATION:
     restore the `get_or_init`).
   - Exact-centre removal pinned ((b)10 E-3).
   - Row 7 retired only with the guard green.
5. **T-PALETTE-DEEP (M).** As (a) §7.4, with these changes:
   - The register's `running`/`owed` ((b)6), with its two tests.
   - The filter's directory-only semantics and table ((b)7).
   - `INDEX_MAX_DIRECTORIES` ((b)8), with its test.
   - Unheard on initial or terminal failure ((b)5), with the heard-to-unheard dirty mark.
     MUTATION: no dirty mark on that transition.
   - Arming through the steward ((b)4).
   - Removable roots conditional on ticket 2 ((b)9).
   - The DESIGN entry states E-5's measured retained and peak bytes ((b)8).

The dependencies are: 1 → 2 → 3 → 4, and 1 + 2 + 3 → 5. Ticket 4's locale half depends
only on ticket 1 and may run in parallel with ticket 3, if the coordinator splits it out.

### (b)14 · The owner questions, revised (five)

- **Q1 — watch the `PATH` directories?** **Yes, subject to E-4's measured cost and the
  lifecycle work of tickets 1–2.** Removable entries are watched only after ticket 2
  ((b)9); remote entries are never watched.
- **Q2 — the palette's background walk?** **Yes, with (b)6's queue (one walk running and
  one owed per root) and (b)8's bounds.**
- **Q3 — let an ejected drive go silently, never refuse?** **Yes, through (b)3's
  suspension state machine.** No toast. The member stays unheard until the drive
  returns, or until the eject is cancelled.
- **Q4 — the macOS locale re-read applies to new panes only?** **Yes.** Running shells are
  unchanged.
- **Q5 — may a cold first pane be born without the locale declaration?** **No.** Preserve
  today's outcome with the in-birth wait of (b)11, bounded by the probe's 10 s deadline.

### Departures

None. Each of the twelve required changes is applied as the review asks, and the
review's four agreements with the brief's corrections stand. The one choice the review
left open is (b)8 ("enforceable bounds *or* a qualified estimate"): this revision does
both for traversal (a directory bound plus the existing entry bound) and declines a byte
cap, for the reason given in (b)8.

### Sources added

- [S29] `CM_NOTIFY_ACTION`: https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/ne-cfgmgr32-cm_notify_action
- The Codex review: `B5-os-signals-design.review-codex.md` (0.4.8 trace, 2026-10-11).
- Folio at `ae933eb7`: `lib.rs:16274-16281` and `macos_watch.rs:365-372` (`is_armed`,
  `take_failure`); `lib.rs:16407-16430` (start and the join on refusal); `lib.rs:16489`
  (`watch_loop`); `window_waits.tsv:62` (row 8); `palette_index.rs:209-221`, `:256-266`,
  `:290-306` (the queue's `Pending`, the per-directory read cap, the directory-only skip);
  `environment.rs:20-24` (no logon block off Windows); `shell_integration.rs:636`, `:709`
  (the locale's product call sites).

---

## Revision (c), 2026-10-11, after the Codex confirmation round

Codex checked revision (b) (`a3f3876a`; review `B5-os-signals-design.review-codex-b.md`).
Its verdict was again **adopt with changes**. Eight of the twelve changes were applied,
and four were partial: (b)3, (b)4, (b)11 and (b)12. Those four left seven required
changes at implementation level. **All seven are applied below**, each against the code
at `ae933eb7`, and the review's "Directory bounds" paragraph is adopted as (c)8. **Where
(c) and (b) differ, (c) stands.** The Departures heading at the end records none.

### (c)1 · The removal callback cannot wait; the registration is retained; the cancelled read is drained: adopted

(b)3's callback took `HandleSlot`'s mutex. That is a wait: a watcher thread pre-empted
while holding the lock would hold the callback for as long as it is not scheduled. (b)3
also reopened the handle with a **new** registration after `QUERYREMOVEFAILED`. Microsoft's
procedure keeps the original registration: after a failed query-remove it stays valid on
the now-closed handle and is not replaced [S30]. And (b)3 did not say that the cancelled
read must complete before its `OVERLAPPED` and buffer are used again. `CancelIoEx` asks for
cancellation without waiting for it, and its contract forbids reusing the operation's
state until the cancellation has completed [S31].

**The slot becomes one atomic word**, `HandleSlot(AtomicU32)`, holding a small state and
no lock. The directory `HANDLE` itself sits beside it and is written only by whichever side
wins a compare-and-swap:

| state | meaning | who moves it out |
|---|---|---|
| `Idle` | Handle open, read outstanding, the watcher **not** touching the handle (it is parked in `WaitForMultipleObjects`) | the callback (`Idle → Closing`), or the watcher on a completion (`Idle → Issuing`) |
| `Issuing` | The watcher is inside its few-microsecond window: `GetOverlappedResult(bWait = FALSE)` then `ReadDirectoryChangesW` to re-issue | the watcher, back to `Idle`, or to `Closing` if a close was asked for meanwhile |
| `CloseAsked` | The callback found `Issuing`, recorded the request and returned | the watcher, right after its re-issue: `CloseAsked → Closing` |
| `Closing` | `CancelIoEx` + `CloseHandle` are being done by whichever side won | the same side, to `Draining` |
| `Draining` | The handle is closed; the cancelled read has not yet completed into the watcher's `OVERLAPPED` | the watcher only, after observing the cancelled read's completion (`ERROR_OPERATION_ABORTED` on its event) |
| `Suspended` | Drained, no handle; the **original registration retained** | callbacks: `QUERYREMOVEFAILED → Reopening`; `REMOVEPENDING`/`REMOVECOMPLETE → Absent` |
| `Reopening` | The watcher is reopening by path, reusing the drained buffer | the watcher: `Reopening → Idle` on success; `→ Failed` ((c)2) on an error; a callback's `REMOVEPENDING`/`COMPLETE` here moves it to `Absent`, and the watcher closes the handle it just opened as soon as its own `Reopening → Idle` swap fails |
| `Absent` | Removed. The registration is retired by the steward ((c)3), never by the callback | only a `Volumes` arrival covering the path ((b)3) |

**The callback, entire:** read the state, then one compare-and-swap.
- `Idle → Closing` wins: the callback runs `CancelIoEx` and `CloseHandle`, stores
  `Draining`, signals the watcher's event, and returns `ERROR_SUCCESS`.
- The state is `Issuing`: the callback swaps it to `CloseAsked` and returns
  `ERROR_SUCCESS` without waiting.
- Every other state: the callback records the action (`QUERYREMOVEFAILED`, `PENDING`,
  `COMPLETE`) by a swap and returns.

No lock, no join, no wait, no unregister [S10]. The handle is never closed while the
watcher may be using its value, because only the side that won `Idle → Closing` or
`CloseAsked → Closing` closes it. So a recycled handle value can never be read by the
other side.

**The one residual, said plainly.** If the query-remove lands inside the watcher's
`Issuing` window, the handle closes a few microseconds **after** the callback returns
(when the watcher sees `CloseAsked`). In that window the system's removal may still find
the handle open and fail once, as "in use". The person's next eject attempt then succeeds.
Folio never *answers* a refusal: the callback always returns `ERROR_SUCCESS`. E-2 measures
how often this window is hit (expected: never in practice, given its microsecond length).
The alternative is to have the callback wait for the watcher, which is what the review
forbids. The handle cannot be closed from both sides without the swap.

**Drain before reuse.** The watcher never reissues a read into, frees, or reuses its
`OVERLAPPED` and buffer for a reopened handle until it has seen the cancelled read
complete. That wait is the watcher's own `WaitForMultipleObjects` on its own event, on its
own thread, which is legal there. Only then does it leave `Draining`.

**Retained registration.** On `QUERYREMOVEFAILED` the watcher reopens by path into the
same buffer, under the **original** registration, with no unregister/register cycle
[S30]. The steward retires the registration only from `Absent`, or when the member leaves
the wanted set.

**Tests** (T-WATCH-LETS-GO), replacing (b)13's list for this ticket:
- `…::a_query_remove_while_idle_closes_inside_the_callback` (the callback driven directly
  on a test thread with the watcher parked). MUTATION: the callback only signals the
  watcher (the handle is still open on return).
- `…::a_query_remove_while_issuing_is_closed_by_the_watcher_and_the_callback_does_not_wait`
  (the watcher held inside `Issuing` by a gate; the callback must return with the gate
  shut). MUTATION: the callback spins until `Idle`.
- `…::the_buffer_is_not_reused_before_the_cancelled_read_completes` (a fake read whose
  cancellation completes only when a gate opens). MUTATION: the reopen proceeds from
  `Closing`.
- `…::query_remove_failed_reopens_under_the_original_registration` (a counting fake
  registration door). MUTATION: unregister and register again.
- `…::a_removal_during_reopening_ends_absent_and_closes_the_new_handle`. MUTATION: the
  watcher's `Reopening → Idle` is an unconditional store.
- `…::a_suspended_member_is_not_reopened_by_a_sync` and
  `…::each_handle_on_one_volume_lets_go_of_itself`, with their (b)13 mutations.
- `…::a_removal_is_never_refused` (MUTATION: `ERROR_CANCELLED`).
- The source guard (MUTATION: `CM_Unregister_Notification` in the callback).
- A further source guard: no `Mutex`, `lock(`, `join(` or `wait` call in the callback's
  body. MUTATION: put back (b)3's lock.

### (c)2 · A distinct terminal `Failed` (unheard) state with its own rearm rule: adopted

A watch that fails without a removal message (an initial start failure; a terminal read
error; the watched directory deleted and later recreated on the same mounted volume; an
FSEvents root change) is neither `Suspended` nor removal-caused `Absent`. Its member goes
to **`Failed`**, which is unheard, with (b)5's consequences. The three states now carry
three rearm rules:

| state | how it is entered | what may rearm it |
|---|---|---|
| `Suspended` | a query-remove | only `QUERYREMOVEFAILED` ((c)1) |
| `Absent` | `REMOVEPENDING` or `REMOVECOMPLETE` | only a `Volumes` arrival covering the path |
| **`Failed`** | an initial start error, a terminal read error, or an `Ended` wake not caused by removal | **one attempt on the next reader-triggered event**: for a palette root, the palette opening over it, or the walk that open asks for landing; for a PATH directory, the next walk landing whose snapshot still lists it; also a `Volumes` arrival covering it. **Never** on an ordinary loop sync, which runs every turn and would poll a missing directory |

Each attempt is one start on the steward. A failed attempt stays `Failed` and is said
once in `diagnostics.log` per transition, not per attempt. A directory recreated on the
same volume is heard again from the next open or walk after it reappears.

Tests (T-DIRWATCH-LIFECYCLE):
- `…::a_failed_root_is_attempted_again_on_the_next_open_and_not_on_a_sync`. MUTATIONS:
  rearm `Failed` on every sync (the attempt count rises without an open); never rearm
  `Failed` (the recreated directory is never heard).
- `…::a_failed_path_directory_is_attempted_again_when_a_walk_lands`. MUTATION: rearm on
  the wanted-set comparison.

### (c)3 · The steward's exact door, its wake, and two held-door tests: adopted

**The door.**
- `bt_app::watch_steward::WatchSteward::serve(self, ctx: &bt_platform::admission::WorkerCtx)`
  is the worker's whole body. It is started by `watch_steward::request` through
  `bt_platform::spawn_at_priority("watch-steward", ThreadPriority::BelowNormal,
  move |ctx| steward.serve(ctx))`, the shape `programs_lane` uses (`programs_lane.rs`'s
  `WorkerCtx` import).
- Every `DirWatch` / `DirWatchSet` start, reconfigure and drop takes `&WorkerCtx`, so only
  a thread lent one by the door can call it. This is the A1 capability rule (TD§2), and
  the compiler enforces it.
- The window thread's side is
  `watch_steward::request(ear: Ear, wanted: BTreeSet<PathBuf>, reasons: Rearm)`, a
  nonblocking `Mutex<Requests>` store of the newest set per ear, plus a `Condvar` notify.
  It takes the same lock-free-for-the-reader shape as `programs_lane::request`; the
  window thread never waits on that `Condvar`.

**The wake.** The steward publishes per-member reports into
`watch_steward::REPORTS: Mutex<BTreeMap<(Ear, PathBuf), MemberReport>>`
(`MemberReport = Armed | Suspended | Absent | Failed(cause)`, with a report number). It
then sends **`AppEvent::WatchStewardAnswered`** through the `EventLoopProxy` it was handed
at the first request: the report first, the wake after (the lane contract). The loop's
arm drains `REPORTS` once per turn and hands each report to its ear (`ProgramsEar`,
`IndexEar`).

**Two tests, each with its own calling-thread mutation**, replacing (b)4's single one:
- `watch_steward::tests::a_sync_returns_while_a_watch_start_is_held`: a fake door whose
  `start` blocks on a gate; `request` from the test thread must return, and the loop arm
  must run with the gate shut. MUTATION: `request` starts the watch on the calling thread.
- `watch_steward::tests::a_retirement_returns_while_a_watch_drop_is_held`: a fake door
  whose `Drop` blocks on a gate; a `request` that removes the member must return with the
  gate shut, and the drop must happen on the steward's thread (asserted by thread name
  inside the fake `Drop`). MUTATION: the steward hands the retired `DirWatch` back in its
  report, so it is dropped by the loop arm on the calling thread.

### (c)4 · A macOS-capable contained probe with ONE ten-second budget; what the waiting pane shows: adopted

`probe_output_with_raw_tail` is `#[cfg(windows)]` (`lib.rs:5128-5134`), so (b)11 named a
door that cannot run the macOS locale probe. And with two children each held to ten
seconds, the worst case was about twenty.

**The door.** A new `bt_platform` function, built only on parts that already exist on
every platform:
`probe_outputs_within(worker: &WorkerCtx, commands: Vec<Command>, total: Duration) -> io::Result<ProbeRun>`.
- It runs the commands **in order** through `spawn_probe` (`lib.rs:4343`, all platforms)
  and `ProbeChild::wait_within` (`lib.rs:3499`; its macOS arm reads processor time through
  `proc_taskinfo`, `lib.rs:3588`).
- Each child gets `ProbePatience::fixed(remaining)` (`lib.rs:3387-3395`), where
  `remaining = total − elapsed since the first spawn`.
- A child still running when `remaining` reaches zero is ended with its contained tree, as
  `ProbeChild`'s `Drop` does today (`lib.rs:3364`). The run then answers
  `ProbeRun::Overran { finished: Vec<Output> }`, and no later command is started.
- `ProbeRun::Finished(Vec<Output>)` answers when every command ended within the budget.
- It takes `&WorkerCtx`, so it cannot run on the window thread.

The locale cell calls it with the two commands of `read_system_locale_declaration`
(`lib.rs:7821-7833`) and `total = 10 s`. **The worst case is one ten-second total,
for both children together.** `choose_locale_declaration` is unchanged: it gets
`defaults`' output, or empty on a failure or an overrun (as `unwrap_or_default` does today),
and `locale -a`'s output on the same terms.

Tests: bt-platform `probe_run_tests::two_children_share_one_budget`. The first child
sleeps for 70 % of the budget and the second for 70 %; the run must answer `Overran`
within the budget plus one 20 ms poll. MUTATION: each child gets the full `total`, so the
run lasts about 140 % of it. This runs on the Mac at the Mac gate and on Windows locally;
the children come from the test-shell door.

**What the waiting first pane shows** (stated for Q5). It takes B2's road for a pane in
birth (`PaneBirth`, `main.rs:11907`). The pane, its tab and its chrome draw normally, and
the terminal grid is **empty**: no prompt, no output. Everything typed is kept, in order,
in the birth's `typed` queue and written to the shell when it is born. That is pinned for
this road by `a_pane_draws_before_its_shell_exists_and_its_shell_gets_what_was_typed_in_order`
(`main.rs:39505`). The tab reads the profile's name, because the profile is known. The
longest a person can look at that empty grid for the locale's sake is **ten seconds in
total**, only on a cold first pane, and only when the probe hangs. On an ordinary Mac the
two children take tens of milliseconds (**estimate**: E-3 measures it), well inside window
and GPU creation, so the first pane does not wait at all.

### (c)5 · A failed refresh is retried even when an older success remains: adopted

(b)11's "a birth that finds no success held … requests a probe" leaves a trap. If the
sequence is success, then a locale change, then a failed refresh, the old success stays
held and no birth ever asks again: a cached failure in a new form. **The retry predicate
keys on the newest outcome.**

The cell holds `{ last_success: Option<(n, LocaleDeclaration)>, newest: Option<(n, Outcome)>,
out: Option<n> }`.
- A birth requests a probe, without waiting, whenever `newest` is a failure (or an
  overrun) and `out` is `None`, **whether or not `last_success` exists**.
- What the birth uses meanwhile is `last_success`, the fallback declaration (`None` if
  there never was one).
- The cold case of (c)4 waits only when `newest` is `None`, meaning nothing ever finished.
- Each retry says one `diagnostics.log` line ("locale probe N failed (…); asked again by a
  pane's birth").

Test: `locale_declaration::tests::a_failed_refresh_after_a_success_is_retried_by_the_next_birth_and_the_old_success_is_used_meanwhile`.
MUTATION: the predicate is (b)11's "no success held", so no second probe is requested.

### (c)6 · An owed palette rerun waits for its clock: adopted

(b)6 claimed an owed rerun immediately on `accept`. With walks shorter than the floor and
events arriving during each one, that chains reruns faster than one per 2 s, and the
"30 walks a minute" ceiling of (b)12 is false. **The rerun goes through the root's
`WatchClock`, and the floor carries forward.**
- A heard event while a walk runs does what any heard event does: `note_event` on the
  root's clock (`watch_clock.rs:69`). Nothing else is recorded; `owed` is no longer a
  stored Boolean, but *the clock owes a reading* (`due_at().is_some()`).
- The loop calls `take_due` for a root **only when no walk is running for it**. So a clock
  that becomes due during a walk stays owed, and a running walk is never overtaken.
- On `accept`, the loop asks the clock for the root. If `take_due(now)` is true, it claims
  now; otherwise the loop's deadline is `due_at`. The floor's anchor is `last_reread`,
  which `take_due` set at the **previous** claim (`watch_clock.rs:61-63`, `:104-110`). So
  two ear-driven claims of one root are always at least `WATCH_FLOOR` apart, however
  short the walks are.

**The ceiling, now true:** at most one ear-driven walk per root per 2 s, so at most 30 a
minute per root, plus one walk per palette **open** (person-driven, merged into a running
walk as today). Opens are not throttled, because they are the reader asking.

Test: `palette_index::tests::fast_walks_with_events_during_each_are_at_most_one_per_floor`.
A fake clock, walks that each land 50 ms after their claim, and an event during every walk,
for 60 simulated seconds: at most 30 claims. MUTATION: (b)6's immediate rerun on `accept`
(about 1 000 claims).

### (c)7 · Ticket sizes, and per-root `DirWatch` for the palette: adopted

- **T-DIRWATCH-LIFECYCLE is L**, not M. It carries a cross-platform terminal-failure
  contract ((b)5), the `Failed` state and its rearm rule ((c)2), a new application lane
  with its door and wake ((c)3), and the Windows `HandleSlot` refactor ((c)1's atomic slot
  without notifications yet).
- **T-PALETTE-DEEP uses one `DirWatch::start_named` per root**, not the shared
  `DirWatchSet`. (b)9's "roots can share a set" is withdrawn. The roots are few (the front
  tabs' files columns), each root's tree watch is its own contract, and keeping them
  per-root keeps T-PALETTE-DEEP **independent of T-OS-SIGNALS-MAC**. Its dependencies stay
  tickets 1, 2 and 3 (3 for `Volumes`). The cost per root stays (b)9's: on Windows one
  thread, one handle and one 64 KiB buffer; on macOS one stream and its run-loop thread.

### (c)8 · Directory bounds, stated as the review states them: adopted

The two bounds are **independent dimensions**, not one combined limit:
- **`INDEX_MAX_ENTRIES`** (30 000, `palette_index.rs:103`) bounds the **files retained**
  in the index, and also the **names collected from any one directory**
  (`palette_index.rs:256-266`).
- **`INDEX_MAX_DIRECTORIES`** ((b)8) bounds the **total number of directories enqueued**
  by one walk. The ticket fixes the root convention and states it in the constant's doc
  comment: **the root counts as the first enqueued directory**, so a value of `n` admits
  the root plus `n − 1` descendants.
- **One `truncated` truth.** Reaching either bound sets the same `FileIndex::truncated`,
  and the palette says the same `PaletteFilesTruncated` note. There is no second flag and
  no second sentence, because the person's question ("is this all of it?") is the same.
- **It is not a "30 000 entries in total" limit.** A walk may hold up to 30 000 files *and*
  have enqueued up to `INDEX_MAX_DIRECTORIES` directories. Thirty thousand queued `Pending`
  values are not small, because each carries its path, its relative path and its ancestors
  (`palette_index.rs:209-221`). E-5 measures peak memory on its 200 000-entry tree and
  **confirms or replaces** the estimated 30 000 before the constant is committed. The
  ticket's DESIGN entry records the chosen value with its measurement.

Test (T-PALETTE-DEEP): `…::either_bound_sets_the_one_truncated_flag`. MUTATIONS: the
directory bound never sets `truncated`; the root is not counted (one extra directory is
admitted at the boundary).

### (c)9 · The tickets, revised again

1. **T-DIRWATCH-LIFECYCLE (L; first).** (b)5, (c)2, (c)3, and (c)1's atomic slot (no
   notifications). Tests: (c)2's two, (c)3's two, (b)5's unavailable-root and
   failure-after-arming pair, and the source guard of (b)13. Mac gate.
2. **T-WATCH-LETS-GO (M, Windows).** E-2 first. (c)1's callback, drain and retained
   registration, with (c)1's test list.
3. **T-OS-SIGNALS-WIN (M).** As (b)13 item 3.
4. **T-OS-SIGNALS-MAC (M).** As (b)13 item 4, with (c)4's `probe_outputs_within` (its
   bt-platform half also runs on Windows), the one ten-second budget, and (c)5's predicate
   and test. Depends on ticket 1 only for the steward; its `DirWatchSet` half follows
   ticket 3's Windows arm.
5. **T-PALETTE-DEEP (M).** As (b)13 item 5, with per-root `DirWatch::start_named` ((c)7),
   (c)6's clock-gated rerun and its test, and (c)8's bound and test. Depends on tickets 1,
   2 and 3, and **not** on 4.

Order: 1 → 2 → 3 → 4, and 1 + 2 + 3 → 5.

### (c)10 · The owner questions, revised (five)

- **Q1 — watch the `PATH` directories?** **Yes, subject to E-4 and tickets 1–2**
  (unchanged from (b)14).
- **Q2 — the palette's background walk?** **Yes.** With (c)6, walks the ear causes are at
  most one per 2 s per root (at most 30 a minute), plus one per palette open. With (c)8,
  each walk is bounded in files and in directories.
- **Q3 — let an ejected drive go silently, never refuse?** **Yes, through (c)1's
  non-waiting callback.** One residual: an eject that lands inside a watcher's
  microsecond re-issue window may fail once as "in use" and succeed when retried. Folio
  itself never answers a refusal.
- **Q4 — the macOS locale re-read applies to new panes only?** **Yes** (unchanged).
- **Q5 — may a cold first pane be born without the locale declaration?** **No.** Keep
  today's outcome. The cost, stated: on a cold first pane, while the probe is still out,
  the pane shows its tab and chrome with an **empty terminal grid** and keeps everything
  typed. The worst case is **ten seconds in total** for both probe children together, and
  only when the probe hangs. On an ordinary Mac it is tens of milliseconds, inside window
  creation (estimate, E-3).

### Departures

None. All seven required changes are applied as worded, and the "Directory bounds"
paragraph is adopted as (c)8. One consequence is said rather than hidden: (c)1's
microsecond residual is the price of a callback that cannot wait, and E-2 measures it.

### Sources added

- [S30] Registering for notification of device interface arrival and device removal:
  https://learn.microsoft.com/en-us/windows-hardware/drivers/install/registering-for-notification-of-device-interface-arrival-and-device-removal
  (after a failed query-remove, the registration on the original handle remains valid).
- [S31] `CancelIoEx`: https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex
  (the `OVERLAPPED` must not be reused or freed until the cancelled operation completes).
- The confirmation review: `B5-os-signals-design.review-codex-b.md` (0.4.8 trace,
  2026-10-11).
- Folio at `ae933eb7`: `lib.rs:3210` (`ProbeChild`, legal on every platform), `:3364`
  (its `Drop`), `:3380-3395` (`ProbePatience`, `fixed`), `:3499` (`wait_within`), `:3588`
  (macOS processor time), `:4343` (`spawn_probe`), `:5104` (`probe_output`), `:5110`
  (`ProbeEnding`), `:5128-5134` (`probe_output_with_raw_tail`, `#[cfg(windows)]`);
  `watch_clock.rs:54-110` (`last_reread` as the floor's anchor); `main.rs:11907`
  (`PaneBirth`), `:39505` (the birth-road drawing test).

---

## Revision (d), 2026-10-11, after the scoped confirmation

Codex's scoped confirmation of revision (c) (`e0bf0107`; review
`B5-os-signals-design.review-codex-c.md`) answered **adopt with changes**, with one blocker
and everything else accepted. The blocker, verbatim:

> (c)1 must replace its stipulated one-shot CAS with retry/re-evaluation and boundary-race
> coverage: if the callback reads `Idle` as the watcher wins `Idle → Issuing`, or reads
> `Issuing` as the watcher restores `Idle`, its sole CAS fails and neither side closes the
> handle.

The blocker is correct. In (c)1 the callback read the state and then made one
compare-and-swap whose expected value came from that read. If the watcher moved in between,
the swap failed and the callback returned, and no transition recorded that a close had been
asked. **This revision replaces (c)1's slot protocol entirely.** Where (d) and (c) differ,
(d) stands. Every other part of (c) is unchanged.

### (d)1 · The protocol: a close-request bit that every handle-holding exit must observe

**The word.** `HandleSlot` stays one `AtomicU32`. It now holds a **base state** in its low
bits and one flag, **`CLOSE`**. The handle is open exactly while the base is `Idle`,
`Issuing` or `Reopening` (in `Reopening`, once the new handle has been opened). The other
bases (`Closing`, `Draining`, `Suspended`, `Absent`, `Failed`) hold no usable handle.

**The callback's intent** (what should follow a close) is kept beside the word, in a second
atomic, `intent: AtomicU8`. Its values are `None`, `Rearm` and `Removed`. `Removed` is
sticky: once written, nothing replaces it.

The CM callbacks are written as follows. Each is **wait-free**: a bounded number of atomic
operations, with no wait and no unbounded retry.

```text
QUERYREMOVE:
  intent: Rearm → None (one compare-and-swap; Removed stays)
  prev = word.fetch_or(CLOSE)                     // the request is recorded first, always
  if base(prev) == Idle and !prev.CLOSE:
      if word.compare_exchange(Idle|CLOSE, Closing) succeeds:
          CancelIoEx; CloseHandle; word.store(Draining); signal the watcher
  return ERROR_SUCCESS

REMOVEPENDING / REMOVECOMPLETE:
  intent.store(Removed); then exactly QUERYREMOVE's last four lines   // a removal while Active closes too
QUERYREMOVEFAILED:
  intent: None → Rearm (one compare-and-swap; Removed stays); signal the watcher; return
```

**The watcher's transitions** follow one rule: **every transition the watcher makes out of a
handle-holding base is a compare-and-swap whose expected value has `CLOSE` clear.** If that
swap fails, the watcher reads the word again. The only thing that can have changed while the
watcher owned the base is the `CLOSE` bit. The watcher then takes the close path itself:

```text
completion while parked (base Idle):
  CAS(Idle → Issuing)
    fails ⇒ CLOSE is set (base Idle|CLOSE):
            CAS(Idle|CLOSE → Closing): won ⇒ watcher closes; lost ⇒ the callback won and closes
                                        (base Closing/Draining); the watcher goes to drain
end of Issuing (read re-issued, nonblocking):
  CAS(Issuing → Idle)
    fails ⇒ the word is Issuing|CLOSE (only the bit can change while the watcher owns Issuing;
            the callback never swaps out of Issuing) ⇒ watcher stores Closing and closes
end of Reopening (new handle opened, first read issued):
  CAS(Reopening → Idle)
    fails ⇒ Reopening|CLOSE ⇒ watcher stores Closing and closes the new handle
open failed in Reopening ⇒ store Failed ((c)2); there is no handle to close
drain done (the cancelled read's completion observed, (c)1):
  store by intent: Removed ⇒ Absent; Rearm ⇒ Reopening (intent ← None); otherwise Suspended
  (the store clears CLOSE: the close it asked for has happened)
Suspended, signalled with intent Rearm:
  CAS(Suspended → Reopening)
    fails ⇒ CLOSE was set by a later QUERYREMOVE ⇒ store Suspended (clear it) and stay closed
```

**Why exactly one side closes.**
1. While the handle is open, `CLOSE` can only be **set** (by a callback). Nothing clears it
   until the close it asked for has happened: the clearing stores are the drain's end, and
   the `Suspended` and `Failed` stores, all made with no handle open.
2. Each handle-holding base has exactly **one** party that may move it out:
   - `Issuing` and `Reopening` belong to the watcher alone.
   - From `Idle`, the watcher's exit is `Idle → Issuing`, and a swap to `Closing` may be
     made only from `Idle|CLOSE`, by either side.
3. Every exit from a handle-holding base expects `CLOSE` clear, **except** the swap to
   `Closing`. So once `CLOSE` is set, the next transition out of that base must be to
   `Closing`. Exactly one compare-and-swap succeeds at that, so exactly one party closes.
4. The callback's `fetch_or` cannot fail. A request is therefore never lost, which was the
   defect: (c)'s request lived only in a swap that could fail.
5. The callback either wins `Idle|CLOSE → Closing` (it closes), or returns having set a bit
   that the watcher's next swap must observe (the watcher closes), or finds the handle
   already closed or being closed.

**Termination and the residual.** The callback performs at most one `fetch_or` and one
compare-and-swap per call, so it is wait-free. It does not loop, and it cannot be starved
by a busy watcher. When the watcher is the closer, the close happens at the watcher's next
transition: within its current re-issue (microseconds), or at once when woken. That is
(c)1's stated residual, and E-2 measures it. The ordering of `intent` assumes the CM callbacks
for one registration arrive one at a time, in order. That is assumption **A-1**, and E-2
checks it by logging the action sequence. If it does not hold, `intent` takes a sequence
number per callback, and the drain's end reads the newest.

### (d)2 · The boundary races, and who closes

| # | interleaving | what each side sees | who closes |
|---|---|---|---|
| 1a | Callback at `Idle`; the watcher wins `Idle → Issuing` **before** the callback's `fetch_or` | The callback's `prev` is `Issuing`, so it returns. The watcher's `Issuing → Idle` swap fails (`CLOSE`) | **watcher** |
| 1b | Callback at `Idle`; the callback's `fetch_or` lands **before** the watcher's `Idle → Issuing` | The watcher's swap fails (`CLOSE` set). Both try `Idle\|CLOSE → Closing` | **whichever wins that swap**, exactly one |
| 2a | Callback at `Issuing`; its `fetch_or` lands before the watcher's `Issuing → Idle` | The watcher's swap fails | **watcher** |
| 2b | Callback at `Issuing`; the watcher restores `Idle` **before** the callback's `fetch_or` | The callback's `prev` is `Idle` with no `CLOSE`, so it tries `Idle\|CLOSE → Closing`, racing the watcher's next completion as in 1b | **callback**, or the watcher if its next completion wins the swap; exactly one |
| 3 | Callback during `Closing` or `Draining` | `prev` is `Closing`/`Draining`, so the callback returns. The drain's end clears `CLOSE` | **already closing** (the earlier winner); no second close |
| 4 | Callback during `Reopening`, before the new handle's first read | The callback returns. The watcher's `Reopening → Idle` swap fails | **watcher** (closes the new handle) |
| 5 | Callback during `Suspended`, `Absent` or `Failed` | No handle. The bit makes the watcher's next `Suspended → Reopening` or (c)2 rearm swap fail, and the member stays closed (a later query-remove wins over an earlier failed one) | **nobody: nothing is open** |
| 6 | Two callbacks (`QUERYREMOVE` then `REMOVECOMPLETE`) around one watcher transition | The second `fetch_or` finds `CLOSE` already set and attempts nothing. `intent` becomes `Removed` | **the closer of the first callback**; the drain ends `Absent` |

### (d)3 · The test that pins it: an exhaustive interleaving model, hand-written

**`loom` is not available**: no crate in the workspace depends on `loom` or `shuttle`
(`Cargo.toml` files and `Cargo.lock` searched at `ae933eb7`). Adding one is a new
third-party dependency, with a crate-edge, notice and wasm-floor review of its own, and this
protocol does not need it. **T-DIRWATCH-LIFECYCLE ships a hand-written exhaustive interleaving
test over the real transition functions.**

**Production code is written as pure steps** over a `u32` and the intent byte, in
`bt-platform`'s slot module:
- `callback_steps(action) -> [Step; ≤3]`
- `watcher_step(base_event, word) -> (Step, Effect)`

The atomics are thin wrappers that apply one step as one atomic operation: `fetch_or`,
`compare_exchange` or `store`. `Effect` names `Close`, `Drain` or nothing.

**The test** (`slot_model_tests::every_interleaving_closes_the_handle_exactly_once`) keeps
the word in a plain cell and enumerates by recursion **every** interleaving of:
- one callback (`QUERYREMOVE`, and in a second run `REMOVECOMPLETE`, and in a third
  `QUERYREMOVE` followed by `QUERYREMOVEFAILED` followed by `QUERYREMOVE`);
- the watcher, starting from each of `Idle` (parked), `Issuing` and `Reopening`, and running
  up to two completion cycles.

That is at most a few hundred schedules. In every schedule it asserts that:
- the handle is closed **exactly once**;
- the close happens while no other side is in a handle-using step;
- the callback ran at most two atomic operations (wait-freedom);
- the end state is `Suspended`, `Absent` or `Reopening`/`Idle` as the intent says.

The table of (d)2 is the test's named-schedule list. A failure prints the schedule.

**Mutations** (each observed red):
- (c)'s one-shot protocol (the callback reads, then makes one swap, with no `fetch_or`):
  schedules 1a and 2b close **zero** times.
- The watcher's `Issuing → Idle` written as an unconditional store: 1a and 2a close zero
  times.
- The `Idle|CLOSE → Closing` swap written as a store on both sides: 1b closes **twice**.
- The drain's end store not clearing `CLOSE`: the third run's final rearm never happens.

These replace (c)1's first two tests for this protocol:
`…::a_query_remove_while_idle_closes_inside_the_callback` and
`…::a_query_remove_while_issuing_…`. They remain as two named executed tests over the real
thread with gates, on Windows. The model test runs on every platform: the slot logic is
portable, and only the `CancelIoEx`/`CloseHandle` effects are Windows-only.

**Where it lands:**
- The protocol and the model test belong to **T-DIRWATCH-LIFECYCLE** (the slot is (c)7's
  "atomic slot without notifications").
- The callbacks that drive it, and the executed gate tests, stay in **T-WATCH-LETS-GO**.
- Sizes and the order are unchanged (L, M, M, M, M; 1 → 2 → 3 → 4, and 1 + 2 + 3 → 5).
- The owner questions are unchanged (Q1–Q5 as in (c)10). The answer to Q3 is unaffected:
  the residual is the same microsecond window, now with a proof that the close is never lost.

### Departures

One, argued: **the callback does not loop.** The review asks for retry or re-evaluation
until the callback "either wins a close-owning transition or observes a state in which the
watcher is guaranteed to honour `CloseAsked`". (d)1 meets that end condition in one step
rather than by looping. The `fetch_or` records the request unconditionally, so after it
**every** state either has the callback attempting the one close-owning swap (`Idle|CLOSE`)
or is a state whose owner is guaranteed to observe the bit (the watcher's exit swaps expect
it clear). A compare-and-swap loop over a state-only word would be lock-free but not
wait-free: a watcher cycling `Idle ↔ Issuing` could make it retry repeatedly inside a
callback that must return quickly [S10]. The bit makes the retry unnecessary. The boundary
races the review names are rows 1a and 2b of (d)2, and the model test enumerates them.

### Sources

- The scoped confirmation: `B5-os-signals-design.review-codex-c.md` (0.4.8 trace, 2026-10-11).
- No new external source. (d)1 uses `fetch_or`/`compare_exchange` semantics as the Rust
  standard library defines them (`std::sync::atomic::AtomicU32`), and [S10] (return quickly)
  and [S31] (drain before reuse) as in (c).
