# Folio on macOS — the 0.4 implementation plan

2026-09-12, revised the same day after a read-only Codex review (see the Review
record at the end). Written against `main` at `9ff64ed` and against the
measurements in `docs/plans/port/macos-spike-2026-09-07.md`, which this plan does
not redo. The spike answered *how much of Folio is already a macOS program*; this
answers *what order the rest is written in, on which machine, and what the owner
has to see with their own eyes at each stop*.

Everything here is a proposal. Nothing is dispatched until the owner has read it.

---

## 0. The ruling this plan is written under, and what it supersedes

**The owner's ruling, 2026-09-11: after 0.3 ships, 0.4 is the macOS graphical
version and 0.5 is remote. The first milestone is "opens a window and you can
type in a shell". The Apple Developer account is in place. The plan goes to a
Codex read-only review before any ticket is dispatched.**

That ruling supersedes two dated statements still standing in the tree, and this
section is the only place they are reconciled:

- `docs/plans/port/macos-spike-2026-09-07.md:11` records an earlier ruling to
  "take the class-B days now, **ship 0.4 on Windows**, and make the port its own
  milestone after it". The class-B half happened; the sequencing half is
  superseded.
- `docs/plans/remote/research-2026-09-10.md:9` says "The order is ruled — **0.4 =
  remote**, **0.5 = the macOS GUI port**." That is reversed.

**The entry condition is the 0.3 release.** No ticket in this plan is dispatched
before 0.3 is tagged and published. The one piece of shared work — a Unix rule
for `resolve_default_shell`, which is also remote T2 — is dispatched from this
plan and carries the remote requirement with it (M1-6).

---

## 1. Goal and non-goals

**0.4 ships a signed and notarized macOS preview that a stranger can download,
open with the ordinary identified-developer confirmation, and use: a native shell
in a pane, the files column, the preview pane with Markdown (including 0.3's
in-place editing), images, video and typeset math, several windows that come back
where they were, and the attention channel that says a background agent
finished.** Out of 0.4: Linux (the spike's §7 costs it separately at +35–45
agent-days); the 0.5 remote split, beyond the rule in §4.6 that keeps it
possible; the games; any MSIX equivalent, because macOS has no first-page context
menu to register into; a universal binary; a Homebrew cask; in-place
self-update; and a macOS twin of `scripts/dev/ui-probe.ps1`, which the spike's §5
excludes from its numbers and which this plan excludes too, with the same warning
that it is the likeliest thing here to be underestimated later.

**On the locked dependencies.** `Cargo.lock` pins **winit 0.30.13**, **wgpu
30.0.0** and **wgpu-hal 30.0.0**. Their sources contain the AppKit backend, IME
facilities, `OptionAsAlt`, custom-delegate guidance and the Metal surface
backend. That establishes that **support exists; it does not establish
correctness on macOS 26.6.** Nothing in this plan claims that this combination
has passed Folio's IME, composition or lifecycle acceptance on that OS, and the
probe phase in §3 exists precisely to find out.

---

## 2. Preflight and milestones

Preflight is administrative and is **not** a product milestone. The first product
milestone is the owner's.

### P — preflight (administrative)

Xcode selected and licensed; a Developer ID Application certificate usable by
`codesign` in the execution context releases will actually run in; the bundle
identifier, signing team, and minimum supported macOS version fixed. §5 says what
of this only the owner can do.

> **Gate (not an acceptance line).** On the Mac mini, `codesign` signs a
> throwaway bundle with the real Developer ID identity from the same kind of
> session a release will use, and `codesign -dv --verbose=4` reads the identity
> back. This is P-3, and it is here rather than in M5 because every permission
> Folio asks for is granted against a signing identity (§3, X-5).

### M1 — a fresh, Finder-launched Folio opens a window and a native shell

The owner's named first gate, stated the way they stated it. A winit window; wgpu
presenting through Metal; **the whole existing startup path**, which today
requires an `HWND`, a custom frame, a compositor and four Win32 bridges before it
reaches a frame; a native shipped profile that resolves to a real shell; the
keyboard with its Command/Control routing settled; IME good enough to type
Chinese; clipboard.

> **Acceptance.** With `~/Library/Application Support/Folio` deleted first, and
> launched from Finder (not from a terminal): a window opens with a `zsh` prompt
> in it — `echo $0` answers `zsh` and `pwd` answers the home directory. Type
> `ls`, Enter, the directory lists. Select the word `Documents` in that output
> with the mouse, `Cmd+C`, then `Cmd+V` into the same pane and it arrives as
> text. Run `sleep 30` and `Ctrl+C` interrupts it — Command copies, Control still
> reaches the child. Switch to the system Pinyin source, type `nihao`, pick 你好
> from the candidate window, and 你好 appears in the pane. `Cmd+T` opens a second
> tab, `Cmd+W` closes it.

### M2 — the reading surfaces

The files column and all three of its watch contracts; the preview pane over the
same `read_head` worker it uses on Windows; Markdown including 0.3's editing;
images; typeset math; the glyph output measured rather than assumed.

> **Acceptance.** With a clean data directory: open a folder in the files column;
> click a `.md` file with a table and a CJK paragraph and the preview renders it;
> change a heading in place, press the save chord, and `md5 <file>` in a pane
> shows bytes that changed; then replace that file from a pane with
> `printf '# other\n' > new && mv new <file>` and the preview updates without a
> click; create a file in a *subdirectory* of the open folder from a pane and the
> tree shows it; click a `.png` and it shows; open a file containing
> `$$\int_0^1 x\,dx$$` and the integral is typeset, not printed as source.

### M3 — chrome, the application lifecycle, several windows, persistence

The custom window frame against macOS's traffic lights; the application menu bar;
**the AppKit application delegate** — Finder and Dock reopen, Services delivery,
termination, last window closed — which is a different thing from the launch
socket and is not covered by it; multi-window restore; the storage directory;
one data directory, one writer.

