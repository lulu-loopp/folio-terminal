# Folio — the rules in force

This file is the current rules. `DESIGN.md` is the history of how they were
decided. When a dated entry overrides a rule, this file changes in the same
commit; the dated entry is not the rule.

`docs/ARCHITECTURE.md` holds the shape — processes, crates, ownership, lanes,
doors, chains. This file holds one row per subsystem: what is true now, and
where it was decided.

**How to read a row.** `folded` means the rule below was written from the entries
after every later correction, and is the rule. `not yet folded` means nobody has
read the entries end to end yet; the addresses are listed and **the entries
themselves are still the only authority for that subsystem.** A `not yet folded`
row is a promise of where to look, not a summary.

**How to fold a row.** Read every entry listed, in order, including the trailing
undated ones at the end of `DESIGN.md`, which are the most recent and are usually
the correction. Write the rule as it stands after all of them. Change the state
to `folded` in the same commit. Never cite a line number.

The subsystem list is the one the 2026-09-21 breadth survey opened: its
fifty-one rows, plus two cross-cutting rows (doors, threads and lanes) that are
not a subsystem but are a rule. Row 54, the look of the window, was added on
2026-09-22 with the written UI spec.

---

## The history's own defects, recorded as facts

These are properties of `DESIGN.md` that a reader will hit, listed so that
hitting one is not a discovery:

- **Duplicated section numbers.** §7.14, §7.19, §7.45 and §7.46 each appear
  twice, under different titles and different dates. A citation of one of these
  numbers alone does not identify an entry; cite the title with it.
- **About twenty-five trailing entries carry no section number at all.** They
  are the most recent entries and they are frequently the correction to a
  numbered one above. A fold that stops at the numbered sections is wrong.
- **`## 8` was overwritten.** Commit `3d46a3e7` replaced the `## 8. 依赖策略`
  heading with a §7 sub-entry. Its one-paragraph body still dangles at the end
  of §7 and is about the vendored terminal seam, not about dependencies. Sixteen
  comment lines in four manifests cite "`docs/DESIGN.md` §8's bar" for a rule
  that never had that body. **The dependency policy now lives in
  `docs/ARCHITECTURE.md` §3.3**, restated from what those manifests practise.
- **Superseded rules stay inline**, marked in place rather than removed, so
  currency is recoverable only by reading an entry plus every later correction.
  That is the cost this file exists to remove.

---

## The index

### 1. Terminal grid and scrollback — `not yet folded`
Entries: §3.1 *the content lifecycle event table (v3.7, single-ownership
revision)*; §3.3 *documents and viewport projections*; the `lifecycle.rs` module
doc in `bt-term`. Owner: `TerminalAdapter` / `TranscriptStore` /
`DualPlaneSession`.

