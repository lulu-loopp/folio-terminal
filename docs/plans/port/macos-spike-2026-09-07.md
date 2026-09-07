# Folio on macOS — a portability spike

2026-09-07. Measured against `main` at `05bf018`, compiled on an Apple Silicon
Mac (macOS 26.6.2, arm64) under Rust 1.94.1 `aarch64-apple-darwin`, `-j 4`.

**No product code was changed and nothing is proposed here.** This document
answers one question — how much of Folio already is a macOS program, and what
the rest would cost — and leaves every decision it raises open.

---

## 1. The short answer

Of the workspace's **187,686 non-test source lines**, about **90 % compile on
macOS today**, and nine of the seventeen crates pass their own test suites there
without a single edit. The Windows-specific code is not spread through the tree:
it is one crate (`bt-platform`) plus the modules of `bt-app` that consume it.

Two errors — **two source lines** — stand between the workspace and a macOS
build of everything below `bt-app`:

| # | Crate | Error | The line |
|---|---|---|---|
| 1 | `bt-render` | `E0599` no variant `CompositionVisual` on `wgpu::SurfaceTargetUnsafe` | `crates/bt-render/src/lib.rs:3422` |
| 2 | `bt-term` | `E0425` `bt_platform::set_current_thread_priority` not found | `crates/bt-term/src/inline_image.rs:203` |

With those two lines stubbed on the Mac (a throwaway shim, reverted; see
Appendix C), `bt-render`, `bt-term`, `bt-corpus` and `bt-pty` all compile
clean, and `bt-app` reaches its own errors: **321 of them, naming 79 distinct
items missing from `bt_platform`**. That number is the port. It is not a
diffuse problem — it is one interface with 79 entry points, and every one of
them is already named, documented and called from a single place.

The credit for that belongs to a decision made for a different reason:
`bt-platform` carries the workspace's only `unsafe`, so every Win32 call in the
product had to be pushed behind its front door. The consequence is that the
front door is the port.

---

## 2. Per-crate results

`cargo check -p <crate> --all-targets`, then `cargo test -p <crate>` for those
that check. Raw logs in Appendix A.

| Crate | check | test | First error class |
|---|---|---|---|
| `bt-unicode` | ok | ok — 3 | — |
| `bt-doc` | ok | ok — 7 | — |
| `bt-detect` | ok | ok — 114 | — |
| `bt-layout` | ok | ok — 52 | — |
| `bt-persist` | ok | ok — 173 | — |
| `bt-winres` | ok | ok — 5 | — |
| `bt-math` | ok | ok — 26 | — (150 s: typst rasters the fixtures) |
| `alacritty_terminal` | ok | ok — 188 | — |
| `bt-transcript` | ok | **6 failed / 130** | Windows path grammar (§3.3) |
| `bt-viewport` | ok | **1 failed / 127** | one `D:/…` test fixture |
| `bt-render` | **fail — 1** | — | `SurfaceTargetUnsafe::CompositionVisual` |
| `bt-term` | **fail — 1** | — | `set_current_thread_priority` |
| `bt-platform` | **lib ok**, test target fails — 2 | — | a `cfg(test)` module not gated `windows` |
| `bt-corpus` | fail (via `bt-render`) | — | dependency only; clean with the shim |
| `bt-pty` | fail (via `bt-term`) | — | lib clean with the shim; **5 windows-only test errors** |
| `portable-pty` | lib ok, `--all-targets` fails | — | 4 example files were not vendored |
| `bt-app` | fail (via `bt-render`) | — | **321 errors / 79 missing platform items** |

Three things worth pulling out of that table.

**Every third-party dependency builds on arm64 macOS.** Not one error in the
`bt-app` probe came from outside `crates/`. typst, mitex, resvg, hayro,
syntect, two-face, image, glyphon, wgpu, winit, portable-pty and the rest all
resolved and compiled without a version change. The lock file is portable as it
stands.

