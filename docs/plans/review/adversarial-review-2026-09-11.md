# Adversarial code review, 2026-09-11

Reviewed: everything merged to `main` since v0.2.5-preview (tag `251a603`) — nineteen merges,
about 17,000 lines under `crates/` — read at `8ce4dbc` and re-verified at `207b246`, in four
slices.

A. **The in-place Markdown editor.** Whether the block editing that 0.3 exists for can lose or
   corrupt a person's file, and whether the caret model — byte offsets, CRLF seams, wrapped rows,
   provenance both ways — is sound.
B. **Pictures, animations and the render/worker pipeline.** What still stalls, loops or grows
   without bound now that a page holds standing answers, a GIF streams through a bounded ring and
   a draw holds its own texture.
C. **The single-instance launch pipe and the process boundary.** Whether another process on the
   machine can misuse the second named pipe, and whether the handover races.
D. **The files column's write verbs and the rest of the surface.** What the small tickets got
   wrong: making and recycling a row, the inline name box, the rail's hover Open, the Linux job,
   and whether the CHANGELOG, the READMEs and DESIGN describe what the code does.

How: one Codex reviewer read each slice, without a build and without running the program. Every
finding was then re-verified against the code by a separate reader who owns the verdict and the
severity, opens each line rather than trusting the reviewer's anchor, folds duplicates and names
the ticket. **Where the two disagree the verifier wins, and the severities in this document are
the verifier's.** No build, no test and no compiler was run at any point, no window was launched,
and nothing in the repository was modified.

Row ids here are `R<slice>-<n>`: `RA-3` is slice A's finding A3, `RC-2` is slice C's C-2. The
ticket briefs carry the slice's own ids, because they quote the verifier's file:line.

## The numbers

| Slice | Reported | Verified real | Critical | High | Medium | Low |
|---|---|---|---|---|---|---|
| A the in-place Markdown editor | 10 | 10 | 0 | 3 | 7 | 0 |
| B pictures, animations, the worker pipeline | 10 | 10 | 0 | 4 | 6 | 0 |
| C the launch pipe and the process boundary | 7 | 7 | 0 | 1 | 5 | 1 |
| D the files column's write verbs | 9 | 9 | 0 | 2 | 4 | 3 |
| **Total** | **36** | **36** | **0** | **10** | **22** | **4** |

