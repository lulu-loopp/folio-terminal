# T-SELF-UPDATE — stage 1 design, 2026-09-16

Design only. Inspected `design/self-update` at `134ff9c0`; no product code, no
build, no test run, no application launch belongs to this stage. Paths below are
this worktree's unless said otherwise.

The owner reversed two standing rulings on 2026-09-16: `crates/bt-app/src/update.rs`
may now download, and `docs/plans/port/macos-plan-2026-09-12.md` §M4's deferral of
in-place self-update out of 0.4 no longer holds. Folio 0.4.2 ships an in-app
updater. Everything below is written against that reversal.

## A. What the rule becomes

The feature that exists today is one `GET` of `api.github.com/repos/lulu-loopp/folio-terminal/releases`,
at most once a day across every window, answered by a dot on the gear and a
sentence on the General page's last row (`crates/bt-app/src/update.rs:1-40`,
`docs/DESIGN.md` §7.52). **That whole mechanism stays.** It is the *check*, and
the updater is a second thing that starts where the check ends. Nothing in §7.52's
①–⑧ about cadence, the claim file, the two-window rule, version precedence, the
`User-Agent`, the silence of a refusal, or the privacy documents changes.

What changes is one clause, in five places:

| Where | Sentence today | Sentence after |
|---|---|---|
| `update.rs:1-2` module title | "answered by a mark on the gear, and **never acted on**" | "…and acted on only when the reader presses *Update and restart*" |
| `update.rs:10` | "…and **downloading nothing** whatever it learns" | "…and downloading nothing until a press asks it to" |
| `update.rs:31-34` bullet **Downloads nothing** | "There is no installer, no replacement, no restart. The most this feature can do is put a dot on a gear and a sentence in a dialog." | Becomes **Downloads nothing unasked**: the check itself still downloads nothing; the download, the verification and the swap are a separate module that only a press can enter, and a launch on which nobody presses is byte-for-byte the launch this module describes today. |
| §7.52 block quote ("每 24 小时至多一次…**不下载、不替换、不重启。**") | the three negatives are the stated bound | the bound becomes: the *check* downloads nothing, replaces nothing and restarts nothing; a *press* may do all three, and never anything else |
| §7.52 ① ("**系统 toast**…出局" / "有新版不是一件需要打断人的事") | the three-verb surface was rejected as disproportionate | a card is now warranted, and the reason is in the ruling: the offer is no longer "go and read a page", it is a thing the reader can finish here. The toast stays out — it is still outside the window. The pane notice strip (`crates/bt-app/src/notice.rs`) stays out for its own original reason: it eats 30 px of a working pane's body for as long as it is up, and a download is minutes. |

Two more documents carry the old claim and must be revised in stage 2 with the
code, not before: `docs/plans/port/macos-plan-2026-09-12.md:48-49` and `:153-154`
("in-place self-update, which becomes 'open the release page'") and its **M4-10**
row, and `docs/PRIVACY.md`, which must gain the one new fact — that pressing
*Update and restart* fetches two named files from `github.com` and nothing else.
**No new settings key and no `settings.json` schema bump.** `SettingsV1::update_check`
already exists and already defaults to on (`crates/bt-persist/src/settings.rs`,
schema v34); the General row already stands last on its page
(`crates/bt-app/src/settings.rs:5385`). The owner's "Check for updates automatically"
is a **rename of `Text::RowUpdateCheck` and a rewrite of `Text::DescUpdateCheck`**
(`crates/bt-app/src/i18n.rs:2921-2926`), because the present description —
"Checks once a day for a newer Folio. The new version is named on this row." —
is now the smaller half of what the switch buys.

## B. The state machine

One machine per process, owned by the window thread, driven by messages from one
worker. `Idle` is every launch on which nobody presses.