**`bt-platform`'s library compiles on macOS.** Its `[target.'cfg(windows)'.dependencies]`
table means the crate has *no* dependencies off Windows, and `mod windows_impl`
is gated whole. Only its own test target fails, because four test modules
(`web_security_tests` and its neighbours) are `#[cfg(test)]` rather than
`#[cfg(all(test, windows))]` — a two-word fix, and the sort of thing the port
should tidy on the way past rather than plan around.

**`bt-pty` is already a unix crate.** It calls `native_pty_system()`, and its
ConPTY-specific entry points (`conpty_source`, `clear_host_buffer`) already
carry `#[cfg(not(windows))]` arms. The vendored `portable-pty` supports unix by
construction. A macOS pane opens a real pty on the day the crate above it links.

---

## 3. The Win32 surface

Every `windows::`/`webview2` reference in the tree, by file:

| File | Hits | Non-test lines |
|---|---|---|
| `crates/bt-platform/src/lib.rs` (`mod windows_impl`) | 45 | 3,449 |
| `crates/bt-platform/src/webview.rs` | 18 | 1,418 |
| `crates/bt-platform/src/hang.rs` | 12 | 305 |
| `crates/bt-platform/src/explorer_command.rs` | 12 | 215 |
| `crates/bt-platform/src/attention_pipe.rs` | 12 | 657 |
| `crates/bt-platform/src/video/` | 16 | 1,074 |
| `crates/bt-platform/src/hotkey.rs` | 6 | 158 |
| `crates/bt-platform/src/msix.rs` | 3 | 187 |
| `crates/bt-platform/src/http.rs` | 1 | 133 |
| `crates/bt-pty/src/shell.rs` | 2 | one `MetadataExt` |
| `crates/bt-app/src/{files,cli,palette_index,main}.rs` | 6 | five `std::os::windows` uses |

That is the whole of it. **Outside `bt-platform` there are six Win32 references
in the entire product**, and five of them are `std::os::windows` extension
traits (file attributes, wide argv, `symlink_dir` in a test), not API calls.
The sixth is `winit::platform::windows::EventLoopBuilderExtWindows`, installing
the quake hotkey's message hook — already inside `#[cfg(windows)]`.

`bt-platform`'s public surface is 458 non-test lines of portable policy
(hit-testing a custom frame, the DPI arithmetic, the composition offset, the
window skirt, the toast XML, the file-URI parser, the context-menu shape, the
monospace ordering — all of it pure, and all of it already passing off Windows)
sitting in front of **7,453 lines of Windows implementation**.

### 3.1 Classification

Class **A** moves unchanged · **B** swap implementation (portable-pty, winit,
wgpu Metal) · **C** rewrite a layer · **D** new work with no Windows counterpart.

| Class | Where | Non-test lines |
|---|---|---|
| **A** | `bt-unicode`, `bt-doc`, `bt-detect`, `bt-layout`, `bt-persist`, `bt-math`, `bt-corpus`, `alacritty_terminal`, `portable-pty`, `bt-viewport`, `bt-term`, `bt-pty`, `bt-transcript` (less §3.3), `bt-render` (less §3.3), `bt-winres`, and the non-platform bulk of `bt-app` | **≈ 168,900** |
| **B** | `bt-render` surface + font policy (~90), `bt-transcript/paths.rs` grammar (~250 of 994), `bt-term` 1 line, `bt-pty` + `bt-platform` test gating (~60), two workspace `Cargo.toml` feature rows | **≈ 400** touched |
| **C** | `bt-platform` Windows implementation 7,453 + `bt-app`'s platform-facing modules ≈ 8,900 | **≈ 16,350** |
| **D** | nothing exists yet | **0** (est. 3,000–4,000 new) |

Class A is **90 %** of the workspace. Class C is **8.7 %**.

### 3.2 Class C, itemised

`bt-platform` — 7,453 lines to be re-answered against AppKit:

