# Folio on macOS — the 0.4 implementation plan

2026-09-12. Written against `main`, and against the measurements in
`docs/plans/port/macos-spike-2026-09-07.md`, which this plan does not redo. The
spike answered *how much of Folio is already a macOS program*; this answers
*what order the rest is written in, by whom, on which machine, and what the
owner has to check with his own eyes*.

Everything here is a proposal. Nothing is dispatched until the owner has read it
and a Codex read-only review has been taken on it.

---

## 1. Goal and non-goals

**0.4 ships a signed and notarized macOS preview of Folio that a stranger can
download, drag to `/Applications`, and open with no Gatekeeper dialog — with the
reading surfaces at parity: a shell in a pane, the files column, the preview pane
with Markdown (including in-place editing, which is 0.3's work and arrives on
macOS for free), images, video and typeset math, multiple windows that come back
where they were, and the attention channel that tells you a background agent
finished.** Explicitly out of 0.4: Linux (the spike's §7 costs it separately at
+35–45 agent-days and it reuses none of the expensive half); the 0.5 remote split
(this plan is constrained not to make it harder — §4.6 — and otherwise leaves it
alone); the games; anything shaped like an MSIX equivalent, because macOS has no
first-page context menu to register into; a universal (`x86_64` + `arm64`)
binary; a Homebrew cask; in-place self-update; and a macOS twin of
`scripts/dev/ui-probe.ps1`, which the spike's §5 excludes from its numbers and
which this plan excludes too, with the same warning that it is the likeliest
thing here to be underestimated later.

---

## 2. Milestones

Each acceptance line is something the owner can check on the Mac mini's own
screen, in one sitting, without reading a log.

### M0 — the toolchain, the empty backend, and the CI lane

Xcode selected and licensed; a Developer ID Application certificate in the
login keychain; a notarization credential that works over a non-interactive ssh
session; `crates/bt-platform/src/macos/` in the tree with **every** public item of
the platform interface present and answering "not on this platform"; `bt-app`
compiling and linking on macOS; `core-macos` in `.github/workflows/ci.yml` grown
to check `bt-platform` and `bt-app` as well as the thirteen portable crates.
The `.app` bundle layout arrives here too, unsigned — see §4.5 for why it cannot
wait for M5.

> **Acceptance.** On the Mac mini: `security find-identity -v -p codesigning`
> prints one `Developer ID Application` identity (it prints `0 valid identities
> found` today), and `open ~/folio-port/wt/m0/target/debug/Folio.app` puts a
> Folio icon in the Dock for a second and quits with a message saying the window
> backend is not written yet.

### M1 — "opens a window and you can type in a shell"

The owner's named first gate, and the spike's own honest waypoint. A winit
window on macOS; wgpu presenting through Metal on a `CAMetalLayer`; a zsh in a
pane over the POSIX pty; the keyboard including the Cmd dialect; IME good enough
to type Chinese; clipboard; the window's own geometry, backing scale, and dark
mode.

> **Acceptance.** On the Mac mini's screen: double-click `Folio.app`, a window
> opens with a prompt in it; type `ls` and Enter and the directory lists; switch
> to the system Pinyin input source, type `nihao`, pick 你好 from the candidate
> window, and 你好 appears in the pane; `Cmd+C` / `Cmd+V` copy and paste;
> `Cmd+T` opens a second tab and `Cmd+W` closes it.

### M2 — the reading surfaces

The files column and its directory watcher; the preview pane over the same
`read_head` worker it uses on Windows; Markdown including 0.3's in-place editing;
images; typeset math; hover cards; the font and contrast policy checked against
CoreText output rather than assumed from DirectWrite's.

> **Acceptance.** On the Mac mini's screen: open a folder in the files column,
> click a `.md` file with a table and a CJK paragraph in it and the preview
> renders it; edit a heading in place and `cat` the file in a pane to see the
> change; click a `.png` and it shows; open a file with `$$\int_0^1 x\,dx$$` in it
> and the integral is typeset, not printed as source.

### M3 — chrome, several windows, and persistence

The custom window frame against macOS's traffic lights; the application menu bar
and the Cmd dialect of the whole shortcut table; multi-window restore; the
storage directory; single instance over a Unix socket.

> **Acceptance.** On the Mac mini's screen: open two windows with different tabs
> in each, quit with `Cmd+Q`, reopen — both windows come back with their tabs and
> on the same screen; the menu bar has File / Edit / View / Window / Help and
> every item in it works; `ls ~/Library/Application\ Support/Folio` lists
> `session.json` and `settings.json`; launching Folio a second time from Finder
> opens a window in the process that is already running rather than a second
> Dock icon.

### M4 — the platform features that need rewriting

Notifications via `UNUserNotificationCenter` and the Dock tile; the web preview
via `WKWebView`; video first frame and playback via AVFoundation; the global
hotkey and the Accessibility permission flow; "Open in Folio" from Finder via
`NSServices`; the attention channel over a Unix domain socket; hang reports;
the update check. **In 0.4:** all of the above. **Deferred out of 0.4:** a Finder
Sync extension (a second signed bundle inside the app, for a submenu that is
still not the first page — §8, Q3); the sparse-MSIX equivalent, which does not
exist; in-place self-update, which becomes "open the release page"; `wsl.rs`,
`psreadline.rs`, `msix.rs` and `explorer_command.rs`, which are Windows facts and
are simply absent.

> **Acceptance.** On the Mac mini's screen: start a long command in a background
> tab, switch away, and when it finishes a Folio notification arrives in
> Notification Center and clicking it raises that tab; a `.html` file in the
> files column previews as a page; hovering a `.mp4` shows its first frame and
> opening it plays with sound; the summon chord pulls a terminal down over
> whatever is frontmost and puts the foreground back when it retracts;
> right-clicking a folder in Finder offers *Services ▸ Open in Folio*.