```
Idle ──offer──> Available(tag) ──press──> Downloading{tag, got, total}
                     │                         │
                     │                         ├─ok─> Verified{tag, staged}
                     └─Later / Skip ─> Idle    └─fail─> Failed(reason) ─dismiss─> Idle
                                               ↑
Verified ──ready──> Swapping{tag} ──ok──> Relaunching{child} ──> (quit)
                         │                      │
                         └─fail(rolled back)────┴─fail─> Failed(reason)
```

**`Available(tag)` is entered** when `update::newer_than(latest_tag, VERSION)`
answers `Some(tag)`, the tag is not `skipped_tag`, the install location is one
the updater owns (§D), and this build is a release build (§D). It is entered at
most **once per launch**, in the most recently active ordinary window — the same
"most recently active window, and never the summoned terminal" rule §7.59 already
states for a handed-off launch. The gear dot keeps its own meaning and is
unchanged: it is lit by `latest_tag != seen_tag` and put out by the reader
reaching the page the row is on (§7.52 ①).

**The card.** A float window on the first-run card's footing —
`settings::push_float_window` (`crates/bt-app/src/settings.rs:14840`), the `.btn`
pair, `restore::wrap` (`crates/bt-app/src/first_run.rs:22-27`) — with
`first_run::Card`'s exact shape: a `Card` struct the window owns, a `Target` enum
for what a press means, a `raise_…_if_due` on the window thread and an
`answer_…(target)` beside `main.rs:52411` / `main.rs:52595`. It shows the version
number, one highlight line, and three verbs. It never dims the window behind it,
for `notice.rs`'s stated reason: the shell behind it is working.

**The card is rebuilt from live state every frame**, on the web-fault card's
footing rather than the first-run card's: `WebSeat.fault` is a field, and
`seats::PreviewCardContent` is recomputed from it each paint
(`crates/bt-app/src/webhost.rs:1791`, `crates/bt-app/src/seats.rs:20302`), which is
why overwriting the field is both raise and update. A progress bar needs exactly
that and nothing else. **There is no progress surface in the product to reuse**:
the only thing called progress today is `ChromeMark::ProgressRing` on a tab title,
which is a different assertion on a different surface. The determinate bar is new
drawing, and it belongs to ticket 1.

| State | What the card says | The three verbs |
|---|---|---|
| `Available(tag)` | `0.4.2 is available.` + one highlight line | **Update and restart** · **Later** · **Skip this version** |
| `Downloading` | `Downloading 0.4.2 — 12 MB of 41 MB` + a determinate bar | **Cancel** replaces the first verb; the other two are gone |
| `Verified` | `0.4.2 is ready. Every tab's program will be closed. Your tabs and layout come back.` | **Restart now** · **Later** |
| `Swapping` | `Replacing the installed files…` | none |
| `Relaunching` | `Starting 0.4.2…` | none |
| `Failed(reason)` | the reason, then `Nothing installed was changed.` | **Open releases page** · **Close** |

**"Later"** closes the card and writes nothing. The dot stays lit, the row keeps
naming the version, and the card is offered again on the next launch. **"Skip this
version"** writes `skipped_tag = tag` **and** `seen_tag = tag` into
`update-check.json` — Skip is an answer, so the dot goes out too — and the card is
not offered again until `latest_tag` moves past it. Skip is per tag, never a mode:
there is no "stop updating" state, because the switch on the General page is that.

**Failure edges.** Every one of them ends in `Failed(reason)` with the installed
files untouched, and every reason is a sentence the card prints: the releases page
could not be reached; the download did not finish; the download is larger than the
updater will take (the cap is 200 MB; the archive is ~40 MB); `SHA256SUMS.txt` did
not name the asset; the hash did not match; the new `folio.exe` is not signed by
this product's certificate; the new bundle did not pass `codesign`; this folder
cannot be written to; a file could not be renamed; the new build would not start.
The last two roll back first and say so (§C).

**If the window closes mid-download.** The worker holds a cancel flag and a
`Weak` back to nothing else; the window thread sets the flag on its way out and
does not wait. The partial file is under `<data>/Folio/update/<tag>/` and is
deleted by the *next* launch's sweep, not by the dying one — a process on its way
out must not block on a filesystem. If the process is killed outright, the sweep
still finds it, because the sweep's rule is "anything under `update/` that is not
this launch's staging directory". **Nothing installed can be mid-swap at that
moment**: the swap (§C) happens on the worker between the session save and the
quit, and the quit does not start until the swap reports.

