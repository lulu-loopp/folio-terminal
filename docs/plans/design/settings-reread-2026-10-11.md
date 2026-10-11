# Hand-edited settings, keybindings, profiles and pins, re-read as a storage transaction: T-SETTINGS-REREAD (0.4.8 B6), design note, 2026-10-11

Status: **revision (a), the draft for the Codex review.** No ticket below has been
dispatched. Plan row B6 ("hand-edited settings/keybindings/profiles/pins re-read — design
first (storage transaction: revision preconditions, field-level merge, explicit conflict,
durable owed edits), Codex review, then build"). Read at `main` = `77399d57` on
`design/settings-reread`. No product code, no cargo run, no window, and no read of any
real data directory: facts about the files on disk come from the schema document and the
code. Facts about Folio are cited `file:line` at `77399d57` with the symbol beside the
line, so a later reader can find it again. A claim the code cannot settle is an experiment
`E-<n>` (§7), and the ticket step that rests on it waits for it. Everything this note says
about today's behaviour that is a **defect** is a reading of the code, marked as one; none
of it was reproduced, because reproducing it needs a window.

**Section signs.** A bare `§n` is a section of this note. `A§n` is a section of
`docs/ARCHITECTURE.md`, `S§n` a section of `docs/M2-persistence-schema-v1.md`, `R<n>` a row
of `docs/RULES.md`, and `OS§n` a section of `docs/plans/design/os-signals-2026-10-11.md`
(B5, whose ears this design rides).

**The rulings this answers**

| date | ruling | where |
|---|---|---|
| 2026-08-03 (owner), S§5.1 | `settings.json` is written at once on a change in the dialog, never debounced | §3.2 |
| 2026-08-03 (owner), S§1.3 rule 4 | A key this build has no name for is dropped on the next write; hand-extending the schema is not supported | §1.6 |
| 2026-08-17 (owner), DESIGN §7.1.6c-2 | `keybindings.json` holds only departures, `null` unbinds, a damaged file is reported and never repaired; no dirty gate, every press lands at once | §3.2, §3.4 |
| 2026-08-19, DESIGN §7.1.6c-6d | `profiles.json` is followed live; a file that will not parse mid-run leaves the table in force; a deleted row falls to the floor profile with no Undo; a running pane keeps its shell; the editor keeps its draft and the typing hand wins its field | §1.2, §3.4, §5 |
| 2026-08-19 (owner), DESIGN §7.5 | `pins.json`: the target string is the identity; every press is written | §3.4 |
| 2026-08-26 (owner), DESIGN §7.1.6c-2′ | A keybindings file describes a table, not a sequence: it is read in two passes, so a swap of two chords is legal | §3.4 |
| 2026-09-22/23 (owner), R33 | An import enters each part by the door a hand edit takes; settings values go through each row's own apply function | §5.1 |
| 2026-10-05 (owner), R33 | A pane is born with the account's current state; a running shell is not rewritten | §5.1 |
| ARCHITECTURE §4, §5.3; B5 | No timer. The window thread waits on nothing new (`window_waits.tsv` does not grow). One owner per fact. A stale answer never replaces a fresh one | §2, §3, §5.3 |
| R31 (DESIGN §7.1.3g ②) | A file is not read because time passed; a change notification is not time passing | §2 |
| owner, user-facing copy | Statements, no editorialising (「UI 文案不夹议」); settings descriptions in book-style statements (「设置描述用书面陈述句」); few words (「界面字要少」) | §3.6 |

---

## 0. In one page

- **What stands today** (§1). Four documents in the data directory are written by Folio
  when a person changes something and may also be edited by hand: `settings.json`,
  `keybindings.json`, `profiles.json` and `pins.json`. There is one `App` per process,
  and it holds all four stores (`main.rs:12543-12559`, fields `:12897-12939`). **Two of
  the four are already followed live.** Since 2026-08-19 one nameless tree watch on the
  data directory (`storage_watch.rs:87-90`, `dir_news.rs:110-125`) re-reads `profiles.json`
  and `pins.json` after `WatchClock`'s quiet window. It compares each with the document
  in force (`main.rs:48494-48500`). **The other two are read at start only**
  (`persist.rs:1594-1617`, `:1783-1805`), and R33 says so in as many words
  (`RULES.md:990`, `:1030-1034`). Every write of all four is the whole document, written
  synchronously on the window thread, with no condition (`persist.rs:1733-1752`,
  `:1863-1877`, `:2059-2072`, `pins.rs:255-263`).
- **What that loses** (§1.5; a reading of the code, not reproduced):
  1. **A change Folio could not write is undone by the next re-read** of `profiles.json`
     or `pins.json`. The re-read compares the file with the *in-memory* document, which
     holds the unwritten change. It finds them different and installs the file
     (`persist.rs:2026-2030`, `pins.rs:223-227`). The failed write's own temporary file
     is enough to wake the watch.
  2. **A hand edit saved less than one quiet window before a press in Folio is
     overwritten.** The press writes the whole document over it. The re-read that
     follows finds the file equal to what is in force and says nothing.
  3. **A hand edit of `settings.json` or `keybindings.json` is overwritten by Folio's
     next write of that file.** Three of the settings writes are not a person's gesture
     at all: the cards' hint, the startup theme carry-forward and the web-pages
     receipt.
  4. **Each re-read that finds `profiles.json` or `pins.json` broken writes one more
     `.rejected-<stamp>` copy** beside it.
- **The design, in five parts:**
  1. **The ear** (§2). The storage watch becomes a **named, shallow** ear
     (`Depth::ShallowNamed`, B5's door). A pure name filter turns each notification into
     stamps for the documents it names; an overflow stamps all four, and so does the
     first `Armed` (B5's "first `Armed` is one re-read"). There is one `WatchClock` for
     the folder, so a `git checkout` of four files is one pass. A `session.json` flush
     no longer wakes it.
  2. **Revisions** (§3.1). Each store keeps a **revision**: the exact bytes Folio last
     read or wrote (or "absent"), and the document they parse to. A re-read whose bytes
     equal the revision, or a write still in flight, is Folio's own, and nothing
     follows. Anything else is a hand edit.
  3. **The conditional write** (§3.2). Every write of the four leaves the window thread
     for the existing storage worker (the `session-writer`, `persist.rs:564`), which is
     window-waits row 20's own disposition. The worker reads the file and writes only
     if it is still the revision. It then replaces the file **through an exchange that
     hands back what it displaced** (Windows `ReplaceFileW` with a backup name, macOS
     `renamex_np(RENAME_SWAP)`). A hand edit that lands between the check and the
     replace therefore comes back as bytes to merge, never as a loss.
  4. **The merge** (§3.3–§3.4). A hand edit is merged three ways: the base is the
     revision, ours the in-memory document, theirs the file. A unit changed on one side
     takes that side. A unit changed on both sides differently is a **conflict, and
     the file wins**. The unit is the field for settings; the action, then the chord,
     for keybindings; the id (then field, then order) for profiles; the identity for
     pins, a three-way union in which a removal on either side is a change. Each change
     is put in force through the door a press already uses.
  5. **Owed edits** (§4). An edit is owed exactly when the in-memory document differs
     from the revision's. Nothing else records it, so it survives every merge by
     construction. It is written again on the next change and after the next pass of
     the ear, never on a clock. One still owed at quit is tried once more on the storage
     worker, before the quit's last session write. One that still cannot land is kept
     beside its file and named in `diagnostics.log`.
- **What a person sees** (§3.6):
  - A clean hand edit: the window follows, with no card. The person is looking at the
    file they saved.
  - A hand edit that replaced a value set in Folio: **one card per pass**, naming the
    file and the values.
  - A file that will not parse mid-run: the existing "kept" card, now for all four
    files.
- **What must never happen, and why it cannot** (§3.5):
  - A hand edit silently lost: the exchange hands it back.
  - A Folio write over a newer file: the worker checks first, then exchanges.
  - A half-written file read as a document: Folio's own writes rename into place, and
    a torn external write is never valid JSON for these documents (§3.5 says why).
- **Readers** (§5):
  - Settings: each changed field goes through its row's apply function, as an import
    does. Every other window learns through the existing `pending_application_change`
    sweep.
  - Keybindings: the one table is rebuilt from the defaults and the merged departures.
  - Profiles: `take_profile_table`.
  - Pins: a chrome refresh.

  **Running panes keep their shell; a changed default profile applies to new panes
  only.** There is no new window-thread wait: the re-read's reads and parses also move
  to the storage worker.
- **Several windows, several processes** (§6). The writer is a process, not a window.
  The pass runs once, in the window that turns the application's clocks. A second
  process over the same directory exists only when the launch hand-over failed. It
  already follows `profiles.json` and `pins.json` today. With this design it follows
  all four by the same merge, and it still writes nothing.
- **Tickets** (§9), in dependency order: **T-STORAGE-EAR-NAMED** (S) →
  **T-STORE-REVISION** (S) → **T-STORE-CONDITIONAL-WRITE** (M; E-1, E-2) →
  **T-STORE-MERGE** (M) → **T-STORE-NOTICE** (S). Each names its tests and the mutation
  each must be seen failing under (§8).
- **Five owner questions** (§10), each with a recommendation:
  - When the same value was changed in Folio and in the file, should the file win, for
    each document? (Recommended: yes.)
  - Should a clean hand edit pass without a card, with a card only when a Folio value
    gave way? (Recommended: yes.)
  - Should `settings.json` and `keybindings.json` be followed live? This reverses R33's
    sentence. (Recommended: yes.)
  - Should a change still unwritten at quit be kept as a copy beside its file?
    (Recommended: yes.)
  - Should a document that is a link into another folder stay out of scope?
    (Recommended: yes, for now.)

---

## 1. What stands on `77399d57`

### 1.1 The four documents, their stores and their writers

| document | store | read | written | write sites |
|---|---|---|---|---|
| `settings.json` | `persist::SettingsStore` (`persist.rs:1559-1587`) | at start (`persist.rs:1594-1617`, called at `main.rs:43944`) | `SettingsStore::store` → `write_now` (`persist.rs:1695-1710`, `:1733-1752`): the whole document, through `bt_persist::write_settings_atomic` (`bt-persist/src/lib.rs:167-173`) | 44 call sites in 16 files, each `loaded().clone()`, one field changed, `store(...)` — e.g. `runtime/profiles.rs:644-655`, `runtime/clipboard.rs:172-186`; plus `web_spare::reconcile_web_pages_used` (`main.rs:43949`) and the startup theme carry-forward (`main.rs:44232-44238`) |
| `keybindings.json` | `persist::KeybindingsStore` (`persist.rs:1771-1779`) | at start (`persist.rs:1783-1805`), laid over the defaults by `Shortcuts::apply_overrides` (`main.rs:44198-44224`) | `store` → `write_now_on` (`persist.rs:1837-1848`, `:1863-1877`) | one: `Runtime::store_keybindings` (`runtime/keyboard.rs:262-274`), which derives the departures from the live table |
| `profiles.json` | `persist::ProfilesStore` (`persist.rs:1922-1930`) | at start (`persist.rs:1953-1982`, `main.rs:44165-44171`), **and on every storage-watch pass** (`ProfilesStore::reread`, `persist.rs:2019-2031`) | `store` → `write_now` (`persist.rs:2037-2048`, `:2059-2072`) | the profile dialog's verbs, one write per editor keystroke (`runtime/profiles.rs:458`); an import (`runtime/configuration.rs:189`) |
| `pins.json` | `pins::PinsStore` (`pins.rs:121-129`) | at start (`pins.rs:149-178`, `main.rs:44192`), **and on every storage-watch pass** (`PinsStore::reread`, `pins.rs:216-228`) | `store` → `write_now` (`pins.rs:234-245`, `:255-263`) | every pin press (`pins.rs:34-40`; `main.rs:50811`) |

Every write is the store's whole in-memory document, through `bt_persist::atomic_write`:
a temporary sibling, `fsync`, then a rename over the name (`atomic.rs:28-38`,
`:212-273`). It is synchronous on the window thread. Three of the four writers are
measured by a `hang_watch` station (`persist.rs:1747`, `:1872`, `:2067`); the pins
writer is not (`pins.rs:259-262`). These writes are window-waits row 20, which is open
with the disposition "storage lane, with document-revision preconditions and receipts"
(`window_waits.tsv:74`, `ARCHITECTURE.md:1239`). **This design is that disposition.**

A store whose process is not the data directory's writer writes nothing and owes nothing
(`persist.rs:1734-1739`, `:1864-1868`, `:2060-2064`, `pins.rs:256`). During an update's
trial, every write is deferred and released at the commit (`update_trial.rs:1-26`,
`persist.rs:1742-1744`).

### 1.2 The ear that already exists

`StorageWatch` (`storage_watch.rs:62-119`) is one `DirNews` (`dir_news.rs:56-74`):
- an ear on the watch steward at `Depth::Tree`, nameless (`dir_news.rs:110-125`);
- a one-slot mailbox holding the last `Instant`;
- one `WatchClock`: 300 ms quiet, 2 s floor (`watch_clock.rs:33`, `:51`, `:89-97`).

It is armed once, at startup (`main.rs:45019-45024`). A first `Armed` report is stamped
as news, so the folder is read once more after the steward's start
(`dir_news.rs:146-180`). Its clock is advanced in the window that turns the
application's clocks (`runtime/frame.rs:1747-1758`, deadline `:2426-2430`). When the
clock is due, the window re-reads `profiles.json` and then `pins.json`
(`main.rs:48494-48500`).

The watch's header gives reasoning this design keeps, and one sentence it changes.
- **Kept**: "this window's own writes have to be answered by that comparison anyway"
  (`storage_watch.rs:42-45`).
- **Changed**: the notification is not filtered by name, because "parsing the kernel's
  `FILE_NOTIFY_INFORMATION` names … is the one thing `DirWatch` promises never to do"
  (`storage_watch.rs:39-42`; DESIGN §7.1.6c-6d, `DESIGN.md:2815`).

B5 has since given `DirWatch` named doors (`Depth::TreeNamed`, `Depth::ShallowNamed`,
`watch_steward.rs:45-60`). The program and index ears already filter names on the
watcher thread (`programs_ear.rs:1-6`, `index_ear.rs:142-154`). An overflow arrives as
`DirChange::Unknown`, which a filter must let through (`bt-platform/src/lib.rs:16603-16618`).
That promise is therefore no longer the constraint.

### 1.3 How "Folio's own write" is told apart today

It is told apart by document equality with the store's in-memory document:
`self.loaded == file` (`persist.rs:2026`, `pins.rs:223`). This is correct only while the
in-memory document equals the disk. That holds after a successful synchronous write and
at no other time. Defects 1 and 2 in §1.5 are the two other times.

### 1.4 What a damaged file does

- **At start**, a refused document falls back to its defaults. Its bytes are kept beside
  it as `<name>.rejected-<YYYYMMDD-HHMMSS>` (`migrate.rs:1585-1599`, `:1701-1705`,
  `:1735-1750`), and one card names the copy (`persist.rs:157-177`).
- **Mid-run** (profiles and pins only), a refused document is **not** taken. The table in
  force stays in force. One card says so, and it is not repeated until the file reads
  cleanly again (`runtime/profiles.rs:699-713`, `main.rs:48514-48524`; DESIGN §7.1.6c-6d,
  `DESIGN.md:2817`).
- **A file deleted mid-run** means the shipped table (`DESIGN.md:2819`; test
  `a_file_that_is_deleted_means_the_shipped_table_rather_than_an_error`,
  `persist.rs:4320`).

### 1.5 What is lost today (a reading of the code)

1. **An unwritten change is undone by the next re-read** (`profiles.json`, `pins.json`).
   - `store` puts the new document in memory before it writes (`persist.rs:2042`,
     `pins.rs:239`).
   - If the write fails, `DocumentWrites` records the document as unsaved
     (`persist.rs:115-134`). Only after five failures in a row does a card say so
     (`persist.rs:56`, `:129-131`).
   - The failed write itself made and removed a temporary file in the watched folder
     (`atomic.rs:212-243`, `:262-272`), and the watch is a tree watch on that folder.
   - About 300 ms later the re-read reads the old file and compares it with the
     in-memory document (`persist.rs:2026`). It finds them different and installs the
     old file (`:2029`).

   The profile dialog then shows the change gone, with no card. Ways in: a
   `profiles.json` marked read-only, an antivirus holding the file, a sync client in
   the middle of a rename.
2. **A hand edit is overwritten by a press that lands before the clock does.** A person
   saves `pins.json` in an editor and, within the quiet window (300 ms to 2 s), pins a
   folder. The press writes the whole in-memory table, which does not have the hand
   edit, over the file (`pins.rs:202-207`, `:255-263`). The re-read that the editor's
   save scheduled then finds the file equal to what is in force, and answers
   `Unchanged`. The same holds for every profile-dialog keystroke
   (`runtime/profiles.rs:458`).
3. **A hand edit of `settings.json` or `keybindings.json` lasts until Folio's next write
   of that file.** This is ruled: R33 (`RULES.md:990`), ARCHITECTURE §9
   (`ARCHITECTURE.md:1659`), `settings_bundle.rs:28-29`. But not every such write is a
   person's gesture:
   - a frame spends the cards' gesture hint (`main.rs:46199-46207`);
   - the theme carry-forward writes at start (`main.rs:44232-44238`);
   - so does the web-pages receipt (`main.rs:43949`).

   A person who edits `settings.json` while Folio runs loses the edit at a moment they
   did not choose.
4. **Each mid-run pass over a broken file costs a new copy.** `reread` reads with
   `Keeping::Now` outside a trial (`persist.rs:2020`, `pins.rs:217`,
   `update_trial.rs:543-549`). Each refusal writes a new `.rejected-<stamp>`
   (`migrate.rs:1590-1592`), and the stamp counts up rather than overwrite
   (`migrate.rs:1733-1750`). An autosaving editor open on a half-typed `profiles.json`
   leaves up to one copy every 2 s.

### 1.6 What a hand edit cannot carry, and this design leaves alone

`SettingsV1`, `KeybindingsV1` and `ProfilesV1` keep no keys they have no name for. A
hand-added key is therefore dropped at the next write; that is the 2026-08-03 ruling
(S§1.3 rule 4, `M2-persistence-schema-v1.md:75-80`). `PinsV1` and `PinEntryV1` do keep
theirs (`bt-persist/src/pins.rs:26-28`, `:54-58`). R14's one-line summary
(`RULES.md:407-408`) says unknown keys are kept, which is true of `pins.json` only; the
ticket that folds R14 should correct it, and this note does not change it. The merge
below works on typed documents, so each document keeps its own rule unchanged.

---

## 2. The event and the ear

### 2.1 The event

A person changes one of the four files while Folio runs. This happens in three ways:
- **An editor saves.** Most editors rename a temporary file into place; some truncate
  and write in place (E-3 lists Notepad, and vim, which depends on `backupcopy`).
- **A sync client replaces the file.** Syncthing writes a temporary file, then renames
  it.
- **`git checkout`** runs in a dotfiles repository whose working tree is the data
  directory. Git unlinks each changed file and writes a new one in place, not by rename
  (E-3).

Folio must learn **which documents** moved, and when the folder went quiet. It must not
mistake its own write for any of these.

### 2.2 The ear (adopted)

**`StorageWatch` becomes a named, shallow ear with one clock.**

- **Depth: `ShallowNamed`.** The four documents are entries of the data directory itself.
  A tree watch also hears `schemes\`, which has its own ear (`main.rs:12710-12725`), and
  every `session.json` flush. Today those flushes cost two re-reads every 1.5 s while a
  layout changes. A shallow named watch hears only the directory's own entries, by name,
  which is what the program ear already does for `PATH` directories
  (`programs_ear.rs:48-50`).
- **A pure filter on the watcher thread**, `storage_watch::documents_named(change) ->
  DocumentSet`, in the shape of B5's `bears_on_index` (`index_ear.rs:149`):
  - `Named(names)` stamps every document of `{settings.json, keybindings.json,
    profiles.json, pins.json}` whose exact name appears, including the new name of a
    rename, so an editor's rename-into-place counts. The temporary names that Folio's
    writer and editors use (`<name>.tmp-…`, `<name>~`, `.syncthing.<name>.tmp`) match
    nothing, and neither do `.rejected-…` copies;
  - `Unknown` (an overflow) stamps all four;
  - `Ended` stamps nothing: it is the steward's terminal report
    (`watch_steward.rs:24-31`).

  An empty set never reaches the mailbox, so a folder that moved for another file wakes
  nothing.
- **The mailbox** holds the latest `Instant` for each document. It is not a queue, for
  the reason `DirNews` gives (`dir_news.rs:67-72`).
- **One `WatchClock` for the folder, not one per document.** A `git checkout` of four
  files is one burst, so one clock makes it one pass: the pass reads the four documents
  and says at most one sentence (§3.6). Four clocks would mean four passes and four cards
  for one gesture.
- **The first `Armed` stamps all four.** This is B5's rule (`dir_news.rs:146-152`): the
  documents were read at open, before the steward's start, so a write in between reached
  nobody. A `Failed` member is retried at the next reader-triggered event, never on a
  clock (`watch_steward.rs:26-31`). Here, that event is any press that writes one of the
  four.
- **What the pass asks for**: the stamped documents, read on the storage worker (§3.2).
  The answer comes back on a later turn; nothing is read on the window thread.

**Superseded**: the sentence "不在通知那一端过滤" in DESIGN §7.1.6c-6d
(`DESIGN.md:2815`), and `storage_watch.rs:33-45`. The reason recorded there (no name
parsing in `DirWatch`) was removed by B5. The paragraph's other half stands: Folio's own
writes are still recognised by comparison (§3.1), not by name.

**No timer, no polling.** The clock starts only on a stamp (R31; `watch_clock.rs:9-17`).
A folder nobody touches schedules nothing.

**Linux has no ear.** `DirWatch` refuses on the portable arm (OS§0). On Linux the
documents are read at start only, and the conditional write (§3.2) still prevents the
losses in §1.5.

---

## 3. The storage transaction

### 3.1 Revisions

Each of the four stores gains one field:

```text
revision: Revision<T> { bytes: Option<Vec<u8>>, document: T }   // None = no file
```

- **At open**, it is set to the bytes read and the document they parsed to.
  - A file that was absent gives `bytes: None` and the default document.
  - A file refused at start gives the refused bytes and the default document. The file
    was not taken, and it is what the next write replaces, after R4-3 has kept its copy
    (§1.4, unchanged).
- **Every landed write** sets it to the bytes written and their document (§3.2's
  receipt).
- **Every merge** sets it to the file's bytes and document (§3.3).
- **While a write is with the worker**, the store also holds that write's
  `(generation, bytes)`.

**Folio's own write.** A re-read's answer is Folio's own write, or no change at all, when
its bytes equal `revision.bytes` or the in-flight bytes. Then nothing follows. The test
is on bytes, not documents, because the question is "is this the file Folio left?". An
editor that re-indents produces a second spelling of the same document. That is a hand
edit that changes no field: the merge adopts the new bytes as the revision and changes
nothing else.

**The in-memory document is never the comparison.** This one sentence is the fix for
§1.5 defect 1: the re-read compares with the revision, so an unwritten change is not
mistaken for the file "moving back".

Memory: four documents of a few kilobytes each, plus their bytes, once each.

### 3.2 The conditional write, on the storage worker

**Where.** `persist::SessionWriter` (`persist.rs:564-866`) is the one storage worker. It
takes jobs in order on one channel (`persist.rs:559-563`, `:640-676`) and already writes
three kinds of document (`StorageJob`, `persist.rs:490-511`). It gains two job kinds:
- `StorageJob::Document { which, generation, expected: Option<Vec<u8>>, bytes, answer }`;
- `StorageJob::Read { which: DocumentSet, generation, answer }`, the pass's read
  (§5.3).

Each answers on its own channel, as `Receipt` and `Handoff` do (`persist.rs:497-510`).
The worker then wakes the loop with one new `AppEvent::DocumentsAnswered`: the answer
first, the wake after, which is the steward's order (`watch_steward.rs:308-312`). The
four stores' `write_now` hand their document to the worker instead of writing it.

**Ordering is not a question.** There is one worker, so a `Read` queued after a
`Document` sees that write. A quit's final session job lands after every document job
queued before it.

**The worker's steps for one `Document` job:**

1. **Precondition.** Read the file (bounded, on the settings lane,
   `migrate.rs:1657-1688`). If its bytes are not `expected`, or the file exists when
   `expected` is `None`, answer `Moved { bytes }` and write nothing. The file moved on,
   and Folio does not write over a file it has not read.
2. **Write the temporary sibling and `fsync` it** (`atomic.rs:212-243`, unchanged).
3. **Exchange, keeping what was displaced.**
   - *Windows:* `ReplaceFileW(target, temp, backup)` with a fresh sibling backup name.
     The platform already makes this call, and already keeps the backup whenever it
     holds the only copy (`file_replace.rs:284-359`, `:366-375`). The new door,
     `bt_platform::replace_keeping_displaced`, returns with the backup in place instead
     of retiring it.
   - *macOS:* `renamex_np(temp, target, RENAME_SWAP)`. The platform already uses this
     call for the bundle exchange (`install_flip.rs:7-15`, `install_txn.rs:197-200`).
     After the swap, the temporary name holds the displaced file.
   - *No file yet* (`expected` is `None`): `bt_platform::rename_without_replacing`
     (`bt-platform/src/lib.rs:2278-2306`). An `AlreadyExists` answer is treated as
     `Moved`.
4. **Verify.** Read the displaced file.
   - If its bytes are `expected`, answer `Landed { bytes }` and remove the displaced
     file.
   - If not, a hand edit landed between steps 1 and 3. Folio's document is now on disk,
     and the person's bytes are in the displaced file. Answer
     `Displaced { bytes, kept: path }` and leave that file in place. The window thread
     merges (§3.3) and writes again, with `expected` set to what Folio just wrote.

   The kept file is removed after that second write lands, not before. A crash in
   between leaves the person's bytes on disk under a name in the `.rejected-` family
   (`<name>.displaced-<stamp>`, using `migrate.rs:1735-1750`'s naming).
5. **Refused by the disk.** Answer `Refused { error }`. This is today's failure, and
   `DocumentWrites` counts it as it does today (`persist.rs:115-134`).

**A volume that cannot exchange.** Some redirectors refuse `ReplaceFileW`, and the
platform already classifies that refusal as `VolumeCannot` (`file_replace.rs:261-275`).
Whether `RENAME_SWAP` works on volumes other than APFS is E-2. On such a volume the write
is step 1 followed by a plain rename, which leaves the race window between steps 1 and 3
open: microseconds long, but open. The receipt then says `Landed { verified: false }`, and
`diagnostics.log` says once per run per file that writes there are unverified. This is a
property of the volume, stated rather than hidden. A local data directory on NTFS or APFS
takes the exchange.

**What the window thread does with an answer.** It acts only on the newest generation.
An answer for a generation older than the store's newest send is dropped, as the session
store already drops a superseded receipt (`a_receipt_for_a_superseded_document_is_dropped`,
`persist.rs:3059`).

| answer | revision | next |
|---|---|---|
| `Landed { bytes }` | ← `bytes` and the document sent | if the in-memory document still differs (a press while in flight), send again |
| `Moved { bytes }` | unchanged | merge `bytes` as a hand edit (§3.3), then send again if anything is owed |
| `Displaced { bytes, kept }` | ← what Folio wrote | merge `bytes` (§3.3), then send the merged document; remove `kept` after that write lands |
| `Refused { error }` | unchanged | owed (§4); `DocumentWrites` counts the streak and raises today's card at five |

**Why on the worker and not in place.** A precondition read on the window thread would be
a new window-thread file read beside row 20's write. The worker already owns the disk for
`session.json` for that reason (`persist.rs:543-553`). Moving the four writes there
narrows row 20 instead of growing the census.

**The cost is a departure from "当场落盘"** (DESIGN §7.1.6c-2, `DESIGN.md:2418`; the
keybindings recorder in §7.1.6c-2′, `DESIGN.md:8193`). A press now hands its document to
the writer at once, and the document lands milliseconds later, not before the press
returns. The in-memory document, which every reader reads, is still updated at the press,
so nothing a person sees moves later. The ticket records this as a departure in its
DESIGN entry.

### 3.3 The merge

A re-read's bytes that are neither the revision nor the in-flight write are merged three
ways:
- the **base** `B` is `revision.document`;
- **ours**, `O`, is the in-memory document: the store's `loaded()`, or for keybindings
  the live table's departures (`runtime/keyboard.rs:262-272`);
- **theirs**, `T`, is the file's document. A missing file is the default document,
  which is §1.4's ruling "deleted file = shipped table", unchanged.

For each unit of the document (§3.4 says what a unit is, per document):

| `O` vs `B` | `T` vs `B` | result |
|---|---|---|
| same | same | `B` |
| changed | same | `O`, which stays owed |
| same | changed | `T`, put in force through its door |
| changed | changed, `O == T` | `T` (no conflict) |
| changed | changed, `O != T` | **conflict: `T`**, named in the pass's sentence (§3.6) |

Then, **in this order**:

1. `revision` ← the file's bytes and `T`.
2. Writes are held: `SettingsStore::hold_writes` (`persist.rs:1712-1724`), in the shape
   of the import's batch (`runtime/configuration.rs:311-325`).
3. Every unit that takes `T` is put in force through its door (§5). The door's `store`
   makes that unit equal to the new base, so it is not owed.
4. Writes are released. If the in-memory document still differs from the revision (the
   units that kept `O`), it is sent, conditional on the file's bytes.

**Why this order.** Setting the base before the doors run lets the doors stay as they
are. A door that stores a value equal to the base creates no owed edit, and a unit that
kept `O` still differs from the base, so it is owed. No list of owed fields exists
anywhere (§4.1).

**An unreadable re-read** is not merged. That covers a parse refusal, a future schema
version and an oversized file. The revision is unchanged, and the document in force
stays in force: §1.4's mid-run rule, now applied to all four documents.

**No `.rejected-` copy is written mid-run** (§1.5 defect 4). Once writes are conditional,
a file that differs from the revision cannot be written over (§3.2 step 1). Its bytes are
in no danger, and the copy would protect nothing. The read therefore uses `Keeping::Owed`
semantics: it reports the refusal and keeps no copy. Any owed edit waits, because its
write would answer `Moved` until the file parses and merges.

### 3.4 One policy per document

#### `settings.json`: by field; the file wins a conflict

- **The unit is the field.** The merge destructures `SettingsV1` with no `..`, as
  `settings_bundle::plan_settings` does (`settings_bundle.rs:252-318`). A key added
  later then fails to compile until somebody says how it merges.
- **The diff from `B` to `T` is `plan_settings(B, T, platform)`.** It already turns every
  changed value into the `SettingChange` that puts it in force, and sorts the changes by
  platform into `apply` and `elsewhere` (`settings_bundle.rs:241-250`, `:501-507`).
- **The three machine receipts** are `first_run_card`, `cards_gesture_hint_offer` and
  `web_pages_used` (`settings_bundle.rs:33-41`). An import from another machine leaves
  them alone. A hand edit on this machine is a statement about this machine, so here they
  merge like any other field and are put in force by storing, since they have no door.
  The ticket checks the pin `the_cards_offer_is_spent_in_one_place_and_given_back_in_one`
  (`settings_bundle.rs:37`); if it counts writers of that field, the merge is added to
  it as a named third writer.
- **Why the file wins.** A conflict exists only while Folio's value is unwritten: owed,
  in flight, or held by a process that writes nothing (§4.2). The file's value is on
  disk, where the person can read it, diff it and change it again. Folio's value is one
  press away, and the card names it. If Folio's value won, the file's value would be
  gone from the only place it was written down.
- **What it forbids:**
  - a conflict resolved silently;
  - a value from the file applied without its row's door. A theme from the file and a
    theme from a press must not be able to make two different windows, which is the
    reason `reread_schemes` gives (`main.rs:48409-48411`).

#### `keybindings.json`: by action, then by chord; the file wins a conflict

- **The unit is one action id's departure** (`BindingOverrideV1 { action, chord }`,
  `bt-persist/src/keybindings.rs:79-93`). An action changed on one side takes that side.
  An action changed differently on both sides takes the file's departure.
- **Then by chord.** The table has an invariant that departures alone do not carry: one
  chord answers one action (DESIGN §7.1.6c-2′ ruling two, `DESIGN.md:2429`). The merged
  departures can give one chord to two actions, for example when Folio bound `Ctrl+K` to
  one action and the file bound it to another. Then:
  - the file's action keeps the chord;
  - Folio's action gets what the two-pass reader gives a refused line: its default, or
    unset when another action holds the default;
  - the pass's sentence names it.
- **The table is rebuilt** from this build's defaults and the merged departures by the
  two-pass reader, exactly as at start and on an import (`main.rs:44198-44224`,
  `settings_bundle.rs:510-560`).
- **Why the file wins**: the settings reason. This is also "the one file this product
  invites the user to hand-edit" (`DESIGN.md:2418`).
- **What it forbids:**
  - a merged table in which one press has two answers;
  - a resolution that depends on order, which is why the reader has two passes.

#### `profiles.json`: by id, then by field, then order; the file wins a conflict

- **Rows are matched by `ProfileEntryV1::id`** (`bt-persist/src/profiles.rs:120-129`),
  never by position. This is the reason given by DESIGN §7.1.6c-6d and
  T-PROFILE-TABLE-MOVE (`DESIGN.md:2821`).
- **Within a row, the unit is the field** (`display_title`, `hidden`, `program` and so
  on). Every field except `id` is optional, and an absent field means "the shipped value"
  (`bt-persist/src/profiles.rs:110-119`). So "the file removed my `display_title`" is a
  change of that field to absent, not a deletion of the row.
- **Added and deleted rows:**
  - a row added on either side is kept;
  - a row deleted on one side and unchanged on the other is deleted;
  - a row deleted on one side and changed on the other is a conflict, and the file's
    side stands, whichever side that was.

  A deleted row's panes follow the existing rule: they keep running and meet the floor
  profile at their next birth, with no Undo (`DESIGN.md:2819`,
  `runtime/profiles.rs:668-692`).
- **Order is data** (`bt-persist/src/profiles.rs:91-98`). If the file's order of the rows
  both sides share differs from the base's, the file's order is taken; otherwise Folio's
  order is kept. Rows that only Folio added go at the end, in Folio's order.
- **The editor sub-page** keeps its draft and follows its id (`DESIGN.md:2823`,
  `runtime/profiles.rs:741-765`). Its next keystroke writes the draft, so the typing hand
  still wins its field, at the next keystroke, which is when the ruling places it.
- **What it forbids:**
  - a pane re-pointed by position;
  - a row resurrected because one side still had it;
  - a draft overwritten under the typing hand.

#### `pins.json`: by identity; a three-way union

- **The identity is `(kind, target)`**, with the target string taken verbatim
  (`pins.rs:24-32`, `:102-113`).
- **The rows of the result:**
  - rows present in `B`, `O` and `T` stay;
  - a row added on either side is added;
  - a row removed on either side is removed.

  **This is a union of the additions, not a union of the tables.** A plain union would
  bring back every pin removed on one side, and an unpin is as much a change as a pin.
- **A row whose opaque `extra` changed:**
  - changed on one side and removed on the other: a conflict, and the file's side
    stands;
  - changed on both sides: the file's `extra`.
- **Order is display order** (`bt-persist/src/pins.rs:46-48`). Rows in the file keep the
  file's order, and Folio's additions are appended after them, the same rule a press
  follows (`pins.rs:54-56`).
- **What it forbids:**
  - a pin resurrected by the merge;
  - a row of a kind this build cannot name being dropped or retagged
    (`bt-persist/src/pins.rs:10-24`).

### 3.5 What must never happen, and why it cannot

- **A hand edit silently lost.** There are three ways it could be lost, and each is
  closed:
  - (a) Folio writes over it. The worker's precondition (§3.2 step 1) refuses any file
    that is not the revision.
  - (b) It lands between the check and the replace. The exchange hands back the
    displaced bytes (step 4). They stay on disk until the merge that includes them has
    landed, and they are merged as a hand edit.
  - (c) The merge drops it. A unit the file changed is the file's, unless Folio changed
    the same unit, and that case is a named conflict.

  The one residue is a volume that cannot exchange (§3.2), and that is reported, not
  silent.
- **A Folio write over a newer file.** This is (a) and (b) above, seen from Folio's
  side. Folio writes only after reading that the file is the one it last saw. In the race
  window, it writes and gets the newer file back.
- **A half-written file read as a document.**
  - *Folio's own writes* reach the name only by renaming a file that is already complete
    and `fsync`ed (`atomic.rs:245-273`). The name always points at either the old
    complete file or the new complete one.
  - *An external writer* that truncates and writes in place can be read half-way, and for
    these four documents that read is always refused:
    - each document is one JSON object whose last non-blank byte is its closing `}`, so
      no proper prefix of it is a JSON value;
    - when a longer old file is overwritten in place without truncation, bytes remain
      after the new `}`, and `serde_json::from_slice` refuses trailing characters;
    - preallocated zero bytes are not JSON either.

    A torn read is therefore a `ParseError` (`migrate.rs:1611-1639`). Mid-run, that means
    the file is not taken, the document in force stays, and one card says so (§3.3).
  - *A file absent for a moment* (git's unlink-then-write) reads as "no file", which is
    the default document. A pass lands inside that moment only when a burst outlasts the
    2 s cap (`watch_clock.rs:79-87`); the defaults are then put in force. The same
    burst's last write stamps the mailbox again, and the next pass puts in force the file
    the burst ended with. Folio writes nothing in between unless a press asks it to, and
    that write is conditional. The cost is a visible flicker during a checkout longer
    than 2 s; nothing is lost. E-4 measures how long a checkout's burst is.

### 3.6 What is said

- **A clean hand edit: nothing.** The person is looking at the file they just saved, and a
  card telling them what they typed is a card about nothing. That is `reread_schemes`'
  ruling on a scheme rename (`main.rs:48460-48464`) and what `reread_profiles` already
  does on `Changed` (`runtime/profiles.rs:714-729`).
- **A conflict: one card per pass.** It is a `ToastKind::Info` anchored to the window
  that ran the pass, and it lists every file and every value the file replaced. Draft
  wording, in English, with `// zh: pending` for the Chinese writer:
  > `settings.json changed on disk. Its Theme and Font size replaced the values set here.`

  It is a statement, with no advice and no "please" (the copy rules above). Each value
  is named the way the reader knows it:
  - a setting by its row's title (`SettingsRow::title`, as the import names it,
    `runtime/configuration.rs:331-336`);
  - a shortcut by its action's name;
  - a profile by its title;
  - a pin by its target.

  **One card per pass, not per document**: a `git checkout` of four files is one
  gesture.