### M5 — signing, notarization, the DMG, and the release lane

`codesign` with the hardened runtime and an entitlements file; `notarytool
submit --wait`; `stapler staple`; a `.dmg` with the bundle and an `/Applications`
alias; a `release.yml` lane that produces them.

> **Acceptance.** On the Mac mini: `spctl -a -vvv -t install Folio.dmg` says
> `accepted` and `source=Notarized Developer ID`, and `xcrun stapler validate
> Folio.app` says `The validate action worked!`.

### M6 — acceptance on a machine that has never seen this build

A second macOS account (or a fresh VM), the DMG fetched over the network rather
than copied, and the six things a first run does.

> **Acceptance.** Logged into a second account on the Mac mini: download the
> DMG from the release page in Safari, open it, drag Folio to Applications,
> launch it — no Gatekeeper dialog, no "damaged and can't be opened", no crash —
> and the M1 and M2 acceptance lines above pass again from that account, with
> the only permission prompts being the ones we intend (Notifications at first
> toast, Accessibility at first summon).

---

## 3. The tickets

Sizes are the spike's own unit: an agent-day is one focused delegated session
plus the owner's review of it. **S** ≈ 1, **M** ≈ 2–3, **L** ≈ 4–6.

The "where" column matters more than usual, because there is one Mac and the
isolation rules make it a serial resource. `check` means a Windows agent can do
the whole ticket with `cargo check -p <crate> --target aarch64-apple-darwin`;
**Mac** means it must run on the Mac mini, in its own worktree under
`~/folio-port/wt/<ticket>`, never touching anything else in the owner's home,
**one cargo at a time**.

### M0

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-01 | Select and license Xcode; verify the signing identity and the notarization credential; record the exact commands in `docs/BUILDING.md` | S | — | owner's §5 work | Mac |
| MAC-02 | Prove the Windows cross-check lane: `rustup target add aarch64-apple-darwin`, then `cargo check -p bt-platform --target aarch64-apple-darwin` from a Windows worktree; if it fails, every "check" row below becomes a "Mac" row | S | — | — | check |
| MAC-03 | **Re-measure the platform surface.** The spike's *79 missing items* was taken at `05bf018`; `bt-app` names `bt_platform::` at 519 sites across 36 files today, 249 of them in `main.rs`, and three modules have been added since (`launch_pipe`, `instance`, `handoff`). Produce the current list and check it in as `docs/plans/port/platform-surface.md` | M | — | MAC-02 | check |
| MAC-04 | **The empty macOS backend.** `crates/bt-platform/src/macos/` with every item on MAC-03's list present and refusing; every `#[cfg(windows)] pub mod` that `bt-app` names becomes a portable module name with a Windows body, a macOS body and a nothing body (§4.3) | L | bt-platform | MAC-03 | check |
| MAC-05 | `bt-app` links on macOS: the five `std::os::windows` uses in `{files,cli,palette_index,main,git_panel}.rs`, the one `winit::platform::windows::EventLoopBuilderExtWindows`, and `bt-winres`'s build script | M | bt-app, bt-winres | MAC-04 | Mac |
| MAC-06 | The unsigned `.app` bundle: `packaging/macos/Info.plist.in`, a bundle identifier, and `scripts/release/mac-bundle.sh` that lays out `Folio.app` from `target/<profile>/folio` | M | bt-winres | MAC-05 | Mac |
| MAC-07 | CI: `core-macos` grows `-p bt-platform -p bt-app`; `scripts/check-portable-core.ps1` grows the macOS spellings (§4.2); a new `scripts/check-app-platform-cfgs.ps1` holds the named list of `bt-app` files allowed a `#[cfg(target_os)]` | M | — | MAC-05 | check |

### M1

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-10 | Window and screen: `NSWindow` / `NSScreen` for `get_window_rect`, `set_window_outer_rect`, `get_work_area`, `work_area_at`, `virtual_screen_rect`, `monitor_id_at`, `dpi_at`, `get_dpi_for_window`, `window_is_exposed`, `is_window_minimized`, `set_window_topmost`, `request_window_close`, `stand_window_at` | L | bt-platform | MAC-04 | Mac |
| MAC-11 | The Metal surface: a `CAMetalLayer` under the window's content layer, reached through `WindowTarget::Hwnd(SurfaceTarget)` — the portable door `bt-render` already documents at `lib.rs:3352`. **The `Compositor` / `CALayer` tree is not in M1**; it exists for the web panes and belongs with MAC-40 | M | bt-render, bt-platform | MAC-10 | Mac |
| MAC-12 | `resolve_default_shell` learns Unix: `BT_SHELL`, `$SHELL`, the password database, `/bin/sh`, and a real-pty test. **This is also remote T2** — one ticket, two milestones; coordinate before dispatching | M | bt-pty | MAC-04 | Mac |
| MAC-13 | Keyboard: modifiers, `virtual_key_for_character`, `wheel_scroll_amount`, `take_keyboard_focus`, `thread_mouse_capture`, `pointer_position`, `top_level_window_at`, and the Cmd dialect of `BINDINGS` as a third column of the one table (§4.7) | L | bt-app, bt-platform | MAC-10 | Mac |
| MAC-14 | IME: winit's `Ime::Preedit` / `Ime::Commit` and `set_ime_cursor_area` on macOS; `ImeSystemCaret` becomes a no-op type, because the caret rectangle is winit's whole story there and `ImmGetCompositionWindow` has no counterpart | M | bt-platform, bt-app | MAC-13, R1 | Mac |
| MAC-15 | Clipboard: `NSPasteboard`. **Drop the `hwnd` parameter from `clipboard_text`** — it exists only because Win32 clipboard access is window-scoped, which the spike's §4.8 correctly calls Windows leaking into the interface | S | bt-platform, bt-app | MAC-10 | check |
| MAC-16 | Dark mode, backdrop and the window skirt: `NSVisualEffectView`, `system_uses_light_apps`, `set_window_dark_mode`, `set_system_backdrop`, `system_backdrop_available`, `client_area_animation_enabled`, `install_window_class_background` | M | bt-platform | MAC-10 | Mac |
| MAC-17 | `storage_dir()` learns macOS: `~/Library/Application Support/Folio`, with the cache halves (`WebView2`, `player`) going to `~/Library/Caches/Folio` | S | bt-app | MAC-04 | check |