## C. The swap, step by step

Two owners throughout. **W** is the window thread; **K** is the one worker thread,
started at `ThreadPriority::BelowNormal` through `bt_platform::spawn_at_priority`
exactly as `update::begin` starts the check's thread. Every message from K to W
arrives as `AppEvent::UpdateProgress` / `AppEvent::UpdateOutcome` through the
existing `EventLoopProxy` (`crates/bt-app/src/main.rs:36798-36800`), and both need
an arm in `AppEvent::station` (`main.rs:588`) — that is where `user_event` reads
the station it opens. Every W step
below stands at a new `hang_watch::Station::UpdateSwap`, entered with
`hang_watch::enter` and put back with `hang_watch::at`
(`crates/bt-app/src/hang_watch.rs:1440-1466`). K never touches a window, a
compositor, or `KNOWN`; W never opens a socket or a file bigger than a listing.

**Shared, both platforms (K unless marked):**

1. **(W)** press → set `Downloading`, hand K the tag. One press, one worker; a
   second press is ignored while a worker is live.
2. Create `<data>/Folio/update/<tag>/`; sweep every sibling that is not it.
3. `GET github.com/lulu-loopp/folio-terminal/releases/download/<tag>/SHA256SUMS.txt`
   (or `SHA256SUMS-macos.txt`). Text, small, through today's `bt_platform::https_get`.
4. `GET …/releases/download/<tag>/folio-windows-x64.zip` (or `Folio-macos-arm64.dmg`)
   through the **new** `bt_platform::https_download` (§E). Progress to W on each
   chunk, coalesced to at most one message per 100 ms.
5. Hash the file; compare to the line `SHA256SUMS.txt` carries for that bare name.
   Mismatch or missing line → `Failed`, files deleted.
6. Unpack (Windows) or attach (macOS) and run the platform check below. A failure
   here is `Failed` and the staging directory is deleted.
7. **(W)** `Verified`. On **Restart now**: flush the session store so `session.json`
   (schema v15, `crates/bt-persist/src/session.rs:51`) holds every window's
   placement, tab strip, pane tree and per-pane directory as it stands, then hand
   K the go-ahead. The flush must be forced rather than waited for: the store's
   1.5 s debounce (`crates/bt-app/src/persist.rs:248`) is longer than the gap
   between this step and the quit.
8. K performs the platform swap, then spawns the new build (below), then reports.
9. **(W)** on success, the ordinary quit path — the same one `Quit` runs, which
   closes every window and releases the data-directory claim.

### Windows

The nine files are `folio.exe`, `folio.msix`, `conpty.dll`, `OpenConsole.exe`,
`folio-here.cmd`, `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md`,
`TRADEMARK.md` (`scripts/release/package.ps1:278-298`).

1. Extract the zip into `<data>/Folio/update/<tag>/new/`. Refuse any entry whose
   path is not exactly one of the nine bare names — no directories, no traversal.
2. `WinVerifyTrust` on `new\folio.exe` and on `new\folio.msix`: the signature must
   verify, must carry a countersignature time stamp, and the signer's subject must
   be the same distinguished name as the **running** `folio.exe`'s, compared with
   `bt_platform::msix::distinguished_name` / `publisher_matches_subject`
   (`crates/bt-platform/src/msix.rs:121,175`). Reading our own certificate rather
   than a baked-in literal is the point: the day the certificate is renewed, the
   updater does not need a new build to accept it. `scripts/release/smoke.ps1:342-365`
   is the same check made on the artefact at release time.
3. **The writability probe.** `MoveFileExW` the running `folio.exe` to
   `folio.exe.old`. This *is* the probe — a running image can be renamed but not
   overwritten, and no other test answers the question honestly. Failure here is
   the whole of "this folder is not ours": Program Files, a read-only share, a
   locked folder. It ends in `Failed` with the release page offered, and nothing
   has moved.
