# Hand-edited settings, keybindings, profiles and pins, re-read as a storage transaction: T-SETTINGS-REREAD (0.4.8 B6), design note, 2026-10-11

Status: **revision (b), 2026-10-11, after the Codex review** (`B6-settings-reread-design.review-codex.md`,
verdict *adopt with changes*, fifteen required changes). Revision (b) is the dated section at
the end. **Where revision (a)'s text and revision (b) differ, revision (b) stands**: in
particular §1.5's defect 1 and the failure card, §2.2's filter, §3.1's memory, §3.2's
fallback, `Displaced` row and receipts, §3.3's base, §3.4's four policies, §3.5's torn-write
claim, §3.6's wording, §4, §7, §8, §9 and §10 are replaced or amended by (b)1–(b)17.
**Revision (c)**, after the Codex confirmation round (`B6-settings-reread-design.review-codex-b.md`),
follows (b) and stands wherever the two differ: (c)1 replaces (b)3's revision bookkeeping,
(c)2 replaces (b)5's quit, (c)3 amends (b)16's J5 join, and (c)4 and (c)5 replace (b)17's
tickets and (b)18's questions. **Revision (d)**, after the scoped review of (c)
(`B6-settings-reread-design.review-codex-c.md`), closes its three blockers and stands over (c)
there: `kept` files follow derived versions (d)1, leaving without a change is a choice on the card
(d)2, and a forced stop's `Lost` is said through the system's message box (d)3.
Revision (a) was the draft for review. No ticket below has been dispatched. Plan row B6 ("hand-edited settings/keybindings/profiles/pins re-read — design
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

---

## Revision (b), 2026-10-11, after the Codex review

The review (`B6-settings-reread-design.review-codex.md`, in the 0.4.8 trace)
found "the right architecture" and four transaction holes: "unsafe non-exchange fallback,
incorrect `Displaced` merge base, discarded side-effectful receipts, and a quit path that
cannot process document answers before exit". It lists fifteen required changes. **All
fifteen are applied**; none is argued away. One finding outside the list (the pins
remove-then-re-pin case) is accepted as a stated limit rather than built, and is argued
under *Departures*. The J5 cross-reference the coordinator asked for is (b)16. The revised
tickets are (b)17, the revised owner questions (b)18.

Every fact the review cites was checked at `77399d57` before it was adopted:
- the dynamic write fault is never delivered: the five `take_fault()` calls are all at
  start (`main.rs:43955`, `:43956`, `:44166`, `:44193`, `:44208`), and nothing later moves a
  `DocumentWrites` fault to a card;
- the ordinary quit blocks in its write step (`main.rs:65789-65800`; only an update's quit
  writes by receipt, across turns);
- the terminal font is one door for three fields (`SettingChange::TerminalFont`,
  `settings_bundle.rs:85-89`);
- keybindings' duplicate ids resolve last-wins (`bt-persist/src/keybindings.rs:71-73`),
  profiles' first-wins (`ProfileFault::Duplicate`, `profiles.rs:2511-2514`);
- the macOS deployment minimum is 14.0 (`packaging/macos/Info.plist.in:88-89`);
- `read_with_fallback_bytes` returns no bytes for a refused document (`migrate.rs:1538-1540`).

### The change table

| # | the review's words | disposition | where |
|---|---|---|---|
| 1 | "Replace every plain-rename-after-precheck fallback with a refusal that leaves the edit owed, and add Linux `renameat2(RENAME_EXCHANGE)`." | applied | (b)1 |
| 2 | "Define the exchange door's metadata, fresh-backup collision, partial-failure, link, and directory-durability contracts per platform." | applied | (b)2 |
| 3 | "Make `Displaced` merge against the pre-exchange revision, not the document Folio just placed at the target." | applied | (b)3 |
| 4 | "Serialize/coalesce to one in-flight write per document or process every receipt in order; never drop a receipt carrying displaced bytes or cleanup obligations." | applied, both halves | (b)4 |
| 5 | "Redesign quit as a bounded terminal document-and-session transaction that finishes merge/sidecar decisions before exit." | applied (the multi-turn form) | (b)5 |
| 6 | "Define owed state as `writer_of_record && desired_persisted_document != revision.document`, keeping the persisted desired document separate from runtime projection." | applied | (b)6 |
| 7 | "Route asynchronous refusals into the person-visible failure-card path and add a regression test proving post-start faults are consumed." | applied | (b)7 |
| 8 | "Complete the settings, keybindings, profiles, and pins merge units and deterministic ordering rules, including every envelope field and duplicate identity." | applied | (b)8 |
| 9 | "Reframe Q5 so following link targets may remain out of scope, but observed symlinks and multi-link files are refused rather than replaced; include hard links explicitly." | applied | (b)2, (b)18 Q5 |
| 10 | "Add stable-read handling for in-place/chunked writers and platform-equivalent filename matching to the watch design." | applied | (b)9 |
| 11 | "Replace the 'few kilobytes' memory claim with the 16 MiB-per-document bound and shared byte ownership." | applied | (b)10 |
| 12 | "Shorten and bound the conflict notice, leaving complete unit details in diagnostics." | applied | (b)11 |
| 13 | "Expand E-1–E-3 and the mutation suite with the platform, receipt, quit, link, metadata, and valid-intermediate cases listed above." | applied | (b)13, (b)14 |
| 14 | "Re-estimate T-STORE-CONDITIONAL-WRITE and T-STORE-MERGE as L or split each into independently gated tickets." | applied (split) | (b)17 |
| 15 | "Enumerate the exact R33, ARCHITECTURE §9, DESIGN §7.1.6c-2/c-2′/c-6d, `settings_bundle`, storage-watch, and window-waits edits." | applied | (b)15 |
| — | findings §1: soften defect 1's trigger; the five-failure card is not delivered; the race after a precheck | applied | (b)12 |
| — | findings §4, pins: "A remove followed by re-pin … cannot be detected by a state-only merge" | accepted as a limit, not built | *Departures* |
| — | coordinator: the J5 save lane (S-6, T-DOCUMENT-SAVE-LANE) | stated | (b)16 |

### (b)1 · Refuse rather than replace; Linux exchanges too: adopted

§3.2's paragraph "A volume that cannot exchange" is **withdrawn**. The review: "it reopens
the exact clobber race the design promises to remove. It also contradicts §12". The rule is
now one sentence: **where the exchange is unavailable, the write is refused, the target is
not touched, and the edit stays owed.**

- The door answers `Refused(ExchangeUnavailable)` when the platform call says the volume
  cannot (Windows: `volume_cannot_replace`'s codes, `file_replace.rs:261-275`; macOS:
  `ENOTSUP`/`EINVAL` from `renamex_np`, and a volume whose `volumeSupportsSwapRenaming` is
  false; Linux: `EINVAL`, `ENOSYS`, `EXDEV`).
- `Landed { verified: false }` no longer exists. Every `Landed` is verified.
- The refusal is a write failure and reaches the person through (b)7's card, naming the
  file. Such a volume therefore cannot save these four files: that is a behaviour change
  for anyone whose data directory sits on one, and it is (b)18's Q5.
- **Linux**: `renameat2(temp, target, RENAME_EXCHANGE)`, which "atomically exchanges two
  existing paths" (Linux 3.15). The portable arm stops refusing on Linux and refuses
  elsewhere. Linux still has no ear (§2.2), so there the precondition and the exchange
  protect the file, and the documents are followed only at start.

### (b)2 · The exchange door's contracts, per platform: adopted

`bt_platform::replace_keeping_displaced(temp, target) -> Result<Displaced, ExchangeRefusal>`,
worker-only. It answers where the displaced bytes now are; it never deletes them (the
caller does, (b)4). Per platform:

| contract | Windows (`ReplaceFileW`) | macOS (`renamex_np(RENAME_SWAP)`) | Linux (`renameat2(RENAME_EXCHANGE)`) |
|---|---|---|---|
| **metadata** | merged by the call (DACL, streams, creation time, compression, encryption); the attribute word staged on the temp, as today (`file_replace.rs:321-328`) | the swap exchanges entries only, so the temp **first** takes the target's mode, owner, ACL and xattrs through `bt_platform::carry_metadata` (`file_replace.rs:48-60`), born `OwnerOnly` (`atomic.rs:197-210`) | as macOS |
| **the backup / displaced name** | a fresh sibling `<name>.displaced-<pid>-<nanos>-<n>` (`temp_sibling_path`'s recipe, `atomic.rs:275-303`); asked absent first; **a backup name that exists is a refusal**, never passed to the call, because Microsoft does not specify the call's behaviour for one | none needed: after the swap the temp's name holds the displaced file | as macOS |
| **partial failure** | 1175, 1176 and 1177 go through the existing ladder, kept **inside** the door, not cited: 1177 renames the backup back (`file_replace.rs:335-339`, `:404-422`); 1175/1176 are asked of the filesystem (`target.exists() && !backup.exists()`, `:334`) before anything is cleaned; a refusal whose ladder cannot restore names where the bytes are, as today (`:418-421`) | the call is one operation: it happened or it did not | as macOS |
| **links** | **refused before the call**: a target that is a symlink (`symlink_metadata`) or has more than one name (`file_link_count`, `file_replace.rs:4-32`) answers `Refused(Link)`; nothing is written | same | same |
| **durability** | `REPLACEFILE_WRITE_THROUGH` is documented as unsupported, so after the call the door flushes the directory (`FlushFileBuffers` on a `FILE_FLAG_BACKUP_SEMANTICS` handle, `install_txn`'s step, `ARCHITECTURE.md:1402`); `Landed` means: temp `fsync`ed, exchanged, directory flushed | `F_FULLFSYNC` on the temp and on the directory's descriptor (`install_txn`'s step) | `fsync` of the temp and of the directory |

The first-write case (`expected: None`) keeps `rename_without_replacing`
(`bt-platform/src/lib.rs:2278-2306`) and the same link, metadata and durability rules.

### (b)3 · `Displaced` merges against the pre-exchange revision: adopted

The review: "That makes Folio's just-written document the base and can turn an O-only edit
into a file-side change." The `Displaced` row of §3.2's table becomes:

| answer | merge | next precondition |
|---|---|---|
| `Displaced { expected, written, theirs, kept }` | `B` = the document of `expected` (the revision the write was conditional on), `O` = the desired document (b)6, `T` = `theirs` | the bytes Folio just wrote (`written`) |

The revision is **not** moved to `written` before the merge. After the merge: revision ←
`(theirs bytes, T)` for the merge's purposes, and the next write is sent with `expected =
written`, because that is what the target holds. The receipt carries `expected` so the merge
never has to remember it. Test: (b)14 item 4.

### (b)4 · One write in flight per document; every receipt processed in order: adopted

Both halves of the review's either/or are taken, because they protect different things:

- **One in flight per document.** A store holds at most one `Document` job on the lane. A
  change while one is in flight replaces the store's *pending* desired document (one slot,
  newest wins) and is sent when the in-flight answer has been processed. This is J5's
  coalescing ("a coalesced request is **still completed**, as *superseded by n*"): the
  replaced pending document never reached the lane, so it has no receipt to lose.
- **Every receipt is processed, in worker order.** §3.2's sentence "an answer for a
  generation older than the store's newest send is dropped" and §5.3's "Stale never over
  fresh" bullet are **withdrawn for receipts**. Revision transitions, `kept` cleanup,
  displaced-byte merges and diagnostics are applied for every receipt. Generations remain
  only to keep a stale answer's *visible* effects (a card, a re-derivation) from landing
  after a newer one's — the review's words: "generation filtering may suppress stale UI
  effects, but never cleanup, revision transitions, displaced-byte merges, or
  diagnostics".
- `Read` answers are processed in order too; a `Read` answer older than a `Document`
  receipt for the same document is merged against the revision that receipt set, never
  against an older one.

### (b)5 · Quit is a bounded, multi-turn document-and-session transaction: adopted

§4.3's "the quit's existing wait covers them" is **withdrawn**. The review is right: "Ordinary
quit blocks inside `flush_judged()` waiting for the session receipt; it cannot process
`DocumentsAnswered` meanwhile."

Of the review's two forms, the **multi-turn quit state** is taken, because it is the shape
the update's quit already has (`writes_by_receipt`, `main.rs:65789-65800`, a clock checked
per turn, `quit::UPDATE_RECEIPT_DEADLINE`) and the shape J5 asks every document save's close
gate to have (J5 §6 Q5: "the gate waits for the owed receipt across turns … never a wait on
the thread"). A worker-owned terminal batch would put merge decisions on the worker, which
holds no desired document and no merge.

The quit gains one step before `Write`, **`Documents`**:
1. Each store with an owed desired document (b)6 sends it (or keeps its in-flight job).
2. The loop turns. Each answer is processed as in the running window — a `Moved` or
   `Displaced` is merged (pure: no door runs; the windows are leaving) and the merged
   document sent again — until every store has `Landed` or the deadline passes.
3. **The deadline is the existing budget, shared**: one `SESSION_SAVE_BUDGET` (3 s,
   `persist.rs:399`) from the start of `Documents` covers this step and the session write
   after it. No new wait, and no longer total.
4. At the deadline, or on a terminal refusal (`Refused`, or `Moved` against a file that will
   not parse), the store's desired document goes to the worker as a **best-effort sidecar**
   `<name>.unsaved-<stamp>` (create-new, temp `fsync`ed, directory flushed — "durable"
   includes the directory). Its own answer is waited for inside the same deadline; a sidecar
   the disk refuses is **said**: one `diagnostics.log` line naming the file, the units and
   the sidecar's failure. §4.3's "is kept beside its file" becomes "is kept beside its file
   when the disk allows, and said when it does not".
5. Then `Write`, as today, with what is left of the budget.

Tests: (b)14 items 6–7.

### (b)6 · Owed = `writer_of_record && desired != revision.document`; desired is not the projection: adopted

Each store now holds three things, not two:
- `revision` — the bytes and document on disk, as far as Folio knows (§3.1);
- `desired` — **the persisted document Folio wants on disk**;
- the runtime projection — what readers read (`loaded()` for settings and pins, the live
  table for keybindings and profiles), which is what doors produce.

**Owed** ⇔ `writer_of_record && desired != revision.document`. A non-writer owes nothing
(§4.2 and §4.1 now agree).

How each is moved:
- **A press** applies to `desired` exactly the units in which it changed the projection —
  the unit delta of the press, not the whole projection. So a value the file carries and a
  door refused or normalised (an unknown keybinding id, a font this machine lacks) stays in
  `desired` as the file said it, and is never written out as a change.
- **A merge** sets `desired` to the merged document (§3.3), then runs the doors from the
  projection to that document (b)8. A clean file-only merge therefore leaves `desired ==
  revision.document`: no debt.
- **A landed write** moves the revision; `desired` is untouched.

§3.3's step 3 ("The door's `store` makes that unit equal to the new base") is replaced by
this. While a merge applies its doors, a door's `store` updates the projection only.

### (b)7 · Asynchronous refusals reach the failure card: adopted

The review: "the four stores' `take_fault()` calls are consumed during startup … I found no
later transfer of a dynamic write fault to those card fields." Confirmed (the five call sites
above). Revision (a)'s "five refusals in a row raise the card once" (§1.5, §4.1) was a reading
of `DocumentWrites` alone; operationally **no write-failure card is raised after start
today**. That is a fifth present defect (b)12.

The `DocumentsAnswered` handler, after draining every answer, asks each store's
`take_fault()` and raises what it returns through the toast door the start-up announcement
uses (`announce_persistence_faults`'s card). A `Refused(ExchangeUnavailable)` or
`Refused(Link)` is a fault at its **first** occurrence (it will not change by retrying); a
disk refusal keeps the five-in-a-row streak (`persist.rs:56`). Regression test: (b)14 item 8,
"a post-start fault is consumed".

### (b)8 · The four policies, completed: adopted

**One rule for all four first:** *what the file says and the reader does not put in force is
carried, verbatim, in the file's position, and never rewritten by Folio* — an unknown
keybinding id, an earlier duplicate keybinding line, a later duplicate profile id, a later
duplicate pin, an unknown pin kind. It lives in `desired` (b)6, not in the projection, so
it is neither applied nor owed. Each document's envelope: `schema_version` is always this
build's current version (a newer file is refused before any merge, S§1.3 rule 2).

**`settings.json`.**
- Units: every field of `SettingsV1`, destructured with no `..` (as `plan_settings`,
  `settings_bundle.rs:260-318`), merged field by field into the **merged document first**.
- **The doors are then derived from the runtime state to the merged document**:
  `plan_settings(projection, merged, platform)`. This is the review's fix for the font: if
  Folio changed the family and the file changed only the size, the merged document has
  Folio's family and the file's size, and the one `TerminalFont` door
  (`settings_bundle.rs:85-89`) is called with both.
- The three machine receipts: as revision (a) §3.4.

**`keybindings.json`.**
- Envelope: `writing_platform` of the result is this platform (`keybindings_document`,
  `persist.rs:1894-1903`). Before merging, `B`, `O` and `T` are each normalised into this
  platform's dialect with `translate_command_modifier` from their own `writing_platform`
  (as `import_shortcuts` does, `settings_bundle.rs:517-544`); a line that cannot be
  translated is carried (the one rule) and named.
- Duplicate actions: each side is read last-wins (the reader's rule,
  `bt-persist/src/keybindings.rs:71-73`) into one departure per action for the merge; the
  shadowed earlier lines of `T` are carried.
- Unknown actions: carried.
- Units: one per action id; then chord collisions, file's action keeps the chord (revision
  (a)).
- **Output order** (deterministic): the merged result's lines in `T`'s order for actions `T`
  names; then actions only Folio departs on, in the shortcut table's own order
  (`Shortcuts::overrides`' walk, `runtime/keyboard.rs:262-272`); carried lines in their file
  positions.

**`profiles.json`.**
- Duplicate ids: each side is read first-wins, the installer's rule (`profiles.rs:2511-2514`);
  later duplicates in `T` are carried.
- Units: rows by id, fields within a row (revision (a)).
- **Order, total**: (1) the common rows' relative order: `T`'s if `T` reordered them against
  `B`, else `O`'s if `O` did, else `B`'s; (2) each added row (on either side) is placed
  immediately after its nearest predecessor *in its own side's sequence* that is present in
  the result, or first if it has none; (3) two rows anchored to the same predecessor: `T`'s
  before `O`'s, each side's in its own order. This places a file-only addition when only
  Folio reordered, which revision (a) did not.
- Shipped ids the file never names are appended by the installer as today; they are not
  merge units.

**`pins.json`.**
- Duplicate identities: each side is read first-wins (the menus' `row_of`, `pins.rs:108-113`);
  later duplicates in `T` are carried.
- **Units and the contradiction resolved.** Per identity, with presence `p` and opaque
  `extra` `e` on each side:
  - presence: if `pO == pB`, take `T`'s presence; if `pT == pB`, take `O`'s; if both differ
    from `B` they agree (both removed, or both added);
  - **remove against change is a conflict, and the file wins**: Folio removed and the file
    changed `e` → kept, with `T`'s `e`; the file removed and Folio changed `e` → removed.
    Revision (a)'s bullet "a row removed on either side is removed" now reads "… unless the
    other side changed that row, which is a conflict the file wins";
  - `e` when present on both: three-way per key of `e`; file wins a key changed on both.
- **Top-level `PinsV1.extra`** (`bt-persist/src/pins.rs:54-58`): three-way per key; file wins
  a key changed on both.
- Order: as revision (a) (file order, then Folio's additions appended).

### (b)9 · Stable reads, and names as the volume compares them: adopted

**Stable read.** The `Read` job, and step 1 of every `Document` job, read through one
function: `metadata` (length, modified time, file id) → bounded read → `metadata` again. A
mismatch answers `MovedDuringRead` and nothing is merged; the ear has (or will have) a stamp
for that document, so the next pass reads again. An in-place writer that pauses longer than
the quiet window between two **valid** intermediate documents is indistinguishable from two
saves, and is treated as two: the first is merged, the second merged after it. Because every
write is conditional, that costs at most one extra merge and a flicker, never a loss. §3.5's
sentence "for these four documents that read is always refused" is **narrowed** to: a torn
*prefix* is always refused; a valid intermediate document is merged as a save, and the
document it is followed by is merged next.

**Names.** The filter compares case-insensitively (ASCII folding: the four names are ASCII)
on every platform. On a case-sensitive volume an extra stamp for a different file costs one
read that finds the revision's bytes and does nothing; a missed stamp would cost a hand
edit. Windows may also report a short (8.3) name; a name containing `~` whose extension
begins `.JSO` (case-folded) stamps all four. E-3 measures which names arrive.

### (b)10 · Memory: the real bound, shared ownership: adopted

§3.1's "four documents of a few kilobytes each" is **withdrawn**. Each document is bounded at
16 MiB (`MAX_DOCUMENT_BYTES`, `migrate.rs:1446`). Revision, in-flight and pending bytes are
`Arc<[u8]>`, so one buffer is shared by the revision, the job and its receipt. Worst case per
document: the revision's bytes, one in-flight buffer, one displaced buffer in a receipt (each
≤ 16 MiB) and their parsed documents — about 48 MiB of bytes per document, about 192 MiB for
all four, in a pathological run; the ordinary case is a few kilobytes each. Startup's refused
bytes need a new read outcome: `read_with_fallback_bytes` gains `Refused { bytes }` for a
document refused with its bytes in hand (`migrate.rs:1538-1540` today returns none).

### (b)11 · The conflict card, shorter and bounded: adopted

The review's form, adopted: **"`settings.json`: Theme and Font size came from disk."** One line
per file, at most three names per line, then "and N more" (`// zh: pending`). One card per pass
as before, at most four lines. The full list of units, with both values, goes to
`diagnostics.log` (§3.6's merge line gains the names).

### (b)12 · Corrections to §1.5: adopted

- **Defect 1's trigger is softened.** "The failed write's own temporary file is enough to wake
  the watch" holds when the failure comes after the temporary file was made; a failure at
  serialisation, open or temp creation makes no event. The loss happens on the next pass,
  however triggered (a `session.json` flush is enough today, since the watch is a tree).
- **Defect 2 is wider than the quiet window.** None of the four stores checks the disk before
  writing, so a hand edit landing at any moment before Folio's rename is overwritten — for all
  four files, not only profiles and pins.
- **Defect 5 (new): no write-failure card after start** (b)7.

### (b)13 · Experiments, expanded: adopted

- **E-1 (Windows)** adds: a backup name that already exists (what the call does — the door
  never relies on it, (b)2); metadata, streams and ACL after the exchange; driving 1175, 1176
  and 1177 through the existing injected-operation seam (`replace_file_preserving_with`,
  `file_replace.rs:311-318`); the directory flush after the call.
- **E-2 (macOS)** adds: `volumeSupportsSwapRenaming` against the call's answer on APFS, HFS+
  and SMB; mode, owner, ACL and xattrs after `carry_metadata` + swap.
- **E-3 (both)** adds: case-only names; 8.3 names on Windows; a symlinked and a hard-linked
  document; an in-place, chunked writer whose intermediate states are valid JSON.
- **E-5 (Linux, new)**: `renameat2(RENAME_EXCHANGE)` on ext4 (required), and the errno on an
  unsupported volume (a FUSE or network mount).
- E-4: unchanged.

### (b)14 · Tests, added (each with its mutation)

1. `bt-platform` `exchange_tests::an_unsupported_exchange_refuses_and_leaves_the_target`. MUTATION: fall back to a plain rename.
2. `exchange_tests::linux_rename_exchange_hands_back_the_displaced_file` (Linux CI). MUTATION: `RENAME_NOREPLACE` in place of `RENAME_EXCHANGE`.
3. `exchange_tests::replace_file_partial_states_1175_1176_1177_keep_the_document` and `…::an_existing_backup_name_is_refused` (Windows, injected operations). MUTATIONS: drop the 1177 restore / pass an existing backup name to the call.
4. `persist::conditional_write_tests::displaced_merges_against_the_pre_exchange_revision` — Folio changed field A only, the crossing edit changed field B; result keeps both and names no conflict. MUTATION: set the revision to `written` before the merge.
5. `…::a_stale_receipt_with_displaced_bytes_is_never_dropped`. MUTATION: filter receipts by generation.
6. `quit_tests::a_moved_or_displaced_answer_at_quit_is_merged_and_written_before_the_session` (fake disk, fake clock). MUTATION: send the session job without the `Documents` step.
7. `quit_tests::a_document_refused_at_quit_is_kept_or_said` — the sidecar lands; and, with the disk refusing the sidecar too, one diagnostics line. MUTATIONS: skip the sidecar / swallow its failure.
8. `app_diagnostics_tests::a_write_fault_after_start_reaches_a_card`. MUTATION: never call `take_fault` after start (today's code).
9. `settings_merge::tests::folios_font_family_and_the_files_font_size_both_survive`. MUTATION: build the door change from `T`.
10. `keybindings_merge::tests::a_file_written_on_another_platform_merges_in_this_dialect` and `…::duplicate_and_unknown_lines_are_carried`. MUTATIONS: skip the translation / drop carried lines.
11. `profiles_merge::tests::a_file_only_row_is_placed_when_only_folio_reordered` and `…::a_later_duplicate_id_is_carried`. MUTATIONS: revision (a)'s order rule / drop the duplicate.
12. `pins::tests::top_level_extra_merges_by_key`, `…::remove_against_change_goes_to_the_file`, `…::a_later_duplicate_pin_is_carried`. MUTATIONS: take `O`'s top-level `extra` / removal always wins / collapse duplicates.
13. `exchange_tests::a_symlinked_or_hard_linked_document_is_refused_not_replaced`. MUTATION: drop the link check.
14. `storage_watch::tests::case_equivalent_and_short_names_stamp_the_document`. MUTATION: exact-case comparison.
15. `persist::stable_read_tests::a_valid_intermediate_document_is_merged_then_the_final_one` and `…::a_read_that_raced_a_writer_is_not_merged`. MUTATIONS: no second `metadata` / merge a `MovedDuringRead`.
16. `persist::revision_tests::identical_bytes_and_a_return_to_old_bytes_are_no_change`. MUTATION: compare modified times instead of bytes.
17. `persist::owed_tests::a_value_a_door_refused_is_not_written_back` (an unknown keybinding id from the file). MUTATION: derive `desired` from the projection.

### (b)15 · The exact text edits, enumerated: adopted

DESIGN.md is append-only (CONVENTIONS; `_codex-rules.md`): each change below to an earlier
entry is made by a **new dated entry** that names the entry it supersedes; no earlier entry is
edited.

| file:line | today's words | becomes | ticket |
|---|---|---|---|
| `RULES.md:990` (R33) | "applied in process at its own door and **not re-read from disk during a run**" | "applied in process at its own door, and re-read from disk when the storage watch hears it (§ this note)" | T-STORE-MERGE-SETTINGS |
| `RULES.md:1030-1034` (R33) | "The contrast that matters: `profiles.json` and the pins are watched live … while `settings.json` is not. Two reload disciplines already" | one sentence: the four documents are followed live by one discipline (named watch, revision, conditional write, merge) | T-STORE-MERGE-SETTINGS |
| `RULES.md:1053-1054` (R33) | "never by writing `settings.json` and waiting" | unchanged in meaning (an import still goes through the doors); add: "a hand edit enters by the same doors" | T-STORE-MERGE-SETTINGS |
| `ARCHITECTURE.md:1659` (§9 table) | "two disciplines … `settings.json` is applied in process … and is not re-read from disk during a run" | one discipline for all four | T-STORE-MERGE-SETTINGS |
| `ARCHITECTURE.md:1669-1671` (§9) | "… each row's own apply function, never through a re-read" | "… each row's own apply function, which a re-read also uses" | T-STORE-MERGE-SETTINGS |
| `ARCHITECTURE.md:1239`, `window_waits.tsv:74` (row 20) | "settings/keybindings/profiles writes" | removed from row 20's call cell; disposition notes the lane | T-DOCUMENT-SAVE-LANE |
| DESIGN §7.1.6c-2 (`DESIGN.md:2418`) | "每一次点击当场落盘" | new entry: a press is handed to the storage lane at once and lands by receipt | T-DOCUMENT-SAVE-LANE |
| DESIGN §7.1.6c-2′ / `DESIGN.md:8193` | "`store_keybindings` 当场落盘 … 它存的东西现在没有一刻不在磁盘上" | new entry: the table is live at once; the file follows by receipt | T-DOCUMENT-SAVE-LANE |
| DESIGN §7.1.6c-6d (`DESIGN.md:2815`) | "不在通知那一端过滤" and the tree watch | new entry: a named shallow watch, names filtered on the watcher thread | T-STORAGE-EAR-NAMED |
| DESIGN §7.1.6c-6d (`DESIGN.md:2811`, scope) | `profiles.json` (and later pins) followed live | new entry: all four followed live, by merge | T-STORE-MERGE-SETTINGS |
| DESIGN §7.1.6c-6d (`DESIGN.md:2817`, mid-run broken file) | "一个字都不取 … + 一张卡" | unchanged; extended to settings and keybindings; no `.rejected-` copy mid-run | T-DOCUMENT-SAVE-LANE |
| `settings_bundle.rs:28-29` | "Never by writing `settings.json` and waiting — nothing re-reads that file during a run." | "Never by writing `settings.json` and waiting for the re-read: an import uses the doors directly." | T-STORE-MERGE-SETTINGS |
| `storage_watch.rs:1-52` (header; `:33-45` the filter paragraph, `:47-52` the meaning) | tree watch, nameless, two files | named shallow watch, four documents, the stamp rule | T-STORAGE-EAR-NAMED |
| `main.rs:715-729` (`AppEvent::StorageChanged` doc), `main.rs:12712-12725` (`App::storage_watch` doc), `main.rs:48484-48500` (`advance_storage_watch` doc) | "both files … re-reading each and comparing" | the four documents, by stamp | T-STORAGE-EAR-NAMED |
| `persist.rs:1995-2018`, `pins.rs:209-215` (`reread` docs) | "a document identical to the one in force is `Unchanged`" | compared with the revision's bytes | T-STORE-REVISION |
| `CHANGELOG.md` `## Unreleased` | — | revision (a)'s line | T-STORE-MERGE-SETTINGS |

### (b)16 · The J5 save lane: this protocol **is** that lane's protocol

J5's ticket S-6, T-DOCUMENT-SAVE-LANE (`side-effects-contract-2026-10-11.md:240`), asks for a
document's save as "a request on a storage lane, `save(revision)` → `Saved { revision }` /
`Conflict` / `Failed`", with "B6's settings stores join[ing] the same lane when B6 is built".
**The conditional write of §3.2 as revised by (b)1–(b)5 is that lane's protocol.** The
mapping:

| J5 | this note |
|---|---|
| `save(revision)` | `Document { expected: Option<Arc<[u8]>>, bytes, mode, link_policy }` — `expected` is the disk revision as exact bytes |
| `Saved { revision }` | `Landed { bytes }` — the new disk revision is the bytes written |
| `Conflict` | `Moved { theirs }` (the precondition saw another file; nothing written) and `Displaced { expected, written, theirs, kept }` (another file crossed the exchange) |
| `Failed` | `Refused { cause }`, with `ExchangeUnavailable`, `Link` and the disk's own errors |
| "serialised per document", "a coalesced request is still completed, as *superseded by n*" | (b)4: one in flight per document, a pending slot, every receipt processed |
| §6 Q5, the close gate waits across turns | (b)5: the quit's `Documents` step, across turns, inside the shared budget |

**What the join costs this design** — nothing in the protocol; four parameters and one
scheduling dependency:

1. **A mode per request.** A preview buffer has no merger, so a `Displaced` answer for it
   cannot be merged. The lane takes `mode: Merge | Restore`. In `Restore` the worker, on
   finding displaced bytes that are not `expected`, exchanges again to put them back
   (conditional on the target still holding `written`) and answers `Conflict` with nothing
   lost; if that second exchange is itself crossed, both files are kept and named. The four
   stores use `Merge`; the preview uses `Restore`, which also closes the race its mtime check
   leaves open today (`preview.rs:5785-5791`). The four stores also use `Restore` between the
   lane's ticket and the merge tickets (b)17, which is what lets those land separately.
2. **A link policy per request.** The four documents refuse links (b)2. The preview today
   writes a linked file with the plain writer, a gap "nobody has ruled" (`preview.rs:5766-5773`);
   that ruling is not this note's. The lane takes `link_policy: Refuse | PlainWriter`, the
   preview keeps `PlainWriter` until it is ruled, and J5 owns that question.
3. **Two revisions for a buffer.** J5's dirty flag follows the *body* revision (the undo log's,
   `preview.rs:5794-5800`); the lane's precondition is the *disk* revision. A preview request
   carries both; the four stores need only the disk one. The preview's disk revision becomes
   exact bytes instead of a modified time (`preview.rs:5785`), which costs one encoded copy of
   the file per open buffer, bounded by the preview's own read cap.
4. **Metadata.** The preview's save already keeps metadata through `ReplaceFileW`
   (`atomic.rs:150-164`); (b)2's exchange door keeps the same on Windows and adds the carry on
   macOS and Linux, so the preview loses nothing by moving onto it.
5. **Scheduling.** S-6 depends on J5's S-3 and OC O-7 (`side-effects-contract-2026-10-11.md:240`).
   The lane is therefore **one ticket, jointly owned** ((b)17 ticket 4): if S-3 is not ready, the
   lane ships with the four stores and the preview joins under S-6 with no protocol change.
   Head-of-line: one worker means a large preview save on a slow share delays a settings write
   and the session autosave. Nothing in (b)4–(b)5 relies on one thread any longer (the quit no
   longer relies on FIFO), so the lane may later serialise per document on more than one worker
   without changing the protocol; this note does not split it.

### (b)17 · The tickets, revised (split, independently gated)

| # | ticket | size | after | the work | tests |
|---|---|---|---|---|---|
| 1 | T-STORAGE-EAR-NAMED | S | — (E-3) | §2.2 with (b)9's names; the edits of (b)15 for the watch | 1; the two of §9.1; (b)14-14 |
| 2 | T-STORE-REVISION | S | 1 | §3.1 with `Arc<[u8]>` and the refused-bytes outcome (b)10; profiles/pins compare against the revision | 2; §9.2's; (b)14-16 |
| 3 | T-EXCHANGE-DOOR | M | — (E-1, E-2, E-5) | (b)1–(b)2: `replace_keeping_displaced` on three platforms, its refusals, metadata, links, durability | 7; (b)14-1, -2, -3, -13 |
| 4 | **T-DOCUMENT-SAVE-LANE** (= J5 S-6, jointly owned) | M | 2, 3 | the lane (b)16; `Document`/`Read` jobs with stable reads (b)9; one in flight, every receipt (b)4; `desired` and owed (b)6; `Restore` mode for the four stores until 6; faults to cards (b)7; no mid-run `.rejected-`; row 20 | 2's in-flight variant, 5, 8, 9, 10, 17, 18, 19; (b)14-5, -8, -15, -17 |
| 5 | T-QUIT-DOCUMENTS | M | 4 | (b)5: the `Documents` quit step, the shared budget, the sidecar | 4; (b)14-6, -7 |
| 6 | T-STORE-MERGE-SETTINGS | M | 4 | §3.3 with (b)3 and (b)6; the one carried-units rule; settings by (b)8; `Merge` mode; settings and keybindings join the pass; R33, A§9 and the rest of (b)15 | 3, 15, 16; (b)14-4, -9 |
| 7 | T-STORE-MERGE-KEYBINDINGS | S | 6 | (b)8 keybindings | 14; (b)14-10 |
| 8 | T-STORE-MERGE-PROFILES-PINS | M | 6 | (b)8 profiles and pins | 11, 12, 13; (b)14-11, -12 |
| 9 | T-STORE-NOTICE | S | 7, 8 | (b)11's card; the two "kept" sentences; diagnostics | 6; §9.5's |

Ticket 5 may run beside 6–8; 7 and 8 beside each other. Every ticket is S or M; none is L.

### (b)18 · The owner questions, revised (five)

**Q1. When a value was changed both in Folio (not yet saved) and in the file, does the file's
stand?** Unchanged from revision (a), with one addition: a pin unpinned and pinned again in
Folio looks untouched to the merge, so a removal of that pin in the file stands.
**Recommendation: yes, the file wins every conflict, and one short card names what came from
disk.**

**Q2. Should a hand edit that conflicts with nothing take effect silently?** Unchanged.
**Recommendation: yes.**

**Q3. Should `settings.json` and `keybindings.json` be followed while Folio runs?** Unchanged.
**Recommendation: yes.**

**Q4. When you quit, a change Folio could not write is kept as a copy beside the file when the
disk allows, and named in `diagnostics.log` when it does not. Keep it?** **Recommendation:
yes.**

**Q5 (reframed). When Folio cannot replace one of these files safely, should it refuse and say
so rather than overwrite?** This covers three cases: the file is a symbolic link, the file has
a second name (a hard link — a dotfiles tree that links it in), or the volume cannot exchange
two files in one step (some network shares). In each, Folio writes nothing to that file, keeps
the change for the run, and raises the "could not be saved" card naming the file. Following a
link to its target stays out of scope. **Recommendation: yes.** Today a save replaces a link
with a plain file, splits a hard link, and on such a volume can overwrite a hand edit; each is
the silent loss this design exists to end.

### Departures

- **Pins: a remove followed by a re-pin in Folio is not detected.** The review: "A remove
  followed by re-pin that returns `O` to the same state as `B` cannot be detected by a
  state-only merge; the file removal wins unless the design introduces an operation log." Not
  built, and argued: the state Folio holds after an unpin and a re-pin is the state it held
  before both, and the merge is a merge of states by design (§3.3); an operation log for one
  document would be a second record of what the pin table already says, with the drift that
  §4.1 avoids by deriving "owed" from documents. The case is named in Q1 so the owner rules on
  it with the rest of the conflict policy.
- **Revision (a)'s "§3.2 a volume that cannot exchange" is not argued; it is withdrawn** (b)1.

### Sources added

- The review: `B6-settings-reread-design.review-codex.md` (the 0.4.8 trace, 2026-10-11).
- J5: `docs/plans/design/side-effects-contract-2026-10-11.md` (on `j5-contracts`), §2.3 V1,
  §4 "A save the agent and the person both asked for", §5 S-6, §6 Q5.
- `ReplaceFileW`: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
  (backup name, error codes 1175–1177, `REPLACEFILE_WRITE_THROUGH` unsupported), as the review
  reports it; E-1 confirms.
- `volumeSupportsSwapRenaming`: https://developer.apple.com/documentation/foundation/urlresourcevalues/volumesupportsswaprenaming
- `renameat2(2)`: https://man7.org/linux/man-pages/man2/renameat2.2.html
- Folio at `77399d57`: `main.rs:43955-44208` (the five `take_fault` calls), `main.rs:65789-65800`,
  `settings_bundle.rs:85-89`, `bt-persist/src/keybindings.rs:71-73`, `profiles.rs:2511-2514`,
  `migrate.rs:1538-1540`, `preview.rs:5766-5801`, `packaging/macos/Info.plist.in:88-89`.

---

## Revision (c), 2026-10-11, after the Codex confirmation round

The confirmation (`B6-settings-reread-design.review-codex-b.md`, in the 0.4.8 trace) adopts
revision (b)'s items 1–2, 4, 6–13 and 15. It finds the pins departure acceptable "if Q1
adopts it" and the refusal on volumes that cannot exchange correct. It finds the two preview
revisions and the stated head-of-line cost "sound in principle". It leaves **three blockers**.
Blockers 1 and 2 are applied as stated. Blocker 3 was overtaken, before this revision was
finished, by the confirmation on J5's own revision (b), which ruled the other way; the
coordinator adopted that ruling for both notes, and (c)3 applies it (see *Departures*).
**Where revision (b) and revision (c) differ, revision (c) stands**: (c)1 replaces (b)3's
revision bookkeeping, (c)2 replaces (b)5, (c)3 amends (b)16, (c)4 replaces (b)17, and (c)5
replaces (b)18. Everything else in (b) stands.

### The change table

| # | the review's words | disposition | where |
|---|---|---|---|
| 1 | "After `Displaced`, revision (b) incorrectly moves the revision to `theirs` although the target contains `written`, so a subsequent `Moved` can merge against the wrong base and discard a change recovered from the first displacement" | applied: after a landed exchange the revision is `written`; the displaced bytes are merged as `T` against the pre-exchange revision; the merged result becomes `desired`, owed when it differs from `written` | (c)1 |
| 2 | "The quit design submits its sidecar at an already-expired deadline … timeout must instead cancel quit or reserve a realizable recovery phase" | applied, both: a documents phase with its own sub-budget, a recovery phase that writes the copy **first** with the time that is left, the session write only behind a landed copy, and **the quit cancelled** when either phase runs out — J5 (b)8's rule | (c)2 |
| 3 | "The claimed J5 join is not the current J5 contract: J5 revision (b) specifies one worker with two protocols, the preview's existing baseline semantics, explicit quit cancellation on timeout, and S-6a/b/c" | **superseded by the coordinator's ruling** after J5's own confirmation (`J5-05-contracts.review-codex-b.md` item 3: "B6 is right: the conditional exchange is the lane protocol"): one worker, **one** request protocol (this note's, with `Merge \| Restore` and a link policy), **two** semantic policies (the stores merge; the preview reports `Conflict` and keeps its body revision); no `StorageJob::Save`; ticket 4 stays the shared request ticket, jointly owned with S-6a; what the preview's policy needs from it is listed | (c)3, *Departures* |
| — | consequences: the ticket table and the owner questions | revised | (c)4, (c)5 |

### (c)1 · After an exchange, the revision is what the target holds: adopted

Revision (b) said, after a `Displaced` answer: "revision ← `(theirs bytes, T)` for the merge's
purposes, and the next write is sent with `expected = written`". The two halves disagree: the
revision named `theirs` while the target held `written`. A later `Moved` then merged against
`theirs`, so a unit recovered from the displaced file and kept in `desired` looked unchanged
against that base, and the file's next change to it was taken as the only change. That
discards what the first merge recovered.

**The bookkeeping, restated as one invariant: the revision is always the bytes Folio last
read from or placed at the target, and the document they parse to.** On each answer:

| answer | revision ← | merge | `desired` ← | owed when |
|---|---|---|---|---|
| `Landed { bytes }` | `(bytes, document sent)` | none | unchanged | `desired != revision.document` (a press while in flight) |
| `Moved { theirs }` | `(theirs, T)`, **after** the merge | `B` = current revision's document, `O` = `desired`, `T` = `theirs` | the merged document `M` | `M != T` |
| `Displaced { expected, written, theirs, kept }` | **`(written, the document written)`** | `B` = the document of `expected` (the pre-exchange revision, (b)3), `O` = the document written, `T` = `theirs` | the merged document `M` | `M != written` |
| `Refused` | unchanged | none | unchanged | as before |

Two details make the `Displaced` row right:
- **`O` is the document written, not the current `desired`.** The exchange put `written` on
  disk and handed back what it replaced; the merge reconciles exactly those two against the
  base both were derived from. A press that arrived after the write was sent has already moved
  `desired` by its unit delta ((b)6); that delta is re-applied to `M` (the same unit-delta rule a
  press uses), so it is neither lost nor counted twice.
- **`kept` is removed only when a write whose `expected` is `written` lands** — that is, when the
  merged document, which contains the displaced units, is on disk. Until then the displaced
  bytes stay on disk under `kept` ((b)2's naming), so a crash in between loses nothing.

A second `Displaced` before the first's merge lands is the same row again: its `expected` is
`written₁`, its base is `written₁`'s document, and its `O` is `written₂`'s. Each displaced file
is merged against the revision its own exchange was conditional on, and `kept₁` is removed only
after a document derived from it lands.

Tests (replacing (b)14 item 4's mutation and adding one):
- `persist::conditional_write_tests::displaced_merges_against_the_pre_exchange_revision`.
  MUTATION: set the revision to `theirs` (revision (b)'s text).
- `…::a_unit_recovered_from_a_displaced_file_survives_a_later_moved`. Folio writes A; a crossing
  edit sets B (displaced, recovered into `desired`); the file then changes C; the merge keeps A,
  B and C. MUTATION: revision (b)'s bookkeeping (revision ← `theirs` after `Displaced`).
- `…::a_kept_file_is_removed_only_after_a_document_derived_from_it_lands`. MUTATION: remove
  `kept` on the `Displaced` answer.

### (c)2 · Quit: a documents phase, a recovery phase, and cancellation: adopted

Revision (b)'s step 4 issued the copy "at the deadline", on the same worker, possibly behind the
job that used the deadline up. Neither the copy's receipt nor the session write then had any
time, and a change held only in memory could vanish on exit. Revision (c) replaces (b)5's steps
2–5 with two phases inside the one budget, and the rule J5 (b)8 chose: **a quit that cannot
keep the person's change is cancelled, not completed.**

The budget stays `SESSION_SAVE_BUDGET` (3 s, `persist.rs:399`), measured from the start of the
documents phase, and is split by two named constants (S-6c's quit uses the same two; (c)3):

1. **Documents phase** — until `QUIT_DOCUMENTS_BY` (start + 1.5 s). Each owed store sends its
   desired document; each answer is processed as in the running window ((c)1; merges are pure,
   no door runs). The phase ends when every store has landed or has a **terminal answer**:
   `Refused` (disk, `Link`, `ExchangeUnavailable`) or `Moved` against a file that will not parse.
2. **Recovery phase** — only for stores with a terminal answer, from the end of phase 1 until
   `QUIT_RECOVERY_BY` (start + 2.5 s). Each such store's desired document is written **first**
   into the data directory's `recovered` folder (`preview::RECOVERED_FOLDER`, `preview.rs:7059`;
   T-RECOVERED-FOLDER's folder, the one `keep_dirty` writes on a forced stop,
   `main.rs:66335-66348`), create-new, the file `fsync`ed and the directory flushed. Revision
   (b)'s `<name>.unsaved-<stamp>` beside the file is **withdrawn**: one place for kept changes,
   whose next start already says "An unsaved edit was kept at {folder}" (`recovered.rs`,
   `i18n.rs:3797-3802`). Nothing else is queued on the worker behind these copies until their
   receipts are back.
3. **Session write** — queued **only behind landed copies, and only if time remains** before the
   3 s budget ends; it then waits as today (row 16).

**The quit is cancelled** — the window stays, and the existing failure card names the file
(b)7 — in each of these cases:
- phase 1 reaches `QUIT_DOCUMENTS_BY` with a document job still unanswered (the worker is inside a
  write; a copy would queue behind it and could not land);
- a copy is refused by the disk, or phase 2 reaches `QUIT_RECOVERY_BY` without its receipt;
- the session write is refused (today's rule, `SaveRefusal::quit_may_proceed`,
  `persist.rs:462-466`).

This is A§11's "a refused disk means do not leave", which J5 (b)8 applies to document saves
("Under A§11 … the quit is **cancelled** and the gate comes back saying the save did not
finish"). A session write that times out still lets the quit proceed, as today; that rule is
row 16's and unchanged.

**A quit asked again after a cancellation for the same refusal leaves.** The person has seen the
card naming the file; a second quit is the explicit choice to go without that change. The change
is then named in `diagnostics.log` (file and units) before the way out. Without this, a data
directory on a full disk or on a volume that cannot exchange would make Folio impossible to quit
by its own menu. This is (c)5's Q4.

**A forced stop** (`FolioApp::fail`, `exiting`; A§11) cannot wait, and takes J5's road: each
store's owed desired document is written into the `recovered` folder on the failing thread's
existing recovery step, beside the dirty documents `keep_dirty` writes, and named in
`diagnostics.log`. (c)3 lists this as something B6 needs from S-6c.

Tests (replacing (b)14 items 6–7):
- `quit_tests::a_moved_or_displaced_answer_at_quit_is_merged_and_landed_before_the_session`.
  MUTATION: queue the session job at the start of the documents phase.
- `quit_tests::a_refused_document_is_copied_first_and_the_session_waits_behind_the_copy`. MUTATION:
  queue the copy behind the session job.
- `quit_tests::an_unanswered_document_at_the_phase_deadline_cancels_the_quit` (fake clock, a fake
  disk holding the write). MUTATION: issue the copy at the expired deadline and proceed (revision
  (b)'s step 4).
- `quit_tests::a_refused_copy_cancels_the_quit_and_a_second_quit_leaves_and_says_so`. MUTATIONS:
  proceed on the first quit / never proceed.
- `app_diagnostics_tests::a_forced_stop_keeps_an_owed_settings_change_in_recovered`. MUTATION: skip
  the stores in the forced stop's recovery step.

### (c)3 · The J5 join: one worker, one request protocol, two semantic policies: adopted as the coordinator ruled

The confirmation's third blocker asked this note to restate the join as J5 revision (b) then
had it: one worker, two protocols, with the preview keeping its modified-time save as a separate
`StorageJob::Save`. Before revision (c) was finished, the confirmation on J5's own revision (b)
(`J5-05-contracts.review-codex-b.md`, item 3) ruled the other way, and the coordinator adopted
that ruling for both notes:

> "B6 is right: the conditional exchange is the lane protocol. The preview may retain its semantic
> policy — conflict rather than three-way merge — but must use B6's shared request
> `Document { expected: exact_disk_bytes, bytes, mode: Restore, link_policy }`. It must not retain
> the old mtime-only `PreviewBuffer::save` as a separate `StorageJob::Save`. B6's `Restore` mode
> restores displaced bytes and reports `Conflict`; the preview separately carries its body
> revision so dirty clears only for the current body. S-6c must also use B6's multi-turn
> `Documents` quit phase; configuration debts may take B6's sidecar road; an ordinary quit with
> an unresolved preview save may still be cancelled as J5 requires."

So (b)16 **stands**, with the amendments below, and the blocker is answered by the ruling rather
than by withdrawing (b)16 (see *Departures*).

**The contract, in three lines:**
- **One storage worker**: `persist::SessionWriter` on `session-writer`, woken with
  `AppEvent::DocumentsAnswered`.
- **One request protocol**, this note's:
  `Document { which, generation, expected: Option<Arc<[u8]>>, bytes, mode: Merge | Restore, link_policy }`;
  the stable precondition read (b)9; the exchange that hands back the displaced file (b)1–(b)2;
  every receipt processed in worker order (b)4; the answers `Landed`, `Moved`, `Displaced`,
  `Refused`. There is no `StorageJob::Save`.
- **Two semantic policies**, chosen by `mode`:
  - **`Merge`** — the four stores. A `Displaced` answer keeps `kept` on disk and is merged by
    (c)1's bookkeeping; a `Moved` answer is merged; owed edits are (b)6's.
  - **`Restore`** — the preview. On displaced bytes that are not `expected`, the worker exchanges
    **again**, conditional on the target still holding `written`, which puts the displaced bytes
    back under their own name, and answers `Conflict { theirs }`. If that second exchange is itself
    crossed (the target no longer holds `written`), both files are kept, nothing is removed, and the
    answer is `Conflict { theirs, kept }` naming where each is. A `Moved` answer is `Conflict` with
    nothing written. Nothing is merged; the conflict is said to the person, as today.

**Revision bookkeeping under `Restore`.** (c)1's invariant holds for both modes: the revision is
what Folio last read from or placed at the target. After a restoring exchange the target holds
`theirs` again, so the preview's disk revision becomes `theirs` — the file the person now has —
and its body stays dirty.

**What the preview's policy needs from ticket 4** (and what S-6a builds on it):
1. **`mode: Restore`**, implemented in the worker, with the second conditional exchange above and
   its own test (`…::restore_puts_the_displaced_file_back_and_answers_conflict`; MUTATION: answer
   `Conflict` without the second exchange).
2. **`link_policy: Refuse | PlainWriter`** on the request. The four stores send `Refuse` (Q5). The
   preview's links are J5's to rule (`preview.rs:5766-5773`, "nobody has ruled it"); until then it
   sends whatever J5 decides, and `PlainWriter` — the current behaviour — is honoured only for a link,
   never for a no-exchange volume, which is refused for every request ((b)1).
3. **An exact-byte disk revision for a buffer.** The preview's precondition moves from
   `file_mtime(&path) != self.disk_mtime` (`preview.rs:5785`) to the bytes it last read or wrote. That
   is one encoded copy of the file per open buffer with a save owed or possible, bounded by the
   preview's own read cap; `Arc<[u8]>` shares it with the job ((b)10).
4. **The body revision travels with the request and comes back on the receipt untouched.** The
   worker never reads it; the preview clears dirty only on `Landed` for its current body revision
   (J5). Ticket 4's `Document` carries an opaque `asker: u64` for this, echoed on every answer.
5. **Per-file order** across every `Document` and `Read` naming one path, whichever store or buffer
   queued it: a preview open on `settings.json` and the settings store then write one file through one
   queue, and each sees the other's write as a file that moved.
6. **Metadata**: the exchange door's (b)2 contract already carries what the preview's preserving save
   carries today (`atomic.rs:150-164`), so the preview loses nothing by moving onto it.

**The quit, aligned** ((c)2): S-6c uses this note's multi-turn `Documents` phase and its two
constants. Owed preview saves are sent in phase 1 beside owed configuration documents. A preview save
not landed by `QUIT_DOCUMENTS_BY`, or answered `Conflict`/`Refused`, **cancels the quit** as J5
requires (the gate comes back over a dirty document); configuration debts take (c)2's recovery phase
(the copy into `recovered`). A forced stop writes both into `recovered` (c)2.

**Ticket 4 stays the storage worker's shared request ticket**, now named
**T-STORAGE-WORKER-REQUEST**, jointly owned with J5's S-6a: ticket 4 builds the request, both modes,
the link policy, the `asker` echo and per-file order, and moves the four stores onto it; S-6a moves
`PreviewBuffer::save` onto it in `Restore` mode and retires its separate write. J5's S-6a dependency
on "B6's T-STORE-CONDITIONAL-WRITE" points at ticket 4.

**What the join costs this design**: `Restore`'s second exchange and its test; the `asker` echo; the
link-policy parameter; and one worker's head-of-line delay ((b)16 item 5). The request protocol itself
is unchanged.

### (c)4 · The tickets, revised

| # | ticket | size | after | the work | tests |
|---|---|---|---|---|---|
| 1 | T-STORAGE-EAR-NAMED | S | — (E-3) | as (b)17 | as (b)17 |
| 2 | T-STORE-REVISION | S | 1 | as (b)17 | as (b)17 |
| 3 | T-EXCHANGE-DOOR | M | — (E-1, E-2, E-5) | as (b)17 | as (b)17 |
| 4 | **T-STORAGE-WORKER-REQUEST** (jointly owned with J5's S-6a) | M | 2, 3 | the one request protocol (c)3: `StorageJob::Document` with `mode: Merge \| Restore` (`Restore`'s second conditional exchange and `Conflict`), `link_policy`, the `asker` echo, and `StorageJob::Read`, with stable reads (b)9; **per-file order** across every request naming one path (c)3; one in flight per document and every receipt processed (b)4; `desired` and owed (b)6 with (c)1's bookkeeping; **a `Moved` or `Displaced` before ticket 6 leaves the document owed and keeps `kept` on disk** (no merge yet, nothing removed); faults to cards (b)7; no mid-run `.rejected-`; row 20's four stores | 2's in-flight variant, 5, 8, 9, 10, 17, 18, 19; (b)14-5, -8, -15, -17; `…::jobs_for_one_path_run_in_queue_order_across_kinds` (MUTATION: a queue per kind); `…::restore_puts_the_displaced_file_back_and_answers_conflict` (MUTATION: answer `Conflict` without the second exchange); `…::the_asker_comes_back_on_every_answer` (MUTATION: drop it on `Displaced`) |
| 5 | T-QUIT-DOCUMENTS | M | 4 | (c)2: the two phases, the two constants, the copy into `recovered`, cancellation, the second quit, the forced stop's step for the stores | 4; (c)2's five |
| 6 | T-STORE-MERGE-SETTINGS | M | 4 | as (b)17, with (c)1's table | 3, 15, 16; (c)1's three; (b)14-9 |
| 7 | T-STORE-MERGE-KEYBINDINGS | S | 6 | as (b)17 | as (b)17 |
| 8 | T-STORE-MERGE-PROFILES-PINS | M | 6 | as (b)17 | as (b)17 |
| 9 | T-STORE-NOTICE | S | 7, 8 | as (b)17 | as (b)17 |

J5's S-6a (moving `PreviewBuffer::save` onto the shared request in `Restore` mode) follows ticket 4;
S-6c (the preview's saves in the `Documents` quit phase) follows ticket 5 and S-6b. Ticket 5 may run beside 6–8; 7 beside 8.
Between tickets 4 and 6 a crossing edit is kept, not merged: the store stays owed against a file it
will not write over, and the displaced file stays on disk — the protocol is safe before it is
complete, which is what lets the merge land separately.

### (c)5 · The owner questions, revised (five)

**Q1. When a value was changed both in Folio (not yet saved) and in the file, does the file's
stand?** Including the pins case the review accepts only if this question adopts it: a pin
unpinned and pinned again in Folio looks untouched to the merge, so a removal of that pin in the
file stands. **Recommendation: yes, the file wins every conflict, and one short card names what
came from disk.**

**Q2. Should a hand edit that conflicts with nothing take effect silently?** **Recommendation:
yes.**

**Q3. Should `settings.json` and `keybindings.json` be followed while Folio runs?**
**Recommendation: yes.**

**Q4 (revised). When you quit and Folio cannot save a change to one of these files, nor a copy of
it into the `recovered` folder, should the quit be cancelled — the window stays, with a card naming
the file — and a second quit leave without that change?** Folio first tries the file, then the copy
(which the next start announces, as it does for documents today). Only when both fail, or the disk
does not answer in time, is the quit cancelled. **Recommendation: yes.** It is the rule J5 adopts
for documents, and the second quit is what keeps a full disk or a volume that cannot exchange from
making Folio impossible to quit.

**Q5. When Folio cannot replace one of these files safely — a symbolic link, a file with a second
name (a hard link), or a volume that cannot exchange two files in one step — should it refuse and say
so rather than overwrite?** Unchanged from (b)18. **Recommendation: yes.**

### Departures

- **Blocker 3 is not applied as the confirmation states it.** The confirmation asked for "one
  worker with two protocols, the preview's existing baseline semantics … and S-6a/b/c", and for
  `Merge | Restore` and the jointly owned ticket to be reconciled away. The confirmation on J5's
  revision (b) then ruled, of the same question, "B6 is right: the conditional exchange is the lane
  protocol … [the preview] must not retain the old mtime-only `PreviewBuffer::save` as a separate
  `StorageJob::Save`", and the coordinator adopted that ruling for both notes: one worker, one
  request protocol, two semantic policies. (c)3 follows the ruling. What remains of blocker 3 is
  applied: J5's timeouts (a close keeps its gate open; an ordinary quit with an unresolved preview
  save is cancelled; a forced stop writes the recovery copy) are this note's too, and the preview's
  conflict policy and body revision stay J5's.
- Blockers 1 and 2 are applied as stated; (c)2 takes both of the review's alternatives at once (a
  recovery phase *and* cancellation when it cannot be realised), the combination the coordinator's
  message names.

### Sources added

- The confirmation: `B6-settings-reread-design.review-codex-b.md` (the 0.4.8 trace, 2026-10-11).
- The confirmation on J5's revision (b): `J5-05-contracts.review-codex-b.md`, item 3 (the 0.4.8
  trace, 2026-10-11), and the coordinator's ruling adopting it for both notes.
- J5 revision (b): `docs/plans/design/side-effects-contract-2026-10-11.md` (on `j5-contracts`),
  (b)8 (`:535-554`) and S-6a/b/c (`:586-588`).
- Folio at `77399d57`: `preview.rs:7059` (`RECOVERED_FOLDER`), `main.rs:66335-66348` (the forced
  stop's recovery step), `i18n.rs:3797-3802` (the start's sentence), `persist.rs:399`
  (`SESSION_SAVE_BUDGET`), `persist.rs:462-466` (`quit_may_proceed`).

---

## Revision (d), 2026-10-11, after the scoped review of revision (c)

The scoped review (`B6-settings-reread-design.review-codex-c.md`, in the 0.4.8 trace) adopts
revision (c) with three blockers. This revision is scoped to exactly those three. All three are
applied; none is argued away. Each closure is stated as a rule and pinned by named tests with
their mutations. **Where revision (c) and revision (d) differ, revision (d) stands**:
- (d)1 replaces (c)1's sentence on removing `kept`;
- (d)2 replaces (c)2's paragraph "A quit asked again after a cancellation … leaves";
- (d)3 extends (c)2's forced-stop paragraph;
- (d)4 amends (c)4's ticket 5;
- (d)5 replaces (c)5's Q4.

### The change table

| # | the review's words | disposition | closure | pinned by |
|---|---|---|---|---|
| 1 | "The `kept` cleanup rule is unsound across `Displaced → Moved → Displaced`: after `Moved`, no future write necessarily has `expected = written₁`, so cleanup obligations must follow derived generations and all applicable `kept` files must be removed only when such a generation lands" | applied | Each `kept` file is tagged with the desired-document version its merge produced. A landed write of version `v` removes every `kept` file tagged `≤ v`, and no other answer removes any. | (d)1's three tests |
| 2 | "A repeated Quit is not an explicit discard merely because the earlier card named the file; provide a person-visible 'leave without this change' choice and amend A§11's 'refused disk means do not leave' rule, since a post-exit diagnostics line is neither warning nor consent" | applied | The cancelled quit's card carries one choice, **Leave without this change** (`// zh: pending`). Only that choice leaves. A repeated Quit runs the transaction again. A§11's rule gains its one exception, worded in (d)2. | (d)2's three tests |
| 3 | "The forced-stop path specifies and tests only a successful recovery copy; it must define and test recovery refusal as an explicit `Lost` outcome reported through a surviving native channel" | applied | A store's forced-stop outcome is `Kept { copy }` or **`Lost { file, error }`**. `Lost` is said through the operating system's own message box (`bt_platform::message_box`), which needs neither the disk nor the renderer. `diagnostics.log` is still attempted, but it is not the channel. | (d)3's three tests |
| — | consequences: ticket 5, and Q4's wording | revised | — | (d)4, (d)5 |

### (d)1 · `kept` follows derived generations: adopted

**The flaw.** Revision (c) said `kept` is removed "only when a write whose `expected` is
`written` lands". Consider `Displaced₁` (keeps `kept₁`), then `Moved` (the revision becomes the
file's `theirs`), then `Displaced₂`. After the `Moved`, every later write is conditional on
`theirs` or something newer, never on `written₁`. So `kept₁` would never be removed, although
documents derived from its merge did land.

**The rule.**
1. Each store's `desired` document carries a **version**, a counter on the store that rises by
   one at every change to `desired`: a press's unit delta (b)6, or a merge (c)1.
2. Every `Document` job carries the version of the document it sends. Its answer echoes it.
3. When a `Displaced` answer's merge folds the displaced bytes into `desired`, the resulting
   version `m` is recorded beside that answer's `kept` file: an obligation `(kept, m)`.
4. **A `Landed` answer for version `v` removes every `kept` file whose `m ≤ v`.**
5. **Nothing else removes a `kept` file**: not `Displaced`, `Moved` or `Refused`, and not a quit.
   A file left behind by an exit or a crash stays on disk under its `.displaced-` name, never
   read back, like a `.rejected-` copy.

**Why `m ≤ v` is "derived from".** `desired` is one variable with one line of descent. Every
version after `m` is `m`'s document changed by press deltas and later merges. So any document of
version `v ≥ m` descends from the merge that took `kept`'s units in. A later merge may let the file
win over one of those units (c)1. That unit is then on disk in the file's newer value, named on that
pass's card. The `kept` copy has been superseded, not lost, and removing it is correct.

**The sequence the review names, run through the rule.**

| step | answer | result |
|---|---|---|
| 1 | `Displaced₁` | obligation `(kept₁, m₁)` |
| 2 | `Moved` | its merge makes version `m₁+1` |
| 3 | `Displaced₂` | obligation `(kept₂, m₂)`, with `m₂ > m₁` |
| 4 | a `Landed` of version `v ≥ m₂` | removes both `kept₁` and `kept₂` |
| — | a `Landed` of a version in `[m₁, m₂)`, had one come between | removes only `kept₁` |

Tests (T-STORAGE-WORKER-REQUEST, ticket 4; the merge itself in ticket 6):
- `persist::conditional_write_tests::kept_files_survive_displaced_moved_displaced_until_a_derived_version_lands`.
  This drives the review's sequence with a fake disk. Both `kept` files stay on disk through steps
  1–3, and both are gone after step 4. MUTATION: remove `kept` only when `expected == written`
  (revision (c)'s rule). `kept₁` is never removed, and the test fails at step 4.
- `…::a_landed_older_version_removes_only_the_kept_files_it_derives_from`. MUTATION: on any `Landed`,
  remove every obligation.
- `…::no_answer_but_landed_removes_a_kept_file`. MUTATION: remove `kept` on the `Displaced` answer
  (revision (b)'s rule).

### (d)2 · Leaving without a change is a choice on the card: adopted

**The flaw.** Revision (c) let a second Quit leave and wrote the loss to `diagnostics.log` on the
way out. The review: a repeated Quit "is not an explicit discard merely because the earlier card
named the file", and "a post-exit diagnostics line is neither warning nor consent".

**The card.** When (c)2 cancels a quit for a configuration document, the window stays. It raises one
Error card, anchored to the window, under the copy rules (a plain statement, few words, no advice):

> `settings.json could not be saved.` · **Leave without this change** `// zh: pending`

- Several documents share one card: `settings.json and profiles.json could not be saved.` At most
  three names are shown, then "and N more" (b)11.
- The choice covers exactly the documents the card names. The full list, with the reason for each,
  goes to `diagnostics.log`, as (b)11 does.

**The rule.**
- **Only the card's choice leaves.** Pressing **Leave without this change** resumes the cancelled
  quit at the session write. The named documents' owed changes are dropped from this run, and each
  is written to `diagnostics.log` before the way out.
- **A repeated Quit is not a discard.** It runs the transaction again from the documents phase. The
  disk may have recovered, in which case the quit simply completes. If it has not, the same card
  comes back.
- **Dismissing the card is not a choice either.** Folio stays open, and its changes stay owed.

**A§11, amended** (`ARCHITECTURE.md`, §11, the normal-quit paragraph). Today it reads "a refused disk
means do not leave". It becomes:

> "a refused disk means do not leave, **unless the person chooses to leave without the change the
> refusal names, on the card that names it**".

This is the same exception the dirty gate already gives a preview document (*Save · Discard ·
Cancel*): leaving with unsaved work is the person's decision, made where the loss is stated, never a
default. The amendment goes into the ticket's DESIGN entry and into A§11. That adds one row to (b)15's
list of edits, owned by ticket 5.

Tests (ticket 5):
- `quit_tests::a_repeated_quit_runs_the_transaction_again_and_does_not_leave`. MUTATION: revision
  (c)'s rule (a second Quit after a cancellation leaves).
- `quit_tests::only_leave_without_this_change_leaves_and_drops_only_the_named_documents`. Two stores
  are owed: one refused, one landing. Pressing the choice leaves. The refused store's change is
  dropped and named in diagnostics before the exit; the landed one was saved. MUTATIONS: drop every
  owed document; leave on the card's dismissal.
- `app_quit_tests::the_cancelled_quits_card_names_the_files_and_carries_one_choice`. This asserts the
  card's statement, the bound of three names, and the one choice. MUTATION: raise the card with no
  choice.

### (d)3 · A forced stop's `Lost` outcome, said through a surviving native channel: adopted

**The flaw.** Revision (c)'s forced stop wrote each store's owed document into `recovered` and named it
in `diagnostics.log`. It did not say what happens when that write is refused. With no writable disk,
both the copy and the log line fail, and the loss is silent. A§11 already asks for the repair: "take a
durable receipt or retain an explicit unpreserved state; retire only once that outcome is known,
reporting through a surviving native path if the renderer failed" (`ARCHITECTURE.md`, §11).

**The outcome.** The stores' forced-stop step answers one value per owed store, in the shape the
preview's `keep_dirty` already uses (`preview::Kept`, `preview.rs:7063-7080`):
- `Kept { file, copy }` — the copy landed in `recovered`, the file was `fsync`ed and the directory
  flushed;
- **`Lost { file, error }`** — the copy was refused. The change went with the process.

**The channel.** After the step, and before the windows are closed, every `Lost` is said once,
together, through **`bt_platform::message_box`** (`bt-platform/src/lib.rs:17847-17891`). This is the
operating system's own dialog: `MessageBoxTimeoutW`, ownerless and bounded at fifteen minutes, on
Windows; an `NSAlert` on the main thread on macOS (`macos_dialogs.rs:781-848`). It needs no file, no
data directory and no renderer of Folio's, so it survives exactly the failures that silence the other
two. The text is a plain statement (`// zh: pending`):

> `Folio stopped. A change to settings.json could not be kept.`

Several stores give one box: "Changes to settings.json and profiles.json could not be kept."
`diagnostics.log` is still attempted for each `Lost`, but it is not relied on.

- **No `Lost`, no box.** A copy that landed is announced at the next start, as today (`recovered.rs`).
- **Why a box here.** `message_box`'s own doc allows "the single message box this product is allowed to
  raise" only because there is no window to put a card on (`lib.rs:17849-17854`). A forced stop is the
  same situation: the windows are being torn down, and the renderer may be the thing that failed. The
  doc gains this second case in ticket 5, and the guard that holds `message_box`'s callers gains one
  row.
- **The preview's own `Lost`** (`preview.rs:7074`; today a diagnostics line only,
  `runtime/windows.rs:1719`) has the same gap. Folding it into the same box is J5/G7's to adopt (S-6c),
  and is named here, not built.

Tests (ticket 5; the box through an injected `NativeSay` seam, so no dialog opens in a test):
- `app_diagnostics_tests::a_refused_recovery_copy_is_lost_and_said_natively`. The fake disk refuses
  both the copy and the log line. The store answers `Lost`, and the seam is asked once, naming
  `settings.json`. MUTATION: report `Lost` to `diagnostics.log` only (the preview's behaviour today).
- `…::several_lost_stores_are_one_native_message`. MUTATION: one box per store.
- `…::a_landed_recovery_copy_raises_no_native_message`. MUTATION: say every outcome natively.

### (d)4 · Ticket 5, amended

| # | ticket | size | after | the work | tests |
|---|---|---|---|---|---|
| 5 | T-QUIT-DOCUMENTS | M | 4 | (c)2's two phases, constants, copy into `recovered` and cancellation; **(d)2's card with the choice Leave without this change, the repeated Quit that runs again, and A§11's amendment**; **(d)3's `Kept`/`Lost` forced-stop outcome and the native message for `Lost`**; `message_box`'s doc and its caller guard | (c)2's quit tests, without the second-quit test; (d)2's three; (d)3's three |

Ticket 4 also gains (d)1's version tag and obligation rule, with its three tests. Every other row of
(c)4 stands. Ticket 5 stays M: the card reuses the toast's verb road (the next start's `recovered`
toast already carries a verb, `ARCHITECTURE.md` §11), and the native message reuses an existing door.

### (d)5 · Q4, reworded

**Q4. When you quit and Folio can save neither a change to one of these files nor a copy of it in the
`recovered` folder, should the quit stop and a card say "settings.json could not be saved", with
one choice, "Leave without this change"?** Quitting again would try again; only the choice would leave
without the change. And if Folio is stopped by a failure and cannot keep the change anywhere, should
the system's own message box say so, because nothing else may survive to say it? **Recommendation:
yes, to both.** Leaving with unsaved work is the person's decision, made where the loss is stated, as
the dirty gate already makes it for documents.

Q1, Q2, Q3 and Q5 stand as (c)5 states them.

### Departures

None. The three blockers are applied as the review states them. For (d)3's channel, the review
suggested "the OS's own notification/event log". The system message box is chosen because it is
that kind of channel and already a product door, which a notification and an event-log source are
not: both would add a registration that can itself fail at stop time.

### Sources added

- The scoped review: `B6-settings-reread-design.review-codex-c.md` (the 0.4.8 trace, 2026-10-11).
- Folio at `77399d57`:
  - `bt-platform/src/lib.rs:17847-17899` (`message_box`, `standalone_alert`);
  - `macos_dialogs.rs:781-848`;
  - `preview.rs:7063-7080` (`Kept`, `Lost`);
  - `runtime/windows.rs:1719`;
  - `main.rs:66328-66354` (`stop_every_window`);
  - `docs/ARCHITECTURE.md` §11 (the normal-quit paragraph's "a refused disk means do not leave"; the
    controlled-failure repair's "surviving native path").
