# T-SELF-UPDATE — stage 1 design, rev 2, 2026-09-16

Design only. Inspected `design/self-update` at `0f737e65`; no product code, no build, no test run, no application launch belongs to this stage.

**Rev 2 replaces rev 1's §C entirely.** The adversarial review
(`docs/plans/design/self-update-review-codex-2026-09-16.md`, `0f737e65`) was right
on its four blockers and on nearly every must-fix; its evidence was re-read against
the source and holds. Rev 1 treated the replacement as an ordered set of renames a
live worker could undo. It is not: the worker dies with the process, a half-swapped
Windows install has no executable left to run a repair, and a successful
`CreateProcess` is not a working Folio. §C is now written around a recovery
contract instead. §I is a ledger answering R-1…R-23 by number.

The owner reversed two standing rulings on 2026-09-16: `crates/bt-app/src/update.rs`
may now download, and `docs/plans/port/macos-plan-2026-09-12.md` §M4's deferral of
in-place self-update out of 0.4 no longer holds.

## A. What the rule becomes

The check that exists today — one `GET` of the releases list, at most once a day
across every window, answered by a dot on the gear and a sentence on the General
page's last row (`crates/bt-app/src/update.rs:1-40`, `docs/DESIGN.md` §7.52) —
**stays exactly as it is**. Cadence, the claim file, the two-window rule, version
precedence, the `User-Agent`, the silence of a refusal: none of it changes. The
updater is a second thing that starts where the check ends.

| Where | Sentence today | Sentence after |
|---|---|---|
| `update.rs:1-2` | "answered by a mark on the gear, and **never acted on**" | "…and acted on only when the reader presses *Update and restart*" |
| `update.rs:10` | "…and **downloading nothing** whatever it learns" | "…and downloading nothing until a press asks it to" |
| `update.rs:31-34` bullet **Downloads nothing** | "There is no installer, no replacement, no restart." | Becomes **Downloads nothing unasked**: the check downloads nothing; the download, the verification and the replacement are a separate module only a press can enter, and a launch on which nobody presses is byte-for-byte the launch this module describes today. |
| §7.52 block quote ("**不下载、不替换、不重启。**") | the three negatives are the stated bound | the bound becomes: the *check* downloads nothing, replaces nothing, restarts nothing; a *press* may do all three and nothing else |
| §7.52 ① ("有新版不是一件需要打断人的事") | a three-verb surface was disproportionate | a card is warranted now, because the offer is a thing the reader can finish here. The system toast stays out — still outside the window. The pane notice strip stays out — it eats 30 px of a working pane for the length of a download. |

Also revised **with the code, not before**: `docs/plans/port/macos-plan-2026-09-12.md:48-49`,
`:153-154` and its **M4-10** row; and `docs/PRIVACY.md`, which must gain the new
facts — that a press fetches two named files from `github.com` **and from whatever
asset host GitHub redirects to** (`objects.githubusercontent.com` today), that
macOS additionally consults Apple's notarization service through Gatekeeper, and
that none of this happens without the press.

**No `settings.json` schema bump.** `SettingsV1::update_check` already exists,
already defaults on, and its row already stands last on General
(`crates/bt-app/src/settings.rs:5385`). The owner's "Check for updates
automatically" is a rename of `Text::RowUpdateCheck` and a rewrite of
`Text::DescUpdateCheck` (`crates/bt-app/src/i18n.rs:2921-2926`).

## B. The state machine

One **job** per process, owned by the application and not by a window. A job is an
immutable **offer** — `{ txn, tag, asset, hash_doc, to_version }` — captured when
the card is raised and never re-derived. A later check that moves `latest_tag`
under an open card does not change the job.

```
Idle ─offer→ Available ─press→ Downloading ─→ Staged ─→ Verified
  ↑              │                  │                      │
  └─Later/Skip───┘                  └─fail→ Failed          │
                                                            ↓ Restart now
                          Quitting (the ordinary quit, cancelable)
                             │cancelled / save failed → Verified (says why)
                             ↓ session landed
                          Committing  →  [process exits; see §C]
```