4. Rename the other eight the same way, in the listed order. Any failure retries
   that one rename 5 times at 200 ms — the bound `persist.rs:32-47` already sets
   for a file an antivirus or a sync client is holding — and then **rolls back**:
   every `.old` renamed back, in reverse order, and `Failed`.
5. `MoveFileExW` the nine staged files in, with `MOVEFILE_REPLACE_EXISTING` unset
   (nothing is there). A failure rolls back both halves.
6. **Spawn before quitting.** `bt_platform::quiet_command(install_dir\folio.exe)`
   with `--await-exit <our pid>`, detached. If the spawn fails, roll back
   completely and `Failed` — the window is still up, so this is recoverable, and
   that is exactly why the spawn precedes the quit.
7. The old files are **not** deleted here. The next start deletes `*.old` beside
   `folio.exe`, best-effort, on K. That is the "never delete the old files until
   the new build has started once" rule, mechanically.

**The sparse MSIX.** `folio.msix` is an identity registered against the folder, and
Windows keys a registration by the package's version. When the msix bytes change —
they do every release, because `package.ps1:349-397` writes the version into the
manifest — an existing registration still names the old version and the old
manifest. **Re-registration is needed, and only when a registration exists.** On
the next start, if `msix::registration()` answers a registration whose
`EffectiveExternalPath` is this folder and whose version is not `version::VERSION`,
re-register: `AddPackageByUriAsync` with `SetExternalLocationUri(folder)` replaces
what is there rather than refusing (`crates/bt-platform/src/msix.rs:298-310`), so
there is no remove-then-add window in which the machine has neither. It runs on
`explorer_menu`'s own thread, never on W. A machine that never turned the first-page
verb on has no registration and nothing happens.

**A second Folio.** §7.59 makes one process per data directory, so a second Folio
over the *same* `%APPDATA%` cannot exist. A second Folio over an isolated data
directory but the *same* install folder can (that is how test windows are opened).
It survives the swap: its image is already mapped, renaming the file underneath it
does nothing, and it keeps running the old build until it exits. What it does break
is the `.old` sweep, which will fail on a file still mapped — which is why the
sweep is best-effort and retried at every start.

### macOS

1. `NSBundle.mainBundle().bundlePath`. If it does not end in `.app`, this is not an
   installed bundle → §D, Development.
2. `hdiutil attach -nobrowse -readonly -noverify -mountpoint <temp>/mnt <dmg>`.
3. `spctl -a -vvv -t exec <mnt>/Folio.app` must answer `accepted` **and**
   `source=Notarized Developer ID` — the same assertion `scripts/release/macos/notarize.sh:192-204`
   makes before the tag is published, asked here on the reader's machine, by
   Gatekeeper, about the bytes that just arrived. Then
   `codesign --verify --strict --verbose=2 <mnt>/Folio.app`. Either refusal →
   detach, `Failed`.
4. `ditto <mnt>/Folio.app <parent>/Folio.app.new` — `ditto`, not `cp -R`, because
   it carries extended attributes and leaves the signature intact.
5. `codesign --verify --strict --verbose=2 <parent>/Folio.app.new` — the copy, not
   the source, because the ticket's rule is that what gets installed is what was
   checked.
6. `rename(2)` `Folio.app` → `Folio.app.old`, then `rename(2)` `Folio.app.new` →
   `Folio.app`. Two renames in the same directory, so each is atomic and the first
   failing leaves nothing done. A failure of the second renames the first back.
7. `open -n -a <parent>/Folio.app --args --await-exit <pid>`. Spawn before quit,
   rollback on failure, as on Windows.
8. `hdiutil detach <temp>/mnt`; delete `Folio.app.old` on the **next** start.

### The relaunch handoff