### 2. The resize transaction — `not yet folded`
Entries: §3.1; §7.53a *a resize transaction is opened by a reflow and can only be
closed by one*; the M1.7 and M1.8 plan documents. Owner: `ResizePlan`,
`DualPlaneSession::resize_at`, and roughly ten free functions in `main.rs`.
The rule is spread across four places and that is itself the finding.
Also: trailing entry 2026-09-23 *Decoration never covers text* — a pane's first
shell-integration mark causes at most one extra grid change in its lifetime (the
rail's reserve); the alternate screen and later marks cause none.

### 3. PTY and ConPTY — `not yet folded`
Entries: §1.3 *the thread and resource model*; the `bt-pty` crate doc. Owner:
`PtySession` plus the `Retirements` reaper.

### 4. Shell integration marks — `folded`
**Rule.** The `OSC 133 A/B/C/D` plus `OSC 7` marks are the authoritative
prompt/input/output boundary for each screen that emits them: scanned before
`vte` swallows them by `bt_term::inline_image::Osc1337Scanner`, kept in
`bt_term::command_marks::CommandMarkLedger`, surfaced as
`AdapterEvent::ShellIntegration`. PowerShell integration stays opt-in: Folio
appends exactly one generated managed line (`profile_marks::managed_line_for`,
carrying its own guard and version comment) to the `$PROFILE` **the shell itself
named** — never a path this build assembles — keeps a dated `.bak-<YYYYMMDD>`
beside it, and records the write in `integration-marks.json` so that
`profile_runtime::begin_removal`, `--remove-shell-integration` and
`--uninstall-cleanup` can undo exactly what Folio wrote. Every writer of that
record takes the advisory lock with an explicit asker: `Asker::InApp` waits
behind Folio's own writers with no deadline, and up to `OUR_TURN` for a holder
in another process, reporting only that second wait running out; `Asker::Door`
refuses at once with the same error. Which holder is ours is known by
construction — every writer in this process stands in `profile_marks`'s
in-process queue before it takes the OS lock — never by asking the OS. `cmd.exe` carries `A` and `D` through its
prompt string; `B` is still refused where there is no `C`.
**From.** §7.6 *terminal notifications* (the single scanner seam, no vendor
patch); §7.57 *`cmd.exe` finally has a scale on its command rail*; §7.1.6j (the
opt-in `$PROFILE` offer, the backup, "ask the shell for `$PROFILE`, never spell
it"); `docs/shell-integration.md`, which states it is the authority for the
protocol; trailing entry 2026-09-21 *Folio's own writers wait their turn for the
marks record*; trailing entry 2026-09-21 *a removal that found nothing says
nothing in the window*; trailing entry 2026-09-23 *a writer of Folio's waits
behind another of Folio's writers for as long as that one takes; the two-second
bound is only for a holder in another process*.
**Overrides.** The 2026-09-21 lock entry replaces a bare `try_lock` whose busy
case surfaced as a red toast. The 2026-09-23 entry replaces that entry's
"`Asker::InApp` … waits its turn up to `OUR_TURN`" for a holder in this process,
which gave a false `WouldBlock` whenever our own writer's I/O outlasted two
seconds on a slow disk. There is no §7.1.5c; the marks rules live in the
sections above.

### 5. The printed-path chain — `folded`
**Rule.** A pane recognises a bare printed path **in its own shell's namespace
only** — `bt_transcript::paths` with the pane's namespace read from the profile's
directory-namespace and integration pair, never guessed from the text — across
every file type and directories, off the projection layer's whole logical line,
and underlines it only once the disk says the name exists (`bt_term::verify_path`,
which is an existence question: the trailing-dot rule, then
`may_read_unasked_through_links`, then one metadata call). A candidate touching
the last visual cell of a row is suppressed, because it may be truncated. **A
relative name is read against the pane's own folder and nowhere else** — the
last `OSC 7` if one arrived, else the spawn place (`reference_directory()`); a
pane that has neither marks nothing, and no parent, sibling, workspace root or
recently printed absolute path is tried. A seam is one character-class
transition, not a list of stops: a mark a path is never spelled with ends the
name in whichever width it was typed (`is_seam_separator` reads its class off
`is_path_tail_char`). An ASCII separator needs a witness — a non-ASCII character
glued behind it — because it may be part of a name (`a.md,b`); a non-ASCII
separator is prose and needs none, so `…md。18` ends at the `。`. Every reading is
still offered longest first, so a name that really holds a `、` is asked about
whole before the shorter one. **The separator that admits a bare relative name is one
that divides two segments**, so that test is asked of the name with its trailing
separators taken off: `docs/` is `docs` with a slash after it and is refused,
while `docs/plans/` is a directory somebody named and is a link whose span
carries the slash as printed.
Freshness is asymmetric: **a "yes" is never re-asked** and expires only at the
command boundary; **a "no" is re-asked** whenever the program prints the name
onto a freshly changed row (a repaint is not a printing); and **the press puts
the question again** as its own re-check. The gesture is plain click = open it
in this window, `Ctrl`/`⌘`+click = hand it to this machine's registered handler,
reveal for a folder. **A share on another machine (`\\server\share\…`) is never
asked about** — not on a hover, a press or the modifier going down; a plain click
raises the preview's network card, and `Ctrl`/`⌘`+click hands it to the system
through the files column's door on the OS hand-off lane, where it meets the same
program list a local file meets. A device path, a verbatim spelling and a WSL
distribution the pane is not standing in are not another machine's share and keep
their answers. **A link written in a previewed document answers the same row**
(`preview::link_action` feeds the one table, `reference_activation`).
**All verification runs on a lane of its own, never the
window thread**, and the worker produces the door's input with the door's own
function (`bt_platform::resolved_for_a_door`), arriving as `VerifiedTarget`.
**From.** §7.1.5j *bare printed paths are recognised, underlined and clickable,
for every file type*; §7.1.5k *systematic hardening: the end-of-row truncation
gate and the five gates rewired*; §7.30 *a half-width punctuation mark followed
by a Chinese character ends the name — a candidate's several readings, asked
longest-first*; §7.1.5g *link activation, Ctrl+click and the five-arm routing
table*; trailing entries 2026-09-20 *the window thread never asks the disk about
a path a program printed*, *a name printed again is asked about again*, *a press
asks what a pointer move asks*; trailing entries 2026-09-21 *correction: what a
reveal must still know*, *correction: the component-by-component link walk is
withdrawn*, *correction: the worker produces the door's input with the door's own
function*, *a printed path may hold spaces; the disk still says which reading is
real*; trailing entry 2026-09-22 *a seam sits on the mark that ended the name,
whichever keyboard wrote it; and a relative name is read against the pane's own
folder or not at all*; trailing entry 2026-09-22 *a trailing slash is not the
evidence a bare reference is admitted on; it is the person naming a directory*;
trailing entry 2026-09-22 *a full-width stop needs no witness*;
the project owner's ruling of 2026-09-21 that Ctrl+click on a printed path opens
it, as before; the owner's ruling of 2026-09-21 that a relative name's folder
is never guessed; trailing entry 2026-09-23 *Ctrl+click hands a share on another
machine and a link of any scheme to the system; a plain click and a hover are
unchanged, and a document's links answer the same row*.
**Overrides.** The owner's 2026-09-21 ruling withdrew the 2026-09-20 reveal-only
rule and removed the volume question it had introduced. The owner's 2026-09-21
ruling (「UNC 与任意协议链接 Ctrl+点击交给系统、悬停不碰 UNC、普通点击不变」) and
2026-09-23 ruling (document links follow the terminal's rule) replaced "a share is
the card under either modifier" for `Ctrl` only, landed with ticket 14 once
hand-offs were off the window thread. The per-component link
walk, its hop limit and its locality type were withdrawn the same day. The
denial-permanence rule was reversed twice: on 2026-08-25 (denials expire) and
again on 2026-09-20 (re-ask on reprint).

### 6. `OSC 8` hyperlinks — `folded`
**Rule.** An `OSC 8` target and a recognised bare path are the same object: the
bare path is written into the cell as an implicit `CellHyperlink` carrying a
`file:` target, so both travel one routing table with one gesture policy, and an
`OSC 8` target is asked of the same verdict ledger on the pointer event that
meets it. `OSC 8` is exempt from the end-of-row truncation gate alone, because
the application declared the whole target itself. The gesture: plain click = this
window's answer (the seat for a page or a file, the files column for a folder,
the network card for a share, nothing for `mailto:` or any other scheme);
`Ctrl`/`⌘`+click = the system's — the browser for `http`/`https`, the registered
handler for a file, Explorer for a folder, **the system for a share on another
machine, and whatever the machine has registered for any other scheme**
(`mailto:`, `vscode:`, `ssh:` …), with no list of schemes; what the machine
refuses is said on the hover line as a refused address. A single letter before a
colon is a drive, not a scheme, and a path never leaves as a URI. **A link in a
previewed document follows this row**, both halves.
**From.** §7.1.5g *link activation and the five-arm routing table*; §7.1.5j ①
(a hit folds into a `file:` target and feeds the existing table); §7.1.5k ①;
trailing entry 2026-09-23 *Ctrl+click hands a share on another machine and a link
of any scheme to the system; a plain click and a hover are unchanged, and a
document's links answer the same row*.
**Overrides.** §7.1.5g's original "plain click does nothing, Ctrl hands it over"
was reversed by the 2026-08-20 ruling *plain click stays in the window, Ctrl+click
hands it over*, aligning hyperlinks with image references. The owner's rulings of
2026-09-21 (share and any-scheme links to the system on `Ctrl`) and 2026-09-23
(document links follow the same rule) replaced the refusal of every scheme but
`http`, `https` and `file` under `Ctrl`.

### 7. Math and table detection (`bt-detect`) — `not yet folded`
Entries: §4.6b *a window may begin in the middle of a block without needing
history first*; §4.6d *two formulas on one line do not suppress each other, and a
row separator need not be an ampersand to be read*; the `bt-detect` crate doc.

### 8. Selection — three models, by ruling — `folded`
**Rule.** This window keeps three selection models on purpose, each in its own
coordinate system, and that is a ruling and not a debt: the terminal grid's is a
pair of `bt_doc::ContentAnchor`s over cells, owned by `DualPlaneSession`; the
source and editing face's is `preview_edit::EditCaret`, two byte offsets into one
flat string on a monospace grid; the rendered-markdown face's is
`preview_select::Place`, a block/piece/offset triple ordered by document order so
a place survives a scroll. §7.31 rules that the source face's arithmetic cannot
be carried onto proportional reflowed text. **They converge at the clipboard
door, not in a selection type** — `main::recoverable_clipboard_write` over
`bt_platform::set_clipboard_text` — which the terminal reaches through
`write_selection_text` and the rendered page through `preview_select::copy_text`.
**From.** §7.31 *the words a page renders can be taken away: the selection lands
on the layout layer and what copies is what was read*; §7.1.3q; §7.1.3w. The grid
model's own rule is `nowhere written` as an entry; it lives in
`bt_doc::anchor::Selection` and `DualPlaneSession::selection_text`.
**Overrides.** §7.1.3w (owner's ruling 2026-09-11) reverses §7.1.3q's face rule:
the caret's prose block now stays in the body face and only code, tables and
formulas turn monospace.
**Correction to an earlier claim.** The three do **not** converge at
`write_selection_text`; that function serves `DualPlaneSession` only.

### 9. Copy and paste, and `OSC 52` — `folded`
**Rule.** This terminal acts on `OSC 52` in **neither direction**:
`TerminalAdapter::new` configures it disabled, so a program can neither put text
on the reader's clipboard nor read it back, and no store is decoded. Clipboard
reads and writes happen at one door (`bt_platform::set_clipboard_text` and its
read twin), today on the window thread.
**From.** The rule is `nowhere written` in `DESIGN.md`; it lives in
`crates/bt-term/src/adapter.rs` with its pin in `bt-term`'s bounded-bytes test,
adopted from the 2026-09-08 adversarial review finding R1-27, where an `OSC 52`
store was decoded in full and then discarded. §7.1.5e and the `input.rs` header
carry the paste rules.
**Overrides.** none found.

**A multi-line paste into a shell that would run it line by line asks first**
(owner's rulings 2026-09-22 and 2026-09-23; `DESIGN.md` trailing entry
2026-09-23 *A multi-line paste into a shell that would run it line by line waits
on a card in the middle of the window*). One rule, `paste_road`, asked once per
paste by `stage_paste` from `Runtime::deliver_paste`, where all four paste doors
converge: the card is raised when the `multiline_paste_ask` setting is on, the
payload was the clipboard's own text, the pane has not set `?2004`, and
`input::pasted_line_count` is more than one (a single trailing separator does not
count). **PowerShell asks nothing** (ruling 2026-09-22; `DESIGN.md` trailing
entry 2026-09-23 *A multi-line paste into PowerShell lands whole on the input
line and runs on one Enter*, 0.4.4 ticket 03): a pane whose paste grammar is
PowerShell at a prompt the shell opened in order, on Windows, is never shown the
card, whatever the setting; the block lands on PSReadLine's input line and runs
on one Enter. One write: the byte `0x16` (`bt_pty::PSREADLINE_PASTE_INPUT`, and
PSReadLine pastes the clipboard itself) when `input::psreadline_pastes_it_unchanged`,
otherwise Folio's cleaned bytes with each break a Shift+Enter record
(`input::input_line_bytes`). Only the clipboard's own text takes it; a paste the
clipboard is rewritten under within ~20 ms is an accepted limit. A PowerShell
pane missing any fact (no marks, a program running, macOS) gets the card. The card, centred on the
window, says `N lines → <shell>`; `Enter` = *Run line by line* (today's bytes),
`Tab` = *Join into one line* (`input::join_lines`: one space per run of breaks,
no `\r`, no invented separator), `Esc` or `×` = nothing sent, clipboard
untouched. **It is modal (2026-09-23)**: it answers only those three keys, every
other key reaches nothing, and a drop under it is refused. The pending paste is
`LeafSession::pending_paste` and dies with its shell; the answer is re-checked
with `live_paste_target` before a byte is sent. Nothing is remembered but the
setting.

### 10. Drag and drop — `not yet folded`
Entries: §2.12 *a tab and a pane dragged out of this window: an application-level
drag broker*; §7.1.1; §7.14d *a replay is neither a join nor a leave*.

### 11. Clipboard pictures — `not yet folded`
Entries: §7.61 *`T-CLIPBOARD-IMAGE-PASTES-PATH`: a picture on the clipboard is
not an argument, so Folio writes it one*; the `clipboard_picture` module doc.

### 12. The layout solver — `not yet folded`
Entries: the standalone specification under `docs/plans/` (rules L1–L13,
decisions D1–D5). `bt-layout` is a pure function with no state and no
dependencies; it is the one subsystem whose rule already has a single address.

### 13. Panes, tabs, windows — `not yet folded`
Entries: §2.4 *the runtime's two layers: application and window*; §2.5 *one
process, many windows: routing and lifecycle*; the pinned layer-shape tests.
Owner: `FolioApp` / `WindowRuntime` / `TabState` / `Seats`.

### 14. The persistence schema — `not yet folded`
Entries: the M2 persistence schema document, which is the authority; §2.7 *one
file, many windows*. Owner: `SessionV1` and the settings migration list.
The one rule worth stating before the fold: migrations are forward-only and
structural, and unknown keys are kept, not dropped.

### 15. Session restore — `not yet folded`
Entries: §2.7; §7.1.4; §7.54 *a window nobody can see is the one that needs a key
that lives in no window* (the quick terminal's interaction with restore).

### 16. The quick terminal — `not yet folded`
Entries: §7.54 and its lettered continuations §7.54a–§7.54e, of which §7.54e
*behaviour model reorganised* is the latest and rules.

### 17. The files column — `not yet folded`
Entries: §7.5 *"go in" and "pin" in the files column*; §7.15 *a row in the tree
has a two-faced menu*; §7.24 *the files column watches those folders itself*;
the `files.rs` module doc; the trailing entry of 2026-09-23 *the glance card's
foot names the folder that holds the file* — rulings of 2026-09-20 (the glance
card's foot locates its file in this column: the folder kept when it is inside
the root, re-rooted when it is not, the file's row selected) and 2026-09-23 (the
column does not switch back afterwards; it is navigation, not a peek). **Note
the door gap**: this subsystem's directory enumeration has no read-ledger lane
(see row 52).

### 18. The preview dispatcher — `folded`
**Rule.** Exactly one ladder decides what a preview surface shows:
`preview::preview_ftype` classifies the file by extension and, where the name
cannot answer, by content sniff; `preview::preview_view` returns the single
`PreviewView`; and `PreviewView::chrome()` maps that to one of four
`PreviewChrome` machines. **Every surface — pane, pop-out float, hover peek card,
focus thumbnail — reads that one answer** instead of descending a ladder of its
own. A new preview kind joins by adding a variant and one rung at the ruled
position (the rung order is itself a ruling: a diff name is tested before the
text surface so a patch never gets a text area) and by declaring which chrome
paints it. **A document's links are answered by the terminal's table**
(2026-09-23): `preview::link_action` says what a target names and
`reference_activation` — the table row 6 reads — says what a press spends; plain
click stays in the window, `Ctrl`/`⌘`+click hands it over.
**From.** §7.1.3 *the file tree, the preview minimum contract, and the tab-level
shared buffer pool*; §7.10 *a local file can also be a web page*; §7.32 *when a
name cannot answer, ask the file itself: text is decided by content, and a page's
source face is an editor*; trailing entry 2026-09-23 *Ctrl+click hands a share on
another machine and a link of any scheme to the system; a plain click and a hover
are unchanged, and a document's links answer the same row*.
**Overrides.** §7.32 overrode extension-only classification and made the source
face editable; §7.10 moved `html`/`htm` from the text extensions to the page
extensions by the owner's 2026-08-23 ruling; the video view was split out of the
image view by the owner's 2026-08-27 ruling.

### 19. Live editing in the preview — `folded`
**Rule.** The document is the buffer, not the pane. `preview::PreviewBuffer`,
owned by the tab's preview pool, holds the content, the revision, the
incarnation, the encoding and line-ending facts, the undo log, and the disk
baseline; the dirty dot is simply the undo log's position at the last save. The
caret (`preview_edit::EditCaret`, seated by `preview_live::CaretSeat`) belongs to
the **view**. `PreviewBuffer::save` compares the recorded modified time against
the file and, on disagreement, **writes nothing** and answers a conflict — the
edits survive and the foot says so. A rename moves the buffer's identity onto the
new path; the new suffix changes the view but cannot take back the content sniff.
On a rendered page **every block the selection touches is drawn as source, as one
unbroken run with the caret's own block** (`preview_live::source_span`), each in
the face its kind wears; with nothing selected that is the caret's one block. The
selection is the caret's, or else the rendered selection mapped back to the file.
**A table swept by a selection stays rendered; only the caret entering a table
flips it to source**, until the 0.5 in-cell table editor replaces the flip. Faces
change only when a gesture ends: **the span a gesture starts on is held until the
release** (`preview_press::held_span`), a press or a drag may move the caret
anywhere the held span draws as source (`preview_press::keeps_the_span`), and a
lost focus ends the drag. **The source blocks are banded from one range of file
bytes** (`preview_live::source_band`), stretched while a drag is in flight to the
last byte the hand reached.
**From.** §7.1.3; §7.1.3q; §7.1.3s *undo lives on the buffer, and the dirty dot is
a position in it*; §7.1.3v *a buffer knows which disk state it is holding, and a
read is answered against the body it was issued for* (`T-EDIT-DISK`); §7.1.3w;
the 2026-09-21 entries *a block shows its source when the gesture ends, not
while a selection is drawn* and *a press inside the source block is answered
where it stands, and the seat holds until the gesture ends*; the 2026-09-23 entry
*a selection crossing the source block bands it too, a table only a selection
sweeps stays rendered, and a caret crossing into another block is measured*; the
2026-09-23 entry *every block a selection touches is drawn as source, a table it
only sweeps stays rendered, and the span is held until the gesture ends*.
**Overrides.** §7.1.3v overrides §7.1.3p: a read now carries the base it was
issued for, and a late answer about a body the buffer has moved past is refused
— it raises a disk-changed notice and keeps body, undo, caret and selection —
instead of landing. §7.1.3v also deleted a second copy of the atomic write.
The 2026-09-23 span entry overrides §7.1.3q's one source block and its "no block
is exempt", and the 2026-09-21 sentences "the page's face is a function of the
caret's seat alone" and "nothing is pinned in the painter".

### 20. The PDF glance — `not yet folded`
Entries: §7.10 item ⑥; the dependency essay in `crates/bt-app/Cargo.toml` that
admits the renderer under the dependency bar.

### 21. Math typesetting — `not yet folded`
Entries: §4.6 and its lettered continuations; §5 is a stub; the `bt-math` crate
doc, which carries the worker stack contract.

### 22. Animation and pacing — `folded`
**Rule.** One frame clock per window, its interval taken from the display and
defaulting to one sixtieth of a second when the display will not say. **A
journey's liveness is arithmetic on its own clock** — start ≤ now < landing —
and never a flag. **A wait is not motion.** An advancer's three jobs are
separate and only the third is paced: *service* (ungated — picture service, drag
autoscroll), *sample and draw* (at compose), and *ask for a frame of its own*
(the only gated one). Every composed frame carries every running journey, and
**landings are never paced**.
**From.** §7.1 items ⑬ and ⑬′ (one pacer per window; the hover leaves with the
pointer); the `pace.rs` and `animation.rs` module docs; §7.18 *motion tokens:
three steps, one travel distance, two curves, and a register that forbids a
fourth*; §7.19 *an overlay's entry and exit: a picture that fades, not a menu that
can still be clicked*; §7.20 *the ones that are not overlays*.
**Overrides.** ⑬′ supersedes the earlier half-second hover delay. The pacing
closure series replaced four patches with three general rules, which are the
three above.

### 23. Present and the frame pipeline — `folded` for what is ruled; the lane is a design, not a rule
**Rule in force.** **Only an acknowledged presentation advances the picture.**
The presented picture revision is set solely in the presented arm; a textless
present and every skip re-file the frame through `LatestFrameSlot::publish` and
request another turn. Hit testing and hover are re-asked from that revision only
— a submitted or deferred frame does not advance it.
**Not built, and stated so.** The per-window presentation lane was specified
twice — `docs/plans/design/render-handoff-2026-09-16.md` and
`docs/plans/design/present-never-blocks-input-2026-09-20.md` — and built neither
time; both notes say design only, nothing was run. **The swapchain present mode
has no owner**: `configure_window_surface` starts from the surface's default
configuration, which takes the backend's first-listed mode, so the two platforms
run different modes from one source line and nobody chose either.
**From.** §7.1 ⑬; §2.2 item 4 ② *a textless present is a frame still owed*; the
two design notes, whose own §8 rules over their earlier sections.
**Overrides.** The present note's §8 withdraws revision 1's suppression of an
empty present, its zero-timeout waitable poll, and its force-a-present-after-N
rule.

### 24. GPU device lifecycle — `not yet folded`
Entries: §2.2 *the renderer's two layers: device and window*; §7.1.3m ⑤; §7.36
*whether the engine is there must be answered by a real creation attempt within a
bounded time*.

### 25. Fonts and the glyph atlas — `not yet folded`
Entries: §13.22 *glyphs on the Metal road are measured*; §7.1.3l and §7.1.3m.
There is no font charter; the rules are per-decision.

### 26. IME — `not yet folded`
Entries: §7.1.5a″ and §7.1.5a‴; §13.16 *one composition*; §7.34 *a closed window
must leave the screen first, and a float holds the keyboard only when somebody
presses into it*.

### 27. Keyboard routing — `not yet folded`
Entries: §7.1.5 and §7.1.5e; the `shortcuts.rs` module doc; the generated chord
table held by `scripts/check-shortcuts-table.ps1`. **The rung order of
`Runtime::keyboard_input` is today the specification** and it is written nowhere
— that is finding, not rule.

Ruling added 2026-09-22 (owner, keybinding-panel design note, ruling 4; 0.4.4
ticket 06): **a shipped string never spells a chord of the shortcut table by
hand.** A sentence that names a table chord composes it from the one effective
table at the draw (`Shortcuts::accelerator`, in the table's own dialect) — the
`Cards` row's is `i18n::focus_mode_row_in`. Held by
`i18n::tests::no_shipped_string_spells_a_chord_the_table_does_not_produce`, which
scans every `Text` × `Lang` × `HostPlatform`; its allowlist names keys that are
not table rows (the search field's `Shift+Enter`) and may not hold a table chord.
`docs/features.md` names verbs, not keys, and points at `docs/shortcuts.md`. The
row stays `not yet folded` (the rung order is still unwritten).

Rulings added 2026-09-22 and 2026-09-23 (owner, keybinding-panel design note,
rulings 1 and 2; 0.4.4 ticket 04): **a bare `Ctrl`+letter is recordable, at the
recorder and in `keybindings.json`; the row says `shell` instead of being
refused.** The note is on every row whose chord is a bare `Ctrl` and one ASCII
letter, default or recorded, in any scope (2026-09-23;
`shortcuts::takes_a_shell_control_key`, read by `Shortcuts::editor_rows`). The
AltGr zone and "a desktop-wide key needs a modifier" stay refusals. **The default
table does not change**: no two-key-first re-derivation, and a reader who wants
`Ctrl+N` records it once. This supersedes, for recordings, discipline ① of the
2026-08-17 audit. Held by `shortcuts::tests::a_bare_ctrl_letter_is_free_and_the_row_says_shell`,
`the_file_door_keeps_a_recorded_ctrl_letter` and
`every_row_on_a_shell_control_key_says_shell_default_or_custom`. **The terminal's copy and paste answer before the table** (owner, 2026-09-23):
in `Runtime::keyboard_input` the copy rung (`input::should_copy_selection`, a
selection present) and the paste rung (`input::is_paste_shortcut`) are asked
before `Shortcuts::lookup`, so a row recorded on `Ctrl+V` never fires on a
terminal and one on `Ctrl+C` fires only with nothing selected (macOS: `Cmd`;
`Ctrl+Shift+V`/`C` the same). This is kept, not fixed: the terminal's own copy
and paste are never displaced by a recording. Still `not yet
folded`: folding this row (the full rung order of `Runtime::keyboard_input`) is
design-note T5, not this ticket.

Entry added 2026-09-23 (0.4.4 ticket 02; owner's ruling 2026-09-23: "the card
is modal and answers only Enter, the Join key and Esc"): **the multi-line paste
card is a modal rung.** In `Runtime::keyboard_input` it stands directly under the
PSReadLine invitation and above the settings dialog, and it returns for every
key; `paste_card_key` is the whole of what it answers. In `KeyboardOwner` it is
part of `menu_or_dialog`, so `is_modal` is true while it is up and a composition
resolves to `ImeOwner::Modal`.

### 28. Mouse routing — `not yet folded`
Entries: §7.1.5f, §7.1.5g, §7.1.5i; §7.21 and §7.22 *gesture disclosure*; §7.60
*`T-WHEEL-TRACE`: the wheel has no road in a recording, so an aiming question
cannot be settled by reading*. Same finding as row 27 for the rung order of
`Runtime::mouse_input`.

Ruling added 2026-09-23 (owner, next86 touch test, 2026-09-22; 0.4.4 ticket 11),
refining the 2026-09-21 entry *Owner ruling: touch is handed to the system, which
turns it into the mouse*: **the system recognises every gesture; Folio answers one
of them, the pan, as the wheel.** The touch door asks for single-finger pan with
the system's gutter and inertia (`SetGestureConfig` at window creation) and
answers `WM_GESTURE` / `GID_PAN` by parking its travel — the difference between
successive `ptsLocation`s, `bt_platform::PanTrack` — for
`Runtime::spend_parked_pans`, which moves the pointer to where the pan went down
(`pointer_moved`) and enters `Runtime::queue_wheel` with a `PixelDelta`. No
recogniser, threshold, timer or inertia of Folio's own; no second scroll road.
Every other gesture id, `WM_GESTURENOTIFY` and `WM_TABLET_QUERYSYSTEMGESTURESTATUS`
still reach `DefWindowProc`. A one-finger slide no longer selects text. Held by
`bt_platform::touch_pan::tests::a_pans_travel_is_the_difference_between_successive_points`,
`a_pan_gesture_is_answered_and_every_other_gesture_goes_to_the_system`,
`the_gesture_configuration_enables_single_finger_pan` and
`bt_app::tests::a_pan_enters_the_wheel_road_as_pixels`. Entry: DESIGN
2026-09-23 *A finger sliding over a pane scrolls it*. The row stays `not yet
folded` (the rung order of `Runtime::mouse_input` is still unwritten).

### 29. Attention — `folded`
**Rule.** An **episode** is one unanswered request from one pane.
`bt_app::attention::AttentionLedger` keeps a per-leaf account — strictly
increasing generations per credential, a watermark per answer, one live episode —
and **only the ledger may mint one**, so a generation above the watermark means
unanswered and at or below means dealt with. Three ingress lanes feed it and all
of them name the pane **by capability, never by coordinates**: the weak tier is a
fact about bytes (`OSC 1337;RequestAttention=yes` through
`AdapterEvent::AttentionRequest`, withdrawn by `=no`); the strong tier is the
pane-local endpoint (`folio attention wait`, parked in `attention_wire::INBOX`,
woken by `AppEvent::AttentionSpoke`, drained by `deliver_attention`); the third
is the `folio attention <family>:<event>` verb, which reads two environment
variables, looks up one map row and writes one line. Announced-tier signals — a
bare bell, a one-shot request, the other notification escape sequences, a turn
end — mint nothing and take no place. The routing field is `FOLIO_ATTENTION`
(128 unguessable bits, minted at pane birth, dead with the leaf); `FOLIO_PANE` is
diagnostic and routes nothing, so a hook cannot name a neighbouring pane.
**Seeing is not answering**: the look retires only the bell and failure latches
on a tab that is both active and in a focused window and takes no place away; the
ticket is retired only by `answer_attention` / `answer_attention_in`, **named for
the seat and not the tab**, so typing in a sibling pane does not answer it.
**Reach** is ordered `Nothing < Marks < Flash < Toast`: `notify::desktop_reach`
computes it from whether the tab is active and where the window stands, and
`notify::interruption` turns it into nothing, a taskbar flash, or a desktop
message. A desktop interruption is allowed **at most once per episode** and only
when the reach is the top tier and the notification setting is on.
**From.** `docs/plans/attention/plan.md` — the specification (the capability, the
reach table, the credential tiers, the ledger, the wait bounds, the trace
vocabulary and turn end); §7.1.5b *the session-state taxonomy: one dot, one
assertion*; §7.51 *a turn-end notification must carry the sentence the agent
ended on*; §13.37 *M4-7: the attention endpoint runs over a Unix socket — the
boundary becomes the user, written rather than implied*.
**Overrides.** §7.1.5b's original signal source is struck through in place (the
crate it named never existed; the real sources are the escape-sequence lane and
the pipe, wired 2026-08-25). The plan reopened and replaced the 2026-08-21 "a
bell enters the queue" and the earlier "only Enter dequeues" rulings. The reach
table gained its second tier by the owner's ruling of 2026-09-01. §13.37 replaced
the login-session named pipe with a per-user socket on macOS.

### 30. Notifications and the asking/telling surfaces — `folded`, and the family rule is `nowhere written`
**Rule.** The one written rule in this family is the attention lane's reach rule
(row 29): at most one of the in-window marks, a taskbar flash — once per turn
however many panes spoke, and **absent entirely on a desktop whose taskbar
auto-hides** — or a desktop message, with the notification setting gating only
the desktop arm. **Nothing maps a message kind to a surface kind.** Fifteen
surface types exist before menus and inline fields, eighteen to twenty-one with
them, and their mutual priority exists only as the rung order of the ladders
inside `Runtime::keyboard_input` and `Runtime::mouse_input`.
**From.** `notify.rs` (`desktop_reach`, `interruption`, `Interruption`) and
`Runtime::raise_attention`; §7.1.5o items ③ and ③′ *which two settings rows can
stop an announcement* (owner's ruling 2026-08-26); §7.6 *terminal notifications*.
**Overrides.** The owner's 2026-08-28 ruling removed the flash tier on
auto-hiding taskbars — a request there goes to the desktop instead. The old
three-tier table's second row was retired in favour of the marks tier on
2026-09-01.
**Open.** The kind × urgency × modality → surface table is **to be ruled before
0.5, by the project owner**. See `docs/ARCHITECTURE.md` §8.
**Ruled and not yet built.** Owner, 2026-09-21 (recorded in the multi-line paste
design note's "Owner rulings" 1 and 6): **pane strips become notifications.**
The pane notice strip (`notice.rs`, `NoticeShape::Band`) is to stop being a
surface for news; no ticket has moved a strip yet, so the strips in the tree
today are still strips.
**A datum for the table** (2026-09-22/23, 0.4.4 ticket 02): the multi-line paste
question is a *synchronous gate the reader's own gesture opened* — not a
notification — and its ruled surface is a small **modal card centred on the
window**, in the first-run card's and the dialogs' family, not anchored to the
pane it is about (the pane association is the focus border and the card's own
`→ <shell>`). And a second datum (2026-09-23, ticket 03): where the program can
take the block without running it — PowerShell at an open prompt — the same
gesture raises **no surface at all**; a question is put only where no road
lands the block unexecuted.

### 31. Settings and migrations — `not yet folded`
Entries: the M2 schema document §2; the `migrate.rs` module doc; §7.19 *the words
on the settings page are written for the reader: a copy standard, a forbidden-word
table, and a punctuation gate* (the second §7.19); 2026-09-23 *settings travel as
one exported file* — the bundle's own shape version is `folio_export` 1
(`bt_persist::FOLIO_EXPORT_VERSION`), separate from each part's `schema_version`,
and a part is read by `migrate::parse_document`, the chain its own file is read by.

### 32. Profiles — `not yet folded`
Entries: §7.1.6c-6 *profiles as data*; §7.1.6c-6d *`profiles.json` is followed
live*, which reverses §7.1.6c-6's explicit statement that the file is not watched;
§7.27 *the best shell out of the box, and the invitation bar asks once per run*.

### 33. The three configuration entrances — `folded`
**Rule.** Three entrances, each with a declared audience, and **a configuration
fact declares which one it uses**. There is no precedence ladder between them,
because they are different kinds of input.
*Settings file* — audience: the person using Folio; carries durable scalar
preferences; persists under the data directory; versioned through
`persist::SettingsStore` / `SettingsV1` and a forward-only migration list;
applied in process at its own door and **not re-read from disk during a run**.
*CLI* — audience: the outside world (a shell, Explorer, another Folio); carries
per-launch placement and the six argv doors; never persists; `cli::parse` is pure
and total over the raw arguments, producing a request or a fault, and `resolve`
asks this machine once, at launch.
*Environment* — audience: whoever can already run programs as this user; carries
diagnostics only and grants no privilege; read at process start with no reload;
**set-but-empty is off**, and a name containing `TRACE` keeps the console.
**The contrast that matters**: `profiles.json` and the pins are watched live
through the storage watch on the data directory, re-read behind the shared quiet
window and compared field by field, with an unparseable mid-run file leaving the
live table untouched — while `settings.json` is not. Two reload disciplines
already, before a fourth entrance exists.
**From.** The M2 persistence schema document (two files, unknown-field and
failure policy); §7.2 *the command-line front door* and the `cli.rs` module doc;
`docs/BT-ENVIRONMENT.md`, held complete in both directions by
`bt_app::diagnostics::bt_environment_doc_tests`; §7.1.6c-6 and §7.1.6c-6d.
**The export is not a fourth entrance** — it is the configuration files carried
by hand (owner rulings 2026-09-22: "Export / Import, no WebDAV … import validates through the
same doors as a hand edit and reports faults per row", and
2026-09-23: "ONE JSON file bundling settings + profiles + keybindings + schemes,
with a schema version — readable and diffable"). `Settings > About > Export…`
writes what is in force as one pretty-printed JSON document in a fixed key order
(`bt_persist::export`, `folio_export` 1; each part keeps its own
`schema_version`). `Import…` enters each part by that document's own door:
*schemes* through `parse_scheme`, each bad one named by its file and the rest
written into the `schemes` folder, which is then re-read in process
(`reread_schemes`); *profiles* through the store's compare and
`take_profile_table`, the half of `reread_profiles` a changed file takes;
*keybindings* through this build's defaults and `Shortcuts::apply_overrides`,
each refused line named; *settings* through `parse_document` — an older part
migrated, a future part refused whole — and then **each changed value through the
function a press on its row calls, never by writing `settings.json` and waiting**,
with the store's writes held so the batch lands as one write. A part the file does
not carry is left alone. Three keys are receipts about this machine and are not
imported (`first_run_card`, `powershell_install_pending`,
`cards_gesture_hint_offer`); `Focus mode` and `Offer PowerShell integration` are
imported by pressing the row's own item, their only door; a row this platform
does not have is stored and named. No confirmation before an import — it is the
reader's deliberate gesture — and no network: syncing the file or the folder is a
folder-sync tool's job. `Settings folder` opens the data directory through the
reveal door on the OS hand-off lane.
**Overrides.** §7.1.6c-6d reverses §7.1.6c-6 on watching `profiles.json`.
**Open.** 0.5's outward interface is a fourth entrance and declares its row in
`docs/ARCHITECTURE.md` §9 before it accepts its first flag.

### 34. Bilingual text — `not yet folded`
Entries: §7.19 *the words on the settings page are written for the reader* (the
second §7.19); the copy guide under `docs/plans/ui-style/`; the `i18n.rs` module
doc. Strings are compiled in; the language revision invalidates the caches.

### 35. First run — `not yet folded`
Entries: §7.56 *a card that appears once: four questions about what this machine
lets Folio touch, asked together, with the answers still going through the
settings page*.

### 36. The update check — `not yet folded`
Entries: §7.52 *an installed preview has no way to know it is out of date: one
request, a stamp good for a day, and a gear*; the `update.rs` module doc;
`PRIVACY.md`.

### 37. The Explorer and Finder verbs — `not yet folded`
Entries: §7.4 *the Explorer context-menu verb*; §7.4a *the first-level context
menu on Windows 11 (sparse package)*; §7.4b *two switches make one three-state
row* (owner's ruling 2026-09-07); §13.36 *"Open in Folio" in Finder*.

### 38. PSReadLine — `folded`
**Rule.** Folio ships and installs its own patched copy of the module, because
the stock version on Windows PowerShell 5.1 anchors the edit line to the
pre-resize width, which makes Folio's own resize-anchor chord a no-op. The probe
(`psreadline::run_probe`, its result in `PROBE`) reads the highest installed
version and the effective execution policy **out of process**; the module is
written only into the Documents modules directory that the system names; and
`install_checked` is the one writer — a leaf is Folio's by bytes or by its
version stamp, anything else is foreign and both verbs go dark. **Every switch
outcome must be spoken** — installed, or a refusal with its reason — never
silence.
**From.** §7.1.6c-3b *the PSReadLine invitation: the probe, the trigger table,
byte-identical removal, and the patched version pinned to the integration
script*; §7.47 *a switch that will not move must say why: a new machine's
execution policy is restricted, and that window said nothing from beginning to
end*; trailing entry 2026-09-20 *Folio installs its module only where the place
is empty or its own*; trailing entry 2026-09-21 *Folio's own writers wait their
turn for the marks record*.
**Overrides.** §7.47 replaces the silently refusing switch. The 2026-09-20
occupancy entry replaces row-level checks with `install_checked`.
**The debt this rule carries, from the entry's own admission.**
`Runtime::apply_psreadline`, the three agent-hook rows, `add_to_profile` and
`spend_powershell_intent` — five press handlers — still take the marks lock **on
the window thread**, deliberately, because each already reads and writes those
files synchronously there. Moving them onto workers is named as a 0.4.4 ticket
and **no ticket id has been issued**. See `docs/ARCHITECTURE.md` §5.3 rows 2–4.
Since 2026-09-23 such a press waits behind one of our own workers' I/O for as
long as that I/O takes, instead of failing after two seconds; that wait is
bounded by our own finite writes and is paid off by the same move.
**Ruled and not yet built.** Owner's verbal ruling of 2026-09-21, written down on
2026-09-22 (multi-line paste design note, "Owner rulings" 6): **option A — Folio
replaces its own older copy of the module on upgrade.** No ticket in the 0.4.4
set implements it (`00-INDEX.md`, open decisions); today an older copy of
Folio's own module is shown on the Settings row as installed with a newer
one available (`i18n::psreadline_row_update_in`), and nothing replaces it
without the reader's press.

### 39. Single instance and the launch pipe — `not yet folded`
Entries: §7.59 *a second launch is no longer a second process: it hands its one
sentence to the copy already running, then exits*; §7.59a *a second launch opens a
window, unless you say otherwise* (owner's ruling 2026-09-11); §7.59b *a launch
needs one thing that sees it from end to end*.

### 40. The attention pipe — `not yet folded` (its rule is folded in row 29)
Entries: the attention plan's endpoint section; §7.51; §13.37.

### 41. The uninstall doors — `folded`
**Rule.** Folio writes four classes of thing outside its own folder — edits to
other programs' files (agent hooks, the `$PROFILE` line, the patched module),
system registrations (the Explorer verb, the sparse package, the toast identity),
and its own data roots — and **every one of them must be inert and silent when
`folio.exe` is missing, and must have a non-interactive undo owned by the module
that wrote it**, all reachable through one door, `folio --uninstall-cleanup`
(`--purge` for user data, never by default). Cleanup removes only marks belonging
to **this copy**, compared by executable path, and a mark naming a vanished path
is nobody's and is removed; per-account marks (the `$PROFILE` line, the module)
are removed and reported. **How Folio was installed is read from a written
channel marker, never inferred from a path.**
**From.** `docs/plans/design/clean-uninstall-2026-09-20.md` — §1 what is left
outside, §3 the six committed rules, and §6 *what the reviews changed*, which
states that it rules, together with its closure addendum.
**Overrides.** §6 supersedes §§2–5 wherever they disagree: §3's ownership rule is
replaced by §6's two-kinds-of-mark rule, and §6 corrects revision 1's harm
ranking and its claim about which removal code was new.

### 42. Diagnostics — `folded`
**Rule.** Where a diagnostic goes is decided by one line: **a synchronous answer
to the command just typed goes to the console; a resident asynchronous diagnostic
goes to the log.** The front door borrows the console for its answer only;
everything resident goes to `diagnostics.log` under the data directory, which
rolls to a previous copy at its size limit; the console is kept for the whole run
**only** when some `BT_…TRACE…` name is set. A log file that will not open goes
to the null device, never back to somebody else's shell. Trace lines go through
`trace_sink::Queue` — one writer, a bounded queue, dropped lines counted — while
`diagnostics::note` opens and closes its own handle per line and shares no lock
with a trace. Hang reports go to their own directory, written by the watchdog
before it says anything. **A new `BT_*` variable is admitted only by documenting
it**: `docs/BT-ENVIRONMENT.md` lists every name, its value grammar and exactly
what can land in any file it writes, and the doc-diff test fails on disagreement
**in either direction**.
**From.** §1.5b *the channel split*; §1.5c *trace writes leave the window thread*;
§7.35 and §7.43 item ④ (the run footer and the panic log's path);
`docs/BT-ENVIRONMENT.md`.
**Overrides.** §1.5b's lifetime fix replaced a console adopted for the life of
the process; nothing later reverses it.
**Open.** There is no event model — no event type, no severity, no schema —
behind that plumbing. The ruled shape is in `docs/ARCHITECTURE.md` §10.

### 43. The failure roads — `folded`
**Rule.** A normal quit is a four-phase transaction (`quit::Quit` / `QuitStep`,
advanced by `FolioApp::settle_quit`): ① one application-wide dirty gate over
every dirty preview buffer and uncommitted editor, offering save-all, discard-all
or cancel; ② a read-only photograph of every window; ③ the judged atomic write
(`Runtime::quit_save` and the judged flush — **a refused disk means do not
leave**); ④ retirement, with windows hidden and the loop still pumping only the
browser-exit clock to its deadline. **The two failure roads skip phases ① and ③
entirely.** `FolioApp::fail` — twelve call sites — asks the device-loss latch
first, then prints its stopped line, closes **every** window with the ending flag
through `Runtime::close_window` so that no shell outlives its window, finishes the
application and exits the loop. `install_panic_log_hook` / `install_panic_log_hook_at`
ends the announcing panic by hiding every window of this process through a system
enumeration — not Folio's own window table — then leaves the process with a run
footer, and only for the one thread that won the announcement.
**From.** §7.17 *seven interaction rulings* (the quit transaction's four phases,
"save each, all or nothing"); §7.35 *a closed window waits for its engine to
leave first, and a process that has hosted a web view may not walk out through
`main`*; §7.43 item ④ *a panic leaves by the road a shutdown leaves by*.
**Overrides.** §7.35 supersedes the immediate window removal on ordinary close.
§7.17's button order was overturned by §7.17 item ② on 2026-08-25.
**The fact, recorded because no entry records it.** Whether losing unsaved
preview edits on the two failure roads is an accepted trade-off is
**`nowhere written`**. The dirty gate is structurally unreachable from both
roads; the session snapshot carries paths, names and source kinds and no edited
content. **0.4.4 owns this** — either the preservation transaction in
`docs/ARCHITECTURE.md` §11, or a written ruling with quantified impact under
`CONVENTIONS` §十 rule 7.

### 44. Git status — `not yet folded`
Entries: §7.1.3g; the git backend adjudication document under `docs/plans/`; the
no-polling rule recorded there. Owner: the git worker, its cache and its watch.
Where the git binary is found (`profiles::find_git_on`, DESIGN 2026-09-22
*on macOS the Git page finds git*): Windows — `git.exe` on `PATH`, then
`%ProgramFiles%`, `%ProgramFiles(x86)%`, `%LocalAppData%\Programs`; macOS — `git` on
`PATH`, then `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, where Apple's
`/usr/bin/git` stub counts only when a developer directory holds a git and is
never run to find out; other Unix — `git` on `PATH`, then `/usr/bin/git`.

### 45. WSL — `not yet folded`
Entries: §7.40 *a terminal may not open another terminal at startup, nor boot a
virtual machine in order to write its own title*. `bt_app::wsl` reads the
registry through a trait and starts nothing; the module carries a source gate
asserting it constructs no command and spawns no thread.

### 46. The `bt-platform` boundary — `folded` as the layering rule; the crate's own shape is `not yet folded`
**Rule.** Platform-specific code lives behind `bt-platform`'s interface and no
crate below `bt-app` calls the platform directly; `bt-app` is the one crate that
may ask what platform it is on, and only in the files its own list names. Two
guards, answering different questions about different files:
`scripts/check-portable-core.ps1` over the thirteen named portable crates and
over `bt-app`'s list, and `scripts/check-adapter-boundary.ps1` over the two
vendor-seam files, which may not import a policy crate.
**From.** §13.1 *the rule is one sentence, and it is not new*; §13.2 *the portable
core, named one crate at a time*; §13.3 *two guards, and they are two different
things*; §2 of `docs/CONVENTIONS.md` (policy does not enter the vendor, and not
its facade either).
**Overrides.** none found.
**Open.** The crate is 64,102 lines over 52 files with 19 external dependencies
and five dependents. Whether groups of it leave is debt, not rule — see
`docs/plans/structural-debt.md`.

### 47. The Windows platform layer — `not yet folded`
Entries: §12.2 *PE resources: why the resource file is written by hand*; §7.4;
§13.17. Plus the vendored ConPTY notes under `vendor/conpty/`.

### 48. The macOS platform layer — `not yet folded`
Entries: §13.8 through §13.52 — forty-odd numbered entries, each an M-series or
T-series ticket, written over two weeks. The macOS plan under `docs/plans/port/`
is the index to them.

### 49. The web host — `not yet folded`
Entries: §7.7 through §7.16, the W-series (the web seat's shape, the host and its
input, a page as a preview buffer, a local file as a page, the card, naming by
pane, the site's own icon, the floor, the hole); §7.14 (both of them); §13.29
*`WKWebView` host*. §7.35 carries the teardown rule and is folded in row 43.
Trailing entries: 2026-09-20 *a page that may not fetch may not open a socket*;
2026-09-21 *a previewed local document is not walled off from the network*;
2026-09-23 *a previewed local page reaches the network, and its requests for
files on this machine are the engine's to answer* — the network rule (owner,
2026-09-21) and the two file-read rulings (owner, 2026-09-23: exactly what a
browser allows, no Folio list; a script's `fetch`/XHR of a local file is refused
by the engine, not by Folio's door), which supersede R1-10's "reads its own
folder and reaches no server".
2026-09-23 *a web pane asks its page for Folio's light or dark* (0.4.4 ticket 09):
the page is told `prefers-color-scheme` — WebView2's profile `PreferredColorScheme`,
WKWebView's own `appearance` — from Settings ▸ Appearance ▸ *Web pages* (`Theme`
by default, or `Light` / `Dark`), in the install step before the first navigation and
again on every theme or setting change; nothing else about a page is changed.

### 50. The video engine — `not yet folded`
Entries: §7.23 *video has a face: the first frame comes from the platform decoder,
and the set that can be drawn is not the set that can be played*; §7.42 *video no
longer borrows the browser's mouth*; §7.44 *a recording is the same recording on
every face*; §7.16 *video does not enter the page lane*.

### 51. CI gates and source readers — `not yet folded`
Entries: the scripts themselves; `docs/plans/bt-app-split-inventory-2026-09-21.md`
§5. **The rule worth stating before the fold**: a guard must prove it can go red
(`CONVENTIONS` §三), and when the pins stop naming files the rules they enforce
are stated in prose here, because a guard that outlives the understanding of its
rule becomes superstition.

### 52. Doors — a side effect has one named entrance — `folded`
**Rule.** A side effect with a door has exactly one named entrance and a pin that
keeps it the only one. **File bytes** go through `bt_platform::file_reads` on one
of ten named lanes (`inline_image`, `peek`, `animation`, `preview`, `pdf`,
`git_pipe`, `settings`, `fonts`, `attention`, `other`), with the source guard
`bt_app::file_reads_source_tests` against `file_reads_doors.txt` failing the
build when a product read appears outside an inventoried door. **Child
processes** are constructed only by `bt_platform::quiet_command` /
`quiet_command_named`: silent, an absolute program path resolved beforehand, an
explicit working directory. **Hand-offs to the operating system** — the four
verbs that leave the window — live only in `bt_platform::handoff`, which holds
the workspace's only `ShellExecuteW` and its only platform hand-off sites, and
run on the OS hand-off lane (`bt-app::handoff_lane`), never the window thread;
nothing else in `bt-app` names a door (`no_handoff_runs_on_the_window_thread`).
**Named threads** come only from `bt_platform::spawn_at_priority`, which sets the
band as the new thread's first statement and is the single `unsafe` boundary for
it. **A new side effect gets a door.**
**From.** `docs/BT-ENVIRONMENT.md`'s file-read self-report (the lanes, the budget,
and the exclusions); trailing entry 2026-09-20 *clock-run disk reads — a clock run
is a deadline or an edge, never a poll*; §7.40 item ① *every child process that
does not go through the pseudoconsole comes out of one silent door*; §13.18
*M2-2/M2-4: the four verbs handed to the machine live in one module*; §1.4
*resilience under CPU starvation* (three bands, one spawner); 2026-09-22 *a
hand-off to the system runs on its own lane, and the window that receives it may
take the front*.
**Overrides.** none found.
**The gap, recorded as a fact.** The self-report declares that directory
enumeration and metadata are **excluded from the ledger's accounting**. That is a
statement about what the counters measure; it is not a decision that enumeration
needs no door. The files column's `bt_app::files::read_directory` therefore has
no lane, no door and no guard, and nothing rules whether it should.
`folio-web-thumb` is the matching gap in the thread door.

### 53. Threads, bands and lanes — `folded`
**Rule.** **Three bands**, set as the new thread's first statement through
`bt_platform::spawn_at_priority`: the event and render loop above normal, the PTY
reader normal, **every** worker below normal — files, git and its two pipe
threads, preview, math, image scaling, the OS hand-off lane (`bt-os-handoff`),
the machine probes and the hang watchdog.
Multimedia scheduling is explicitly refused. **A drain turn takes one quantum,
never "until the ring is empty"**: a fixed slice per pane, capped by the
per-turn slice count or the turn's time budget, then back to the pump; and when
the ring ran dry on a read that filled the transport's transfer unit, the turn
does **not** publish and books a short wait for the rest of the burst
(`coalesce::decide`, whose one caller is `Runtime::drain_pty`). **Liveness is
"it should have woken and did not" plus "it does not answer"** — a parked
deadline that expired, and a bounded ask of the window thread — and never "the
loop is not turning".
**From.** §1.3 *the thread and resource model*; §1.4 *resilience under CPU
starvation*; §1.5 *a hang leaves evidence of itself: a resident heartbeat and a
watchdog*; §1.5a *liveness is "should have woken" and "does not answer", not
"busy"*; §1.5c *trace writes leave the window thread*; §1.6 *a drain turn waits
for the rest of a burst the kernel said was coming*.
**Overrides.** §1.5a explicitly supersedes §1.5's turn-counter criterion, which
produced 200 false reports out of its first 205.
**Open.** Which calls may be made on the window thread, the wait-budget table and
the result-return contract are ruled in `docs/ARCHITECTURE.md` §5, not here.

### 54. The look of the window — `folded`
**Rule.** **The current UI is the baseline** (2026-09-22): `docs/design/UI-SPEC.md`
describes the look as it ships, value by value, and does not redesign it. A rule
there is the value most surfaces already use, or the value a ruling below chose;
every constant that differs is a row in `docs/design/UI-DEVIATIONS.md`, and
bringing it into line is the whole of the work — no new values, components or
layout ride along. Four rulings of the same date close the questions the product
used to answer two ways:
- **The terminal pane is square at rest** (2026-09-22). Radius 0, panes flush on a
  1-pt hairline; only while a divider is held do the panes inset 5 and become r8
  cards (`SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX`). The change of shape is the drag
  signal.
- **Two boolean controls, each in its place** (2026-09-22). Settings uses a combo
  reading `On` / `Off` for every boolean and has no switch; the first-run card uses
  switches, with an accent track when on. That track is the one persistent state the
  accent marks.
- **The icon-to-label gap is 8, everywhere** (2026-09-22). The 6s (files row, files
  foot, float head, focus-card head, peek head, git badge), the 7s (pane head, drag
  ghost, palette dot), the 9s (palette row, graph row) and the menus' 10 are
  deviations to 8.
- **Every head title is 11** (2026-09-22). The pane head's 11.5
  (`SEAT_TITLE_FONT_LOGICAL_PX`) is a deviation to 11, the float and glance heads'
  `HEAD_TITLE_FONT_LOGICAL_PX`; 11.5 is not on the type ladder.
- **A site's icon that would vanish into its ground stands on a plate** (2026-09-21,
  colour 2026-09-23). Where a site icon's luminance (measured once, when it is
  learned) and the ground it is drawn on are under 3:1 — WCAG 2.1 SC 1.4.11,
  `marks::SITE_ICON_CONTRAST_MINIMUM` — it is drawn on a circle of its own box, no
  border, no shadow, in the light theme's panel tone `#F7F7F5` in both themes
  (`marks::SITE_ICON_PLATE`), never pure white (2026-09-23). A pale icon on the light
  theme gains little from it (1.07:1), and that is accepted. The ground is whatever the
  frame laid under that box.
- **Decoration never covers text** (2026-09-23). The terminal grid reserves the
  command rail's width when the pane has a rail: the resting band (tick plus its
  padding, inboard of the scroll lane), not the hover crest. A pane has a rail from
  its shell's first mark for the rest of its life, alternate screen included; one
  function, `cmdrail::terminal_grid_for`, turns every seat into a grid.
**From.** trailing entry 2026-09-22 *The current UI gets its written
specification*; `docs/UI-UX.md` §二 (accent is attention, not position), §六 (the
divider drag); §7.28 *the small tags floating over the text wear one outfit: one
face, one hairline, one legible ink*; §7.18 *the icon system: one verb table, one
slot table, one optical gate*; §7.18 *motion tokens: three steps, one travel
distance, two curves, and a register that forbids a fourth* (two entries share
the number); trailing entry 2026-09-23 *Decoration never covers text*; 2026-09-23 *a
web pane asks its page for Folio's light or dark, and a site's icon without contrast
stands on a plate*.
**Overrides.** The two redesign proposals of 2026-09-22 were declined; nothing of
them is a rule. The motion entry's "two curves" predates `GRAB_EASE`; the code's
three are the rule.
**Open.** The deviations are scheduled for 0.4.4 and 0.4.5 by the ticket groups at
the end of `docs/design/UI-DEVIATIONS.md`.

---

## What is folded, and what is not

**Folded (19 rows):** 4, 5, 6, 8, 9, 18, 19, 22, 23, 29, 30, 33, 38, 41, 42, 43,
52, 53, 54. Row 46 is folded for the layering rule only.

**Not yet folded (35 rows):** everything else. For those rows the entries listed
are still the authority, and a ticket that depends on one of them folds it — into
this file, in the same commit.
