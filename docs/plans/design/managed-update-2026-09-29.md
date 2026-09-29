# U-41 — managed copies update through the same transaction (design, 2026-09-29)

Design only: phase 1 of U-41, written on `design/managed-update` from `main` at
`6abd3f5e`. No product code was written, nothing was built, and no package
manager was run. Facts about Folio come from the code, cited by symbol. Facts
about scoop, winget and Homebrew come from their own documentation and source,
cited by file and function (never by line); the URLs are under **Sources** at
the end. Homebrew's source was read from the 7.0.6 tree on the Mac mini, where
only `brew info` and `brew cat` were run. A claim no source settles is written
as an experiment `E-M<n>` (§6), and the phase-2 ticket that depends on it waits
for it.

**The rulings this answers**

| date | owner ruling | where |
|---|---|---|
| 2026-09-27 | A managed copy (scoop, winget, Homebrew, and the zip, "ours") presses one **重启以更新** and the update completes. Today the card shows the manager's command and does nothing. | §1–§4 |
| 2026-09-29 | All four managers ship in 0.4.7: one road with adapters, not four logics ("做得好的话应该不需要分开的四套逻辑"). | §1 |
| 2026-09-28 | Whatever happens, Folio never breaks. Interrupted anywhere, the next start opens. The new version is best; the old one still offers the update. | every row of §2's tables |
| 2026-09-20 | "Managed installs do not self-update." | **superseded** for the three managers by the 2026-09-27 ruling. §4 narrows one sentence of `RULES.md` row 41. |

**What stays exactly as built** (`docs/plans/design/self-update-2026-09-16.md`
revisions (a)–(h)):
- the phases of `update_txn::Phase`, the transition table `update_txn::next`,
  the writer and effect rights, and `update_txn::decide`'s rows for the two
  existing layouts;
- the frozen header v1 and receipt v1 (H.1);
- the quit barrier and `Handoff` (U-21);
- the entrance at logon (U-22, U-26) and the admission locks;
- the exit guard (revision (e)), the job owner's pass at a later launch
  (revision (d)), adoption and deferral (revision (h));
- the card's words (C9) and the rollout contract of (h).

This note adds **no phase, no transition and no writer right**. It adds two
layouts to the journal body. `decide` gains the rows those layouts need, each
answering with a transition already in `next`'s table (§2.3, §2.4).

---

## 1. One road, two adapter steps

### 1.1 The rules

**R1. One road.** The transaction of `update_txn` is the only road a copy of
Folio is updated by, whoever installed it. A package manager appears on it as an
**adapter** at exactly two steps and nowhere else:
- **Prepare** (`Allocated → Prepared`): *where the new set comes from, and where
  it is staged*.
- **Move** (the effects taken in `Moving`, and their inverse in
  `RollbackIntent`): *how the installed set is replaced, how that is undone, and
  how the disk says which set is live*.

**R2. The adapter is chosen once.** It is chosen at the press, from the channel
(`install_channel::channel()`), and recorded in the journal body (§1.3). The
applier, the recovery build and every later lock holder read it from the
journal and never from the channel.

**R3. Folio moves only what the manager lets it move.** Folio performs a Move
only when that Move is **exactly the manager's own move, with the manager's own
bookkeeping written the way the manager writes it**:
- Homebrew moves an `auto_updates` app by the app replacing its own bundle in
  `/Applications`. That is the Cask Cookbook's definition of `auto_updates`
  (HB1).
- scoop moves an app by making a version folder and pointing the `current`
  junction at it. That is what `scoop reset` does (S4).
- winget's bookkeeping is its SQLite portable index and its uninstall record
  (WG2). Folio cannot write either as winget writes it, so winget performs its
  own move.

**R4. The trial comes before the first step Folio cannot undo.** Where Folio
performs the Move, that first step is the retirement after `Committed`, so the
order is today's: Move, Trial, `Committed`. Where the manager performs it
(winget), the manager's step is the first step Folio cannot undo. So the trial
runs on the staged set, and the manager's step happens between the trial's
receipt and `Committed` (§2.4). Both orders use the same phases and
transitions.

### 1.2 The decision table