- **A file that will not parse mid-run**: the existing "kept" card, shown once until the
  file reads cleanly again (`profiles_file_kept`, `pins_file_kept`;
  `main.rs:12922-12939`). Two new sentences of the same family are added:
  `settings_file_kept` and `keybindings_file_kept`.
- **`diagnostics.log`** gets one line:
  - per pass that merged (`BT_PERSIST <file> merged: took <n> from the file, kept <m>
    from Folio, conflicts <k>`);
  - per `Displaced` answer;
  - per unverified volume (§3.2);
  - per edit still owed at quit (§4.3).

---

## 4. Owed edits

### 4.1 What "owed" is

**An edit is owed when the store's in-memory document differs from its revision's
document.** Nothing else records it, so there is no "dirty" bit and no list of owed fields
that could drift from the documents. It follows that:

- **An owed edit survives every merge by construction.** A unit Folio changed and the file
  did not keeps `O`, which still differs from the new base (§3.3).
- **It is retried when a write could succeed, never on a clock.** The triggers are:
  - the next change, meaning any `store` of that document;
  - the next pass of the ear that touched that document (§3.3 step 4, and the `Moved`
    and `Displaced` rows of §3.2);
  - a `Landed` receipt for a document older than the in-memory one;
  - the quit (§4.3).

  A `Refused` write is not retried until one of these happens. That is today's rule (a
  press re-arms the streak, `persist.rs:136-139`), extended to the ear's passes. B5's
  rule for a failed watch is the same: "tried again once at the next reader-triggered
  event, and never because time passed" (`watch_steward.rs:26-31`).