`Available` is entered at most once per launch, in the most recently active
ordinary window (never the summoned terminal, §7.59's own rule), when **all** of:
the switch is on; `update::newer_than` answers a tag; that tag has higher
precedence than `skipped_tag`; §D classifies the install as **ours**; and this
build is updater-capable (§D). An offer suppressed because a cached tag was skipped
does not consume the once-per-launch gate — a newer tag arriving later still gets
its card.

**The card.** A float window on the first-run card's footing —
`settings::push_float_window` (`settings.rs:14840`), the `.btn` pair, `restore::wrap`
(`first_run.rs:22-27`) — with `first_run::Card`'s shape: a `Card` the application
owns, a `Target` enum, `raise_…_if_due` / `answer_…(target)` beside `main.rs:52411`
and `:52595`. Escape and the close box mean **Later**. It never dims the window
behind it. Content is recomputed from live state every paint, on the web-fault
card's footing (`webhost.rs:1791`, `seats.rs:20302`), which is what lets a progress
bar live on it. **There is no progress surface to reuse**: the only thing called
progress today is `ChromeMark::ProgressRing` on a tab title, a different assertion
on a different surface. The determinate bar is new drawing.

| State | What the card says | Verbs |
|---|---|---|
| `Available` | `0.4.2 is available.` + the highlight line | **Download and install** · **Later** · **Skip this version** |
| `Downloading` | `Downloading 0.4.2 — 12 MB of 41 MB`, or `12 MB` with an indeterminate bar when the length is unknown | **Cancel** |
| `Staged`/`Verified` | `0.4.2 is ready. Every tab's program will be closed. Your tabs and layout come back.` | **Restart now** · **Later** |
| `Quitting` | whatever the ordinary quit is asking (its own card) | the quit's own |
| `Failed(reason)` | the reason, then either `Nothing installed was changed.` or `The previous version was put back.` | **Open releases page** · **Close** |
| manager-owned (§D) | `Folio was installed by Homebrew. Run brew upgrade --cask folio.` | **Copy command** · **Close** |

The first verb is **Download and install**, not "Update and restart": the press
downloads, and the restart is asked for again at `Verified`. The owner's three
verbs are preserved in shape; only the first one's wording is made honest.

**Later** writes nothing; the dot stays as it was, and the card returns next
launch. From `Verified`, **Later** keeps the staged transaction and leaves a
**Restart to finish updating** entry on the General row's picker foot, so the work
is not stranded and not silently discarded; the staging is swept if it is still
unfinished two launches later. **Skip this version** writes `skipped_tag` **and**
`seen_tag`; it is compared by **precedence**, not equality, so a withdrawn release
does not re-offer an older-but-still-newer tag. Turning the switch off cancels an
in-flight job and suppresses cached offers.

**Cancel / window close / process death during `Downloading` or `Staged`** touches
nothing installed: the job lives under its own transaction directory and is swept
by the startup pass of §C.6. Closing the presenting window does not cancel a job
that other windows can still be told about; it re-presents on the next active
window. There is no reachable state in which a card verb exists with no handler:
the enumeration above is total and is gated by a test.

## C. The replacement, as a recovery contract

### C.0 Actors, and the one invariant

| | Who | Runs from |
|---|---|---|
| **O** | the old, running Folio | the installed set |
| **P** | the **applier** — the *staged* new executable, run with `--update-apply <journal>` | the staging directory, which is never part of the install set |
| **N** | the new Folio after the flip | the installed set |
| **R** | recovery — the first few milliseconds of *any* Folio start, plus a Windows logon hook | wherever it was started from |

O never performs the destructive step. It cannot: the review's R-4 requires the
cancelable quit to finish first, and after that O is a process on its way out. P is
a real, separate, already-verified executable, and the flip cannot touch it.

> **Invariant I1.** From the instant the first destructive call is made until
> health is acknowledged, at least one **complete** copy of the install set exists
> at a path the on-disk journal names, and the journal says which phases are
> possible. Recovery never remembers anything; it looks at what is on disk.

### C.1 The journal, the lock, and where they live

`<install-root>/.folio-update/` — beside the installed files, on the destination
volume, reachable without the data directory. `<install-root>` is the folder
holding `folio.exe` on Windows and the bundle's parent on macOS. It holds
`journal.json` (fsynced before every phase change, written through a temporary file
and an atomic rename), `lock`, `staging/`, and `backup/`.

```
{ v:1, txn, install:{path, volume_id, file_id}, from_version, to_version, tag,
  files:[…], phase: Prepared|Flipping|Flipped|RollingBack|RolledBack|Failed|Healthy,
  health:{pid, version, at_ms}|null, attempts, last_error }
```

**The lock is installation-scoped, not data-directory-scoped** (R-7). Windows: an
exclusive `CreateFileW` on `lock` with no sharing; Unix: `flock(LOCK_EX|LOCK_NB)`.
The data-directory claim does not serialize this: two isolated `APPDATA`s, or two
logon sessions, address the same binaries. Install identity is the canonical
volume + file id of `<install-root>` recorded in the journal, so a folder moved
between phases is detected and the transaction is failed rather than applied to a
stranger. On macOS every path is derived from the **actual bundle basename**, so a
`Folio Test.app` updates itself and not `/Applications/Folio.app`.

### C.2 Prepare — nothing installed is touched (K, the worker thread)

1. Take the installation lock. Held → the card says another copy of Folio is
   already updating this installation; no download.
2. Create `<install-root>/.folio-update/staging/<txn>/` with exclusive creation.
   **This is on the destination volume by construction**, which is what makes the
   flip a rename rather than a cross-volume copy (R-6). The *download cache* may
   be under `persist::storage_dir()`; the staged tree may not.
3. Reserve space for archive + expansion + a complete backup set; refuse with a
   sentence naming the shortfall.
4. Download the asset (§E) into the cache. Hash it against the release's checksum
   document, fetched by the **same offer's tag**.
5. Expand / attach, and verify **identity, not merely validity** (§E).
6. Copy the verified tree into `staging/<txn>/`, flush every file and the
   directory, then **re-verify the staged copy in place** — signature, version,
   architecture — because what gets installed must be what was checked.
7. Journal `Prepared`, fsync. Card → `Verified`.

Every failure here deletes the transaction directory, releases the lock, and
reports `Failed(… )` with *Nothing installed was changed.*, which is true.

### C.3 The barrier — the ordinary quit runs to completion first

On **Restart now**, O begins the ordinary quit (`crates/bt-app/src/quit.rs:147-174`,
driven at `main.rs:110990-111085`) with a reason of `UpdateRestart`. Every
cancelable step runs unchanged: `Ask` over dirty documents, `Save`/`Discard`,
`Photograph`, `Write`.

- `QuitStep::Abandon` — the reader chose Cancel, or `flush_judged` reported a
  failed write — **abandons the update too**. Journal stays `Prepared`, card
  returns to `Verified` naming the reason. This is the whole of R-4.
- The session write is the store's own `flush_judged` on **W**, but W must not
  block on it: W requests a named generation, keeps pumping events, and acts on the
  receipt. `SessionWriter::wait_for` is an unbounded `recv`
  (`crates/bt-app/src/persist.rs:365-385`), which is why the update path uses a
  deadline and treats expiry as `Abandon`, never as success.
- From `Photograph` onward no new session mutation is admitted and
  `launch_wire` requests are refused, so the document that landed is the document
  that comes back.
- At `QuitStep::Exit`, and only after `written(true)`, O spawns P detached and then
  exits by the ordinary path. A spawn failure there journals `Failed` and exits
  anyway — nothing has moved, and the next start says so.

### C.4 The flip

P first waits for O to be gone: it holds the installation lock, polls O's process
handle, and — the authoritative test — polls `bt_platform::instance::claim_data_directory`
until it succeeds, then **immediately releases it** and proceeds, because P is not
the writer and must not become one. If 60 s pass, P journals `Failed`, deletes
staging, and exits without touching anything.

**macOS is atomic and needs no repair.** One call:

```
renamex_np("<parent>/<Name>.app", "<staging>/<txn>/<Name>.app", RENAME_SWAP)
```

Both paths are on the same filesystem by construction (C.1), so the exchange is a
single atomic operation: afterwards the launch path holds the new bundle and the
staging path holds the old one, which *is* the backup. **There is no interrupted
state.** The journal still moves `Flipping` → `Flipped` around the call so recovery
can tell which side it died on; recovery decides by reading the installed bundle's
version, not by trusting the phase.

**Windows has no atomic multi-file exchange**, so it gets the journal, the applier,
and a logon hook. P, before the first destructive call, writes
`HKCU\…\CurrentVersion\RunOnce\FolioUpdateRecovery` = `"<staging>\<txn>\folio.exe"
--update-recover "<journal>"` — a path outside the install set — and journals
`Flipping`, fsync. Then:

1. For each of the nine, in a fixed order: `MoveFileExW(<install>\<n>,
   <backup>\<n>, MOVEFILE_WRITE_THROUGH)`.
2. For each of the nine: `MoveFileExW(<staging>\<txn>\<n>, <install>\<n>,
   MOVEFILE_WRITE_THROUGH)`.
3. Journal `Flipped`, fsync.

`ReplaceFileW` is not used: it needs an existing target, and step 1 has removed it.
Retry policy is its own, stated rather than borrowed: a sharing violation or an
access denial retries 10 times over 5 s — `persist.rs:32-47` is five *write*
attempts on a 1.5 s debounce and is not this. A rename that still fails rolls the
transaction back and journals `Failed`.

**I1 holds through every instant of this.** Before step 1 the install set is
complete; during step 1 `staging` is complete; during step 2 `backup` is complete;
after step 2 the install set is complete again. Every one of those paths is named
in the journal, and `staging\<txn>\folio.exe` is a verified executable that the
RunOnce entry can run when the install set cannot.

### C.5 Health, and only then deletion

P launches `<install>\folio.exe --update-health <journal>` (macOS: `open -n -a
<parent>/<Name>.app --args --update-health <journal>`) and waits **90 s**.

Health is not a spawn and not `--version`. N must reach the point where it has
**claimed the data directory** (C.7), read settings and session without a
future-schema refusal (`crates/bt-persist/src/migrate.rs:836-842` already refuses a
newer schema), and drawn its first pane text — the same `first_text_present`
criterion `docs/plans/release/clean-vm.md:611` already uses. Then N writes
`health {pid, version, at_ms}` into the journal, fsyncs, and signals P.

Any Folio of `to_version` starting from this installation satisfies the criterion,
including a launch the reader started by hand, so a race with a manual launch
**completes** the transaction instead of failing it.

- **Health arrives** → P deletes exactly the recorded backup set — never an
  arbitrary `*.old` glob — removes the RunOnce entry, journals `Healthy`, removes
  the journal, releases the lock.
- **N exits, crashes, or the deadline passes** → P journals `RollingBack` and
  reverses the flip from what is on disk (macOS: the same `RENAME_SWAP` back). It
  then relaunches the restored old build with `--update-failed <journal>`, which
  raises the card at `Failed` with *The previous version was put back.*
- **Rollback itself fails** → journal `Failed` with `last_error`, backups kept, the
  RunOnce entry kept. The card's sentence is then *Folio could not finish
  replacing itself; see* the path. There is a third outcome and it is named, rather
  than being reported as "nothing was changed".
- A backup file another process holds open is **retained and reconciled**, not
  deleted and not treated as success.

### C.6 Recovery at start, idempotent by construction

Every Folio start, before the single-instance fork at `main.rs:118196-118201`,
off the window thread:

1. Read `<install-root>/.folio-update/journal.json`. Absent → done.
2. Take the installation lock, non-blocking. Held → an actor owns this
   transaction; do nothing at all.
3. Verify `install.volume_id`/`file_id` still name this folder. They do not → fail
   the transaction and leave the files alone.
4. Decide from **what is on disk**, never from a remembered index:

| Phase found | Disk | Action |
|---|---|---|
| `Prepared` | — | delete staging + cache, remove journal |
| `Flipping` | install set complete at `to_version` | continue as `Flipped` |
| `Flipping` | install set complete at `from_version` | delete staging, remove journal |
| `Flipping` | install set incomplete | roll **forward** if staging is complete, else roll **back** from backup; I1 guarantees one is |
| `Flipped`, no health | this process is `to_version` | write health, delete the recorded backups, remove journal |
| `Flipped`, no health | this process is `from_version` | roll back |
| `RollingBack` | — | finish the rollback |
| `Failed` / `RolledBack` | — | show the sentence once, then remove the journal |

Every row is a re-evaluation, so recovery interrupted during recovery is simply
recovery again. The Windows `RunOnce` entry is what makes this reachable when the
install set has no runnable executable; on macOS nothing equivalent is needed
because C.4 is atomic. **The RunOnce key is the one thing this feature writes
outside the data directory**, it exists only between the flip and health, and it is
removed by whichever actor finishes the transaction — the owner should be told
this, on §7.56's measure.

Staging under a transaction directory is created exclusively, never followed
through a link out of its own root, and swept only when it is provably abandoned:
a `Prepared` journal with no live lock holder, two launches old.

### C.7 The claim, adopted without a gap

`crates/bt-app/src/persist.rs:223-233` caches `Option<DataDirectoryClaim>` in a
process-wide table with `or_insert_with`, so **a single failed attempt is
remembered forever**. A child that acquired its own claim outside that table would
then be told by `is_writer_of` that it is not the writer, and would hand itself off
to nobody. Rev 1's `--await-exit` walked straight into this.

Two changes, both in `persist.rs`:

- `try_claim(dir) -> Option<Claim>` calls `bt_platform::instance::claim_data_directory`
  **without caching a refusal**.
- `adopt_claim(dir, claim)` inserts an already-acquired guard into the same table
  under `instance::claim_name(dir)`, before any other code asks `is_writer_of`, so
  the guard is never dropped and there is no window in which a third process can
  take it.

N, started with `--update-health`, retries `try_claim` for up to 30 s and adopts on
success. **On timeout it does not start anyway and does not hand over** — it exits
non-zero, P observes that, and P rolls back. Handing over would put a tab in some
other window and call an unfinished update a success. Distinguish "the process is
gone" from "the query was denied": on Windows an `OpenProcess` refusal is not
death, and pid reuse is why the claim, not the pid, is the authority. The
transaction id and the canonical data-directory path are carried across the
relaunch on the command line so N cannot acknowledge the wrong journal.

`launch_pipe.rs:111`'s 2 s `HANDOVER_BUDGET` is an IPC deadline and is never
evidence that the old process has exited.

## D. Where it is installed — ownership, not writability

Classification is **read-only**. Nothing in §B's eligibility test may move a file;
rev 1's "rename `folio.exe` to find out" both mutated before consent and confused a
sharing violation with a policy refusal.

| Class | How it is told (read-only) | Behaviour |
|---|---|---|
| **Ours** | `<install-root>` is writable by probe *file creation* in `.folio-update/`, no manager receipt claims it, identity resolves | the full transaction |
| **Homebrew** | a cask receipt for `folio` names **this** bundle path (the tap is `lulu-loopp/homebrew-folio`, `docs/RELEASING.md:1003-1007`) | `brew upgrade --cask folio`, **Copy command** |
| **winget** | a winget package receipt names **this** root; the identifier is `WeiyiShi.Folio` (`packaging/winget/…/WeiyiShi.Folio.installer.yaml:3`) | `winget upgrade --id WeiyiShi.Folio --exact`, **Copy command** |
| **Not writable** | probe creation refused | releases page |
| **Unknown** | any classification step failed, or receipts disagree | **releases page** — unknown fails safe |
| **Not updater-capable** | the build says so | check, dot and row unchanged; no card |

A bare directory *named* `Microsoft\WinGet\Packages`, or a leftover Caskroom
directory for a Folio that is not this one, must not decide anything: a false
positive costs a command that does nothing, a false negative mutates a
manager-owned install and desynchronizes its receipts. So the test is "does a
receipt name **this** canonical path", with symlinks and custom prefixes resolved,
and anything short of an answer is **Unknown**.

**Updater capability.** Rev 1 proposed `version::CHANNEL` set by the packaging
step. That cannot work: `build.rs` embeds at compile time (`crates/bt-app/build.rs:36-37`)
and packaging copies an already-built binary (`package.ps1:154`,
`.github/workflows/release.yml`), so an environment variable set during packaging
changes nothing in the bytes. The honest form is a compile-time capability set by
the **release build invocation** — `rerun-if-env-changed`, exact-value parsing, set
in `release.yml`'s cargo step and in `RELEASING.md`'s local instructions, verified
by `smoke.ps1` — and it is a *capability*, not a provenance claim: copied bytes
carry it, so it must never be described as "this is a released build". A candidate
build simply does not set it. No binary-only signal can distinguish identical
copied bytes, and the design does not pretend otherwise.

## E. Verification

**Both requests use the offer's captured tag**, never `latest` and never a tag
reconstructed from `VERSION`/`RELEASE_TAG`; the tag is validated and percent-encoded
before it enters a URL. A mutable release can have its asset and its checksum
document replaced together, in which case SHA-256 agrees and the binding is worth
nothing on its own — rev 1's claim that mismatched pairs "would both verify" was
simply wrong; they fail the hash. So the hash is the **integrity** check and
**identity** is checked separately and is what actually decides:

- **Windows.** Full Authenticode trust on the new `folio.exe` *and* `folio.msix`:
  chain validity, revocation, and an RFC 3161 timestamp. The signer's subject is
  compared to the **running** executable's by `msix::distinguished_name` /
  `publisher_matches_subject` (`crates/bt-platform/src/msix.rs:121-177`), which
  parses RDNs and preserves value case — never a substring match, which is all
  `smoke.ps1:351-361` does. The msix manifest's `Publisher` is compared to its own
  verified signer. `conpty.dll` and `OpenConsole.exe` are Microsoft's and are
  verified as `package.ps1:450-464` verifies them. The embedded `VERSIONINFO`
  version and the machine architecture must match the offer. A certificate renewal
  with the same subject passes; a changed subject requires an explicit migration
  and is a refusal until then.
- **macOS.** `codesign --verify --strict --deep` plus an explicit **requirement**
  naming Folio's Team ID and bundle identifier, on the **mounted source and again
  on the destination copy**. Gatekeeper acceptance is checked in addition, not
  instead: `spctl` answers "some notarized Developer ID", which is not "Folio", and
  on a machine with `spctl --master-disable` it answers nothing useful. The
  destination is created fresh and exclusively; a pre-existing `.new` is never
  written into. `CFBundleShortVersionString` and architecture must match the offer.
- **Quarantine is preserved, never stripped.** Rev 1 asserted a downloaded file
  carries no quarantine; that is an untested assumption. The design instead
  preserves the attribute through download, mount, copy and launch, and reports an
  assessment failure rather than deleting the attribute. Mount ownership is
  recorded in the journal and detached on every success, failure and cancellation
  path; a failed detach is cleanup debt, not a reason to undo a healthy install.

**Redirects are expected and already safe.** WinHTTP follows up to ten hops and
never HTTPS→HTTP (`crates/bt-platform/src/http.rs:39-51`); the macOS arm permits
only HTTPS and at most ten (`macos_http.rs:311-329`). "One GET" means one logical
fetch, and the whole-call deadline spans the redirect chain.

**`https_download`** is a new function beside `https_get` in all three arms, keeping
every discipline `http.rs` already states — one GET, HTTPS only, no caller headers,
the platform's own proxy and trust store, no configuration of our own. What it adds:
a fixed-size buffer and a bounded file sink rather than a growing `Vec`
(`http.rs:226-260`, `macos_http.rs:391-402` both accumulate today); a checked byte
counter enforced **before** each chunk is written, against a 200 MB cap; unknown
length treated as `None` — macOS's negative `expectedContentLength` sentinel must
never become a huge unsigned number — driving an indeterminate bar; verified EOF or
exact known length before success; short writes, ENOSPC and flush/close failures
all failing rather than reaching `Verified`; a monotonic end-to-end deadline that
periodic bytes cannot defeat; and cancellation with a stated latency during DNS,
headers, body, hashing and expansion, not merely between chunks. Progress carries
**at most one pending wake**, so a stalled W cannot accumulate a backlog. Its
timeouts are its own: 30 s idle, 10 min end-to-end.

**The archive's real layout.** `package.ps1:472-497` calls `CreateFromDirectory(…, $true)`,
so the zip contains `folio-<version>/<name>` — a versioned root, spelled with the
**manifest** version and no `v` or `-preview` — and `docs/RELEASING.md:682-685`
documents it. The extractor accepts exactly one expected root and its nine regular
children, strips the root, and refuses duplicates, case-collisions, absolute, drive
or UNC paths, traversal, alternate data streams, links, reparse entries, extra
roots, and any entry or total exceeding its expanded-size bound.

## F. Tests

CPU-only, deterministic, over fake clocks, processes, transports, locks and a **durable** fake filesystem that can be cut at any operation and re-opened. The review's list at R-22 is adopted essentially whole; these are its groups, and every name is one gate.

- **Recovery.** `every_crash_boundary_recovers_a_complete_launchable_install` — one case per row of §C.6 and per instant of §C.4 — `recovery_can_itself_be_interrupted`, `rollback_failure_preserves_journal_and_backups`, `a_moved_installation_fails_the_transaction_without_touching_files`.
- **Health.** `spawn_without_health_never_commits`, `early_crash_or_health_timeout_restores_old_build`, `a_manual_launch_of_the_new_build_satisfies_health`, `unacknowledged_backups_are_never_swept`, `unknown_old_files_are_never_deleted`, `a_future_schema_refusal_fails_health`.
- **Claim.** `acquired_claim_is_adopted_without_a_gap`, `transient_claim_refusal_is_not_cached`, `await_timeout_never_hands_over_or_starts_a_nonwriter`, `manual_launch_and_relaunch_have_one_writer`.
- **Locking.** `two_data_directories_share_one_install_lock`, `locked_backup_blocks_reuse`, `staging_is_always_on_the_destination_volume`, `renamed_bundle_updates_only_itself`.
- **Quit.** `quit_cancel_or_failed_save_prevents_swap`, `close_during_each_phase_preserves_recovery`, `stale_progress_cannot_revive_a_cancelled_job`, `session_receipt_matches_the_final_snapshot`.
- **Archive.** `packaged_zip_root_is_accepted` over a fixture built to `package.ps1`'s real layout, `duplicate_case_alias_stream_link_and_traversal_entries_are_refused`, `expanded_size_limit_precedes_disk_exhaustion`.
- **Identity.** `offered_tag_survives_latest_changes`, `mutated_asset_hash_pair_refuses_before_swap`, `validly_signed_wrong_product_or_version_is_refused`, `dn_values_preserve_case_and_order`, `timestamp_policy_accepts_the_packagers_format`.
- **Transport.** `unknown_length_stream_is_bounded`, `https_redirects_work_and_http_redirects_refuse`, `redirect_loop_and_trickle_body_hit_deadlines`, `cancel_interrupts_idle_wait`, `short_write_disk_full_and_flush_failure_never_verify`, `progress_queue_has_one_pending_wake`.
- **Offer.** `skip_racing_check_and_seen_keeps_all_fields`, `skip_failure_is_not_reported_as_persistent`, `cached_skipped_tag_stays_hidden_inside_daily_cadence`, `newer_tag_is_offered_after_skip` (by precedence, not inequality), `switch_off_suppresses_cached_and_inflight_offers`, `verified_later_has_a_resume_or_discard_path`, `manager_receipt_must_match_this_install`, `unknown_ownership_fails_to_the_page`, `every_card_state_has_a_handler_for_every_verb`.

**`update-check.json` goes to schema v2** with `skipped_tag`, the first entry `UPDATE_CHECK_MIGRATIONS` has ever carried (`crates/bt-persist/src/migrate.rs:836-842`). All four fields move through **one owner holding one lock across the whole read-modify-write**: `update.rs:479-482` re-reads before writing, but a Skip landing between that read and its write is still lost, and atomic file replacement does not make read-modify-write atomic.

**The clean-machine plan.** `docs/plans/release/clean-vm.md` gains a `### 4.4 更新器` checklist beside its per-machine tables at `:601-656` and rows in its `## 8` known-gaps table. It **cannot start from 0.4.1**: that build ships no updater and rejects the new flags (`crates/bt-app/src/cli.rs:406-407`), and an anonymous client cannot see a draft release (`update.rs:95-98`). So the baseline is a **signed, notarized, updater-capable release candidate** updating to a second one, both published accessibly; corrupt-checksum and wrong-signature cases use a test-only injected release source, never an edited public asset. The run covers: a usable restored multiwindow startup (`first_text_present`, not mtimes); immediate child death; a hung child; claim timeout; power loss forced at each boundary of §C.4; a locked rollback file; staging on a second volume; each manager-owned class; the quarantine path; a failed session save; and a cancelled quit. Deletion assertions are conditional on `Healthy`, never on "the second start". Identity is read from the installed bytes.

## G. Stage 2 — one core, then two drivers, each behind its own gate

| # | Ticket | Files | Size |
|---|---|---|---|
| 0 | **Capability and claim groundwork.** the compile-time updater capability in the release build invocation and its verification; `try_claim`/`adopt_claim`; the installation lock; the journal type and its recovery decision table as a pure function | `crates/bt-app/build.rs`, `src/{persist,version}.rs`, `crates/bt-platform/src/instance.rs`, `.github/workflows/release.yml`, `scripts/release/{package,smoke}.ps1`, `docs/RELEASING.md` | **M** |
| 1 | **The shared core.** offer/job, `should_offer`, the card and its states, `update-check.json` v2 under one lock, `https_download` in all three arms, the archive reader, the quit barrier, stations, `AppEvent` arms, the startup recovery pass | `crates/bt-app/src/{update,update_card,update_job}.rs`, `{main,cli,i18n,settings,persist,quit,hang_watch}.rs`, `crates/bt-persist/src/{update,migrate,lib}.rs`, `crates/bt-platform/src/{http,http_portable,macos_http}.rs` | **L** |
| 2 | **Windows driver.** trust decision, flip, RunOnce hook, rollback, msix registration policy, winget receipt classification | `crates/bt-platform/src/{trust,msix}.rs`, `crates/bt-app/src/update_apply_windows.rs` | **L** |
| 3 | **macOS driver.** `renamex_np` flip, mount lifecycle, `codesign` requirement, Caskroom receipt classification | `crates/bt-platform/src/macos_app.rs`, `crates/bt-app/src/update_apply_macos.rs` | **M** |

**No panic stub.** `todo!()` panics and cannot "report Failed". Ticket 1 ships with
a typed `Unsupported` returned **before** any network, staging, flush, wait or
mutation, and with automatic install offers **capability-gated off**, so what ships
is today's behaviour exactly. A visible "Download and install" that then admits no
driver exists is misleading even when it is harmless, so the verb is not shown
until its platform's integration gate passes. Both drivers have injectable cores
and their own CPU tests; ticket 1 is not the only testable one.

**Chinese, by opus46, after the English lands**: `Text::RowUpdateCheck` (renamed),
`Text::DescUpdateCheck` (rewritten — it must now say Folio can install a new
version and never does so without a press), `update_card_available_in`,
`update_card_progress_in`, the five verbs (`Download and install`, `Later`,
`Skip this version`, `Cancel`, `Restart now`), `Text::UpdateWarnTabsClose`,
`Text::UpdateRestartToFinish`, the failure sentences of §B including the three
distinct outcomes of §C.5, and `Text::UpdateUseBrew` / `UpdateUseWinget` /
`UpdateFolderNotOurs` / `UpdateOwnerUnknown` / `CopyCommand`. Each is a `Text`
variant with a `pick(lang, en, zh)` arm and must be added to the completeness array
the crate's tests walk (`crates/bt-app/src/i18n.rs:5620` onward).

## H. Risks, and the experiment that answers each

| Risk | Experiment |
|---|---|
| **`renamex_np(RENAME_SWAP)` on a bundle** — the atomicity claim C.4 rests on. Behaviour with a running process's image inside the swapped directory, and on a non-APFS volume, is not established here | swap a running bundle on APFS and on HFS+, with and without the process live; assert both paths afterwards and that the running process survives to exit |
| **Renaming a running exe on a OneDrive-synced folder.** `persist.rs:272` already records OneDrive costing a second and a half on a write; an online-only placeholder may refuse the rename | run the flip in a synced folder, hydrated and as a placeholder; measure latency and the failure code. A refusal is §D's Unknown/not-writable arm, not a workaround |
| **`RunOnce` survivability** — whether the key is honoured after a hard power cut mid-flip, and whether security software strips it | force power loss at each boundary of C.4 on the Win11 clean VM and observe the next logon |
| **Quarantine through the whole path** | `xattr -l` at every step of C.2–C.5 for a real downloaded dmg; if it appears, find which step applies it |
| **A manual launch racing health** | start Folio by hand during the 90 s window, repeatedly, and assert the transaction completes rather than rolling back a healthy install |
| **200 MB / 10 min / 90 s / 60 s are estimates** | measure the real asset sizes at the candidate release and the download time on a throttled 1 Mbit link; a release that outgrows the cap must fail the release gate, not the reader's machine |
| **MSIX registration after a version change** | on the Win11 VM with the first-page verb on, run a full transaction and read `msix::registered()` before and after. A repair failure must be visible, must not claim nothing changed, and must not leave a broken registration silently enabled |

## I. Review ledger

- **R-1 (blocker) — done.** §C is rebuilt around invariant I1, a fsynced journal, an applier that runs from outside the install set, a Windows `RunOnce` recovery entry that does not depend on the replaced executable, an atomic macOS flip that removes the failure mode entirely, and the idempotent decision table of §C.6. Every row of the review's boundary table is a §F fault-injection case.
- **R-2 (blocker) — done.** §C.5: health is claim + schema + `first_text_present`, not spawn. Backups are transaction-named, deleted only at `Healthy`, never by glob, retained when locked. A failed rollback is a third, named outcome; §B's "Nothing installed was changed" is used only where it is true.
- **R-3 (blocker) — done.** §C.7: `try_claim` + `adopt_claim` close the caching hole at `persist.rs:223-233`; timeout fails the update explicitly and never hands over; pid reuse and query-denial are distinguished; txn id and canonical directory travel on the command line. `--await-exit` is gone.
- **R-4 (blocker) — done.** §C.3: the ordinary `quit::Quit` runs to completion first; `Abandon` and a failed `Write` abandon the update; the job is application-owned, so closing a window strands nothing; the spawn is at `Exit`.
- **R-5 — done.** §E: one `folio-<manifest version>/` root, nine children, stripped; the full refusal list and expanded-size bounds; the fixture is built to `package.ps1`'s real layout.
- **R-6 — done.** §C.1–C.2: staging is inside `<install-root>`, so same-volume by construction; space reserved for archive, expansion and rollback; §D's probe is read-only file creation, never a rename; rename retry policy stated as its own.
- **R-7 — done.** §C.1: an installation-scoped lock keyed by canonical volume+file id; macOS paths derived from the real bundle basename; `.old`/`.new` ownership recorded.
- **R-8 — done.** §B/§E: an immutable offer carries the tag; both requests use it; the tag is validated and encoded; the "both would verify" sentence is withdrawn and identity, not the hash, carries the product claim. Whether this repository enables immutable releases is **open and flagged for the owner**.
- **R-9 — done.** §E: full chain + revocation + RFC 3161; parsed-DN comparison against the running executable; msix publisher against its own signer; both Microsoft sidecars verified; version and architecture checked; renewal policy stated.
- **R-10 — done.** §E: an explicit requirement naming Team ID and bundle identifier, on source and on the fresh destination copy, independent of Gatekeeper. Not conditional on any experiment.
- **R-11 — done.** §E: quarantine preserved, never stripped; mount ownership in the journal with detach on every path; failed detach is debt; tool exit status and bounded runtimes required.
- **R-12 — done (note).** §E states the existing redirect rules, keeps them in both arms, defines "one GET" as one logical fetch, and §A rewrites the privacy claim to include redirected asset hosts and Gatekeeper's own network activity.
- **R-13 — done.** §E: fixed buffers, bounded sink, cap checked before each write, unknown length as `None` with the macOS sentinel named, EOF/length completeness, short write and ENOSPC handling, monotonic deadline, cancellation latency per phase, one pending progress wake.
- **R-14 — done.** §C.6: an off-thread startup recovery pass before any job; recovery data separated from disposable cache; unique exclusive transaction directories; no link-following out of root; sweep only provably abandoned work; failed deletion is retriable debt. The path is named once, relative to `<install-root>`, which also removes rev 1's ambiguous `<data>/Folio`.
- **R-15 — done.** §D: renamed to a *capability*, set in the release **build** invocation with `rerun-if-env-changed` and exact-value parsing, verified by `smoke.ps1`, listed in ticket 0's files. The design states plainly that copied bytes carry it and that no binary-only signal distinguishes identical copies.
- **R-16 — done.** §F: one owner, one lock across the whole read-modify-write, all fields retained, one coherent snapshot published and every window repainted; a failed Skip is surfaced; comparison is by **precedence**; a cached skipped tag stays hidden inside the daily cadence; one process-level offer.
- **R-17 — done.** §B: the switch gates cached and in-flight offers; manager-owned classes get an informational card, not an installable one; the highlight line's source is bounded — the check stores only `tag_name` today, so the line is fetched with the offer or omitted, and it is **omitted by default** pending the owner's word; Escape/close mean Later; the once-per-launch gate is not consumed by a suppressed offer; `Verified → Later` has a named resume entry; the first verb is relabelled **Download and install**; Later does not relight an acknowledged dot.
- **R-18 — done.** §C.3: W requests a named generation and keeps pumping, with a deadline that ends in `Abandon`; all disk, network, signature and swap work is on K or in P; distinct stations for offer/dispatch, progress, outcome, session barrier and quit transitions, restored on early returns; both new `AppEvent` arms get `station()` rows at `main.rs:588-609`; K has its own phase diagnostics and never stamps W's heartbeat.
- **R-19 — done.** §D: `winget upgrade --id WeiyiShi.Folio --exact`; receipts must name **this** canonical root; symlinks and custom prefixes resolved; an explicit **Unknown** class that fails to the releases page; writability is never treated as ownership.
- **R-20 — done.** §C/§H: the function is `msix::registered()`; since `PackageRegistration` carries no version (`msix.rs:242-250`), ticket 2 must expose the package version and compare it typed against the manifest's four-part number, not against three-part `version::VERSION`. Re-registration is deferred until `Healthy`, rollback restores the previous registration and refuses a downgrade, and a repair failure is visible and never reported as "nothing changed".
- **R-21 — done.** §F: the baseline is a signed, notarized, updater-capable candidate, not 0.4.1; corrupt cases use an injected test source, never edited public assets; `first_text_present` replaces mtimes; deletion assertions are conditional on `Healthy`; the full interruption and ownership matrix is listed.
- **R-22 — done.** §F adopts the list, grouped, plus `a_moved_installation_fails_the_transaction_without_touching_files`, `a_manual_launch_of_the_new_build_satisfies_health`, `unknown_ownership_fails_to_the_page` and `every_card_state_has_a_handler_for_every_verb`.
- **R-23 — done.** §G: a new ticket 0 establishes capability, claim, lock and journal before any driver; ticket 1 returns a typed `Unsupported` before any network, staging, flush, wait or mutation, with offers capability-gated off and no `todo!()`; each driver has its own integration gate and its own injectable core; build and release scripts and `docs/RELEASING.md` are in ticket 0's file list.

**Open for the owner**, both new in rev 2: whether GitHub's immutable-releases setting is enabled on this repository (R-8), and whether the highlight line is worth a second request per offer (R-17).

---

## Revision 2026-09-25 for 0.4.6

Appended; nothing above is edited. Written against `design/self-update` after
merging `main` at `f7826bd4` (the merge is `48723656`). Documents only: nothing
was built, run, signed or installed. Where this section and §A–§I disagree, this
section rules; where it is silent, §A–§I stand.

### R.0 The rulings and facts this revision folds in

| date | ruling (owner) | what it does here |
|---|---|---|
| 2026-09-16 | `update.rs` may download; the macOS §M4 deferral no longer holds | unchanged — the basis of §A |
| 2026-09-20 | **0.4.6 = the updater.** The right-click menu is on by default **only in installs that have an uninstall hook**, told by an **install-time marker file, never a path guess**; **managed installs (scoop, winget, Homebrew) do not self-update** | §D's receipt lookups are replaced by the marker (R.2 C2); the Explorer default follows the marker (C3) |
| 2026-09-20 | **Install and uninstall must be effortless**: anything Folio writes outside its own folder needs a UI-less undo | the ledger in R.4; the Windows download moves into Folio's own folder (C6); the RunOnce value gets a `--uninstall-cleanup` row (C5, U-17) |
| 2026-09-23 | **Fewer words in the UI** | the card is rewritten in C9 |
| 2026-09-25 | **0.4.6 code merges only after the 0.4.5 tag** | every ticket in R.6 ends at "committed, CI green on the branch" |

Facts taken as given: the Windows executable is compiled by `build-release.yml`
and signed on the owner's machine through Artifact Signing (`sign.ps1`;
certificates valid three days; every signature time stamped; CI and
`smoke.ps1 -ExpectSigned` check it); the macOS build is Developer ID signed with
the hardened runtime, notarized and stapled (`sign.sh`, `notarize.sh`,
`dmg.sh`); a release page carries `folio-<version>-windows-x64.zip`,
`folio-windows-x64.zip`, `SHA256SUMS.txt`, `folio-<version>.cdx.json`,
`Folio-<version>-macos-arm64.dmg`, `Folio-macos-arm64.dmg` and
`SHA256SUMS-macos.txt`, with bare file names in both checksum files; the sparse
`folio.msix` travels inside the zip, not as an asset; scoop and Homebrew
manifests are rendered by `update-manifests.ps1` / `cask.sh` after the page
exists; `/releases/latest/download/<stable name>` resolves; the app keeps
`diagnostics.log` and a stall self-report (`hang_watch`).