`open -n` and a detached `CreateProcess` both start a real second process, and both
would then meet §7.59's handoff: a second Folio asks `persist::is_writer_of`, finds
the data-directory claim held, calls `launch_wire::hand_over` down the well-known
pipe, and exits through `leave_process` (`crates/bt-app/src/main.rs:118196-118201`).
Its two-second `HANDOVER_BUDGET` (`crates/bt-platform/src/launch_pipe.rs:111`) is
far shorter than a quit, so without a wait the new process would reliably be
swallowed into the old one as a new tab, and the old build would keep running.
**`--await-exit <pid>` is the one new thing.** It is parsed in
`crates/bt-app/src/cli.rs` before anything else happens, and it means: wait for
that process id to be gone, then start normally.
**The pid is the fast path; the claim is the authority.** The wait ends when
`bt_platform::instance::claim_data_directory` succeeds — that is the one fact that
actually decides whether this process becomes the running Folio or hands itself off
— and the pid is only what it polls between attempts, so the loop costs nothing
while the old process is winding down. Windows: `OpenProcess(SYNCHRONIZE)` +
`WaitForSingleObject`; a handle that cannot be opened means gone. macOS:
`kill(pid, 0)` at 100 ms. Waiting on the claim rather than the pid alone is also
what makes pid reuse harmless. The wait gives up after **30 s** and starts anyway —
`instance.rs` already recovers a claim whose owner died holding it, so a wedged old
process cannot make the new one unstartable. Placed before
`diagnostics::enter_resident_run`, beside the single-instance fork §7.59 puts there.

## D. Where it is installed, and what that means

One function, `update::location()`, answering one enum. Every arm is a fact about
the machine, asked of the machine; none is a version string or a guess.

| Location | How it is told | Behaviour |
|---|---|---|
| **Portable folder** (Windows) | the rename probe in §C step 3 succeeds | the full swap |
| **Unwritable folder** (Program Files, a share, a policy-locked directory) | the rename probe fails | card says where it is installed and offers **Open releases page**; nothing is downloaded a second time |
| **Homebrew cask** (macOS) | `/opt/homebrew/Caskroom/folio` or `/usr/local/Caskroom/folio` exists — the cask's own receipt directory; the bundle may also be a symlink into it | card says to run `brew upgrade --cask folio` and offers **Copy command**; no download |
| **winget** (Windows, when winget installs land) | the install folder has a `Microsoft\WinGet\Packages` ancestor | card says `winget upgrade lulu-loopp.Folio`, **Copy command**; no download. Must be re-verified against the real package when `docs/plans/release/winget.md` lands — a portable-archive package's layout is winget's fact, not ours |
| **macOS bundle elsewhere** (`~/Applications`, a second volume) | `bundlePath` ends in `.app`, parent is writable | the full swap, into that parent — the running bundle's location, never a hard-coded `/Applications` |
| **Development build** | `version::CHANNEL` is `Development` | the check still runs, the dot still lights, the row still names the version; **the card is never raised** |

**`version::CHANNEL` is new and is the only honest way to tell a `dist/nextNN`
candidate from a release.** It cannot be a path test — a candidate folder and an
extracted release folder are the same nine files — and it cannot be the commit,
because a build has no way to know offline which commit a tag points at. So it is a
build-time fact, on `version::COMMIT`'s exact footing (`crates/bt-app/src/version.rs:24-40`):
`build.rs` reads an environment variable that **only the release packaging step
sets**, and `CHANNEL` is `Release` when it is present and `Development` otherwise,
including for a source tarball where `COMMIT` is already `unknown`. A developer who
wants the card has to set the variable, which is a thing they can only do on
purpose.

## E. Verification

**The hash file is fetched by tag, never by `latest`.** Both addresses are
`github.com/lulu-loopp/folio-terminal/releases/download/<tag>/<name>`, with the
same `<tag>` the check returned, for one reason: `/releases/latest/download/` is
resolved by GitHub at request time (`docs/RELEASING.md:29-42`), so a release
published between the two requests would hand back an asset from one release and a
hash from another, and both would verify. By tag, they cannot come apart.