- **How many times**: once per trigger. The disk's streak counts as it does today. Five
  refusals in a row raise the "could not be written" card once (`persist.rs:56`,
  `:129-131`, `i18n::persisted_file_unwritable`). A precondition refusal (`Moved`) is not
  a failure and does not count, because it means "merge first".
- **`DocumentWrites::unsaved`** (`persist.rs:76-77`) is replaced by the comparison with the
  revision. `wants_write` becomes "differs from the revision", which is the meaning R4-10
  gave it (`persist.rs:94-102`).

### 4.2 Who owes

- **The writer process** owes as described above.
- **A process that is not the writer** writes nothing and owes nothing
  (`persist.rs:1734-1739`). Its choices stay in its memory as `O` until the file changes
  the same unit. Then the file wins (§3.4), and the card says so, as it would in the
  writer.
- **An update's trial** (`update_trial.rs:8-26`) defers its writes. At the commit,
  `release_trial` hands each store's document to the worker as a write conditional on the
  revision, so a hand edit made during the trial is merged, not overwritten.

### 4.3 At quit

The quit's last session write already waits for the storage worker, within its budget
(`SessionStore::flush_judged`, `persist.rs:1336-1391`; window-waits row 16, ruled to
stay). **Owed documents are handed to the worker before the quit's final session job.**
The worker takes jobs in order, so the quit's existing wait covers them, and there is no
new wait.