### M2

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-20 | `DirWatch` / `DirChange` over FSEvents, keeping `ReadDirectoryChangesW`'s non-recursive contract and the `watch_clock` debounce the five `*_watch.rs` consumers already expect | M | bt-platform | MAC-04 | Mac |
| MAC-21 | The process door on macOS: `handoff.rs` gets `open_local_file`, `open_local_path`, `reveal_in_explorer`, `shell_execute`, `open_system_fonts_page` over `NSWorkspace`, and `recycle` over `NSFileManager trashItem`. `quiet_command` becomes a portable `Command` builder that sets no flag off Windows — the `no_command_is_built_outside_the_quiet_door` gate (§7.40 ①) keeps its meaning | M | bt-platform | MAC-04 | Mac |
| MAC-22 | Pickers: `FolderPicker`, `ImagePicker`, `FilePickKind` over `NSOpenPanel`; `message_box` over `NSAlert` | M | bt-platform | MAC-10 | Mac |
| MAC-23 | `monospace_font_families()` over `CTFontCollection` — the one place the spike found DirectWrite, and the one piece of the font policy that class B could not take because it lives in `bt-platform` | S | bt-platform | MAC-04 | Mac |
| MAC-24 | Contrast and stem darkening measured against CoreText: `bt-render/src/contrast.rs` was tuned against DirectWrite output and its constants are a reading of one rasterizer | M | bt-render | MAC-11 | Mac |
| MAC-25 | Reading-surface acceptance sweep: files column, preview pane, Markdown editing, images, math, hover cards — almost all of it class A, so this ticket is a walk with screenshots and a list of what is wrong, not a rewrite | M | — | MAC-20…24 | Mac |

### M3

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-30 | The application menu bar: File / Edit / View / Window / Help, wired to the same verbs the palette and the chrome already call | L | bt-app, bt-platform | MAC-13 | Mac |
| MAC-31 | `CustomWindowFrame` against the traffic lights: the hit-testing policy in `bt-platform`'s portable half is already pure and already passes off Windows; what is new is where the three buttons sit and that the title bar is the system's | M | bt-platform, bt-app | MAC-16 | Mac |
| MAC-32 | Multi-window restore on macOS: `windows[]` in `session.json` is portable, the screen arithmetic under it is not; `hide_every_window_of_this_process`, `flash_window`, `is_window_cloaked`, `Taskbar` (Dock tile) | M | bt-platform, bt-app | MAC-10 | Mac |
| MAC-33 | Single instance over a Unix domain socket: `instance::claim_data_directory` over an `flock`'d file in the data directory (it already returns `None` off Windows at `instance.rs:158`), and `launch_pipe` over a socket at the same address-by-digest scheme, keeping §7.59b's `Decision` / `Admission` / `Refusal::NotServing` and the client-`CONFIRM`-is-the-commit-point rule verbatim. **`AllowSetForegroundWindow` has no counterpart** — macOS activation is `NSApp.activate()` and needs no permission from the other side, so step ③ of §7.59's four-step dance collapses | M | bt-platform, bt-app | MAC-21 | Mac |
| MAC-34 | The first-run card on macOS: §7.56 ③ already says a row only appears when it can be honoured, so the four Windows rows are absent rather than special-cased, and two macOS rows (Notifications, Accessibility) take their place | M | bt-app | MAC-30 | Mac |
| MAC-35 | Console and standard streams: `adopt_parent_console`, `detach_console`, `write_to_console`, `redirect_std_streams_to_file`, `silence_std_streams`, `install_console_ctrl_handler`, `leave_process`. On macOS a GUI app launched from a terminal already has that terminal's stdio, so most of these become the empty answer — which is a decision worth writing down, not a gap | S | bt-platform | MAC-04 | check |