> **Acceptance.** Open two windows with different tabs, move one, change a
> setting, `Cmd+Q`, relaunch from Finder: both windows return with their tabs,
> their geometry and the changed setting. Close every window and the app stays in
> the Dock; click the Dock icon and a window appears. With Folio running, launch
> it again from Finder — a window opens in the running process and no second Dock
> icon appears. Start a second copy from a terminal with a *different*
> `--data-dir`-equivalent and it runs independently; start one with the same data
> directory and it hands over rather than writing. `ls ~/Library/Application\
> Support/Folio` lists `session.json` and `settings.json`.

### M4 — the platform features that need rewriting

Notifications; the web preview; video first frame and playback with sound; the
global hotkey and its authorization; Services; the attention socket; crash
reporting; the update check.

**In 0.4:** all of the above. **Deferred out of 0.4:** a Finder Sync extension
(§8, Q3); the sparse-MSIX equivalent, which does not exist; in-place
self-update, which becomes "open the release page"; `wsl.rs`, `psreadline.rs`,
`msix.rs` and `explorer_command.rs`, which are Windows facts and are absent.

> **Acceptance.** Each feature has its own procedure, because "a long command"
> is not one. ① In a background tab with shell integration active, run
> `sleep 5; printf '\a'` and switch away: a Folio notification arrives, and
> clicking it raises that tab. With the tab in view and the window focused, the
> same command produces no notification. ② Open `tests/assets/folio-pdf-test.html`
> from the files column: it renders; the X-2 fixture set is re-run against it and
> every row matches its recorded verdict. ③ Hover `folio-video-test.mp4`: the
> first frame appears. Open it: it plays **with audible sound**, pauses, seeks,
> and ends without a stuck frame. ④ With the shortcut disabled, the settings row
> reads *not authorized*; use the in-app *Enable global shortcut* action, grant
> Accessibility, and the chord then pulls a terminal down over a frontmost
> TextEdit and returns the foreground to TextEdit on retract. ⑤ Right-click a
> folder in Finder: *Services ▸ Open in Folio* opens a tab in that folder,
> including for a folder whose name contains a space and a CJK character. ⑥ Kill
> Folio with `kill -ABRT`; the next launch finds the system crash report and
> names it in `diagnostics.log`. ⑦ The update check reports the current release
> from a pane-visible settings row.

### M5 — signing, notarization, the DMG, and the release lane

> **Acceptance.** In order, on the Mac mini: `codesign --verify --deep
> --strict --verbose=2 Folio.app` passes; `spctl -a -vvv Folio.app` says
> `accepted` with `source=Notarized Developer ID`; `spctl -a -vvv -t open
> --context context:primary-signature Folio.dmg` says `accepted`; `xcrun stapler
> validate` passes on **both** `Folio.app` and `Folio.dmg`. The notarization log
> for each submission is retained beside the artifact.

### M6 — clean-user acceptance

A second macOS account, the DMG fetched over the network rather than copied.
**This is clean-*user* acceptance, not clean-machine**: a second account shares
the OS, machine-wide installations and the system's trust history, so it cannot
establish that a defect masked by a development install is absent.

> **Acceptance.** Logged into a second account: download the DMG in Safari, open
> it, drag Folio to Applications, launch it. The ordinary identified-developer
> confirmation is **expected and acceptable**; what must not appear is an
> unidentified-developer warning, a notarization failure, or "damaged and can't
> be opened". Then re-run the M1, M2, M3 and M4 acceptance lines from that
> account against the downloaded build, including the refusal paths: deny
> Notifications and confirm the settings row says so rather than silently doing
> nothing; deny Accessibility and confirm the shortcut row says *not authorized*.
> Disconnect the network and launch once to confirm the stapled ticket is used.

---

## 3. The high-risk contracts, probed before they are implemented

The review's central point, and it is correct: *a mostly portable Rust
implementation does not imply a mostly interchangeable native GUI contract.* Six
probe tickets run between preflight and M1. Each has a pass/fail an agent can
report without judgement, and each retires a design decision that would otherwise
be discovered in M4.

**X-1 — Metal alpha and composition, against the locked wgpu.**
The blocker. `bt-render`'s `required_alpha_mode` (`crates/bt-render/src/lib.rs`)
demands `Opaque` for `WindowTargetKind::Hwnd` and **`PreMultiplied`** for
`CompositionVisual`, and `choose_alpha_mode` refuses a surface that is not
offered what it requires. The cached **wgpu-hal 30.0.0 Metal backend advertises
only `Opaque` and `PostMultiplied`** (`src/metal/adapter.rs:468`) and sets the
layer non-opaque only for `PostMultiplied` (`src/metal/surface.rs:231`).
**`PreMultiplied` does not exist on Metal in this version.** So the Windows
composition contract cannot be carried over, and the plan's earlier "premultiplied
holes" sentence was wrong.
*Probe:* build Folio's own renderer against a `CAMetalLayer` configured
`PostMultiplied`, draw the real frame with a hole where a web pane would be, put
a real `WKWebView` behind it, and check the hole, the partially transparent
edges, a resize, and a device-loss reconstruction.
*Pass:* the page is visible through the hole with correct edge alpha through all
four. *Fail:* anything else — and then alpha representation and surface ownership
become explicit design work before M1-4 is scoped.

**X-2 — WKWebView policy enforcement.**
`webview.rs` enforces the navigation policy through `WebResourceRequested` with a
filter over every context (`crates/bt-platform/src/webview.rs:1831`), which is
broader than top-level navigation. WKWebView's `WKURLSchemeHandler` applies only
to schemes WebKit does not already handle and is not an equivalent hook for
arbitrary HTTP(S) interception. `webnav.rs`'s decision function is pure and
portable, but reusing it proves nothing about where it can be *called*.
*Probe:* a requirement-to-public-API matrix and an adversarial fixture set —
subresources, frames, redirects, local files, downloads, popups — run against a
real `WKWebView`.
*Pass:* every requirement maps to a public API, or is listed as an unsupported
guarantee the owner has seen (§8, Q5). *Fail:* an unmapped requirement nobody has
decided about.