### R.1 What in the 2026-09-16 design still stands

- **§A** — the check is unchanged: one `GET` of the releases list, at most once a
  day across windows, silent on failure, `User-Agent: Folio`. The updater starts
  where the check ends. `update.rs` on `main` is still exactly what §A
  describes (no download code).
- **§B's state machine** — one application-owned job, an immutable offer
  captured when it is raised, `Idle → Available → Downloading → Staged →
  Verified → Quitting → Committing`, Later / Skip / Cancel semantics, Skip by
  precedence, switch-off cancels. Only the card's words change (C9) and the
  managed arm leaves the card (C2).
- **§C's recovery contract** — invariant I1, the fsynced journal, the applier P
  running from outside the install set, health = claim + schema + first pane
  text, deletion only at `Healthy`, the idempotent decision table of §C.6, the
  claim adoption of §C.7, the quit barrier of §C.3. Two corrections (C5, C6)
  and one macOS residual (C7) are the only changes.
- **§E** — both requests use the offer's tag; the hash is integrity, the
  signature is identity; bounded streaming download; quarantine preserved; the
  redirect rules. The Windows identity paragraph is sharpened in C8, the macOS
  one replaced in C7.
- **§F** — the test list is kept whole; R.6 distributes it over tickets.
- **§H** — every experiment stays; R.5 adds five.
- **Velopack stays rejected** (`design/velopack-spike`, 2026-09-16): its Windows
  applier has no journal, deletes its backup on the failure path, has no health
  acknowledgement, and kills every process in the install folder sixty seconds
  into a quit. Two things are still taken from it: `renamex_np(RENAME_SWAP)` for
  the macOS flip (already §C.4), and `apply_windows_impl.rs` as the worked
  example of what the review warned about.