An owed document can still be refused on that last attempt: `Moved`, against a file that
will not parse, or `Refused` by the disk. It is then **kept beside its file** as
`<name>.unsaved-<stamp>` (this is Q4):
- the worker writes the copy create-new, through the `rename_without_replacing` door;
- one `diagnostics.log` line names the file and the units;
- Folio never reads the copy back. The schema document's rule for `session.prev.json`
  applies (S§5.2, `M2-persistence-schema-v1.md:378-384`): it is material a person can put
  back by hand, so the next start does not argue with a file somebody was in the middle of
  editing.

**Why not a sidecar that the next start merges in.** An edit is still owed at quit for one
of two reasons. Either the disk refuses writes, and then it refuses the sidecar too. Or
the person left the file broken, and merging the sidecar back at the next start would be
the product overruling a file the person is still editing.

---

## 5. Readers

### 5.1 What each re-read rebuilds, and what it leaves alone

| document | rebuilt (through the door a press uses) | not rebuilt | cost |
|---|---|---|---|
| `settings.json` | Every field that takes `T`, through `apply_imported_setting` (`runtime/configuration.rs:348-490`): the row's own apply function, which an import already walks. The look (palette, schemes, language), the face, the caret and the Option key reach every other window through `pending_application_change` (`main.rs:15607-15641`; noted by `runtime/diagnostics.rs:34-37`; swept at the loop's door, `main.rs:63875-63902`). An open Settings dialog re-derives its content and keeps focus reachable (`runtime/configuration.rs:226-234`). `Focus mode` and `Shell integration` go through their row's press (`runtime/configuration.rs:417-433`). | A running shell: a pane keeps its program and environment. A changed `default_profile` applies at the next birth only (`runtime/profiles.rs:668-678`; `apply_default_profile`, `:644-655`; the 2026-10-05 ruling, R33 `RULES.md:998-1003`). | Each door's own cost, paid only for a field that changed. A font change re-measures every cell and resizes each shell once (`ApplicationChange::font`, `main.rs:15619-15620`). A theme change rebuilds the palette's rasters once; `watch_clock.rs:45-50` explains why the 2 s floor protects this. |
| `keybindings.json` | The one table, `app.shortcuts` (`main.rs:12898-12904`), rebuilt from the defaults and the merged departures. Dispatch reads it on the next press. The summoned terminal's global chord re-claims on the next turn (`DESIGN.md:8193`). An open shortcuts page re-derives (`runtime/keyboard.rs:249-252`). On macOS the menu's key equivalents are generated from the table (DESIGN §13.26); the ticket follows the path a recorded chord takes today. | Nothing else holds a chord. | One pass over the bindings to rebuild the table. |
| `profiles.json` | `take_profile_table` (`runtime/profiles.rs:737-780`): the install, the editor re-pointed by id, the row menu closed, the dialog content, `adopt_profile_table`. Menus and pickers derive from the table when they are built, and `profile_revision` is in `LayoutKey` (`runtime/profiles.rs:661-667`). | A running pane's shell (`runtime/profiles.rs:668-678`). The stored `default_profile` id, which is not rewritten when its row goes (`DESIGN.md:2819`). | Today's cost. |
| `pins.json` | `refresh_chrome` and a present (`main.rs:48527-48529`). Both PINNED sections derive from the store whenever a menu is built (`main.rs:48502-48507`). | — | A chrome refresh. |