Every one of the thirty-six survived verification: thirty as reported (REAL), six with a cause,
trigger or consequence the reviewer had not got right (REAL-BUT-DIFFERENT — RA-10, RB-6, RC-1,
RD-5, RD-6, RD-8). Nothing was ruled not a defect at the level of a finding; one *sub-claim* was
(RD-8's uncoloured empty draft, which is the stated ruling).

Six severities moved, in both directions: **RA-2 down to medium** (the quadratic needs a document
of thousands of one-character lines, not any 64 KiB paragraph), **RA-4 up to high** (it is not a
race, it is what an ordinary Ctrl+S does), **RB-2 down to medium** (48 MiB is a great many
formulas), **RB-8 up to high** (it is half of the GIF the user reported today), **RC-1 down to
medium** (a stalled listener degrades to the behaviour that existed before the feature, and the
reviewer's UNC and device vectors are refused by the lexical gate already), and **RD-6 down to
low** (an out-of-order listing is a no-op unless a third change intervenes, and the damage is a
lost highlight, not a lost reveal). **RC-7 stays at medium**: the verifier confirmed that the
`ASFW_ANY` wildcard round-trips and that the endpoint can be squatted, and equally confirmed that
a hostile process running as the same user is already outside this channel's stated threat model,
so what it buys is a foreground token and a swallowed launch, not privilege. **RA-3** is the one
row two slices reached: slice A's verifier rated it medium-high, slice B's verifier rated the same
cache high, and it is recorded here as high.

Three of the user's own reports of 2026-09-11 are in this table: the GIF that stalls on switching
and does not start from its first frame (RB-3 + RB-8), the empty new `.md` that cannot be edited
(RA-7, with its producer traced in slice D), and the caret drawn off the character on Chinese
lines — which **no finding covered**, and which slice A's verifier traced anyway while reading
RA-5 and RA-8. It is carried by T-CARET-SEAT.

## Slice A — the in-place Markdown editor

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RA-1 | high | A disk read that lands after a keystroke replaces the body and empties the undo log: `accept` forgets the history in its preamble, before it knows whether anything changed while the read was in flight, and never recomputes `dirty` — so the buffer is left claiming to be dirty with an empty log, and never re-reads again. | typing during a watched file's re-read, or after "Reload from disk" | `crates/bt-app/src/preview.rs:5744`, `crates/bt-app/src/main.rs:61085` | REAL | T-EDIT-DISK |
| RA-2 | medium | Provenance composition asks `run_at` for every run it composes, and `run_at` scans from the front, so composing one paragraph costs lines squared. | a paragraph of thousands of very short unseparated lines | `crates/bt-app/src/preview_provenance.rs:265`, `crates/bt-app/src/preview.rs:3212` | REAL (lowered from high) | left open |
| RA-3 | high | The per-block intrinsic cache keys on an owned copy of the block's whole source and is bounded in entries, not bytes, so every keystroke in a big fence mints another key holding that fence — and the value keeps a copy of its highlighting too. | typing inside a large fenced block or table | `crates/bt-app/src/main.rs:1727`, `:58005`, `:1811` | REAL | T-BLOCK-CACHE |
| RA-4 | high | Folio's own save comes back through the watcher as somebody else's change, because nothing compares the file's present stamp against the `disk_mtime` the buffer already holds; the re-read that follows empties the undo log, and a reader who is typing when the news lands gets a false "changed on disk" strip whose Reload button discards their work by design. | Ctrl+S on a watched file, pause ~300 ms, Ctrl+Z | `crates/bt-app/src/preview.rs:5296`, `crates/bt-app/src/preview_watch.rs:341`, `crates/bt-app/src/main.rs:48025` | REAL (raised from medium) | T-EDIT-DISK |
| RA-5 | medium | A vertical step admits a wrapped row's exclusive end column, which the row lookup assigns to the next row, so Up sticks whenever the desired column exceeds the target row's width — which in a real wrapped paragraph is constantly. | Up in any wrapped block; a click past a wrapped row's last character | `crates/bt-app/src/preview_live.rs:238`, `crates/bt-app/src/preview_edit.rs:259` | REAL | left open |
| RA-6 | medium | A file whose last line has no break ends at a position no block owns, so the caret is drawn under the paragraph at the page's left margin and that paragraph is never drawn as source. | End in a file with no trailing newline | `crates/bt-app/src/preview_live.rs:98`, `:150`, `crates/bt-app/src/main.rs:58858` | REAL | T-CARET-SEAT |
| RA-7 | medium | An empty or text-less Markdown document has no hit target, so a press seats no caret, is read as a press on empty ground and leaves the page, and every key afterwards is dropped. | click the body of an empty `.md`, or of one that is only a thematic break | `crates/bt-app/src/main.rs:54930`, `:54850`, `:55023` | REAL — the user's report today | T-CARET-SEAT |
| RA-8 | medium | An IME composition is turned into paint only by the plain source face, so composing Chinese or Japanese inside a live block shows nothing at all until commit. | compose in a live source block | `crates/bt-app/src/main.rs:56269`, `:58918`, `:5068` | REAL | left open |
| RA-9 | medium | A caret intent recorded on a truncated page is spent unconditionally when the whole-file read lands, and the blur path cannot clear an intent for a surface that never held the keyboard. | click a `.md` over 64 KiB, click a terminal, type | `crates/bt-app/src/main.rs:55035`, `:84632`, `:75916` | REAL | left open |
| RA-10 | medium | An over-cap re-read sets the refusal and leaves the old body editable and stale; nothing ever clears the refusal, so a file that grew past 8 MiB can never be edited again for the life of the buffer — and the answer still passes through `accept`'s preamble, wiping the log for a body it did not replace. | a buffer that bought the whole file, then the file grows past 8 MiB | `crates/bt-app/src/preview.rs:5844`, `:5392` | REAL-BUT-DIFFERENT (narrower trigger, wider consequence) | T-EDIT-DISK |

Also recorded by slice A's verifier and belonging to no finding: **the CJK caret drift**. The
caret's x is `box_of_row[0] + source.advance * column` (`crates/bt-app/src/main.rs:5951`), where
`column` counts a CJK cluster as two cells (`crates/bt-app/src/preview_edit.rs:122`, via
`bt_unicode::cluster_width`) while the row itself is shaped by cosmic-text with
`Shaping::Advanced` and `source.advance` is measured from thirty-two `M`s of the monospace family
(`crates/bt-render/src/lib.rs:6558`). A fallback face whose full-width advance is not exactly
twice the Latin advance drifts the caret by a fraction of a cell per preceding wide cluster, and
the same arithmetic is in the plain source face. This is the user's third report of the day, and
it is in T-CARET-SEAT.

## Slice B — pictures, animations and the render/worker pipeline

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RB-1 | high | A standalone picture pane reads a decode-cache miss as never asked, so a visible working set over the 192 MiB cache sustains a decode → evict → rebuild → re-request loop with no input: the freeze `f353c91` fixed for pages, alive in the consumer that fix did not touch. | three or four visible picture panes or floats whose decoded pixels exceed 192 MiB | `crates/bt-app/src/main.rs:59888`, `:59440`, `:79341` | REAL | T-STANDING-ANSWERS |
| RB-2 | medium | Preview math has the same loop: an eviction removes a ready formula and bumps the generation, and the rebuild that follows resolves every formula again and re-requests the evicted key. | a page whose distinct formula rasters exceed 48 MiB, or zooming one that nearly does | `crates/bt-app/src/main.rs:2026`, `:57741` | REAL (lowered from high) | T-STANDING-ANSWERS |
| RB-3 | high | The GPU layer key names a surface while the upload gate compares a per-animation frame counter, so a surface switched to an already-cached GIF of the same size keeps showing the old file's last picture until the new one's counter climbs past it; the same key collides across windows on the shared context. | switch a pane from a long-running GIF to a cached same-size GIF | `crates/bt-app/src/main.rs:59514`, `crates/bt-render/src/lib.rs:5683`, `crates/bt-app/src/animation.rs:750` | REAL — the user's report today | T-ANIMATION-IDENTITY |
| RB-4 | high | The same intrinsic cache as RA-3, reached from the picture slice: keyed by whole block source, bounded in entries, and the doc comment's "a handful of floats and a line's worth of spans" describes only the value. | typing inside a large fence opened through the whole-file edit path | `crates/bt-app/src/main.rs:1729`, `:58012`, `:1806` | REAL (the defect of RA-3) | T-BLOCK-CACHE |
| RB-5 | medium | The glance card's document lives on `window.peek_pane`, which is in neither the awaited-picture walk nor the invalidation walk, so its pictures stay on placeholders until an unrelated invalidation, and its standing answers survive the watched file moving. | hover a Markdown file with an uncached image | `crates/bt-app/src/main.rs:47833`, `:47918`, `:50529` | REAL | T-STANDING-ANSWERS |
| RB-6 | medium | A decode requested to sharpen a picture is recorded as a request but not as a dependency, so it joins no awaited set and the page keeps the soft picture. | a page whose picture is drawn from a cached smaller raster | `crates/bt-app/src/main.rs:2483`, `:2317` | REAL-BUT-DIFFERENT (stays soft until something else rebuilds the page; not a dead request) | T-STANDING-ANSWERS |
| RB-7 | medium | A resample the scale worker coalesced away leaves a `Pending` raster entry that no completion replaces and eviction skips, so that width draws stretched native pixels for the rest of the session. | two widths for one picture reaching the worker while it is mid-pass on a large Lanczos3 | `crates/bt-app/src/main.rs:955`, `:1035`, `:57905`, `:2626` | REAL | left open |
| RB-8 | high | Animations advance wherever they are cached but are refilled only where they are drawn, so a GIF in a background tab eats its queued second and stands on a dry ring; and `animation::open` stamps the play clock on the worker thread, so a completion landing in a busy turn starts several frames in. Coming back resumes mid-file and waits on a worker round trip. | play a GIF, switch tab or file, wait a second, come back | `crates/bt-app/src/main.rs:59767`, `:59582`, `crates/bt-app/src/animation.rs:456`, `:798` | REAL (raised from medium) — the user's report today | T-ANIMATION-IDENTITY |
| RB-9 | medium | Animation accounting counts the ring and the file but neither the cursor's full-frame compose canvas nor the frames in flight, so peak footprint is about twice the advertised 240 MiB ceiling. | several large animations at once | `crates/bt-app/src/animation.rs:686`, `:509`, `:820`, `crates/bt-app/src/main.rs:24971` | REAL | T-ANIMATION-IDENTITY |
| RB-10 | medium | The block drawn as source is still charged typesetting on every reparse, and the decoration worker is an unbounded FIFO with no coalescing and no priority, so each keystroke in a formula queues obsolete work ahead of the animation fill and the path verification behind it. | type in a display formula while a GIF plays | `crates/bt-app/src/main.rs:57528`, `:1319`, `:1993` | REAL | T-BLOCK-CACHE (caret-block half) + T-ANIMATION-IDENTITY (queue half) |

Read and found sound in this slice: the seven draw loops now holding `Arc<MathTextureTile>` leak
nothing across frames (`crates/bt-render/src/lib.rs:1154`), the hang watchdog's own cost is a
two-second sample over atomics (`crates/bt-app/src/hang_watch.rs:177`), and the release-profile
change is the measurement the commit itself wrote into `Cargo.toml:226`, not a finding.

## Slice C — the single-instance launch pipe and the process boundary

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RC-1 | medium | The grammar's locality test is lexical, and the existence test that follows is an unbounded synchronous stat on the sole listener thread, outside both deadlines, repeated in `commit` and a third time on the window thread. A mapped network drive, a junction whose target is remote, or `\\wsl.localhost\<distro>` naming a stopped distribution stalls every handover for that period — and starting a virtual machine is elsewhere in this codebase called a click the user did not make. | `folio --cwd Z:\team` with `Z:` mapped to a server that has stopped answering | `crates/bt-app/src/launch_wire.rs:206`, `crates/bt-platform/src/launch_pipe.rs:322`, `crates/bt-app/src/cli.rs:387`, `crates/bt-app/src/main.rs:35330` | REAL-BUT-DIFFERENT (lowered from high; the UNC and device vectors are refused already) | T-LAUNCH-STALL |
| RC-2 | high | `Taken` is a syntax verdict, not an admission: the listener answers it without asking whether the window thread is alive, whether Quit has begun, or whether the inbox will keep the request. A frozen Folio answers in microseconds and the second launch exits 0 having opened nothing — which removes the one recovery a person reaches for; a retiring process parks a request the early return never drains; a ninth request in one turn evicts the first. | wedge the window thread, then start `folio.exe` | `crates/bt-app/src/launch_wire.rs:294`, `:327`, `crates/bt-app/src/main.rs:100560` | REAL | T-LAUNCH-ADMISSION |
| RC-3 | medium | The listener is opened in `resumed`, hundreds of milliseconds after the data-directory claim is decided, so a process that lost the claim but found no pipe yet carries on and can create the endpoint itself. Every later launch then lands in a process whose session and settings writes are dropped, and when it closes there is no listener at all. | two `folio.exe` started within the same few hundred milliseconds, the loser reaching `resumed` first | `crates/bt-app/src/main.rs:106506`, `:33884`, `crates/bt-app/src/launch_wire.rs:354` | REAL | T-LAUNCH-RACE |
| RC-4 | medium | Two bugs on one route: the server commits on its own write and disconnects 250 ms later while the client is promised 2 s, so a descheduled client loses the reply and opens its own window over the tab that already opened; and `read_reply` reports `TimedOut` even when the collection succeeded, throwing away a reply that landed in the race window. | a 250 ms deschedule of a freshly started `folio.exe`; a reply arriving within a scheduler tick of the budget | `crates/bt-platform/src/launch_pipe.rs:331`, `:537`, `:641`, `:499` | REAL | T-LAUNCH-ADMISSION |
| RC-5 | low | The request is decoded and accepted twice, by two closures that keep nothing of each other's answer, so a folder deleted between reply and commit makes a successful exit open nothing, and one created in that window makes a refusal open a tab. | a folder appearing or disappearing inside the 250 ms window | `crates/bt-app/src/launch_wire.rs:292`, `crates/bt-platform/src/launch_pipe.rs:347` | REAL | T-LAUNCH-ADMISSION |
| RC-6 | medium | A relative directory is resolved against the launching process's own directory when nothing is running and refused outright when Folio is, so `folio .` opens a shell cold and prints "There is no ." warm — a regression, in a message naming a spelling the person never typed. | `folio .` or `folio ..\sibling` with Folio already running | `crates/bt-app/src/cli.rs:35`, `crates/bt-app/src/launch_wire.rs:94`, `:206` | REAL | T-LAUNCH-ADMISSION |
| RC-7 | medium | The client grants the foreground to whatever pid the peer named and never asks the kernel who answered the pipe: `Reply::decode` admits any `u32`, `0xFFFF_FFFF` included, which is `ASFW_ANY` — the wildcard the call's own comment says in bold it is not. The endpoint name is computable by any process in the session, and a squatter that creates it first keeps it, Folio's own listener failure being swallowed in silence. | a same-session process holding the endpoint name and replying with a chosen pid | `crates/bt-app/src/launch_wire.rs:245`, `:372`, `crates/bt-platform/src/hotkey.rs:436`, `crates/bt-platform/src/launch_pipe.rs:388` | REAL (held at medium: a foreground token and a swallowed launch, not privilege) | T-LAUNCH-ADMISSION |

Read and found sound in this slice: the logon-SID DACL is the attention endpoint's and is asserted
by test, remote clients are rejected, frames are capped at both ends, overlapped operations are
collected before their buffers are reused, profile ids are validated through `profiles::has_id`
before a spawn, quake windows are excluded from the raise and a minimised target is restored
first, and both Explorer entries pass `--cwd`, so the deliberate bare-document exception is
consistent with their argv.

## Slice D — the files column's write verbs and the rest of the surface

| id | Severity | Defect | Trigger | Anchor | Verdict | Ticket |
|---|---|---|---|---|---|---|
| RD-1 | high | A float claims every point inside its frame, but the row lookup consumes that claim for one variant only and every other part falls through to the docked chrome — so a right press on a float's body raises the hidden column's row menu, `Delete` included, drawn over the float, about a file whose name the menu never shows. The ground path has the same hole and raises the covered column's Root menu. | float a preview over a files column, right-press its text, choose Delete | `crates/bt-app/src/main.rs:67307`, `:67387`, `crates/bt-app/src/float.rs:1211` | REAL | T-FLOAT-OWNERSHIP |
| RD-2 | high | The name box scrolls its window forward one character at a time and measures the whole remaining prefix at each step, and each measurement builds a fresh shaping buffer — quadratic text shaping on the window thread, with no bound and no cancellation, now reachable because the field accepts a pasted line of any length. | paste a long single line into the tree's New file box | `crates/bt-app/src/main.rs:18523`, `:89534`, `crates/bt-app/src/text_field.rs:417`, `crates/bt-render/src/lib.rs:10225` | REAL | T-NAME-FIELD |
| RD-3 | medium | An empty folder's column is one notice row, so it has no row geometry, so the ground hit test skips the column entirely — and the Root menu is the only door to `New file…` there. The feature is unreachable in the situation that wants it most, and the CHANGELOG's first Unreleased bullet is false as written. | right-press the ground of a column rooted at an empty folder | `crates/bt-app/src/seats.rs:17221`, `:16146`, `crates/bt-app/src/files.rs:573` | REAL | T-FLOAT-OWNERSHIP |
| RD-4 | medium | A blur that ends the inline name box removes the pending row before the same press is re-resolved, so the click names the row that moved up into those coordinates; a successful commit that re-sorts has the same shape. | leave the New file draft empty and click the first row below it | `crates/bt-app/src/main.rs:84620`, `:84149`, `:63375` | REAL | T-NAME-FIELD |
| RD-5 | medium | Delete calls `SHFileOperationW` synchronously from the menu handler on the window thread, with `FOF_SILENT` — which suppresses the shell's own progress dialog, the very thing that would have pumped a message loop — so a large or slow-volume delete is a silently frozen window. | Delete a large subtree, or a folder on a slow share | `crates/bt-app/src/main.rs:47284`, `crates/bt-platform/src/lib.rs:6248` | REAL-BUT-DIFFERENT (the flags make it worse than stated) | T-RECYCLE-WORKER |
| RD-6 | low | The files worker's traffic carries no request generation, so a listing issued before a create can be applied after it and clear the selection the create set; the locate survives, so the row is still revealed and scrolled to. | a third change to the folder between the last accepted listing and a stale read | `crates/bt-app/src/main.rs:47237`, `:63712`, `crates/bt-app/src/files.rs:354` | REAL-BUT-DIFFERENT (narrower trigger, smaller damage; lowered from medium) | left open |
| RD-7 | medium | A floating tree's rows are given the same menu face as a docked row, and all four write verbs refuse silently because each handler requires a docked column — four rows of a six-row menu that do nothing, with no field, no card and no explanation. | right-press a row in a floating tree, choose Rename or Delete | `crates/bt-app/src/main.rs:67308`, `:47039`, `:47149`, `:47284`, `crates/bt-app/src/profiles.rs:6897` | REAL | T-FLOAT-OWNERSHIP |
| RD-8 | low | (a) The advisory collision test compares names byte for byte while the commit asks the volume, so on an ordinary Windows disk `notes.md` beside `Notes.md` shows no red and Enter does nothing, for ever — which also makes the CHANGELOG's "turns red in the box" untrue for the commonest case. (c) The reserved-device test matches ASCII digits only, so `COM¹` passes. (b) The uncoloured empty draft is the stated ruling, not a defect. | type a case-different duplicate name and press Enter | `crates/bt-app/src/main.rs:76439`, `:47225`, `crates/bt-app/src/files.rs:1362` | REAL-BUT-DIFFERENT (one sub-claim refuted) | (a) T-NAME-FIELD; (b), (c) left open |
| RD-9 | low | The CHANGELOG says the row "goes to the Recycle Bin and never to a permanent delete", while the adapter deliberately pairs `FOF_ALLOWUNDO` with `FOF_WANTNUKEWARNING`: a reader who approves Windows' warning does get a permanent delete. The code is right; the sentence is not. | read the CHANGELOG; delete a file the bin cannot take | `CHANGELOG.md:41`, `crates/bt-platform/src/lib.rs:6248` | REAL | T-RECYCLE-WORKER |

Three notes from this slice's verifier, each folded into a ticket rather than filed on its own:
the three CHANGELOG sentences that outrun the code (RD-3's, RD-8(a)'s and RD-9's) belong to those
tickets; `delete_files_row` does not re-ask the live tree for its key while `open_files_row_rename`
does, which is the cheap guard T-FLOAT-OWNERSHIP should carry; and the new `core-linux` job will
warn on an ungated `NamedVarsRealFiles` in `crates/bt-pty/src/shell.rs:275` whose only users are
`cfg(windows)`, with a comment beside it that miscounts its own ungated tests — noise rather than
a red run, and left open.

Read and found sound in this slice: the file arm uses `File::create_new`, so an existing file is
never truncated; a dirty preview whose file has been recycled keeps its buffer and answers
`SaveOutcome::Conflict` rather than silently recreating the file; the Root subject excludes Rename
and Delete, and the tree's own root cannot be binned; the rail's hover clock raises only the
read-only face, so no disk mutation follows a hover alone; `docs/shortcuts.md` matches `BINDINGS`
exactly; the i18n table is complete, with four declared untranslated entries and real Chinese in
every string added in this range; and `core-linux`'s crate list is byte-for-byte `core-macos`'s.

## Tickets

Eleven tickets. Seven are **before 0.3 ships**; four follow it. Two are already in flight on their
own branches and are marked so; one already has a written brief; the remaining eight have briefs
written beside this review, one file each, self-contained for an implementing agent.

### Before 0.3 ships

#### T-EDIT-DISK — a buffer has no identity for the disk state it holds

RA-1, RA-4, RA-10. Brief: `scratchpad/review2/tickets/T-EDIT-DISK.md`.

Why together: `accept` replaces the body and forgets the history with no regard for what happened
between issuing a read and landing it, and `note_disk_moved` cannot tell Folio's own write from
somebody else's. One shape closes all three — a read tagged with the buffer's incarnation and the
revision it was issued against, disk news reconciled against the `disk_mtime` the buffer already
knows, and `undo.forget()` moved out of the preamble every outcome passes through and into the arm
that actually replaces bytes. RA-10's over-cap answer becomes an explicit state that `is_editable`
refuses and that any later eligible read clears.

#### T-CARET-SEAT — a file position that no drawn piece owns is unreachable (in flight, `fix/source-block-caret-cjk`)

RA-6, RA-7, and the CJK caret drift that no finding covered. In flight: the branch exists and the
work is in the worktree `bt-wt/cjk-caret`; these rows are its scope, and it carries them.

Why together: the end of an unterminated last block, the whole of an empty or text-less document,
and the x of a caret on a wide-cluster line are three questions about where a position *is*,
answered today by three different arithmetics. The document's one insertion position must be a hit
target, the last block must own its own end, and the caret's x must be measured from the shaped
run the row actually drew rather than from a Latin advance multiplied by a cell count.

#### T-ANIMATION-IDENTITY — an animation has an identity of its own, and only a drawn animation advances

RB-3, RB-8, RB-9, and RB-10's queue half. Brief already written:
`scratchpad/mdedit/ticket-gif-switch.md` — it carries the fix, the red tests and the gates, and
this review does not restate them.

Why together: both halves of the user's GIF report are the animation lane failing to distinguish
*this playback* from *that surface* — the layer key names a surface, and the advance walks the
cache while the refill walks the glass. Fixing one leaves half the symptom. RB-9 is the same
object's accounting, and RB-10's queue half is what makes the pause visible.

#### T-STANDING-ANSWERS — the standing-answer rule of §7.1.3u, everywhere a document is held

RB-1, RB-2, RB-6, RB-5. Brief: `scratchpad/review2/tickets/T-STANDING-ANSWERS.md`.

Why together: §7.1.3u's rule — a page keeps the answer it was given, and a bounded cache letting go
of a decode is not the page forgetting that it asked — was written for Markdown pages and is asked
by nobody else: not by the standalone picture pane, not by preview math, not by the sharpening
completion, and not about the glance card's document at all. Each row is one holder reading a
cache miss as "never asked". The fix is one sentence applied in four places: separate *answered*
from *resident*, and enumerate every document holder — `peek_pane`, docked panes, floats — through
one helper.

#### T-FLOAT-OWNERSHIP — a float owns its own ground, and a menu's rows come from the host that must act

RD-1, RD-7, RD-3. Brief: `scratchpad/review2/tickets/T-FLOAT-OWNERSHIP.md`.

Why together: all three are the question "who is this gesture about" answered by pixels rather than
by ownership. A float's claim must be terminal for rows and for ground alike —
`preview_open_pill_at` already implements exactly that rule and says so in its own doc comment. The
menu's row set must be derived from the host's capability, which removes RD-7's four dead verbs
and, out of the same change, gives RD-3's empty root the face it can act on, hit-tested against the
column's body rather than against rows it does not have.

#### T-NAME-FIELD — the name box, from what it accepts to what it says no to

RD-2, RD-4, RD-8(a). Brief: `scratchpad/review2/tickets/T-NAME-FIELD.md`.

Why together: one editor, three ways it is wrong — the window fit re-shapes where it should read
advances, the blur re-derives a choice from stale pixels where it should resolve an identity first,
and the refusal the reader sees is computed by different rules than the refusal that actually stops
the commit. The third is the one that leaves a field sitting there looking valid and doing nothing
for ever.

#### T-LAUNCH-ADMISSION — one admitted launch, decided once and answered for (in flight, `feature/launch-opens-a-window`)

RC-2, RC-4, RC-5, RC-6, RC-7. In flight: the branch exists and the work is in the worktree
`bt-wt/launch-window`, bumping the wire version and moving the tab-or-window decision into
`launch_wire::landing` — it rewrites the very arm that loses the launch, so these rows must land
with it.

Why together: there is no object that owns a launch from admission to visible result. The grammar
decides twice, the server's write stands in for the client's receipt, "parked" stands in for
"serviced", and the reply carries a pid the peer chose. One `AdmittedLaunch` — decided once,
reserved in the inbox once, refused when the UI cannot serve it or the process is retiring,
confirmed by the client before the server commits — closes RC-2, RC-4 and RC-5 together, and
fixing RC-4 alone would make the duplicate window worse. RC-6 is in this ticket because `from_cli`
is what the branch edits, and RC-7 because a wire version bump is the one free moment to drop the
`pid` field in favour of the kernel's answer.

### After 0.3

#### T-BLOCK-CACHE — a cache is keyed by what the content is, and bounded by what it costs

RA-3 / RB-4, and RB-10's caret-block half. Brief: `scratchpad/review2/tickets/T-BLOCK-CACHE.md`.

Why together: the intrinsic cache retains a copy of every historical revision of the block being
typed in — the one block it can never help — while that same block is simultaneously charged
typesetting it cannot use, because it is being drawn as its own source. Content-digest keys with a
stated collision policy, a byte budget over keys and values, eviction chosen by reference, and no
decoration work for the block under the caret.

#### T-LAUNCH-STALL — the launch's locality and existence are decided once, off both threads

RC-1. Brief: `scratchpad/review2/tickets/T-LAUNCH-STALL.md`.

#### T-LAUNCH-RACE — the endpoint belongs to whoever holds the claim

RC-3. Brief: `scratchpad/review2/tickets/T-LAUNCH-RACE.md`.

#### T-RECYCLE-WORKER — a delete is work, and the changelog says what the code does

RD-5, RD-9. Brief: `scratchpad/review2/tickets/T-RECYCLE-WORKER.md`.

Why together: the delete leaves the window frozen and silent while it runs, and the sentence that
describes it promises something the adapter deliberately does not guarantee. Both are answered by
moving the operation onto a worker that reports through the existing toast path, and by writing the
Recycle Bin's real contract in the reader's words.

## Left open

Eight rows carry no ticket in this round. None of them blocks 0.3; each is recorded here with the
verifier's severity, so the next round starts from this list rather than from the reports.

- **RA-2, medium** — quadratic provenance composition. Binary-search `run_at` and carry a forward
  cursor through `through`; the runs are sorted and contiguous already.
- **RA-5, medium** — Up sticks at a wrapped row whose width is under the desired column. The fix is
  wrap affinity in `BlockRows::offset_at`, not a clamp in `step_by_row`. Slice A's verifier would
  have fixed this third; it is left open only because nothing is lost by it.
- **RA-8, medium** — an IME composition is invisible in a live block. Carry the existing
  `PreviewPreedit` into `MarkdownCaretPaint` and paint it from the row geometry the caret uses.
- **RA-9, medium** — a late whole-file read takes the keyboard back after the reader clicked away.
  Stamp the caret intent with a focus generation and drop it on any intervening focus change.
- **RB-7, medium** — a coalesced-away resample leaves an immortal `Pending`. Keep the in-flight
  record outside the ready-raster cache, or have the worker acknowledge the cancellation by
  identity.
- **RD-6, low** — no generation on the files worker's traffic. Tag `DirRequest`/`DirResponse` per
  key and drop answers older than the newest issued for that key.
- **RD-8(b) and (c)** — (b) is the stated ruling and stays as it is; (c) is the superscript device
  names, a one-line completion of a predicate that exists for exactly this class.
- **The `core-linux` warning** — `NamedVarsRealFiles` in `crates/bt-pty/src/shell.rs:275` is
  ungated while its only users are `cfg(windows)`, and the comment above the ungated tests
  miscounts them.

## Method

Four slices, one reviewer and one independent verifier each — half the reviewer count of
2026-09-08, over a quarter of the surface — and the verification step earned its cost again: it
moved six severities, in both directions, and in every case the reason was the *trigger*, not the
code. Two were raised because the trigger turned out to be an ordinary gesture (a save; switching
a tab), and four were lowered because the stated trigger needed a document or a machine nobody
has. That is the same lesson this exercise produced in September: pin every finding to the gesture
a reader actually makes, and the severity follows.

What the verifiers caught that the reviewers did not is worth recording. Three of the six
REAL-BUT-DIFFERENT verdicts were *worse* than reported once traced — RA-10's refusal turned out to
be permanent, RD-5's flags turned out to suppress the progress dialog that would have pumped the
message loop, RD-8's advisory turned out to leave a field that looks valid and does nothing for
ever. Two were narrower. One reviewer's headline (RC-1) rested on vectors the lexical gate already
refuses, while the real vectors — a mapped drive, a junction to a share, a stopped WSL
distribution — were in no report at all. And the user's third report of the day, the caret drawn
off the character on Chinese lines, was found by a verifier reading two *other* findings, and is
in nobody's list.

The recurring root cause of this round is **identity**: something in the window is named by where
it is drawn rather than by what it is. A buffer has no identity for the disk state it holds
(T-EDIT-DISK), an animation is named by its surface (T-ANIMATION-IDENTITY), a cache entry is named
by its whole content because nothing minted it a name (T-BLOCK-CACHE), a launch has no object
between admission and result (T-LAUNCH-ADMISSION), a gesture is resolved against the pixels it
landed on rather than the row it meant (T-FLOAT-OWNERSHIP, T-NAME-FIELD), and a decode answer is
identified with its residency in a cache (T-STANDING-ANSWERS). The last of those is the class that
froze this window on 2026-09-10; it survived here in four more consumers, which is the argument
for writing a rule like §7.1.3u as something every holder asks rather than something one holder
implements.