### R.2 What changed

**C1. Updater capability is the running build's own signature, not a build
flag.** §D (answering R-15) put a compile-time capability into the release build
invocation. Since then the shipped Windows executable is compiled by
`build-release.yml` for candidates and releases alike, from one command
(`cargo build --release --locked -p bt-app`, `docs/WINDOWS-CI-RELEASE.md`), and
what makes a release a release happens *after* compilation, on the owner's
machine: `package.ps1 -Sign`. The CI artifact a candidate is made from is
unsigned ("keep the unsigned original"). On macOS a candidate bundle is ad-hoc
signed (`sign.sh` defaults `--identity -`) and only a release is Developer ID
signed and notarized. So the one fact that separates the two is already in the
bytes, and the updater needs it anyway, because identity (§E) is a comparison
**against the running build's own signer**:

- **Windows:** capable ⇔ `WinVerifyTrust` accepts the running `folio.exe` with a
  time-stamped signature and its leaf subject parses as a distinguished name.
- **macOS:** capable ⇔ the running bundle carries a valid signature whose
  designated requirement is a Developer ID requirement (not ad-hoc).

It is a capability, not a provenance claim, as R-15 asked: a signed copy anywhere
is capable, and that is correct, because what the updater establishes is "the
successor is the same publisher's Folio", not "this copy came from the page". It
removes rev 2 ticket 0's changes to `build.rs`, `release.yml`, `package.ps1` and
`smoke.ps1`. A signed candidate, should one ever exist, would update to the next
public release — Q4.