### 5.2 Several windows

There is one `App`, and one set of stores, per process (`main.rs:12543-12559`). A
`Runtime` is one window paired with that `App` for the length of one call
(`main.rs:15594-15606`, `:15694-15697`). The pass runs once, in the window that turns the
application's clocks (`runtime/frame.rs:1747-1758`), and its cards belong to that window.

Every other window learns through in-process news that already exists:
- `pending_application_change` carries the look, the face, the caret and the Option key;
- menus, pins and profiles are derived when they are built.

No new cross-window mechanism is introduced.

### 5.3 The window thread

- **Reads and parses move to the worker.** `StorageJob::Read` reads the stamped documents
  (bounded, on the settings lane) and parses each through `bt_persist`'s chain
  (`migrate.rs:1611-1639`). It answers each document's bytes and parsed document, or its
  refusal. The window thread compares, merges and runs the doors. Today's profiles and
  pins re-reads read on the window thread (`persist.rs:2020`, `pins.rs:217`); they leave
  it.
- **Writes move to the worker** (§3.2). Row 20 narrows from "settings/keybindings/profiles
  writes" to none of the four documents. Its other members (the preview save, renames,
  diagnostics) are unchanged. `window_waits.tsv` gains no row, and only row 20's text is
  edited. The census
  (`hang_watch::window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site`) must
  show only removals.