| Concern | Windows today | macOS answer | Lines |
|---|---|---|---|
| Window, screen, DPI, work area, exposure, topmost, dark mode, backdrop | `windows_impl` in `lib.rs` | `NSWindow`, `NSScreen`, `NSVisualEffectView` | 3,449 |
| Web pane host | WebView2 (`ICoreWebView2`) | `WKWebView` | 1,418 |
| First frame of a video | Media Foundation `IMFSourceReader` | `AVAssetImageGenerator` | 1,074 |
| Attention channel | named pipe | unix domain socket | 657 |
| Compositor | DirectComposition visual tree | `CALayer` tree | inside the 3,449 |
| Right-click menu server | `explorer_command.rs` (`IExplorerCommand`) | Finder Sync extension or a Service | 215 |
| First-page menu registration | `msix.rs` (sparse MSIX `PackageManager`) | no counterpart (§4.3) | 187 |
| Global hotkey | `RegisterHotKey` + a thread message hook | `CGEventTap` or an `NSEvent` global monitor | 158 |
| Update check | `WinHttpOpen` | `NSURLSession` or a Rust client | 133 |
| Hang and crash reports | `RtlCaptureStackBackTrace`, `SetUnhandledExceptionFilter` | `backtrace` + `NSSetUncaughtExceptionHandler` | 305 |
| Clipboard, IME caret, pickers, notifications, dir watch, taskbar, console | `windows_impl` | `NSPasteboard`, `NSTextInputClient`, `NSOpenPanel`, `UNUserNotificationCenter`, `FSEvents`, Dock tile, tty | inside the 3,449 |

`bt-app` — the ≈ 8,900 lines that consume those, and would follow them:

| Module | Lines | Why it moves |
|---|---|---|
| `webhost.rs` + `webnav.rs` + `web_thumb.rs` + `web_trace.rs` | 2,427 | the whole WebView2 conversation |
| `main.rs` platform regions (238 `bt_platform::` sites, 150 `hwnd` mentions) | ≈ 2,500 | window creation, compositor wiring, frame, clipboard, IME, taskbar |
| `first_run.rs` | 1,157 | four of its six rows name a Windows facility (§4.5) |
| `video_seat.rs` | 847 | the Media Foundation seat |
| `hang_watch.rs` | 742 | hang reporting |
| `psreadline.rs` | 594 | PowerShell-only; inert on macOS |
| `shell_integration.rs` | 505 | the `$PROFILE` probe |
| `persist.rs` | 492 | `storage_dir()` resolves `%APPDATA%` (§4.6) |
| `attention_wire.rs` | 359 | named-pipe client |
| `cli.rs` | 321 | wide argv, console attach |
| `update.rs` | 318 | download and in-place swap |
| `explorer_menu.rs` + `context_menu.rs` | 274 | the Explorer menu |
| `quake.rs` | 236 | the global hotkey (§4.2) |
| the five `*_watch.rs` + `dir_news.rs` | 618 | `DirWatch` consumers |
| `wsl.rs` | 154 | no macOS analogue |
| `notify.rs`, `diagnostics.rs` | 189 | toasts, log paths |

### 3.3 Class B, itemised

Small, and worth doing whether or not the port is ever built.

