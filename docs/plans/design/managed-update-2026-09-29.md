# U-41 — managed copies update through the same transaction (design, 2026-09-29, revision (b))

Design only: phase 1 of U-41, on `design/managed-update` from `main` at
`6abd3f5e`. No product code was written, nothing was built, and no package
manager was run. Facts about Folio come from the code, cited by symbol. Facts
about scoop, winget and Homebrew come from their own documentation and from
their source **at pinned releases** (§9 Sources: the pins, and the function
each claim rests on; never a line number). Homebrew's source was read from the
7.0.6 tree on the Mac mini, where only `brew --version`, `brew info` and
`brew cat` ran. A claim no source settles is an experiment `E-M<n>` (§6), and
the phase-2 ticket that depends on it waits for it.

**Revision (b)** answers Codex's review of `aeaa80a8`
(`U-41-review-codex-2026-09-29.md`, verdict *not yet*, fifteen findings) and the
coordinator's rulings on it. It is amended in place; the ledger of what changed
is §0. Where revision (a)'s text and this one differ, this one stands.

**The rulings this answers**

| date | ruling | where |
|---|---|---|
| 2026-09-27 (owner) | A managed copy (scoop, winget, Homebrew, and the zip, "ours") presses one **重启以更新** and the update completes. | §1–§4 |
| 2026-09-29 (owner) | All four in 0.4.7: one road with adapters, not four logics ("做得好的话应该不需要分开的四套逻辑"). | §1 |
| 2026-09-28 (owner) | Whatever happens, Folio never breaks. Interrupted anywhere, the next start opens. The new version is best; the old one still offers the update. **Literal, not waivable** (coordinator, revision (b) ruling 2). | the invariant test in every table of §2 |
| 2026-09-20 (owner) | "Managed installs do not self-update." | superseded for Homebrew and scoop by the 2026-09-27 ruling; winget stays on it in 0.4.7 (§2.4) |
| 2026-09-29 (owner, via the coordinator) | Q1: build winget, eligibility off until its design is resolved. Q2: holds and pins are honoured before allocation, no card, the row names the exact command. Q3: carry the marker. Q4: leave scoop's old folder. Q5: a `scoop cleanup` during an apply is **not** an accepted limit. | §8 |

**The invariant test**, applied to every row of §2: *from the first destructive
step to the end of the trial, at least one complete install set (old or new) is
at a place the journal names, the manager's own record of the copy agrees with
it or can be made to agree offline, and the rescue build can reach it without
the network.* A row that fails the test is a design that is not adopted.

**What stays exactly as built** (`docs/plans/design/self-update-2026-09-16.md`
revisions (a)–(h)): the phases of `update_txn::Phase` and the transition table
`update_txn::next`; the writer and effect rights; `update_txn::decide`'s rows for
the two existing layouts; the frozen header v1 and receipt v1 (H.1); the quit
barrier and `Handoff` (U-21); the entrance at logon (U-22, U-26) and the
admission locks; the exit guard (revision (e)); the job owner's pass at a later
launch (revision (d)); adoption and deferral (revision (h)); the card's words
(C9); the rollout contract of (h); `release_manifest::MIN_UPDATER` (0.4.6).

This note adds **no phase, no transition and no writer right**. It adds one
journal layout (`Link`, scoop) and one field (`marker`, the bundle layouts).
`decide` gains the rows `Link` needs, each answering with a transition already in
`next`'s table.

---

## 0. Revision (b): what changed, finding by finding

| finding | Codex | what this revision does |
|---|---|---|
| F1 | winget's step is a third call site; "exactly two" is false | the interface is named `Prepare`, `Activate` (scheduled by the layout at a recorded phase boundary) and `Prove/Recover`; no claim of two call sites (§1.1) |
| F2 | a staged trial does not make winget's destructive upgrade transactional | the two winget designs side by side, the invariant test applied to each; (a) recommended: winget stays on the Copy row in 0.4.7 (§2.4) |
| F3 | digests are not proof that winget committed its bookkeeping | the analysis of (b) states what winget's index and uninstall record hold and where each is cut (§2.4.2) |
| F4 | `scoop cleanup` can delete the sibling home and a version during the relink | the scoop home leaves `apps\folio`; the transaction keeps both complete sets outside scoop's folder; the relink is one operation if E-M5 proves it, and the two-call form is guarded by construction either way (§2.3) |
| F5 | shim and Start-menu targets unproved; renderer equality | E-M2 covers both launch surfaces; the renderer refuses unless its output names the exact offer (§2.3) |
| F6, F12 | apply-after-publish leaves a downgrade window; the release needs a gate | tap and bucket applied and read back **before** the page is published; a ticket of its own, U-41e0, that can land now (§5.1) |
| F7 | a moved Homebrew bundle keeps its marker | the road only at the app target Homebrew recorded (§2.2 R-H2) |
| F8 | the winget probe is a network and consent operation | under design (a) there is no probe; under (b) it is named in full in `PRIVACY.md` and never accepts terms (§2.4) |
| F9 | machine scope; the ten-minute kill | machine-scope copies stay on the Copy row; the kill hazard is part of (b)'s analysis (§2.4) |
| F10 | the failure table omits cases | Codex's rows added verbatim, each with card and journal (§3.2) |
| F11 | carry the marker, with five pins | accepted (Q3); the five pins are rules M1–M5 (§4) |
| F13 | experiments insufficient; pins | E-M1–E-M6 as Codex writes them; sources pinned; minimum manager versions (§6, §9) |
| F14 | sizes and dependencies | re-cut (§7) |
| F15 | owner questions | the answers are recorded (§8) |

---

## 1. One road; the adapter interface

### 1.1 The rules

**R1. One road.** The transaction of `update_txn` is the only road a copy of
Folio is updated by, whoever installed it. A package manager takes part through
an **adapter** that the layout of the journal body names. The adapter has three
operations, and the road calls them at recorded phase boundaries; nothing else
about the road depends on the manager.

| operation | what it answers | when the road calls it |
|---|---|---|
| **`Prepare`** | where the new set comes from, where it is staged, what bookkeeping travels with it | `Allocated → Prepared`, on the job's worker |
| **`Activate`** (forward and back) | how the staged set becomes the live one, and how that is undone | at the phase boundary the layout records: for every adapter in 0.4.7, after `Moving` is durable and before `TrialBegan` (forward), and after `RollbackIntent` is durable (back) |
| **`Prove / Recover`** | which set is live, read from the disk (and, for a manager that keeps its own record, from that record); whether the pair is consistent | by `decide`, at every lock holder's step and every recovery |

The earlier wording, "exactly two adapter steps, Prepare and Move", is withdrawn:
a layout can in principle schedule `Activate` at another boundary (winget's
design (b) would, §2.4.2), and `Prove / Recover` is asked at every step. What
is fixed is the phase table: an adapter chooses its boundary among existing
transitions and adds none.

**R2. The adapter is chosen once**, at the press, from the channel
(`install_channel::channel()`), and recorded in the journal body. The applier,
the recovery build and every later lock holder read it from the journal, never
from the channel.

**R3. Folio activates only what the manager lets it activate.** Folio performs
`Activate` only when it is exactly the manager's own move, with the manager's own
bookkeeping written the way the manager writes it:
- Homebrew's move of an `auto_updates` app is the app replacing its own bundle in
  the `/Applications` target Homebrew recorded (the Cask Cookbook's definition,
  HB1);