- **Stale never over fresh.** Every `Read` and `Document` job carries the store's next
  generation. An answer older than the newest one sent for that document is dropped.
  Because the worker takes jobs in order, the newest answer reflects every write sent
  before it.

---

## 6. Several processes

- **The writer is a process.** The data directory's claim is held for the life of the
  process (`persist.rs:179-249`, `bt_platform::instance::try_claim_data_directory`). A
  second launch normally hands its command line to the running process and exits
  (`persist.rs:187-202`). A second process with a window exists only when that hand-over
  could not reach the running one: no endpoint, nobody listening, or no answer. That
  window writes nothing (`persist.rs:196-202`, `:1734-1739`), and it says so once
  (`i18n::second_instance_notice`, `i18n.rs:8716`; `main.rs:43957-43960`).
- **What it sees today.** Its own storage watch re-reads `profiles.json` and `pins.json`;
  `reread` has no writer check (`persist.rs:2019-2031`, `pins.rs:216-228`). So it already
  follows the writer's writes of those two, but never follows `settings.json` or
  `keybindings.json`.
- **With this design**, it follows all four by the same merge, owes nothing (§4.2) and
  writes nothing. Its re-reads go to its own storage worker, which only reads for it and
  never writes.
- **Another account** has another data directory (`persist.rs:2145-2195`). Nothing is
  shared, and nothing in this note applies.

---

## 7. Experiments owed (each before the ticket step that rests on it)