**C2. Managed-install detection is the install marker, not a receipt or a
path.** The 2026-09-20 ruling and `docs/RULES.md` §41 ("How Folio was installed
is read from a written channel marker, never inferred from a path") replace §D's
Caskroom and winget-receipt lookups. Nothing writes or reads a marker on `main`
(`clean-uninstall-2026-09-20.md` §6.7 scheduled it as T-C2; it was not built —
`explorer_menu.rs` mentions only a withdrawn `pre_uninstall`).

| | Windows | macOS |
|---|---|---|
| **where** | `folio-install.json` beside `folio.exe` — inside Folio's own folder | an extended attribute `io.github.lulu-loopp.folio.install` on the bundle directory — never a file inside the bundle (that breaks the seal, `clean-uninstall` §6.4 item 3) and never a sibling file in `/Applications` |
| **content** | `{"v":1,"manager":"scoop","uninstall_hook":true}` | the same JSON as the attribute's value |
| **who writes it** | scoop's `post_install`, into `$dir` (each version directory, so `current` always has one) | the tap's cask `postflight` (`lulu-loopp/homebrew-folio` is our own tap) |
| **who removes it** | scoop, with the version directory | Homebrew, with the bundle |
| **what reads it** | `install_channel::read`, once, at start, off the window thread | the same |

Classification becomes: **marker present and well formed → Managed(manager)**;
**no marker → Ours** when the install root passes §D's read-only probe, else
**Not writable**; **marker malformed or of an unknown `v` → Unknown** (fails to
the releases page, as §D). A copy of a managed folder carries its marker and is
treated as managed — a false positive whose cost is a command that does nothing,
the direction §D chose to fail in.

**winget has no hook** (zip + portable installer,
`packaging/winget/…/WeiyiShi.Folio.installer.yaml`), so it cannot write a marker.
The recommended reading is winget's own uninstall record — the entry winget
writes under `…\CurrentVersion\Uninstall\` naming this folder as its install
location — which is a record written at install time by the manager, not a path
guess. Q1.

**Managed copies get no card.** §7.52 ① said a new version is not a thing that
should interrupt anybody; the card is justified only when the press can finish
the job here (§A's table). A managed copy keeps today's dot and General row, and
the row names the manager's command with one **Copy** verb: `scoop update folio`,
`brew upgrade --cask folio`, `winget upgrade --id WeiyiShi.Folio --exact`. Q3.

**C3. The uninstall hook, and the Explorer default.** The marker's
`uninstall_hook` field is the fact the 2026-09-20 ruling names for the
right-click default. `first_run::rows_for` builds the Explorer row with
`on: false` unconditionally today; after U-3 it is `on: true` exactly when the
marker says `uninstall_hook: true`. The hooks, in the manifests this repository
renders:

- scoop: `post_install` writes the marker; `pre_uninstall`, guarded by
  `if ($cmd -eq 'uninstall')` because scoop also runs it on update (verified in
  `clean-uninstall-review-codex-2026-09-20.md`), runs
  `& "$dir\folio.exe" --uninstall-cleanup`. Its exit 2 (a Folio is running) is
  the door's existing answer, and scoop then abandons its uninstall with
  everything intact.
- Homebrew: `postflight` writes the attribute; `uninstall_preflight` runs
  `Folio.app/Contents/MacOS/folio --uninstall-cleanup` with
  `must_succeed: false`, so a refused removal never blocks `brew uninstall`.
- The plain zip and the plain DMG have no install time and no hook: no marker,
  Explorer default off, self-update on. That is the ruling's intent, not a gap.

**C4. The Windows archive is ten files, and the updater does not hard-code the
list.** §C.4 and §E say nine. Since 0.4.3 the archive carries `uninstall.cmd`
too (`package.ps1`'s `$manifest`;
`uninstall_tests::uninstall_archive_has_ten_files_and_cleanup_only_wrapper`).
More to the point, the old binary performs the flip for a *newer* archive, whose
list it cannot know. So:

- **The new set** is what the archive holds under its one root
  `folio-<manifest version>/`: regular files only, nothing below the root, at
  most `ARCHIVE_MAX_ENTRIES` (32) entries and `ARCHIVE_MAX_BYTES` (200 MB)
  expanded, with `folio.exe`, `conpty.dll` and `OpenConsole.exe` required, and
  every refusal of §E kept.
- **The old set** is the running build's own shipped list, a constant pinned to
  `package.ps1`'s `$manifest` by a `bt_source` reader.
- The flip moves `old ∩ on-disk` to backup and the new set in. A name in the new
  set that exists on disk and is not in the old set is somebody else's file: the
  transaction fails before any move with *Nothing changed*. Files in neither set
  — a scoop marker, `.folio-update/`, anything the person put there — are never
  touched.

**C5. The applier runs from a copy no step moves (a hole in rev 2 §C.4).** Rev 2
runs P from `<staging>\<txn>\folio.exe`, points the RunOnce entry at that same
path, and then step 2 moves `<staging>\<txn>\*` into the install. From the
moment step 2 moves the executable, the recovery entry names nothing and P has
moved its own image. The fix: Prepare writes the verified new `folio.exe` twice
— into `staging\<txn>\set\` (the files the flip moves in) and into
`staging\<txn>\applier\folio.exe` (never moved, deleted only with the
transaction). P starts from the applier copy and the RunOnce value names it. The
applier copy runs headless (`--update-apply`, `--update-recover`) and needs no
sidecar. The value is written as `!FolioUpdate-<txn8>`: the `!` prefix makes
Windows delete a RunOnce value only after its command completes, so a recovery
interrupted at logon runs again at the next logon; the finishing actor removes
it explicitly either way. I1 now holds through step 2, as §C.4 claimed.

**C6. Nothing new lands in the roaming profile.** Rev 2 let the download cache
sit under `persist::storage_dir()` (roaming `%APPDATA%\Folio`, R-14's
complaint). On Windows the archive now downloads straight into
`<install-root>\.folio-update\staging\<txn>\` — Folio's own folder, on the
destination volume, removed with the transaction or with the folder. On macOS
there is no own folder to write into (the bundle is sealed and its parent is
`/Applications`), so:

- **staging and the downloaded image** go into the system's item-replacement
  directory for the bundle's volume (`NSFileManager`
  `URLForDirectory:NSItemReplacementDirectory appropriateForURL:<bundle>`),
  which is on the destination volume by construction — what `RENAME_SWAP`
  needs — and is the operating system's temporary space;
- **the installation lock** is `flock(LOCK_EX|LOCK_NB)` on a descriptor opened on
  the bundle directory itself, which writes nothing;
- **the journal** lives in Folio's data root
  (`~/Library/Application Support/Folio/update/<txn>.json`), class O data that
  `--purge` already removes.

**C7. macOS: the DMG is the transport, the swap is in place.** "DMG replace" —
telling the person to drag a new image — is what the releases page already is,
and it is what every class but **Ours** gets. For **Ours**:

1. Download `Folio-<version>-macos-arm64.dmg` and `SHA256SUMS-macos.txt` of the
   offer's tag into the item-replacement directory; hash.
2. `hdiutil attach -nobrowse -readonly -noautoopen -mountrandom <staging>`
   through `quiet_command_named` with the absolute path, bounded; the mount
   point is recorded in the journal and detached on every exit path (R-11).
3. Verify the mounted `Folio.app` (below), `ditto` it into the staging directory,
   **verify the copy again**, detach.
4. After the quit barrier, P swaps with `renamex_np(RENAME_SWAP)`, launches the
   new bundle's executable with `--update-health`, and on health deletes the old
   bundle, which the swap left in staging.

**What notarization requires, and what the updater must not do.** The release
already made the bundle Developer ID signed, hardened, notarized and stapled.
The updater notarizes nothing and must **change nothing inside or on the copied
bundle** — no plist edit, no attribute stripped, no file added — because the seal
and the stapled ticket are what let Gatekeeper accept it offline; `ditto`
carries both. Identity is checked **without a Team ID constant**
(`docs/RELEASING.md`: no team identifier in this repository): the new bundle
must satisfy **the running bundle's own designated requirement**
(`SecCodeCopyDesignatedRequirement` of self, then `SecStaticCodeCheckValidity`
on the new bundle with strict validation, all architectures and nested code,
against that requirement), which names the certificate lineage and the bundle
identifier `io.github.lulu-loopp.folio`. Gatekeeper's assessment is recorded,
not required (`spctl` can be disabled, §E). `CFBundleShortVersionString` and the
architecture must match the offer. **A translocated bundle** (run from a
quarantined download without being moved) sits on a read-only randomized mount:
**Not writable**, releases page.

**The one residual.** On Windows the RunOnce value makes recovery reachable when
the install cannot start. On macOS the swap is atomic, but a double fault — P
dies (power loss) inside the 90 s health window **and** the new bundle cannot
start at all — leaves a bundle that does not start and the old one in the
item-replacement directory, which the system may purge at reboot. The fix would
be a user LaunchAgent carrying the recovery command: another mark outside the
bundle, with its own cleanup row. Q5.

**C8. Windows identity, with three-day certificates.** Artifact Signing issues a
certificate valid for three days, so **every release is signed by a different
certificate** than the running build: "renewal with the same subject" (§E) is
the everyday path, not an edge. The comparison is never a thumbprint:
`WinVerifyTrust` with the system's revocation policy and a required RFC 3161 time
stamp inside the certificate's validity; then the leaf subject compared as a
parsed DN (`msix::distinguished_name`, value case preserved) against the running
`folio.exe`'s own leaf subject — read from the running file, never a constant in
the updater; the package's `Publisher` against its own signer; the two Microsoft
sidecars as `package.ps1 -Sign` checks them; `VERSIONINFO` and machine type
against the offer. A changed subject is a refusal.

**C9. The card, in fewer words (2026-09-23).** A state is a word or a number; no
sentence the reader did not need.

| state | line | verbs |
|---|---|---|
| `Available` | `Folio 0.4.7` | **Update** · **Later** · **Skip** |
| `Downloading` | bar + `12 / 41 MB` (bar alone, indeterminate, when the length is unknown) | **Cancel** |
| `Verified` | `Ready. Running programs will close.` | **Restart** · **Later** |
| `Failed`, nothing moved | the reason + `Nothing changed.` | **Releases** · **Close** |
| `Failed`, rolled back | the reason + `Previous version restored.` | **Releases** · **Close** |
| `Failed`, rollback failed | `Update incomplete.` + the journal's folder | **Show folder** · **Close** |

The first verb becomes **Update**: it does not promise a restart (which is what
§B's "Download and install" was guarding against), and the restart is asked for
at `Verified`. The General row's foot gains **Restart to update** while a job
waits at `Verified`. The highlight line (R-17) is dropped. The quit's own card
still asks about unsaved documents.

**C10. The release window, and the first real update.** Nothing in R.6 merges
before the 0.4.5 tag (ruling 2026-09-25). 0.4.5 and every earlier build ship no
updater, so **the first in-app update any reader sees is 0.4.6 → 0.4.7**; a
0.4.5 reader moves to 0.4.6 by hand, as today. The clean-machine baseline (§F,
R-21) is a signed 0.4.6 candidate updating to a signed, reachable successor or to
an injected test source.

**C11. What it checks, and when.** The cadence is the daily check, unchanged.
When it answers a newer tag and the copy is eligible (switch on; newer than
`skipped_tag` by precedence; class Ours; capable, C1), the card is raised at most
once per launch in the most recently active ordinary window. **Nothing is
downloaded until the press.** A press fetches exactly two files, by the
**offer's tag**: `/releases/download/<tag>/folio-<version>-windows-x64.zip` and
`SHA256SUMS.txt` (macOS: the versioned `.dmg` and `SHA256SUMS-macos.txt`) —
never the `/releases/latest/download/` stable names, because "latest" can move
under an open offer (R-8). `<version>` is the tag without `v` and `-preview`,
the rule `docs/RELEASING.md` "The tag" states. Redirects to GitHub's asset host
follow §E. `docs/PRIVACY.md` gains that paragraph in the enabling ticket.

### R.3 Lanes and doors

| step | where it runs | lane (`ARCHITECTURE.md` §5.1) | door (§6) |
|---|---|---|---|
| daily check | `bt-update-check` (existing) | observation | `spawn_at_priority`; `http::https_get` |
| install marker read | once at start, a named worker | observation | `file_reads`, new lane for the marker; macOS `getxattr` inside `bt_platform` (metadata, outside the ledger — §6's stated bypass) |
| offer, card, progress, verbs | window thread, accepting results only | window | new `AppEvent` arms with `station()` rows; distinct `hang_watch` stations for offer, progress, outcome and barrier |
| lock, staging, download, hash, extract, verify, copy, journal `Prepared` | `bt-update-job`, one per job, `BelowNormal` | storage and integration transactions — the txn id is the operation identity, the journal the durable outcome | `spawn_at_priority`; new `http::https_download`; `file_reads` lane for the archive and journal; `quiet_command_named` for `hdiutil` / `ditto`; trust checks in-process (`WinVerifyTrust`, `Security.framework`) |
| quit barrier, session receipt | window thread, the ordinary `quit::Quit` with reason `UpdateRestart` | window, receipt from `SessionWriter` | existing |
| spawn P at `QuitStep::Exit` | window thread, one call | — | `quiet_command_named`, detached (a new child kind in §2.2) |
| flip, health wait, rollback, deletion | **P**, the separate headless process `folio --update-apply` | not a thread of the running Folio | new `bt_platform::install_flip` (the only moves of installed files) and `bt_platform::logon_hook` (the only RunOnce writer) |
| health acknowledgement | **N**, after `adopt_claim` and at the first pane text | window (one bounded journal write) | `install_flip::acknowledge` |
| startup recovery | `fn main`, after the argv doors, before `launch_wire::hand_over`; one `stat`, and work only if a journal exists | no loop exists yet (§5.3 row 18's reasoning) | `install_flip` |

K never stamps W's heartbeat; its phases go to `diagnostics.log`, one line per
phase change with the txn id. P writes its phases into the journal and into
`.folio-update/applier.log`; N copies them into `diagnostics.log` at health.
`ARCHITECTURE.md` §2.1 gains two argv doors (`--update-apply`,
`--update-recover`); `--update-health` is a flag on the ordinary launch.

### R.4 What is written outside Folio's own folder, and its undo

| what | platform | when it exists | undone by, without UI |
|---|---|---|---|
| HKCU `RunOnce` value `!FolioUpdate-<txn8>` naming this copy's applier | Windows | from the first destructive move to `Healthy` / `RolledBack` | the finishing actor; Windows after a completed run; **`--uninstall-cleanup`** (new per-copy row: removed if it names this copy's `.folio-update` or a path that no longer exists) |
| staging, downloaded image, mount point, the old bundle until health | macOS | during a job | the transaction; the startup sweep (journal with no live lock); the system's temporary-item purge |
| journal `update/<txn>.json` | macOS | during a job | the transaction; `--purge` (covers the data root already) |
| `update-check.json` v2 | both | exists today | `--purge`, unchanged |
| the sparse package re-registered at a new version | Windows | only after `Healthy`, only if already registered | the existing Explorer rows of `--uninstall-cleanup` / `--remove-explorer-menu` |
| the install marker | both | written by the **package manager**, not by Folio | the manager, with the folder or bundle |

Everything else — `.folio-update/{journal.json, lock, staging/, backup/,
applier.log}` — is inside the Windows install folder and leaves with it.

### R.5 The review's risks, now

| item | status on 2026-09-25 |
|---|---|
| R-1 recovery entry | answered in rev 2; **corrected** by C5 (the applier copy is never moved) |
| R-2 health, backups | answered; stands |
| R-3 claim adoption | answered; refined: `is_writer_of` keeps caching a refusal (a non-writer must not become the writer mid-run); only the new `try_claim` does not cache |
| R-4 quit first | answered; stands |
| R-5 archive layout | **changed** by C4: ten files today, and the set is read from the archive under bounds |
| R-6 same volume, read-only probe | answered; C6 moves the Windows download into the install folder and gives macOS the item-replacement directory |
| R-7 installation lock | answered; the macOS lock is `flock` on the bundle directory (C6); another process still mapping the old image is detected by P's exclusive open of `folio.exe` (E3) |
| R-8 tag pinning | answered; whether the repository has immutable releases enabled is still **unknown**, and no ticket depends on it |
| R-9 Windows trust | answered; sharpened by C8 |
| R-10 macOS identity | **changed** by C7: the running bundle's designated requirement, no Team ID constant |
| R-11 quarantine, mounts | answered; the mount lives under staging (C7) |
| R-12 redirects | answered; stands |
| R-13 bounded download | answered; U-7 |
| R-14 sweep and paths | answered; paths restated in C6 and R.4 |
| R-15 capability | **changed** by C1: the running signature; Q4 |
| R-16 skip race | answered; U-6 |
| R-17 card eligibility | answered; C2 removes the managed card, C9 rewrites the words, the highlight line is dropped |
| R-18 W never blocks | answered; R.3 names the stations |
| R-19 manager ownership | **changed** by C2: the marker; winget is Q1 |
| R-20 MSIX version | answered; U-19 |
| R-21 clean machine | answered; baseline restated in C10 |
| R-22 tests | adopted; distributed over R.6 |
| R-23 split, no panic stub | answered; R.6 keeps the typed `Unsupported` and one enabling ticket per platform |

Experiments added to §H:

| id | risk | experiment |
|---|---|---|
| E1 | the marker attribute on the bundle directory breaks `codesign --verify --strict --deep` or Gatekeeper | write it on a notarized `Folio.app`, verify, assess, launch from a fresh account |
| E2 | the shape of winget's uninstall record for a portable zip (hive, key name, install location) | `winget install WeiyiShi.Folio` on the Windows 10 VM; export the key; uninstall; export again |
| E3 | an exclusive open of `folio.exe` refuses while another process runs from the folder and succeeds after it exits | two isolated-`APPDATA` Folios from one folder on the VM; open from a third process |
| E4 | a `!`-prefixed HKCU RunOnce value survives an interrupted run and runs again | on the VM: a value whose command ends itself early; two logons |
| E5 | the item-replacement directory for `/Applications` and `~/Applications` is on the bundle's volume and `RENAME_SWAP` works across it | on the Mac mini |

### R.6 Ticket split

Each ticket is S or M, keeps a typed `Unsupported` or no-op path until the
enabling ticket for its platform, and is mergeable alone **after the 0.4.5 tag
exists on main** (ruling 2026-09-25): every brief ends at "committed, CI green
on the branch", and the coordinator merges after the tag. Numbers are `U-n`; the
coordinator maps them into the ticket set at dispatch. Every brief carries
`_standing-rules.md` in full; anchors are names, found by grep. Who: Opus for
all; Codex may take U-7, U-9 and U-10 (pure, well bounded).

**Order.** U-1, U-2, U-5 … U-12 are independent. U-3 needs U-1; U-4 needs U-1
and Q1. U-13 needs U-6; U-14 needs U-13; U-15 needs U-7, U-8, U-9, U-10, U-13;
U-16 needs U-13; U-17 needs U-8; U-18 needs U-5, U-15, U-16, U-17; U-19 needs
U-18; U-20 needs U-7, U-8, U-11, U-12, U-13; U-21 needs U-5, U-16, U-20; U-22
needs U-1, U-14, U-18, U-19 and the VM run; U-23 needs U-14, U-21 and the Mac
run.

| # | title | size | lane |
|---|---|---|---|
| U-1 | the install marker, read once | S | local |
| U-2 | the package managers write the marker and call the door | S | no compile; VM + Mac mini |
| U-3 | the Explorer menu is pre-ticked where an uninstall hook exists | S | local |
| U-4 | winget copies are told by winget's own record (only if Q1 = read) | S | local + VM |
| U-5 | `try_claim` and `adopt_claim` | M | local |
| U-6 | `update-check.json` v2 under one owner | M | local |
| U-7 | `https_download`: bounded streaming to a file | M | local + CI macOS |
| U-8 | the journal, the lock and the recovery table as pure code | M | local |
| U-9 | the Windows archive reader | S | local |
| U-10 | Windows identity and capability | M | local |
| U-11 | macOS identity and capability | M | CI macOS + Mac mini |
| U-12 | macOS image mount and copy | S | CI macOS + Mac mini |
| U-13 | the job, application-owned, headless | M | local |
| U-14 | the card and the row | M | local |
| U-15 | Windows Prepare | M | local |
| U-16 | the quit barrier | M | local |
| U-17 | the recovery entrances | M | local + VM |
| U-18 | the Windows flip, health and rollback | M | local + VM |
| U-19 | the sparse package after an update | S | local + VM |
| U-20 | macOS Prepare | M | CI macOS + Mac mini |
| U-21 | the macOS swap, health and rollback | M | Mac mini |
| U-22 | enable on Windows | S | VM |
| U-23 | enable on macOS | S | Mac mini |

#### U-1 — The install marker, read once (S)
**True on BASE.** `RULES.md` §41 says the channel is read from a written marker;
no code writes or reads one (`rg -n "folio-install" crates` is empty).
**Goal.** `install_channel::{read, classify}`: the Windows file beside
`current_exe()`, the macOS attribute on the bundle directory;
`classify(evidence) -> Channel { Ours, Managed(Manager), NotWritable, Unknown }`
pure; read once at start on a named worker; one `diagnostics.log` line. Nothing
acts on it yet.
**Design.** C2. `Manager = Scoop | Homebrew | Winget`. No marker is `Ours` only
after §D's read-only probe; a malformed or future marker is `Unknown`.
**Tests red on BASE.** `a_well_formed_marker_makes_this_copy_managed`;
`a_malformed_or_future_marker_is_unknown_and_never_ours`;
`no_marker_and_a_writable_root_is_ours` (a real temp folder through the real
reader).
**Docs in the same commit.** `RULES.md` §41 gains the format and both
locations; one `DESIGN.md` entry; the `file_reads_doors.txt` row; no CHANGELOG
line (nothing visible) — the report says so.
**Architecture impact.** (a) new fact *how this copy was installed*, owner
`install_channel`; (b) `file_reads` (new lane), `spawn_at_priority`, macOS
`getxattr` in `bt_platform` (metadata, outside the ledger); (c) none; (c′) none;
(d) no.

#### U-2 — The package managers write the marker and call the door (S)
**True on BASE.** `cask.sh` renders `app`, `zap` and `depends_on`, no flight
blocks; the scoop manifest `update-manifests.ps1` rewrites has no `post_install`
or `pre_uninstall`.
**Goal.** C3's hooks: scoop `post_install` marker and `pre_uninstall` gated on
`$cmd -eq 'uninstall'`; cask `postflight` attribute and `uninstall_preflight`
door with `must_succeed: false`; `update-manifests.ps1` preserves both. The
owner pushes the two manifests.
**Design.** C2, C3. E1 first; if E1 fails, stop and report — the macOS marker
place is then an owner question.
**Tests red on BASE.** A rendered cask contains both blocks; a render over a
manifest carrying them keeps them byte-identical (in `smoke-tests.ps1`'s style);
on the Windows 10 VM, scoop install / update / uninstall: marker present after
install and after update, cleanup ran only on uninstall; on the Mac mini,
`brew install` / `upgrade` / `uninstall` likewise.
**Docs in the same commit.** `RELEASING.md` "Distribution manifests" names the
hooks and why `$cmd` is checked; `docs/install.md` and `install.zh-CN.md`
uninstall sections (Chinese by opus46 after); CHANGELOG *Added*: "scoop and
Homebrew now clean up after Folio when they uninstall it."
**Architecture impact.** (a) writes U-1's fact from outside the process;
(b) no Folio door — the managers' own; they call `--uninstall-cleanup`; (c) none;
(c′) a new writer of the install channel (the manager); reader: U-1 only;
(d) no.

#### U-3 — The Explorer menu is pre-ticked where an uninstall hook exists (S)
**True on BASE.** `first_run::rows_for` builds the Explorer row with
`on: false`.
**Goal.** `on` = the marker's `uninstall_hook` (ruling 2026-09-20). Nothing else
on the card changes.
**Tests red on BASE.** `the_explorer_row_is_on_only_where_an_uninstall_hook_exists`
over `Ours`, `Managed(Scoop)`, `Managed(Winget)`, `Unknown`.
**Docs in the same commit.** `DESIGN.md` entry naming the §7.56 default it
supersedes; `RULES.md` §35 and §37 fold the default; CHANGELOG *Changed*.
**Architecture impact.** (a) a new reader of U-1's fact; (b) none new; (c) none;
(c′) none; (d) no.

#### U-4 — winget copies are told by winget's own record (S; only if Q1 = read)
**True on BASE.** Nothing reads winget's uninstall entries.
**Goal.** E2 first; then `install_channel` reads the entry whose install
location canonicalizes to this folder → `Managed(Winget)` with
`uninstall_hook: false`. A failed read is `Unknown`.
**Tests red on BASE.** `a_winget_record_naming_this_folder_makes_it_managed`;
`a_record_naming_another_folder_changes_nothing` (injected reader; the real one
on the VM).
**Docs in the same commit.** `RULES.md` §41 names this one exception to "a marker
written by a hook"; `DESIGN.md` entry.
**Architecture impact.** (a) U-1's fact gains a source; (b) a registry read in
`bt_platform` — there is no door for registry reads; the brief names that and
does not invent one; (c) none; (c′) new source: winget's record; reader: the
classifier; (d) no.

#### U-5 — `try_claim` and `adopt_claim` (M)
**True on BASE.** `persist::is_writer_of` caches `claim_data_directory`'s
answer, a refusal included, for the life of the process (`or_insert_with`).
**Goal.** §C.7: `try_claim(dir)` without caching; `adopt_claim(dir, claim)`
inserts an acquired guard into the same table before anyone asks.
`is_writer_of` is unchanged (R.5, R-3).
**Tests red on BASE.** `acquired_claim_is_adopted_without_a_gap`;
`transient_claim_refusal_is_not_cached_by_try_claim`;
`is_writer_of_still_remembers_a_refusal`;
`manual_launch_and_relaunch_have_one_writer`.
**Docs in the same commit.** `DESIGN.md` entry; `ARCHITECTURE.md` §2.1's claim
sentence.
**Architecture impact.** (a) the data-directory claim, owner `persist`'s table;
(b) none new; (c) none; (c′) a new writer of the claim table (`adopt_claim`);
readers: every `is_writer_of` caller, none of which may see a gap — that is the
test; (d) no.

#### U-6 — `update-check.json` v2 under one owner (M)
**True on BASE.** `update::run` and `update::mark_seen` each read and write on
their own; a Skip written between them would be lost; `UPDATE_CHECK_MIGRATIONS`
is empty.
**Goal.** v2 adds `skipped_tag`; one lock across every read-modify-write;
comparison by precedence; a cached skipped tag stays hidden inside the day;
switching off suppresses cached offers. No UI yet.
**Tests red on BASE.** `skip_racing_check_and_seen_keeps_all_fields`;
`cached_skipped_tag_stays_hidden_inside_daily_cadence`;
`newer_tag_is_offered_after_skip`;
`switch_off_suppresses_cached_and_inflight_offers`; the first migration's round
trip.
**Docs in the same commit.** `DESIGN.md` entry; `PRIVACY.md`'s
`update-check.json` row gains the field (both languages; Chinese marked
pending); `structural-debt.md` D-53 notes fact 11's part repaid.
**Architecture impact.** (a) the update check's memory, file and claim (fact
11), owner `update`; (b) existing; (c) repays D-53's fact-11 part; (c′) a new
writer (Skip) of the state file; readers: `mark_is_lit` and the gear; (d) no.

#### U-7 — `https_download`: bounded streaming to a file (M)
**True on BASE.** `http::https_get` and the macOS arm accumulate a `Vec`; nothing
downloads to a file.
**Goal.** §E's `https_download` in all three arms: fixed buffer, byte cap checked
before each write, unknown length as `None`, monotonic deadlines (30 s idle, 10
min total), cancellation latency stated per phase, one pending progress wake.
**Tests red on BASE.** §F's Transport group over a local fake transport,
including `https_redirects_work_and_http_redirects_refuse`.
**Docs in the same commit.** `DESIGN.md` entry; the `http.rs` module doc's bound
restated for downloads.
**Architecture impact.** (a) none owned; (b) extends the HTTP door; progress by
the one-wake contract; (c) none; (c′) none; (d) no.

#### U-8 — The journal, the lock and the recovery table as pure code (M)
**True on BASE.** None of it exists.
**Goal.** `update_txn`: the journal type (§C.1) and its fsynced write through a
temporary file; the installation lock (Windows exclusive open, Unix `flock`);
§C.6's table as `decide(phase, disk) -> Action`; C4's set rule; C5's applier
copy — all over a durable fake filesystem that can be cut at any operation.
**Tests red on BASE.** `every_crash_boundary_recovers_a_complete_launchable_install`
(one case per row and per instant, the applier copy included);
`recovery_can_itself_be_interrupted`;
`rollback_failure_preserves_journal_and_backups`;
`a_moved_installation_fails_the_transaction_without_touching_files`;
`two_data_directories_share_one_install_lock`;
`a_name_in_the_way_fails_before_any_move`.
**Docs in the same commit.** `DESIGN.md` entry carrying the table.
**Architecture impact.** (a) new fact *the installation's transaction*, owner
`update_txn`; (b) none (pure); (c) none; (c′) none; (d) no.

#### U-9 — The Windows archive reader (S)
**True on BASE.** No zip reader in product code.
**Goal.** C4: one root `folio-<version>/`, regular children, bounds, §E's
refusals; the running build's shipped-list constant pinned to `package.ps1`'s
`$manifest` through `bt_source`.
**Tests red on BASE.** `packaged_zip_root_is_accepted` (a fixture built to the
real ten-file layout); `duplicate_case_alias_stream_link_and_traversal_entries_are_refused`;
`expanded_size_limit_precedes_disk_exhaustion`;
`the_shipped_list_is_package_ps1s`.
**Docs in the same commit.** `RELEASING.md` "What gets published": the updater
reads the root name, so renaming it breaks in-app updates.
**Architecture impact.** (a) none; (b) `file_reads` (the transaction's lane);
(c) none — the pin reads through `bt_source`, no `MIGRATION-DEBT.tsv` row;
(c′) none; (d) no.

#### U-10 — Windows identity and capability (M)
**True on BASE.** `msix::distinguished_name` and `publisher_matches_subject`
exist; nothing in product code verifies a downloaded file's signature.
**Goal.** C8's decision in `bt_platform::trust`, and C1's Windows capability.
**Tests red on BASE.** `validly_signed_wrong_product_or_version_is_refused`;
`dn_values_preserve_case_and_order`;
`timestamp_policy_accepts_the_packagers_format`;
`an_unsigned_running_build_is_not_capable`;
`a_new_certificate_with_the_same_subject_is_accepted` — fixtures signed in the
test with a certificate made in the test; the real `WinVerifyTrust` runs.
**Docs in the same commit.** `RELEASING.md` "What signs what": changing the
subject breaks in-app updates.
**Architecture impact.** (a) none owned; (b) in-process trust calls, no child;
(c) none; (c′) none; (d) no.

#### U-11 — macOS identity and capability (M)
**True on BASE.** `sign.sh` and `notarize.sh` verify at release time only.
**Goal.** C7's check against the running bundle's designated requirement, the
version and architecture, the stapled ticket's presence; C1's macOS capability
(ad-hoc is not capable).
**Tests red on BASE.** `a_bundle_outside_the_running_requirement_is_refused`;
`an_ad_hoc_running_build_is_not_capable`; version and architecture mismatches —
real `Security.framework` calls on CI macOS over bundles signed in the test.
**Docs in the same commit.** `packaging/macos/README.md`: the updater relies on
the designated requirement staying stable (identifier and certificate lineage).
**Architecture impact.** (a) none; (b) in-process `Security.framework`; (c) none;
(c′) none; (d) no.

#### U-12 — macOS image mount and copy (S)
**True on BASE.** Nothing mounts an image at run time.
**Goal.** `hdiutil` attach / detach and `ditto` through `quiet_command_named`,
bounded, the mount point recorded; copy into the item-replacement directory;
detach on every path; E5.
**Tests red on BASE.** `every_exit_path_detaches_what_it_attached` (fake tool);
one real attach of a test image on CI macOS.
**Docs in the same commit.** `ARCHITECTURE.md` §2.2 gains the row.
**Architecture impact.** (a) none; (b) `quiet_command_named` with absolute
paths; (c) none; (c′) none; (d) no.

#### U-13 — The job, application-owned, headless (M)
**True on BASE.** `update.rs` stops at a tag; there is no job.
**Goal.** §B's state machine as `update_job`, owned by the application, with the
immutable offer, C11's eligibility, C1's capability gate, two `AppEvent` arms
with `station()` rows, stale events refused. The driver is a typed
`Unsupported` that returns before any network, staging, flush or wait; offers
stay gated off.
**Tests red on BASE.** `offered_tag_survives_latest_changes`;
`stale_progress_cannot_revive_a_cancelled_job`;
`every_card_state_has_a_handler_for_every_verb`;
`an_unsupported_driver_fails_without_downloading`;
`a_suppressed_offer_does_not_consume_the_launch_gate`.
**Docs in the same commit.** `DESIGN.md` entry; `ARCHITECTURE.md` §4.2's
durability row names the job.
**Architecture impact.** (a) new fact *the update job*, owner the application;
(b) none used yet; (c) none; (c′) none; (d) no.

#### U-14 — The card and the row (M)
**True on BASE.** The General row offers the releases page; there is no card.
**Goal.** C9's card on `first_run::Card`'s footing and the float-window surface,
the determinate bar (new drawing), the row foot **Restart to update**, the
managed row with **Copy** (C2). English only; every new `Text` listed in
`Text::CHINESE_PENDING`. Offers still gated off (U-13).
**Tests red on BASE.** Paint-model tests per state;
`later_from_verified_leaves_a_resume_entry`;
`a_managed_copy_shows_its_command_and_no_card`; the two-line budget test.
**Docs in the same commit.** `DESIGN.md` successor entry to §7.52 for the
surface; no CHANGELOG line until U-22.
**Architecture impact.** (a) reads the job; (b) none new; (c) none; (c′) none;
(d) no.

#### U-15 — Windows Prepare (M)
**True on BASE.** U-13's driver returns `Unsupported`.
**Goal.** §C.2 on K for Windows: lock; staging under `.folio-update`; download
(U-7) of the offer's two files; hash; extract (U-9); verify (U-10); write `set\`
and `applier\` (C5); re-verify in place; journal `Prepared`. Cancel and every
failure delete the transaction and say *Nothing changed*.
**Tests red on BASE.** `mutated_asset_hash_pair_refuses_before_swap`;
`short_write_disk_full_and_flush_failure_never_verify`;
`staging_is_always_on_the_destination_volume`;
`cancel_leaves_no_transaction` (real temp folders, fake transport).
**Docs in the same commit.** `DESIGN.md` entry.
**Architecture impact.** (a) writes the transaction (U-8's owner); (b)
`spawn_at_priority` `bt-update-job`, `https_download`, `file_reads`; (c) if D-3's
common probe contract has landed, the worker uses it; otherwise the worker is
added to D-3's list in the same commit; (c′) none; (d) no.

#### U-16 — The quit barrier (M)
**True on BASE.** `quit::Quit` carries no reason; the quit write waits on
`SessionWriter::wait_for`.
**Goal.** §C.3: `UpdateRestart`; `Abandon` and a failed write abandon the update
(card back to `Verified`, reason named); a named session generation with a
deadline while W pumps; admissions refused from `Photograph`; at `Exit` one call
spawns P from the applier copy; a spawn failure journals `Failed`.
**Tests red on BASE.** `quit_cancel_or_failed_save_prevents_swap`;
`session_receipt_matches_the_final_snapshot`;
`close_during_each_phase_preserves_recovery`;
`no_launch_is_admitted_after_the_photograph`.
**Docs in the same commit.** `DESIGN.md` entry; `RULES.md` §43 names the new
reason.
**Architecture impact.** (a) the quit transaction, owner `quit::Quit`, and the
session document; (b) `quiet_command_named`, detached (new §2.2 row); (c) none;
(c′) a new trigger of the quit (the update); readers that assumed only a person
quits — `launch_wire::admit` and the restore card — are named and tested;
(d) no.

#### U-17 — The recovery entrances (M)
**True on BASE.** Six argv doors; nothing recovers at start;
`--uninstall-cleanup` has no RunOnce row.
**Goal.** The `--update-recover <journal>` door; the startup pass in `fn main`
(R.3); `bt_platform::logon_hook` (set and remove the `!`-prefixed value); the
per-copy `--uninstall-cleanup` row; E4.
**Tests red on BASE.** `a_journal_found_at_start_is_decided_from_disk`;
`the_cleanup_door_removes_this_copys_recovery_value_and_a_dead_one_only`;
`startup_with_no_journal_costs_one_stat`.
**Docs in the same commit.** `ARCHITECTURE.md` §2.1 and §6 tables;
`RULES.md` §41's list of marks; `docs/install.md` cleanup list; `DESIGN.md`
entry.
**Architecture impact.** (a) the transaction; (b) new door `logon_hook`, new
argv door; (c) none; (c′) a new actor advancing the transaction at start;
(d) no.

#### U-18 — The Windows flip, health and rollback (M)
**True on BASE.** U-17's entrances with no flip.
**Goal.** `--update-apply`: wait for O (E3's exclusive open and the claim probe),
write the logon hook, flip per C4 through `bt_platform::install_flip`, launch N
with `--update-health`, wait 90 s, delete the recorded backups at `Healthy` or
roll back; N acknowledges after `adopt_claim` (U-5), the schema check and the
first pane text.
**Tests red on BASE.** §F's Health group;
`a_manual_launch_of_the_new_build_satisfies_health`;
`await_timeout_never_hands_over_or_starts_a_nonwriter`;
`locked_backup_blocks_reuse`.
**Docs in the same commit.** `ARCHITECTURE.md` §6 (`install_flip`); `DESIGN.md`
entry.
**Architecture impact.** (a) the transaction; the installed file set, a new fact
owned by `install_flip`; (b) new door `install_flip`; (c) none; (c′) a new
writer of the installed files; readers: the sparse package and the classic verb,
which name the folder and are unchanged by an in-place flip — the brief states
it; (d) no.

#### U-19 — The sparse package after an update (S)
**True on BASE.** `msix::registered()` returns no version.
**Goal.** R-20: expose the registered version; after `Healthy`, on the
registration worker, re-register when the registered version is older than the
manifest's four-part one; a failure shows on the Explorer row and is never
reported as "nothing changed".
**Tests red on BASE.** `an_older_registration_is_renewed_only_after_health`;
`a_rollback_never_touches_the_registration`.
**Docs in the same commit.** `RELEASING.md` "The sparse MSIX package" notes the
renewal.
**Architecture impact.** (a) the Explorer registration (fact 10); (b) the
existing registration worker; (c) touches D-52's fact 10 — noted, not widened;
(c′) a new trigger (health) of registration; reader: the Explorer row; (d) no.

#### U-20 — macOS Prepare (M)
**True on BASE.** U-13's driver returns `Unsupported` on macOS.
**Goal.** C7 steps 1–3 on K; translocated or unwritable → `NotWritable`;
journal `Prepared`.
**Tests red on BASE.** `renamed_bundle_updates_only_itself`;
`a_translocated_bundle_is_not_writable`; quarantine preserved end to end (§H).
**Docs in the same commit.** `DESIGN.md` entry.
**Architecture impact.** (a) the transaction; (b) as U-12, plus
`https_download`; (c) as U-15; (c′) none; (d) no.

#### U-21 — The macOS swap, health and rollback (M)
**True on BASE.** U-20 stops at `Prepared`.
**Goal.** `--update-apply` on macOS: `RENAME_SWAP`, health, swap back on
failure, delete the old bundle at `Healthy`; the LaunchAgent hook only if Q5
says so.
**Tests red on BASE.** `a_failed_health_swaps_back`;
`swap_leaves_no_instant_without_a_bundle` (E5 and §H's running-bundle swap).
**Docs in the same commit.** `DESIGN.md` entry; `packaging/macos/README.md`.
**Architecture impact.** (a) the installed bundle (owner `install_flip`); (b)
`install_flip`; (c) none; (c′) a new writer of the installed bundle; (d) no.

#### U-22 — Enable on Windows (S)
**True on BASE.** Everything above merged; offers gated off.
**Goal.** Run the new `clean-vm.md` §4.4 on Windows 10 and 11 with a signed
0.4.6 candidate and an injected successor; then turn the Windows gate on.
**Tests.** The §4.4 checklist as recorded evidence; `the_windows_gate_is_on`.
**Docs in the same commit.** `PRIVACY.md` (the press, the two files, the asset
host); the `update.rs` module doc per §A's table; `DESIGN.md` successor to
§7.52; `RULES.md` §36 folded; the macOS plan's §M4 note; CHANGELOG *Added*:
"Folio can update itself when you press Update."
**Architecture impact.** (a)–(d) none new.

#### U-23 — Enable on macOS (S)
As U-22, for macOS, after U-21, with a notarized successor on the Mac mini.

### R.7 Open questions for the owner

1. **winget:** tell a winget copy by winget's own uninstall record naming this folder (U-4), or treat it as ours and let it update itself?
2. **Installer:** 0.4.6 ships no Windows installer (zip, scoop and winget stay the routes; the Apps entry and removal of shipped files stay deferred) — agreed?
3. **Managed copies:** only the gear dot and a row showing the manager's command (recommended), or the update card as well?
4. **Capability:** "the running build is signed" decides whether it may update itself (a signed candidate would then update to the next public release) — agreed, or keep a build flag?
5. **macOS double fault:** accept that a power cut inside the 90 s health window plus a new build that cannot start means reinstalling from the DMG, or add a LaunchAgent recovery hook (one more mark outside the bundle, with its cleanup row)?