| | **Prepare: where the set comes from** | **Prepare: where it is staged** | **Move: forward** | **Move: back** | **Move: which set is live** | **after `Committed`** |
|---|---|---|---|---|---|---|
| **ours, Windows** (`Layout::Members`) | Folio downloads the offer's zip and `SHA256SUMS.txt` (U-20, unchanged) | `H\<txn>\set\` | one `MoveFileExW` per file (U-23) | the moves back (U-24) | by digest, file by file | `backup\` deleted |
| **ours, macOS** (`Layout::Bundle`) | Folio downloads the offer's DMG and `SHA256SUMS-macos.txt` (U-27, unchanged) | `H/<txn>/stage/Folio.app` | one `renamex_np(RENAME_SWAP)` (U-28) | the same swap back (U-29) | by bundle identity | the old bundle in `stage/` deleted |
| **Homebrew** (`Layout::Bundle`, adapter `homebrew`) | as ours, macOS | as ours, macOS, **plus the marker attribute carried onto the staged bundle** (§4) | as ours, macOS | as ours, macOS | as ours, macOS | as ours, macOS |
| **scoop** (`Layout::Link`, new) | as ours, Windows: the same zip scoop's manifest names | **a new scoop version folder** `<scoop>\apps\folio\<v>\`, beside the running one: the archive's members, scoop's receipt (`install.json` carried; `manifest.json` rendered by the manifest's own `autoupdate` rules) and the marker carried | the `current` junction pointed at the new folder, as scoop's `link_current` makes it | the junction pointed back at the old folder, which was never written | by the junction's target | none: the old version folder is scoop's, and `scoop cleanup` removes it as it removes every old version (S6) |
| **winget** (`Layout::Manager`, new) | as ours, Windows: for the identity check, the trial and the digest proof. winget downloads again at its step. | `H\<txn>\set\`, where the trial runs | after the trial's receipt, `winget upgrade --id WeiyiShi.Folio --exact --version <v>` run by the lock holder (§2.4) | before winget's step nothing was moved; after it, none (R4) | by the installed set's digests, at winget's recorded location, against the staged set's | none that Folio owns |

### 1.3 What each adapter records

The journal body belongs to the rescue build's version ((b).2: "header v1
frozen; body owned by the rescue build's version"). So a new layout can be read
by exactly the builds that can write it. `update_txn::Layout` gains two arms, and
the bundle arms gain one field. The header is untouched.

| layout | recorded at `Allocated` | added at `Prepared` |
|---|---|---|
| `Members` (ours, Windows) | unchanged | unchanged |
| `Bundle` / `BundleIntent` (ours macOS, Homebrew) | unchanged, plus `marker: Option<Vec<u8>>`: the attribute's bytes read from the running bundle, `None` for ours | unchanged, plus the same field |
| `Link` (scoop) | the app folder (`<scoop>\apps\folio`), the old version folder's name, and its members' digests (the old shipped list, as `Members` records it) | the new version folder's name, its members' digests, and the digests of the three bookkeeping files written (`install.json`, `manifest.json`, `folio-install.json`) |
| `Manager` (winget) | the manager, the package identifier, the record's source identifier, the old version, the record's `InstallLocation` (U-4's reading), and the old set's digests | the staged set's digests, the manager's absolute path (§3.1) and the exact argument vector of its step |

### 1.4 What stays common, and where the adapter is asked

| step | owner today | adapter asked? |
|---|---|---|
| the check, the offer | `update::run`, `update_job::Job` | no |
| eligibility | `update_job::Evidence::eligibility` | the channel's answer widens (§1.5) |
| the card, its words and verbs | `update_card`, C9 | no. A managed copy gets **the same card as ours**, with no manager word on it. |
| the press, the worker, the checksum, abandonment | `update_prepare::{WORKER, fetching, sum_for, abandon, clear, finish}` | **Prepare** |
| the archive reader, the identity checks | `update_archive`, `bt_platform::trust`, `bt_platform::macos_identity` | no |
| the rescue copy | `update_prepare_windows`, `macos_update::rescue_clone` | no; its place follows the home (§1.6) |
| the quit barrier, `Handoff` | `update_handoff` (U-21) | no |
| the entrance at logon | `bt_platform::logon_hook`, the LaunchAgent door | no |
| admission, the process check | `update_startup::pass`, `install_flip::held_open` | the process check reads the final path (§1.6) |
| the effects in `Moving` | `update_apply_windows`, `update_apply_macos` | **Move** |
| the trial, the receipt, `Committed` | `update_trial`, `update_apply::watch_trial` | where the trial's executable lies (winget: the staged set) |
| rollback's decision, `Stuck`, the attempts | `update_txn::decide` | **Move** (the inverse) |
| the exit guard, `opens_now` | `update_apply::ExitGuard` | which program is "installed" (§1.6) |
| adoption, deferral, hand-back | revision (h) | no |
| the start's pass, the job owner's pass | `update_startup::pass`, `update_prepare::settle_at_launch` | the home's locator (§1.6) |

The applier's line stays `--update-apply <home> <txn> <nonce>`: the adapter is in
the journal it opens.

### 1.5 Eligibility

`update_job::Evidence::eligibility` answers `Channel::Managed` with
`NotEligible::Managed { command }` today. After U-41:

| channel | answer |
|---|---|
| `Ours` | `Eligible` (unchanged) |
| `Managed { Homebrew, .. }` | `Eligible`, adapter `homebrew`, wherever the macOS gate is on |
| `Managed { Scoop, .. }` | `Eligible`, adapter `scoop`, when §2.3's precondition holds; otherwise `NotEligible::Managed { command }`, as today |
| `Managed { Winget, .. }` | `Eligible`, adapter `winget`, when §2.4's R-W1 holds; otherwise the offer waits (§5.2) or `NotEligible::Managed { command }` |
| `NotOurs`, `Unknown` | unchanged: the releases page |

`update_prepare_windows::eligible` and `update_prepare_macos`'s road check
refuse everything but `Channel::Ours` today (`Stop::NotOurs`). After U-41 they
accept the channel their adapter names, and nothing else. The row keeps the
manager's command with **Copy** only where the answer is still
`NotEligible::Managed`.

**The rollout.** The adapter's code runs in the **source** build: the applier
and the recovery are copies of the running build (revision (h), "which update
gets which half"). A managed copy of 0.4.6 or earlier has no adapter. It keeps
today's row until the person updates it once through the manager to 0.4.7. So
the first managed update through Folio is 0.4.7 → the next release. The
release note says so.

### 1.6 The home, when the install unit is a link or the manager's

The home `H` holds the lock, the admission file, the journal, the staged set and
the rescue build. Every start finds it from its own executable
(`update_txn::Home::of`). Two adapters change the executable's folder under that
rule, so each needs a locator of its own:

| adapter | the home | why there |
|---|---|---|
| ours, Windows | `<exe folder>\.folio-update\` (unchanged) | the folder is the install unit |
| ours, macOS; Homebrew | `<parent>/.<Bundle>.folio-update/` (F-3, unchanged) | Homebrew never touches a sibling of the bundle (HB5) |
| scoop | `<scoop>\apps\folio\.current.folio-update\`: beside the junction, named by the macOS formula (`.<unit name>.folio-update`) | the executable's folder as launched is the `current` junction, and the Move re-points it. A home inside it would be in the old version folder before the Move and out of reach after it. |
| winget | `<InstallLocation>\.folio-update\`: inside winget's package folder, beside the version folder | winget's upgrade deletes the recorded `folio-<version>\` folder recursively and leaves files it did not record (WG3). The version folder's name changes at every version, so no formula built on it survives the step. |

**L1, the lookup.** A start reads the in-folder home first, as today. Only if
that holds no journal, it looks in two more places:
- (a) beside the executable's folder, when that folder is a directory link;
- (b) in the executable folder's parent.

A home found in (a) or (b) counts only when its journal's frozen header names a
`rescue` inside that same home. That is a consistency check on Folio's own
record, not a guess from a folder's name. An ordinary copy with no update in
flight pays one `symlink_metadata` and one failed `stat` more than today.
Every read goes through `file_reads` on `Lane::Install`; `symlink_metadata` is
metadata, the lane's stated bypass.

**L2, sibling homes exist only during a transaction.** The scoop and winget
homes are outside the unit the manager removes. So the retirement removes them
**whole**: lock, admission file and journal, once no other process holds
admission. A file still held is left for the next start's retirement. Without
L2, `winget uninstall` would leave its package folder behind with "files
remain" (WG3). `scoop cleanup` and `scoop uninstall` delete every child of
`apps\folio` except the current version and `current` (S5); L2 means that
between transactions there is nothing there for them to delete.

**L3, the installed program is named by the body.** `Home::of_rescue` derives
the installed program as `<home's parent>\folio.exe`, which is right only for
`Members`. For the new layouts the body names it:
- `Link`: `<app folder>\current\folio.exe`;
- `Manager`: whichever version folder of `InstallLocation` holds the set by
  digest, read when the program is started, because winget's step renames it.

**L4, processes are told apart by the image's final path.** This covers the
process check before a Move, H.3's candidate witness and the trial's stop.
They read `GetFinalPathNameByHandleW` of the process image, never the launched
path. Through a junction, the old and the new build are both launched as
`…\current\folio.exe`; only the final path (`…\apps\folio\0.4.7\folio.exe`) tells
them apart. For the other adapters the two paths are the same, so this narrows
today's reads and adds no second rule.

---

## 2. Per manager

Each subsection gives, for one manager:
- who downloads and who verifies;
- where the rescue copy comes from;
- the Move and the rollback;
- the power-cut rows, mapped onto (b).2's W and M rows;
- what the manager says afterwards.

### 2.1 zip / ours

Unchanged: W1–W15 and M1–M11 as built (U-23, U-24, U-28, U-29, U-29b, U-34,
U-37). For an ordinary folder, §1.6's L1 and L4 answer exactly as today.

### 2.2 Homebrew

**Facts** (source: Homebrew 7.0.6).
- **HB1.** The Cookbook: `auto_updates true` "Asserts that the cask artifacts
  auto-update."
  - Plain `brew upgrade` upgrades such a cask only when the bundle's
    `Info.plist` version is **older** than the tap's. That is
    `Cask::Cask#outdated_version` → `auto_updates_bundle_outdated?`, which is the
    default now (`HOMEBREW_UPGRADE_AUTO_UPDATES_CASKS`; `HOMEBREW_NO_UPGRADE_AUTO_UPDATES_CASKS`
    turns it off).
  - The FAQ: "Blindly replacing the app based on that record could downgrade
    it."
- **HB2.** Every other comparison is the Caskroom's record against the tap,
  **by equality**:
  - `installed_version` reads `Caskroom/folio/.metadata/<version>/…`;
    `outdated_version` returns early only when `installed_version == version`.
  - This covers a named `brew upgrade --cask folio` (`Cask::Upgrade.outdated_casks`
    asks `outdated?(greedy: true)`), `--greedy`, `--greedy-auto-updates`,
    `HOMEBREW_UPGRADE_GREEDY` and `HOMEBREW_UPGRADE_GREEDY_CASKS`.
  - So a tap one version **behind** the bundle makes these commands **downgrade**
    the bundle.
- **HB3.** `brew uninstall` copies whatever is at `/Applications/Folio.app` back
  into the Caskroom and deletes it (`Cask::Artifact::Moved#move_back`). It
  never compares versions, so a bundle Folio replaced is removed like any other.