### M4

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-40 | The `CALayer` compositor: `Compositor::{attach,detach,place,web}_visual` and `commit` over a layer tree, with the `CAMetalLayer` non-opaque so the holes `set_web_holes` punches show what is underneath | M | bt-platform, bt-render | MAC-11 | Mac |
| MAC-41 | The `WKWebView` host: `WebHost`, `WebEvent`, `WebChord`, `WebMouseEvent`, `WebNavigationVerdict`, `RehostOutcome`, `RehostSide`, `web_mouse_buttons`, `forget_web_environment`. `webnav.rs`'s policy is pure and portable and is not touched; what is rewritten is the conversation, and `bt-app`'s 2,427 lines of it in `{webhost,webnav,web_thumb,web_trace}.rs` follow | L | bt-platform, bt-app | MAC-40 | Mac |
| MAC-42 | Video first frame over `AVAssetImageGenerator`, replacing `IMFSourceReader` | M | bt-platform | MAC-11 | Mac |
| MAC-43 | Video playback over `AVPlayerItemVideoOutput` + `copyPixelBufferForItemTime`, keeping route B's shape exactly: decode off the render thread, read back on the platform side, hand `bt-render/src/video.rs` a buffer. The reason that shape exists — `bt-render` may not hold `unsafe` — is a workspace rule, not a Media Foundation detail, so it survives the port unchanged | M | bt-platform, bt-render | MAC-42 | Mac |
| MAC-44 | Notifications: `Notifier` over `UNUserNotificationCenter`, and the Dock tile for `flash_window`. **Needs a bundle identifier and a bundle**, which is why MAC-06 is in M0 | M | bt-platform | MAC-06, MAC-32 | Mac |
| MAC-45 | `AttentionPipe` over a Unix domain socket in the data directory, with the same 4096-byte frame cap, the same declared-fields-only rule, and file-mode `0600` in place of the logon-SID DACL | M | bt-platform, bt-app | MAC-33 | Mac |
| MAC-46 | The global hotkey: `GlobalHotkey`, `Hotkey`, `HotkeyFault` over a `CGEventTap`, the Accessibility permission asked for at first summon rather than first run, and `give_foreground_to` over `NSRunningApplication.activate` | M | bt-platform, bt-app | MAC-32, R10 | Mac |
| MAC-47 | "Open in Folio" as an `NSServices` entry in `Info.plist`, taking a folder and spawning `folio --cwd <dir> --from-explorer` — which §7.59a's table already routes to a tab rather than a window, and that routing is correct here for the same reason | S | bt-app | MAC-33 | Mac |
| MAC-48 | The update check over `NSURLSession`, replacing `http.rs`'s WinHTTP wrapper, and keeping its argument: the machine's proxy configuration and certificate store are the machine's answers. The in-place swap in `update.rs` becomes "open the release page" | M | bt-platform, bt-app | MAC-21 | Mac |
| MAC-49 | Hang and crash reports: `backtrace` plus `NSSetUncaughtExceptionHandler` in place of `RtlCaptureStackBackTrace` / `SetUnhandledExceptionFilter`. The suspend-and-sample trick in `hang.rs` has no safe macOS counterpart; recommend the watchdog keeps its liveness half (§1.5a) and loses its stack half there, stated rather than faked | M | bt-platform, bt-app | MAC-32 | Mac |

### M5 and M6

| ID | Title | Size | Crates | Depends on | Where |
|---|---|---|---|---|---|
| MAC-50 | `packaging/macos/Folio.entitlements`, the hardened runtime, and `scripts/release/mac-sign.sh` | M | — | MAC-06 | Mac |
| MAC-51 | Notarization and stapling: `scripts/release/mac-notarize.sh` over an App Store Connect API key (§5, R6), and `docs/RELEASING.md` grows a macOS section beside the Windows one | M | — | MAC-50 | Mac |
| MAC-52 | The DMG: `scripts/release/mac-dmg.sh`, a background image, an `/Applications` alias | M | — | MAC-51 | Mac |
| MAC-53 | `release.yml` grows a `macos-latest` lane that builds, bundles, signs on the runner or hands the artefact back for signing on the Mac, and attaches the DMG and its SHA-256 to the release | M | — | MAC-52 | check |
| MAC-54 | Docs: `README.md` and `README.zh-CN.md` get a macOS download and first-run section in lockstep; `docs/shortcuts.md` regenerates with the Cmd column; `docs/BUILDING.md` grows the macOS build; new fixtures get `PROVENANCE.md` entries | M | bt-app | MAC-30, MAC-53 | check |
| MAC-60 | Clean-account acceptance: the M6 line, run from a second macOS account, with screenshots | M | — | MAC-53 | Mac |

**Count: 38 tickets — 9 S, 22 M, 7 L.** Nine can run on Windows agents
(`check`), twenty-nine need the Mac.

**Dispatch order.** MAC-02 first and alone, because every `check` row below it is
a claim that it works. Then MAC-03 and MAC-04 together — the empty backend is the
single largest unblocker in the plan and it is pure `#[cfg]` work that a Windows
agent can do. Then the Mac becomes the bottleneck and stays the bottleneck.

---

## 4. Architecture rules the tickets inherit

### 4.1 The standing rule does not change

**Platform code lives behind `bt-platform`'s interface; no crate below `bt-app`
calls the platform directly.** `docs/DESIGN.md` §13.1 states it, and it is not a
portability rule by origin — the workspace's `unsafe_code = "deny"` exempts
exactly one crate, and every Win32 call is `unsafe`, so the front door was built
for a different reason and turns out to be the port. The same sentence now binds
`objc2`: every message send lives in `bt-platform` or it does not exist.

### 4.2 The gate learns a second platform

`scripts/check-portable-core.ps1` refuses `windows::`, `windows_sys::`, `winapi`,
`webview2` and `std::os::windows` in the thirteen portable crates outside a
`#[cfg(windows)]`. MAC-07 adds `objc2`, `objc2_*`, `core_foundation`,
`core_graphics` and `core_text` to `$forbidden`, gated on
`#[cfg(target_os = "macos")]` by the same brace-depth walk. `std::os::unix` is
deliberately **not** added: `bt-pty` is a unix crate by construction and the
vendored `portable-pty` under it is more so.

### 4.3 `bt-app` gains no new `#[cfg(target_os)]`

Today `bt-app` carries a platform `cfg` in exactly eleven files —
`psreadline.rs`, `attention_copilot.rs`, `files.rs`, `explorer_menu.rs`,
`wsl.rs`, `update.rs`, `shell_integration.rs`, `settings.rs`,
`palette_index.rs`, `git_panel.rs`, `main.rs` — and calls `bt_platform::` at 519
sites across 36 files with no gate at all. **That ratio is the whole design and
the port must not spoil it.** The named list above is the permitted set; MAC-07
writes `scripts/check-app-platform-cfgs.ps1` to hold it, and a ticket that wants a
twelfth file has to argue for it in review.