The fixed names exist precisely so an address can be assembled without a version in
it: `package.ps1` writes `folio-windows-x64.zip` beside the long name and `dmg.sh`
writes `Folio-macos-arm64.dmg` beside its own, one set of bytes copied by the
packaging step and covered by the same checksum file (`docs/RELEASING.md:29-36`).
`SHA256SUMS.txt` and `SHA256SUMS-macos.txt` carry **bare names** in `sha256sum -c`
format — lowercase hex, two spaces, the file name, LF (`docs/RELEASING.md:157-173`,
`scripts/release/package.ps1:555-568`) — so the updater looks up the exact bare name
it asked for and refuses a file the checksum document does not name.

**The new platform call.** `bt_platform::https_download` is a second function beside
`https_get`, in both arms, and it keeps every discipline `http.rs` already states:
one `GET`, `https` only, no caller headers, the platform's own proxy and certificate
store (`WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY` on Windows, an ephemeral
`NSURLSession` on macOS), no configuration of our own. What it adds is exactly
four things: it writes to a file instead of a `String`; it reads
`WINHTTP_QUERY_CONTENT_LENGTH` (Windows) / `NSURLResponse.expectedContentLength`
(macOS) and **refuses before the first byte** if it exceeds `MAX_UPDATE_BYTES`
(200 MB) — and refuses again mid-stream if the body outruns it, because a
`Content-Length` is a claim; it calls a progress closure; and it polls a cancel
flag. Its timeouts are its own: a 30 s idle phase timeout and a 10-minute whole-call
budget, because `http.rs`'s 5 s / 15 s are sized for an 18 KB JSON document.

**Windows Authenticode**: `WinVerifyTrust` with `WINTRUST_ACTION_GENERIC_VERIFY_V2`
on the new `folio.exe` and the new `folio.msix`, plus a signer-subject comparison
against the *running* executable's own certificate subject, by distinguished name
(§C). The expected subject on a release today is
`CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US`, and it is read rather than
written down.

**macOS Gatekeeper**: `spctl -a -vvv -t exec` on the mounted bundle, requiring both
`accepted` and `source=Notarized Developer ID`, and `codesign --verify --strict`
on the copy. **The quarantine attribute is never removed.** A bundle we wrote
ourselves carries no `com.apple.quarantine` — that attribute is applied by
LaunchServices-aware downloaders, not by `write(2)` — and an updater that stripped
it would be doing the one thing a malicious updater needs. If the attribute turns
out to be present (§H), the answer is to find out why, not to delete it.

## F. Tests

All CPU-only, all in-process, none touching the network, the registry, a real
install folder or a real process.

| Gate | Shape |
|---|---|
| `a_swap_plan_is_computed_from_a_listing_and_nothing_else` | `plan_swap(installed: &[FileName], staged: &[FileName]) -> Result<SwapPlan, SwapRefusal>`, a pure function over two listings. `SwapPlan` is an ordered `Vec<SwapStep>` of `RenameAside`/`MoveIn`, and its inverse is a second `Vec`. No filesystem is involved in computing either |
| `a_rollback_undoes_exactly_the_steps_that_ran` | for every prefix length of a plan, applying the inverse of that prefix to a fake filesystem returns it to its start state |
| `a_staged_set_missing_a_file_is_refused_before_anything_moves` | eight of nine, ten of nine, a name not on the list, a name with a separator in it |
| `the_state_machine_never_replaces_without_both_checks` | drive `Idle → Relaunching` over a fake `Downloader` and a fake filesystem; every arm in which the hash or the signature answers no must reach `Failed` with the fake filesystem unmodified. The mutation is to let one check's `false` through |
| `a_skipped_tag_is_not_offered_and_a_newer_one_is` | `should_offer(latest, running, skipped, channel)` over the same 14-tag ladder §7.52 ④ already uses |
| `a_development_build_is_never_offered_the_card` | `channel = Development` answers `false` for every input |
| `later_leaves_the_file_alone_and_skip_writes_two_fields` | over a temp `update-check.json` |
| `an_update_check_file_written_by_v1_is_read_as_v2` | the first entry in `UPDATE_CHECK_MIGRATIONS`, which is empty today (`crates/bt-persist/src/migrate.rs:842`) |
| `a_location_is_decided_by_what_the_machine_answers` | `detect_location` over an injected prober (probe result, Caskroom presence, ancestor names, channel) — the six rows of §D, one case each |
| `a_body_longer_than_the_cap_is_refused_before_it_is_read` | the fake transport claims 300 MB; the fake transport then lies and claims 10 MB while delivering 300 MB |