**X-3 — Command, Option and IME routing.**
Clipboard is not in `BINDINGS`: `is_copy_shortcut` and `is_paste_shortcut` are
independent Control predicates in `crates/bt-app/src/input.rs`. `WebChord`
(`bt-platform/src/webview.rs:107`) carries `virtual_key, ctrl, shift, alt` and
**no Command field**, and `webhost.rs:556` copies only those three — so a Command
shortcut over a focused page degrades to an unmodified key. winit 0.30.13 exposes
`OptionAsAlt` including left/right.
*Probe:* a matrix over shell, Markdown editor, palette and web focus: Command
shortcuts, Control to the child, Option as Alt versus Option as text, dead keys,
a non-US layout, preedit position at backing scale 2, preedit cancellation, and
no duplicate commits. `set_ime_allowed(true)` is already called at both window
constructors and stays.
*Pass:* a written routing rule that covers every cell. *Fail:* a cell with two
plausible answers — which is a product decision (§8, Q9), not a bug.

**X-4 — the AppKit application delegate.**
A second Finder launch of an already-running app does not start a second
executable; it delivers an application **reopen** event. winit 0.30.13's own
macOS documentation says this is not directly exposed and recommends a custom
delegate. So the Unix launch socket can be perfect and Finder will still only
activate the running app and never open the requested window.
*Probe:* a delegate bridge in `bt-platform` handling reopen, Services delivery,
termination, last-window-closed, and hidden/minimized windows, exercised from
Finder and the Dock.
*Pass:* each event reaches a named application action with an explicit origin.
*Fail:* an event that cannot be routed on the main thread without blocking.

**X-5 — a stable signed identity before anything touches TCC.**
Accessibility and Notifications are granted against a code signature's designated
requirement. An agent iterating on the hotkey re-signs on every build; if each
build is a new grant, the loop is unusable and every acceptance result is
suspect.
*Probe:* sign the same bundle twice with the real Developer ID and confirm the
grant survives; then confirm an ad-hoc signature does not.
*Pass:* stable identity retains the grant. *Fail:* it does not, and MAC-hotkey
work needs a different development procedure. Depends on P-3.

**X-6 — what a Windows agent can actually check.**
`cargo check -p bt-platform --target aarch64-apple-darwin` passing today proves
almost nothing: `bt-platform` has **no** non-Windows dependencies
(`crates/bt-platform/Cargo.toml:9` opens the Windows-only table), so the check
compiles an empty crate. The planned backend adds objc2 and its neighbours, and
the app adds winit, Metal and the rest.
*Probe:* a scratch crate with the representative objc2 dependency graph, plus
`cargo check --target aarch64-apple-darwin --all-targets` of a real app target,
from Windows, with `.cargo/config.toml`'s `+crt-static` in force.
*Pass:* both check. *Fail:* record the actual error before blaming `crt-static`;
either way, **`check` is an authoring and compile venue, never acceptance** —
clipboard, stdio, linking and every native behaviour are accepted on the Mac.

---

## 4. Architecture rules the tickets inherit

### 4.1 The standing rule does not change

**Platform code lives behind `bt-platform`'s interface; no crate below `bt-app`
calls the platform directly** (`docs/DESIGN.md` §13.1). The workspace's
`unsafe_code = "deny"` exempts `bt-platform`, which is why every Win32 call was
already behind that door, and the same sentence now binds `objc2`.