The consequence is that the modules `bt-app` names must exist on every platform.
`#[cfg(windows)] pub mod webview` and its five siblings (`video`,
`attention_pipe`, `launch_pipe`, `explorer_command`, `http`) become plain
`pub mod` declarations whose bodies are `#[cfg(windows)] mod win`,
`#[cfg(target_os = "macos")] mod mac`, and `#[cfg(not(any(windows, target_os = "macos")))] mod none`,
re-exporting one set of names. One interface, three bodies.

### 4.4 Stub at runtime, not at compile time — with one exception

**Recommendation: a Windows-only feature is present on macOS and refuses at
runtime.** The item exists, keeps its signature, and answers `false`, `None`,
`Err(PROGRAM_REFUSED)` or the empty list. This is not invented here; it is
already the repository's practice in four places —
`bt_platform::set_current_thread_priority`'s portable arm answering `false`,
`instance::claim_data_directory` returning `None` at `instance.rs:158`, three
`#[cfg(not(windows))]` arms in `hang.rs`, and one at `hotkey.rs:623`. It is the
right default because it keeps `bt-app` free of gates (§4.3), because a refusal
can carry a reason to a toast and an absence cannot, and because §7.56 ③ already
gives the product a rule for a row that cannot be honoured: the row does not
appear.

The exception is a module that is *wholly* one platform's SDK conversation and
has a genuinely different macOS body — `webview`, `video`. There the `#[cfg]`
stays inside the module (§4.3) and the refusing arm is the third body, `none`,
used by neither shipped platform.

Three things are absent rather than refusing, because they name a Windows fact
that has no macOS referent at all: `wsl.rs`, `psreadline.rs`, and `msix.rs` +
`explorer_command.rs`. Their `bt-app` call sites are already inside the eleven
permitted files.

### 4.5 Paths, directories, and the bundle

`bt_app::persist::storage_dir()` (`persist.rs:1168`) resolves `%APPDATA%\Folio\`
with a `OnceLock` and a one-time relocation from the old name. MAC-17 gives it a
macOS arm — `~/Library/Application Support/Folio` — and moves the two cache
halves (`%LOCALAPPDATA%\Folio\WebView2`, `…\player`) to `~/Library/Caches/Folio`.
**The key names inside those files do not change.** `settings.json` has one
schema on both platforms, one migration chain, and no `_mac` suffixes; a setting
that only one platform can honour is still present in the file, because a
profile carried between machines that silently lost fields would be worse than
one carrying a field nobody reads.

The bundle lives at `packaging/macos/`, beside `packaging/msix/`:
`Info.plist.in`, `Folio.entitlements`, and the DMG's staging. **The bundle layout
is pulled forward into M0 (MAC-06), not left to M5**, and the reason is a
dependency the ticket order would otherwise hide:
`UNUserNotificationCenter` refuses to register for a process with no bundle
identifier, `WKWebView`'s data store is keyed on one, and `NSServices` is read
out of `Info.plist`. Three M4 tickets therefore depend on the bundle existing,
and only *signing* depends on M5. M5 signs, notarizes, packages and ships what
M0 already built.

`bt-winres`'s `a_release_is_one_version_in_four_places` becomes five places, and
the fifth is `Info.plist`'s `CFBundleShortVersionString` and `CFBundleVersion`.
MAC-06 grows the crate a plist emitter as the sibling of its `VERSIONINFO`
emitter, which is exactly the analogy the spike's §5 drew.

### 4.6 Nothing here makes the 0.5 remote server harder

`docs/plans/remote/research-2026-09-10.md` §1 puts the remote server on a Unix
box taking the portable core plus `bt-pty` — "everything: spawn, write, resize,
read, wait, the output ring" — and taking `bt-platform` as an *empty shell*, which
works today only because that crate has no dependencies off Windows. MAC-04 ends
that: the macOS body brings `objc2` and its neighbours. **The rule the tickets
inherit is that those dependencies go under
`[target.'cfg(target_os = "macos")'.dependencies]`, never under a bare
`[dependencies]`**, so a Linux server still links a crate with no dependencies at
all. MAC-12 is the point where the two milestones actually meet, and it is a gift
rather than a cost: a Unix `resolve_default_shell` is remote T2, and doing it
once for both is the whole reason to name it here.

### 4.7 The documents get twins, not forks

`docs/shortcuts.md` is generated — `scripts/generate-shortcuts-table.ps1` runs
`bt_app::shortcuts::tests::docs_shortcuts_md_is_the_bindings_table`, which walks
`BINDINGS` and asks each row for its name, chord and scope *in both languages*.
MAC-13 adds a third dimension to that walk, not a second file: the table grows a
Cmd column, `scripts/check-shortcuts-table.ps1` stays one gate, and a row whose
macOS chord differs is a fact the source table states rather than a document
somebody remembers to edit.

`README.md` and `README.zh-CN.md` grow a macOS download and first-run section in
lockstep, by the rule already recorded for them: the same fact in both, no new
section on one side only. The Chinese copy is written by the project's usual
route and not translated inline by the ticket. `docs/design/PROVENANCE.md` and
`tests/assets/PROVENANCE.md` are unchanged in kind; any new fixture the port adds
(a `.mov`, a Retina screenshot) arrives with its generating command, as every
fixture there already does.

---

## 5. What the owner must do personally

Five of these cannot be delegated, because they need an Apple ID, a `sudo`
password, or a hand on the machine's own keyboard. Each is followed by the exact
command the agent will run to confirm it is done — all of them read-only, all of
them safe over `ssh -o BatchMode=yes mac-mini`.

**① Point the developer tools at Xcode.** Xcode 26.6 is already installed at
`/Applications/Xcode.app` (measured today), but `xcode-select -p` still answers
`/Library/Developer/CommandLineTools`, so `xcodebuild` refuses to run.

```
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
```
*Verify:* `xcode-select -p` prints the Xcode path, and `xcodebuild -version`
prints `Xcode 26.6` instead of today's `requires Xcode` error.

**② Create the Developer ID Application certificate.** Xcode ▸ Settings ▸
Accounts, sign in with the Apple ID that holds the Developer Program membership,
select the team, Manage Certificates ▸ **+** ▸ Developer ID Application.
*Verify:* `security find-identity -v -p codesigning` names a
`Developer ID Application: … (TEAMID)` line and ends `1 valid identities found`.
It says `0 valid identities found` today.

**③ Put a notarization credential where a headless session can reach it.**
**Recommendation: an App Store Connect API key, not an app-specific password and
not a `notarytool store-credentials` keychain profile.** This is not a
preference. Measured today: `xcrun notarytool history --keychain-profile folio`
over a non-interactive ssh session answered
`Error: keychainLocked(keychainName: "default")` — the login keychain is not
unlocked in an ssh session, so a keychain profile makes notarization impossible
to automate without the owner typing a password every release. An API key is a
file. Create it at App Store Connect ▸ Users and Access ▸ Integrations ▸ Keys,
with the **Developer** role, download the `.p8` once, and place it at
`~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8` with mode `600`; put the key
id and issuer id in `~/.appstoreconnect/folio.env`, also `600`.
*Verify:*

```
xcrun notarytool history \
  --key ~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8 \
  --key-id <KEYID> --issuer <ISSUER-UUID>