- **E-1 (Windows; before T-STORE-CONDITIONAL-WRITE step 3).** Run `ReplaceFileW` with a
  backup name on NTFS, on a data directory redirected to an SMB share, and on a
  OneDrive-backed folder. It must answer three questions:
  - Does the backup hold exactly the displaced bytes?
  - Which error does a redirector give? The answer extends `volume_cannot_replace`
    (`file_replace.rs:261-275`).
  - Can the platform's existing ladder (`:329-349`) recover a backup that a refused call
    left behind?
- **E-2 (macOS; the same step).** Run `renamex_np(RENAME_SWAP)` on APFS (the data
  directory's ordinary volume), on HFS+ and on SMB. Which volumes accept it, and which
  `errno` do the others give? `ENOTSUP` is expected.
- **E-3 (both; before T-STORAGE-EAR-NAMED).** Which names reach a `ShallowNamed` watch on
  the data directory for each of these:
  - VS Code saving;
  - Notepad saving;
  - vim with `backupcopy=yes` and with `backupcopy=no`;
  - Syncthing replacing a file;
  - `git checkout` replacing a file;
  - Folio's own write.

  Windows reports `FILE_ACTION_*` names, and macOS reports FSEvents paths. Each case must
  name the document or arrive as `Unknown`.
- **E-4 (both; informs the flicker in §3.5).** How long is the notification burst for a
  `git checkout` that changes all four documents? Does it fit inside the 300 ms quiet
  window (one pass), or can it exceed the 2 s cap?

---

## 8. Tests the build ships

Each test must be seen failing under its named mutation; the mutation is then reverted.
Every test carries Chinese and mixed-script samples (CONVENTIONS §三): the terminal font
`更纱黑体 Sarasa Mono SC`, the pinned folder `项目/folio 笔记` and the profile title
`Claude 代理`. No test is timing-bound:
- the clock is a fake `Instant`;
- the ear is fed through the steward's fake door (`watch_steward::fake::FakeDoor`, as at
  `index_ear.rs:167`);
- the worker's file system is a fake `DocumentDisk` seam, which can be told to land a hand
  edit between the precondition read and the exchange.

**The brief's six**

1. `storage_watch::tests::a_hand_edit_while_running_is_read_within_the_clock`. A
   `Named(["settings.json"])` change arrives at `t`. The pass is due at
   `t + WATCH_QUIET`, and it asks to read `settings.json` and nothing else. MUTATION:
   `documents_named` drops `settings.json`, or the stamp is not folded into the clock.
2. `persist::revision_tests::folios_own_write_is_not_a_hand_edit`. A store writes, the ear
   reports the document, and the read answers the written bytes. No merge runs and no
   door is called. Variant `…::a_write_still_in_flight_is_recognised_as_folios_own`: the
   read answers before the `Landed` receipt. MUTATION: compare with `loaded()` (today's
   `persist.rs:2026`), or ignore the in-flight bytes.
3. `settings_merge::tests::a_field_changed_on_both_sides_is_a_conflict_and_the_file_wins`.
   `B`, `O` (font `更纱黑体 Sarasa Mono SC`) and `T` (font `Cascadia Mono`) give `T`, and
   the conflict list names the row. Its companion is
   `…::a_field_changed_only_in_folio_survives_a_hand_edit_of_another_field`. MUTATIONS:
   ours wins; theirs is taken wholesale.
4. `persist::owed_tests::an_owed_edit_is_retried_after_the_merge_and_never_lost`. The disk
   refuses a pin, and a hand edit adds another pin. The pass merges, the conditional
   write sends both, and it lands. Its companion is
   `…::an_edit_still_owed_at_quit_is_kept_beside_its_file_and_named`. MUTATIONS: reset the
   in-memory document to `T` on merge; skip the quit's hand-over.
5. `persist::torn_tests::a_torn_file_is_refused_reported_and_never_written_over`. The file
   holds the first half of a pretty-printed `settings.json`, cut in the middle of a
   Chinese font name. The test checks that:
   - the pass answers unreadable;
   - the document in force is unchanged, and one "kept" card is raised;
   - a following press's write answers `Moved`, and the file's bytes are unchanged;
   - no `.rejected-` copy was written.

   MUTATIONS: take the default document on a mid-run refusal; write without the
   precondition; read with `Keeping::Now`.
6. `storage_reread_tests::a_checkout_of_all_four_files_is_four_merges_and_one_notice`.
   One burst names all four documents, and each document has one owed unit that
   conflicts with its file. The result is one pass, four merges and exactly one card,
   naming four files. MUTATIONS: one card per document; one clock per document.

**The conditional write**