- **HB4.** `brew upgrade` moves the new bundle's contents into the existing
  `/Applications/Folio.app` folder (`Moved#move`, `Quarantine.copy_xattrs`), and
  the cask's `postflight_steps` then writes the marker again (U-2).
- **HB5.** `Moved` acts only on `/Applications/Folio.app` and on Caskroom paths.
  A sibling such as `/Applications/.Folio.app.folio-update/` is never touched.
- **HB6.** Casks from third-party taps must be trusted: `brew install --cask
  lulu-loopp/folio/folio` trusts that one item, and `brew trust` trusts more
  (`docs/Tap-Trust.md`, `trust.rb` `raise_untrusted!`). The owner's Mac answers
  `brew info --cask folio` with "Refusing to load cask … from untrusted tap". The
  road never runs `brew`, so this does not reach it. It does reach
  `docs/install.md`'s Homebrew line (§5.1).

**Rules.**
- **R-H1. The cask declares `auto_updates true`.** `packaging/homebrew/folio.rb`
  gains the line, and `update-manifests.ps1` and `cask.sh` keep it byte for byte.
  - Without it, plain `brew upgrade` compares the Caskroom's record by equality
    (HB2). It would downgrade a bundle Folio updated whenever the tap is behind,
    and download again a bundle that is already the tap's version when it is
    not.
  - With it, plain `brew upgrade` compares the bundle's own version (HB1): a
    bundle Folio updated is left alone.