One correction the review is right about: **`bt-render` already has a narrow,
documented `unsafe` exception** for surface creation
(`crates/bt-render/src/lib.rs`, "The one `unsafe` in this crate, and why it is
here rather than in `bt-platform`"). The rule is "one exception, written down",
not "none". Native video FFI still belongs in `bt-platform`, and that is a
placement decision rather than a consequence of the lint.

### 4.2 The gate learns a second platform

`scripts/check-portable-core.ps1` refuses Win32 spellings in the thirteen
portable crates outside a `#[cfg(windows)]`. It grows `objc2`, `objc2_*`,
`core_foundation`, `core_graphics` and `core_text`, gated on
`#[cfg(target_os = "macos")]` by the same brace-depth walk. `std::os::unix` is
deliberately not added: `bt-pty` is a unix crate by construction.

### 4.3 `bt-app`'s platform gates are a named list, and the list is wrong today

`bt-app` carries a platform `cfg` in eleven files — `psreadline.rs`,
`attention_copilot.rs`, `files.rs`, `explorer_menu.rs`, `wsl.rs`, `update.rs`,
`shell_integration.rs`, `settings.rs`, `palette_index.rs`, `git_panel.rs`,
`main.rs` — and names `bt_platform::` at **519 occurrences across 36 files**, 249
of them in `main.rs`, with no gate at all. That ratio is the design.

**`cli.rs` is not on that list and has two ungated `std::os::windows` uses** —
one in production at `cli.rs:445` and one in a test at `cli.rs:1240` — which
`check-portable-core.ps1` never sees, because it scans the thirteen crates and
`bt-app` is not one of them. M1-10 either admits `cli.rs` to the list or gives
`value_for` a portable implementation; the recommendation is the second, because
what that function does is read a non-UTF-8 argument, and a lossy-preserving
`OsString` route exists on both platforms.

The modules `bt-app` names must exist everywhere. `#[cfg(windows)] pub mod
webview` and its siblings (`video`, `attention_pipe`, `launch_pipe`,
`explorer_command`, `http`) become plain `pub mod` declarations whose bodies are
`#[cfg(windows)] mod win`, `#[cfg(target_os = "macos")] mod mac` and a
`not(any(...))` third body, re-exporting one set of names.

### 4.4 Refuse at runtime — with named exceptions, and they are type-level

**Recommendation: an item is present on every platform and refuses when
invoked.** Precedent exists: `set_current_thread_priority`'s portable arm
answering `false`, **four** `#[cfg(not(windows))]` arms in `hang.rs` and **two**
in `hotkey.rs`. It keeps `bt-app` free of gates and lets a refusal carry a reason
to a toast, which an absence cannot.

The rule is not mechanical, and M1-2 exists to say where it breaks. Four kinds of
exception, each verified in the tree:

1. **Compile-time absence, correctly, today.** `msix::explorer_command_clsid()`
   returns `windows::core::GUID` behind a Windows gate; `DataDirectoryClaim`
   holds a Windows `HANDLE` in its gated definition and is a unit struct in its
   non-Windows one. **Lifting these mechanically would create the leak the gate
   exists to stop.** SDK types stay inside backend definitions.
2. **Signatures that encode Windows without naming it.** `WebChord` has a Win32
   virtual key and no Command modifier; `RehostSide` carries an `hwnd:
   NonZeroIsize`. These have to change shape, not gain an arm.
3. **Return types with no empty answer.** `leave_process(code: i32) -> !` cannot
   refuse; it must really terminate. `redirect_std_streams_to_file` and
   `silence_std_streams` are load-bearing for resident diagnostics
   (`crates/bt-app/src/diagnostics.rs`) and cannot be no-ops either.
4. **A refusing arm that is wrong.** `instance::claim_data_directory` off Windows
   returns **`Some(DataDirectoryClaim)`** — it always succeeds, with the comment
   "a machine with no kernel to ask always answers 'you are the one writer'".
   So **the single-writer guarantee is absent by construction off Windows**, and
   M3-5 must supply it rather than preserve it. (The first draft of this plan and
   the review both said this arm returns `None`. It does not; both were wrong,
   and the consequence runs the other way.)

`wsl.rs`, `psreadline.rs`, `msix.rs` and `explorer_command.rs` are absent rather
than refusing, and §4.3's eleven files already contain their call sites. The
first draft said `explorer_command` was both portable and absent; absent is the
answer.

### 4.5 Paths, the bundle, and the version gate

`bt_app::persist::storage_dir()` (`persist.rs:1168`) gets a macOS arm —
`~/Library/Application Support/Folio`. **The `player` cache is not migrated**:
`main.rs` documents that nothing writes `%LOCALAPPDATA%\Folio\player` and an
adjacent test refuses the resurrection of `player.rs`. WKWebView persistence is
`WKWebsiteDataStore`'s own question and is specified in M4-2, not by moving a
directory. Key names inside `settings.json` do not change; one schema, one
migration chain, no `_mac` suffixes.

The bundle lives at `packaging/macos/`: `Info.plist.in`, `Folio.entitlements`,
the DMG staging. **The bundle is built from M1, not M5**, because
`UNUserNotificationCenter` refuses a process with no bundle identifier,
`WKWebsiteDataStore` keys on one, and `NSServices` is read out of `Info.plist`.
M5 signs and ships what M1 already lays out.

The version gate is **`bt-app/src/version.rs:96`,
`the_version_is_the_manifests_and_nothing_elses`** — not a `bt-winres` test, and
not named "four places". M5-5 extends that gate to the generated plist's
`CFBundleShortVersionString` *and* `CFBundleVersion`.

### 4.6 What this plan actually guarantees the remote server

Putting objc2 under `[target.'cfg(target_os = "macos")'.dependencies]` keeps a
**Linux** server free of them. It does **not** keep a *macOS* headless server
free of a GUI backend, and the remote research contemplates macOS servers. So the
rule is stated at its real strength, plus one more:

- **Unconditional:** the server never depends on `bt-app` or `bt-render`.
- **Target-scoped:** macOS GUI dependencies are declared only for macOS targets,
  so Linux links a `bt-platform` with no dependencies at all.
- **New:** if a macOS headless server must also be an empty shell, the GUI
  backend goes behind a cargo feature, and CI grows a headless dependency check
  for both Unix targets. Scheduling primitives must not require AppKit
  initialization. §8 does not ask about this; it is a rule, and 0.5 may relax it.

### 4.7 The documents get twins, not forks

`docs/shortcuts.md` is generated by
`bt_app::shortcuts::tests::docs_shortcuts_md_is_the_bindings_table`, which walks
`BINDINGS` in both languages. M1-7 adds a platform dialect to that walk. **It is
not sufficient on its own**: the clipboard predicates in `input.rs` and the web
chord conversion in `webhost.rs` are outside `BINDINGS` and are part of the same
ticket. `README.md` and `README.zh-CN.md` grow a macOS section in lockstep;
`docs/RELEASING.md` and `docs/BUILDING.md` grow macOS sections; new fixtures
arrive with `PROVENANCE.md` entries.

---

## 5. What the owner must do personally

**① Point the developer tools at Xcode.** Xcode 26.6 is installed at
`/Applications/Xcode.app` (measured), but `xcode-select -p` answers
`/Library/Developer/CommandLineTools`, so `xcodebuild` refuses.
`sudo xcode-select -s /Applications/Xcode.app/Contents/Developer` and
`sudo xcodebuild -license accept`.
*Verify:* `xcode-select -p` prints the Xcode path; `xcodebuild -version` prints
`Xcode 26.6`.

**② Create the Developer ID Application certificate.** Xcode ▸ Settings ▸
Accounts, sign in, select the team, Manage Certificates ▸ **+** ▸ Developer ID
Application.
*Verify:* `security find-identity -v -p codesigning` names a `Developer ID
Application: … (TEAMID)` line and ends `1 valid identities found`. It says
`0 valid identities found` today.

**③ Decide where releases are signed, and how the private key is reached.**
This is a decision, not a command, and the first draft of this plan got it
wrong. **An App Store Connect API key authenticates notarization; it does not
sign.** `codesign` needs the private key in a keychain it can open, and measured
today, `xcrun notarytool history --keychain-profile folio` over a
non-interactive ssh session answered
`Error: keychainLocked(keychainName: "default")` — an ssh session does not have
the login keychain open, and the same is true for `codesign`'s access to the
key. Three routes, and §8 Q7 asks the owner to pick one:
a dedicated non-login signing keychain unlocked for the duration of a release by
a password the owner supplies interactively; the owner running the signing step
at the machine; or a CI runner with the identity installed. Whichever is chosen,
the notarization credential should still be an API key file
(`~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8`, mode `600`, with the key
and issuer ids beside it), because that half genuinely works headlessly.
*Verify, both halves:*

```
codesign -s "Developer ID Application: … (TEAMID)" -o runtime --timestamp /tmp/Probe.app
codesign -dv --verbose=4 /tmp/Probe.app
xcrun notarytool history --key ~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8 \
  --key-id <KEYID> --issuer <ISSUER-UUID>
```
Notarization history returning an empty list is a pass for that half and **is
not a release rehearsal**; M5-2 is. Nothing of this goes in the repository, and
`packaging/macos/` gets a `.gitignore` for `*.p8` on the way past —
`scripts/check-machine-paths.ps1` would not catch a leaked key.

**④ Grant the two permissions at the machine, when asked.** Notifications at the
first toast, Accessibility through the in-app *Enable global shortcut* action.
Neither can be granted over ssh.

**⑤ Decide the disk and the build budget.** `/` has **48 GiB free** and
`~/folio-port` already holds **13 GiB**. A release `target/` was 5.0 GiB on that
machine. Recommend resetting the spike checkout at P-1 and sharing one
`CARGO_TARGET_DIR` under `~/folio-port/target`. "One cargo at a time" is not a
complete resource budget: the Mac mini also runs the alpha production daemon, so
every launcher carries `nice`, a job count that leaves the machine a fifth of
itself, and a check that the daemon is untouched.

Optional but worth a minute: **a second display**. The Mac mini drives a single
DELL S2725QS at 3840×2160 presenting as 1920×1080 — backing scale exactly 2.0,
everywhere. A scaled resolution is **not** proof of a different backing scale;
the mixed-display transition cannot be exercised here at all (§8, Q11).

---

## 6. Risks, and what retires each

The four largest are now probe tickets (§3, X-1 to X-4) rather than list entries,
because a risk with an owner and a pass/fail is a ticket. What remains:

**R1 — `cargo check --target` from Windows may prove nothing, or may not run.**
Retired by X-6. The failure mode to avoid is attributing a failure to
`.cargo/config.toml`'s `+crt-static` before reading the actual error.

**R2 — the surface inventory is stale and always will be.** The spike's *79
missing items* and its 3,449 / 2,427 / 8,900 line counts are **historical
measurements taken at `05bf018`**, not current sizes, and this plan labels them
that way wherever it quotes them. M1-2 re-measures before M1-1 is scoped.

**R3 — real Unix child I/O has never run.** The first draft said `core-linux`
tests `bt-pty` "on a real pty". The job's own comment says the opposite:
"**nothing here spawns a child yet**, because off Windows this crate has no
default shell to spawn" (`.github/workflows/ci.yml`). So spawn, resize, exit and
reap over a real Unix pty are **new validation**, not inherited coverage. M1-6
owns it and it is the ticket that makes that CI line start a process.

**R4 — the glyph question is not CoreText versus DirectWrite.**
`crates/bt-render/src/contrast.rs` is a **colour-contrast policy** module —
minimum ratios against the paper — and has nothing to do with rasterization.
Folio rasterizes through Swash via glyphon on both platforms, so the rasterizer
does not change at all; what changes is the Metal presentation path around it.
M2-5 measures the actual output rather than assuming a substitution that does
not happen.

**R5 — `directory_tag` folds identity the Windows way.**
`instance::directory_tag` lowercases a lossy path string, which is correct on
NTFS and wrong on a case-sensitive APFS volume, and says nothing about symlinks.
Combined with §4.4 ④ — the claim always succeeds off Windows — M3-5 has to
supply canonical directory identity, a private runtime directory, peer
verification (DESIGN §7.59b already requires the client to verify the server's
image before sending its command line), socket-path length limits, stale-endpoint
cleanup under the ownership lock, and crash recovery. The `Decision` /
`Admission` / client-`CONFIRM`-is-the-commit-point semantics are preserved; the
security model differences are documented rather than hidden.

**R6 — the attention endpoint's boundary changes meaning.**
`attention_pipe`'s DACL names the **logon session**, deliberately, "so a second
session of the same user (a service, another desktop) is outside it". Unix owner
permissions identify a **user**, not a session. M4-7 states that difference as a
decision rather than substituting `0600` and calling it equivalent.

**R7 — the crash ticket cannot replace APIs that are not there.**
`hang.rs` says in its own header that `RtlCaptureStackBackTrace` "is not
available here" and does not use it, and **`SetUnhandledExceptionFilter` appears
nowhere in the tree**. `bt-app` already installs a Rust panic hook
(`main.rs:107020`). `NSSetUncaughtExceptionHandler` catches Objective-C
exceptions and is not a general crash mechanism. M4-11 therefore preserves the
panic hook, implements a real event-loop liveness handshake for the hang half,
and collects and symbolicates the system's own crash reports — it does not
"replace" two functions.

**R8 — Accessibility grants and re-signing.** Retired by X-5, which is
positioned before any TCC-dependent ticket rather than beside it.

**R9 — the toolchain pin off Windows.** `rust-toolchain.toml` names
`1.94.1-x86_64-pc-windows-msvc`, which rustup rejects as a channel off Windows.
Solved in CI by `.github/actions/toolchain`; on the Mac mini every launcher
exports `RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin`, which is installed there
(measured, beside a default `stable` of 1.98.1), and the toml is untouched.

---

## 7. The tickets, the graph, and the effort

### 7.1 The inventory

**52 tickets — 10 S, 33 M, 9 L.** `check` means a Windows agent can author and
compile it; **Mac** means the Mac mini, in a worktree under
`~/folio-port/wt/<ticket>`, one cargo at a time. Per X-6, `check` is never
acceptance.

| ID | Title | Size | Where | Depends on |
|---|---|---|---|---|
| **P-1** | Xcode select + license; reset the spike checkout; shared target dir | S | Mac | owner §5 ① |
| **P-2** | Bundle identifier, team, minimum macOS version, deployment target, `packaging/macos/` skeleton | S | check | §8 Q4, Q6 |
| **P-3** | A minimal real signing script, exercised in the execution context releases will use | M | Mac | owner §5 ②③ |
| **X-1** | Metal alpha + composition against locked wgpu and a real WKWebView | M | Mac | P-1 |
| **X-2** | WKWebView policy matrix + adversarial fixtures | M | Mac | P-1 |
| **X-3** | Command / Option-as-Alt / IME routing matrix across four focus surfaces | M | Mac | P-1 |
| **X-4** | AppKit application delegate bridge: reopen, Services, termination, last window | M | Mac | P-1 |
| **X-5** | Stable signed identity and TCC grant retention | S | Mac | P-3 |
| **X-6** | What a Windows agent can check: objc2 graph + an app target, `--all-targets` | S | check | P-2 |
| **M1-1** | Native handle abstraction; both window constructors; device-loss reconstruction; deferred-service construction made harmless | L | Mac | M1-2, X-1 |
| **M1-2** | The backend inventory: signatures, fields, ownership/thread, caller failure, and the five classifications of §4.4 | M | check | X-6 |
| **M1-3** | Window and screen backend: geometry, work area, backing scale, exposure, topmost, dark mode | L | Mac | M1-2 |
| **M1-4** | The Metal surface and a `WindowTargetKind` arm with its own alpha policy | M | Mac | X-1, M1-1 |
| **M1-5** | Native shipped profiles, automatic selection, fallback, home/cwd, Finder-launch environment, zsh integration | L | Mac | M1-6 |
| **M1-6** | `resolve_default_shell` Unix rule; PowerShell arguments made Windows-specific; real Unix child spawn/resize/exit/reap tests — **also remote T2** | M | Mac | M1-2 |
| **M1-7** | Keyboard routing per X-3: application-command vs terminal-control, Option-as-Alt, the `input.rs` predicates, `WebChord` gaining Command, the `BINDINGS` dialect | L | Mac | X-3, M1-3 |
| **M1-8** | IME: preedit, cursor area at scale 2, cancellation, focus changes, no duplicate commits | M | Mac | M1-7 |
| **M1-9** | Clipboard over `NSPasteboard`; drop `clipboard_text`'s `hwnd` parameter | S | check | M1-2 |
| **M1-10** | `--all-targets` on macOS; the Windows-assuming runtime tests; the `cli.rs` gap; `core-macos` grows `bt-platform` and `bt-app`; the `bt-app` cfg allowlist gate | M | check | M1-1 |
| **M2-1** | `DirWatch` over FSEvents preserving **all three** contracts — `Tree`, `HereOnly`, named `HereOnly` — with overflow/rescan, rename, root replacement, readiness, cancellation | L | Mac | M1-1 |
| **M2-2** | Process door over `NSWorkspace`; trash over `NSFileManager`. `quiet_command` is **already portable** and is only re-verified | M | Mac | M1-2 |
| **M2-3** | Pickers over `NSOpenPanel`; `message_box` over `NSAlert` | M | Mac | M1-3 |
| **M2-4** | `monospace_font_families()` over `CTFontCollection` | S | Mac | M1-2 |
| **M2-5** | Glyph output measured on the Metal path at scale 2 | M | Mac | M1-4 |
| **M2-6** | `storage_dir()` on macOS; `WKWebsiteDataStore` persistence specified | S | check | M1-2 |
| **M2-7** | Reading-surface acceptance sweep to M2's line | M | Mac | M2-1…M2-6 |
| **M3-1** | The application delegate productionized from X-4 | L | Mac | X-4, M1-1 |
| **M3-2** | The application menu bar | M | Mac | M1-7 |
| **M3-3** | `CustomWindowFrame` against the traffic lights | M | Mac | M1-3 |
| **M3-4** | Multi-window restore and monitor identity | M | Mac | M2-6, M3-3 |
| **M3-5** | One data directory one writer, **built rather than preserved** (§4.4 ④): canonical identity, runtime dir, peer verification, stale cleanup, crash recovery, launch socket | L | Mac | M3-1, M2-6 |
| **M3-6** | First-run card: capability-driven row visibility; the portable update row stays | M | Mac | M3-2 |
| **M3-7** | stdio for terminal and Finder launches; real redirect/silence; `leave_process` really terminates | M | Mac | M1-2 |
| **M4-1** | The `CALayer` composition implementation from X-1 | M | Mac | X-1, M1-4 |
| **M4-2** | The `WKWebView` host and `bt-app`'s conversation with it; `WKWebsiteDataStore` lifecycle | L | Mac | M4-1, X-2 |
| **M4-3** | Web policy enforced to X-2's matrix; unsupported guarantees stated in the product | M | Mac | M4-2 |
| **M4-4** | Video first frame over `AVAssetImageGenerator` | M | Mac | M1-4 |
| **M4-5** | Video playback: `AVPlayer` ownership, **audio**, timing, seek, pause/end/error, frame format, stride and colour conversion | L | Mac | M4-4 |
| **M4-6** | Notifications over `UNUserNotificationCenter`; Dock tile | M | Mac | M3-1 |
| **M4-7** | Attention endpoint over a Unix socket, with the session-vs-user boundary stated | M | Mac | M3-5 |
| **M4-8** | Global hotkey over `CGEventTap`; in-app authorization action; denied/revoked state; foreground restoration tested | M | Mac | X-5, M3-1 |
| **M4-9** | Services provider object, callback and main-thread bridge; folder URL validation, multi-selection, spaces and non-ASCII, cold and warm delivery | M | Mac | M3-1, M3-5 |
| **M4-10** | Update check over `NSURLSession`; the in-place swap becomes "open the release page" | S | Mac | M2-2 |
| **M4-11** | Event-loop liveness handshake; panic hook preserved; system crash reports collected and symbolicated | M | Mac | M3-7 |
| **M5-1** | Minimal entitlements, hardened runtime, secure timestamps, nested-code signing order, signature verification | M | Mac | P-3, M4 complete |
| **M5-2** | Notarize and staple the app; retain submission logs | M | Mac | M5-1 |
| **M5-3** | Build, sign, notarize and staple the DMG | M | Mac | M5-2 |
| **M5-4** | `release.yml` macOS lane | M | check | M5-3 |
| **M5-5** | `the_version_is_the_manifests_and_nothing_elses` grows the plist's two version fields | S | check | P-2 |
| **M5-6** | `README` ×2, `docs/shortcuts.md`, `BUILDING`, `RELEASING`, `PROVENANCE` | M | check | M5-4, M3-2 |
| **M6-1** | Clean-user acceptance of the downloaded artifact, including refusal paths and the offline ticket | M | Mac | M5-4 |
| **M6-2** | Clean-machine coverage: a VM or snapshot, or the gap written down and accepted | S | Mac | M6-1, §8 Q11 |

### 7.2 The graph

Three kinds of edge the first draft did not have, and the review was right that
without them the graph permits shipping an incomplete product:

- **Milestone-completion joins.** `M2-7` joins all of M2; `M5-1` joins all of M4;
  `M6-1` joins all of M5. An acceptance ticket that re-checks an earlier
  milestone depends on that milestone's join, not on nothing.
- **Release-readiness edges.** **M4-2 gates the release**, because the web
  preview is in 0.4 scope; so does M4-5, M4-8 and M4-9. They are not optional
  leaves.
- **Probe-before-implementation edges.** X-1 gates M1-4 and M4-1; X-2 gates
  M4-3; X-3 gates M1-7; X-4 gates M3-1; **X-5 gates M4-8**.

**The dependency critical path** is
P-1 → P-3 → X-5 → X-1 → M1-2 → M1-1 → M1-3 → M1-4 → M1-5 → M1-7 → M3-1 →
M3-2 → M4-2 → M5-1 → M5-2 → M5-3 → M6-1, which sums to **44–65 agent-days**.

**The single-Mac schedule is a different number.** Forty-three of the fifty-two
tickets need the Mac and they cannot overlap. The nine `check` tickets can run on
Windows agents in parallel with Mac work, which shortens elapsed time but removes
no agent-days. The elapsed schedule is therefore bounded below by the Mac
tickets' own sum, not by the critical path.

### 7.3 Effort, bottom-up, and reconciled with the spike

| Phase | Tickets | Agent-days |
|---|---:|---:|
| P preflight | 3 | 4–5 |
| X probes | 6 | 10–14 |
| M1 window and shell | 10 | 27–40 |
| M2 reading surfaces | 7 | 14–20 |
| M3 chrome, lifecycle, persistence | 7 | 18–27 |
| M4 rewritten features | 11 | 25–37 |
| M5 sign, notarize, ship | 6 | 11–16 |
| M6 clean-user acceptance | 2 | 3–4 |
| **Total** | **52** | **112–163** |

**This is roughly twice the spike's remaining 55–68, and the difference is not a
disagreement about the code.** Four things account for it, each checkable:

- **The spike costed the platform backend, not the application integration.** Its
  §3.2 lists `bt-app`'s ≈8,900 platform-facing lines and says they "would follow"
  the backend; it does not cost them separately. M1-1, M1-5, M1-7, M4-2's app
  half and M3-1 are that work, and they are the largest block here.
- **The spike had no probe phase**, and no ticket for an inventory. Ten to
  fourteen days buy back the M4 redesign the review identifies as the dominant
  schedule risk.
- **Two spike rows need restating rather than re-estimating.** Its whole-core
  10–14 covered *all* of `windows_impl` — window, screen, DPI, clipboard, dark
  mode, pickers, dir watch, process, console — which this plan spreads across
  M1-3, M1-9, M2-1, M2-2, M2-3, M2-4 and M3-7 for **16–25**; the increase is
  FSEvents' three contracts and real stdio, both of which the spike's one-line
  row did not see. Its compositor 3–4 plus WKWebView 8–10 is **11–14**; the first
  draft's Q5 compressed those to 6–9 without demonstrating a saving, and this
  draft restores them as X-1 + M4-1 + M4-2 + M4-3 = **12–18**, the excess being
  the policy-enforcement matrix the spike did not know it needed.
- **Video was costed at 3 days for the first frame only.** Playback with audio,
  seeking and colour conversion (M4-5) is new against that row.

Both numbers are honest readings of different things: the spike read the code,
this reads the work.

---

## 8. Open questions for the owner

Twelve, recommendation first.

**Q1 — Where do settings live?** *Recommendation: `~/Library/Application Support/
Folio`.* Folio is a window before it is a command, it will be an `.app` with a
bundle identifier, and the cache, notification registration and web data store
all follow Apple's layout already. Nothing on macOS to migrate from.

**Q2 — The global shortcut: which mechanism, and when is it authorized?**
*Recommendation: `CGEventTap`, authorized through an in-app "Enable global
shortcut" action.* The first draft said an `NSEvent` global monitor "needs no
permission". **That is wrong** — Apple requires Accessibility trust for it too,
and it still cannot suppress the event, so the chord would also reach the
frontmost app. And "ask at first summon" has a bootstrap problem: without
authorization there is no first summon to notice. An explicit action, with a
visible *not authorized* state, is the only shape that works.

**Q3 — Does "Open Folio here" survive, and how?** *Recommendation: `NSServices`
only; no Finder Sync extension.* One correction: a Service is **not** one
dictionary. It needs a registered provider object and a method that receives
pasteboard data, which is why M4-9 is a `bt-platform` ticket rather than a plist
edit. A Finder Sync extension is a second signed bundle for a submenu that is
still not the first page.

> **Ruled 2026-09-12: `NSServices` only.** The owner asked how other terminals do it: Terminal.app ("New Terminal at Folder"), iTerm2 and Ghostty all ship Services entries; Warp, kitty and Alacritty ship nothing and leave it to the user. Services is the convention, not a compromise.

**Q4 — Bundle identifier, signing team, and minimum macOS version.**
*Recommendation: a reverse-DNS identifier under a name the project controls, the
owner's existing team, and a deployment target of macOS 14.* The identifier is
permanent — TCC keys every granted permission on it. The deployment target must
be stated rather than inherited from whatever `macos-latest` happens to be, or
compatibility is decided by accident.

**Q5 — Is a reduced WKWebView capability set acceptable?** *Recommendation: yes,
if X-2 names exactly what is reduced and the product says so where a reader can
see it.* WebView2's `WebResourceRequested` filter covers more than WKWebView's
public hooks reach. The alternative — dropping the web preview from 0.4 — saves
12–18 days and is the only real schedule lever, but §7.9 makes a page a preview
buffer rather than an extra, and a preview pane that refuses one file type is a
hole a reader finds on the first afternoon.

**Q6 — arm64 only?** *Recommendation: yes for the preview.* A universal binary
doubles every compile on the one Mac that is already the constraint.

**Q7 — Where are releases signed, and how is the private key reached?**
*Recommendation: a dedicated non-login signing keychain on the Mac mini, unlocked
by the owner for the duration of a release.* An API key does not sign (§5 ③).
The alternatives are the owner signing at the machine, or a CI runner holding the
identity. This has to be settled before M4, because X-5 and every TCC feature
depend on a stable identity being reachable.

**Q8 — What is the default shell, and is it a login shell?**
*Recommendation: the user's `$SHELL`, started as an interactive non-login shell,
with the shipped profiles offering zsh, bash and `/bin/sh`.* This matters more
than it reads: a Finder-launched app inherits almost no environment, so a
non-login shell will not see a `PATH` set in `.zprofile`. If the owner wants
Terminal.app's behaviour, the answer is a login shell and the cost is slower
startup.

**Q9 — Option as Alt, or Option as text?** *Recommendation: Option as text by
default, with a setting, matching Terminal.app and iTerm's defaults.* Option-as-
Alt breaks every accented character a reader types; the terminal users who want
`Alt+f` know to turn it on. winit 0.30.13 offers left/right granularity if the
owner wants the split.

**Q10 — What happens when the last window closes?** *Recommendation: the app
stays in the Dock, and a Dock or Finder click opens a new window.* That is the
macOS convention and it is what the quake terminal already needs — §7.54e's
"companion" model maps onto it exactly. The Windows behaviour (last window
leaving is the process leaving) is asserted by a test in `main.rs` and would need
a platform arm.

**Q11 — Which coverage gaps may ship?** *Recommendation: ship with the
clean-machine gap and the mixed-scale gap both written down, and close neither.*
A second account is not a pristine machine; a scaled resolution is not a
different backing scale. Closing the first needs a VM, the second a display.
§7.50 records that a window that could not cross the seam between two screens
reached a user on Windows, so the second gap is the one with history.

> **Ruled 2026-09-12: the mixed-scale gap closes — the owner has a second display and will attach it for M3 (it must run at a different backing scale from the 4K panel, or it proves nothing). The clean-machine gap closes at M5/M6 with a macOS guest (Tart or UTM on the external SSD; the internal volume's 48 GiB is not enough), restored to a clean snapshot before each acceptance; nothing is needed before then.**

**Q12 — A macOS UI-acceptance harness in 0.4?** *Recommendation: no, and say so
out loud.* Every acceptance line in §2 is the owner in front of the Mac mini
rather than a script, which is why the milestones are six rather than twenty, and
a defect there comes back as a screenshot and a sentence. Revisit in 0.5.

> **Ruled 2026-09-12: build it.** A macOS `ui-probe` is a ticket after M1 (screenshots via `screencapture` / `CGWindowListCreateImage`, input via `CGEvent` posting, the pixel comparison shared with the Windows script). Injection needs Accessibility, the same grant the global shortcut needs (Q2), so the two land together. 5–8 agent-days; §7.3 rises by that much, and from M2 on the acceptance lines are run by an agent and read by the owner.

---

## Appendix — what was measured on the Mac mini

Read-only over `ssh -o BatchMode=yes mac-mini`, 2026-09-11. Nothing installed,
built, started or changed.

| Question | Answer |
|---|---|
| Machine | Apple M4, 10 GPU cores, Metal 4; macOS 26.6.2 (build 25G83), arm64 |
| Developer tools | `/Applications/Xcode.app` **is installed**, version 26.6; `xcode-select -p` still answers `/Library/Developer/CommandLineTools`, so `xcodebuild` refuses to run |
| Rust | `rustc 1.98.1` as default `stable-aarch64-apple-darwin`; `1.94.1-aarch64-apple-darwin` also installed |
| Signing | `security find-identity -v -p codesigning` → `0 valid identities found` |
| Notarization | `xcrun --find notarytool` present; `xcrun notarytool history --keychain-profile folio` → `Error: keychainLocked(keychainName: "default")` over a non-interactive session |
| Display | one DELL S2725QS, 3840×2160 presenting as 1920×1080 — backing scale 2.0, and no second scale available |
| Disk | 228 GiB volume, 48 GiB free; `~/folio-port` holds 13 GiB |
| The spike checkout | `~/folio-port/repo`, detached at `ffdd444`, working tree clean |

---

## Review record

**Codex, read-only, 2026-09-12.** 25 findings — 3 blockers, 22 major — against
the plan's first draft. Every citation was re-verified against the tree before
this revision; line numbers in the review are from a slightly different reading
of `main.rs` and shift by a few hundred lines, but no cited fact was wrong about
its subject.

**Accepted, and the revision acts on all of them:** the three blockers (the
startup path that requires an `HWND`, a custom frame and a compositor before a
first frame; native shipped profiles rather than one resolver function; and
wgpu-hal 30's Metal offering only `Opaque` and `PostMultiplied`, which makes the
carried-over `PreMultiplied` contract impossible) and the twenty-two major
findings, including the superseded milestone rulings now recorded in §0; the
type-level exceptions to the stub rule; `cli.rs`'s ungated uses; the three
`DirWatch` contracts; clipboard predicates and `WebChord`'s missing Command;
Finder reopen being an application event rather than a second process; the
Unix socket's identity and trust differences; `NSEvent` global monitors also
needing Accessibility; a Service needing a provider object; App Store Connect
keys not signing; the notarization and Gatekeeper artifact order; clean-user
versus clean-machine; §4.6 proving only a Linux property; the stale "already
true" claims (`quiet_command` already portable, `core-linux` spawning no child,
`contrast.rs` being colour policy rather than rasterization, the `player` cache
nobody writes, the first-run card's real rows, the real version gate's name and
location, the hang and hotkey arm counts); and the seven missing owner questions,
now Q4, Q5, Q7, Q8, Q9, Q10 and Q11.

**One finding corrected in the other direction.** Finding 5 says
`instance::claim_data_directory`'s non-Windows arm "returns `None`". It returns
**`Some(DataDirectoryClaim)`** — always succeeding, with a comment saying so.
The first draft of this plan made the same mistake. The consequence is worse than
either statement: the single-writer guarantee is absent off Windows by
construction, so M3-5 builds it rather than preserving it (§4.4 ④).

**Nothing else was rejected.** Two findings were narrowed rather than refused:
the review's §4.1 correction is accepted but restated — `bt-render` has one
documented `unsafe` exception rather than none, and keeping native video FFI in
`bt-platform` stays a placement decision; and the ticket arithmetic was not
adopted as a number but rebuilt from a different inventory, which is why §7.3
reads 112–163 against the review's 89–130 rather than adopting it.