**wgpu.** The workspace pins
`wgpu = { default-features = false, features = ["std", "dx12", "wgsl"] }`.
On macOS that compiles and enumerates **no adapter at all**. Adding `metal` is
one word. The harder half is that `bt_render::WindowTarget` has exactly two
variants — `Hwnd` and `CompositionVisual` — and the second is the path the
product actually presents through, because the web panes are DirectComposition
visuals composed over the terminal's own picture. On macOS the same shape exists
(a `CAMetalLayer` as a sublayer of the window's content layer) but it is a
different call, so `WindowTarget` grows a third variant rather than losing one.

**Fonts.** `bt-render` already has a `#[cfg(not(target_os = "windows"))]`
`terminal_font_system()` — it builds an empty `FontSystem` carrying the embedded
Noto Color Emoji and nothing else. The Windows arm loads Consolas, SimSun,
Segoe UI Emoji/Symbol and the CJK chain by file name out of `%WINDIR%\Fonts`,
sets the monospace and sans families, and installs a `Fallback` with a
CJK-first script chain. The macOS arm needs the same policy written against
SF Mono / Menlo, PingFang SC, Hiragino, Apple SD Gothic Neo and Apple Color
Emoji. This is not DirectWrite work — the shaper is `cosmic-text`/`fontdb` on
both platforms and it is portable. **DirectWrite is used in exactly one place**:
`monospace_font_families()`, which fills the settings font picker.
`CTFontCollection` answers the same question.

**Bare paths.** `bt-transcript/src/paths.rs` (994 lines) recognises a printed
file path in terminal output. Its grammar is Windows: `is_windows_drive_absolute`,
`is_drive_prefix_at`, backslash separators, and the indent-chain rejoin that
stitches a path wrapped over as many as eight rows. Six of its tests fail on
macOS for exactly this reason, and one `bt-viewport` test fails on a
`D:/src/a.md` fixture. A POSIX arm — `/`-rooted, `~`-rooted, no drive letter —
is perhaps 250 lines and a second fixture set. The cross-platform version is
*stricter* than either, and that is the interesting part: on POSIX a bare
`/usr/lib` is a plausible path far more often than a Windows `C:\` is a false
positive.

---

## 4. The design questions a macOS port raises

Listed, not answered. Each is a decision, not a task.

1. **Shell integration.** zsh is the macOS default. The bash script already
   works; does zsh get its own file, or does one file grow a dialect switch?
   And what installs it — the first-run card writes to `$PROFILE` on Windows;
   the macOS equivalent is `~/.zshrc`, which is a file people guard.
2. **The quake terminal.** A global hotkey on macOS needs either a `CGEventTap`
   (which requires the **Accessibility** permission, granted by hand in System
   Settings, and revoked on every re-signing) or an `NSEvent` global monitor
   (which cannot swallow the key, so the chord also reaches the frontmost app).
   Neither is `RegisterHotKey`. Is a quake terminal that asks for Accessibility
   on first run still the same feature?
3. **The sparse-MSIX equivalent.** There is none. macOS has no first-page
   context menu to register into. The nearest things are a **Finder Sync
   extension** (a second bundle inside the app, with its own signing) and a
   **Service** (`NSServices` in `Info.plist`, which lands in the *Services*
   submenu, not the first page). Both are weaker than what Windows 11 gives.
   Does "open Folio here" survive the port, and in what form?
4. **WebView2-specific features.** Three of them do not obviously carry:
   composition hosting (a `WKWebView` is an `NSView`, and putting it *inside*
   the Metal layer tree rather than over it is a different problem),
   drag-and-drop *over* a page, and the environment/user-data-folder model that
   `forget_web_environment` exists for.
5. **The first-run card.** Four of its six rows name a Windows facility: the
   Explorer first-page menu, the context-menu verb, the PowerShell profile
   offer, and the update check's Windows path. Does the card become
   platform-conditional, or does macOS get a card of its own?
6. **Where settings live.** `bt_app::persist::storage_dir()` resolves
   `%APPDATA%\Folio\`, falling back to the temp directory. macOS convention is
   `~/Library/Application Support/Folio` — but a *terminal* is a tool whose
   users often expect `~/.config/folio` instead. One function, two conventions,
   and a migration question if it is ever changed.
7. **Font rendering.** CoreText stems and hints differently from DirectWrite,
   and Folio's shaping goes through `cosmic-text` on both — so the difference is
   not shaping but gamma and stem darkening. The contrast work in
   `bt-render/src/contrast.rs` was tuned against Windows output.
8. **Clipboard and OSC 52.** `NSPasteboard` is a different model
   (change-counted, multi-type, no owner window). macOS has no window-scoped
   clipboard, so the `clipboard_text(hwnd)` signature — which exists only
   because Win32 clipboard access is window-scoped — is Windows leaking into the
   interface.
9. **DPI and Retina.** Windows reports DPI as an integer per monitor; macOS
   reports a backing scale factor per window. `logical_px_for_dpi` and the
   monitor-id-at-point plumbing are shaped by the first model.
10. **`unsafe_code = "deny"`.** The workspace lint means every `objc2` message
    send has to live in `bt-platform`, exactly as every Win32 call does. That is
    worth stating up front, because it decides the shape of the macOS backend
    before a line is written.
11. **What `bt-winres` becomes.** The one-version-in-four-places gate has to
    grow a fifth place, or split.
12. **WSL.** `wsl.rs` has no macOS meaning. Does the profile model simply have
    fewer built-ins there, or does it grow a notion of a remote host?

---

## 5. Class D — what macOS asks for that Windows never did

Nothing below exists in the tree in any form.

- **An app bundle.** `folio.app/Contents/{MacOS,Resources,Info.plist}`.
  `bt-winres` is the Windows analogue (it turns the one version literal into a
  `VERSIONINFO` resource, and its `a_release_is_one_version_in_four_places`
  test is the gate); an `Info.plist` generator would be its sibling.
- **Developer ID signing and notarization.** `codesign` with a hardened runtime,
  `notarytool submit --wait`, `stapler staple`, and an entitlements file. This
  is a hard external dependency: it needs an Apple Developer Program membership
  and it cannot be faked. The Windows release already signs on one machine by
  hand (`docs/RELEASING.md`); the macOS lane would be the same shape.
- **A menu bar, and Cmd.** macOS expects an application menu, and every row of
  the default shortcut table has a Cmd dialect. This is the largest *design*
  item in class D, not the largest engineering one.
- **zsh integration.** `scripts/shell-integration/folio.bash` already exists —
  234 lines, OSC 133 A/B/C/D and OSC 7, with nothing Windows-specific in it.
  zsh needs `precmd`/`preexec` hooks rather than `PROMPT_COMMAND`, and that is
  the whole of the difference. Roughly 150 lines, and it is already on the 0.3
  plan.
- **DMG and a Homebrew cask.** A `.dmg` with the bundle and an `/Applications`
  alias; a cask formula pointing at the release asset with its SHA-256.
- **Retina.** macOS backing scale is 2.0 almost everywhere and 1.0 almost
  nowhere, which inverts the Windows default and exercises the fractional-scale
  paths far less. The glyph atlas and the procedural shapes are already
  scale-parametric; what is untested is the *transition* — a window dragged
  between a 2.0 built-in display and a 1.0 external one.
- **A macOS CI lane.** `ci.yml` is `windows-2025` throughout.
- **UI acceptance has no macOS equivalent.** `scripts/dev/ui-probe.ps1` is
  `SendInput` and Win32 window enumeration. The autonomous screenshot-and-keys
  acceptance loop this project depends on would have to be rebuilt against the
  macOS Accessibility API — and that itself needs the Accessibility permission
  granted to the test runner. **This is not costed below**, and it is the most
  likely thing here to be underestimated.

---

## 6. Effort

An *agent-day* below is one focused delegated session plus the user's review of
it. Assumptions: no product redesign; the Mac stays available; the class-B work
lands first; an Apple Developer membership exists before class D starts; and
**UI acceptance tooling is excluded** (see §5).

| Class | Work | Agent-days |
|---|---|---|
| **B** | two compile fixes · wgpu `metal` + a third `WindowTarget` · macOS font policy · POSIX path grammar and fixtures · gate the windows-only test modules | **4** |
| **C** | `bt-platform` macOS backend — window, screen, DPI, clipboard, dark mode, pickers, dir watch, process, console (the 3,449-line core) | 10–14 |
| **C** | compositor: a `CALayer` tree with a `CAMetalLayer` surface | 3–4 |
| **C** | the `WKWebView` host, and `bt-app`'s 2,427 lines of conversation with it | 8–10 |
| **C** | video first frame via AVFoundation | 3 |
| **C** | global hotkey and the Accessibility permission flow | 2 |
| **C** | Finder integration in place of the Explorer menu and MSIX | 3–4 |
| **C** | notifications, IME caret, attention socket, hang reports, update check | 8 |
| **C** | first-run card, storage paths, shell-integration probe | 3 |
| | **class C subtotal** | **40–48** |
| **D** | app bundle, `Info.plist`, and the version gate | 2 |
| **D** | Developer ID signing, notarization, stapling, release lane | 3–4 |
| **D** | menu bar and the Cmd dialect of the shortcut table | 4–5 |
| **D** | zsh integration | 1–2 |
| **D** | DMG and Homebrew cask | 2–3 |
| **D** | Retina and the cross-scale transition | 2 |
| **D** | macOS CI lane | 1–2 |
| | **class D subtotal** | **15–20** |
| | **Total, to a signed and notarized preview** | **59–72** |

Two waypoints inside that:

- **A window that runs — 20–25 days.** Class B, the `bt-platform` core, the
  compositor, and the paths. No web pane, no video, no Finder menu, no
  notarization; run from `target/release` rather than from a bundle. This is the
  honest first milestone and it is worth reaching before any of the rest is
  scheduled, because it converts every estimate above from a reading of the code
  into a measurement.
- **The class-B work alone — 4 days — is worth doing regardless.** It removes
  the only two hard compile errors, gives the tree a POSIX path grammar it
  arguably should have had anyway, and means every future change is checked
  against a second target instead of drifting further into Win32 by default.

### Sequencing

Where this sits against 0.3 (winget, colour schemes, bash and zsh integration)
and 0.4 (markdown editing). Three observations, and a recommendation the user is
free to overturn:

- 0.3 already contains **half a class-D item**: zsh integration. Doing it with
  macOS in mind costs nothing extra, and it is the port's only prerequisite in
  that milestone.
- 0.4 is `bt-app` and `bt-render` work — markdown editing lands in class A and
  arrives on macOS for free *if the port exists*, or has to be ported *after* if
  it does not. Porting before 0.4 therefore saves nothing and costs something:
  every new 0.4 surface would have to be born on two platforms, and this
  project's practice is to design a surface once and audit it at birth.
- The port is 59–72 agent-days. That is not a milestone increment; it is a
  milestone.

**Recommendation:** take the 4 class-B days inside 0.3 (they are cheap and they
stop the drift), ship 0.4 on Windows, and make the macOS port its own milestone
after it — with "a window that runs" as its first gate, and the decision on
whether to carry on taken there rather than now.

---

## 7. What Linux would additionally need

Short, because most of the mac port's class C is the expensive half and Linux
reuses none of it — but the *shape* it establishes, a second backend behind
`bt-platform`'s 79 entry points, is reused entirely.

- **Web pane:** WebKitGTK (`webkit2gtk-rs`) — a third engine, and the one with
  the weakest composition story of the three.
- **Windowing:** winit is currently `default-features = false`, so **neither
  `x11` nor `wayland` is enabled**. Both are one word each, but they are two
  different worlds for anything positional. Wayland in particular has no
  client-side global hotkey and no "put this window at these screen
  coordinates", which takes the quake terminal and the multi-window restore
  policy with it.
- **wgpu:** `vulkan`, and `gl` as a floor.
- **Fonts:** fontconfig through `fontdb`'s own system loader — the least work of
  the three platforms.
- **Notifications, pickers, recycle:** XDG portals over `zbus`, which are a
  runtime dependency that may simply be absent.
- **Packaging:** AppImage, Flatpak and a `.deb`; no signing, but a Flatpak
  repository to host. `portable-pty` and the shell integration need nothing.

Rough order: **+35–45 agent-days** on top of a completed macOS port, most of it
in the web pane and in Wayland's refusals.

---

## Appendix A — raw per-crate results

Environment: an Apple Silicon Mac, macOS 26.6.2 (arm64), Command Line Tools;
rustup installed into the home directory; Rust 1.94.1 `aarch64-apple-darwin`.
The repo's `rust-toolchain.toml` pins `1.94.1-x86_64-pc-windows-msvc`, which
rustup rejects as a channel name off Windows, so the run set
`RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin` and left the toolchain file
untouched. Cloned from GitHub at `05bf018`; `-j 4` throughout.

```
CHECK bt-unicode          rc=0    1s
CHECK bt-transcript       rc=0    1s
CHECK bt-doc              rc=0    6s
CHECK bt-viewport         rc=0    1s
CHECK bt-detect           rc=0    0s
CHECK bt-layout           rc=0    1s
CHECK bt-persist          rc=0    2s
CHECK bt-winres           rc=0    0s
CHECK bt-math             rc=0   29s
CHECK bt-corpus           rc=101 16s   (via bt-render)
CHECK bt-term             rc=101 10s   E0425 set_current_thread_priority
CHECK bt-render           rc=101  6s   E0599 SurfaceTargetUnsafe::CompositionVisual
CHECK bt-platform         rc=101  1s   lib ok; lib test: E0432 + E0282
CHECK bt-pty              rc=101  8s   (via bt-term)
CHECK bt-app              rc=101  7s   (via bt-render)
CHECK alacritty_terminal  rc=0     4s
CHECK portable-pty        rc=101   0s  4 example files absent from vendor/

TEST bt-unicode           ok       3 passed
TEST bt-transcript        FAILED   122 passed, 6 failed, 2 ignored
TEST bt-doc               ok       7 passed
TEST bt-viewport          FAILED   126 passed, 1 failed
TEST bt-detect            ok       114 passed
TEST bt-layout            ok       52 passed
TEST bt-persist           ok       173 passed
TEST bt-winres            ok       5 passed
TEST bt-math              ok       26 passed (150 s)
TEST alacritty_terminal   ok       188 passed
```

The two `bt-platform` test-target errors:

```
error[E0432]: unresolved imports `super::WEB_SETTINGS`, `super::WebSetting`
   --> crates/bt-platform/src/lib.rs:709:17    mod web_security_tests is #[cfg(test)],
error[E0282]: type annotations needed          not #[cfg(all(test, windows))]
   --> crates/bt-platform/src/lib.rs:734:17
```

The eight failing tests, all one cause — a path grammar that only knows Windows:

```
bt-transcript  paths::tests::a_bullet_paragraphs_hanging_indent_is_a_wrap_and_not_a_peer_row
bt-transcript  paths::tests::a_chain_is_refused_past_eight_rows
bt-transcript  paths::tests::a_chain_that_could_stop_at_a_real_directory_is_asked_about_the_file_first
bt-transcript  paths::tests::a_path_cut_into_four_indented_rows_is_one_reference
bt-transcript  paths::tests::a_path_cut_into_three_indented_rows_is_one_reference
bt-transcript  paths::tests::group_g_every_boundary_table_row_is_asked_at_both_placements
bt-viewport    tests::a_verified_printed_path_is_a_file_link_indistinguishable_from_osc_8
```

Two of the failure messages, which say what the cause is better than a summary
can — the detector returned `None` for a real, existing POSIX path:

```
left:  None
right: Some((["/var/folders/…/T/", "parent-…/scratchpad/signed", "/folio-next31.exe"], …))

panicked at crates/bt-viewport/src/lib.rs:7024:
  D:/src/a.md was already answered for, so there is nothing to ask
```

`bt-pty`'s five test-target errors with the shim applied, all of them in
windows-only test code: `os::windows` unresolved, `CONPTY_SIDECAR_VERSION`,
`Metadata::file_attributes`, `ConPtySource::Sidecar`, `ConPtySource::System`.

## Appendix B — `bt-app`'s missing platform surface

321 errors naming 79 distinct items. By error code: 168 × E0425 (name not
found), 79 × E0433 (module not found), 50 × E0282 (inference, downstream of the
first two), 9 × E0432, 8 × E0277, 5 × E0599, 2 × E0422.

Missing modules: `video`, `webview`, `attention_pipe`, `explorer_command`,
`hotkey::GlobalHotkey`, `web_mouse_buttons`.

Missing types: `Compositor`, `CustomWindowFrame`, `DirWatch`, `DirChange`,
`FolderPicker`, `ImagePicker`, `FilePickKind`, `ImeSystemCaret`,
`MathContextMenu`, `Notifier`, `SystemSettingsWatch`, `Taskbar`, `WebHost`,
`WebEvent`, `WebChord`, `WebMouseEvent`, `WebNavigationVerdict`,
`RehostOutcome`, `RehostSide`, `VideoFrame`, `AttentionPipe`, `PROGRAM_REFUSED`.

Missing functions (52): `adopt_parent_console`, `client_area_animation_enabled`,
`clipboard_text`, `detach_console`, `dpi_at`, `flash_window`,
`forget_web_environment`, `get_dpi_for_window`, `get_window_rect`,
`get_work_area`, `hide_every_window_of_this_process`,
`install_console_ctrl_handler`, `install_context_menu`,
`install_window_class_background`, `is_window_cloaked`, `is_window_minimized`,
`leave_process`, `message_box`, `monitor_id_at`, `open_local_file`,
`open_local_path`, `open_system_fonts_page`, `os_ui_language`,
`pointer_position`, `read_context_menu`, `recycle`,
`redirect_std_streams_to_file`, `remove_context_menu`, `request_window_close`,
`reveal_in_explorer`, `set_clipboard_text`, `set_current_thread_priority`,
`set_system_backdrop`, `set_window_dark_mode`, `set_window_outer_rect`,
`set_window_topmost`, `shell_execute`, `silence_std_streams`,
`spawn_at_priority`, `stand_window_at`, `system_backdrop_available`,
`system_uses_light_apps`, `take_keyboard_focus`, `taskbar_is_auto_hidden`,
`thread_mouse_capture`, `top_level_window_at`, `virtual_key_for_character`,
`virtual_screen_rect`, `wheel_scroll_amount`, `window_is_exposed`,
`work_area_at`, `write_to_console`.

Errors by file: `bt-app/src/main.rs` 177, `bt-platform/src/lib.rs` 253 (the
"configured out" notes), `bt-app/src/webhost.rs` 54,
`bt-app/src/preview_watch.rs` 11, `bt-app/src/attention_wire.rs` 11,
`bt-app/src/quake.rs` 8, `bt-app/src/cli.rs` 7, `bt-app/src/git_watch.rs` 4,
`bt-app/src/files_watch.rs` 4, and single digits in `explorer_menu.rs`,
`context_menu.rs`, `diagnostics.rs`, `git.rs` and `video_seat.rs`.

## Appendix C — method

- Line counts are **non-test, non-comment, non-blank** source lines, produced by
  a brace-depth scanner that removes `#[cfg(test)]` and
  `#[cfg(all(test, …))]` modules and resets its state per file. Folio's test
  modules are interleaved through every file and are three to four times the
  size of the code they check, so a raw `wc -l` (379,247 in `crates/`) is a
  reading of the test suite, not of the product. The workspace's non-test total
  is 187,686.
- `cargo check -p <crate> --all-targets`, then `cargo test -p <crate>`, each
  crate separately, `-j 4`.
- To see past the two dependency-wall errors and count `bt-app`'s real gap, two
  **throwaway one-line shims** were applied to the Mac's clone
  (`WindowTarget::CompositionVisual` to `unimplemented!`, and the
  `set_current_thread_priority` call to a no-op), `bt-app` was checked, and the
  clone was reverted with `git checkout --`. `git status` was clean afterwards.
  Nothing was committed there and nothing was changed in this repository.
- Everything on the Mac lives under `~/folio-port`. Disk used: **5.1 GB**
  (5.0 GB of it `target/`), plus 0.9 GB of cargo registry and 0.9 GB of
  toolchains in the home directory — **7.0 GB** in all. Nothing is running.