7. `bt-platform` `file_replace::tests::a_displacing_replace_hands_back_what_it_displaced`,
   on Windows and macOS, over a temporary folder. MUTATION: retire the backup before
   returning (today's `retire_recovery`, `file_replace.rs:357`).
8. `persist::conditional_write_tests::a_file_that_moved_between_the_check_and_the_exchange_is_merged_not_lost`,
   with the fake disk landing a hand edit after step 1. MUTATION: answer `Landed` without
   comparing the displaced bytes.
9. `…::a_file_that_moved_before_the_check_is_not_written`. MUTATION: drop step 1.
10. `…::a_first_write_never_replaces_a_file_that_appeared`. `expected` is `None`, and a
    file appears first. MUTATION: a plain rename.

**The policies**

11. `pins::tests::a_three_way_merge_keeps_both_sides_additions_and_both_sides_removals`.
    MUTATION: a plain union of `O` and `T`, in which a removed pin returns.
12. `profiles_merge::tests::rows_merge_by_id_and_a_reordered_file_keeps_its_order`.
    MUTATION: match rows by index.
13. `…::a_row_deleted_in_the_file_and_edited_here_is_deleted_and_named`. MUTATION:
    Folio's side wins.
14. `keybindings_merge::tests::a_chord_claimed_on_both_sides_goes_to_the_files_action`.
    MUTATION: resolve chord collisions in departure order.

**Readers, windows, processes**

15. `app_configuration_tests::a_settings_reread_reaches_every_window`, with two windows
    and a theme in the file. MUTATION: store the merged document without the row's door,
    so no `ApplicationChange` is noted.
16. `app_profiles_tests::a_reread_default_profile_applies_to_new_panes_only`. MUTATION:
    re-spawn the focused pane.
17. `persist::revision_tests::an_older_answer_never_lands_over_a_newer_one`. MUTATION:
    apply every answer.
18. `persist::owed_tests::a_process_that_is_not_the_writer_follows_and_writes_nothing`.
    MUTATION: hand the document to the worker regardless of `writer_of_record`.

**The census**

19. `hang_watch::window_waits_tests::every_bare_site_is_a_row_and_every_row_a_site` stays
    green. Row 20's text changes, and the diff of `window_waits.tsv` shows only that edit
    and removed sites. This test has no mutation: it is the census itself.

---

## 9. Tickets, in dependency order

Each ticket starts from a worktree off `main` and runs the usual gates:
- rustfmt, and workspace clippy with `-D warnings`;
- the whole bt-app suite and the crate suites;
- every `scripts/ci/check-*.ps1`, with `window_waits.tsv` gaining no row;
- the guard set (`hang_watch::`, `cross_window_drag_tests::`, `every_bare_site_is_a_row`,
  and `bt-source`'s census, real_workspace and tripwire tests);
- the Mac gate for any `cfg(macos)` arm.

### 9.1 T-STORAGE-EAR-NAMED (S; first; E-3 first)

The work:
- `StorageWatch` becomes a `ShallowNamed` ear on the steward, with the pure filter
  `documents_named` (§2.2), a per-document mailbox and one `WatchClock`. `Armed` stamps
  all four documents.
- `advance_storage_watch` (`main.rs:48494-48500`) re-reads only those of its two documents
  that were stamped. Behaviour is otherwise unchanged; there are just fewer re-reads.
- `DirNews` stays, for the schemes folder alone.

Docs: a DESIGN entry superseding §7.1.6c-6d's sentence "不在通知那一端过滤" (§2.2); the
storage watch's header.

Tests:
- test 1;
- `storage_watch::tests::a_session_flush_wakes_nothing`. MUTATION: the filter answers all
  four for any name;
- `…::a_first_armed_reads_all_four`. MUTATION: `Armed` stamps nothing.

### 9.2 T-STORE-REVISION (S; after 9.1)

The work:
- `Revision<T>` on the four stores (§3.1), set at open and on every landed write.
- `ProfilesStore::reread` and `PinsStore::reread` compare the file's bytes with the
  revision, instead of comparing the document with `loaded()`. This alone fixes §1.5
  defect 1: an unwritten change is no longer undone.
- Writes stay where they are in this ticket.

Docs: a DESIGN entry; the sentence at `storage_watch.rs:42-45`.

Tests:
- test 2 (its in-flight variant waits for 9.3);
- `persist::revision_tests::an_unwritten_change_is_not_undone_by_a_reread`. MUTATION:
  compare with `loaded()`.

### 9.3 T-STORE-CONDITIONAL-WRITE (M; after 9.2; E-1 and E-2 first)

The work, in four parts:
1. `StorageJob::Document` and `StorageJob::Read` (§3.2, §5.3), with
   `AppEvent::DocumentsAnswered`.
2. `bt_platform::replace_keeping_displaced`, with Windows and macOS arms, and
   `rename_without_replacing` for a first write.
3. The four stores hand their writes to the worker:
   - owed means "differs from the revision" (§4.1);
   - answers are handled as in §3.2's table;
   - the quit hands owed documents over before its final session job, and writes the
     `.unsaved-` copy when the last attempt is refused (§4.3);
   - a mid-run refusal keeps no copy (§3.3).
4. Window-waits row 20 narrowed (§5.3).

Docs:
- a DESIGN entry, including the departure from "当场落盘" (§3.2);
- A§5.1 (the storage worker's new jobs) and A§0.1 (the `AppEvent` count);
- row 20 in `window_waits.tsv` and A§5.3.

Tests: 4, 5, 7, 8, 9, 10, 17, 18 and 19, and the in-flight variant of 2.

### 9.4 T-STORE-MERGE (M; after 9.3)

The work:
- the three-way merge (§3.3) and the four policies (§3.4);
- `settings.json` and `keybindings.json` join the pass;
- the readers of §5.1, reached through the existing doors, with writes held for the
  batch.

Docs:
- a DESIGN entry;
- **R33's text**, which this ticket changes and says so: "not re-read from disk during a
  run" and "the contrast that matters" (`RULES.md:990`, `:1030-1034`);
- ARCHITECTURE §9's row (`ARCHITECTURE.md:1659`), from "two disciplines" to one;
- the sentence at `settings_bundle.rs:28-29`;
- one CHANGELOG line: "Settings, shortcuts, profiles and pins edited in a text editor
  while Folio runs take effect without a restart, and a change made in Folio is no longer
  lost to one made in the file".

Tests: 3, 11, 12, 13, 14, 15 and 16.

### 9.5 T-STORE-NOTICE (S; after 9.4)

The work:
- the one conflict card per pass (§3.6);
- `settings_file_kept` and `keybindings_file_kept`;
- the `diagnostics.log` lines.

Each new English string carries `// zh: pending` and is listed for the Chinese writer.

Tests:
- test 6;
- `app_configuration_tests::a_clean_hand_edit_raises_no_card`. MUTATION: raise the card on
  every merge.

---

## 10. Questions for the owner (recommendation first)

**Q1. When a value was changed both in Folio (and not yet saved) and in the file, which
one stands?** The proposal for each file:
- settings: the file's value, field by field;
- shortcuts: the file's binding, and a key claimed on both sides goes to the file's
  action;
- profiles: rows matched by id, with the file's value for each field, and the file's
  order when the file reordered them;
- pins: every pin added on either side is kept, and every pin removed on either side is
  removed.

**Recommendation: yes, the file wins every conflict, and one card names what gave way.**
The file is the copy the person can read and change again. The value set in Folio is one
press away, and the card names it. Conflicts occur only while Folio's value is unwritten,
which is rare.

**Q2. Should a hand edit that conflicts with nothing take effect silently?**
**Recommendation: yes.** The person is looking at the file they just saved, and a card
repeating it says nothing (the scheme-rename ruling). The card appears only when
something set in Folio was replaced, and at most once per pass, so a `git checkout` of
all four files gives one card.

**Q3. Should `settings.json` and `keybindings.json` be followed while Folio runs, like
`profiles.json` and `pins.json`?** This reverses R33's "not re-read from disk during a
run". **Recommendation: yes.** Today a hand edit of either file is overwritten at Folio's
next write, which may not even be a press (the cards' hint, the theme carry-forward). One
discipline for all four files is what ARCHITECTURE §9 names as missing.

**Q4. When you quit, a change Folio still could not write (the disk refused it, or the
file was left half-edited) would be kept as a copy beside the file. Should it be?** The
copy would be named `settings.json.unsaved-<time>`, and Folio would never read it back,
the same footing as `session.prev.json`. **Recommendation: yes.** Without the copy, that
change exists only in `diagnostics.log`.

**Q5. If one of these files is a link into another folder (a dotfiles repository linked
into the data directory), should following it stay out of scope?** Folio would hear
changes made through the link, but not a `git checkout` in the other folder. Its writes
would also replace the link with a file, as they do today (`atomic.rs:137-140` names the
same gap for documents). **Recommendation: yes, for now.** Following links means
watching a second folder for each link and writing through the link, which needs its own
design. A dotfiles repository whose working tree is the data directory itself works with
this design as it stands.

---

## 11. Sources

Folio, at `77399d57`:
- every `file:line` above;
- `docs/M2-persistence-schema-v1.md` §1.3, §2 and §5.1–§5.4;
- `docs/DESIGN.md` §7.1.6c-2 (`:2409-2419`), §7.1.6c-2′ (`:2421-2429`), §7.1.6c-6
  (`:2763`), §7.1.6c-6d (`:2811-2825`), §7.5 (`:3630`), and `:8193`;
- `docs/RULES.md` rows 14 (`:404-408`), 32 (`:938-944`) and 33 (`:983-1066`);
- `docs/ARCHITECTURE.md` §5.3 row 20 (`:1239`), §6's `install_txn` row (`:1402`) and §9
  (`:1650-1684`);
- `docs/plans/design/os-signals-2026-10-11.md` (B5): the steward, the named doors, the
  first-`Armed` rule and `WatchClock`.

Platform documentation, named for the tickets' E-1 and E-2. This draft did not read it:
it relies only on the calls the codebase already makes.
- `ReplaceFileW`: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
- `renamex_np(2)`, `RENAME_SWAP` and `RENAME_EXCL`: the macOS `rename(2)` manual page.

---

## 12. Architecture impact (for the tickets' briefs)

- **The storage worker** (`SessionWriter`) gains `StorageJob::Document` and
  `StorageJob::Read`. After start, it is the one thread that reads and writes the four
  documents (A§5.1). Start-time reads stay where they are, before the loop exists.
- **`AppEvent`** gains `DocumentsAnswered`, which changes A§0.1's count.
- **New owned facts** (A§4.4). All of them are fields of existing App-owned stores:
  - each store's `Revision<T>` and in-flight generation. One writer: the store, on the
    window thread, from the worker's answers;
  - `StorageWatch`'s per-document mailbox. The watcher thread's listener writes it and
    the window thread drains it, the shape of B5's ears.

  The ownership census is regenerated.
- **One new platform door**, `bt_platform::replace_keeping_displaced`: on Windows,
  `ReplaceFileW` with the backup returned; on macOS, `renamex_np(RENAME_SWAP)`; refused
  elsewhere. It is worker-only, and a `worker-door-body` row of `# effects`.
- **`window_waits.tsv`**: row 20 is narrowed, and no row is added.
- **Rules text changed:**
  - R33, by T-STORE-MERGE;
  - §7.1.6c-6d's no-filtering sentence is superseded, by T-STORAGE-EAR-NAMED;
  - recorded as a departure, by T-STORE-CONDITIONAL-WRITE: "当场落盘" becomes "handed to
    the writer at once".
- **No timer anywhere.** The only clock is the existing `WatchClock`, and only a
  notification starts it (R31).