**`update-check.json` goes to schema v2**, gaining `skipped_tag: Option<String>`.
It is the first entry `UPDATE_CHECK_MIGRATIONS` has ever carried, and the step is
"add the key as `null`", because a file that has never skipped a version has not
skipped one.

**The clean-machine plan.** `docs/plans/release/clean-vm.md` (1012 lines, the Gate 5
runbook) gains a `### 4.4 更新器` checklist beside its existing per-machine tables
at `:601-656`, and rows in its `## 8` known-gaps table at `:943`. The procedure,
both platforms: install **0.4.1** by the ordinary route; serve a staged **0.4.2**
from a draft release on the real repository (so that the tag, both asset names and
both checksum documents are the real ones and no address is faked); launch, wait
for the check, press **Update and restart**; assert the nine files' modification
times moved, `--version` answers 0.4.2, `session.json`'s tabs came back, and the
`.old` files are gone after the second start. Then the four refusals, each its own
run: a folder made read-only; a checksum document edited by one character; an
unsigned `folio.exe` substituted into the zip; the network cut mid-download. Each
must leave `--version` answering 0.4.1. On Windows, one more run with the first-page
context-menu verb turned on, asserting the verb still works after the swap (the
re-registration in §C). On macOS, one run from a Homebrew-installed bundle,
asserting the card offers the command and downloads nothing.

## G. Stage 2 — three tickets

| # | Ticket | Files | Size |
|---|---|---|---|
| 1 | **The machine and the card.** `UpdateState`, `should_offer`, `plan_swap`, `SwapPlan`, the `Downloader` trait, `version::CHANNEL`, `update-check.json` v2, the card's geometry and three verbs, the two renamed i18n strings, the `AppEvent` arms and their `station()` rows, `Station::UpdateSwap`, the forced session flush, `--await-exit` | `crates/bt-app/src/update.rs`, `update_card.rs`(new), `{main,cli,i18n,settings,persist,hang_watch,version}.rs`, `build.rs`, `crates/bt-persist/src/{update,migrate,lib}.rs`, `crates/bt-platform/src/{http,http_portable,macos_http,instance}.rs` | **L** |
| 2 | **The Windows swap.** the rename probe, the nine-file swap and its rollback, `WinVerifyTrust`, the zip reader, the `.old` sweep, the msix re-registration, the winget arm of `detect_location` | `crates/bt-platform/src/{trust.rs(new),msix}.rs`, `crates/bt-app/src/update_swap_windows.rs`(new), `update.rs` | **M** |
| 3 | **The macOS swap.** `bundlePath`, the `hdiutil`/`spctl`/`codesign`/`ditto` sequence, the two renames, `open -n`, the Caskroom arm | `crates/bt-platform/src/macos_app.rs`, `crates/bt-app/src/update_swap_macos.rs`(new), `update.rs` | **M** |

**Order is 1 → 2 → 3.** Ticket 1 is the only one with a testable core, and it must
land with a `SwapDriver` trait whose Windows and macOS implementations are a `todo!`
that reports `Failed` — so the shipped state after ticket 1 is exactly today's
behaviour plus a card that says "not on this platform yet", and never a half-swap.
Tickets 2 and 3 are independent of each other.

**Strings for opus46 to write in Chinese** (English first, in ticket 1; the Chinese
is a separate pass, per the standing rule that all Chinese copy is written by
opus46 against the seven description rules):