- scoop's move is a version folder and the `current` junction pointed at it
  (`scoop reset`'s move, S4).

**R4. The trial comes after `Activate` and before the first step Folio cannot
undo** (the retirement after `Committed`). With winget on the Copy row (§2.4),
this holds for every adapter in 0.4.7; revision (a)'s "trial before the
manager's step" is withdrawn with the design it served.

### 1.2 The decision table

| | **`Prepare`: source** | **`Prepare`: staged where** | **`Activate` forward** | **`Activate` back** | **`Prove`: which set is live** | **after `Committed`** |
|---|---|---|---|---|---|---|
| **ours, Windows** (`Layout::Members`) | Folio downloads the offer's zip and `SHA256SUMS.txt` (U-20) | `H\<txn>\set\` | one `MoveFileExW` per file (U-23) | the moves back (U-24) | by digest, file by file | `backup\` deleted |
| **ours, macOS** (`Layout::Bundle`) | Folio downloads the offer's DMG and `SHA256SUMS-macos.txt` (U-27) | `H/<txn>/stage/Folio.app` | one `renamex_np(RENAME_SWAP)` (U-28) | the same swap back (U-29) | by bundle identity | the old bundle deleted |
| **Homebrew** (`Layout::Bundle`, `marker` set) | as ours, macOS | as ours, macOS, **plus the marker carried onto the staged bundle** (§4) | as ours, macOS, at the target Homebrew recorded only (R-H2) | as ours, macOS | as ours, macOS | as ours, macOS |
| **scoop** (`Layout::Link`, new) | as ours, Windows | `H\<txn>\set\` (the new version folder's full content, bookkeeping included) **and** the scoop version folder `<scoop>\apps\folio\<v>\` made from it; `H\<txn>\backup\` holds a copy of the running version folder | the `current` junction pointed at `apps\folio\<v>` (§2.3: one operation if E-M5 proves it, else two calls, guarded) | `current` pointed back at the old folder, which is restored from `H\<txn>\backup\` first if anything removed it | by the junction's target, then by the target folder's digests | nothing in `apps\folio`: the old version folder is scoop's, and `scoop cleanup` removes it (S6) |
| **winget** | — design (a): **not eligible in 0.4.7**; the row keeps `winget upgrade --id WeiyiShi.Folio --exact` with **Copy** (§2.4) | | | | | |

### 1.3 What each layout records

The journal body belongs to the rescue build's version ((b).2), so a new layout
is read by exactly the builds that can write it. The header is untouched.

| layout | recorded at `Allocated` | added at `Prepared` |
|---|---|---|
| `Members` | unchanged | unchanged |
| `Bundle` / `BundleIntent` | unchanged, plus `marker: Option<Vec<u8>>` — the attribute's bytes read from the running bundle before `Allocated` (`None` for ours) — and, for Homebrew, the recorded app target (R-H2) | unchanged, plus the same fields |
| `Link` (scoop) | the app folder `<scoop>\apps\folio`, the `current` link's path, the old version folder's name, and the digests of **every** file in it (members, `install.json`, `manifest.json`, `folio-install.json`), and `marker` | the new version folder's name and the digests of every file in it; the digests of `H\<txn>\set\` and `H\<txn>\backup\` (equal to the two folders') |

### 1.4 What stays common

| step | owner today | adapter asked? |
|---|---|---|
| the check, the offer | `update::run`, `update_job::Job` | no |
| eligibility | `update_job::Evidence::eligibility` | the channel's answer widens, and each adapter adds its precondition (§1.5) |
| the card, its words and verbs | `update_card`, C9 | no: an eligible managed copy gets **the same card as ours** |
| the press, the worker, the checksum, abandonment | `update_prepare::{WORKER, fetching, sum_for, abandon, clear, finish}` | `Prepare` |
| the archive reader, the identity checks | `update_archive`, `bt_platform::trust`, `bt_platform::macos_identity` | no |
| the rescue copy | `update_prepare_windows`, `macos_update::rescue_clone` | no; its place follows the home (§1.6) |
| the quit barrier, `Handoff` | `update_handoff` (U-21) | no |
| the entrance at logon | `bt_platform::logon_hook`, the LaunchAgent door | no |
| admission, the process check | `update_startup::pass`, `install_flip::held_open` | the process check reads the final path (§1.6 L4) |
| the effects in `Moving` / `RollbackIntent` | `update_apply_windows`, `update_apply_macos` | `Activate` |
| the trial, the receipt, `Committed` | `update_trial`, `update_apply::watch_trial` | no |
| rollback's decision, `Stuck`, attempts | `update_txn::decide` | `Prove / Recover` |
| the exit guard, `opens_now` | `update_apply::ExitGuard` | which program is "installed" (§1.6 L3) |
| adoption, deferral, hand-back | revision (h) | the candidate executable (L3, L4) |
| the start's pass, the job owner's pass | `update_startup::pass`, `update_prepare::settle_at_launch` | the home's locator (§1.6 L1) |

The applier's line stays `--update-apply <home> <txn> <nonce>`.

### 1.5 Eligibility

| channel | answer |
|---|---|
| `Ours` | `Eligible` (unchanged) |
| `Managed { Homebrew, .. }` | `Eligible`, adapter `homebrew`, wherever the macOS gate is on **and** R-H2 holds; otherwise `NotEligible::Managed { command }` |
| `Managed { Scoop, .. }` | `Eligible`, adapter `scoop`, when §2.3's precondition holds (including no `hold`, Q2); otherwise `NotEligible::Managed { command }` naming the exact command (`scoop unhold folio` for a held copy, `scoop update folio` otherwise) |
| `Managed { Winget, .. }` | `NotEligible::Managed { command }` in 0.4.7 (design (a), §2.4): the constant `WINGET_ROAD` is `false` |
| `NotOurs`, `Unknown` | unchanged: the releases page |

Every precondition is read **before `Allocated`**: a copy that is not eligible
never has a journal. `update_prepare_windows::eligible` and
`update_prepare_macos`'s road check, which accept only `Channel::Ours` today,
accept the channel their adapter names and nothing else.

**The rollout.** The adapter's code runs in the **source** build (revision (h):
the applier and the recovery are copies of the running build). A managed copy of
0.4.6 or earlier has no adapter and keeps today's row until its manager updates
it once to 0.4.7; the first managed update through Folio is 0.4.7 → the next
release, and its release note says so. `MIN_UPDATER` stays 0.4.6.

### 1.6 The home, the installed program, and process identity

| adapter | the home `H` | why there |
|---|---|---|
| ours, Windows | `<exe folder>\.folio-update\` (unchanged) | the folder is the install unit |
| ours, macOS; Homebrew | `<parent>/.<Bundle>.folio-update/` (F-3, unchanged) | Homebrew never touches a sibling of the app target (HB5) |
| scoop | **`%LOCALAPPDATA%\Folio\update\<key>\`**, where `<key>` is the first 16 hex digits of the SHA-256 of the `current` link's path as launched, canonicalised and case-folded | outside `apps\folio`, which `scoop cleanup` and `scoop uninstall` empty of everything but the current version (S5, F4); per account, as a per-user scoop install is; `%LOCALAPPDATA%\Folio` is already Folio's (the WebView2 profile), and nothing new goes to the roaming profile (C6) |

**L1, the lookup.** A start reads the in-folder home first, as today. Only if
the executable's folder, as launched, is a directory link does it also compute
the scoop key and look there — one `symlink_metadata` more than today for every
start, and one `stat` more only behind a link. A home found this way counts only
when its journal's body names this link (the `Link` layout's recorded path, read
by the builds that wrote it — L1 is the rescue build's and later) and its
header's `rescue` lies inside that home. The key is a function of the link's
path alone, so the start finds the same home before and after `Activate`.

**L2, the scoop home exists only during a transaction.** Its retirement removes
it whole — lock, admission file, journal — once no process holds admission; a
file still held is left for the next start's retirement. `--uninstall-cleanup`
gains a per-copy row for it (a home whose journal names this copy's link, or a
link that no longer exists).

**L3, the installed program is named by the body.** `Home::of_rescue` derives
the installed program as `<home's parent>\folio.exe`, which is right only for
`Members`. For `Link` it is `<the recorded link>\folio.exe`; the rescue build
reads the body to know it (it is the same version that wrote it).

**L4, processes are told apart by the image's final path**
(`GetFinalPathNameByHandleW` on the process image): the process check before
`Activate`, H.3's candidate witness, the trial's stop. Through the junction, the
old and the new build are both launched as `…\current\folio.exe`; only the final
path (`…\apps\folio\0.4.7\folio.exe`) tells them apart. For the other layouts
the two paths are the same, so this narrows today's reads.

---

## 2. Per manager

### 2.1 zip / ours

Unchanged: W1–W15 and M1–M11 as built. L1 and L4 answer exactly as today for an
ordinary folder. Invariant test: passes (the rows as built and rehearsed).

### 2.2 Homebrew

**Facts** (Homebrew 7.0.6, §9).
- **HB1.** The Cookbook: `auto_updates true` "Asserts that the cask artifacts
  auto-update." Plain `brew upgrade` then upgrades the cask only when the
  bundle's `Info.plist` version is **older** than the tap's
  (`Cask::Cask#outdated_version` → `auto_updates_bundle_outdated?`; on by default,
  `HOMEBREW_UPGRADE_AUTO_UPDATES_CASKS`). The FAQ: "Blindly replacing the app
  based on that record could downgrade it."
- **HB2.** Every other comparison — a named `brew upgrade --cask folio`,
  `--greedy`, `--greedy-auto-updates`, `HOMEBREW_UPGRADE_GREEDY(_CASKS)` — is the
  Caskroom's record against the tap **by equality**
  (`installed_version` from `Caskroom/folio/.metadata/<version>/…`;
  `Cask::Upgrade.outdated_casks` asks `outdated?(greedy: true)`). A tap behind
  the bundle therefore **downgrades** it.
- **HB3.** `brew uninstall` copies whatever is at the app target back into the
  Caskroom and deletes it (`Cask::Artifact::Moved#move_back`); a zap step then
  runs from that copy.
- **HB4.** `brew upgrade` moves the new bundle's contents into the existing
  target folder (`Moved#move`, `Quarantine.copy_xattrs`), and `postflight_steps`
  writes the marker again (U-2).
- **HB5.** `Moved` acts only on the app target and on Caskroom paths; a sibling
  such as `/Applications/.Folio.app.folio-update/` is never touched.
- **HB6.** Third-party taps must be trusted (`docs/Tap-Trust.md`,
  `trust.rb` `raise_untrusted!`); the owner's Mac answers `brew info --cask folio`
  with "Refusing to load cask … from untrusted tap". The road never runs `brew`.
- **HB7.** Where the app target is recorded: `Cask#config_path` is `<Caskroom>/folio/.metadata/config.json` (`cask/cask.rb`), holding `{default, env, explicit}` (`Cask::Config#to_json`, `cask/config.rb`); the app folder is `explicit.appdir`, else `env.appdir`, else `default.appdir` (`/Applications`), and the artifact's name comes from the installed caskfile under `.metadata/<version>/<timestamp>/Casks/folio.{rb,json}` (`installed_caskfile`). Whether the cask's tab (`cask/tab.rb`, `uninstall_artifacts`) also stores the resolved target is not confirmed; R-H2 does not need it. `brew pin` works for casks at 7.0.6 (`Cask#pin`), but it pins the Caskroom version Homebrew installs, not what an `auto_updates` app does to itself, so it is not a hold on Folio's road (§8 Q2).

**Rules.**
- **R-H1. The cask declares `auto_updates true`** (`packaging/homebrew/folio.rb`;
  `update-manifests.ps1` and `cask.sh` keep it byte for byte;
  `check-manager-hooks.ps1` asserts it). With it, plain `brew upgrade` compares
  the bundle's own version and leaves an updated bundle alone (HB1).
- **R-H2. The road runs only at the app target Homebrew recorded.** Before
  `Allocated`, the running bundle's path (canonical) must equal the `Folio.app`
  target of the installed cask, read from HB7's record; and exactly one bundle
  carries the marker there. A custom `--appdir` is honoured by reading the
  record, not by assuming `/Applications`. A bundle moved or copied by hand, a
  second marked bundle, a record that cannot be read → `NotEligible::Managed`
  with `brew upgrade --cask folio` and **Copy**. The marker stays the channel's
  evidence of provenance; the record is the evidence that *this path* is
  Homebrew's live artifact (F7).
- **`Prepare`** = ours, macOS (U-27), plus: the running bundle's marker bytes,
  read before `Allocated` and recorded, are written onto
  `H/<txn>/stage/Folio.app` after its second identity check and read back equal
  (M1–M5, §4). The attribute is outside the seal (U-16's measurement;
  `docs/DESIGN.md` 2026-09-26, "The macOS updater can tell whether a copied
  bundle is the same publisher's Folio …"); E-M1 repeats it on a notarized,
  stapled bundle.
- **`Activate`, trial, commit, rollback** = ours, macOS (M1–M11). Extended
  attributes belong to the bundle directory and travel through `RENAME_SWAP`: the
  new live bundle carries the marker; rollback brings the old bundle back with
  its attribute unchanged.
- **Downloads and verifies:** Folio (U-27). The cask's `sha256` is the same
  bytes' hash and is not consulted. **Rescue copy:** the clone of the running
  bundle (U-26).
- **What Homebrew says afterwards.** `brew list --versions folio` names the
  version Homebrew installed (the Caskroom record is the one stale entry, as
  `auto_updates` declares). Plain `brew upgrade` leaves Folio alone while the
  bundle is at least the tap's version. A named or greedy upgrade reinstalls the
  tap's version when it differs from the Caskroom record; with the tap applied
  **before** the page is published (§5.1), that version is never older than a
  bundle Folio installed. `brew uninstall` and `brew uninstall --zap` act on the
  swapped bundle (HB3; E-M1 runs both after a swap).

**Invariant test.** M1–M11 as built: one complete bundle is live or in `stage/`
at every cut, the rescue clone runs offline, and Homebrew's record (the Caskroom
version) names a set Homebrew can still uninstall and upgrade — the stale version
number is not a disagreement Homebrew acts on (HB1) except through a named or
greedy upgrade, which after §5.1 reinstalls the same or a newer version.
**Passes**, subject to E-M1.

### 2.3 scoop

**Facts** (Scoop v0.5.3, `b588a06e`, §9).
- **S1.** An app lives in `<scoop>\apps\<app>\<version>\`; `apps\<app>\current`
  is a directory junction to it (`lib/install.ps1` `link_current`:
  `New-DirectoryJunction`, then `attrib +R /L`; `unlink_current`: `attrib -R /L`,
  then `Remove-Item`). With `NO_JUNCTION` there is no junction.
- **S2.** `install_app` writes `manifest.json` (`save_installed_manifest`, the
  bucket's manifest) and `install.json` (`save_install_info`: `architecture`,
  `bucket`, `url` for a URL install; `hold: true` after `scoop hold`).
- **S3.** The installed version is `current\manifest.json`'s `version`
  (`lib/versions.ps1` `Select-CurrentVersion`), and `app_status`, `update`,
  `uninstall`, `reset` and `cleanup` then read `versiondir <app> <version>`: the
  version must equal the folder's name. With no readable `current\manifest.json`,
  `Select-CurrentVersion` falls back to `Get-InstalledVersion`'s last entry: the
  folders holding `install.json`, ordered by that file's write time.
- **S4.** `scoop reset <app>[@<version>]` is `link_current` on that folder, then
  shims, shortcuts, env and persist made again. Folio's manifest has no
  `persist`, `env_add_path` or `env_set`. What the shim and the Start-menu
  shortcut record: `shim` (`lib/core.ps1`) runs `Convert-Path` on `apps\folio\current\folio.exe` and writes the result into `folio.shim`; `Convert-Path` converts PowerShell paths and is not known to resolve a junction, so the shim is expected to name `current`. **E-M2 measures it**, and if it names the resolved folder, `Activate` rewrites `folio.shim` exactly as `shim` writes it. The Start-menu shortcut's target is `[IO.FileInfo]::new(Combine($dir, $item)).FullName` with `$dir` = `current` (`lib/shortcuts.ps1` `startmenu_shortcut`), not resolved.
- **S5.** `scoop cleanup` (`libexec/scoop-cleanup.ps1`) and `scoop uninstall`
  delete every child of `apps\<app>\` except the current version and `current`,
  recursively; `cleanup` lists them with `Get-ChildItem $appDir -Name`, without `-Force`, so a Hidden child is skipped; `cleanup` has no running-process check (only `update`, `uninstall` and `reset` call `test_running_process`). With no readable `current\manifest.json`, S3's fallback decides which child is "current". Scoop has no lock.
- **S6.** `scoop update` leaves the old version folder until `scoop cleanup`; it
  updates only when the bucket's version is **newer** (`Compare-Version`), unless
  `-f` or `FORCE_UPDATE`; it skips, exiting 0, while any process runs from under
  `apps\<app>\` (`test_running_process`).
- **S7.** A manifest's `autoupdate` block is scoop's documented way to derive a
  version's manifest (`lib/autoupdate.ps1`; the wiki, "App Manifest Autoupdate").

**The chosen design** (revision (a)'s option (c), with the home moved and both
sets kept): Folio writes the version folder the way scoop does and points
`current` at it. Options (a) (swap inside `current\`) and (b) (run
`scoop update`) stay refused for revision (a)'s reasons.

**The precondition**, read before `Allocated`. The running executable's folder,
as launched, is a directory junction named `current`, whose target is a sibling
folder of the same parent, and:
- the app folder is under this account's scoop root (a global install under
  `%ProgramData%` stays on the Copy row);
- the target holds `install.json` **without `hold`** (a held copy: the row names
  `scoop unhold folio`, Q2), `manifest.json` whose `version` equals the running
  build's and whose `autoupdate` block uses only what the renderer reads
  (`$version`, `$basename`, the hash line of `SHA256SUMS.txt`), and the scoop
  marker;
- `<scoop>\apps\folio\<offer's version>\` does not exist.
Anything else: `NotEligible::Managed` with the exact command, no journal.

**`Prepare`**
1. The download, checksum, archive reader and identity checks of U-20, into
   `H\<txn>\` (`H` per §1.6).
2. `H\<txn>\set\` receives the archive's members (durably, checked again where
   they lie), then **the bookkeeping**:
   - `install.json`: the running version folder's, byte for byte;
   - `manifest.json`: the running version folder's with the lines carrying
     `version`, the 64-bit `url`, `hash` and `extract_dir` replaced by its own
     `autoupdate` block's rendering for the offer — line by line, each line
     present exactly once, as `update-manifests.ps1` renders the bucket. **The
     renderer refuses** (→ `Abandoned`, *Nothing changed.*) unless the rendered
     `url` is byte-equal to the URL Folio downloaded the archive from, the
     rendered `extract_dir` equals the archive's one root, the rendered `hash`
     equals the verified `SHA256SUMS.txt` line **and** the digest of the archive
     Folio holds, and the rendered `version` equals the offer's (F5). This file is
     scoop's bookkeeping, not a carried marker (M5);
   - `folio-install.json`: the carried marker (§4).
3. `H\<txn>\backup\` receives a copy of **every** file of the running version
   folder, each checked against the digests recorded at `Allocated`.
4. `apps\folio\<v>\` is made and filled from `set\`, `install.json` written last
   (so scoop does not list a partial folder, S3's glob), each file durable and
   checked.
5. The rescue copy into `H\<txn>\rescue\`; `Prepared` with the `Link` layout.

From step 4 on, scoop lists `<v>` as installed but not current.

**`Activate` forward**, after `Moving` is durable, under exclusive admission,
after the process check by final path (L4):
- **if E-M5 proves an atomic retarget** — the driver documentation of `FSCTL_SET_REPARSE_POINT` describes modifying an existing reparse point in one call when the tag matches ("If an existing reparse point is being modified, the tag … must match"; `STATUS_IO_REPARSE_TAG_MISMATCH` otherwise), so a junction (`IO_REPARSE_TAG_MOUNT_POINT`) can be given a new target without being deleted first; whether a concurrent opener can observe anything but the old or the new target is not documented, which is what E-M5 measures — `current`'s target is
  replaced in that one operation: at every instant `current` names a complete
  folder;
- **otherwise**, two calls as `unlink_current` and `link_current` make them
  (`attrib -R /L`; the link removed, never recursively; the parent flushed; the
  junction created to `apps\folio\<v>`, `attrib +R /L`, flushed, read back). The
  interval with no `current` is guarded by construction, not by timing (below).

**`Activate` back**: before pointing `current` at the old folder, `Prove`
compares the old folder with the digests recorded at `Allocated`; any file
missing or different is restored from `H\<txn>\backup\` (the folder recreated if
it is gone, `install.json` last); then the same one- or two-call retarget. After
`RolledBack` is durable, the files of `apps\folio\<v>\` the journal records are
removed.

**`Prove`** reads the live side `L` from the junction (old, new, or none) and
checks the target folder by digest. **`Recover`** repairs a version folder from
`set\` or `backup\` before any retarget at it.

**Why `scoop cleanup` cannot break it (F4).** The home, `set\` and `backup\` are
outside `apps\folio`, which is the only place scoop deletes (S5); the rescue runs
from the home. So from `Prepared` to retirement **both complete sets exist
outside scoop's reach**, and every version folder scoop could delete is one
`Recover` can rebuild from them offline, by recorded digests. Case by case, with
`scoop cleanup` run at the worst moment:

| moment | what cleanup deletes | the next actor |
|---|---|---|
| SW2 (`Prepared`) | `apps\folio\<v>\` (not current) | the resume's revalidation finds it gone: `Recover` rebuilds it from `set\` before `Activate`, or the job owner discards (*Nothing changed*). The old install was never touched. |
| between unlink and link (two-call form only) | S3's fallback picks the folder whose `install.json` is newest — `<v>`, written at Prepare — and deletes the **old** folder | `Activate` goes on to link `<v>` (complete; it is the one cleanup kept); a rollback restores the old folder from `backup\` first |
| the same, if the fallback picks the old folder | `apps\folio\<v>\` | `Prove` finds the target incomplete before linking; `Recover` rebuilds it from `set\`, then links |
| `Trial` | the old folder (not current) | a commit needs nothing from it; a rollback restores it from `backup\` |
| `RollbackIntent` before the retarget | the old folder | restored from `backup\`, then retargeted |
| `RollbackIntent` after the retarget | `apps\folio\<v>\` | nothing needed: the old side is live; the journal's removal of `<v>` finds nothing to remove |

E-M5 runs every row of this table with a real `scoop cleanup` and a power cut
(§6).

**Power cuts: the SW rows** (`L` is the live side from the junction).

| # | durable state | on disk | next actor |
|---|---|---|---|
| SW1 | `Allocated` | old live; partial `H\<txn>`; perhaps a partial `apps\folio\<v>\` (no `install.json` yet, so scoop does not list it) | as W1, plus the partial version folder removed by the journal's record |
| SW2 | `Prepared` | old live; `<v>` complete; `set\`, `backup\` complete | as W2 (revalidation includes `Recover` of `<v>` from `set\`) |
| SW3–SW5 | `Handoff`, `Armed` | as SW2 | as W3–W5 |
| SW6 | `Moving` | `L` old, none, or new | `L` old → `Prepared` (M5's rule). `L` none → `Recover` the old folder, link it, `Prepared`. `L` new → `RollbackIntent`, then SW9. |
| SW7, SW8 | `Trial` | `L` new | as W7, W8 |
| SW9 | `RollbackIntent` | `L` new, none, or old | stop the trial (L4); `Recover` the old folder from `backup\`; if `L` is not old, retarget; old live and whole by digest → `RolledBack`; a failure → `Stuck` |
| SW10–SW13 | `Stuck`, `RolledBack`, `Committed` with debt, `Abandoned` | as W10–W13 | as W10–W13; after `RolledBack`, the recorded files of `<v>` removed; after `Committed`, `set\` and `backup\` retired with `H\<txn>` |

While `L` is none (two-call form only), no start can run — the shim's target does
not exist — and the entrance at logon, which names `H\<txn>\rescue\folio.exe`
outside `apps\folio`, finishes the step. This is what W4–W6 already require of a
Windows copy mid-move.

**What scoop says afterwards.** `scoop list`/`status` name `<v>` (S3); while the
bucket names an older version, `scoop update folio` reports it up to date (S6),
unless the person passes `-f` or sets `FORCE_UPDATE`; `scoop cleanup folio`
removes the old folder (Q4: left to it); `scoop reset folio@<old>` goes back by
scoop's hand; `scoop uninstall folio` runs `pre_uninstall` from
`current\manifest.json`, the rendered copy of the hook the old version had.

**Invariant test.** At every SW row, both complete sets exist in `H` outside
scoop's reach and one of them is live or reconstructible offline before any
start can run; scoop's record (`current\manifest.json`, `install.json`) always
belongs to the live folder, because it travels inside it. **Passes**, subject to
E-M2 (the launch surfaces follow `current`) and E-M5.

### 2.4 winget

#### 2.4.1 Facts (winget-cli v1.29.380, `000f6b55`, §9)

- **WG1.** winget is not live for Folio: microsoft/winget-pkgs#431006 is open.
  Today's winget copies were installed from a local manifest; their record names
  a local source (`WeiyiShi.Folio__DefaultSource`, U-4).
- **WG2. The install.** A zip holding a portable is extracted into
  `%LOCALAPPDATA%\Microsoft\WinGet\Packages\<PackageId>_<SourceId>\`
  (`GetPortableProductCode`, `PathName::PortablePackageUserRoot`); with
  `--scope machine`, `%ProgramFiles%\WinGet\Packages\<ProductCode>\` with its record under HKLM (`GetPortableInstallRoot`, the `PortableARPEntry` constructor); an upgrade keeps the recorded scope (`InitializePortableInstaller`) and writes HKLM and Program Files.
- **WG3. What winget records.** **The portable index** `<ProductCode>.db` (`GetPortableIndexFileName`), Hidden, in the install folder, written for archive installs only: SQLite, table `portable(filepath TEXT UNIQUE COLLATE NOCASE, filetype INT64, sha256 BLOB, symlinktarget TEXT)` and a `metadata` table with `majorVersion`/`minorVersion` 1.0 (`Schema/Portable_1_0/PortableTable.cpp`, `SQLiteMetadataTable.cpp`). winget opens only 1.0 and refuses any other (`ERROR_NOT_SUPPORTED`, `APPINSTALLER_CLI_ERROR_CANNOT_WRITE_TO_UPLEVEL_INDEX`; `PortableIndex::CreateIPortableIndex`); there is no migration (the portable index: file,
  schema, version) and **the uninstall record** `HKCU\…\Uninstall\<ProductCode>` (`RegisterARPEntry`, `SetAppsAndFeaturesMetadata`, `CreateTargetInstallDirectory`, `InstallFile`): `WinGetPackageIdentifier`, `WinGetSourceIdentifier`, `UninstallString` (`winget uninstall --product-code …`), `WinGetInstallerType`, `DisplayName`, `DisplayVersion`, `Publisher`, `InstallDate`, `URLInfoAbout`, `HelpLink`, `InstallLocation`, `InstallDirectoryCreated`, `InstallDirectoryAddedToPath` (the uninstall record's values), and the
  `PATH` entry (`AddToPathVariable`, `PathVariable.cpp`): the folder holding the executable appended to `HKCU\Environment` `Path` (`REG_EXPAND_SZ`), and `WM_SETTINGCHANGE "Environment"` broadcast. **A tracking catalog**, `installed.db`, in the App Installer package's `LocalState\<source id>\` (`RecordInstall`, `GetPackageTrackingFilePath`): winget's general index schema, created at the latest schema version of the running winget (V1_7 at the pin; `CreateISQLiteIndex`), holding the installed version, `PinnedState`, architecture, locale and the install's intent. None of the four is documented as a public or stable contract (any other tracking state).
- **WG4. An upgrade's order** (`PortableInstaller::Install` →
  `ApplyDesiredState`): every expected (old) entry is removed, then for each
  desired entry the index row is written **before** its file is installed
  (`AddOrUpdatePortableFile`, then `InstallFile`); on an upgrade the uninstall
  record is written **after** `ApplyDesiredState` (`RegisterARPEntry`). There is
  no rollback on an upgrade (`PortableInstallImpl` cleans up only when not an
  update).
- **WG5.** Before an upgrade or uninstall, `VerifyExpectedState` re-hashes every
  recorded file; a mismatch stops with 0x8A150057 unless `--force`.
- **WG6.** Installs take `CrossProcessInstallLock`: a second winget **waits**.
- **WG7. Pins** live in `pinning.db` (`LocalState`, table `pin(package_id, source_id, type, version)`). An explicit `winget upgrade --id X --version V` considers pins (`GetManifestWithVersionFromPackage`): a *pinning* pin blocks it unless `--include-pinned` or `--force`, a *blocking* pin unless `--force`, and a *gating* pin blocks a `V` outside its range unless `--force`; a blocked upgrade ends with 0x8A150068 (`EvaluatePinnedStateForVersion`, `PinningData.cpp`). Without `--version`, a single-package upgrade includes pinning pins (`SelectLatestApplicableVersion`).
- **WG8.** `winget show`/`upgrade` may refresh the source when its cache is older
  than `autoUpdateIntervalInMinutes` (15 by default); `--accept-source-agreements`
  **accepts** the agreements rather than only suppressing the prompt.

#### 2.4.2 Two designs, side by side, with the invariant test

**Design (a): winget stays on the Copy row in 0.4.7.** `WINGET_ROAD = false`.
The row names `winget upgrade --id WeiyiShi.Folio --exact` with **Copy**, as
today. Eligibility code for winget may exist (the precondition reads: community
source, per-user scope, no pin; U-41d) but never raises a card while the
constant is off. **There is no catalogue probe in 0.4.7**: Folio starts no
`winget` process and sends nothing to Microsoft's source, so `PRIVACY.md` gains
nothing for winget. The disclosure is one line in the note (this paragraph), in
`CHANGELOG.md` under the release's **Changed** ("A copy installed with winget
still updates with `winget upgrade`; the card is for zip, scoop and Homebrew
copies."), written by U-41e, and in the release note.

*Invariant test:* nothing destructive is performed by Folio or by winget on
Folio's account, so there is nothing to interrupt. **Passes.** Cost: the
2026-09-27 ruling is not met for winget copies in 0.4.7 — of which, with WG1,
there are none from the public source today.

**Design (b): Folio swaps the bytes in winget's install location and repairs
winget's record offline.** The only design that could satisfy the invariant
without the network, because every step would be Folio's own, journaled, and
repeatable from a local description. It would be:
1. `Prepare`: stage the new set in `H` (outside winget's package folder); record
   the old index rows and the old uninstall record's values;
2. `Activate` forward: move files into `<InstallLocation>\folio-<new>\`, remove
   `folio-<old>\`, rewrite the portable index rows (paths, SHA-256), rewrite the
   uninstall record (`DisplayVersion`, and every value that names the version
   folder), move the `PATH` entry from `folio-<old>` to `folio-<new>` and
   broadcast the environment change;
3. `Activate` back: the same writes with the old description.

What that requires, per the pinned source (WG3): Folio would have to write, consistently and offline, four stores winget owns:
   - the **portable index**: SQLite at schema 1.0, which winget opens only at exactly 1.0. Folio could write it (a fixed, small table), but it is winget's internal format with no contract, and a winget that moves to 2.0 refuses Folio's file or migrates it on its own terms;
   - the **uninstall record**: thirteen registry values that Folio could write (it reads four already, U-4);
   - the **`PATH` entry**: `HKCU\Environment` and a broadcast, which Folio could perform;
   - the **tracking catalog** `installed.db`: winget's general index, written at **the running winget's latest schema** (V1_7 at the pin; seven versions so far), inside the App Installer package's own `LocalState`. Folio cannot write this one consistently: its schema moves with winget releases, it is another packaged application's private data, and a record Folio did not update leaves `winget list` and `winget upgrade` reading a version the files are not.

*The cut points.* Folio's own order could be made safe (index written after the
files, record last, each step journaled), but the design has to coexist with
winget's order too: a person's `winget upgrade` or `winget uninstall` run
between Folio's steps meets an index whose rows do not match the files, and
WG5 stops it with 0x8A150057 — or, with `--force`, winget's `ApplyDesiredState`
removes every indexed entry before writing the new ones (WG4), so a Folio
transaction interrupted by the person's forced upgrade is left with winget's
half-emptied folder and winget's own unfinished record (index row before file;
record after the whole apply). Folio's recovery would then have to repair
winget's partial state as well as its own.

*The ten-minute kill (F9).* Revision (a) ran `winget upgrade` as a child and
killed it at ten minutes. winget waits on `CrossProcessInstallLock` (WG6): it
can wait nine minutes behind another install, take the lock, enter
`ApplyDesiredState`, and be killed by Folio during the removal of old entries —
Folio itself would create the neither-set state. Design (b) runs no winget
child, so the hazard does not arise inside it; it is the reason revision (a)'s
winget-runs-the-step design is withdrawn outright rather than repaired with a
longer bound.

*Invariant test for (b):* **fails.** Offline, Folio could keep the files, the index, the uninstall record and `PATH` in step, but not winget's tracking catalog, whose schema is the running winget's and is not Folio's to write. So after an interrupted or a completed Folio swap, "the manager's own record agrees, or can be made to agree offline" does not hold. A design that writes three of winget's four stores and leaves the fourth to disagree is the "repair winget's state" hazard in another form.

**Recommendation: (a).** Design (b) fails the invariant test on winget's tracking catalog; revision (a)'s design (winget runs the step) fails it on WG4's destructive order and on the lock-wait kill. Design (a) passes, costs nothing on the public source today (WG1), and leaves the adapter slot open: a winget road needs either a winget operation that is transactional (a staged install winget can roll back offline) or a documented record Folio may write, and it gets a design review of its own when one exists. Machine-scope winget copies stay on the
Copy row under either design (F9): `install_channel` reads only the account's
HKCU record, and a machine-scope upgrade may need elevation.

---

## 3. What can fail

### 3.1 Finding a manager

No manager program is run by the road in 0.4.7: Homebrew and scoop are never
run (R3), and winget is on the Copy row (§2.4). `ARCHITECTURE.md` §2.2 gains no
child kind.

### 3.2 The failure table

Each row: the cause, the card the person sees (C9's words), and the durable
journal outcome. Rows marked **(Codex)** are the review's F10 sequences,
verbatim in the first column.

| # | sequence | card | journal |
|---|---|---|---|
| F1 | **(Codex)** winget is missing/disabled at probe | no card; the row's **Copy** `winget upgrade --id WeiyiShi.Folio --exact` (design (a): there is no probe, so every winget copy is here) | none |
| F2 | **(Codex)** winget disappears after `Prepared`, before its step | not reachable under (a): a winget copy never reaches `Prepared` | none |
| F3 | **(Codex)** winget waits on another manager operation | not reachable under (a) (WG6 is the hazard of §2.4.2) | none |
| F4 | **(Codex)** network fails during source lookup/download | Folio's own download (every adapter): `Failed` · the reason + *Nothing changed.* · **Releases** · **Close**; a winget source lookup: not reachable under (a) | `Abandoned` → retired |
| F5 | **(Codex)** winget installs another byte set/version | not reachable under (a); Homebrew and scoop run no manager and always install the offered archive Folio verified | none |
| F6 | **(Codex)** concurrent `winget uninstall --purge` | not reachable under (a): Folio has no home or journal in winget's package folder | none |
| F7 | **(Codex)** Homebrew upgrade beside Folio | M4/M6 identity decides: before the exchange, a live bundle that is not the recorded old identity → `Reverted`: *Nothing changed.*; after it, not the recorded new identity → `RollbackIntent` → `Stuck`: *Update incomplete.* + the journal's folder · **Show folder** · **Close** | `Prepared`, or `Stuck` (recoverable: the rescue clone and `stage/` keep a complete bundle) |
| F8 | **(Codex)** Scoop update beside Folio | scoop skips while old Folio or the rescue runs under `apps\folio` (S6) — the rescue now runs from `%LOCALAPPDATA%`, so from `Handoff` on only the old build's exit leaves the window; if scoop's update runs then: before `Allocated`, `<v>` exists → no card this launch; at Prepare step 4, `<v>` exists → *Nothing changed.*; at `Activate`, `current` does not target the recorded old folder → the process check refuses → `Reverted` → the old build (scoop's new one, in fact) starts plainly | none, `Abandoned`, or `Prepared` (then discarded at revalidation) |
| F9 | **(Codex)** Scoop cleanup beside Folio | none: §2.3's table — `Recover` rebuilds whatever it deleted from `set\` or `backup\` | the row's own outcome, unchanged by cleanup |
| F10 | **(Codex)** one managed and one unpacked copy | each copy has its own home and journal (L1 keys the scoop home by link path); the shared data directory's claim can defer one road (H.3); each card and channel are read from its own executable | independent per copy; a deferral records nothing |
| F11 | **(Codex)** managed copy moved by hand | winget: registry containment refuses it (U-4); scoop: the junction precondition refuses it (a moved tree has no `current` junction under a scoop root that names it); Homebrew: R-H2 refuses it — each keeps the Copy row | none |
| F12 | the marker cannot be read, changed since `Allocated`, or cannot be written/read back | *Nothing changed.* | `Abandoned` (M2) |
| F13 | scoop's precondition fails (`NO_JUNCTION`, held, global, a renderer case it does not read, `<v>` already there) | no card; the row names the exact command | none |
| F14 | the scoop renderer's equality check fails (§2.3 Prepare step 2) | *Nothing changed.* | `Abandoned` |
| F15 | Xcode Command Line Tools requested | never: Homebrew is not run | — |

No row adds a refusal, a prompt or a confirmation the person meets; the
preconditions keep today's row where the road cannot complete.

---

## 4. The marker and the channel

**Q3 is answered yes.** The five pins (F11):

- **M1. Read first, recorded.** The marker bytes are read before `Allocated`
  (U-1's read), recorded in the journal, written **only** to the staged/new set,
  read back equal, and checked again **on the live side after `Activate`**.
- **M2. Refused before anything destructive.** An absent, unreadable, changed
  (differing from the recorded bytes at any later read) or unwritable marker
  abandons the transaction before `Armed`: *Nothing changed.*
- **M3. Every row is managed.** Every Homebrew M row and every scoop SW row
  asserts `Channel::Managed` for the executable that can start there (a test per
  row, §6).
- **M4. Rollback restores the old marker unchanged**: the old bundle returns with
  its own attribute (Homebrew); the old version folder is live again, restored
  from `backup\` byte for byte where needed (scoop).
- **M5. The manager still composes it.** The hook tests
  (`install_channel::tests::the_marker_each_package_manager_writes_reads_as_that_manager`,
  `check-manager-hooks.ps1`, `check-scoop-hooks-in-vm.ps1`,
  `check-cask-hooks.sh`) keep proving that the manager writes the canonical
  marker on its own next install or update. The rendered scoop `manifest.json`
  is **manager bookkeeping, not a carried marker**, and has its own proof (the
  renderer's equality check, §2.3; E-M2).

**`RULES.md` row 41** is narrowed (by U-41e) from "The marker is written by the
package manager, never by Folio" to: "**The marker is composed by the package
manager; Folio may carry those exact bytes across an update it performs.**"

| | lives | after Folio's update | after the manager's own next update |
|---|---|---|---|
| Homebrew marker | the attribute on the recorded app target | carried onto `stage/Folio.app` before the swap | the cask's `postflight_steps` |
| scoop marker | `folio-install.json` in the version folder | carried into `set\` and `apps\folio\<v>\` | scoop's `post_install` |
| scoop bookkeeping | `install.json`, `manifest.json` | copied and rendered (§2.3) | scoop |
| winget | winget's record (no marker) | — (Copy row) | winget |

**A hook changed in a new release** reaches a copy Folio updated only at the
manager's next own update (scoop's `manifest.json` is rendered from the running
version's). The door's grammar is frozen, so either order is safe; a release
that changes a hook says so in its note and in §5.1's checklist.

---

## 5. What else changes

### 5.1 The release order (U-41e0, can land now)

`docs/RELEASING.md` today publishes the page first and updates the manifests
after. The new order, each step a gate for the next:

1. Create the release as a **draft** with every asset, and verify it (the
   existing checks: signatures, `smoke.ps1`, the checksum files).
2. `update-manifests.ps1 -FromRelease` renders the cask and the bucket from the
   draft's checksum files (`gh release download` reads a draft for an account
   with push rights); the printed difference is reviewed.
3. `-Apply` writes both, then **reads both remote blobs back** and compares them
   with what it rendered; if either differs, or only one write landed, it stops
   and the page is **not** published until both are confirmed.
4. Publish the page. The anonymous releases list — the feed — now names the
   version; `update::run` offers it.
5. Submit the winget manifest (`wingetcreate update …` or the hand PR,
   `RELEASING.md` "winget") at once; the common feed is never held for winget.
6. winget copies stay on the Copy row (design (a)).

Between steps 3 and 4, `brew upgrade` and `scoop update` see a version whose
asset answers 404 (a draft's assets are not public): both download before they
uninstall or relink, so the old install stays — a harmless unavailable download,
where the old order left a possible downgrade after Folio had installed the new
build (F6). `update-manifests.ps1` gains the read-back and the refusal to report
success on a partial apply; `packaging/homebrew/folio.rb` gains
`auto_updates true`; `check-manager-hooks.ps1` asserts both. `docs/install.md`
says that `brew install --cask lulu-loopp/folio/folio` trusts the cask (HB6).

### 5.2 The card and the row

An eligible copy gets C9's card with no manager word on it. The row keeps the
exact manager command with **Copy** wherever the answer is
`NotEligible::Managed` (§1.5). No new `Text` variant in 0.4.7 (revision (a)'s
`RowFoot::Waiting` is withdrawn with the probe); a held scoop copy's command is
`scoop unhold folio`, a string, not a new state.

### 5.3 `ARCHITECTURE.md` rows

- §2.2 (child processes): no new kind.
- §6 (doors): the junction retarget is a new effect of `bt_platform::install_flip`
  (`relink`, `link_target`), the only writer of a scoop `current`; the marker
  write is a new effect of `bt_platform::macos_update` (`carry_marker`); the
  image's final path is a new read of `install_flip` (`image_final_path`); the
  scoop home is a new place `install_txn` writes (under `%LOCALAPPDATA%\Folio`).
- §4.4: *how this copy was installed* keeps its owner (`install_channel`) and
  gains a carrier (§4), listed under (c′) of each ticket with its readers.

### 5.4 `PRIVACY.md` (English and Chinese halves)

- **Updating.** Homebrew: *replaced in place, as for a copy installed by hand;
  Homebrew's record keeps the version it installed.* scoop: *the new version is
  placed beside the old one in scoop's folder, the way scoop places it; the old
  one stays until `scoop cleanup`.* winget: unchanged (*its row names the
  manager's command*).
- **What is written outside Folio's folder.** During a scoop copy's update only:
  `%LOCALAPPDATA%\Folio\update\<id>\` (the download, the new files, a copy of the
  running version, the journal, the log); removed when the update ends and by
  `--uninstall-cleanup`.
- No new network request (no winget probe in 0.4.7).

---

## 6. Tests and experiments owed (phase 2)

**Tests** (names are sentences; each red on BASE; the real road over temporary
folders; no manager runs, so no fake manager is needed in 0.4.7 — a fake winget,
if design (b) is ever built, must model the portable index and the uninstall
record as well as the files, F14).

| test | adapter |
|---|---|
| `a_scoop_copy_is_offered_the_card_and_not_the_command` | scoop |
| `a_held_scoop_copy_keeps_the_row_with_the_unhold_command_and_no_journal` | scoop |
| `a_scoop_copy_without_the_junction_layout_keeps_the_command` | scoop |
| `the_rendered_manifest_equals_the_bucket_rendering_and_names_the_exact_offer` | scoop |
| `a_rendered_manifest_naming_another_url_or_hash_abandons_with_nothing_changed` | scoop |
| `the_scoop_home_is_outside_the_app_folder_and_found_from_the_link_before_and_after` | scoop |
| `a_version_folder_deleted_at_any_sw_row_is_rebuilt_from_the_home` | scoop |
| `the_scoop_activate_writes_nothing_in_the_old_folder` | scoop |
| `a_scoop_activate_cut_between_unlink_and_link_is_finished_to_the_old_side` | scoop |
| `a_scoop_rollback_restores_the_old_folder_from_backup_and_removes_only_recorded_files` | scoop |
| `an_old_and_a_new_build_behind_one_junction_are_told_apart_by_final_path` | scoop |
| `a_homebrew_copy_at_its_recorded_target_is_offered_the_card` | Homebrew |
| `a_moved_or_second_marked_bundle_keeps_the_command` | Homebrew |
| `the_marker_is_carried_byte_for_byte_and_checked_on_the_live_side` | Homebrew, scoop |
| `a_marker_that_changed_since_allocation_abandons_before_armed` | Homebrew, scoop |
| `every_start_at_every_row_reads_the_same_managed_channel` | Homebrew, scoop |
| `rollback_restores_the_old_marker_unchanged` | Homebrew, scoop |
| `the_cask_declares_auto_updates` | Homebrew (`check-manager-hooks.ps1`) |
| `a_winget_copy_never_raises_the_card_while_the_road_is_off` | winget |
| `no_ours_row_changes` (every existing W and M row test still green, untouched) | ours |

**Rows.** Homebrew: M1–M11 with the marker asserted live (`mac/row.sh`, scratch
`HOME` and `--appdir`). scoop: SW1–SW13 in
`scripts/release/cleanvm/updater/rows.ps1`, on the VM with scoop, each also run
with `scoop cleanup folio` at the row. `clean-vm.md` §4.4 gains 「scoop 副本」:
the install step (`check-scoop-hooks-in-vm.ps1`'s install half), the rows, and
what `scoop status`, `scoop list`, `scoop reset` and `scoop uninstall` print after
`Committed` and after `RolledBack`.

**Experiments** (Codex's F13, as written):

| id | question | before |
|---|---|---|
| E-M1 (Homebrew) | notarized/stapled bundle; marker through stage and `RENAME_SWAP`; `codesign`, `spctl`, offline first launch; plain, named, and greedy upgrade with tap behind/equal/ahead; `brew uninstall --zap` after the Folio swap; custom appdir and moved-copy refusal | U-41b |
| E-M2 (Scoop) | inspect `.shim` and Start-menu targets; invoke both after Folio relink and after cleanup; prove status/list/update/reset/uninstall accept the synthetic folder; include `hold`, URL installs, and the supported autoupdate grammar | U-41c |
| E-M3 (winget) | retain the sibling home while upgrading and uninstalling, but also cut at every portable-index/file/ARP boundary; retry with and without network; verify record version/source/scope/location and PATH; run from the detached old rescue; cover a competing winget lock and the ten-minute boundary; cover `--purge` and explicitly exclude or test machine scope | any winget road (not 0.4.7 under (a)) |
| E-M4 (winget catalogue) | missing-version exit code, cold/warm/disabled source cache, missing agreement, `--disable-interactivity`, proxy/offline, pin types, exact version, and whether `--accept-source-agreements` writes state | any winget probe (not 0.4.7 under (a)) |
| E-M5 (Scoop cleanup/atomicity) | real `scoop cleanup` at SW2, between unlink/link, Trial and RollbackIntent, with power cuts; determine whether an atomic NTFS junction replacement is available. Scoop's own `reset` is demonstrably two calls, not atomic evidence. | U-41c |
| E-M6 (multiple/moved copies) | managed plus unpacked copies sharing the data root; copied/moved Homebrew marker; moved Scoop tree; winget record for another location | U-41a |

**Minimum manager versions supported** (the design reads these contracts; older
versions keep the Copy row where the precondition can tell, and are otherwise
unsupported): scoop **v0.1.0 or later** (the `install.json` glob of `Get-InstalledVersion` arrived in the 2021-11-22 release, `59088a9f`; read at v0.5.3); Homebrew **7.0.6 or later** (every Homebrew claim was read at 7.0.6; R-H2 reads `config.json`, which is older); winget: none in 0.4.7 (design (a)). A scoop older than v0.1.0 is not told apart by the precondition and is unsupported; the road's own proof (the target folder's digests, the junction's target) still refuses anything but the layout it wrote.

---

## 7. The ticket cut for phase 2

| # | title | size | inputs | builds |
|---|---|---|---|---|
| **U-41e0** | the release order | S | — (**can land now**) | §5.1: `RELEASING.md`'s order and gate, `update-manifests.ps1`'s read-back and partial-apply refusal, `auto_updates true` in the cask, `check-manager-hooks.ps1`, `docs/install.md`'s trust line |
| U-41a1 | the journal and the adapter interface | M | — | `Layout::Link`, the `marker` field, the `Prepare`/`Activate`/`Prove-Recover` seam over the two existing layouts (no behaviour change: every W/M row test untouched and green), eligibility by adapter behind one constant each |
| U-41a2 | homes, programs and identity | M | U-41a1, E-M6 | L1 (the link lookup and the `%LOCALAPPDATA%` home), L2 (retirement, the cleanup row), L3, L4 (final-path identity in the process check, H.3's witness, the trial's stop), the multiple-copy tests |
| U-41b | Homebrew | S | U-41a1, E-M1 | R-H2 (the recorded target), the carry (M1–M5), the Homebrew constant on, the M rows with the marker |
| U-41c | scoop | M–L | U-41a2, E-M2, E-M5 | the renderer and its equality check, the version-folder `Prepare` with `set\`/`backup\`, `install_flip::relink`, `Recover`, the SW rows with cleanup, the VM checklist |
| U-41d | winget eligibility, off | S | U-41a1 | the precondition (community source, per-user scope, no pin) and `WINGET_ROAD = false`, with the test that it never raises a card. **L**, and a new design review, if design (b) is ever chosen |
| U-41e | docs, privacy, rules, cards | S | U-41b, U-41c, U-41d | `PRIVACY.md` (both halves), `RULES.md` row 41's narrowing, `ARCHITECTURE.md` rows, the CHANGELOG lines (including the winget disclosure), the release note |

U-41a is split in two because it was L (F14): U-41a1 is the protocol with no new
behaviour, U-41a2 the places and identities. U-41b and U-41d need only U-41a1;
U-41c needs both.

---

## 8. The owner's answers (recorded)

1. **winget before it is live:** build it, with eligibility off until the design
   question of §2.4 is resolved; under design (a), winget copies keep the Copy
   row in 0.4.7 (U-41d builds the precondition and the constant, off).
2. **Holds and pins:** honoured before allocation; no card; the row names the
   exact command (`scoop unhold folio`; for winget, the exact `winget pin remove`
   line once a winget road exists). Winget's pin types are measured in E-M4
   before any winget road. Homebrew has no cask pin that stops an app's own
   updater (§9), so no Homebrew case.
3. **Carrying the marker:** yes, with M1–M5; `RULES.md` row 41 narrowed as §4
   says.
4. **scoop's old version folder:** left for `scoop cleanup`, as after scoop's own
   update. Folio removes only an uncommitted new folder whose recorded files it
   created, after `RolledBack`.
5. **`scoop cleanup` during an apply:** not an accepted limit. §2.3 makes it
   harmless by construction (both sets outside scoop's reach), and E-M5 rehearses
   it at every row.

---

## 9. Sources

**Pins.**

| project | pinned at | URL base (every file below is under it) |
|---|---|---|
| scoop | release v0.5.3, commit `b588a06e41d920d2123ec70aee682bae14935939` | `https://github.com/ScoopInstaller/Scoop/blob/b588a06e41d920d2123ec70aee682bae14935939/` |
| winget-cli | release v1.29.380, commit `000f6b55151cb0f1afd2933bb54c62a4724b9ca8` | `https://github.com/microsoft/winget-cli/blob/000f6b55151cb0f1afd2933bb54c62a4724b9ca8/` |
| Homebrew | release 7.0.6, commit `570982948a8a194f0f42f43f4a5bce2d1c9f64cb` (the Mac mini's `brew --version`; 7.0.7 exists upstream) | `https://github.com/Homebrew/brew/blob/570982948a8a194f0f42f43f4a5bce2d1c9f64cb/` |

scoop:
- `lib/install.ps1`: `install_app`, `link_current`, `unlink_current`, `save_installed_manifest`, `save_install_info`, `create_shims`.
- `lib/core.ps1`: `shim`, `app_status`, `test_running_process`.
- `lib/shortcuts.ps1`: `create_startmenu_shortcuts`, `startmenu_shortcut`.
- `lib/versions.ps1`: `Select-CurrentVersion`, `Get-InstalledVersion`.
- `lib/autoupdate.ps1`; the wiki, "App Manifest Autoupdate": https://github.com/ScoopInstaller/Scoop/wiki/App-Manifest-Autoupdate.
- `libexec/scoop-update.ps1` (`update`), `scoop-reset.ps1`, `scoop-cleanup.ps1` (`cleanup`), `scoop-uninstall.ps1`, `scoop-hold.ps1`; `CHANGELOG.md` (the 2021-11-22 release).

winget-cli:
- `src/AppInstallerCLICore/PortableInstaller.cpp` and `.h`: `Install`, `ApplyDesiredState`, `VerifyExpectedState`, `RegisterARPEntry`, `InstallFile`, `GetPortableIndexFileName`, `InitializePortableInstaller`.
- `src/AppInstallerCLICore/Workflows/PortableFlow.cpp`: `GetPortableProductCode`, `GetDesiredStateForPortableInstall`, `PortableInstallImpl`, `PortableUninstallImpl`.
- `src/AppInstallerCLICore/Workflows/InstallFlow.cpp`: `ExecuteInstallerForType`, `ExemptFromSingleInstallLocking`, `RecordInstall`.
- `src/AppInstallerCLICore/Workflows/UpdateFlow.cpp`: `SelectLatestApplicableVersion`, `EnsureUpdateVersionApplicable`; `Workflows/WorkflowBase.cpp`: `GetManifestWithVersionFromPackage`.
- `src/AppInstallerCommonCore/PortableARPEntry.cpp`, `PathVariable.cpp`, `Runtime.cpp` (`GetPathDetailsFor`, `GetPortableInstallRoot`), `Public/AppInstallerSynchronization.h` (`CrossProcessInstallLock`).
- `src/AppInstallerRepositoryCore/Microsoft/PortableIndex.cpp` (`CreateIPortableIndex`), `Microsoft/Schema/Portable_1_0/PortableTable.cpp`, `PackageTrackingCatalog.cpp` (`GetPackageTrackingFilePath`), `Microsoft/PinningIndex.cpp`, `PinningData.cpp` (`EvaluatePinnedStateForVersion`), `Microsoft/Schema/ISQLiteIndex.cpp` (`CreateISQLiteIndex`).
- `src/AppInstallerSharedLib/Public/AppInstallerErrors.h`, `SQLiteMetadataTable.cpp`.
- `winget upgrade`: https://learn.microsoft.com/windows/package-manager/winget/upgrade; `winget show`: https://learn.microsoft.com/windows/package-manager/winget/show; settings: https://learn.microsoft.com/windows/package-manager/winget/settings; pinning: https://learn.microsoft.com/windows/package-manager/winget/pinning.
- winget-pkgs FAQ: https://github.com/microsoft/winget-pkgs/blob/master/doc/FAQ.md.

Homebrew:
- `docs/Cask-Cookbook.md` (`auto_updates`), `docs/FAQ.md`, `docs/Tap-Trust.md`.
- `Library/Homebrew/cask/cask.rb`: `installed_version`, `outdated?`, `outdated_version`, `auto_updates_bundle_outdated?`, `config_path`, `installed_caskfile`, `pin`.
- `Library/Homebrew/cask/config.rb` (`to_json`), `cask/caskroom.rb` (`cask_installed_version`), `cask/upgrade.rb` (`outdated_casks`, `upgrade_cask`), `cask/artifact/moved.rb` (`move`, `move_back`, `delete`), `cask/tab.rb`, `env_config.rb`, `trust.rb` (`raise_untrusted!`), `extend/os/mac/cask/quarantine.rb` (`copy_xattrs`).

Windows: `FSCTL_SET_REPARSE_POINT`, the driver reference: https://learn.microsoft.com/windows-hardware/drivers/ifs/fsctl-set-reparse-point.

Folio (this repository): `docs/plans/design/self-update-2026-09-16.md` revisions (a)–(h); `docs/DESIGN.md` entries of 2026-09-26 to 2026-09-29 on `install_channel` (U-1, U-4), the hooks (U-2), the update job (U-18, U-31) and the harness; `docs/RELEASING.md` "Distribution manifests", "The hooks" and "winget"; `packaging/scoop/folio.json`, `packaging/homebrew/folio.rb`, `packaging/winget/`; `crates/bt-app/src/{install_channel, update_job, update_card, update_txn, update_startup, update_prepare, update_prepare_windows, update_prepare_macos, update_apply_windows}.rs`; the review `U-41-review-codex-2026-09-29.md`.
