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