- `Text::RowUpdateCheck` — renamed from "Update check" to "Check for updates automatically"
- `Text::DescUpdateCheck` — rewritten: two sentences, written declarative, the reader's view, ≤ two lines, and it must now say that Folio can install the new version and that it never does so without a press
- `update_card_available_in(lang, version)` — the card's headline, composed on `update_row_available_in`'s footing (`i18n.rs:6013`)
- `Text::UpdateVerbInstall` / `UpdateVerbLater` / `UpdateVerbSkip` / `UpdateVerbCancel` / `UpdateVerbRestartNow` — the five verbs
- `Text::UpdateWarnTabsClose` — the one sentence about every tab's program closing and the layout coming back
- `update_card_progress_in(lang, got, total)` — the progress line
- the eleven failure sentences of §B, one `Text` each
- `Text::UpdateUseBrew` / `Text::UpdateUseWinget` / `Text::UpdateFolderNotOurs` — the three §D sentences, and `Text::CopyCommand`

Every one of them is a `Text` variant with a `pick(lang, english, chinese)` arm, and
every one has to be added to the completeness array the crate's tests walk
(`crates/bt-app/src/i18n.rs:5620` onward) — that array is what makes a string with
no Chinese column a red build rather than an English word in a Chinese window. The
English lands in ticket 1 with the Chinese column filled by a literal translation
that is explicitly marked for replacement; opus46 replaces the column, and the
`check-doc-words` / copy-guide vocabulary rules apply to both.

## H. Risks, and the experiment that answers each

| Risk | Experiment |
|---|---|
| **Renaming a running exe on a OneDrive-synced folder.** The rename-aside trick is a documented Windows fact, but a sync client's filter driver sits between it and the disk, and `persist.rs:272` already records OneDrive making a write take a second and a half. A placeholder file that is not hydrated may refuse the rename outright | put an extracted 0.4.1 in a OneDrive-synced folder on a clean VM, once hydrated and once as an online-only placeholder, and run the swap. Measure the rename latency and the failure code. If it refuses, the answer is §D's unwritable arm, not a workaround |
| **The copied bundle carries `com.apple.quarantine`.** If any part of the path — `hdiutil attach`, `ditto`, the copy's provenance — applies it, the new Folio is refused on first launch with a dialog the reader has no context for | on a Mac, download a real dmg with `https_download`, run the full §C sequence, and `xattr -l` the dmg, the mounted bundle, the copy and the installed bundle at each step. If the attribute appears, find which step applies it |
| **The relaunch races the single-instance claim.** The new process must not reach `launch_wire::hand_over` while the old one still holds the claim, or it becomes a tab in the build it was meant to replace — and the failure is silent and looks like "the update did nothing" | instrument a debug build to log the claim release, the last window's destruction and the process exit; run 50 swaps on each platform and assert the new process never took the handoff path. The claim-acquisition wait above is designed to make this unreachable; the experiment is what proves it |
| **The msix re-registration fails on the new build** — a deployment refusal names neither string (`msix.rs:26-31`), so the reader would see a first-page verb quietly stop working | on the Win11 clean VM with the verb turned on, swap 0.4.1 → 0.4.2 and read `msix::registration()` before and after. The refusal path must put the row back to `Off` and say so, never fail silently |
| **A 200 MB cap and a 10-minute budget are guesses.** The archive is ~40 MB today and the dmg is smaller, but a future release with a bundled runtime could approach the cap, and a slow connection could approach the budget | measure the real asset sizes at 0.4.2 and the download time on a throttled 1 Mbit link. Both numbers are constants with tests naming them; a release that outgrows the cap must fail the release gate, not the reader's machine |
| **`spctl` on a machine with Gatekeeper disabled** answers `accepted` for anything, so the notarization check would pass on exactly the machines least able to afford it | on a Mac with `spctl --master-disable`, run the check against an ad-hoc-signed bundle. If it passes, the `codesign --verify` of step 5 plus a requirement on the signing identity — not `spctl` — must be the load-bearing check |