```
returns a history — an empty one is a pass. **Nothing about this goes in the
repository**, and `scripts/check-machine-paths.ps1` would not catch a leaked key,
so `packaging/macos/` gets a `.gitignore` for `*.p8` on the way past.

**④ Grant the two permissions from the machine's own screen, when asked.**
Notifications at the first toast (M4), Accessibility at the first summon (M4).
Neither can be granted over ssh. *Verify:* the agent does not — MAC-44 and
MAC-46's acceptance lines are the owner watching them work.

**⑤ Decide the disk.** `/` has **48 GiB free** and `~/folio-port` already holds
**13 GiB** from the September spike. A release `target/` for this workspace was
5.0 GiB on that machine; several worktrees will not fit. Recommend the spike
checkout is reset to a clean clone of `main` at MAC-01 and every worktree shares
one `CARGO_TARGET_DIR` under `~/folio-port/target`, with a `cargo clean` between
milestones.
*Verify:* `df -h /` and `du -sh ~/folio-port`.

One more, optional and worth a minute of the owner's time: **a second display,
or a scaled mode on the one there is.** The Mac mini drives a single
DELL S2725QS at 3840×2160 presenting as 1920×1080 — a backing scale of exactly
2.0, everywhere, always. The cross-scale transition the spike names in §5 cannot
be exercised on that configuration at all (R7).

---

## 6. Risks, and the probe that retires each one

**R1 — winit IME on macOS with Chinese input.** Folio's IME story has broken
once already on Windows, in a way that took a §7.34 ① investigation to explain,
and the macOS path is winit's `Ime` events plus `set_ime_cursor_area` with no
`ImeSystemCaret` under it. *Probe, before MAC-14 is written:* a forty-line
winit-only example on the Mac that logs `Ime::Preedit` and `Ime::Commit` and
moves the cursor area, driven by the built-in Pinyin input source. If winit does
not report preedit from a third-party IME the way it does from Apple's, that is
the same class of finding the Windows spike recorded about one vendor, and the
plan wants it before the ticket, not inside it.

**R2 — Metal and the layering the floats need.** The web panes are composed
*under* the terminal's picture on Windows: the WebView2 visual is the bottom child
of the DirectComposition tree and wgpu punches premultiplied-transparent holes
through its own frame. *Probe, before MAC-40:* a standalone Mac program with a
non-opaque `CAMetalLayer` over a plain `NSView` in one window, the layer clearing
a rectangle to `(0,0,0,0)`, and a screenshot showing the view through the hole.
If the hole is black rather than transparent the compositor design changes, and
MAC-41 changes with it.

**R3 — ConPTY versus a POSIX pty.** Low, and the spike says why: `bt-pty` calls
`native_pty_system()`, its ConPTY-specific entry points already carry
`#[cfg(not(windows))]` arms, and `core-linux` runs 21 of its tests on a real pty
today. The residual risk is one function: off Windows, `resolve_default_shell`
still answers `powershell.exe`, and its `empty_bt_shell` test passes only because
`find_pwsh` fails to find it. *Probe:* MAC-12's own real-pty test — a ticket
rather than a spike, because only the writing is left.

**R4 — CJK font fallback and the rasterizer.** Half retired already: class B
landed a macOS arm in `terminal_font_system()` with PingFang SC/TC/HK, Hiragino
Sans, Apple SD Gothic Neo and Hiragino Sans GB in the chain, asking `fontdb`'s
system loader for the inventory and checking each family name against the
database. What is unmeasured is how it *looks*: CoreText stems and hints
differently from DirectWrite and `bt-render/src/contrast.rs` was tuned against
the latter. *Probe:* MAC-24 opens a mixed Latin/CJK document at three font sizes
and the owner looks at it beside a Windows screenshot of the same file.