- **Prepare** is ours, macOS (U-27), plus one step after the staged bundle's
  second identity check:
  - the running bundle's marker attribute, read as bytes (the bytes U-1 read in
    `install_channel::read`), is written onto `H/<txn>/stage/Folio.app` under the
    same name and read back equal;
  - its bytes are recorded in the journal (§1.3);
  - the attribute is outside the code signature's seal. U-16 measured this:
    `codesign --verify --strict --deep --all-architectures` stays at exit 0 with
    it on the bundle root (`docs/DESIGN.md` 2026-09-26, "The macOS updater can
    tell whether a copied bundle is the same publisher's Folio …"). So the staged
    bundle is still the release's bytes;
  - **E-M1** repeats that measurement on a notarized, stapled bundle with
    Gatekeeper's assessment. That is E1's remaining half.
- **Move, rollback, trial and commit** are ours, macOS (M1–M11, unchanged). The
  swap exchanges the two bundle directories:
  - the old bundle, with Homebrew's own attribute, goes to `stage/` and is
    deleted after `Committed`;
  - on rollback it comes back, attribute and all.
- **Downloads:** Folio. **Verifies:** Folio, against the checksum document, the
  running bundle's designated requirement, Developer ID, the version and the
  architecture (U-27). The cask's `sha256` is the same bytes' hash
  (`update-manifests.ps1` copies it from `SHA256SUMS-macos.txt`) and is not
  consulted.
- **The rescue copy** is the clone of the running bundle (U-26), unchanged.
- **What Homebrew says afterwards:**
  - `brew list --versions folio` still names the version Homebrew installed;
    the Caskroom's record is the one stale entry, which is what `auto_updates`
    declares Homebrew accepts;
  - plain `brew upgrade` leaves Folio alone while the bundle is at least the
    tap's version (HB1);
  - a named or greedy upgrade reinstalls the tap's version whenever it differs
    from the Caskroom's record (HB2). With the tap rendered before the page is
    published (R-REL1, §5.1), that version is the one Folio installed or a newer
    one: a second download, never a downgrade, except in the minutes R-REL1
    leaves (§5.1).
- **Power cuts:** M1–M11, unchanged. One row changes content. At **M6–M11** the
  live bundle may be the new one, and it carries the marker, which was written
  at Prepare, before the swap. So a start at every M row reads
  `Managed(Homebrew)`, and the channel never reads `Ours` during a transaction.

### 2.3 scoop

**Facts** (source: ScoopInstaller/Scoop, `master`).
- **S1.** An app is installed into `<scoop>\apps\<app>\<version>\`.
  `apps\<app>\current` is a directory junction to it, which `link_current`
  (`lib/install.ps1`) creates with `New-DirectoryJunction` and marks
  `attrib +R /L`; `unlink_current` removes it with `attrib -R /L` and
  `Remove-Item`. With the `NO_JUNCTION` setting there is no junction.
- **S2.** `install_app` writes `manifest.json` into the linked folder
  (`save_installed_manifest`, a copy of the bucket's manifest) and
  `install.json` (`save_install_info`: `architecture`, `bucket`, and `url` for an
  install from a URL; `hold: true` after `scoop hold`).
- **S3.** The installed version is `current\manifest.json`'s `version`
  (`lib/versions.ps1` `Select-CurrentVersion`). `app_status`, `installed`,
  `update`, `uninstall`, `reset` and `cleanup` all use it, and then read
  `versiondir <app> <version>`. **So `manifest.json`'s version must equal its
  folder's name.**
- **S4.** Shims, Start-menu shortcuts and `env_add_path` name the `current`
  path (`create_shims`, `create_startmenu_shortcuts`). `scoop reset
  <app>[@<version>]` (`libexec/scoop-reset.ps1`) is `link_current` on that
  version's folder, followed by shims, shortcuts, env and persist created again.
  Folio's manifest has no `persist` and no `env_add_path`.
- **S5.** The installed versions are the children of `apps\<app>\` that hold an
  `install.json` (`Get-InstalledVersion`). But `scoop cleanup`
  (`libexec/scoop-cleanup.ps1`) and `scoop uninstall` delete **every** child
  except the current version and `current`, `Remove-Item -Recurse -Force`.
- **S6.** `scoop update <app>` (`libexec/scoop-update.ps1` `update`) leaves the
  old version folder in place until `scoop cleanup`:
  - it runs the old manifest's `pre_uninstall` with `$cmd = 'update'`, then
    `unlink_current`, then `install_app` into a new folder;
  - it updates only when `app_status` finds the bucket's version **newer**
    (`Compare-Version`), unless `-f` or the `FORCE_UPDATE` setting is given;
  - it skips, exiting 0, if any process runs from under `apps\<app>\`
    (`test_running_process`);
  - it has no lock file.
- **S7.** A manifest's `autoupdate` block is scoop's documented way to derive
  a new version's manifest from the current one (the scoop wiki, "App Manifest
  Autoupdate"; `lib/autoupdate.ps1`): `$version` substituted into `url` and
  `extract_dir`, and the hash taken from `hash.url` by `hash.regex`.
  `scoop install app@version` goes through the same rendering
  (`generate_user_manifest`). `packaging/scoop/folio.json`'s `autoupdate` names
  `SHA256SUMS.txt` and the zip's line in it.

**Three options, and the one chosen**

**(a) Today's swap inside `current\`.** Refused.
- The folder would be named `0.4.5` and hold 0.4.7, while `manifest.json` still
  says 0.4.5 (S3). `scoop status` would lie.
- `scoop update` would install whatever newer version the bucket names. With the
  bucket at 0.4.6, that is a **downgrade** (S6).
- Rewriting `manifest.json` in place to say 0.4.7 breaks scoop outright: it
  would look for `apps\folio\0.4.7\` (S3).

**(b) Running `scoop update folio` as the Move, in the applier.** Refused.
- It needs the bucket to carry the version first, and the network after the
  quit: scoop downloads again, and pulls its buckets when they are three hours
  old.
- It needs git, PowerShell and scoop in the applier's environment.
- It skips, exiting 0, while any process runs from under `apps\folio` (S6). The
  rescue build is such a process, and "skipped" is indistinguishable from
  "updated" by exit code.
- scoop has no command that installs a version folder without linking it, so
  the trial could not come before the link.

**(c) Folio writes the version folder the way scoop does, and re-points
`current`.** Chosen.
- The Move is scoop's own (S4: `scoop reset`'s).
- Nothing runs after the quit but Folio, and nothing needs the network.
- The old version folder is never written.
- It has the macOS shape: one exchange, whose live side the disk states.

**Rules (option c)**

- **The precondition**, checked at Prepare before `Allocated`: the running
  executable's folder, as launched, is a directory junction named `current`.
  Its target is a sibling folder of the same parent, holding:
  - `install.json` without `hold` (Q2);
  - `manifest.json` whose `version` equals the running build's and whose
    `autoupdate` this renderer reads (below);
  - the scoop marker.

  Anything else keeps today's answer, `NotEligible::Managed` with the Copy
  command. That covers `NO_JUNCTION` and a global install under `%ProgramData%`
  that this account cannot write (`install_channel`'s owner check already reads
  that one as `NotOurs`). This is the Move's own precondition, read from the
  disk the Move will change. The channel itself is still the marker's
  (`RULES.md` row 41).
- **Prepare**
  1. The download, checksum, archive reader and identity checks of U-20, into
     `H\<txn>\` (`H` = `apps\folio\.current.folio-update\`, §1.6), unchanged.
  2. The version folder `apps\folio\<v>\` is made. If the name exists, Prepare
     stops with *Nothing changed*: it is another installer's, or an earlier
     attempt's that the journal does not record. The archive's members are
     copied in durably and checked again where they lie, which is U-20's rule
     for `set\` with this folder as `set\`.
  3. **scoop's receipt**, written as scoop writes it (S2), each file durably:
     - `install.json`: the running version folder's, byte for byte. Its bucket
       and architecture do not change with the version.
     - `manifest.json`: the running version folder's, with the lines that carry
       `version`, the 64-bit `url`, `hash` and `extract_dir` replaced by the
       rendering of its own `autoupdate` block for `<v>`. The hash is taken from
       the `SHA256SUMS.txt` that Prepare already verified (S7).
     - This is exactly the substitution `update-manifests.ps1` performs to
       render the bucket: line by line, not re-serialized, each line present
       exactly once or nothing is written. So when `packaging/scoop/folio.json`
       has not changed between the two versions, the rendered file equals the
       bucket's `folio.json` for `<v>` byte for byte.
     - Phase 2 moves the substitution into one Rust function, with the script's
       own cases as its tests. An `autoupdate` block that uses anything else
       (only `$version`, `$basename` and the hash line are read) is refused at
       the precondition, so the person keeps the Copy command. Nothing is
       guessed.
  4. **The marker is carried**: the running version folder's
     `folio-install.json`, byte for byte (§4).
  5. The rescue copy goes into `H\<txn>\rescue\` (unchanged), and `Prepared` is
     recorded with the `Link` layout (§1.3).

  From the end of step 3, scoop lists `<v>` as installed but not current (S5).
- **Move** (`Moving`, U-23's place in the road). Under exclusive admission,
  after the process check by final path (L4):
  - `current` is removed as `unlink_current` removes it (`attrib -R /L`, then
    the link alone: `RemoveDirectoryW` on the junction, never a recursive
    delete), and its parent is flushed;
  - `current` is created as `link_current` creates it (a junction to
    `apps\folio\<v>`, then `attrib +R /L`), flushed, and read back;
  - the live side is whatever `current` targets: the old folder, the new one,
    or **nothing**, between the two calls. The door is `install_flip::relink`
    (§5.3).
  - If E-M2 finds that the shim names the resolved version folder rather than
    `current`, the Move also rewrites the shim as `create_shims` writes it.
- **Trial:** `…\apps\folio\current\folio.exe --update-trial <txn> <nonce>`, as
  today.
- **Rollback:** `current` is pointed at the old folder by the same two calls.
  Then, **only after `RolledBack` is durable**, the new version folder's files
  are removed: exactly the files the journal recorded writing (the members and
  the three bookkeeping files). The old folder was never written, so nothing
  moves back into it.
- **After `Committed`:** the entrance is removed, `Retired{Committed}`, and the
  home is retired whole (L2). The old version folder is left for `scoop cleanup`,
  which is where scoop's own update leaves it (S6).
- **Downloads:** Folio, from the URL scoop's manifest names. **Verifies:** Folio,
  against `SHA256SUMS.txt` and the running build's signer. scoop's `hash` for
  the version is the same line of the same document (S7).
- **The rescue copy** is the running `folio.exe`, copied into `H\<txn>\rescue\`.
  That is outside both version folders and outside `current`.

**Power cuts: the SW rows.** These are the W rows with the Move replaced. `L` is
the live side, read from the junction.

| # | durable state | on disk | next actor |
|---|---|---|---|
| SW1 | `Allocated` | old live; a partial `H\<txn>`, perhaps a partial `apps\folio\<v>\` | as W1, and the partial version folder is removed from the journal's record. It has no `install.json` until step 3 ends, so scoop never lists it (S5). |
| SW2 | `Prepared` | old live; `apps\folio\<v>\` complete, with its receipt | as W2. If `scoop cleanup` runs while the transaction waits, it deletes `<v>` and the home (S5). The resume's revalidation then finds neither: the transaction is discarded, *Nothing changed*. |
| SW3–SW5 | `Handoff`, `Armed` | as SW2 | as W3–W5 |
| SW6 | `Moving` | `L` = old, nothing, or new | `L` old → `Prepared` (M5's rule: nothing changed). `L` nothing → re-point at the old folder, then `Prepared`. `L` new → `RollbackIntent` (no trial began), then SW9. |
| SW7, SW8 | `Trial` | `L` new | as W7, W8 |
| SW9 | `RollbackIntent` | `L` new, nothing, or old | stop the trial (final path, L4); if `L` is not old, re-point at the old folder; `L` old and the old folder's digests unchanged → `RolledBack`; a failure → `Stuck` |
| SW10–SW13 | `Stuck`, `RolledBack`, `Committed` with debt, `Abandoned` | as W10–W13 | as W10–W13. After `RolledBack`, the recorded files of `apps\folio\<v>\` are removed. |

While `L` is nothing, no start can run: the shim's target does not exist. The
entrance at logon names `H\<txn>\rescue\folio.exe`, which is outside `current`,
and that finishes the Move. This is what W4–W6 already require of a Windows
copy whose executable is mid-move.

**What scoop says afterwards**
- `scoop list` and `scoop status` name `<v>` (S3).
- While the bucket still names an older version, `scoop update folio` reports
  it up to date and downgrades nothing (S6's `Compare-Version`). The exceptions
  are the person's own `-f` and the `FORCE_UPDATE` setting.
- `scoop cleanup folio` removes the old folder.
- `scoop reset folio@<old>` goes back by scoop's own hand.
- `scoop uninstall folio` runs `pre_uninstall` from `current\manifest.json`,
  which is the rendered copy of the hook the old version had (§4).

### 2.4 winget

**Facts** (source: microsoft/winget-cli, `master`; winget-pkgs docs).
- **WG1. winget is not live for Folio.** microsoft/winget-pkgs#431006 is open:
  its pipeline passed and it waits for a moderator. A winget copy today exists
  only where someone installed Folio from a local manifest, as the E2 run did.
  Its record names a local source (`WeiyiShi.Folio__DefaultSource`, U-4).
- **WG2. The install.** A zip holding a portable is extracted into
  `%LOCALAPPDATA%\Microsoft\WinGet\Packages\<PackageId>_<SourceId>\`
  (`GetPortableProductCode`, `PortablePackageUserRoot`). What winget records:
  - the uninstall key that U-4 reads (`PortableARPEntry`, `RegisterARPEntry`),
    whose `DisplayVersion` is the installed version (winget-pkgs FAQ);
  - a SQLite portable index, `<ProductCode>.db`, in the package folder,
    listing the archive's top-level items;
  - with `ArchiveBinariesDependOnPath`, the folder that holds the executable,
    put on `PATH`.
- **WG3. An upgrade.** `winget upgrade` of a portable removes every indexed item
  and then moves the new ones in; the recorded folder `folio-<old>\` is removed
  **recursively** (`PortableInstaller::ApplyDesiredState`). Items it never
  recorded are left alone. The `PATH` entry moves to `folio-<new>\`.
  - **There is no rollback on an upgrade:** `PortableInstallImpl` cleans up
    only when it is not an update. A file in use leaves the folder half-emptied
    while the record still names the old version.
  - Uninstall removes the package folder only when it is empty, and otherwise
    says "files remain" (`RemoveInstallDirectory`), unless `--purge` is given.
- **WG4. Before any upgrade or uninstall, winget checks the recorded items**
  (`PortableInstaller::VerifyExpectedState`). A mismatch stops the upgrade or
  uninstall with 0x8A150057 unless `--force` is given, which overrides this
  check only.
- **WG5. The command.** `winget upgrade --id <id> --exact --version <v>
  --source winget --silent --accept-source-agreements
  --accept-package-agreements --disable-interactivity` targets exactly one
  version and never prompts (the `winget upgrade` reference).
  - A version that is not newer than the installed one gives 0x8A15002B
    (`EnsureUpdateVersionApplicable`).
  - A portable installed from another source gives 0x8A150054.
  - A pinned package gives 0x8A150068.
  - Installs take `CrossProcessInstallLock`, so a second winget **waits** rather
    than failing.
- **WG6.** winget checks the installer against the manifest's `InstallerSha256`.
  `docs/RELEASING.md` "What to change for a release" copies that value from
  `SHA256SUMS.txt`, so winget and Folio verify the same bytes.
- **WG7.** A merged manifest is published "typically within one hour"
  (winget-pkgs FAQ). A client refreshes its source cache when it uses the source
  and the cache is more than `autoUpdateIntervalInMinutes` old (15 by default).

**The decision: winget's step is the commit, and the trial runs before it
(R4).**

A rollback after winget's step would be a second winget operation
(`winget install --version <old> --force`). That needs the network, the old
version still in the source, and winget's own success, and Folio cannot promise
any of the three at a logon after a power cut. Nor can Folio restore the old set
itself: the files would come back, but winget's index and record would describe
the new version, so `winget uninstall` and the channel would both break. So the
only point where a winget update can still be refused with *Nothing changed* is
before winget acts.

The trial therefore runs on the staged set. That set is byte for byte the set
winget will install (WG6). The receipt binds to a process of that image (H.1),
and the image is the same bytes wherever it lies.

**It fits the existing table.** Here is the winget sequence, as `next` already
allows it:
1. `Armed →(Admitted) Moving`. Nothing is moved in `Moving`: the staged set is
   where the trial runs.
2. `Moving →(TrialBegan) Trial`. The trial is `H\<txn>\set\folio.exe`, with its
   sidecars beside it.
3. The receipt arrives. The lock holder's *commit* action for the `Manager`
   layout has one effect before the write of `Committed`: **winget's step and
   the digest proof**.
4. `Trial →(ReceiptAccepted) Committed`.

When the step fails:
- **the old set is still whole** (winget refused or never began):
  `Trial →(RollbackDeclared) RollbackIntent → RolledBack`, with nothing to move
  back. The words *Previous version restored.* are true: the running version is
  the old one;
- **neither set is whole** (winget stopped mid-way, WG3):
  `RollbackIntent →(RollbackFailed) Stuck`. `Stuck`'s retry for this layout runs
  winget's step again, with `--force` (WG4: the half-emptied folder is winget's
  own work). On the digest proof, `Stuck →(ReceiptAccepted) Committed`, which is
  the commit-forward retrial that `next` already has.

**The trial stays the person's window.** The trial is already on screen when
winget's step runs. It runs from `H`, not from winget's folder, so winget never
meets it (WG3 leaves unrecorded items alone), and it goes on as the person's
Folio after `Committed`. `H\<txn>` is retired when it exits: the next start
deletes it, as W12 already does for a rescue folder still in use. The person's
next start runs the build winget installed, which is the same bytes.

**Rules**
- **R-W1. The catalogue first.** A winget copy is offered a version only when
  winget's community source has it and the copy's record names that source:
  - a record whose `WinGetSourceIdentifier` is not the community source keeps
    the Copy command (WG5's 0x8A150054). That includes every copy today (WG1);
  - otherwise, before the card is raised, the job asks
    `winget show --id WeiyiShi.Folio --exact --version <v> --source winget
    --accept-source-agreements --disable-interactivity`. It runs on the job's
    worker, at most once per launch, and only when the offer is otherwise
    eligible;
  - exit 0 → eligible;
  - the code for "no package found" (E-M4) → the offer waits, and the row's foot
    names the version with no verb (§5.2);
  - any other answer → `NotEligible::Managed` with the Copy command.
- **R-W2. Prepare** is ours, Windows (U-20), into `H\<txn>\set\`. It also reads
  the record's location, source and version (U-4) and the manager's absolute
  path (§3.1).
- **R-W3. winget's step** is run by the lock holder through `quiet_command_named`,
  by absolute path, with WG5's argument vector as recorded at `Prepared`:
  - it is bounded at 10 minutes (it downloads the zip again), and past that the
    child is ended by the pid it was started with;
  - after exit 0, the set at the record's `InstallLocation` is read, by digest;
    it must equal the staged set;
  - equal → `Committed`. Any other result → the rows below.
- **R-W4. The home** is `<InstallLocation>\.folio-update\` (§1.6). The applier,
  the rescue copy and the trial all run from it, and winget never removes it.
- **Downloads:** Folio, to verify the identity and to run the trial on the same
  bytes; then winget, at its step (its cache is its own). **Verifies:** Folio
  before (checksum, signer, archive, release manifest, trial); winget during
  (its hash, WG6); Folio's digest proof after.
- **The rescue copy** is the running `folio.exe`, in `H\<txn>\rescue\`.

**Power cuts: the G rows**

| # | durable state | on disk | next actor |
|---|---|---|---|
| G1–G5 | `Allocated` … `Armed` | the old set installed, untouched | as W1–W5 |
| G6 | `Moving` (the trial not yet begun) | as G1 | nothing was moved, so M5's rule: the entrance removed, `Moving →(Reverted) Prepared`, and the old build starts plainly; the job owner resumes or discards (W2) |
| G7 | `Trial`, no receipt | as G1 | as W7. Its rollback has nothing to move: `RolledBack`, and the old build starts with `--update-failed`. |
| G8 | `Trial` with a receipt; winget ran or was running | the old set whole, the new set installed, or neither | the lock holder reads the location. New set by digest → `Committed`. Old set whole → winget's step again, once; if that step fails with the old set still whole → `RollbackIntent` → `RolledBack` (F2). Neither → `RollbackIntent` → `Stuck`. |
| G9 | `Stuck` (the `Manager` layout) | neither whole, or the old whole | winget's step with `--force` at every logon and start, three attempts at most (`STUCK_ATTEMPT_LIMIT`). On proof → `Committed`. After the third: *Update incomplete.*, the journal's folder and the manager's command. The exit guard opens the rescue copy with `--update-failed`, the one build that surely exists (U-29b's fallback). |
| G10 | `Committed`, cleanup partly done | the new set installed | as W12 |

**A start during winget's step** runs whatever `PATH` or the person names:
- the old executable, if winget has not removed it yet. L1(b) finds
  `<InstallLocation>\.folio-update` and the start hands itself to the rescue
  build;
- the new one: the same parent, so the same home;
- nothing, if winget removed the old file and has not written the new one. The
  entrance at logon finishes it.

The trial already passed on the same bytes, so a new build started before
`Committed` puts no data at risk. H.3's candidate witness defers the recovery
beside it, as it does today.

**What winget says afterwards.** `winget list` names the new version; the record
is winget's own. `winget upgrade` says it is up to date. `winget uninstall`
works as for any winget install, because winget did the install.

### 2.5 The four side by side

| | downloads | verifies | rescue copy from | trial | rollback | the manager's bookkeeping after |
|---|---|---|---|---|---|---|
| ours | Folio | Folio | the running build | after the Move | Folio's moves back | — |
| Homebrew | Folio | Folio | a clone of the running bundle | after the Move | the swap back | the Caskroom's record is stale, as `auto_updates` declares |
| scoop | Folio | Folio (scoop's hash is the same line) | the running build | after the Move | the junction back | exact: version folder, `manifest.json`, `install.json`, marker |
| winget | Folio, then winget | Folio before, winget during, Folio's digest proof after | the running build | **before** winget's step | none after winget's step; before it, nothing moved | exact: winget wrote it |

---

## 3. What a manager adds that can fail

### 3.1 Finding the manager

| manager | run by the road? | how it is found |
|---|---|---|
| Homebrew | never | — |
| scoop | never | — |
| winget | at R-W1 (`winget show`) and at the step (`winget upgrade`) | at Prepare, as the App Execution Alias `%LOCALAPPDATA%\Microsoft\WindowsApps\winget.exe`. That is the one place winget documents for every account, and it exists once the account has logged on. The path is resolved once and recorded in the journal (§1.3), and the step runs that recorded path. |

The step never uses `PATH`. The applier is a detached child of a quitting
process, and a person's `PATH` edits in a shell never reach it. Of the three
manager programs, only `winget.exe` is ever run, and only on a winget copy.

### 3.2 The failure table

Each row gives the cause, the card the person sees (C9's words) and the journal
state it ends in. "Prepare" rows end before anything is armed; the others belong
to the lock holder.

| # | cause | adapter | when | card | journal |
|---|---|---|---|---|---|
| F1 | winget's program is missing | winget | R-W1 or Prepare | none (R-W1: the Copy command), or `Failed` with the reason and *Nothing changed.* | none, or `Abandoned` |
| F2 | the program disappeared between Prepare and the step | winget | the step | the trial is stopped as W9 stops one, and the old build opens with `--update-failed`: *Previous version restored.* | G8's "old whole" → `RollbackIntent` → `RolledBack` |
| F3 | winget would prompt (a source agreement, a package agreement, UAC) | winget | the step | `--disable-interactivity` and the two `--accept-*` flags make winget answer rather than ask; a portable needs no elevation. A non-zero exit is F2's row. | as F2 |
| F4 | winget installed a set that is not the staged set (another version, a changed manifest) | winget | the digest proof | *Update incomplete.*, the folder and the manager's command | `Stuck` (G9); never `Committed` |
| F5 | another winget install is running | winget | the step | none: winget waits (WG5) within the step's bound, then F2 or G9 | as the step ends |
| F6 | the network is gone mid-step | winget | the step | G8 and G9 | `Committed` on a retry, or `Stuck` |
| F7 | the package is pinned in winget (0x8A150068) | winget | the step | as F2 | `RolledBack` (see Q2) |
| F8 | the person runs `scoop update folio` beside Folio's update | scoop | scoop skips while a Folio runs from `apps\folio` (S6), so this can happen only while no Folio runs. At Prepare, the name `<v>` is then taken → *Nothing changed*. At the Move, `current` does not target the recorded old folder → the process check refuses → `Reverted`. | `Failed`, *Nothing changed.* | `Abandoned` or `Prepared` |
| F9 | `scoop cleanup` during a waiting transaction | scoop | SW2 | none at once; the next launch's revalidation discards | discarded |
| F10 | `scoop cleanup` during the apply's minutes | scoop | SW3–SW9 | the running rescue build cannot be deleted, so the home survives in part. If the journal went, the entrance's recovery finds nothing to finish and `L` stands where the Move left it; the old folder is still whole, so `scoop reset folio@<old>` is always available. | whatever remains; an accepted, stated limit (§8) |
| F11 | `brew upgrade --greedy` beside Folio's update | Homebrew | the swap's identity checks before and after the exchange (M4, M6) refuse a bundle that is neither recorded identity | as the macOS rows | `Prepared` or `Stuck` |
| F12 | the marker cannot be carried (the attribute write, or the file write, refused) | Homebrew, scoop | Prepare | *Nothing changed.* | `Abandoned` |
| F13 | the precondition fails: `NO_JUNCTION`, a held app, an `autoupdate` this renderer does not read | scoop | eligibility | no card; the Copy command, as today | none |
| F14 | winget's catalogue does not have the version yet | winget | R-W1 | no card; the row names the version (§5.2) | none |
| F15 | Xcode Command Line Tools are requested | Homebrew | never: Homebrew is not run | — | — |

No row adds a refusal, a prompt or a confirmation for the person. F13 and F14
keep today's row where the road cannot complete, which is the ruling's own
boundary.

---

## 4. The marker and the channel after an update

**Rule M1. The marker is carried, never composed.** Where Folio performs the
Move on a managed copy, the new set carries the marker that the manager wrote on
the old one, **byte for byte**:
- Folio reads the bytes (U-1 already reads them);
- it writes them on the staged set at Prepare, before the Move;
- it reads them back equal and records them in the journal.

Folio never writes a marker it did not read. That keeps the point of
`RULES.md` row 41, that how a copy was installed is the manager's statement and
never Folio's inference. It narrows the row's sentence "The marker is written by
the package manager, never by Folio" to "**composed** by the package manager, and
carried by Folio across an update Folio performs".

| | where it lives | re-written after Folio's update by | re-written after the manager's own next update by |
|---|---|---|---|
| Homebrew marker | the attribute on `/Applications/Folio.app` | Folio's carry onto `stage/Folio.app`, before the swap (§2.2) | the cask's `postflight_steps` (U-2, HB4) |
| scoop marker | `folio-install.json` in the version folder | Folio's carry into `apps\folio\<v>\` at Prepare (§2.3 step 4) | scoop's `post_install` |
| scoop's receipt | `install.json`, `manifest.json` | Folio's copy and rendering (§2.3 step 3) | scoop |
| winget record | winget's uninstall key; winget has no marker | winget, which performs the step | winget |

**Why the channel stays managed throughout.** The new set carries the marker
before the Move. So every start at every row reads the same channel, and the next
update takes the same adapter. The uninstall hook the marker names is still the
hook the manager's uninstall runs: scoop reads `pre_uninstall` from
`current\manifest.json`, which is the rendered copy of the running version's
manifest.

**A hook changed in a new release** reaches a copy that Folio updated only at
the manager's own next update, because scoop's `manifest.json` is rendered from
the running version's and not fetched. The hooks have kept one form since U-2.
A release that changes one says so in its release note and in R-REL1's
checklist. Either order is safe, because the door's grammar is frozen.

---

## 5. What else changes

### 5.1 The release process

Today, `docs/RELEASING.md` "Distribution manifests" updates both manifests
**after** the release page exists. The feed (the releases list,
`update::GitHubReleases`) offers a version from the moment the page is
published.

- **R-REL1. The tap and the bucket are rendered before the page is published,
  and applied right after.**
  - The page is created as a draft. The anonymous releases list does not show
    drafts, so `update::run` never offers one.
  - `update-manifests.ps1 -FromRelease` reads the two checksum files from the
    draft; it already uses `gh release download`, which reads a draft for an
    account with push rights. The printed difference is reviewed.
  - The page is published, and `-Apply` follows in the same sitting.
  - The window in which the feed names a version that the tap does not is the
    minutes between publish and apply. In that window, only a named or greedy
    `brew upgrade` could downgrade a bundle that Folio had already updated
    (HB2), and Folio cannot update a bundle before the page is published. So
    the risk needs a person who updates in Folio and then runs
    `brew upgrade --greedy` within those minutes. Stated, and accepted.
  - The Homebrew and scoop adapters never read the tap or the bucket (§2.2,
    §2.3).
- **R-REL2. The winget manifest is submitted when the page is published, and a
  winget copy is offered the version only once winget's source has it** (R-W1).
  The feed is not held back for winget. Moderation takes days (WG1), and holding
  every copy for it would make ours, Homebrew and scoop copies wait on a
  stranger's review. R-W1 puts the wait on winget copies alone.
- **R-REL3.** `packaging/homebrew/folio.rb` gains `auto_updates true` (R-H1), and
  `scripts/ci/check-manager-hooks.ps1` asserts it.
- **R-REL4.** `docs/install.md` says that installing from the tap trusts the
  cask (HB6), and names `brew trust lulu-loopp/folio` for a tap added
  separately.
- `update-manifests.ps1` is otherwise unchanged. Its rendering becomes the
  reference that the scoop adapter's renderer is tested against (§6).

### 5.2 The card and the row

Every eligible copy gets C9's card, with no manager word on it: the press
completes the update wherever the copy came from, which is the ruling. The row
keeps today's Copy command where the answer is still `NotEligible::Managed`
(§1.5, F13, and winget copies until R-W1 holds). There is one new foot: a winget
copy whose offer waits on the catalogue shows `Folio <v>` with no verb
(`RowFoot::Waiting`). It is a new `Text` variant, listed in
`Text::CHINESE_PENDING`.

### 5.3 `ARCHITECTURE.md` rows

- **§2.2, child processes.** One new kind: `winget.exe`, by the absolute path
  of its App Execution Alias. It is run on the job's worker for R-W1's
  `winget show` (bounded at 60 s) and by the lock holder for the step (bounded
  at 10 min). Both go through `quiet_command_named`, and each is ended by the
  pid it was started with if it overruns. The census row's caller count grows
  by two.
- **§6, doors.**
  - The junction re-point is a new effect of `bt_platform::install_flip`
    (`relink`, with `link_target`). It is the only writer of a scoop `current`.
  - The attribute write of M1 is a new effect of `bt_platform::macos_update`
    (`carry_marker`).
  - The image's final path is a new read of `bt_platform::install_flip`
    (`image_final_path`).
- **§4.4, facts born with one owner.** *How this copy was installed* keeps its
  owner, `install_channel`, and gains a carrier (§4). Each ticket's
  architecture section lists it under (c′), with its readers (`first_run`'s
  Explorer row, the update job).

### 5.4 `PRIVACY.md`

- **"Updating".** The sentences "A copy that scoop or winget installed gets no
  card" and the Homebrew one are replaced by one sentence per manager:
  - scoop: *the new version is placed beside the old one in scoop's folder, the
    way scoop places it, and the old one stays until `scoop cleanup`*;
  - winget: *Folio asks winget whether it has the version (`winget show`), and
    once the new version has started, winget installs it (`winget upgrade`)*;
  - Homebrew: *replaced in place, as for a copy installed by hand; Homebrew's
    own record keeps the version it installed*.
- **"What is written outside Folio's folder".** During an update only: scoop's
  home, `apps\folio\.current.folio-update\`, and winget's,
  `<package folder>\.folio-update\`. Each is removed whole when the update ends
  (L2) and by `--uninstall-cleanup`'s home row.

---

## 6. Tests owed (phase 2)

**The fake manager.** A stand-in `winget.exe`, built the way
`update_apply_windows_tests` builds its stand-ins. It records its argv to a file
and behaves according to a scripted outcome in a file beside it:
- an exit code and a delay;
- a set of files to write at a given location, standing in for winget's
  install, and whether to stop half-way;
- whether to wait on stdin, which proves that the road never waits on a prompt.

Every adapter test runs the real road: the real Prepare into temporary folders,
the real journal and the real applier entry. Only the manager program and the
download door are replaced. The scoop and Homebrew tests need no fake: no
manager runs.

| test | adapter | red on BASE because |
|---|---|---|
| `a_scoop_copy_is_offered_the_card_and_not_the_command` | scoop | eligibility answers `Managed` |
| `a_scoop_copy_without_the_junction_layout_keeps_the_command` | scoop | new (F13) |
| `a_scoop_prepare_writes_a_version_folder_scoop_reads_as_that_version` | scoop | new. The written `manifest.json` equals `update-manifests.ps1`'s rendering of the same inputs, byte for byte. |
| `the_scoop_move_repoints_current_and_writes_nothing_in_the_old_folder` | scoop | new |
| `a_scoop_move_cut_between_unlink_and_link_is_finished_to_the_old_side` | scoop | SW6, `L` nothing |
| `a_scoop_rollback_removes_only_the_files_it_recorded` | scoop | SW11 |
| `the_home_of_a_copy_launched_through_a_junction_is_beside_the_junction` | scoop | `Home::of`, L1 |
| `a_home_beside_the_folder_counts_only_when_its_header_names_a_rescue_inside_it` | scoop, winget | L1 |
| `an_old_and_a_new_build_behind_one_junction_are_told_apart_by_final_path` | scoop | L4 |
| `a_sibling_home_is_removed_whole_when_its_transaction_retires` | scoop, winget | L2 |
| `a_homebrew_copy_is_offered_the_card` | Homebrew | eligibility |
| `the_marker_is_carried_onto_the_staged_bundle_byte_for_byte` | Homebrew | new |
| `a_bundle_whose_marker_cannot_be_carried_is_abandoned_with_nothing_changed` | Homebrew | F12 |
| `the_cask_declares_auto_updates` | Homebrew | `check-manager-hooks.ps1` |
| `a_winget_copy_from_another_source_keeps_the_command` | winget | R-W1 |
| `a_winget_copy_waits_until_winget_has_the_version` | winget | R-W1, over the fake |
| `the_winget_trial_runs_on_the_staged_set_before_winget_is_asked` | winget | the fake's argv is recorded only after the receipt |
| `winget_is_run_by_its_recorded_path_with_every_no_prompt_flag` | winget | argv |
| `a_set_winget_installed_that_is_not_the_staged_set_is_never_committed` | winget | F4 |
| `a_winget_step_cut_half_way_is_run_again_with_force_by_the_next_holder` | winget | G8, G9 |
| `a_failed_staged_trial_rolls_back_without_moving_anything` | winget | G7 |
| `every_start_at_every_row_reads_the_same_managed_channel` | all | the carried marker |

**The W and M rows that change.**
- Homebrew: M6–M11, with the marker on the live bundle.
- scoop: new rows SW1–SW13. They go into `scripts/release/cleanvm/updater/rows.ps1`
  and run on the Windows VM with scoop installed; the install step is the
  install half of `check-scoop-hooks-in-vm.ps1`.
- winget: new rows G1–G10, on the VM with the manifest installed locally (E2's
  way). The step runs against a local manifest and R-W1 is answered by the fake
  until winget is live, because a local-source record is otherwise not eligible
  (R-W1).
- Homebrew's M rows also run in `mac/row.sh` on the owner's Mac, under a scratch
  `HOME` and `--appdir`, as `check-cask-hooks.sh` does.

**`clean-vm.md` §4.4 additions.** Two subsections, 「scoop 副本」 and
「winget 副本」. Each has its install step, the row table above, and what
`scoop status` or `winget list` must print after `Committed` and after
`RolledBack`.

**Experiments**

| id | question | before |
|---|---|---|
| E-M1 | the carried attribute on a notarized, stapled bundle: `codesign --verify --strict --deep`, `spctl --assess`, a first launch offline | U-41b |
| E-M2 | scoop's `folio.shim` names `apps\folio\current\folio.exe` and not the resolved version folder (`create_shims`; the research could not confirm whether `Convert-Path` resolves the junction) | U-41c |
| E-M3 | winget, on the VM: an upgrade leaves `<InstallLocation>\.folio-update\` and a process running from it alone (WG3 says so from the source), and uninstall says "files remain" while it exists | U-41d |
| E-M4 | `winget show --id … --exact --version <v>`'s exit code for a version the source lacks, and its time with a cold source cache | U-41d |

---

## 7. The ticket cut for phase 2

One ticket opens the road to adapters; then one ticket per adapter, on the
common road. Each ticket stays inert until its own constant is turned on, and
each brief carries `_standing-rules.md` and an architecture-impact section.

| # | title | size | inputs | what it builds |
|---|---|---|---|---|
| U-41a | the road takes an adapter | M | — | `Layout::{Link, Manager}` and the `marker` field; eligibility by adapter, each adapter refused behind its own constant; `Home::of`'s L1; L2; L3; L4's final-path reads; the tests of §6 that need no adapter |
| U-41b | Homebrew | S | U-41a, E-M1 | the carry (M1), `auto_updates true`, the Homebrew constant on, the M-row checks |
| U-41c | scoop | M | U-41a, E-M2 | the renderer, the version-folder Prepare, `install_flip::relink`, the SW rows, the VM checklist |
| U-41d | winget | M | U-41a, E-M3, E-M4 | R-W1's probe, the staged trial, the step and its proof, the G rows, the fake manager |
| U-41e | the release process and the docs | S | U-41b | `RELEASING.md` (R-REL1–R-REL4), `install.md`, `PRIVACY.md`, `RULES.md` row 41's narrowing, the `ARCHITECTURE.md` rows |

That is 2 S and 3 M; the code is L in total, as the brief estimated. U-41b, U-41c
and U-41d are independent once U-41a has landed.

---

## 8. Questions for the owner

Only what this note cannot decide:

1. **winget before it is live.** Under R-W1, no public winget copy sees the card
   until a moderator merges #431006, and today's only winget copies (local
   manifests) keep the Copy command. U-41d can be rehearsed only on a local
   manifest with the catalogue probe faked. Build it in 0.4.7 regardless (my
   recommendation: yes; the adapter is ready the day winget goes live), or hold
   U-41d until winget is live?
2. **A copy the person told its manager to keep** (`scoop hold folio`; a winget
   pin). The standing rules forbid adding a refusal the brief does not ask for,
   so this note does not decide. Two ways:
   - (a) the hold or pin means *no card*, and the row names the manager's
     unhold command; the precondition and R-W1 read it (my recommendation:
     it is the person's own statement to their manager);
   - (b) the press updates anyway. A scoop copy then carries `hold: true`
     forward, and a pinned winget copy ends at F7 (winget itself refuses:
     *Previous version restored.*).
3. **Carrying the marker** narrows one sentence of `RULES.md` row 41, from
   "never by Folio" to "never composed by Folio" (§4). Accept?
4. **scoop's old version folder** stays until `scoop cleanup`, as it does after
   scoop's own update (about 40 MB a version). Accept, or should Folio's
   retirement remove it, which scoop's own update never does?
5. **F10** (`scoop cleanup` run during the minutes of an apply) is accepted as a
   stated limit, not guarded. The old folder survives it, and `scoop reset`
   recovers. Accept?

---

## Sources

scoop (`https://github.com/ScoopInstaller/Scoop/blob/master/…`):
- `lib/install.ps1`: `install_app`, `link_current`, `unlink_current`,
  `save_installed_manifest`, `save_install_info`, `create_shims`,
  `create_startmenu_shortcuts`, `Invoke-HookScript`.
- `lib/versions.ps1`: `Select-CurrentVersion`, `Get-InstalledVersion`.
- `lib/core.ps1`: `app_status`, `test_running_process`, `is_scoop_outdated`, and
  the `$scoopdir` / `$globaldir` resolution at the file's end.
- `lib/commands.ps1`: `exec` (the `$cmd` hooks see).
- `lib/autoupdate.ps1`: `Invoke-AutoUpdate`.
- `libexec/scoop-update.ps1`: `update`.
- `libexec/scoop-reset.ps1`, `libexec/scoop-cleanup.ps1`, `libexec/scoop-uninstall.ps1`,
  `libexec/scoop-hold.ps1`.
- The wiki, "App Manifest Autoupdate":
  https://github.com/ScoopInstaller/Scoop/wiki/App-Manifest-Autoupdate.
- This repository's record of the same sources: `reports/U-2.md` in the 0.4.6
  ticket set (hooks, `$cmd`, `post_install` after `link_current`).

winget (`https://github.com/microsoft/winget-cli/blob/master/…`):
- `src/AppInstallerCLICore/Workflows/PortableFlow.cpp`: `GetPortableProductCode`,
  `GetDesiredStateForPortableInstall`, `PortableInstallImpl`,
  `PortableUninstallImpl`.
- `src/AppInstallerCLICore/PortableInstaller.cpp`: `Install`,
  `ApplyDesiredState`, `VerifyExpectedState`, `RegisterARPEntry`, `InstallFile`,
  `RemoveFile`, `RemoveInstallDirectory`.
- `src/AppInstallerCommonCore/PortableARPEntry.cpp`.
- `src/AppInstallerCommonCore/Runtime.cpp`: `PathName::PortablePackageUserRoot`.
- `src/AppInstallerCLICore/Workflows/UpdateFlow.cpp`:
  `SelectSinglePackageVersionForInstallOrUpgrade`, `EnsureUpdateVersionApplicable`.
- `src/AppInstallerCLICore/Workflows/InstallFlow.cpp`: `ExecuteInstallerForType`
  (`CrossProcessInstallLock`).
- `doc/windows/package-manager/winget/returnCodes.md`, `AppInstallerErrors.h`,
  `doc/Settings.md` (`autoUpdateIntervalInMinutes`), `doc/troubleshooting/README.md`
  (the App Execution Alias).
- `winget upgrade`: https://learn.microsoft.com/windows/package-manager/winget/upgrade.
- winget-pkgs FAQ, on the ARP version and on publication after a merge:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/FAQ.md.

Homebrew (`https://github.com/Homebrew/brew/blob/main/…`, read at 7.0.6):
- `docs/Cask-Cookbook.md` (`auto_updates`), `docs/FAQ.md` ("How does `brew
  upgrade` handle apps that update themselves?"), `docs/Tap-Trust.md`.
- `Library/Homebrew/cask/cask.rb`: `installed_version`, `outdated?`,
  `outdated_version`, `auto_updates_bundle_outdated?`.
- `Library/Homebrew/cask/caskroom.rb`: `cask_installed_version`.
- `Library/Homebrew/cask/upgrade.rb`: `outdated_casks`, `upgrade_cask`.
- `Library/Homebrew/cask/artifact/moved.rb`: `move`, `move_back`, `delete`.
- `Library/Homebrew/env_config.rb`: `HOMEBREW_UPGRADE_AUTO_UPDATES_CASKS`,
  `HOMEBREW_UPGRADE_GREEDY`.
- `Library/Homebrew/trust.rb`: `raise_untrusted!`.
- `Library/Homebrew/extend/os/mac/cask/quarantine.rb`: `copy_xattrs`.

Folio (this repository): `docs/plans/design/self-update-2026-09-16.md`
revisions (a)–(h); `docs/DESIGN.md` entries of 2026-09-26 to 2026-09-29 on
`install_channel` (U-1, U-4), the hooks (U-2), the update job (U-18, U-31) and
the harness; `docs/RELEASING.md` "Distribution manifests", "The hooks" and
"winget"; `packaging/scoop/folio.json`; `packaging/homebrew/folio.rb`;
`packaging/winget/` (the 0.4.0 manifests);
`crates/bt-app/src/{install_channel, update_job, update_card, update_txn,
update_startup, update_prepare, update_prepare_windows, update_prepare_macos,
update_apply_windows}.rs`.