**R5 — `.cargo/config.toml`'s `+crt-static`.** That flag is set under `[build]`,
for all targets, and it is what broke `core-linux` until that job overrode it with
`RUSTFLAGS: -C target-feature=-crt-static`; the `ci.yml` comment records that
macOS ignores the feature. What is *not* recorded anywhere is whether
`cargo check --target aarch64-apple-darwin` from a Windows host survives it, and
nine tickets in §3 assume it does. *Probe:* MAC-02, which is the first ticket
dispatched and exists for no other reason. If it fails, the fix is a
`[target.x86_64-pc-windows-msvc]` table rather than `[build]` — which the config
file's own comment warns would be shipping an untested configuration, so the
honest alternative is that those nine rows move to the Mac and the critical path
grows by about a week.

**R6 — the locked keychain.** Measured, not predicted: `notarytool` over a
non-interactive ssh session refused with `keychainLocked`. Retired by §5 ③'s
API-key route, and the reason it is in this list at all is that discovering it
during M5 would have looked like a broken release lane.

**R7 — one display, one scale.** Measured: a single 4K panel at a backing scale
of 2.0. The spike's §5 warns that what is untested on macOS is the *transition*
between a 2.0 display and a 1.0 one, and on this machine that transition cannot
happen. *Probe:* a second display, or a scaled resolution set by hand for one M3
session. Otherwise MAC-32 ships with the cross-scale path stated as untested —
worse than it sounds, because §7.50 records exactly that class of bug, a window
that could not cross the seam between two screens, reaching a user on Windows.

**R8 — the 79 is stale.** The spike measured 321 errors naming 79 missing items
at `05bf018`, five days and three subsystems ago. `bt-app` names `bt_platform::`
at 519 sites today, 249 of them in `main.rs`, against the spike's 238 — and
`launch_pipe`, `instance` and the single-instance work all landed in between.
*Probe:* MAC-03 re-measures before MAC-04 is scoped, and the plan's effort
numbers in §7 carry a widened band because of it.

**R9 — Accessibility is revoked on re-signing.** The spike says it, and it
matters more than it reads: an agent iterating on MAC-46 re-signs the bundle on
every build, and if each build is a new grant the loop is unusable. *Probe:*
sign the same bundle twice with the same Developer ID identity and see whether
the grant survives. TCC keys on the designated requirement, so a stable Developer
ID should survive and an ad-hoc signature should not — if that holds, MAC-46 must
be developed against a signed bundle from its first build.

**R10 — the hang reporter has no macOS half.** `hang.rs` suspends the window
thread and reads its stack with `GetThreadContext` and `ReadProcessMemory`; its
own module comment calls it the only part of the crate that can deadlock the
process, and there is no macOS equivalent that is safe from inside the same
process. *Probe:* none — this is a decision. MAC-49 keeps the liveness half
(§1.5a's "did it wake, does it answer") and drops the stack half rather than
approximating it.

**R11 — `unsafe_code = "deny"` shapes the backend before a line is written.**
Not a risk to retire but a constraint to obey: every `objc2` message send is
inside `bt-platform` and nowhere else, exactly as every Win32 call is. Listed
here so MAC-04's reviewer checks it as a property of the module layout rather
than discovering it in MAC-41.

**R12 — the toolchain pin off Windows.** `rust-toolchain.toml` names
`1.94.1-x86_64-pc-windows-msvc`, which rustup rejects as a channel off Windows —
already solved in CI by `.github/actions/toolchain`. On the Mac mini it is a
launcher rule: every command exports
`RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin`, which is installed there
(measured, beside a default `stable` of 1.98.1), and the toml file is untouched.

---

## 7. Effort, and the critical path

The spike costed the whole port at **59–72 agent-days** to a signed and
notarized preview, of which class B's **4 days are already taken** (2026-09-07),
leaving **55–68**. Its waypoint — "a window that runs" — was **20–25 days**
including those 4.

| Milestone | Agent-days | Where it comes from in the spike |
|---|---|---|
| M0 toolchain, empty backend, CI, bundle | 5–7 | new: the spike costed no stub layer, and it is the largest single thing this plan adds |
| M1 opens a window and types | 13–17 | the 3,449-line core's window/input/clipboard half (10–14) plus the surface, less class B |
| M2 the reading surfaces | 8–10 | the rest of the 3,449-line core: dir watch, pickers, process door, font picker |
| M3 chrome, windows, persistence | 9–12 | menu bar and the Cmd dialect (4–5) + first-run/paths/shell probe (3) + restore |
| M4 the rewritten features | 18–23 | WKWebView (8–10) + video (3) + hotkey (2) + notifications/IME/socket/hang/http (8), less the Finder Sync extension |
| M5 sign, notarize, DMG, CI | 6–8 | bundle+version (2) + signing (3–4) + DMG (2–3) + CI lane (1–2), less the bundle pulled into M0 |
| M6 clean-account acceptance | 2–3 | new |
| **Total** | **61–80** | spike's remaining 55–68 |

**The band is wider and the midpoint higher than the spike's, and three things
account for it.** The empty backend is work the spike did not name, because it
was measuring a gap rather than planning a fill; the platform surface has grown
since `05bf018`, and R8 says how little we know about by how much; and M6 is a
milestone the spike folded into "a signed preview". Everything else maps row for
row, and the spike's exclusion of macOS UI-acceptance tooling is kept along with
its warning about it.

**The critical path is MAC-02 → MAC-03 → MAC-04 → MAC-05 → MAC-10 → MAC-13 →
MAC-30 → MAC-50 → MAC-51 → MAC-52 → MAC-60.** Everything else hangs off it.
Two observations about the path that the ticket table does not show:

*The Mac is the constraint, not the work.* Twenty-nine of thirty-eight tickets
need it and the isolation rule allows one cargo at a time. The nine `check`
tickets sit in M0, M1's edges and M5's tail, which is the best placement
available: M0 is where the path is widest and M5 is where it is thinnest.

*The three long poles are MAC-04, MAC-13 and MAC-41.* MAC-04 gates everything;
MAC-13 gates the menu bar, which gates the release; MAC-41 gates nothing and
costs the most, which makes it the one place the schedule can be cut by a
decision rather than by work (§8, Q5).

---

## 8. Open questions for the owner

**Q1 — Where do settings live?**
*Recommendation: `~/Library/Application Support/Folio`.* Folio is a window before
it is a command, it will be an `.app` with a bundle identifier, and everything
else about it on that machine — the cache, the notification registration, the web
data store — follows Apple's layout already. `~/.config/folio` would make the
settings the only part of the app that disagreed with the rest. No migration
question: there is nothing on macOS to migrate from.

**Q2 — Is the quake terminal in 0.4, and with which mechanism?**
*Recommendation: yes, in 0.4, over a `CGEventTap`, with Accessibility asked for
at the first summon rather than at first run.* The `NSEvent` global monitor is the
tempting answer because it needs no permission, but it cannot swallow the key, so
the chord also reaches the frontmost app — a terminal that pastes your summon
chord into somebody else's editor is not the feature. Asking at first summon
rather than first run means a reader who never presses the key is never asked,
which is the same judgement §7.56 ② already applies to the first-run card.

**Q3 — Does "Open Folio here" survive, and in what form?**
*Recommendation: `NSServices` only, in 0.4; no Finder Sync extension, ever, and
no pretence that either is the first page.* A Services entry is one dictionary in
`Info.plist`, it appears under *Services* on a folder's context menu, and it costs
MAC-47's single day. A Finder Sync extension is a second bundle with its own
signing and its own lifecycle, for a submenu that is still not the first page —
the spike calls both "weaker than what Windows 11 gives", and paying four days
for the weaker of two weak things is the wrong trade.

**Q4 — What is the bundle identifier, and which team signs?**
*Recommendation: a reverse-DNS identifier under a name the project controls —
`com.folioterminal.folio` if the domain is registered, otherwise the repository's
own namespace.* This needs the owner because it is permanent: the identifier is
what TCC keys permissions on, what `UNUserNotificationCenter` registers, and what
a user's granted Accessibility permission is attached to. Changing it after
release re-asks every reader for every permission.

**Q5 — Is the web preview in 0.4 or deferred?**
*Recommendation: in.* It is the most expensive item in the plan (MAC-40 +
MAC-41, 6–9 days) and the one place the schedule could be cut, so the question is
real — but §7.9 makes a page a *preview buffer* rather than an extra feature, the
files column offers `.html` files like any other, and a preview pane that
silently refuses one file type is a hole a reader finds on the first afternoon.
If a shorter 0.4 is wanted, this is the lever; the recommendation is to let M4
finish late rather than ship a preview pane with a gap in it.

**Q6 — arm64 only, or a universal binary?**
*Recommendation: `aarch64-apple-darwin` only for the 0.4 preview.* A universal
binary doubles every compile on the one Mac that is already the critical path,
for a population — Intel Macs, five years out of production — that a preview does
not need to reach. `lipo` is a day's work whenever it is wanted.

**Q7 — Homebrew cask in 0.4?**
*Recommendation: no.* The formula is cheap; what it implies is not — a cadence,
and a promise that the URL keeps working. The winget PR is still waiting on a
human reviewer, and adding a second package ecosystem before the first has
completed one round buys two obligations with one release.

**Q8 — Is a macOS UI-acceptance harness in scope?**
*Recommendation: no, and say so out loud.* The spike excluded it from its numbers
and this plan excludes it from its tickets. The consequence is that every
acceptance line in §2 is the owner in front of the Mac mini rather than a script
— which is why the milestones are six rather than twenty — and that this
project's autonomous screenshot-and-keys loop does not exist there for 0.4, so a
defect comes back as a screenshot and a sentence, exactly as the Windows ones did
before `ui-probe` worked. Revisit in 0.5.

---

## Appendix — what was measured on the Mac mini for this plan

Read-only, over `ssh -o BatchMode=yes mac-mini`, 2026-09-11. Nothing was
installed, built, started or changed.

| Question | Answer |
|---|---|
| Machine | Apple M4, 10 GPU cores, Metal 4; macOS 26.6.2 (build 25G83), arm64 |
| Developer tools | `/Applications/Xcode.app` **is installed**, version 26.6; `xcode-select -p` still answers `/Library/Developer/CommandLineTools`, so `xcodebuild` refuses to run |
| Rust | `rustc 1.98.1` as the default `stable-aarch64-apple-darwin`; `1.94.1-aarch64-apple-darwin` also installed — the pinned version is there |
| Signing | `security find-identity -v -p codesigning` → `0 valid identities found` |
| Notarization | `xcrun --find notarytool` → present in the Command Line Tools; `xcrun notarytool history --keychain-profile folio` → `Error: keychainLocked(keychainName: "default")` over a non-interactive session |
| Display | one DELL S2725QS, 3840×2160 presenting as 1920×1080 — backing scale 2.0, and no second scale available |
| Disk | 228 GiB volume, 48 GiB free; `~/folio-port` holds 13 GiB |
| The spike checkout | `~/folio-port/repo`, detached at `ffdd444`, working tree clean, origin is the project's GitHub remote |
