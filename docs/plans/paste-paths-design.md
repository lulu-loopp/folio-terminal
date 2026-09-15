# A file on the clipboard, a file on the pointer, and a picture with no name

Design for 0.4.1 — GitHub issues #1 and #2. 2026-09-15, branch
`docs/paste-paths-design` off `main` at `76ca0788`. **Revised five times**:
against `docs/plans/review/paste-paths-review-2026-09-15.md` (24 findings, 7
blocking), then against `docs/plans/review/paste-paths-review-2-2026-09-15.md`
(23 findings, 4 blocking), then against
`docs/plans/review/paste-paths-review-3-2026-09-15.md` (20 findings, 7
blocking), then against
`docs/plans/review/paste-paths-review-4-2026-09-15.md` (12 findings, 4
blocking), and now against
`docs/plans/review/paste-paths-review-5-2026-09-15.md` (6 findings, 1 of them
blocking **T-PASTE-2b and T-PASTE-3's terminal picture delivery only** — that
review finds T-PASTE-1 **ready to open**). §10 carries all five ledgers, and the
rulings below are the
five-times-revised ones. **Docs only**: nothing here is built, no crate is touched,
and every claim about this codebase carries a `path:line` so that the ticket
which implements it can check the claim before it trusts it.

Two requests from one outside reader, and they are one feature:

* **#1** — a file copied in Explorer or Finder should paste into a pane as its
  path, and a file dragged onto the window should insert its path.
* **#2** — when the clipboard holds a picture, pasting should write it to a
  temporary file and paste *that* file's path, which is how a person hands a
  screenshot to a command-line agent.

They are one feature because after the second sentence of #2 there is a file on
the disk and a path to put at a prompt, which is #1. Everything below is
arranged around that: **one ladder that turns whatever the clipboard or the
pointer is carrying into a list of paths, and one encoder that spells a path as
one argument for the program the pane's row starts.**

**What this document promises and what it does not.** It settles rulings and
names the places a ruling has to be *measured* before it can be implemented.
Eleven things are marked **PROBE** and no ticket may assume their answer: they
are collected in **§9.1**, and each of them carries a shipped behaviour for the
build that has not run it. **Nothing here waits on an owner ruling.** The one
question that did — §2.5's namespace derivation — was ruled on 2026-09-15: the
standing 2026-09-07 `(paths, integration)` derivation is kept for both
directions, the program-keyed alternative is rejected and recorded as rejected,
and a Cygwin arm is adopted as an explicitly separate change. No **OWNER RULING
NEEDED** marker remains in this document.

---

## 0. The short answer, before the reasons

**Six rulings carry the whole design.**

1. **The clipboard is asked in a fixed order — files, then text, then a
   picture — and the first rung that *answers* wins.** The order is a
   **preference with losses**, not a proof of what the source meant, so the
   losing case gets a verb of its own: an explicit **Paste picture** row. What
   is *advertised* is read first and only the winning rung's bytes are fetched,
   because fetching is the unbounded part (ruling ④).
2. **A path is always quoted — there is no bare form — and the literal is
   encoded per grammar, over a subset each grammar proves.** A path the UTF-8
   transport or the paste sanitiser cannot carry unchanged is **refused
   visibly**; nothing is ever spelled with `U+FFFD` and no filename control
   character is ever sent.
3. **The grammar is a property of the row's resolved launch, stated as a
   spawn-time default and overridable in `profiles.json`; the spelling keeps the
   owner's standing 2026-09-07 derivation**, which the owner confirmed on
   2026-09-15 (§2.5). This window does not know what program is reading the line
   *now* and will not guess from the screen.
4. **A drop is a gesture of this window's drag system, not a winit event** —
   winit 0.30.13 discards the drop point on Windows, has no `draggingUpdated:`
   on macOS, and reads macOS paths through an `NSString`. The drop lands in the
   verb table that already decides what a dragged row means, which gains one
   cell: **a file on a terminal pane's centre inserts its path.**
5. **A picture becomes a PNG under a vetted, owner-only directory in the
   platform's temp**, named `clip-<stamp>.png`, never overwriting, bounded at
   acquisition and in aggregate, and swept by both age and quota. Its delivery
   carries the identity of the pane that asked for it.
6. **The gesture is plain `Ctrl+V` / `⌘V` / `Shift+Insert`.** No chord, no mode.

---

## 1. What the clipboard is asked for, and in what order

### 1.1 What is there today

`bt_platform::clipboard_text` is two implementations of one sentence. On Windows
(`crates/bt-platform/src/lib.rs:5925`) it opens the clipboard through the owner
window M1-9 gave it, asks `IsClipboardFormatAvailable(CF_UNICODETEXT)`, locks
the handle and copies UTF-16 out; it answers `Result<String, String>`, and an
empty clipboard text is `Ok("")` rather than an error (`lib.rs:5940`). On macOS
(`lib.rs:11358`) it is `NSPasteboard::generalPasteboard` and
`stringForType(NSPasteboardTypeString)`, also `Result<String, String>` — the
`Option` from AppKit is turned into an `Err` with a sentence. That file's own
header already names the gap this design fills: a pasteboard holding only a
picture answers `None`, "the same *no Unicode text* the Windows arm answers when
`CF_UNICODETEXT` is absent", and **"a richer pasteboard is a product decision
nobody has taken."** This is that decision.

`macos_services::paths_on` (`crates/bt-platform/src/macos_services.rs:181`)
reads local file paths off a pasteboard for the Finder Service —
`readObjectsForClasses`, `absoluteString` rather than `-[NSURL path]` (because a
path is bytes, not text), and the reader's own item order preserved. It is a
**model**, not a call site: it collapses *nothing there* and *there but
unreadable* into one empty `Vec` and it prints `BT_MAC_APP` lines that belong to
the Service. The clipboard rung gets a shared decoder extracted beneath both,
which distinguishes the two and logs nothing (§1.3).

### 1.2 Ruling — the rungs, and their order

**One door, `bt_platform::clipboard_payload()`, answering one enum:**

```text
ClipboardPayload::Files(Vec<PathBuf>)        // the source said: these are files
ClipboardPayload::Text(String)               // what clipboard_text() answers today
ClipboardPayload::Picture(Vec<PictureBytes>) // every encoding found, best first
ClipboardPayload::Refused(UnsupportedKind)   // something was offered; we will not read it
ClipboardPayload::Nothing                    // there is nothing on it
```

| Rung | Windows | macOS |
| --- | --- | --- |
| 1 — files | `CF_HDROP` | `readObjectsForClasses:[NSURL]`, file URLs only |
| 2 — text | `CF_UNICODETEXT` | `NSPasteboardTypeString` |
| 3 — picture | the registered `"PNG"` format, then `CF_DIBV5`, then `CF_DIB` | `public.png` |

`Refused` is the **fifth state**, and it exists because `Nothing` and *"you
copied something this window will not read"* are different sentences to the
reader. It is set when a kind is advertised and deliberately not read — today
that is one case, a promise (ruling ⑤). **Among the payload's own five values it
is the only one that raises a toast**: `Nothing` stays silent, so `Ctrl+V` at a
fresh login says nothing at all, and `Files`, `Text` and `Picture` are answers
rather than messages. That sentence is scoped to the payload and to nothing else
— an `Unreadable` rung (ruling ③) and every later failure of the picture lane
report through a channel of their own, which reaches the same toast surface by a
different cause, and §7.2 is the list of them. Saying "only `Refused` routes to a
toast" without that scope would have contradicted both.

**Ruling ①: files win over everything.** A file list is the one format whose
meaning is not in doubt — the source did not offer a *representation* of
something, it named objects on the disk. And on macOS the order is not a
preference but a correctness requirement: a Finder copy also puts the file's
**leaf name** on the pasteboard as plain text, so a text-first ladder would
paste `report.pdf` for a file the reader selected at
`/Users/ann/Papers/report.pdf`.

**Ruling ②: text beats a picture, and this is a preference with a stated loss,
not a deduction about intent.** The preference is worth having because the
common cases point one way: a spreadsheet range, a browser text selection, an
IDE and a word processor all offer a bitmap beside their text as a courtesy to a
program that cannot take text, and answering those with a PNG on the disk would
be wrong every time. **The loss is real and is named here rather than
discovered**: a source that offers a picture *and* text — a screenshot tool
configured to put the capture and its saved path on the clipboard together, a
copy-as-picture out of a spreadsheet, a browser's *Copy image* where the page
also advertised text — can never reach the picture rung under this order. Two
consequences follow, and both are part of the design:

* **A verb for the losing case.** The terminal's right-click menu gains a
  **Paste picture** row, enabled only while a picture rung can answer, and a
  bindable command beside it. It is not a second persistent setting and it asks
  the reader nothing in advance; it is the one gesture that says *the picture,
  please*. It reads the clipboard on the same terms as every other paste (§7.2).
* **A fixture table, not an assertion.** §6.2 carries a source × gesture ×
  advertised-formats × chosen-rung table that has to be *filled in by
  measurement* — Explorer and Finder selections, Excel cells and copy-as-picture,
  Word text and images, a browser text selection and *Copy image*, Snipping Tool,
  `⌃⇧⌘4`, and a screenshot tool configured to offer a path. **PROBE 1.**

**Ruling ③: the rungs are tried in order and the first one that *answers* wins —
there is no merging, and "answers" is defined.** Each rung reports one of three
things:

| | meaning | what the ladder does |
| --- | --- | --- |
| **Absent** | the format is not advertised, or it is advertised and names nothing (an `HDROP` with zero files) | fall to the next rung |
| **Present** | the format is there and readable, including a legally empty string | **stop**; this is the answer |
| **Unreadable** | advertised, and the **acquisition** failed — a lock failure, a delayed render that returned nothing or refused | **stop and report**; do not fall through |

`Unreadable` is an *acquisition* verdict and nothing more. The platform layer
does not decode (ruling ㉚), so it cannot see that a PNG is truncated; the word
"malformed" does not appear in its vocabulary. `Unreadable` does not fall
through because a failed read is not evidence of absence, and a ladder that
treated it as absence would answer a broken file list with a PNG. The reader
gets the toast `recoverable_clipboard_read` already writes the diagnostic line
for (`main.rs:110700`) and nothing is typed.

**`Absent` is not always a survey answer, and the ladder says which are which.**
Two of that table's `Absent` cases are visible only *after* a rung has been
fetched: an advertised `CF_HDROP` that names zero files, and a macOS URL
representation whose items are all non-file URLs. A type list cannot count files.
So the survey is **candidate selection**, not the answer — it narrows the ladder
to the rungs the clipboard advertises — and then, inside the same transaction,
those candidates are fetched **in rung order until one is `Present` or
`Unreadable`**. A second rung is fetched only when the first one's *acquired
content* proved it `Absent`. In the ordinary case exactly one rung is fetched,
which is the exposure claim ruling ④ makes; an empty `HDROP` beside readable text
is the case that fetches two, and §6.1 tests it by name.

**The picture rung is fetched as a list, not as a choice** (revised). The
platform layer copies **every** picture encoding the clipboard advertises, best
first — `"PNG"`, `CF_DIBV5`, `CF_DIB` — inside the one transaction, and hands up
`Picture(Vec<PictureBytes>)`. `bt-app` decodes the first and, if that decode
fails, falls to the next. The earlier draft said the fallback was "PNG
unreadable, try `CF_DIBV5`" and that fallback was **unreachable**: the failure it
exists for is a *decode* failure, which is detected in `bt-app` after the
transaction has closed, and re-opening the clipboard to fetch the second
encoding would be the second snapshot ruling ④ forbids. The list is what makes
the sentence true. Its total size is inside the acquisition bound (ruling ㉕); if
adding an encoding would cross the bound the list is truncated there, and a
decode failure past the end of a truncated list is terminal and says so.

An empty `HDROP` is `Absent`: a list that names nothing is not a list. An empty
text is `Present`: an empty string is a legal thing to have copied, and a paste
of it correctly does nothing.

**Ruling ④: availability is surveyed first, the advertised candidates are
fetched in rung order until one answers, the fetch runs on the event-loop
thread, and Folio sets no deadline on a delayed render.** This ruling has been
rewritten twice. The first draft said the read was "bounded" and booked a probe
to find the number; the second said flatly that there is no timeout at all. Both
were wrong in the same direction — they described Folio's *own* absence of a
deadline as though it were the whole of what Win32 does.

* **Two steps, not one.** First the *advertised type list* is surveyed —
  `IsClipboardFormatAvailable` per format on Windows, `NSPasteboard.types` on
  macOS. **Surveying renders nothing**: it is a cheap, local question. It
  produces the **candidate list**, and the candidates are fetched in rung order
  until one answers. The earlier draft fetched up to five formats inside one open
  and therefore multiplied the exposure by five; this fetches **one**, with two
  stated exceptions — a candidate whose acquired content turns out to be `Absent`
  (the paragraph above), and the picture rung's encoding list (§1.2), which is
  bounded by ruling ㉕.
* **One transaction, and coherence is spelled per platform.** Survey and fetch
  happen inside one `OpenClipboard` … `CloseClipboard` pair — the shape
  `clipboard_text` already has — and the acceptance predicate is **not** the same
  on the two platforms, because the two APIs do not promise the same thing.
  * **macOS.** `NSPasteboard`'s `changeCount` is read before and after. AppKit
    advances it when a pasteboard's contents are declared anew, not when a
    reader materialises a representation, so equality across the read is a sound
    acceptance predicate there and is kept: if it moved, the read is discarded
    and the paste is refused with a toast rather than delivering a mixture of two
    clipboards.
  * **Windows.** `GetClipboardSequenceNumber` is **not** the acceptance predicate
    across `GetClipboardData`, and the earlier draft's before-and-after equality
    test is withdrawn. Microsoft's own contract for that call says that "if
    clipboard rendering is delayed, the sequence number is not incremented until
    the changes are rendered" — so a **successful** first fetch of a delayed
    format is itself a legitimate cause of a changed number, and the test cannot
    tell the event it fears from the rendering it asked for. Refusing on it would
    refuse the very paste that worked, and for text and files as much as for
    pictures. What carries coherence on Windows is the **single open interval**:
    `OpenClipboard` holds other processes off for its duration, and *Clipboard
    Operations* requires a delayed-render owner to fulfil `WM_RENDERFORMAT`
    **without opening the clipboard**, because the requester is holding it open.
    The sequence number is still read once, before the open, and carried into the
    diagnostic line as context for a hang report; it decides nothing.
  * **The forbidden repair.** If a read does come back incoherent the answer is a
    visible refusal. Never a silent reopen — that would deliver a different
    clipboard under the same gesture.
  * **The fixture is the check, and on Windows it checks the exclusion rather
    than a refusal** (revised, fourth review finding 2). §6.2 carries a
    **delayed-render success** row — a source that renders on demand, whose
    *first* paste must succeed — beside a **concurrent-copy** row in which
    another process tries to copy while Folio holds the clipboard: its
    `OpenClipboard` **fails**, Folio's snapshot completes, and its replacement is
    what the **next** gesture reads. The earlier "real replacement under the read"
    row asked for an event this interval excludes; the *interval* is the
    guarantee, so the fixture asserts the interval. macOS keeps a genuine
    replacement-and-refusal row against `changeCount`, because AppKit promises no
    exclusion. The visible refusal above stands for any incoherence that is ever
    actually observed, named with its own failure mode.
* **The thread is winit's event-loop thread**, because that is where
  `register_clipboard_owner`'s window lives (`lib.rs:5845`, `:5927` — "all calls
  run on winit's event-loop thread") and because the paste is a keystroke
  (`main.rs:96290`).
* **Therefore it can stall, and the document says whose deadline that is.**
  `GetClipboardData` on a delayed format sends `WM_RENDERFORMAT` to the owning
  process and blocks until that process's thread answers. **Folio imposes no
  responsive deadline and offers no cancellation**: the call takes no timeout
  argument, and there is nothing on this path the application can interrupt. The
  *system* does have a backstop for that rendering path — an owner that does not
  answer is given up on after about thirty seconds and the requester is handed
  `NULL` — which is a failure mode, not a bound a keystroke can live inside. So
  the first draft's "the read is bounded" is struck everywhere, and so is the
  second's flat "there is no timeout". A browser mid-GC, an Office process on a
  stalled network drive, or a remote-desktop clipboard bridge can freeze this
  window's loop — no frame, no keystroke, no resize — for that long. **The
  mitigation is honesty plus a station**: the acquisition runs inside a new
  `hang_watch` station, `ClipboardRead`, beside the other synchronous
  cross-process calls this loop makes (`hang_watch.rs`: `PtyResize`, `WebPage`,
  `WebRetire`), so a hang report names it instead of pointing at the event loop
  in general.
* **PROBE 8 is redefined.** It no longer promises a bound, because a number
  measured from five sources is not a bound on an arbitrary one and because no
  Folio deadline waits on it. It measures the **distribution** — how long a
  delayed render takes from the common Windows sources — and its output decides a
  *design* question, not a constant: whether the transaction must move off the
  loop.
* **The escape hatch is named, with the cost written down honestly.** If the
  distribution is bad, the whole transaction moves to a worker thread of its own.
  On Windows that is **a cost of this codebase's helper, not a Win32 law about
  reading**: `OpenClipboard` accepts a null `HWND`, and the window requirement
  `lib.rs:5832`–`:5840` records is about *writing* — `EmptyClipboard` on a
  clipboard opened with none sets the owner to null, after which
  `SetClipboardData` fails. Folio's helper routes every open through
  `owner_window()` (`lib.rs:5903`–`:5919`), which insists on a window **of the
  calling thread**, so a worker thread either gets a message-pumping window of
  its own or the helper grows a read-only path that opens with no window. Either
  is a different `clipboard_payload` and a different ticket; it is booked in §9.2
  as a debt with both shapes written down rather than assumed into this one.

**Ruling ⑤: a promise is not read, and refusing it is visible on both paths.**
Windows `FileGroupDescriptorW` + `FileContents`, macOS
`NSFilesPromisePboardType` and `com.apple.NSFilePromiseItemMetaData` are
deliberately **not** rungs in 0.4.1. A promise means *tell me where to put it and
I will write it*: a file this window would have to create in a place it chose,
written by another process, with no format it can name in advance and no size it
can bound — the picture lane's problem with none of the picture lane's
guarantees.

The refusal has to be *sayable*, which the first draft's four-variant enum could
not manage:

* **On the clipboard**, a promise-only clipboard is `Refused(Promise)` — not
  `Nothing` — and only that state raises the toast (§1.2).
* **On a drag**, the promise types **are registered** after all, purely so that
  the destination is offered the drag and can answer `NSDragOperationNone` /
  `DROPEFFECT_NONE` and trace the refusal box. `performDragOperation:` is never
  entered and `receivePromisedFiles` is never called. The earlier draft's "not
  registered at all" made §3.4's "refused while hovering" impossible — an
  unregistered type means the destination never sees the drag, and red line 11
  would have been satisfied only by the absence of any mark, which is not the
  same as a refusal the reader can see.

This is what makes §3.4's browser-image case a measured question rather than a
free success.

**Ruling ⑥: the old door stays.** `clipboard_text()` is not replaced and not
re-pointed. The Markdown editor's paste (`main.rs:60428`), the palette and
search fields, the settings text fields (`main.rs:47228`) and the web address
bar are asking for **text**, on purpose, and a field that answered a copied file
with a path would be answering a question nobody asked. The new door is beside
the old one, and only the terminal's paste walks through it.

### 1.3 The shared decoder

`paths_on` is not called from the clipboard path. A decoder is extracted beneath
it that answers `Absent | Present(Vec<PathBuf>) | Unreadable(reason)`, keeps the
byte-preserving `absoluteString` route and the item order, and **logs nothing**;
`macos_services` keeps its own `BT_MAC_APP` lines, which are about a Service and
name paths a Service was handed. A clipboard decoder that printed a path into
`diagnostics.log` would violate red line 3.

### 1.4 What other terminals do

Windows Terminal reads `CF_HDROP` and pastes the path: it did in 1.18, lost it
in 1.19, and the regression is issue #16627 with PR #16634. That is the whole of
the claim — **no inference is drawn from how long the report took**, which is
report timing and not adoption data.

For a clipboard **picture**, no released terminal in the survey writes a file and
pastes its path. kitty defines OSC 5522 so that the *program* can ask for typed
clipboard data, which is a different shape: the terminal transports, the program
decides. Ghostty discussion #10517 is about **image paste over SSH** and links a
proposed implementation; it is not a statement about Ghostty's local clipboard
today. A WezTerm recipe writing into `/tmp/wezterm-clipboard-images/` circulates
in the community; **it is unverified here** — no revision or canonical URL was
located — and it is cited as folklore, not as a design precedent.

---

## 2. One path, spelled as one argument for the program the row starts

### 2.1 What this window already knows, and the defect it is hiding

There is a path-to-shell-text function in the product today, and it is wrong on
both platforms. `inserted_path_text` (`main.rs:8824`) is what
`Insert path into terminal` (K144) puts at the prompt. It quotes with `"` on
every platform, quotes **only when the path contains whitespace**
(`main.rs:8841`), and reaches the string through `to_string_lossy`
(`main.rs:8840`). Its own doc comment gives the reason for the quote character —
"this window's shells are Windows shells" — which stopped being true at 0.4.0.
Three defects, not one:

* **macOS**: `"…"` in zsh expands `$`, and a backtick or backslash in the name is
  an escape.
* **Windows**: a path with `$` and no space is emitted **bare** into a PowerShell
  pane, and a path with `$` *and* a space is emitted in double quotes, which
  PowerShell expands. `$RECYCLE.BIN` is on every volume.
* **Both**: `to_string_lossy` can hand the shell a *different name*, which is the
  thing `macos_files.rs:33` is written about.

**K144 is repaired in T-PASTE-1**, not deferred behind pictures and drops: it
shares `paste_text` and needs the same encoder. The two rules around its quoting
are right and are kept — a space in front unless the cell left of the cursor is
already blank (`input_line_needs_a_space_first`, `main.rs:8861`, which reads the
*grid* because the shell will not tell us what the input line holds), and a space
after — and so is its focus behaviour (`insert_path_into_terminal`,
`main.rs:76921`, moves the keyboard and the layout focus to the pane it is
filling, because the reader is being sent somewhere to finish a command). §3.2's
drop insertion is **target-specific and moves no focus**; K144 keeps its move.

### 2.2 Ruling — three questions, three owners, and a stated contract

A path becomes text by answering three independent questions:

* **Is it representable?** — §2.4's gate, and it can refuse.
* **Which spelling** — `D:\Demo\a.txt`, `/mnt/d/Demo/a.txt`, `/d/Demo/a.txt`.
  That is `PrintedPathNamespace`'s question
  (`crates/bt-transcript/src/paths.rs:61`), and the answer goes **on that type**
  as `to_pane_spelling`, against its inverse `to_local_path` (`paths.rs:134`).
* **Which grammar** — how a string becomes **one argument** to the program
  reading it. `ShellGrammar`, §2.3.

**Ruling ⑦: the grammar and the spelling are a spawn-time default, and the
document says so in as many words.** Folio knows what the row *started*. It does
not know what is reading the line now, and it will not guess: a reader can type
`nu` into a pwsh pane, `wsl.exe -e fish` is a legal row, and
`shell_integration.rs:248`'s `WSL_LOGIN_SHELL` asks the distribution itself which
shell to `exec` — `bash`, `zsh` or `*) exec "$shell" -l`, which can be fish or
nu. Inferring the foreground program from what is on the screen is the kind of
guess §7.30 spent five revisions removing from the path detector, and it is worse
here because the consequence is an argument, not an underline.

So the contract is: **the encoder is correct for the program the row starts, on a
fresh argument boundary (§5.2), and for nothing else.** Everything that follows —
the override, the refusals, the acceptance rows — hangs off that sentence.

**Ruling ⑧: derived from the row's *resolved* launch, not from its id and not
from the integration choice.** `derive_grammar(&ProgramSource) -> ShellGrammar`
is `derive_integration`'s twin (`profiles.rs:799`) and keeps its two structural
answers: `ProgramSource::PowerShellSeven` is a PowerShell without looking at a
file name, and a `FirstOf` row is read off its first candidate because the
candidates of one row are one program family. `served_by` is **not** consulted:
it can answer `Integration::None` for an ordinary `pwsh` whose owner turned
integration off, and quoting has nothing to do with whether a startup script was
installed.

**Ruling ⑨: one `profiles.json` key for the grammar, one for the spelling, and
no settings-page question.** A row may carry
`"paste_as": "powershell" | "cmd" | "posix" | "fish" | "nu" | "agent"`, and a row
that carries it is believed. **Spelling is a second key, `"paste_paths_as"`,
defined in §2.5** — grammar and spelling are two of §2.2's three questions, and
one key that answered both would have to enumerate their product. It exists because inference provably fails for
wrappers — `wsl.exe -e nu`, a `.cmd` shim around PowerShell, `env`, a Python
REPL profile — and because a reader who has built such a row is exactly the
reader who can name its grammar. It is **not** on the Settings page: it is a
property of one row in a file the reader already edits, like `args` and `env`.
This narrows but does not withdraw the earlier ruling against a setting: Windows
Terminal's `pathTranslationStyle` asks *every* profile, including the ones it
ships, because a WT profile is a command line and nothing else; Folio asks
nobody and offers an override to the rows where its derivation has no evidence.

**Ruling ⑩: an unheard-of program gets the platform's own argument convention,
not a bare path — and it does not inherit `cmd.exe`'s refusals.** On a Unix
host, `Posix`. On Windows, the `Cmd` **encoder** — double quotes under the C
runtime rules, which is what a Windows program's own argv parser reads. The
earlier `Plain` answer for unknown Windows programs is **withdrawn**: it was
reasoned from the agent rows, and the agent reasoning itself was wrong (§2.3).

**The encoder and the interpreter are two things** (revised). `%NAME%` and
`!NAME!` expansion is `cmd.exe`'s command-line *interpretation*; it is nothing to
do with CRT argument parsing. So the `%` and `!` refusals of §2.3 belong to the
**`cmd.exe` row specifically**, not to the `Cmd` encoder, and a `python`, `node`
or custom-tool row on Windows gets CRT quoting with **no expansion refusals**.

**And that is a best-effort default with no literal-identity guarantee, which
this ruling now says out loud.** The CRT convention is how Windows *creates a
process*. It is not the language of whatever reads the pane's input line
afterwards, and a profile row's recipient is usually the second thing, not the
first. A `python` row starts a REPL; the CRT output `"C:\new\test.png"` reaches
that REPL as a *string literal*, whose own grammar reads `\n` and `\t` as
escapes, so the value names a different path and the process's own startup argv
parser never sees the line at all. The earlier sentence claiming that
`D:\Data\100%\report.csv` "works perfectly in those panes" asserted the
process-creation convention as though it were the recipient's interactive
language, and it is **struck**. What ruling ⑩ promises for an unheard-of Windows
program is exactly this and no more: the platform's own argument convention,
always quoted, refusing nothing that only `cmd.exe`'s interpreter would have
refused — a defensible default **at an argument boundary** (§5.2), not a proof
about the reader. A row whose recipient is a language REPL is precisely what
`"paste_as"` exists for, and no Python or JavaScript literal mode is offered
until one is measured and added. §6.2 asserts the `Cmd` encoder against a
**defined CRT command-line consumer** — a native argv printer handed the encoded
command line **with no interpreter between**, by `CreateProcessW` from the test
harness — and not against a Python or Node prompt, and **not through a `cmd`
row**: a `cmd` row's recipient is cmd's interactive line, which refuses `%`
(§2.3) and would be testing the interpreter rather than the encoder. §6.2's own
two rows say which is which.

### 2.3 The grammar table

Every grammar **always quotes**. There is no bare form and no "is this path
simple enough" predicate — that predicate was the whole of finding 1, and the
cost of removing it is quotation marks a reader can see, against a benefit of
never emitting a path that a shell re-lexes into something else. Windows
Terminal's own issue #8109 makes the same argument for drops.

| Row | Grammar | Literal | Escape inside |
| --- | --- | --- | --- |
| `pwsh`, `winps`, `powershell`/`pwsh` stems, `PowerShellSeven` | `PowerShell` | `'…'` | every character PowerShell reads as a single quote, doubled — **PROBE 2** |
| `cmd`, or any row that **names** `"paste_as": "cmd"` | `Cmd` **+ the `cmd.exe` interpreter rules** | `"…"` | trailing backslashes doubled (2N); **`"` refused** (the literal has no escape for one); `%` refused; `!` refused when the row's own args turn delayed expansion on |
| `bash`, `zsh`, `sh`, `dash`, `ksh`, `wsl`, `gitbash` | `Posix` | `'…'` | `'` → `'\''` |
| `fish` | `Fish` | `'…'` | `\` → `\\`, `'` → `\'` |
| `nu` | `Nushell` | `r` + *n* `#` + `'` + path + `'` + *n* `#`; at *n* = 1 that is `r#'C:\Demo\a.txt'#`, at *n* = 2 `r##'…'##` | none; the fence grows — **PROBE 10**, and the arm does not ship at all until a pinned nushell version is measured |
| the seven `AGENT_IDS` rows, **emitted string is a Windows path** | `Agent` | `"…"` | nothing to escape; **`"` refused**, as in `Cmd` — unreachable from a Windows filename, reachable through §2.5's override |
| the seven `AGENT_IDS` rows, **emitted string is a POSIX path** | `Agent` | `'…'` | `'` → `'\''` |
| anything else | `Posix` on Unix, the `Cmd` **encoder only** on Windows — a **best-effort default**, ruling ⑩ | as above | as above — **no `%` or `!` refusal**; the `"` refusal is the encoder's and stays |

**PowerShell — single quotes, and the quote character is not only `U+0027`.**
Inside single quotes PowerShell expands nothing: no `$`, no backtick, no
subexpression. That is why single quotes and not double, and it is the fix for
the K144 defect of §2.1. But *about_Quoting_Rules* documents that PowerShell also
accepts the Unicode quotation marks as quoting characters, and a Windows filename
may legally contain `’`. So the file name
`a’;Write-Host PASTE_PROBE;#.txt` would **terminate an apostrophe-quoted literal
and put command syntax on the line** — a real injection, not a spelling problem.
The encoder therefore doubles **each** character in PowerShell's single-quote
class, not just `U+0027`. **PROBE 2: measure, on Windows PowerShell 5.1 and
PowerShell 7, (a) which code points close a single-quoted string and (b) whether
doubling each of them yields one literal character.** The same probe covers
PowerShell's non-ASCII token separators, which is the other half of the reason
`char::is_whitespace()` is not assumed inert.

**Its fallback is a concrete set, not a promise** (revised). "Refuse the affected
code points" was circular: the affected set is the probe's own *output*, so an
unprobed build could not know what to refuse. So an unprobed build **refuses any
path containing one of `U+0027 '`, `U+2018 ‘`, `U+2019 ’`, `U+201A ‚` or
`U+201B ‛`** in a PowerShell pane, that being `about_Quoting_Rules`' documented
single-quote class as this document reads it, and it ships that way. The probe
can only **narrow** the set, by proving that doubling holds for a member; a code
point it finds outside the list is not a narrowing but a finding, and it stops the
ticket.

**`cmd` — double quotes, and the interpreter and the argv parser are two
consumers.** The double-quoted literal has **no escape for a double quote**:
`\"` is a *CRT* rule, read by a native child's own argv parser after `cmd` has
already finished lexing the line, so it does not protect the interpreter's quote
state. The earlier draft turned that into "`"` is illegal in a Windows filename,
so nothing inside the quotes needs escaping", which is a fact about **one
domain** rather than about the encoder — and §2.5's spelling override, §2.4's
gate and a `"paste_as": "cmd"` row on a Unix host all put strings in front of
this encoder that are not Windows filenames. **Ruling: the `Cmd` encoder refuses
any string containing `"`**, with a toast naming the file, and the same refusal
governs the `Agent` arm's Windows literal (§2.5's composition table, rule C2).
On a Windows path the case stays unreachable, which is why no row has ever hit
it; it is now closed rather than assumed away. What needs care besides:

* **Trailing backslashes: 2N, not one extra.** The C runtime's parser — which is
  what a *native* child of `cmd` uses — reads `\"` as a literal quote, so
  `"D:\"` does not hand the program `D:`; it consumes the backslash, emits a
  literal `"` into the argument and leaves the quote state flipped. N trailing
  backslashes need 2N to survive.
* **A `cmd` builtin does not use those rules.** `cd "C:\a b\\"` reaches `cd` with
  the doubled separator still in it. That is harmless for a path — a doubled
  separator names the same directory — but it is *not* the identical string, and
  the acceptance asserts the **file that opens**, not the text.
* **`%NAME%` expands inside double quotes and cannot be escaped on a command
  line** (the `%%` form is a batch-file rule). **Ruling: a path containing `%` is
  refused in a `cmd.exe` pane — and only there**, not in every pane that happens
  to use the `Cmd` encoder (ruling ⑩), with a toast naming the file and the
  reason. **"A `cmd.exe` pane" means a row whose grammar is `Cmd` because it was
  *named*** — derived from a row that resolves to `cmd.exe`, or written as
  `"paste_as": "cmd"` — **and never a row that fell into the `Cmd` encoder as
  ruling ⑩'s unknown-program default.** An explicit `"paste_as": "cmd"` on a
  wrapper row is the reader naming the program that reads the line, which is what
  ruling ⑨ says a carried key means and the only reading that makes the key usable
  for the wrapper it exists for (`cmd.exe /c …`, a `.cmd` shim); so it selects the
  **interpreter** as well as the encoder, `%` and all. Quoting
  it would hand the reader a mangled name that looks like it worked, and this
  product's standing rule is that a visible refusal beats a silent wrong answer.
* **`!NAME!` expands only under delayed expansion**, which is off by default and
  which this window *can* read for the one case that matters: a row whose own
  `args` carry `/v:on` or `/v on`. A path containing `!` is refused in such a
  row and allowed elsewhere, with the residue stated — a reader who turned
  delayed expansion on from inside the shell is outside what the row can say. It
  reads the same row's `args` whether the `Cmd` grammar was derived or named, and
  like `%` it does not fire for ruling ⑩'s unknown-program default.
* **`^` is not escaped.** Inside a double-quoted argument `^` is an ordinary
  character; escaping it would insert a caret into the name.

**bash, zsh, sh, Git Bash — `'…'` with `'\''`.** Close the quote, write an
escaped quote, open it again. WT issue #18006 is precisely the bug of not doing
this: `D:\John's Archive` became `'/mnt/d/John's Archive'`, where the quoting ends
at the apostrophe and the space after it is bare.

**fish gets its own arm, because the POSIX encoder is wrong there.** Inside fish
single quotes `\\` is one backslash and `\'` is a quote, so a POSIX-encoded
literal containing two backslashes silently loses one, and a backslash before the
closing quote escapes it. The earlier claim that `'\''` "happens to be correct in
fish" was reasoning from one sequence to a whole encoder, which is not a proof.
Fish's encoder is `\` → `\\` and `'` → `\'`, both inside the quotes.

**nushell gets a raw string, in one spelling, with its totality marked as a
probe.** The literal is `r` + *n* `#` + `'` + the path + `'` + *n* `#`, written
`r#'…'#` at *n* = 1 — **one spelling, used everywhere in this document**; the
earlier section carried two (`r#'…'#` and `r#…#'`) and that ambiguity is struck.
A raw string has no escapes at all, and the Nushell book's *Working with
strings* says the fence extends: "Additional `#` symbols can be added to the
start and end of the raw string to enclose one less than the same number of `#`
symbols next to a `'` symbol in the string." So the encoder takes *n* = one more
than the longest run of `#` that follows a `'` anywhere in the path.

**That is a total function only if the language accepts an arbitrary *n*, and
the book states no maximum — which is not the same as there being none.**
Asserting totality from a documented example would be the identical error this
section has just withdrawn for fish ("reasoning from one sequence to a whole
encoder"). **PROBE 10 has two halves and they are independent: (a) on a pinned
nushell version, the maximum fence width the parser accepts, and (b) whether a
raw string is accepted in *argument* position at all** — in a builtin's argument
and in an external command's argument, which are different lexers' problems — the
book documenting raw strings under strings and not under command arguments, and
argument position being where every path this feature emits lands.

**The unrun fallback is a refusal of the whole arm, not a narrower fence**
(revised, third review finding 14). Capping the width at *n* = 1 answers half (a)
and says nothing whatever about (b): a build that shipped width-one literals
before (b) was measured would be shipping an arm that may be invalid in every
position it is used in, which is the error this section keeps refusing to make
elsewhere. So: **until a nushell version is named in this document and both
halves are measured on it, the `Nushell` arm does not ship and a `nu` row refuses
every path with a toast that says the lane is unmeasured.** Once a baseline
version is recorded, width-one literals are enabled in the positions that
version accepted, anything needing a wider fence is refused until (a) is
answered, and §6.1's fence-growth test is gated on the measured maximum rather
than asserted unconditionally. A `nu` row is a row the reader built by hand, so a
named refusal there costs one reader a spelling; an invalid raw string costs them
a wrong file. §6.2's `nu` row is where the baseline is recorded.

The earlier claim that a nu path containing `'` is unspellable is **withdrawn** —
it was false — and so was the sentence that "every shell an account can be set to
takes POSIX single quoting."

**An agent row gets a quoted single token — and the quote character differs by
platform, because the parser's branches do.** The earlier bare-path ruling was
reversed by reading Codex's parser; the second review then said the replacement
was wrong on *both* platforms. It is wrong on one. The branches, at revision
`a8964cb1`, in order:

```rust
let unquoted = pasted.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
    .or_else(|| pasted.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    .unwrap_or(pasted);
if let Ok(url) = Url::parse(unquoted) && url.scheme() == "file" { … }
if let Some(path) = normalize_windows_path(unquoted) { return Some(path); }
let parts: Vec<String> = shlex::Shlex::new(pasted).collect();   // the ORIGINAL
if parts.len() == 1 { … }
```

and `normalize_windows_path` returns `None` unless the string begins with a
drive letter (`X:` then `\` or `/`) or with `\\`. Two consequences, and they
point different ways:

* **A POSIX path is safe in the POSIX single-quoted form.**
  `'/Users/ann/John'\''s Papers/a.png'` has its outer pair stripped into a
  `unquoted` that no branch accepts — it is not a `file:` URL and not a Windows
  path — so control reaches `shlex::Shlex::new(pasted)`, over the **original**,
  which lexes the three pieces back into the one token
  `/Users/ann/John's Papers/a.png`. The single-quoted form is correct here, and
  the second review's claim that the quote-strip branch "wins" does not hold for
  a path the Windows recogniser rejects.
* **A Windows path is *not* safe in that form, and this is the real defect.**
  `'C:\a'\''b.png'` strips to `C:\a'\''b.png`, which **does** begin with a drive
  letter, so `normalize_windows_path` returns it verbatim and Codex looks for a
  file whose name contains `'\''`. The first revision suspected this and left it
  to a probe; it is not a suspicion, it is what the code does.

**Ruling: the `Agent` literal is double-quoted for a Windows path and POSIX
single-quoted for a POSIX path — and the path that decides is the *emitted
string*, the one the spelling produced, not the host path behind it.** The
recipient's branch order is run over what reaches it, and
`normalize_windows_path` asks one question of that string: does it begin with a
drive letter or `\\`. So `/mnt/d/Demo/a.txt` takes the POSIX arm however Windows
its origin, `D:/Demo/a.txt` takes the Windows arm because `X:` then `/` is
exactly what the recogniser accepts, and a path the spelling could not translate
takes the arm its **host** form belongs to, that host form being the string that
is emitted (§2.5's composition table, rule B). §4.4's picture path is the same
rule read on the same string, not a second one. `"C:\a'b.png"` strips to `C:\a'b.png`, which
the recogniser returns unchanged and correctly; `"` needs no escape because it is
illegal in a Windows name; and the double-quoted form is additionally the
spelling Explorer's own *Copy as path* produces, which is what these tools are
most likely to have been built against. The bare form stays withdrawn: on macOS a
bare `/Users/ann/My Pictures/screen.png` lexes to two tokens and attaches
**nothing**.

Four consequences are stated rather than assumed:

* **Prose references and automatic attachments are different things.** Claude
  Code's documented workflow allows an image path written in prose, which a
  *model* reads, and describes `@` for file references; Copilot CLI's
  documentation describes `@`, drag-and-drop and clipboard-image attachment.
  **Neither page says what either tool does with a shell-escaped apostrophe
  sequence**, so the earlier sentence that "quotes are read correctly there" is
  demoted to what it is: an **unverified inheritance claim**, not documented
  parser behaviour, and PROBE 3 is where it stops being a claim. Codex is the
  only one of the three whose parser was read. **The "seven agents take a bare
  path" rule of the first draft had no evidence behind it for six of them.**
* **Several paths in one insertion are not several attachments, and the reason is
  narrower than the first statement of it.** Codex's parser returns `None` when
  *shlex* yields more than one token — but shlex is the **last** branch, so that
  sentence holds only when control reaches it. On Windows a two-path insertion
  can be swallowed earlier: the whole line, quote-stripped, may still begin with a
  drive letter, so the Windows recogniser accepts it as one invalid path and the
  consumer then tries to decode that as an image. So the design says only this:
  **multi-file automatic attachment is not guaranteed**, and what each branch does
  with a multi-path line is a PROBE 3 row rather than a claim. The design still
  inserts both, space-separated and each quoted, because that is the text the
  reader asked for and can edit.
* **Shlex identity is asserted for the POSIX form only.** §6.1 checks an `Agent`
  output by running a shlex over it — and that check is **correct only for the
  POSIX single-quoted arm**, because the Windows arm is not a shlex input: a
  Windows root spells `"C:\"`, whose trailing backslash escapes the closing quote
  under shlex rules and yields nothing like one token. That is not a defect in the
  arm; it is the wrong lexer for it. The Windows arm is asserted against **the
  recipient's own branch order** — quote-strip, `file:` URL, Windows recogniser,
  shlex — with a drive root, a UNC root, an apostrophe and a multi-path line as
  its rows.
* **PROBE 3, and what it gates.** Codex is the **measured** recipient: its
  parser was read at a pinned revision and the two rulings above follow from it.
  The other six rows **inherit Codex's spelling as the default**, and that
  inheritance is a recorded risk rather than a finding. The probe measures, per
  agent and with the version recorded: a path with a space; a path with an
  apostrophe on **both** platforms; two paths in one insertion; and a `file:`
  URI. Where an agent is measured to need a different spelling it gets its own
  arm — the table is per recipient, and `AGENT_IDS` is a starting grouping. **The
  gate on T-PASTE-1 is "measure, or record the inheritance", not "refuse":** six
  rows refusing every path paste would be a product decision this table does not
  carry. **That is the policy for a T-PASTE-1-only release as well as for the
  full one**, and §8 says so in the same words; an unmeasured recipient inheriting
  a measured one's spelling, with the inheritance written down, is the same
  answer at both sizes.

### 2.4 The representability gate

**Ruling ⑪: a path is checked before it is quoted, and what cannot be carried
unchanged is refused visibly.** Quoting does not make a name transportable, and
three separate stages of this window will alter one:

* **`Option<String>` is the transport.** `paste_text` takes `&str`
  (`main.rs:110689`). A Unix path that is not valid UTF-8, or a Windows path with
  an unpaired surrogate, has no `&str`. `to_string_lossy` — which K144 uses today
  — substitutes `U+FFFD` and names a *different file*. **Ruling: `path.to_str()`
  is `None` → refuse, with a toast naming the entry it could not spell.** Never
  `to_string_lossy`, here or in the repaired K144.
* **`sanitize_paste` rewrites some controls and drops the rest**
  (`input.rs:711`). Its five arms, exactly (`:718`–`:728`): `'\r'` **keeps the
  CR and swallows a following LF**; `'\n'` becomes `'\r'`; **`'\t'` is kept as
  itself**; any other non-control character is kept; everything else is
  **dropped**. A POSIX filename may legally contain LF, TAB, ESC or any C0 byte.
  Quoting removes none of them: a TAB would survive into the argument, an ESC
  would vanish and change the name, and a CR delivered to a shell that is *not*
  in bracketed-paste mode is an Enter. **Ruling: a path containing any C0 or C1
  control, DEL, or a line terminator is refused** — TAB included, which the
  sanitiser would have *kept*, and which is the strongest of the three arguments
  for having a gate at all rather than trusting the sanitiser. Bypassing the
  sanitiser for paths is refused too: that is how a bracket terminator or a
  control sequence from a filename would reach the terminal.
* **The earlier sentence "a quoted path has no control character and no newline"
  was simply false** and is struck.

The gate runs **before** the spelling and the grammar, answers
`Ok(&str) | Err(Unrepresentable)`, and is the same gate for a paste, a drop, a
picture's own path and K144. With several files, the representable ones are
inserted and the refused ones are named in one toast — the `paths_on` rule that
four openable folders and one thing this program cannot open is four tabs and a
line, not five refusals.

**This amends an acceptance promise.** §6.2's non-UTF-8 SMB case is no longer
"pastes correctly"; it is "**refuses visibly and names the file**", which is a
row that can be checked.

### 2.5 The spelling, per pane

**Ruling ⑫ (settled by the owner on 2026-09-15): the spelling comes from the
standing `(paths, integration)` derivation, in both directions, and the
program-keyed alternative is rejected.**

`printed_path_namespace` (`profiles.rs:3201`) reads
`(paths(index), integration(index))`, and that shape is not an accident of
implementation — it is **the owner's ruling of 2026-09-07**
(`docs/plans/shell-matrix-2026-09-07.md:365`, branch
`fix/unix-path-spellings-and-cmd-hover`): "`bt_app::profiles::printed_path_namespace`
**derives it from the pair a profile already carries** — Windows directories
behind a bash init file is an MSYS bash, `wsl.exe` is a distribution, everything
else speaks this machine's own spelling … **Nothing reads a profile id, and
nothing guesses from the text.**" The code carries the same sentence
(`profiles.rs:3193`). The first revision of this document replaced that
derivation with one keyed on the program, called the result "a strict
improvement", and **cited neither the ruling nor the fact that it was a
reversal**. The second revision withdrew the change into a proposal and asked
the owner for a ruling. **The owner ruled on 2026-09-15: the standing derivation
stands, for insertion as well as for detection.**

So `printed_path_namespace` is used exactly as it is. T-PASTE-1 does **not** add
a program-keyed `derive_namespace`, does **not** re-point the detector, and is
**not** "a two-direction namespace change" — a phrase the earlier draft used in
§8 while §2.5 said the opposite. §5, §8, §6.1 and all three ledgers now say the
one thing.

**Why the alternative was considered, and why it is rejected** — recorded here so
that the next reader finds a closed question rather than an open one. The
argument for it was a real gap: a `gitbash` row whose owner set integration to
`None` answers `Windows` (`profiles.rs:3219`), so that pane is handed
`'D:\Demo\a.txt'` while every path it prints is spelled `/d/…`; the shell is the
same shell either way, and what the reader turned off was a startup script. The
argument against it, which is the one that won, is two-part. The pair is what the
owner chose, and the ruling's own sentence — "Windows directories behind a bash
init file is an MSYS bash" — is a *definition* of MSYS-ness rather than a proxy
for one. And keying on the program instead means deciding what a `bash.exe` is
from where it sits on the disk, which the Cygwin paragraphs below show to be a
weaker signal than it looks, while changing the **detector** on every Git Bash
pane in a release whose headline is paste.

**The residue is disclosed, not fixed.** An integration-off Git Bash row keeps
today's answer. A quoted Windows path is *tolerable* there — MSYS tools accept
one with backslashes — it is simply not the spelling the pane speaks, and the
detector has the matching hole in the reading direction. A reader who wants the
MSYS spelling without the integration has the per-row spelling override below,
which is one line in a file they already edit.

**Cygwin, which the standing derivation has no arm for, in either of its two
configurations.** A Cygwin `bash.exe` row reaches `printed_path_namespace` as
`paths: Windows` plus whatever integration it carries, and the two configurations
land on two different wrong answers:

* **integration `BashInitFile`**, which is what a `bash.exe` derives
  automatically (`profiles.rs:826`–`:836`), answers `Msys` and spells `/d/Demo`,
  which Cygwin cannot open;
* **integration `None`**, set explicitly, answers `Windows`
  (`profiles.rs:3219`) and spells `D:\Demo`, which Cygwin tools do accept but
  which is not the spelling the pane speaks.

Cygwin mounts drives under a prefix whose default is `/cygdrive`, so neither
answer is the pane's own, and the earlier draft's "every Cygwin row answers
`Msys`" was true of only one of the two. The hole is **not introduced by any
proposal**: it exists today in the detector, where a Cygwin pane's printed
`/cygdrive/d/…` is not recognised and a printed `/d/…` would be recognised as a
file it is not — §7.1.5f's "a mark that answers hover but not click". The
design's own cited precedent needs five values where this codebase has three:
Windows Terminal's `pathTranslationStyle` is
`none` / `wsl` / `cygwin` / `msys2` / `mingw`.

**Ruling ⑫a (adopted, and scoped as a change of its own): a `Cygwin` arm is
added to `PrintedPathNamespace` in both directions, and it is entered only on
positive evidence.** Three things are settled by that sentence:

* **No payload.** The arm is `Cygwin`, not `Cygwin { home }`. The earlier
  spelling carried a `home` field with no stated source — nothing in the profile
  table knows a Cygwin installation's home, and a home is not needed to spell a
  drive-rooted path. Home-based detection is **declined**, rather than left as a
  field nobody can fill.
* **The spelling is the documented default prefix, stated as an assumption.**
  Insertion spells `/cygdrive/<lower letter>/…`. Cygwin's own path guide
  documents that this prefix is **configurable**, and documents a stable
  `/proc/cygdrive` route to whatever it currently is. So `/cygdrive` is a
  **default-mount assumption** of exactly the kind ruling ⑬ makes for WSL — named
  here, not presented as a translation — and a reader who moved the prefix gets a
  path that names nothing until the spelling override below is used. Nothing
  reads `/proc/cygdrive` at paste time: that is a process spawn on a keystroke,
  which ruling ⑬ already refuses for `wslpath`. **And no mount fact is inferred
  in the reading direction**: the detector recognises the prefix this design
  spells and nothing else.
* **Both directions, in one change.** A namespace only one direction knows is the
  hover-but-not-click defect over again, so the detector learns
  `/cygdrive/<letter>` in the same change that teaches insertion to write it.

**The classifier is an installation hint, and its table is complete over its own
outcomes.** The sibling runtime DLL — `cygwin1.dll` for Cygwin, `msys-2.0.dll`
for MSYS2 — says what was installed next to a file; it is **not** proof of the
runtime a given executable links against, and this document does not pretend
otherwise. One fact narrows where it may be asked at all: Folio's shipped Git
Bash candidates are `<Git>\bin\bash.exe` (`profiles.rs:1119`–`:1137`), which that
code names as **the MSYS wrapper the Git Bash shortcut itself runs** — Git for
Windows' own packaging installs a `compat-bash.exe` at that path while the
runtime sits under `usr\bin\` beside `usr\bin\bash.exe`. **A check beside the
wrapper is a check beside a wrapper, and finds neither DLL.** So the rule is a
table over the evidence beside the resolved program *and* beside its
`..\usr\bin\` sibling, and every outcome has an answer.

**Eligibility comes first, and no DLL is read for a program outside it.** A
sibling DLL says what was installed in a directory; it says nothing about a
program that does not print a Cygwin spelling in the first place, and a
`cygwin1.dll` sitting beside somebody's `rsync.exe` or `python.exe` is not a
reason to change how this window spells a path for it. So the classifier asks
one question before it touches the disk: **is the resolved program one of the
shells a Cygwin installation ships and whose printed paths would be Cygwin's
own?** The eligible set is the leaf's lower-cased stem — read exactly as
`derive_integration` reads it (`profiles.rs:826`–`:829`) — in
`bash`, `sh`, `dash`, `zsh`, `ksh`, `mksh`, `tcsh`, `fish`. Every other program
**keeps today's answer with no directory listing and no `stat` at all**, which
is also the cheaper rule: the classifier is a derivation that runs per row.

**Evidence from the two locations is aggregated, not raced.** Each of the two —
the resolved program's own directory and its `..\usr\bin\` sibling — yields a
local verdict: which runtime DLLs it holds, or that it could not be read. A
location that **does not exist** is not a failed read; it contributes nothing.
A location that exists and cannot be listed, or whose entries cannot be stated,
is a **partial read** and stops the classification. Then:

| Evidence, over the eligible program's two locations | Answer |
| --- | --- |
| the row resolved to a Folio-shipped Git Bash candidate (`<Git>\bin\bash.exe`) | today's answer, by the standing derivation; the classifier is not consulted for a program this table already knows, and no location is read |
| the program is not in the eligible set above | today's answer; no location is read |
| **either** location was a partial read | today's answer. The half that could not be read is the half that might have held the contradicting DLL, so an incomplete survey decides nothing |
| both read, and the **union** of the runtimes seen is exactly `cygwin1.dll` | **`Cygwin`** — the one positive match |
| both read, union is exactly `msys-2.0.dll` | today's answer |
| both read, union is **both** — including a Cygwin-only directory beside an MSYS-only `usr\bin\` | today's answer; ambiguous evidence decides nothing |
| both read, union is **empty** | today's answer |

The union is what makes the rule **order-independent**: examining the executable's
directory first and its sibling first give the same answer, which a
first-match-wins rule over the two locations would not. And every row but the
positive one is today's answer, so a mixed installation loses nothing it had.
This is a derivation, not a gesture, so none of these rows raises a toast.

**Until PROBE 11 has run the whole arm is dark — the automatic match and the
explicit override alike.** An unrun probe must not switch on an unverified
positive match, so an unprobed build answers "today's answer" in every row above.
And `"paste_paths_as": "cygwin"` is refused in that state too, with a toast
saying the Cygwin spelling is unmeasured — the same shape as the `nu` row's
unmeasured-lane refusal (§2.3), and for the same reason: **one gate, not two.**
An arm that is dark for the classifier and live for the override is not dark.
The remedy costs the reader who wrote that key one line: `"windows"` spells
`D:\Demo\a.txt`, which §2.5's own Cygwin paragraph records that Cygwin tools
accept. PROBE 11 confirms the test on a real Cygwin installation and a real MSYS2
one, and records what actually sits beside the Git for Windows wrapper; §6.2
carries the wrapper row, the integration-off Cygwin row, the ambiguous both-DLL
row, the non-shell row, the cross-level mixed row, the partial-read row, the
unrun-override row and a moved-prefix row.

**Where this change lives: its own ticket, T-PASTE-CYG** (§8). It adds a variant
to `PrintedPathNamespace` — `paths.rs:61` has none today — teaches the detector a
new prefix, and changes what non-pasting panes underline, which is a surface
outside this feature. T-PASTE-1 neither waits for it nor ships any part of it,
and it is gated on PROBE 11.

**MinGW is deliberately not an arm**: `mingw` in WT's list means
the same Windows paths with `/` separators rather than a different namespace, and
a quoted Windows path already works there — so there is nothing for a namespace
to do.

**The per-row spelling override — `"paste_paths_as"`, and it is the only thing
that changes a row's spelling.** Ruling ⑨'s `"paste_as"` names a row's *grammar*
and cannot express a spelling: a `gitbash` row switched to `"paste_as": "cmd"`
changes its quotes while its namespace stays `Msys`, and left at `"posix"` it
still spells `/d/…`. So a row may additionally carry:

```json
"paste_paths_as": "windows" | "windows-slash" | "wsl" | "msys" | "cygwin"
```

Four values name namespaces this codebase has or gains; the fifth,
`windows-slash`, is a Windows path written with `/` separators — `D:/Demo/a.txt`
— which is the one spelling no namespace produces and which ruling ⑭ needs. The
key is **insertion only**: it never reaches the detector, which keeps reading the
pane by the standing derivation, and it changes no environment variable (red line
12). A row that carries it is believed, exactly as `"paste_as"` is — within the
domain ruling ⑫b's rule E gives it — and it is not on the Settings page for
ruling ⑨'s reason. `"paste_paths_as": "cygwin"` is accepted only once
T-PASTE-CYG has added the arm **and** PROBE 11 has run: before the arm exists the
value is refused with the same message an unknown key gets, and while the arm is
dark it is refused with the unmeasured-spelling message the classifier's own
paragraph defines.

So the configuration ruling ⑭'s fallback promises is one row, written out:

```json
{ "id": "gitbash", "paste_paths_as": "windows-slash" }
```

which emits `'D:/Demo/a.txt'` — POSIX single quotes, because the row's grammar is
untouched, around a Windows path spelled with forward slashes. Nothing about the
reader's environment or the detector moves. That is exactly what PROBE 4's unrun
fallback names, and §6.2 asserts it as a row.

#### The composition contract — grammar × spelling

**Ruling ⑫b: the two keys compose over one string, and the product is specified
rather than left to be inferred.** Two independent keys make thirty
configurations, and the third review's "the two are asserted independently" fixed
only that neither key reads the other's value. It did not say what a `wsl`
spelling means on a row that is not WSL, which quoting arm a translated string
takes, or whether an explicit `cmd` grammar brings cmd's interpreter with it.
Those are the questions an implementer would otherwise have to invent an answer
to, so they are answered here.

**Rule A — the order, and what each stage may see.** The spelling runs first and
produces **one string**; the grammar then encodes *that string*, and the only
path-shaped input it reads is that string — not the host path, not the row's
namespace, not the live pane. §2.4's representability gate runs ahead of both, on
the host path, unchanged. This is what makes the table below short: a cell is a
composition of two stages that do not consult each other's inputs.

**And the encoder is a configured encoder, not a bare grammar tag** (revised,
fifth review finding 3). The fourth revision wrote rule A as "the grammar reads
nothing else", which read literally erases the very inputs rules C3 and C4 need:
`ShellGrammar::Cmd` plus a string cannot distinguish a **named** `cmd` row from
ruling ⑩'s unknown-program default, cannot see whether the row's `args` carry
`/v:on`, and cannot know whether PROBE 2 or PROBE 10 has been answered — yet the
same emitted string must be refused in one of those cases and accepted in the
other. So the honest statement is: **spelling produces the string that a
*configured* encoder consumes, and the configuration is captured at the gesture
or spawn boundary**, not looked up inside the encoder. That configuration is
exactly three things beside the grammar tag:

* **the `Cmd` origin** — named (a row resolving to `cmd.exe`, or
  `"paste_as": "cmd"`) versus ruling ⑩'s unknown-program default, which is what
  C3 keys on;
* **the delayed-expansion flag** read from that row's own `args` (`/v:on`,
  `/v on`), which is what C3's `!` half keys on;
* **the applicable probe policy** — PROBE 2's recorded quote class for C1, PROBE
  10's measured baseline for C4, PROBE 11's state for the `cygwin` column.

Given those, the encoder is still **pure**: it never queries the live pane, never
re-reads the profile row and never sees the original host path. Rule A's real
content survives intact — two stages, one string between them, no back-channel —
and what changes is only that the second stage's policy arrives with it instead
of being fetched. **The fixtures say so directly**: `D:\Data\100%\r.csv` and
`D:\Data\a!b.txt` are each encoded under **both** `Cmd` origins — refused under
the named row, accepted under the unknown-program default with the identical
emitted string — and the `!` pair is run once with `/v:on` in the row's `args`
and once without, so the flag is shown to be the thing that decides.

**Rule B — what the spelling produces, and what context it needs.** Every value
is a **lexical** rule over the host path, by ruling ⑬'s own separation of lexical
rules from mount facts:

| `paste_paths_as` | Emitted string's kind | Context it needs, and where it comes from |
| --- | --- | --- |
| `windows` | Windows | none |
| `windows-slash` | Windows, `/` separators | none |
| `wsl` | POSIX | a **distribution name**, and only for `\\wsl.localhost\<d>\…` / `\\wsl$\<d>\…` inputs, which are translated only when `<d>` is this row's own. Source: `wsl_distribution(index)` (`profiles.rs:3237`–`:3243`) — the row's own `-d` / `--distribution` argument, else the machine's default from `wsl::facts()`, three registry reads at startup and no process. On a row that carries neither, it is `None`, and then a distribution share is **not translated** and falls to rule D. Drive-rooted paths need none of this. **`home` is never needed**, because ruling ⑬ never emits `~` |
| `msys` | POSIX | none. `Msys { home }` (`paths.rs:75`) carries a home for `~` in the *reading* direction; insertion never writes one |
| `cygwin` | POSIX | none — the `/cygdrive` prefix is ruling ⑫a's stated default-mount assumption. **T-PASTE-CYG only**, and dark until PROBE 11 |

So a `wsl` spelling on a wrapper row, a `pwsh` row or any other non-WSL row is
**accepted** and spells drive-rooted paths by the mount rule, with the
distribution taken from the row or the machine exactly as a real WSL row's is.
Nothing is invented and nothing is guessed: the same function answers for both.

**Rule C — the refusals, which are properties of the emitted string and fire in
every cell of a grammar's row.** C1: PowerShell's documented single-quote class
(§2.3), refused whole until PROBE 2 narrows it. C2: a `"` anywhere in the string,
under `Cmd` and under `Agent`'s Windows arm, because those literals have no
escape for one (§2.3). It is reachable three ways, all of them through the
override: a POSIX spelling of a distribution-internal path whose Linux name
carries a `"`; rule D's host fallback for that same path, which is a UNC string
carrying one; and a `"paste_as": "cmd"` row on a Unix host. It is closed by the
encoder rather than by a claim about which characters a filename may hold. C3: `%`, and `!` under a `/v:on` row,
under a **named** `cmd` grammar only (§2.3), never under ruling ⑩'s
unknown-program `Cmd` default. C4: the whole `Nushell` arm, until §2.3's baseline
is measured.

**Rule D — the fallback string is the host path, and the grammar reads it the
same way.** When a spelling has no translation for an input — a UNC share, a
foreign distribution's share, a `wsl`/`msys`/`cygwin` value in front of a path
that is not drive-rooted — ruling ⑬'s existing answer stands: **the host path is
emitted, quoted.** Its kind is the host's, so rule A's "the grammar sees the
emitted string" resolves the branch without a second rule. That is how the
`Agent` arm is selected (§2.3), and it is the reason §4.4's picture path cites
the spelled string rather than the temp path.

**Rule E — the key is a Windows key.** `PrintedPathNamespace`'s translation is
`#[cfg(windows)]` (`paths.rs:132`–`:134`), and all five values name Windows-side
spellings of Windows paths. On a Unix host **every value is refused where the row
is read**, once, with the message an unusable key gets, and the pane keeps the
host's own spelling. One refusal at load beats five refusals per paste.

**The product.** Rows are grammars, columns are spellings; the cells say only
what differs, since rules A–E cover the rest. Every cell not naming a refusal is
**accepted**.

| Grammar ↓ / spelling → | `windows` | `windows-slash` | `wsl` | `msys` | `cygwin` |
| --- | --- | --- | --- | --- | --- |
| `PowerShell` | `'…'`, C1 | `'…'`, C1 | `'…'`, C1 | `'…'`, C1 | C1; CYG |
| `Cmd` | `"…"`, 2N trailing `\`, C3 | `"…"`, C3 | `"…"`, **C2**, C3 | `"…"`, **C2**, C3 | **C2**, C3; CYG |
| `Posix` | `'…'`, `'`→`'\''` | as left | as left | as left | CYG |
| `Fish` | `'…'`; every `\` doubles, so `D:\Demo` is `'D:\\Demo'` | `'…'` | `'…'` | `'…'` | CYG |
| `Nushell` | C4 | C4 | C4 | C4 | C4; CYG |
| `Agent` | `"…"` (Windows arm), **C2** | `"…"` (Windows arm — `X:` then `/` is what `normalize_windows_path` accepts), **C2** | `'…'`, `'`→`'\''` (POSIX arm) | as left | as left; CYG |

"CYG" is rule B's last row: the whole column is T-PASTE-CYG's and refuses while
PROBE 11 is unrun. A cell whose spelling could not translate the input is rule
D's, and is then read as that host string's own cell in the same row — a UNC path
under `Agent` × `wsl` is the `Agent` × `windows` cell, C2 included.

**Fixtures for the product** (§6.1, and §6.2 where a program must run). **Every
cell naming a refusal rule is written as a pair, because the rule's own state is
an input** (revised, fifth review finding 2): a pure fixture **selects the probe
policy explicitly** rather than inheriting whatever the build happens to carry,
and each side is asserted. `D:\John's Archive\a.txt` × `wsl` × `PowerShell` is
that pair — **with PROBE 2 unrun the paste is refused by C1** with the documented
quote-class toast and **nothing is inserted**, because the apostrophe is in the
refused set; **with the recorded result that doubling holds for `U+0027`** the
same input emits `'/mnt/d/John''s Archive/a.txt'`, an apostrophe doubled inside a
POSIX string under a Windows grammar. The third revision asserted only the second
half unconditionally, which contradicted §2.3's shipped fallback; both halves are
the fixture now, and T-PASTE-1's gate is satisfied by whichever one the build's
policy selects. The drive
root `D:\` × `Cmd` across the columns → `"D:\\"`, `"D:/"`, `"/mnt/d/"` — the 2N
rule fires in one column only, because it is the only one whose string ends in a
backslash. `D:\` × `windows` × `Agent` → the `"C:\"`-shaped literal §2.3 exempts
from shlex identity. `\\server\share\a.txt` × `wsl` → rule D, the host string,
so `Agent` takes its Windows arm. `\\wsl.localhost\Ubuntu\home\ann\a"b.txt`
× `msys` → rule D (MSYS has no distribution share), a host string containing `"`,
which `Posix` accepts and `Cmd` and `Agent`-Windows **refuse** by C2. The same
path × `wsl` on that distribution's own row → `/home/ann/a"b.txt`, accepted under
`Posix` and refused under `Cmd`. `{"id": "gitbash", "paste_as": "cmd",
"paste_paths_as": "msys"}` with `D:\Data\100%\r.csv` → **refused**, because `cmd`
was named; drop `"paste_as"` and the same row emits `'/d/Data/100%/r.csv'`. And
any `"paste_paths_as"` on a macOS row → refused at the row by rule E, the pane's
spelling unchanged.

**Ruling ⑬: the mount rule, not `wslpath`, and the translation is partial over a
normalised domain.** A WSL pane is handed `/mnt/d/Demo/a.txt` and a Git Bash pane
`/d/Demo/a.txt`, by the inverse of the rule `to_local_path` already reads.
`wslpath` is refused: running it means spawning a process inside the
distribution on every paste, against a latency budget shared with a keystroke;
and writing `$(wslpath …)` into the reader's command line puts a command in their
history that they did not type. **Never inject a substitution into the line.**

What the inverse is, exactly — because the first draft claimed a universal
round-trip it cannot have:

* **Lexical rules and mount facts are separated.** The inverse is *lexical*: it
  maps a drive-rooted Windows path to `/mnt/<lower letter>/…` or `/<lower
  letter>/…`, and a `\\wsl.localhost\<distro>\…` or `\\wsl$\<distro>\…` path of
  **this pane's own distribution** to the path below the share. It knows no mount
  facts, exactly as `drive_mount_to_local_path` (`paths.rs:218`) knows none in the
  other direction.
* **Therefore it is best-effort and says so.** A drive that the distribution has
  not mounted — `automount` disabled, `automount.root` moved, a drive mounted by
  hand elsewhere — is spelled `/mnt/d/…` and names nothing. This is disclosed as
  a **default-mount assumption**, not presented as a translation. A future ticket
  may cache real mount facts from a measured probe; that is named as the upgrade
  path and is not in 0.4.1.
* **The legacy alias is read.** `\\wsl$\<distro>\…` is accepted on input even
  though `paths.rs:235` does not emit it: a path arriving from Explorer or a drop
  comes from outside this program, and the producer's habits are not ours.
* **The ambiguity is named.** `\\wsl.localhost\Ubuntu\mnt\d\x` translates to
  `/mnt/d/x`, which reads back as `D:\x`. The round trip is not an identity there
  and the test suite says so explicitly rather than excluding the case quietly.
* **Drive letters are normalised**, so the round trip is an identity over a
  **normalised** domain: `d:\x` and `D:\x` both spell `/mnt/d/x` and both read
  back as `D:\x`.
* **A path with no translation is spelled in this machine's own form, quoted.**
  A UNC share, a different distribution's share, an MSYS path outside the drive
  mounts. `to_local_path` refuses rather than guesses and that is right for
  *reading*; here refusing means pasting nothing, which is worse than pasting a
  path the shell cannot open. The reader sees what they dragged, inert inside its
  quotes.
* **Absolute always; never `~`.** A quoted `~/…` does not expand, so emitting one
  would produce a literal `~` directory. A literal `~` inside a path is just a
  character and is quoted like any other.

**Ruling ⑭: Git Bash converts again after us, and that is measured rather than
prevented.** MSYS2 rewrites POSIX-looking arguments when it starts a **native**
Windows child, and `MSYS2_ARG_CONV_EXCL` disables that globally or by prefix;
quoting does not turn the stage off. So `/d/Demo/a.txt` is right for an MSYS tool
and right for a native child under default settings, and *wrong* for a native
child in a shell whose owner set an exclusion. **Folio does not change the
reader's environment to force its own answer.** §6.2 carries rows for an MSYS
consumer and a native consumer with exclusions unset and set; if a native
recipient turns out to need it, a forward-slash Windows spelling (`D:/Demo/a.txt`)
is the documented fallback for that row — through the `"paste_paths_as"`
override just defined, not by guessing, and not by exporting anything into the
reader's shell. **PROBE 4.**

### 2.6 Several files

**Ruling ⑮: space-separated, each spelled, gated and quoted on its own, in the
order the source gave them.** The order is load-bearing and already ruled so.
K144's leading and trailing space rules apply to the whole run: one space in
front if the cell left of the cursor is not blank, one space at the end.
`cat 'a b.txt' 'c d.txt' ` is what a person would have typed. In an `Agent` pane
the same run is inserted with the same quoting, under §2.3's stated limit on what
a second path means.

---

## 3. The pointer's half: drag and drop

### 3.1 winit's events cannot carry a drop, and the door has to be ours

`DroppedFile`, `HoveredFile` and `HoveredFileCancelled` appear nowhere in this
workspace, and `with_drag_and_drop` is never called. Reading both backends of
winit 0.30.13 in `~/.cargo/registry`:

* **Windows discards the drop point in all three callbacks.**
  `platform_impl/windows/drop_handler.rs` takes `_pt: *const POINTL` in
  `DragEnter` (`:83`), `DragOver` (`:109`) and `Drop` (`:137`) and uses none of
  them — so not even the entry callback carries a coordinate.
  `DroppedFile(PathBuf)` carries a path and nothing else.
* **A multi-file drop is N events with no batch marker**, on both platforms.
* **macOS implements the dragging methods on `WindowDelegate`, not on a view.**
  `platform_impl/macos/window_delegate.rs:367` is
  `unsafe impl NSDraggingDestination for WindowDelegate` inside a
  `declare_class!`, and the registration is `window.registerForDraggedTypes`
  (`:666`) with the deprecated `NSFilenamesPboardType`. **The first draft named
  the content view; that was wrong**, and a `class_replaceMethod` aimed at a view
  would have replaced nothing on the route that is actually registered.
* **There is no `draggingUpdated:`.** winit implements `draggingEntered:`,
  `prepareForDragOperation:`, `performDragOperation:`, `concludeDragOperation:`
  and `draggingExited:` — and nothing else, so AppKit reuses the answer
  `draggingEntered:` gave for the whole drag. A landing highlight that follows
  the hand is impossible through winit's route, and §3.3 makes that highlight
  non-optional.
* **macOS reads paths through an `NSString`**, which is exactly the round trip
  `macos_files.rs:33` forbids: "a name that is not UTF-8 comes back as a
  *different name*, with `U+FFFD` where its bytes were."

**Ruling ⑯: the drop door is `bt-platform`'s, and the shape of it is a probe
result, not a decision taken here.**

**macOS — PROBE 5, and it is the first work of T-PASTE-3.** The probe evaluates,
in this order of preference:

1. **An application-owned destination `NSView`** installed in the existing
   hierarchy, registered for §3.4's whole admitted list — file URLs, the raw
   picture types, and the promise types that exist only to be refused visibly.
   This is ordinary AppKit with no
   other crate's internals in it, and it is preferred if it can be made not to
   disturb anything. What the probe has to establish: hit testing (a destination
   view must not take mouse events from the panes beneath it), the responder
   chain and IME, the wgpu `CAMetalLayer` beneath it, and the embedded WKWebView
   panes.
2. **A narrow upstream extension** to winit — a `draggingUpdated:` and a
   pasteboard-carrying event — which is the honest fix and the slowest.
3. **Class-wide replacement on `WinitWindowDelegate`** only if neither works, and
   then with a per-window lifetime contract written down: the replacement is
   class-wide and therefore affects every window this process ever creates,
   including ones created later, and the design must say what each of them does.

Whichever shape wins, the door specifies the whole destination contract:
`draggingEntered:`, `draggingUpdated:`, `draggingExited:`,
`prepareForDragOperation:`, `performDragOperation:`, `concludeDragOperation:`,
their native ABI return types, per-window teardown, and the conversion from
AppKit points to this window's physical coordinates — the same conversion
`macos_impl::pointer_position` (`macos_impl.rs:613`) already performs, reused
rather than rewritten.

**Windows — the OLE initialisation comes with the target, and both are ours.**
`platform_impl/windows/window.rs:1167` gates **`OleInitialize` and
`RegisterDragDrop` together** behind `attributes.platform_specific.drag_and_drop`,
so `with_drag_and_drop(false)` removes the STA initialisation this window then
needs for its own target. And `event_loop.rs:1262` calls `RevokeDragDrop` on
`WM_DESTROY` **unconditionally**, whether or not winit registered anything. So
the ruling is:

* Folio owns `OleInitialize`/`OleUninitialize` on the event-loop thread — **but
  it is not the only owner of that apartment, and the ordering rule is written
  down because of it.** `platform/windows.rs:493` says in winit's own words that
  "winit may still attempt to initialize COM API regardless of this option.
  Currently only fullscreen mode does that", and the mechanism is
  `windows/window.rs:1432`: a `thread_local! { static COM_INITIALIZED }` holding
  a `CoInitializeEx(…, COINIT_APARTMENTTHREADED)` whose `CoUninitialize` runs
  from a thread-local destructor at thread exit. Both are STA so they nest
  rather than conflict, but the apartment's initialisation count then has two
  owners with lifetimes neither crate controls. **The rule: Folio's
  `OleInitialize` runs before any window is created, and its `OleUninitialize`
  either runs before winit's thread-local destructor or is deliberately not run
  at all in a process that is exiting.** `with_drag_and_drop(false)` does not
  remove winit's path; the doc says so.
* Folio registers its own `IDropTarget` on **every** HWND it creates — the main
  windows, the summoned terminal, anything a later slice adds. Two targets cannot
  coexist on one HWND: a missed constructor leaves winit's target in place and
  the second registration fails with `DRAGDROP_E_ALREADYREGISTERED`, which is a
  red test rather than a silent degradation.
* Teardown is written against winit's unconditional revoke rather than around it.
* **Effects are non-destructive only.** The target returns `DROPEFFECT_COPY` or
  `DROPEFFECT_NONE` and never `DROPEFFECT_MOVE`: telling a source that a move
  happened, when all Folio did was type a name, can make the source delete the
  original.
* Payloads are copied out before `IDataObject`/`STGMEDIUM` is released; callbacks
  are bounded and re-entrancy-safe; screen-to-client and DPI conversion is pinned
  by a test; source key-state masks, multi-window teardown and cancellation are
  specified.

**The cheap alternative is rejected, for the record.** Reading
`bt_platform::pointer_position()` when a `DroppedFile` arrives gives a point that
is *probably* right, and on macOS, where winit queues the event, probably-right
is a guess about where the hand was. Pointing at the wrong thing is worse than
not pointing (`DESIGN.md` §7.1.5k), and a drop in the wrong pane types a path
into the wrong shell.

### 3.2 Ruling — the drop joins the table that exists, and the table is described accurately

`row_verb` (called from `commit_layout_drop`, `main.rs:88732`) decides what a
**row dragged out of the files column** means. It takes the payload's kind, the
landing and the target pane's kind, and it is reached only behind
`DropLanding::layout_aim`.

| Landing | File | Folder |
| --- | --- | --- |
| root rim / seat **edge** | `Split` — a new pane holding it | `Split` — a new column rooted there |
| **centre** of a Preview pane | `Retarget` — open it there | `Refused` |
| **centre** of a Files column | `Refused` | `Retarget` — re-root the column |
| **centre** of a Terminal pane | `Refused` → **`Insert`** | `Refused` → **`Insert`** |

**The strip is not in this table, and the first draft misdescribed it.**
`main.rs:28995` documents the strip arm as **unreachable** — `layout_aim` is
`None` for all three strip landings — and the strip's own verbs are read off
`row_strip_landing` (`main.rs:29555`) and committed by `commit_strip_extract` and
`commit_strip_adopt`. So there is no "the strip refuses" row to state and nothing
there to defer: **the internal strip verbs are untouched**, and the only decision
is what an *OS* drop does when it lands on the strip. **Ruling: an OS drop on the
strip is refused in 0.4.1**, at the strip's own routing rather than in
`row_verb`, and the refusal is drawn by the strip's own feedback. Opening a new
tab at a dropped folder is a tab-creation verb and a separate ticket.

**Ruling ⑰: a file or a folder on a terminal pane's centre inserts its path, for
both carriers.** The rectangle must not have two answers depending on where the
dragged thing came from; the cell being changed is a refusal, so nothing is taken
away; and the window's own files column gains a verb people try today and are
refused.

**Ruling ⑱: batch admission is defined, because the table's other verbs take one
item.** `row_verb` and its commit are written around a singular payload, and
fifty files dropped on a preview pane have no defined meaning in it.

* **Terminal centre — many.** Every admitted path is inserted, in source order,
  as one run (§2.6). This is the only multi-item verb.
* **Preview centre, seat edge, root rim, files column — one.** These verbs open
  or re-root **one** thing. A drop carrying more than one item on such a target
  is **refused as a whole**, with the refusal box traced while the hand is still
  open, so the hover never promises a commit the release will not make. Opening
  fifty preview panes from one release, or silently picking the first of fifty,
  are both worse than a refusal the reader saw coming.
* **Mixed kinds** — files and folders in one drop — are admitted on a terminal
  centre (they are all paths) and refused everywhere else, by the same rule.
* **A cap.** More than 64 items in one drop is refused with a toast. A number,
  not "a lot", so that the acceptance row can be written.
* **Partial failure.** If some paths fail §2.4's gate, the representable ones are
  inserted and the rest are named in one toast; if *none* is representable,
  nothing is inserted and the toast says why.

**Ruling ⑲: drop insertion is target-specific and moves no focus**, unlike K144,
which moves the keyboard and the layout focus on purpose (`main.rs:76921`). A
drop names the pane by where the hand let go; taking the keyboard as well would
be a second, unasked-for consequence of a pointer gesture. Both call the same
insertion with a seat parameter, the way `paste_from_clipboard_into` already
takes one (`main.rs:96290`).

### 3.3 Hover feedback

**Ruling ⑳: the existing drag chrome, minus the ghost.** The landing highlight
and the traced refusal box are driven by `DropLanding`, computed from the live
pointer — which is why the door of §3.1 must report a continuous update and why
winit's route, with no `draggingUpdated:` at all, cannot serve. Nothing new is
drawn. The **ghost is suppressed for an OS drag**: the system already draws the
dragged file's image under the pointer.

`DESIGN.md` §7.1.5f applies at full force — *a mark is a promise, and a mark that
answers hover but not click is this window lying about what it drew.* Every
rectangle that highlights has a verb; every rectangle that does not traces the
refusal box. That is what routing through one table buys, and it is why §3.2's
batch rules are decided **before** the highlight rather than at the release, and
why §3.4 refuses an unsupported payload while the hand is still open.

### 3.4 What a drag may carry

**Ruling ㉑: a drop has its own admitted-type matrix; it does not inherit the
clipboard's.** The registered types and the decoding are shared code, but what is
registered and what each target accepts is stated here:

The columns are §3.2's own landings and the cells are §3.2's own verb names, so
that the two tables cannot be read as disagreeing. `Placeholder` is a real
`SeatKind` (`crates/bt-layout/src/tree.rs:26`) and gets a stated answer rather
than falling silently into `row_verb`'s `_ => RowVerb::Refused` arm.

| Payload | Registered | Terminal centre | Preview centre | Files centre | Placeholder centre | Seat edge / root rim |
| --- | --- | --- | --- | --- | --- | --- |
| file URLs — a **file** | yes | `Insert` (many) | `Retarget` (one) | `Refused` | `Refused` | `Split` (one) |
| file URLs — a **folder** | yes | `Insert` (many) | `Refused` | `Retarget` (one) | `Refused` | `Split` (one) |
| raw picture (`public.png`; `CF_DIB`/`CF_DIBV5`/`"PNG"`) | yes | §4's lane, then `Insert` | §4's lane, then `Retarget` | `Refused` | `Refused` | §4's lane, then `Split` |
| a file **promise** | yes, **to refuse it visibly** | `Refused` | `Refused` | `Refused` | `Refused` | `Refused` |
| text | not registered | — | — | — | — | — |

**Files and folders are two rows, because they have two verbs.** The earlier
single `file URLs` row distinguished them in its Files-centre cell and not in its
Preview-centre cell, which contradicted §3.2's own table and the code both tables
describe: `row_verb` retargets a Preview leaf only for `RowPayloadKind::File` and
a Files column only for `RowPayloadKind::Folder` (`main.rs:28988`–`:28993`).
Splitting the row states the same distinction once. It applies to the **hover**
as much as to the commit (ruling ⑳): a folder held over a Preview centre traces
the refusal box, and a file held over a Files column traces it, before the hand
opens. The `Placeholder` column and the edge/rim column are unchanged by the
split — both kinds `Split` at an edge, and neither is admitted on a placeholder.

Raw picture types are registered explicitly: a PNG-only drag out of a browser
does not match a file-URL-only registration and would never arrive. A **promise**
*is* registered — and only registered — so that the destination is offered the
drag and can trace the refusal box under the reader's hand (ruling ⑤);
`performDragOperation:` is never entered for one. The earlier draft left promises
unregistered and still promised a visible refusal, which is not a thing an
unregistered type can do.

**The Safari claim is withdrawn.** The first draft asserted that a picture
dragged out of Safari carries `public.png` and no file URL. Apple's own file-
promise sample exists precisely because Safari, Mail and Photos drag image
*promises*, so what an image drag offers is browser- and version-specific and is
a measurement, not a premise. **PROBE 6: record what Safari, Chrome and Firefox
offer for a dragged image, by version.** Until it is run, a browser image drag is
whatever the matrix above says about the types it actually presents — possibly a
refusal.

### 3.5 The rest of the drop's rules

* **A drop while a modal is open is refused** — the first-run card, a dialog, a
  menu, the palette; the predicate `paste_from_clipboard_into`'s neighbours keep
  (`main.rs:96257`).
* **A drop into a pane that is not the focused one does not move the focus**
  (ruling ⑲), and it answers that pane's attention the way a named-seat paste
  already does (`main.rs:96312`).

---

## 4. A picture becomes a file

### 4.1 Where it goes, and what the directory has to prove

**Ruling ㉒: under the platform's own temp, discovered rather than constructed,
and named the way this codebase already names a shared-temp directory.**

* **Windows**: `std::env::temp_dir()`, then `Folio`, then `clipboard` — so
  `%TEMP%\Folio\clipboard` in the ordinary case. The earlier draft named the
  Win32 function `std` happens to call, which is an implementation detail that
  buys nothing and can go stale, and that sentence is struck. What replaces it is
  **not** a claim that the answer is per-account: `std`'s Windows discovery is
  *environment-based*, `TMP` is consulted ahead of `TEMP`, and a policy or a
  launcher can point either anywhere. The answer is therefore **configured, then
  vetted** — ruling ㉓ is what makes it ours, not the name it came back with.
* **Unix**: `folio-<uid>`, then `clipboard`, under `$TMPDIR` — **or under `/tmp`
  when `$TMPDIR` is unset or empty**. That pair of rules is
  `instance::runtime_directory()`'s (`instance.rs:331`–`:337`), and it is
  **Folio's own policy rather than a description of `std`**: `std::env::temp_dir`
  does not filter an empty `TMPDIR`, and on Darwin it falls back to a
  system-provided directory rather than to `/tmp`. The earlier draft's "which is
  what `std::env::temp_dir()` answers there" was wrong about the crate it cited,
  and is struck; this lane calls `runtime_directory()` and inherits whatever that
  function does, which is the one place this product decides the question.
  `runtime_directory`'s header gives the reason for the `<uid>`: "two users on
  one machine must not meet in one directory, and the one that got there first
  must not be able to decide what the second one finds." A fixed `Folio` under
  `/tmp` is a name any other local user can create first, after which the uid
  check refuses it, the lane fails closed, and the feature is **dead for that
  account on that machine with no remedy** — a one-line local denial of service.
  **The suffix is not proof against squatting**: a uid is predictable, so another
  local user can still precreate `folio-<uid>` for a uid that is not theirs. What
  the suffix buys is that ordinary accounts do not collide by accident; what
  vetting buys is that a squatted directory is refused rather than used. The
  fail-closed limitation stays stated.

`folio-<uid>` is the name everywhere: in creation, in the vetting below, in the
sweep, in `PRIVACY.md`'s row and in the delete command §4.5 publishes. The
earlier draft renamed it here and left `Folio` standing in three later places;
those are corrected.

macOS's `/var/folders/…` form and its `/private/var` alias are **not** rejected:
they are the standard answer, and a check that refused them would refuse every
Mac.

**The directory is spelled once, at vetting, or the lane refuses.** §2.4's
representability gate runs on the **directory** as part of this vetting — before
any picture is written — so that a temp path that cannot be spelled as a shell
literal refuses the whole lane with one toast, rather than writing a file and
then refusing to name it. The failure is real: a redirected `%TEMP%` with an
unpaired surrogate, or a `$TMPDIR` with a non-UTF-8 byte.

**And representability is not encodability, so the recipient's own encoder is
preflighted too.** §2.4's gate asks about UTF-8 and control characters, which are
properties of the *path*. The refusals that come after it are properties of the
**pane**: `%` in a `cmd.exe` row, an unprobed PowerShell quote character, a
Nushell fence the measured baseline does not cover. A valid UTF-8 `%TEMP%`
containing `%`, redirected there by policy, passes the directory gate perfectly
and is then refused by the encoder — after the picture has been written. So a
**`TerminalInsertion` job's captured recipient** (ruling ㉖) is used to run the
prospective output name through that recipient's namespace and grammar **before
the write**, and a refusal there refuses the lane with the encoder's own toast
and creates nothing. The distinction is kept rather than collapsed: a globally
vetted directory can be perfectly usable for one pane and refused for another, so
this is a per-job check and not a second property of the directory. §6.2 carries
a redirected-`%TEMP%` `%` row and an unmeasured-grammar row, **each paired with a
recipient that encodes the same directory successfully**, so that a directory
refused for everybody cannot pass the test by accident.

**The other two destinations get the check that is meaningful for them, and not
this one** (fourth review finding 6). A `PreviewRetarget` and a `LayoutSplit`
open the file; they do not spell it for a shell, and there is no recipient whose
encoder could refuse. Their pre-write check is therefore the **native file-open
validation** ruling ㉖ names: the prospective path is absolute, lies below the
directory vetted two paragraphs above, is within the platform's own path-length
limit, and carries no character the platform's file API refuses. It runs at the
same moment, refuses the same way — a toast and no file — and it is the whole of
the preflight there. Borrowing a terminal's grammar to stand in for it is
refused: a picture dropped on a preview must not fail because some unrelated
pane's row is a `cmd.exe`.

**Every post-write failure to deliver deletes the file.** Not only the
representability case the second review asked for: an encoder refusal, a
revalidation failure, a cancelled target, a switch turned off mid-flight and an
unspellable path all end the same way — the file is removed through the owned
path, exactly as a cancellation does (ruling ㉖). A picture on the disk that
nothing points at is the write the §4.6 switch was supposed to be the only cause
of.

**Ruling ㉓: a trust contract, on both platforms, for every Folio-made ancestor,
before every operation.** The earlier draft vetted the final directory once per
run against the rules `launch_pipe_unix::vetted_endpoint` (`:470`) applies to a
*socket*. That is the wrong precedent twice over: a socket's `0600` is not a
directory's mode, and once per run is not before every write. The directory
precedent in this codebase is `instance::prepare_runtime_directory`
(`crates/bt-platform/src/instance.rs:360`), which creates with an explicit mode,
`symlink_metadata`s the result, refuses a symlink or a non-directory, refuses a
foreign uid, and **repairs an existing mode** rather than refusing the claim.
This lane follows it:

* **Unix.** The two levels are `runtime_directory()`'s own
  **`folio-<uid>`** — `$TMPDIR` when it is set and non-empty, else `/tmp`,
  then `folio-{uid}` (`instance.rs:331`–`:337`) — **and** the `clipboard` child
  beneath it. `Folio` is the **Windows** name and appears at no level here; the
  earlier recipe said `Folio` and disagreed with the path this lane selects three
  paragraphs above, with `PRIVACY.md`'s row and with the delete command §4.5
  publishes. `DirBuilder::mode(0o700)` for **both**, because an intermediate
  directory of ours redirected by a link redirects the final one that is not a
  link. Then, for each of the two levels: `symlink_metadata`, not a symlink, is a
  directory, `uid == geteuid()`, and mode repaired to `0700` if it is anything
  else — which is `prepare_runtime_directory`'s own recipe (`instance.rs:360`)
  applied at both levels rather than borrowed for one. A same-owner
  world-readable directory does **not** pass. Files are created `0600`. The
  fail-closed squatting limitation of §4.1's discovery paragraph is unchanged:
  a `folio-<uid>` precreated by another local user is refused here, and the lane
  is dead for that account until it is removed.
* **Windows.** There is no uid, and the counterparts are stated in full rather
  than left implied — because `CreateDirectoryW` applies the security descriptor
  it is handed **only when it creates the directory**: an existing one comes back
  `ERROR_ALREADY_EXISTS` with its own permissions untouched, and the descriptor
  has effect at all only on a filesystem that keeps persistent ACLs. Asking for a
  restricted descriptor at creation therefore proves nothing about a directory
  that was already there, which is the case this design has to survive. At **each
  of the two levels**, existing or created. **The earlier draft of this list got
  the protection flag backwards** — it required the directory *not* to be
  protected, on the theory that protection "would let a later descriptor widen
  what a file gets". `SE_DACL_PROTECTED` does the opposite: it stops the
  descriptor's DACL being modified by **inheritable ACEs from the parent**, which
  is the one thing this directory needs, since its parent is a temp directory
  whose ACEs Folio does not choose. Propagation to children is a different
  mechanism entirely — the `OBJECT_INHERIT_ACE` / `CONTAINER_INHERIT_ACE` flags
  on this directory's own ACE. The two are now stated apart, and the flag is
  **required** rather than forbidden:
  * **One principal, and it is the token's user SID.** The permitted DACL is
    exactly one allow ACE for the **current user's SID** — `TOKEN_USER`, the
    account — with `FILE_ALL_ACCESS`, and nothing else: no `Administrators`, no
    `SYSTEM`, no `Everyone`. That is the counterpart of Unix's `0700`, and it is
    what §4.5's "readable only by you" says. The earlier draft admitted "the
    system's own administrative identities" at the directory while requiring the
    file to grant the owner **only**, so an admitted administrative ACE would
    have propagated to the file and then failed the file's own read-back; the two
    contracts now name one principal set. **It is the *user* SID and not the
    *logon* SID.** `SECURITY.md:39`–`:45` scopes the attention pipe to the logon
    session on purpose — a live IPC endpoint belongs to one desktop session —
    but these are files that must still open after a logoff and a fresh logon, so
    a logon-SID ACE would make the reader's own pictures unreadable tomorrow.
    The pipe is cited here for **one** thing, its fail-closed rule: if the token's
    user SID cannot be read, or the descriptor cannot be constructed, the lane
    **refuses and opens nothing**, exactly as the pipe refuses rather than falling
    back to a default descriptor.
  * **Protected, and inheriting to children.** Each of the two directory levels
    carries, in SDDL, `D:P(A;OICI;FA;;;<user SID>)` — `P` is
    `SE_DACL_PROTECTED`, the boundary against whatever `%TEMP%`'s parent chain
    would otherwise push down; `OICI` is `OBJECT_INHERIT_ACE` +
    `CONTAINER_INHERIT_ACE`, so a file created beneath with no descriptor of its
    own still lands on the one principal. Both are **checked**, not assumed: a
    level missing `P` is one an inheritable parent ACE can reach.
  * **Owner.** Read the security descriptor's owner SID and require it to be the
    current user's. A directory somebody else owns is **refused**, never
    repaired: repairing a foreign directory is either impossible or an act of
    privilege this lane has no business performing.
  * **Repair, on a directory the user owns.** A DACL that is broader than the one
    ACE, or that is not protected, is **rewritten to the exact form above** — the
    counterpart of Unix's mode repair. On a directory they do not own, refused
    (the row above).
  * **Reparse points** are refused at either level, as before.
  * **A filesystem that cannot enforce it is refused.** If the volume does not
    support persistent ACLs — a redirected `%TEMP%` on a FAT or exFAT stick, or a
    share that does not carry them — the owner-only promise cannot be kept, so
    the lane refuses with a toast rather than writing a picture into a place it
    has told the reader is private. §4.5's "readable only by you" is a promise,
    and this is the check that makes it one.
  * **The created file is verified against the same principal policy.** It is
    created with an explicit `D:P(A;;FA;;;<user SID>)` — the same single
    principal, protected, and no inherit flags, a file having no children — so it
    does not depend on the parent's propagation having worked; and then its own
    DACL is **read back before a single picture byte is written** and required to
    be exactly that. Creation and validation therefore ask the identical
    question, which the earlier draft's two lists did not. A read-back that
    disagrees is a refusal and a delete, not a shipped file.
  * **The residue, stated.** An owner-only DACL does not defend against a
    principal who can take ownership or who holds `SeBackupPrivilege`: a local
    administrator can read these files whatever this descriptor says, and no
    descriptor Folio can write changes that. §4.5 says "readable only by you" of
    ordinary access, which is what the check enforces.
* **Before every operation, not once.** The vetting runs before each create and
  before each sweep, because a directory replaced after the first check redirects
  everything after it. `create_new` protects a *name*; it protects no parent.
* **Anchored where the platform allows.** On Unix the creates and the sweep are
  performed relative to a verified directory handle with no-follow semantics. On
  Windows the equivalent handle-relative primitive is out of reach from `std`, so
  the mitigation is the reparse-point check plus `create_new`, and **that residue
  is stated here rather than left to be discovered**.
* **Fail closed, with a sanitised message.** A refusal is a toast and no file —
  never a write somewhere else, and never a message that quotes a path a reader
  did not ask to see.

### 4.2 The name, the format, the bounds, and the delivery

**Name — `clip-<yyyymmdd>-<hhmmss>.png`**, and on a collision
`clip-<yyyymmdd>-<hhmmss>-<n>.png` for `n` from 2. The stamp is **local time
from the system clock**, which is the clock the reader's shell listing uses; the
sweep's age comparison uses the file's own mtime, not the name (§4.3), so a clock
change or a DST step cannot make a file immortal or delete a new one. Creation is
`File::create_new` — `CREATE_NEW` / `O_EXCL` — so a collision is detected by the
filesystem rather than by a `stat` another process can invalidate. Never
`bt_persist::atomic_write`: replacing is the thing that must not happen.

**Format — PNG, and the picture is validated by a full decode while the *original
bytes* are what get written.** If the source offers PNG, those bytes are decoded
end to end to prove they are a whole picture — an `IHDR` that parses says nothing
about a truncated or corrupt stream, and a "successful" filename naming a broken
file is worse than a refusal — and then the **original** bytes are written, so
the colour profile and every other ancillary chunk survive. A `CF_DIBV5` or
`CF_DIB` is decoded through `image`'s **`BmpDecoder::new_without_file_header`**
(`image-0.25.10/src/codecs/bmp/decoder.rs:534`), which exists in that crate
explicitly for `CF_DIB`; the first draft's synthesised 14-byte
`BITMAPFILEHEADER` is **withdrawn** — it was a hand-rolled offset calculation
across headers, masks, palettes and profiles, standing next to an audited entry
point that needs none of it.

**Ruling ㉔: alpha is decided by the bitmap, not by the clipboard format id, and
an unsupported layout is refused rather than converted.** `CF_DIBV5` with
`BI_RGB` does not carry a meaningful alpha channel simply because the header is a
V5, and that decoder's own alpha handling depends on the compression and on a
non-zero mask (`decoder.rs:752`). Treating premultiplied channels as straight
darkens translucent edges; treating undefined legacy alpha as meaningful turns
opaque pixels transparent. So: masks and compression are read; a layout this
design has not specified — an unexpected bit depth, a mask combination not in the
table, a palette form not covered — is **refused with a toast** rather than
emitted as a changed picture. §6.1 carries known-pixel producer fixtures for
each supported layout. **PROBE 7: capture real `CF_DIBV5` payloads from Snipping
Tool, `Win+Shift+S`, Paint, Chrome's *Copy image* and Excel's copy-as-picture,
and record for each what the header, the masks and the alpha actually are.**

**TIFF is out of 0.4.1.** `Cargo.toml:107` turns defaults off and enables
`gif, jpeg, png, webp`; `tiff` is a separate feature in `image-0.25.10`
(`Cargo.toml:120`). Enabling it is a dependency, a lockfile and a
`THIRD-PARTY-NOTICES.md` change, and `public.tiff` is a *fallback* offered beside
`public.png` by macOS sources that already offer PNG. So the macOS picture rung
is `public.png` only, a PNG-less TIFF-only pasteboard is `Absent`, and the TIFF
arm is a named debt with its notices work attached.

**An animated PNG is kept whole, and the first-frame rule is struck.** The
earlier draft promised that "multi-frame sources take the first frame", using a
copied GIF as its example — but GIF is not on either platform's rung list
(§1.2), so the only multi-frame payload this lane can actually receive is an
**animated PNG**, which the PNG specification defines and which `image`'s own
decoder treats as a separate thing from the default image it otherwise returns
(`image-0.25.10/src/codecs/png.rs:140`–`:161`). Writing the original bytes —
which is this section's rule, and the reason the colour profile survives — keeps
such a file animated no matter what was decoded, so "first frame" and "original
bytes" were two promises that could not both be true. The ruling picks the one
that matches the write:

* **The file is the source's own bytes, animation included.** Nothing is
  flattened and nothing is re-encoded, so no frame is silently dropped and no
  metadata is lost.
* **Validation covers what is written.** For a still PNG that is a full decode of
  the image. For a PNG carrying an `acTL` chunk it is a full decode of the
  default image **and** of every animation frame, so that a truncated or corrupt
  animation refuses instead of being written as a "successful" broken file. The
  frames are decoded under ruling ㉕'s caps applied to their **sum**, not
  per frame.
* **Too big to validate is refused, not truncated.** An animation whose frames
  cannot be decoded inside the cap is refused with a toast. Writing bytes this
  lane has not validated would be the exact failure the full-decode rule exists
  to prevent.
* **The toast says so.** A written animation is reported as an animation, because
  a reader who pastes one into an agent should know that what they handed over
  moves.

§6.1 carries an APNG fixture **whose default image differs from animation frame
one**, which is the case that would have exposed the contradiction, plus a
truncated-animation fixture that must refuse.

**Ruling ㉕: bounded at acquisition, in decode, and in aggregate.** Per-picture
caps alone bound nothing — the first draft's caps ran *after* native bytes had
been copied across the boundary, and nothing stopped a reader from starting a
legal 256 MiB job every second.

* **At acquisition.** The native length is read and compared before anything is
  copied: `GlobalSize` on the Windows handle. On macOS `data(forType:)` produces
  an `NSData` before its length can be read, so the bound applies to *our* copy
  and the decode, not to AppKit's own allocation — **stated as a residue**, with
  the mitigation that nothing is copied out of it above the cap.
* **Before allocation.** The header's dimensions are checked against the pixel
  cap before a decode buffer is allocated. The cap is expressed in **decode
  bytes**, not in an RGBA8 multiplication: a 16-bit-per-channel intermediate is
  eight bytes a pixel, and the decoder's own scratch is not free.
* **Numbers.** Source bytes ≤ 64 MiB **for the whole encoding list of one
  picture** (§1.2), not per encoding; decode allocation ≤ 256 MiB; written file
  ≤ 64 MiB; the `clipboard` directory ≤ 512 MiB in aggregate.
* **A 24-hour retention floor, on every path that removes a file.** No
  `clip-*.png` younger than 24 hours is removed by quota eviction, by the startup
  sweep or by the hourly sweep. It is a **retention floor**, not a guarantee
  about any particular command line: §4.3 says plainly that a path on the screen
  is not a promise the file is there, and a line can sit unsubmitted for longer
  than a day. What the floor buys is that eight 64 MiB pastes in five minutes
  cannot evict the first one out from under a half-typed command, which is the
  case that made the floor worth having. The one deletion the floor does not
  govern is a **job removing its own undelivered output** (ruling ㉖): that file
  was never handed to anybody, so removing it takes nothing away.
* **The quota is a hard bound, and it is serialised across processes by a
  crash-released OS lock.** The directory is shared, so "one job in flight per
  window" says nothing about two windows or two Folios, and the earlier draft's
  lock — a `create_new` presence file with a ten-second staleness rule and a
  bypass on contention — was neither safe nor a serialisation: elapsed time
  cannot tell a stalled live writer from a dead one (the same section admits the
  temp may be a network share), stealing on age lets one process unlink another's
  lock, and a writer that bypasses the lock re-opens exactly the overlapping
  check-and-write the lock existed to prevent. All three rules are **withdrawn**
  and replaced by one contract:
  * **The lock is a handle, not a file's existence.** `clipboard/.lock` is
    created once, with `create_new` on the vetted directory and owner-only, and
    **is never deleted** — not by a sweep, not by a releasing process. Holding it
    means holding a kernel lock on an open descriptor: on Unix `flock(fd,
    LOCK_EX)`, on Windows a handle opened with **no sharing** (or an
    `LockFileEx` exclusive range on one). Both are released by the operating
    system when the handle closes, **including when the process dies**, so a
    crashed Folio frees the lock with no age rule and nobody ever removes
    somebody else's lock. The `.lock` name is outside §4.3's owned-name grammar
    and outside the quota's accounting.
  * **The whole sequence runs inside it**: listing, size accounting, eviction
    under the retention floor, the reservation of the new file's budget, the
    write itself, and the quota half of the sweep. Anything that reads the total
    and then acts on it is inside, or the total was a guess.
  * **Contention waits off the loop, with a limit, and then refuses.** The
    picture worker — never the event loop (§4.3) — retries with backoff for at
    most **two seconds**, and if it still cannot take the lock the paste is
    **refused with a toast**. That is the trade this ruling now makes explicitly:
    a rare, bounded refusal while another window is mid-write, in exchange for
    512 MiB being a real maximum rather than a hope. The age sweep's own pass
    needs no lock, because a file older than seven days is past every floor and
    no other process's accounting depends on it.
  * **So 512 MiB is a bound, and §4.3 may call it one.** If eviction under the
    retention floor cannot free enough room for the new picture, the write is
    refused — the directory does not go over.
* **One in flight.** At most one picture job per window. A second picture paste
  while one is pending is **refused with a toast**, not queued — a queue would
  need an ordering contract for a gesture that has no meaningful order.
* **Failures are complete.** A write, flush or close error deletes the partial
  file and reports; disk pressure is a refusal, not a truncated PNG.
* **Acquisition has no latency contract, and this bullet used to claim one.**
  A delayed-rendered format means the *owner* runs code before we get bytes, and
  ruling ④ is now explicit that **Folio sets no deadline** on that and cannot
  cancel it; a source that never answers stalls the loop until the system's own
  backstop hands back `NULL`, which is then `Unreadable` (ruling ③). The earlier
  sentence "the read is bounded" and the request that PROBE 8 supply the number
  are **struck here as well as in ruling ④** — a design cannot carry a bound in
  one section and its absence in another. PROBE 8 measures a distribution and
  decides where the transaction runs; nothing waits on a constant.

**Ruling ㉖: a delayed job carries a *typed* destination and a generation, and
both are revalidated at completion.** Encoding a 4K DIB is a
one-to-three-hundred-millisecond job and must not run on the event-loop thread.
That makes the insertion **asynchronous**, and the first draft's single re-check
of the cell left of the cursor solved none of the real problems: a reader can
switch tabs, restart or close the shell, open a modal, press Enter, or paste
again while it runs.

**Every job carries, whatever its destination**: a **request sequence number**,
globally unique in the way `CONVENTIONS.md:154` requires of worker addresses,
with one owner for the answer; the **window and tab**; and the setting of §4.6 as
it stood when the gesture was made. **The captured *recipient* is not on that
list** (revised, fourth review finding 6). The earlier draft put it there and
said every job runs §4.1's pre-write encoder check — while two paragraphs later
saying that a Preview retarget and a layout split have no shell. A job with no
shell cannot satisfy a mandatory shell preflight, and the only ways out were to
invent a recipient or to borrow the focused pane's, which would make a picture
dropped on a preview refuse because some unrelated terminal's grammar cannot
spell `%`. **A surrogate recipient is never chosen.** So the recipient, its
namespace and its grammar move **into the terminal destination**, and each of
the other two gets the validation that is actually meaningful for it.

**What stays common to all three**: §4.1's directory vetting and its
representability gate on the directory, ruling ㉕'s size caps and quota lock, the
modal gate, the §4.6 setting, revalidation of the typed destination, cancellation,
and the rule that any failure to deliver removes the file through the owned path.

**And then it carries one of three destinations, because §3.4 admits three.**
The earlier draft described a terminal insertion and applied it to "a delayed
picture drop" as well — but a picture dropped on a Preview centre is a
`Retarget` and one dropped on a seat edge is a `Split`, and neither has a shell.
`LeafSession` is a Terminal leaf's own PTY and screen (`main.rs:10283`–`:10292`);
a Preview pane has no incarnation of that kind, and a split's destination seat
does not exist yet when the hand opens. So the destination is typed, and **each
variant names its own pre-write check and its own revalidation**:

* **`TerminalInsertion { leaf, incarnation, input_generation, recipient }`** — the
  seat, its **session incarnation** (so that a restarted shell in the same seat is
  a different recipient and a reused `SeatId` cannot be mistaken for the
  original), the input generation below, and the **captured recipient**: the
  pane's namespace and grammar as they stood at the gesture, **together with the
  encoder configuration rule A names** — the `Cmd` origin, the row's
  delayed-expansion flag, the applicable probe policy, and the effective
  insertion spelling with the context that spelling needs (a `wsl` distribution,
  where there is one) — so that the preflight and the write encode the same
  string the gesture would have (revised, fifth review finding 3). Nothing here
  is a live lookup: the job holds values, not a handle to the row. *Pre-write:* §4.1's
  encoder preflight, run against this recipient, refusing before any file exists.
  *Revalidation:* the leaf exists, the incarnation matches, the input generation
  is unchanged.
* **`PreviewRetarget { leaf, content_generation }`** — the Preview pane and a
  counter of what it has been pointed at. *Pre-write:* **no shell check, because
  there is no shell.** What this destination will do with the file is open it, so
  what is checked is that it can be opened: the prospective path must be one this
  window can name to the platform's own file API — an absolute path, below the
  vetted directory, within the platform's length limit, and carrying no character
  the file API refuses. §2.4's gate has already refused an unspellable *name*;
  this is the open-side counterpart of the terminal's encoder preflight, and it is
  the whole of the pre-write check here. *Revalidation:* the pane exists and the content
  generation is unchanged — if the reader has since opened something else there,
  by any route, the picture does not steal the pane back.
* **`LayoutSplit { anchor, layout_generation }`** — where the hand let go, the
  side being carried by the anchor below, plus the tab's layout generation. *Pre-write:* the same native file-open check
  as `PreviewRetarget`, for the same reason. *Revalidation:* per the anchor
  variant below; the landing is **not recomputed** from anything current, because
  recomputing would open the picture where the layout has since moved to rather
  than where the hand let go.

**A split anchor is not always a seat, and the earlier draft had no shape for the
case it already admitted.** `anchor_seat` could only name a pane, but §3.4's own
matrix admits a raw picture on a **root rim** as well as on a seat edge, and a
rim has no seat: `DropLanding::RootRim { edge }` (`main.rs:28787`) carries an edge
and nothing else, `layout_aim` maps it to `seats::LayoutAim::Rim(edge)`
(`main.rs:28848`), and `aimed_at` answers `None` for it because — in that code's
own words — "the rim aims at the layout as a whole … there is no pane to point
at". So the anchor is two-valued, and revalidation differs per variant:

| `anchor` | Captured | Revalidated at completion |
| --- | --- | --- |
| `SeatEdge { seat, side }` | the seat the edge belonged to, and which side | the seat still exists, is still in the captured tab, and still admits a split on that side |
| `TabRoot { side }` (the rim) | the tab and the side; **no seat**, which is the whole point of the gesture | the captured tab still exists, and its **root** still admits a split on that side. Nothing is asked about any pane, because the gesture was never about one |

Both carry the tab's `layout_generation`, and a moved generation is a
cancellation for either: the layout the hand aimed at is not the layout on the
screen. A rim job is **never** repaired into a seat job by picking whichever pane
now sits at that edge — that is the same surrogate the recipient rule refuses,
one surface over.

**The input generation, and why a unique address is not enough** (third review
finding 16). A job's address proves the *seat* is the one that asked. It proves
nothing about the *line*: the reader can press Enter and start another command
while the encode runs, and every check the earlier draft listed — leaf exists,
incarnation matches, no modal, setting on — still passes, so the path would land
in the next command or in a running program's stdin. That is not a delayed
answer to the reader's gesture; it is a different gesture's line being edited by
this one. So a terminal destination captures a **per-target input generation**.

**And the list of events that advance it is not an enumeration of keys** (revised,
fourth review finding 5). The third revision listed submission, typed characters,
a text paste, a K144 insertion and a drop insertion — which leaves every
*editing* input out. An `ArrowLeft` moves the cursor into the middle of an
existing token; `Home` moves it to the front of the line; `Up` recalls a different
command entirely; `Backspace` removes the character the insertion was going to sit
after; `Tab` asks the shell to complete and rewrite what is there; `Ctrl+C`
abandons the line and starts a new one. None of them types a printable character,
and all of them change the line the path was going to be appended to — so a list
built from "typing or submission" would let the job land on a line the reader has
since edited, which is the exact case the ruling exists to prevent. **Inferring
line stability from printable characters is the error; the rule is stated over
the input's origin instead:**

> **The generation advances on every user-originated byte this window puts into
> that target's PTY.** Navigation and editing keys, control keys, history recall,
> a Tab, an IME commit, a paste, a K144 or drop insertion, a submission — all of
> them, because all of them are the reader's own hand on that line.

**It is one boundary, and this codebase already has it.** The encoder is
`input::keyboard_bytes` (`crates/bt-app/src/input.rs:530`–`:632`): the arrow,
`Home`, `End`, `Insert`, `Delete`, `PageUp` and `PageDown` sequences are at
`:567`–`:582`, `Enter`, `Backspace`, `Tab` and `Escape` at `:619`–`:622`, `Ctrl+C`
at `:540`–`:544` and the rest of the control alphabet at `:558`–`:563`. Every one
of those is a user keystroke by construction, and the function answers `None` for
what is not — a paste shortcut, `NamedKey::Process` mid-composition, an unclaimed
chord. The **target-specific** half is one seat-taking call beside it:
`Runtime::note_user_typing(seat)` (`main.rs:85381`), whose own header states the
scope this ruling needs — it is "called from the four doors a person's own input
reaches a shell through — the keyboard, a paste, an IME commit and a files-row
insert — and from nowhere else", and explicitly not for "a wheel forwarded to a
full-screen program … a terminal protocol reply, a resize repair chord, or the one
line a restored pane puts back on the prompt" (`main.rs:85368`–`:85375`). Those
four doors are `main.rs:96160` (keyboard), `:96322` (paste), `:96482` (IME commit)
and `:76953` (files row), and the drop insertion of §3.2 becomes the fifth, since
it is the same gesture through a different hand. **The generation advances at all
five** — one line beside an existing per-seat call that already means "the
reader's own bytes went into this pane".

**But the generation is its own boundary, not an alias for that call** (revised,
fifth review finding 1). The fourth revision wrote the rule as "advance exactly
where `note_user_typing` is called, and nowhere else", and borrowed that
function's exclusion list wholesale. That was wrong by one door.
`note_user_typing` exists for **command-history provenance** — its own header
says what it buys is `CommandMark::typed_by_user`, the one fact about a
remembered command that a program printing `OSC 133` marks cannot produce
(`main.rs:85377`–`:85381`) — and its narrower scope is not
evidence that an input it ignores cannot move the line a delayed insertion is
aimed at. The counterexample is in this codebase: **a wheel notch over an
alternate-screen row that asked for alternate scroll is delivered as arrow
keys.** `WheelRoute::ArrowKeys` calls `input::alternate_scroll_bytes`
(`main.rs:95117`–`:95136`), and that function builds its bytes by calling the
very same `input::keyboard_bytes` for `ArrowUp` / `ArrowDown`
(`input.rs:456`–`:468`) — the identical `ESC O A` / `ESC [ A` that a pressed
`Up` sends, chosen against the addressed shell's own cursor mode. The route
decision (`main.rs:110810`–`:110823`) keeps this arm distinct from
`WheelRoute::MouseReport` and from `WheelRoute::Local`, so it is neither a mouse
protocol report nor a local scroll: it is **user-generated navigation in that
target's PTY**. A reader at an alternate-screen input editor who nudges the wheel
during an encode is moving through history exactly as pressing `Up` would, and a
job that survived it would insert into a line the reader has since changed —
which is the one thing ruling ㉖ exists to prevent.

**So the picture job's input generation is a per-target counter of its own**,
advancing on the five doors above **and** on `WheelRoute::ArrowKeys` delivery to
that target. This changes nothing about `note_user_typing`: its command-history
meaning, its four doors and its own header are left exactly as they are, and the
new advance is a second call site for the *generation*, taken at the wheel door
beside `send_mouse_input_to`.

**What is deliberately excluded, and why each.** *Terminal replies* — the
`take_pty_writes` drains at `main.rs:35630` and `:35665`, which answer a program's
own query — are this window speaking on the child's behalf, not the reader
typing. *The PSReadLine resize-repair chord* (`main.rs:83223`) and *a restored
pane's replayed command line* (`main.rs:35702`) are this window's own
bookkeeping. *Local-only UI actions* — scrolling (`WheelRoute::Local` included),
selecting, opening a menu, switching tabs — put no byte in the pipe at all and
are outside the rule by construction. Each exclusion is a decision, not an
omission, and each rests on the same test: the bytes are not the reader's.

**Forwarded mouse reports advance it too, conservatively** (revised, fifth review
finding 1). `UserInputKind::MouseButton` / `MouseWheel` / `MouseMotion`
(`main.rs:18590`–`:18606`) write through `send_user_input` without passing
`note_user_typing`, and the third revision excluded them on the ground that "a
program in mouse tracking is not at a prompt". **That sentence is struck**: the
cited enum says which door a byte came through, not what the program on the other
end is doing with it, and a shell whose editor has mouse reporting on is not
excluded by anything this window can read. The bytes are the reader's hand, the
recipient's interpretation is unknown, and the cost of being wrong is asymmetric
— a needless cancellation loses a picture the reader can paste again, while a
needless survival edits a command they did not mean to edit. So a forwarded mouse
report to the job's own target **advances the generation**, and the alternative —
shipping a narrower supported-recipient contract that names the recipients whose
mouse handling is known not to move the line — is rejected, because this document
has no way to enumerate it. A report forwarded to **another** pane still does
nothing to this target's generation, which is what keeps the rule per-target.

**If the generation has moved when the job completes, the job is invalidated**:
nothing is delivered, the file is removed through the owned path, and a toast says
the picture was not inserted because the line moved on. The alternative contract —
"the path goes into whatever line is current" — was considered and rejected: it
makes a keystroke from three hundred milliseconds ago edit a command the reader
has since started, which is the one thing an asynchronous insertion must not do.

**At completion**, then, a job revalidates **its own variant's** conditions — the
table above and the three destination bullets are the whole of the list:
incarnation plus input generation for a terminal, content generation for a
Preview, layout generation plus the anchor's own condition for a split — together
with the modal gate and the setting of §4.6. **Cancellation** is explicit when
the pane closes, the shell restarts, the window closes, the Preview pane is
retargeted, the anchor seat goes away, the captured tab or its root goes away, or
the setting is turned off. On any invalidation or cancellation the file is deleted through the owned
path rather than left for the sweep — that is the same rule §4.1 states for every
post-write failure to deliver. Ordering is unnecessary because only one job is
ever in flight (ruling ㉕), which is a second reason for that rule. The
leading-space rule is re-asked at insertion, as K144 already does — but as the
*last* step, not the whole of the contract.

### 4.3 When the files go

**Ruling ㉗: best-effort retention with a quota-backed maximum, and no promise
about history.** The first draft claimed seven days "exceeds any session" and
"preserves history". Neither is true: a Folio left running for eight days still
needs its file while another Folio's startup deletes it, a continuously running
Folio never sweeps at all, and the operating system's own temp cleaning can
remove a file before either. The honest statement, which goes in `PRIVACY.md` in
these terms:

* Files are removed when they are older than **seven days**, checked at startup
  **and** hourly while a Folio runs, so that a long-lived process is not a
  reason for unbounded retention.
* The directory is additionally bounded at **512 MiB** (ruling ㉕), oldest first
  — and under that ruling's cross-process lock that bound is a real one: a write
  that cannot make room under the retention floor is refused rather than allowed
  to overshoot. Those two limits together are the maximum retention this design
  offers.
* **The 24-hour floor applies to every path that removes a file** — quota
  eviction, the startup sweep and the hourly sweep alike. It is a *retention
  floor*, and the only deletion outside it is a job removing output it never
  delivered (ruling ㉖).
* **The system may remove them sooner.** A temp directory is swept by the
  platform, and Folio does not prevent it.
* **A path on your screen is not a promise that the file is there** — not in
  shell history, and **not in a command line you have typed but not submitted**.
  The 24-hour floor makes the second case unlikely for a line you are still
  typing; it promises nothing about a line left on the prompt overnight, and
  nothing makes it impossible. A reader who wants to keep a picture copies it
  somewhere of their own, and the toast that reports a written file says where it
  is, which is the moment to do that.

**The sweep runs on the picture lane's worker, not on the event loop.** It is a
directory listing, a `symlink_metadata` per level, and up to N unlinks, against a
`%TEMP%` that §4.5 explicitly discloses may be redirected to a network share —
which on the event-loop thread would be an arbitrary stall once an hour with
nothing to name it in a hang report. It is the same worker the encode runs on, so
it is never concurrent with a write of this process's own. Anything of it that
does touch the loop shares ruling ④'s `hang_watch` station. **Its two halves take
the lock differently** (ruling ㉕): the age pass needs none, because a file past
seven days is past every floor and no other process's accounting depends on it,
while any pass that reads the directory's total in order to act on it runs inside
the lock like every other accounting.

The owned-name grammar is exact — `clip-` + 8 digits + `-` + 6 digits +
optional `-` + digits + `.png` — because `clip-family.png` is a file a reader
could have put there and the sweep must not own it. `.lock` is outside that
grammar too, and is never removed by anything (ruling ㉕). Only **regular files**
in that one directory are considered: not directories, not symlinks, not anything
reached through one. Age is the file's mtime. A file currently being written by
this process is excluded by name through the in-flight job; a file being written
by *another* Folio is excluded by the `create_new` + age rule, since a file
younger than the cutoff is never a candidate. Failures are counted and reported
once in `diagnostics.log`, never per file.

**This is not clipboard watching** and the privacy text says so: the sweep looks
at a directory of our own files, on a timer, and reads no clipboard.

### 4.4 The path is a path

**Ruling ㉘: once the file exists it is an ordinary file and takes §2 whole** —
the representability gate, the pane's spelling, the pane's grammar. A picture
pasted into a WSL pane arrives as
`/mnt/c/Users/ann/AppData/Local/Temp/Folio/clipboard/clip-20260915-140233.png`,
and into an agent pane **by §2.3's platform split, read on the string that is
emitted** — double-quoted when the spelled temp path is a Windows path, POSIX
single-quoted when it is a POSIX one, which under a `"paste_paths_as"` row is
the *spelled* form and not the host form the file was created at. The
earlier sentence here said "single-quoted" flatly, which was the withdrawn
one-form rule surviving in a second place; it is corrected. There is no second
rule for pictures, which is the point of writing the file at all: after the
write, #2 *is* #1.

### 4.5 What `PRIVACY.md` and `README.md` must say

A new row in `PRIVACY.md`'s **Elsewhere** list, in both languages, in that
section's voice, saying all of:

* the two directories by name, `%TEMP%\Folio\clipboard` and
  `$TMPDIR/folio-<uid>/clipboard` — or `/tmp/folio-<uid>/clipboard` when
  `$TMPDIR` is unset or empty, and with the `<uid>` written as what it is — and
  that the files are readable only by you. The delete command for each platform
  names the same directory, so the row and §4.1 cannot drift apart again;
* what is in them — *a PNG of whatever picture was on your clipboard when you
  pasted, or of a picture you dropped on a pane* — and that the PNG keeps
  whatever metadata the source put in it;
* that a file is written **only** by those gestures: no clipboard is watched and
  nothing is written when you copy;
* that files are removed after seven days, that the directory is capped, that the
  system may remove them sooner, and that **none of this reaches a path already
  written into your shell history, an agent's own records, or a copy the
  recipient made** — deleting the directory does not undo those;
* that `%TEMP%` can be redirected to a network or roaming location by policy, in
  which case the picture is written there;
* the delete command for each platform;
* the switch that turns it off.

And one clause in the README's privacy paragraph (`README.md:87`), which today
accounts for the network and for settings, profiles and sessions: a picture
written to a temporary file is a **new kind of thing on the disk**, and a
paragraph that lists the kinds has to list it.

### 4.6 The one setting

**Ruling ㉙: one switch, and it covers every picture file Folio creates.**
`Settings ▸ General ▸ Save a pasted picture as a file`, default **on**. Off:

* a clipboard picture pastes nothing and says so in a toast;
* **a dropped picture does the same** — the first draft's switch named the
  clipboard only, while §3.4 writes files too;
* **a job already in flight is cancelled and its file deleted**, so that turning
  the switch off does not leave a write landing a second later;
* **the Paste picture row is disabled**, not enabled-and-then-apologising. A row
  that lights up and answers with "that is turned off" is the second kind of lie
  §7.1.5f is about.

Inserting the path of a picture that **already exists** on the disk is not this
switch's business and is never disabled by it: that is #1, and no file is
created.

There is **no** switch for the path paste. When the clipboard holds files there
is, in the overwhelming case, no text on it at all — Explorer's `Ctrl+C` offers
none, and *Copy as path* offers text and is therefore already the text rung,
quotes and all, unchanged. A setting is a question asked of every reader forever;
ask it only when both answers are real. The mixed picture-and-text case gets the
**Paste picture** verb of ruling ② instead of a second persistent question.

The description is two sentences in the settings voice — written statement,
reader's perspective, no English mode names inside the Chinese, at most two
lines, pinned by the budget test — and it names where the file goes.

---

## 5. Where the feature lives

```text
bt-platform     clipboard_payload()  -> Files | Text | Picture(Vec<bytes + what they are>)
                                      | Refused(UnsupportedKind) | Nothing        [five states]
                                        each candidate rung: Absent | Present | Unreadable
                                        one open interval; macOS changeCount, Windows no
                                        sequence-equality predicate (ruling ④)
                                        acquisition errors answer Unreadable, not Refused
                the drop door:       register / continuous point / payload / effect
                    windows_impl:    own OleInitialize + IDropTarget on every HWND
                    macos:           PROBE 5 — destination view, upstream, or replacement
                the file-URL decoder shared beneath macos_services::paths_on
                (clipboard_text() unchanged, and still what every text field reads)

bt-transcript   PrintedPathNamespace::to_pane_spelling()   — beside to_local_path
                (no derive_namespace: profiles::printed_path_namespace is used as
                 it stands, in both directions — ruling ⑫)
                PrintedPathNamespace::Cygwin  — T-PASTE-CYG only, both directions

bt-app          shell_literal.rs     representability gate + ShellGrammar + encoders  [pure]
                                     spelling-then-grammar composition, ruling 12b
                profiles::grammar()  — derive_integration's twin, plus "paste_as"
                                       (reads the row's program; the NAMESPACE
                                        resolver is the one that does not)
                profiles             "paste_paths_as" — the spelling override (§2.5),
                                     Windows-only, insertion-only
                main.rs              paste routing, drop landing, picture lane + typed
                                     job destinations and generations; the captured
                                     recipient lives on TerminalInsertion alone
                                     inserted_path_text (K144) absorbed and repaired
                the picture worker   decode, encode, write, the cross-process quota
                                     lock — and the hourly sweep
                hang_watch           one new station: ClipboardRead (ruling ④)
```

**The five-state payload is the crate interface, and the two report channels are
different.** `Refused` is the only *payload* value that raises a refusal toast
(§1.2). An `Unreadable` rung and every later failure of the picture lane — a
bound, a layout, a quota, a vetting, an encoder — report through the lane's own
error path, which reaches the same toast surface for a different reason and is
listed in §7.2. A caller that collapsed the two would have to guess which
happened.

**Ruling ㉚: `bt-platform` hands over bytes and what they are; it does not
decode.** It carries no `image` dependency and should not grow one. `bt-app`
already depends on `image` (`crates/bt-app/Cargo.toml:64`), and the decode, the
bounds and the file write live there, above the platform line, where they are the
same code on both platforms. The *acquisition* bound (ruling ㉕) is the
exception and is enforced at the boundary, before the copy, which is what
`CONVENTIONS.md:34` asks of a boundary.

### 5.1 Bracketed paste

Inherited, unchanged, by doing nothing: the built line goes through `paste_text`
→ `input::paste_bytes` (`main.rs:110689`, `input.rs:696`), so it is bracketed
exactly when the shell asked and not otherwise. It is **not** disabled to bypass
a shell's paste hooks. §2.4's gate is what makes the sanitiser a no-op on our
text, rather than the old, false claim that a quoted path cannot contain a
control character.

### 5.2 What the encoder promises, and where the promise stops

**Ruling ㉛: the literal is correct at a fresh argument boundary, and nowhere
else.** The envelope is not quote-context awareness, and
`input_line_needs_a_space_first` sees one cell. Three cases are outside the
promise and are documented rather than defended against:

* **Inside an open token or quote.** Pasting after `Get-Content '` inserts a
  literal that closes the reader's quote and opens its own. Nothing this window
  can read tells it the shell is mid-quote.
* **A wrapped line at column zero**, where the character before the cursor is on
  the previous visual row and the one-cell look sees a blank.
* **A shell that rewrites the paste.** zsh's `bracketed-paste-magic` can run
  widgets over a paste and requote the whole of it, including turning a
  multi-file run into one string.

PSReadLine editing and PowerShell's native-argument marshalling
(`$PSNativeCommandArgumentPassing`) are later stages too, and §6.2's acceptance
asserts the **argument the program received and the file it opened**, not the
text on the glass.

### 5.3 The keyboard

`is_paste_shortcut_on` (`input.rs:299`) already answers `Ctrl+V`, `Ctrl+Shift+V`
and `Shift+Insert` on Windows and `⌘V` on macOS, and every one of them lands in
`paste_from_clipboard_into` (`main.rs:96290`) — as do the terminal menu's
`Paste` row (`main.rs:73388`) and macOS's `Edit ▸ Paste` action. One door, so the
feature arrives at all of them at once. A separate chord for path pasting was
considered and rejected: the reader's gesture is *paste*.

**Paste picture** (ruling ②) is a row on the **terminal's own right-click menu
and nowhere else**, plus a bindable command with no default chord. It is
deliberately **not** in macOS's menu bar: a menu-bar row is validated by AppKit
through `validateMenuItem:` whenever its menu opens *and* on key-equivalent
dispatch, neither of which the application schedules, so a row there would read
the pasteboard's type list at moments red line 2 could not enumerate. A
context-menu row is raised by the reader's own press, which is a gesture, and the
question disappears.

---

## 6. What is tested

### 6.1 Pure — necessary, and not sufficient

These pin the rulings. They do not pin the product: **no pure test observes an
argv, a shell's re-lexing, an agent's attachment or a native drop**, and §6.2 is
where those live. The first draft's claim that the pure set is "the whole of the
specification" is struck.

**The representability gate.** A non-UTF-8 `OsStr` refuses; an unpaired surrogate
refuses; LF, CR, TAB, ESC, NUL, a C1 control and DEL each refuse; the toast names
the entry; several files with one bad entry insert the rest.

**Each encoder, over its own table.** Space; apostrophe; each of PowerShell's
Unicode quote characters; backslash — one, two, and trailing; `"`; `$`;
backtick; `%`; `!`; `#`; `&`; `;`; `(`; `[`; `~` leading and interior; CJK;
emoji; a non-ASCII space; the drive root; a path that is one unsafe character;
and the combinations — apostrophe **with** a space, backslash **with** an
apostrophe. `Cmd`: 1, 2 and 3 trailing backslashes → 2, 4 and 6; `%` refuses; `!`
refuses under a `/v:on` row and passes otherwise; `^` is untouched. `Fish`: two
backslashes survive as two; an apostrophe becomes `\'`. `Nushell`: the whole arm
is behind the measured baseline of §2.3, so the unmeasured build's test is that
a `nu` row **refuses every path**; once a version is recorded, the fence-growth
test over a path containing `'#` is enabled and is gated on that version's
measured maximum rather than asserted unconditionally. `Agent`: the **POSIX**
output is one shlex token, asserted by running a shlex over it — and the
**Windows** output is *not* asserted that way, because `"C:\"` is a legal Windows
literal whose trailing backslash escapes the closing quote under shlex rules; it
is asserted against the recipient's branch order instead (§6.2).

**`to_pane_spelling`, over a normalised domain.** `D:\Demo\a.txt` →
`/mnt/d/Demo/a.txt` and `/d/Demo/a.txt`; `d:\x` and `D:\x` agree; `C:\` →
`/mnt/c/`; `\\wsl.localhost\Ubuntu\home\a\x` and `\\wsl$\Ubuntu\home\a\x` →
`/home/a/x` in that distribution's pane, untranslated in another's; a UNC share
untranslated; an unmounted drive spelled anyway, asserted **as the documented
default-mount assumption**; a `Windows` namespace translates nothing.

**Two round-trip suites, and they are separate.** One asserts
`to_local_path(to_pane_spelling(p)) == p` over the **supported** normalised
domain. The other lists the **known non-identities** and asserts each of them by
name — `\\wsl.localhost\Ubuntu\mnt\d\x` → `/mnt/d/x` → `D:\x` is the first entry.
A single suite claiming a universal identity would be a false pin.

**The derivations.** Every shipped row id on both seed platforms → its grammar
and its namespace, as a table, so that adding a row without deciding either is a
red test. A `pwsh` row with integration `None` is still `PowerShell` — the
grammar does not read integration. **A `gitbash` row with integration `None`
is `Windows` for spelling**, which is the standing 2026-09-07 derivation the
owner confirmed on 2026-09-15 (ruling ⑫), and the test carries that expectation
with the ruling's date beside it so that whoever changes it knows what they are
changing. **The assertion is scoped to the namespace resolver, and the earlier
blanket sentence is struck** (fourth review finding 11). It said "no derivation
reads a program", which would have forbidden `derive_grammar(&ProgramSource)`
(§2.2 ruling ⑧) — the thing T-PASTE-1 ships beside it. The owner's ruling settles
**namespace** derivation, not grammar derivation, so the three claims are tested
as three:

* **Namespace.** The resolver T-PASTE-1 uses is `printed_path_namespace`, and the
  assertion is about **variant selection**: which `PrintedPathNamespace` variant
  comes back is decided by `(paths(index), integration(index))` and by nothing
  else, in particular by no program. Populating the chosen variant's own context
  — the WSL distribution, the MSYS home — from elsewhere in the row is what the
  resolver already does and is left exactly as it is (narrowed, fifth review
  finding 6). No program-keyed `derive_namespace` is called, because none exists.
* **Grammar.** `derive_grammar` **does** read the row's resolved program, and the
  test asserts that it does — `PowerShellSeven` without a file name, a `FirstOf`
  row off its first candidate, `served_by` never consulted.
* **Spelling override.** `"paste_paths_as"` affects insertion only, and the
  detector's answer for the same pane is unchanged.

`wsl.exe -e nu` without an override is `Posix` and **the test says so, naming it
as the spawn-time default**; a `"paste_as"` row is believed, a
`"paste_paths_as"` row is believed, and the two keys are asserted **not to read
each other** —
`{"id": "gitbash", "paste_paths_as": "windows-slash"}` emits `'D:/Demo/a.txt'`,
POSIX quotes around a forward-slash Windows path, and leaves the detector's
answer for that pane unchanged. An unknown value of either key is refused.

**The grammar × spelling product** (ruling ⑫b), which independence does not
cover. Rules A–E are asserted as rules and the table's cells as their
consequences, and **each fixture states the probe policy it runs under rather
than inheriting the build's** (fifth review finding 2): `D:\John's Archive\a.txt`
× `wsl` × `PowerShell` is **two rows** — C1 refusal with no insertion under an
unrun PROBE 2, and `'/mnt/d/John''s Archive/a.txt'` once the recorded result has
narrowed the quote class to allow doubled `U+0027`; the drive root `D:\` × `Cmd` across the columns
→ `"D:\\"`, `"D:/"`, `"/mnt/d/"`, so the 2N rule fires in one column only; `D:\`
× `windows` × `Agent` → the `"C:\"`-shaped literal, exempt from shlex identity;
`\\server\share\a.txt` × `wsl` → the host string by rule D, taking `Agent`'s
**Windows** arm; `\\wsl.localhost\Ubuntu\home\ann\a"b.txt` × `msys` → the host
string by rule D, accepted under `Posix` and **refused under `Cmd` and under
`Agent`'s Windows arm** by rule C2; the same path × `wsl` on that distribution's
own row → `/home/ann/a"b.txt`, `Posix` accepts and `Cmd` refuses; a `wsl`
spelling on a **non-WSL** row spells `/mnt/d/…` with the distribution
`wsl_distribution` answers and, with neither a row argument nor a machine
default, leaves a distribution share untranslated rather than guessing;
`{"paste_as": "cmd", "paste_paths_as": "msys"}` refuses `D:\Data\100%\r.csv` and
the same row without `"paste_as"` emits `'/d/Data/100%/r.csv'`; and on a Unix
seed platform every value of `"paste_paths_as"` is refused at the row by rule E,
once, with the pane's spelling unchanged.

**The Cygwin arm, in T-PASTE-CYG's own suite** (ruling ⑫a): a positive row
spells `/cygdrive/d` and is recognised back; the classifier's table is asserted
over all of its outcomes — shipped Git Bash candidate, ineligible program,
partial read, Cygwin-only union, MSYS-only union, both, empty — with only the
Cygwin-only union changing any answer. **Eligibility is tested before the
disk**: a non-shell executable sitting beside `cygwin1.dll` keeps today's answer
**and reads no directory at all**, which the fixture asserts by counting the
listings the classifier performed, so that a later refactor cannot turn the
cheap rule into an expensive one that happens to answer the same. **Aggregation
is tested for order-independence**: a Cygwin-only executable directory beside an
MSYS-only `..\usr\bin\` answers today's answer whichever location the fixture
presents first, and the mirror pair answers the same. **A partial read stops the
classification**: one readable Cygwin-only directory beside a sibling that exists
and cannot be listed keeps today's answer, while the same Cygwin-only directory
beside a sibling that **does not exist** is the positive match, because an absent
location is not a failed read. And **with PROBE 11 unrun, every outcome including
the positive one keeps today's answer, and `"paste_paths_as": "cygwin"` is
refused with the unmeasured-spelling message** — one gate covering the automatic
match and the explicit override alike, which is the test that stops an unmeasured
build enabling either.

**Rung semantics.** A fake clipboard described by each rung's answer —
`Absent` / `Present` / `Unreadable`, and `Present`-but-empty — across every
combination, asserting the chosen rung, that `Unreadable` stops the ladder, that
an empty `HDROP` is `Absent`, that an empty text is `Present`, that a
promise-only clipboard is `Refused(Promise)` and **not** `Nothing`, that
`Nothing` raises no toast and `Refused` does, and that a picture rung carrying
`[Png, Dib]` falls to the `Dib` when the `Png` fails to decode — and is terminal
when the list was truncated at the bound.

**The ladder's two fetch rules, which the earlier set asserted as one.** That the
survey **selects candidates and fetches nothing**; that an advertised but empty
`HDROP` **beside readable text** fetches the file rung, finds it `Absent` from its
acquired content, then fetches text and answers `Text` — the case a
survey-decides-everything ladder could not reach; that a macOS URL representation
whose items are all non-file URLs behaves the same way; and that in the ordinary
case exactly one rung is fetched.

**Coherence, per platform — and the two platforms are asserted for different
things, because the two APIs promise different things** (revised, fourth review
finding 2). On the macOS fake, a moved `changeCount` discards and refuses: AppKit
makes no exclusion promise, so a replacement under the read is a real event with
a real refusal. On the **Windows** fake the assertion is the **exclusion
guarantee itself**, which is what ruling ④ made the coherence rule:

* a **sequence number that moved because the delayed format rendered** does
  **not** discard — the paste succeeds — which is the fixture that pins ruling
  ④'s correction;
* while Folio holds the clipboard open, a competing process's `OpenClipboard`
  **fails**, and the fake asserts that failure rather than modelling it away;
* Folio's own acquired snapshot **completes** under that held interval and is
  delivered;
* the competing process's replacement, made after the close, is what the
  **next** gesture reads — a second paste answers the new contents.

The earlier fixture required "a genuinely replaced clipboard" to refuse "by the
rule that is not sequence equality", and there is no such rule: inside the open
interval the API does not permit the replacement, and a replacement outside it is
simply a different clipboard for a different gesture. A fake that let another
process write while Folio held the clipboard would be modelling an API guarantee
away in order to test a refusal the design does not have. **No sequence equality
is asserted anywhere**, and no path in the suite reopens the clipboard after a
discard. If a real Windows invalidation is ever found, it enters here as a named
observable failure with its own refusal path — not as a restored equality test.

**The picture.** Known-pixel fixtures per supported DIB layout — top-down and
bottom-up, `BI_RGB` and `BI_BITFIELDS`, V5 with and without a real alpha mask,
palette forms — asserting the written pixels; an unsupported layout refuses; a
truncated PNG with a valid `IHDR` refuses; dimensions over the cap refuse before
allocation; **an animated PNG whose default image differs from animation frame
one is written whole, with every frame decoded during validation and the toast
saying it is animated**; a truncated animation refuses; an animation whose frames
together exceed the decode cap refuses rather than being flattened.

**The file and the sweep.** The name from a fixed stamp; the collision ladder to
`-3`; the owned-name grammar accepting exactly its own shape and rejecting
`clip-family.png`; age by mtime, on both sides of the cutoff; a directory, a
symlink and a foreign file untouched; the quota evicting oldest-first; **a file
younger than 24 hours never removed by quota eviction, by the startup sweep or by
the hourly sweep**, with the write refused instead when eviction cannot free
enough; and `.lock` itself never swept and never counted against the quota.

**The lock.** A second holder cannot take it while the first holds it; the
contending worker waits off the loop and **refuses after the two-second limit**
rather than writing unserialised; a holder that dies releases it with no age rule
and no unlink; **no path in the code removes a lock it does not hold**; and the
accounting, the eviction, the reservation and the write are all shown to happen
inside one hold.

**Job identity, per destination type.** For a `TerminalInsertion`: a completion
whose leaf is gone does not insert; whose incarnation changed does not insert;
whose modal opened does not insert; whose setting was turned off does not insert
and deletes its file; **whose own path fails the representability gate does not
insert and deletes its file**; **whose path is refused by the captured
recipient's encoder is never written at all**, because that check runs before the
write (§4.1); and a second picture paste while one is pending is refused.

**The input generation, which is the case a unique address does not cover — and
the editing half a list of typed characters does not cover either** (revised,
fourth review finding 5). Each of these, delivered to the job's own target before
completion, invalidates it: nothing is inserted, the file is deleted through the
owned path, and the toast says the line moved on.

* **Submission and typing.** Enter; a typed character; an ordinary text paste (the
  path the code already records as the reader putting bytes in,
  `main.rs:96320`–`:96322`); a K144 insertion; a drop insertion.
* **Navigation.** `Left`, and `Home` — each of which moves the cursor inside the
  line the job was going to be appended to without changing a character of it.
  These are the rows that would have passed under the third revision's list.
* **History recall.** `Up` at the prompt, which replaces the whole line with a
  different command.
* **Editing and completion.** `Backspace`, which removes the character the
  leading-space rule was going to be asked about; and `Tab`, which asks the shell
  to rewrite what is there.
* **Control keys.** `Ctrl+C`, which abandons the line, and `Ctrl+U`, which clears
  it.
* **An IME commit** into that target.
* **Wheel-generated navigation** (added, fifth review finding 1): a wheel notch
  over the job's own target while that pane is on the alternate screen with
  alternate scroll on, which `WheelRoute::ArrowKeys` delivers as `Up` / `Down`
  bytes from `input::alternate_scroll_bytes` (`main.rs:95117`–`:95136`,
  `input.rs:456`–`:468`). The fixture asserts **cancellation, no insertion and
  deletion of the owned output**, and it is asserted in **both** cursor modes,
  since `ESC O A` and `ESC [ A` are the same gesture.
* **A forwarded mouse report** into that target — a press, a wheel notch routed
  to `WheelRoute::MouseReport`, or motion under `?1002h`/`?1003h` — which
  advances it conservatively (§4.2).

And four negative rows, so the check is a check and not a refusal machine: a
completion with the generation **unchanged** still inserts; **a terminal reply**
drained back to the child (`main.rs:35630`) does not advance it; **a local wheel**
over the same target — `WheelRoute::Local`, the pane not on the alternate screen,
or scrolled back, or Shift held — does not advance it and the job completes
normally, which is the control that keeps the new row about forwarded bytes
rather than about the wheel; and **the same inputs delivered to another pane** —
the keys, the alternate-screen wheel and the mouse report alike — do not advance
this target's generation, the job completing normally, which is what makes the
rule per-target rather than global.

**The other two destinations.** A `PreviewRetarget` whose pane has since been
pointed at something else does not retarget and deletes its file; one whose
content generation is unchanged does. A `LayoutSplit` carrying a
`SeatEdge` anchor whose seat is gone, whose tab has changed layout generation, or
whose side no longer admits a split, does not split and deletes its file; one
whose anchor is intact splits **on the captured side**, and the test asserts that
the landing was not recomputed from the current pointer or the current layout.
**And the `TabRoot` anchor gets its own pair** (fourth review finding 6): a
delayed picture dropped on a **root rim** whose tab and root are untouched
**splits the root on the captured edge** — the success case the earlier fixtures
had no shape for — while one whose captured tab has closed, or whose layout
generation has moved because the root was restructured under it, **cancels and
deletes its file**, and is asserted **not** to have been repaired into a
`SeatEdge` against whichever pane now occupies that edge.

**Neither nonterminal destination runs a shell preflight**, which is asserted
directly: a `PreviewRetarget` and a `LayoutSplit` created while every open
terminal is a `cmd.exe` row, into a temp directory whose name contains `%`, still
write their file and complete — the case that would fail if the common preflight
had stayed common or if a focused pane were borrowed as a surrogate recipient.
What they do run is the native file-open check, asserted by a prospective path
over the platform's length limit refusing with no file created.

**The line put at the prompt.** A leading space only when the cell left of the
cursor is not blank (the shape at `main.rs:152531`); a trailing space always;
three files in source order; drop insertion moves no focus while K144 still does.

**i18n.** The new toast strings, the setting's two lines, the `Text::ALL` count
(660 today) and the description's two-line budget.

### 6.2 Measured, on both machines — where the feature is actually proved

Every row asserts **the argument the program received or the file it opened**,
not a screenshot. `BT_PTY_DUMP` is on for these runs and the records are kept;
the records are *ours*, and §7.3 distinguishes them from a reader's own.

**The source matrix (PROBE 1).** Source × gesture × advertised formats × chosen
rung, filled in by measurement, for: Explorer one file / three files / a folder /
*Copy as path*; Finder one file / three files; Excel a cell range and
copy-as-picture; Word text and an image; a browser text selection and *Copy
image*; Snipping Tool; `Win+Shift+S`; `⌃⇧⌘4`; a screenshot tool configured to
offer a path beside the picture; **and the three bridged clipboards, which are
where a synthesised type list is most likely to disagree with itself — a file
copied in a WSLg GUI file manager, `clip.exe` text from a WSL shell, and a file
and an image copied inside an RDP session.** Each row records the product or
bridge version.

**Delayed rendering, which is the source matrix's other half** (ruling ④). A
**delayed-render success** row: a Windows source that renders its format only
when asked — Excel's copy-as-picture and a browser's *Copy image* are the
candidates PROBE 1 names — where the **first** paste after the copy must succeed,
because that is the paste the withdrawn sequence-number test would have refused.

Beside it, a **concurrent copy** row that asserts what the API actually
guarantees rather than a refusal it makes impossible (revised, fourth review
finding 2). A second process attempts a copy while Folio is inside its open
interval, and the row records all four of: that process's `OpenClipboard`
**failed**; Folio's snapshot **completed** and the paste landed; the second
process's copy succeeded once Folio had closed; and the **next** paste gesture
delivered that new content. No sequence equality and no ordering assertion — the
row is four observations, each independently checkable, and it is the same row on
a physical machine that the §6.1 fake pins in the small. On **macOS** the pair is
different and stays different: `changeCount` before and after, a genuine
replacement under the read, which must **refuse** and must not reopen, because
AppKit makes no exclusion promise for Folio to assert.

**The shell matrix.** A file whose name has a space, an apostrophe, `$`, `%`,
`!`, a PowerShell smart quote and CJK, into `pwsh`, `winps`, `cmd`, `gitbash`,
`wsl`, `zsh`, `bash` and a `fish` row — each line then **run**, and the opened
file compared. **A cell the design refuses is a refusal observation, not a line
to run** (fifth review finding 2): with PROBE 2 unrun the apostrophe and
smart-quote names are refused in the two PowerShell rows by C1 and the `%` and
`!` names are refused in the named `cmd` row by C3, and the matrix records the
toast and the absence of bytes in the pane instead of an opened file. The rows
that run are the ones the configured policy accepts, and the row's expected
outcome is stated with the policy beside it, so a build that later narrows a
refusal set changes which half of the pair is exercised rather than turning the
matrix red.

* **Under `cmd`**, a builtin (`cd`, `type`) *and* a native child (`findstr`, a
  tiny argv printer) for the trailing-backslash rows, with `/v:on` and without.
* **The `Cmd` encoder outside `cmd.exe`** (ruling ⑩) is **two rows, not one**
  (revised, fourth review finding 3). The third revision had a single row that
  launched the argv printer "from a `cmd` row, with `%` in the name, which must
  *work* rather than refuse" — while §2.3 requires a percent-bearing path to be
  **refused** in a `cmd.exe` pane. Choosing a native child does not get past
  Folio's row-based refusal or past cmd's own interpretation, so that row either
  failed its own gate or asked for an exception to the behaviour it was testing.
  The two questions are separated:
  * **The encoder, with no interpreter.** The test harness builds the encoded
    command line and hands it **straight to `CreateProcessW`** — no `cmd.exe`
    anywhere — and compares the argv the consumer reports against the intended
    path. This is the platform's own process-creation convention, read by the CRT
    parser it belongs to, which is exactly what ruling ⑩ promises. **The harness
    is specified down to the argument index, because the index is the whole
    test** (revised, fifth review finding 4): the consumer is a small
    purpose-built **MSVC CRT `wmain`** program that prints its arguments;
    `lpApplicationName` points at that program; and `lpCommandLine` is a
    **mutable** buffer holding a correctly quoted **program-name token**, a
    space, and then the **exact tested literal** the encoder produced. The
    assertion is `argc == 2` and `argv[1]` equal, code unit for code unit, to the
    intended UTF-16 path. The fourth revision left the command line as the
    encoded path alone, which would have put the literal in **`argv[0]`** — and
    `argv[0]` is parsed by a different rule, under which the backslash and quote
    conventions this row exists to test do not apply, while `CreateProcessW` does
    not prepend `lpApplicationName` as an argument to make up the difference.
    Rows: one, two and three trailing backslashes; a space; an apostrophe; a lone
    `%`; **a paired `%NAME%`**, which is the row that distinguishes cmd's
    expansion from a harmless percent sign and which the encoder must pass
    through unchanged because it has no interpreter in front of it. The
    `cmd`-profile refusal row below stays separate, and no interpreter and no
    REPL is restored by any of this.
  * **Terminal acceptance, in a `cmd` profile.** A lone `%` and a paired `%NAME%`
    are each **refused** with the toast §2.3 names, and no bytes reach the pane.
    That is Folio's row-based rule, and it is asserted where it lives.

  The earlier row before both of these asked a `python` or `node` **profile** to
  open the file, which tested a REPL's string-literal grammar rather than the
  platform's argument convention and would have failed on `\n` and `\t` for
  reasons that are not this encoder's; §2.2 explains the withdrawal, and no REPL
  acceptance row replaces it until a REPL grammar is added. If a non-cmd
  *interactive* row is ever used for an encoder row, it must be one whose input
  protocol actually consumes the grammar under test, named in the row.
* **Under PowerShell**, 5.1 and 7, with PSReadLine default and with bracketing
  off. Under zsh, default and with `bracketed-paste-magic`.
* **Under `nu`** — the baseline run, which is a prerequisite of the arm existing
  at all (§2.3): on the named version, a width-one raw string in a **builtin's**
  argument position and in an **external command's** argument position, and then
  a path needing a wider fence (PROBE 10). With no baseline recorded, the row is
  that a `nu` pane refuses.
* **Under Git Bash**, an MSYS consumer and a native consumer with
  `MSYS2_ARG_CONV_EXCL` unset and set (PROBE 4) — and the override row that
  PROBE 4's unrun fallback promises: `{"id": "gitbash", "paste_paths_as":
  "windows-slash"}` emits `'D:/Demo/a.txt'`, the native consumer opens it, and
  the pane's underlines are unchanged.
* **Cygwin rows, in T-PASTE-CYG** (ruling ⑫a): a Cygwin `bash.exe` row with
  integration left automatic and one with integration `None`; the Folio-shipped
  Git Bash wrapper, to show what actually sits beside `<Git>\bin\bash.exe`; a
  machine carrying both runtime DLLs; **a non-shell executable installed beside
  `cygwin1.dll`** — a Cygwin `rsync.exe` or `python.exe` as a profile's program —
  which must keep today's answer for the eligibility reason and not for a DLL
  one; **a cross-level mixed installation**, Cygwin-only in the executable's
  directory and MSYS-only in `..\usr\bin\`, which must answer today's answer from
  either direction; **a sibling directory that exists and cannot be listed**,
  which must keep today's answer while the same row with **no** sibling directory
  at all is the positive match; **the explicit `"paste_paths_as": "cygwin"` on a
  build whose PROBE 11 is unrun**, which must refuse with the unmeasured-spelling
  message; and a Cygwin whose cygdrive prefix has been moved, which must be a
  disclosed wrong answer repaired by `"paste_paths_as"` rather than a silent one.

**The agents (PROBE 3).** Claude Code, Codex and Copilot CLI, versions recorded:
a spaced path, **an apostrophe path on both platforms**, **a Windows drive root
(`"C:\"`) and a UNC root**, two paths in one insertion, and a `file:` URI — and
for each, whether it attached, referenced or ignored the file. The rows are
asserted **against the recipient's own branch order** — quote-strip, `file:` URL,
Windows recogniser, shlex — and **shlex identity is asserted only for POSIX
outputs**, for the reason §2.3 gives. The Windows apostrophe row is the one that
§2.3's reading of `normalize_windows_path` predicts, so it is also the row that
checks the reading; the Windows multi-path row is the one that checks the
*narrowed* claim, since a combined line can reach the recogniser before shlex
ever runs. Claude Code's and Copilot CLI's documented prose-and-`@` behaviour is
**not** evidence about escaped apostrophes: those rows are measurements, and until
they are taken the six non-Codex recipients carry a recorded inheritance.

**The drop matrix.** From Explorer and from Finder onto: a terminal centre, a
terminal edge, a preview centre, a files column, the tab strip and the window
chrome; out of the window and back, so the box un-traces; a drop while a menu is
open; 3 files; 64 files; 65 files (refused); mixed file-and-folder on a terminal
and on a preview (refused); **a folder held over a Preview centre, which must
trace the refusal box while the hand is open and refuse on release**, beside a
file on the same rectangle which retargets, and the mirror pair on a Files column
(§3.4); **a promise-only drag, which must trace the refusal box rather than pass
through unseen**; and a drop on a `Placeholder` pane.

**The delayed picture drops, which have destinations of their own** (ruling ㉖).
A picture dropped on a **Preview centre** where the reader opens something else
in that pane before the encode finishes: nothing is retargeted and the file is
gone. The same drop with the pane untouched: it retargets. A picture dropped on a
**seat edge** where the anchor pane is closed before completion: nothing is
split and the file is gone; with the anchor intact, the split lands **on the side
the hand chose**, not on a side recomputed afterwards. And a picture dropped on a
**terminal centre** where Enter is pressed before completion: nothing is
inserted. A
browser image drag from Safari, Chrome and Firefox (PROBE 6). On Windows: a
second window and the summoned terminal, to prove every HWND registered; **a
fullscreen transition with a target registered, because that is the one path
winit still initialises COM on** (`platform/windows.rs:493`); a DPI-changing
monitor, to pin the coordinate conversion; and a source that would have accepted
a move, to prove the effect returned.

**The storage.** On Unix, the two levels are `runtime_directory()`'s
`folio-<uid>` and its `clipboard` child, and each row is run at **both**: a
world-readable existing directory is repaired; a foreign-uid directory refuses; a
symlink at either level refuses; a file lands `0600`.

On Windows, one row per clause of ruling ㉓, and **the protection flag is
asserted in the direction the API defines** (revised, fourth review finding 4):

* **A protected directory succeeds.** A `Folio` or `clipboard` level already
  carrying `D:P(A;OICI;FA;;;<user SID>)` — protected, one ACE, inheriting —
  **passes untouched**. This is the row the third revision's rule got backwards:
  it required the directory *not* to be protected, so the most securely
  configured directory on the machine was the one that refused.
* **A permissive parent does not reach in.** The `%TEMP%` parent is given a
  broad inheritable ACE — `Everyone`, or a second local account — *after* Folio's
  two levels exist, and the row asserts that the two levels' effective DACLs are
  **unchanged**, because `SE_DACL_PROTECTED` is what stops an inheritable ACE
  arriving. Then a picture is written and its file's DACL read back: still the
  one principal.
* **Repair, and refusal.** An existing directory with a **broad DACL**, or with
  the one ACE but **not protected**, that the current user owns is rewritten to
  the exact form and then passes; one owned by **another SID** refuses and is not
  repaired.
* **Inheritance flags are checked, not assumed.** A level whose ACE lacks
  `OBJECT_INHERIT_ACE` / `CONTAINER_INHERIT_ACE` refuses.
* **Principal identity.** A directory whose single ACE names the **logon** SID
  rather than the user SID refuses — the row that keeps the two apart, and that
  would otherwise make the reader's own pictures unopenable after the next logon.
* **Reparse points** at either level refuse.
* **The created file** is written with its own explicit protected descriptor, its
  DACL is **read back before any picture byte is written**, and it must grant the
  user SID only; a read-back that disagrees is a refusal and a delete.
* **Storage that cannot enforce it.** A `%TEMP%` redirected to a volume that
  **cannot keep persistent ACLs** — an exFAT stick, a share that does not carry
  them — refuses the lane rather than writing a file it cannot make private. A
  `%TEMP%` redirected to a share that *can* keep them writes there and is
  disclosed.

**The picture job's own preflight, at §6.1's contract and on a real machine**
(fourth review finding 9). Two refusal rows, each **paired with an acceptance row
over the same directory**, so that a directory refused for every pane cannot
satisfy the test by accident:

| Directory | Recipient | Expected |
| --- | --- | --- |
| a valid, vetted `%TEMP%` **redirected to a path containing `%`** | a `cmd.exe` row | **refused before the write**; the encoder's own toast; `clipboard\` contains **no new file** |
| the same directory | a `pwsh` row | written, and the path inserted — the pairing that proves the refusal was the recipient's |
| an ordinary valid directory | a **`nu` row with no measured baseline** (§2.3) | **refused before the write**; the unmeasured-lane toast; **no file created** |
| the same directory | a `bash` row | written, and the path inserted |

Both refusal rows assert **file absence**, not merely that nothing was inserted:
the whole point of moving the check before the write is that no picture reaches
the disk. T-PASTE-2b's gates name these four rows.

**Both.** The Markdown editor, the palette, the search field and the settings
fields still paste **text** when the clipboard holds a file — the regression this
feature is most likely to cause. And the non-UTF-8 name on an SMB or exFAT volume
**refuses visibly and names the file**, which is what §2.4 changed.

---

## 7. Red lines

### 7.1 Never

1. **Make a network request of its own.** Nothing in this feature has an
   address. A path on a UNC share, or a dropped file on a network volume, is
   ordinary filesystem I/O the reader asked for by dragging it; that is a
   different thing from a request this feature initiates, and the line is drawn
   at the one it can control.
2. **Read the clipboard's *content* except on the reader's own gesture** — a
   paste, a **Paste picture**, or a drop. No watcher, no timer, no read on
   focus. The earlier draft wrote this as a prohibition with its own exception
   inside the same sentence, which no test could pin; it is one rule with one
   positively stated permission now:
   > The clipboard's **type list** may be read when the terminal's own
   > right-click menu — the one menu that carries **Paste picture** — is raised
   > by the reader. The clipboard's **content** may be read only on a paste, a
   > **Paste picture** or a drop.

   `Paste` stays always enabled and never consults anything. **Paste picture** is
   enabled from that type-list read; **if the read fails** — another process is
   holding the clipboard — the row is **disabled, not hidden**, so the menu does
   not change shape on a race. The row is not in the macOS menu bar, for the
   reason §5.3 gives. The third-party tools people currently use for #2 watch the
   clipboard continuously; this one does not.
3. **Persist what the clipboard held, with one named exception.** Not in
   `session.json`, not in `pins.json`, not in `diagnostics.log`, not in a `BT_*`
   trace. A diagnostic line may name the rung that answered and the number of
   paths; never a path, a file name or a byte of text. **The exception is the
   feature itself**: a picture is written to a file, which is what #2 asked for,
   and §4.5 discloses it in both languages.
4. **Substitute `U+FFFD`, or send a filename's control characters.** §2.4
   refuses instead.
5. **Resolve a symlink, or canonicalise.** The path pasted is the path the source
   named; handing the shell a different name is the same lie a wrong underline
   is. On Windows there is a second reason: `canonicalize` returns a `\\?\`
   prefix that half the shells in the matrix cannot open.
6. **Overwrite an existing file.** `create_new` only.
7. **Follow a link, or a reparse point, at any level of its own directory**, or
   write into a directory it has not vetted on this operation.
8. **Press Enter**, or emit anything that becomes one. §2.4's refusal of line
   terminators is what makes this true rather than hoped for.
9. **Read a file promise**, or render a delayed format that would make another
   process write a file. A drag's promise types are registered (ruling ⑤), but
   only so that the refusal has a rectangle to be drawn on: the drag is never
   accepted, `performDragOperation:` is never entered, and nothing is ever
   asked to deliver.
10. **Move, copy or delete a file the reader dragged**, or tell a drag source
    that a move occurred.
11. **Answer a highlight with nothing.** Every rectangle that lights under a held
    file has a verb, and the batch and payload rules of §3.2 and §3.4 are decided
    before the highlight, not at the release.
12. **Change the reader's environment** to make its own answer correct — no
    `MSYS2_ARG_CONV_EXCL`, no `$PSNativeCommandArgumentPassing`, no injected
    `$(wslpath …)`.

### 7.2 What the reader is told instead

A refusal is never silent. Every refusal in this design — unrepresentable path,
`%` in a `cmd` pane, a path a `nu` row cannot spell while its baseline is
unmeasured, an unsupported bitmap layout, an unvalidatable animation, an over-cap
picture, a busy job, a full quota, a **contended quota lock**, a vetting failure
(including a Windows directory owned by somebody else and storage that cannot
keep an ACL), an output the captured recipient's encoder refuses, a promise-only
payload, an over-count drop, and a picture job **invalidated because the line
moved on** — is a toast that names what was refused and why, in both languages.

**Which channel each of those uses, split correctly** (revised, fourth review
finding 10). The sentence that stood here said "these" — the whole list above,
promise-only included — "reach the reader through the lane's own error channel,
which is not the payload's `Refused` value", which contradicted §1.2 and §5,
where a promise-only clipboard **is** `ClipboardPayload::Refused(Promise)`. The
list has two kinds in it:

* **The payload's own value.** A **promise-only clipboard** is
  `Refused(Promise)`, the one payload value that raises a toast (§1.2). That is
  its routing, here and in §1.2 and §5, and it is not the lane's error channel.
* **The error paths.** An **acquisition** failure is the rung's `Unreadable`
  verdict (ruling ③), reported through the acquisition error path §5 names. Every
  **later** failure — the representability gate, `%` in a `cmd` pane, an
  unmeasured `nu` row, an unsupported bitmap layout, an unvalidatable animation,
  an over-cap picture, a busy job, a full quota, a contended quota lock, a vetting
  failure, an output the captured recipient's encoder refuses, an over-count drop,
  and a picture job invalidated because the line moved on — reports through the
  **application's own refusal-reporting path**, the one that raises the refusal
  toast for any gesture that got as far as having something to refuse. **That
  path is not the picture worker's** (narrowed, fifth review finding 6): the
  first three of those failures are path refusals that T-PASTE-1 ships before any
  picture exists, and the picture lane is one of its callers rather than its
  owner, so nothing in §7.2 makes a path refusal wait for a picture feature.

All three reach the same toast surface for different causes, which is why a
caller that collapsed them would have to guess what happened; §5 carries the same
split in the crate map, and no required toast and no silent `Nothing` changes.

### 7.3 What is disclosed rather than forbidden

`BT_PTY_DUMP` writes every byte of a pane to a file the reader names, and
`PRIVACY.md:186` already says so; a pasted path is in that file like any other
byte. That is an opt-in recording, not this feature's storage, and §4.5 keeps the
two apart. The Mac package is **not sandboxed**
(`packaging/macos/entitlements.plist:20`), so no App Sandbox container blocker is
invented here, and **whether a child of a Folio row can read a file under
`$TMPDIR` is not an open question**: children inherit `TMPDIR`, and this product
already depends on that answer for its per-run socket. The earlier draft asked it
here anyway, after §9.1 had already withdrawn it; that sentence is **struck**.
What remains to be measured is one thing — whether any TCC prompt appears on the
paths this lane writes. **PROBE 9**, whose unrun behaviour §9.1 states
operationally, and there is no fallback copy into a second location if a denial
does arrive — a refusal instead.

---

## 8. The split

Four tickets. Each is independently shippable, and each states the lane it does
**not** ship so that the refusal is explicit rather than a gap. The fourth is the
Cygwin namespace, kept out of the first on purpose (ruling ⑫a).

### T-PASTE-1 — the clipboard door, and one path as one argument

**Size: L.** It was sized M–L on the assumption that the encoder was a table; it
is six encoders, a refusal gate, two per-row overrides and a recipient contract.
**It is not a namespace change**: the earlier sizing called it "a two-direction
namespace change", which contradicted §2.5 and is struck — the namespace
derivation this ticket uses already exists and is not touched.

`clipboard_payload` on both platforms with the **five-state payload**, the
three-state candidate semantics, the per-platform coherence rules of ruling ④,
and the shared file-URL decoder. The representability gate. The six encoders —
minus the `Nushell` arm, which does not ship until §2.3's baseline is measured
(a `nu` row refuses until then). `derive_grammar`, the `"paste_as"` grammar
override and the **`"paste_paths_as"` spelling override**, with
`printed_path_namespace` **used exactly as it stands** (ruling ⑫): no
`derive_namespace`, no detector change, no `Cygwin` value accepted yet.
`to_pane_spelling`. The routing in `paste_from_clipboard_into`, the
`ClipboardRead` hang station, and the **Paste picture** row's enablement rule
(the row itself is T-PASTE-2's). **K144 absorbed and repaired**, keeping its
focus move. **Its own i18n row** — it ships at least six refusal toasts, so it
adds strings, needs its `Text::ALL` count bumped and carries the two-line budget
test; every ticket that adds a string keeps an i18n row, and the first ledger's
row 23 is corrected to say so. The rest of §6.1 except the picture, file, sweep
and job rows; the shell and agent matrices of §6.2; PROBEs 1–4 and 10.

**Gates:** every §6.2 shell row asserts the opened file; PROBE 2 answered **or**
the documented quote class refused (§2.3's concrete fallback); PROBE 3 answered
**or** the inheritance of Codex's spelling by the other six agent rows recorded
as a known risk — not a refusal, which is a product decision the grammar table
does not carry, **and that is the policy at every release size, including a
T-PASTE-1-only one**; PROBE 4 answered or the `"paste_paths_as": "windows-slash"`
row of §6.2 demonstrated; PROBE 10 answered on a named nushell version or the
`Nushell` arm not shipping at all; **and §6.1's grammar × spelling product rows
(ruling ⑫b) green** — the composition contract is this ticket's, because it ships
both keys. **Does not ship:** pictures, drops, the
`Cygwin` namespace. A clipboard picture is `Absent`, which makes the whole
payload `Nothing` and therefore **silent** — ruling ② keeps `Nothing` silent, so
this ticket does not add a "nothing to paste" message that the reader would see
on an empty clipboard as well; the earlier draft asked for one and contradicted
§1.2. A drop does nothing, as today.

### T-PASTE-CYG — the Cygwin namespace, in both directions

**Size: S–M**, and deliberately alone. It adds the `Cygwin` variant to
`PrintedPathNamespace` (`paths.rs:61`), the `/cygdrive/<letter>` spelling for
insertion and the matching recognition for the detector, the classifier table of
ruling ⑫a with all **seven** of its outcomes — the count is the table's, which
gained its eligibility row in the fourth revision (corrected, fifth review
finding 6) — the `"paste_paths_as": "cygwin"` value,
and §6.1's and §6.2's Cygwin rows.

**Gates:** PROBE 11 answered on a real Cygwin and a real MSYS2 installation,
**including what sits beside the Folio-shipped Git Bash wrapper**; the
eligibility, cross-level aggregation and partial-read rows of §6.1 and §6.2
green; with the probe unrun, every classifier outcome keeps today's answer, the
explicit `"paste_paths_as": "cygwin"` refuses, and the arm ships dark — all three
of which are themselves tests. **Does not ship:** any change to the
`(paths, integration)` derivation itself (ruling ⑫), MinGW as a namespace, or any
reading of `/proc/cygdrive` at paste time. **Nothing depends on it**: T-PASTE-1, 2 and 3
ship with or without it.

### T-PASTE-2 — a picture becomes a file

**Size: L–XL, and it splits in two reviewable halves.**

**2a — acquisition and decoding.** The picture rung on both platforms, the
acquisition bound, the `new_without_file_header` path, the alpha and layout
table, full-decode validation with original-byte write, the animated-PNG rule,
the fixtures, PROBEs 7 and 8.

**2a carries a manifest change of its own, and it is not TIFF's.**
`BmpDecoder::new_without_file_header` lives behind `image`'s **`bmp` feature**
(`image-0.25.10/src/lib.rs:245`–`:246` gates `pub mod bmp` on it;
`image-0.25.10/Cargo.toml:70` declares it), and this workspace turns `image`'s
defaults off and enables only `gif, jpeg, png, webp` (`Cargo.toml:107`,
inherited at `crates/bt-app/Cargo.toml:64`). **As the manifest stands today the
decoder this design names cannot be imported at all.** So 2a adds `bmp` to that
feature list and checks the dependency, lockfile and `THIRD-PARTY-NOTICES.md`
impact **for `bmp` specifically** — TIFF's impact says nothing about it, since
`bmp` is a feature of a crate already in the tree rather than a new package. The
whole DIB lane is contingent on that feature: without it, the `CF_DIB` and
`CF_DIBV5` encodings are not offered and a DIB-only clipboard is `Absent`.

**2b — storage, delivery and the switch.** The directory trust contract on both
platforms — including ruling ㉓'s Windows owner, DACL, inheritance and
ACL-capability checks and the read-back of the created file — the name, the
collision ladder, owner-only files, the age-and-quota sweep with its 24-hour
retention floor, the **cross-process quota lock** of ruling ㉕, the **typed job
destinations and generations** of ruling ㉖ including the per-target input
generation, the pre-write encoder preflight of §4.1, the one-in-flight rule, the
`Settings ▸ General` row in both languages, `PRIVACY.md` and the README clause,
PROBE 9.

**Gates:** the storage rows of §6.2, Windows ACL rows included — the
**protected-directory success** row and the **permissive-parent inheritance** row
among them; a written picture compared pixel-for-pixel against its source for
each supported layout; a refused layout refuses; **§6.2's four picture-job
preflight rows** — the redirected-`%`-temp directory refused for a `cmd`
recipient and accepted for a `pwsh` one, and the valid directory refused for an
unmeasured `nu` recipient and accepted for a `bash` one, each refusal asserting
that **no file was created**; the input-generation fixtures of §6.1 **including
the navigation, history-recall, editing and control-key rows**, not only Enter
and text paste; **the wheel-to-`Up`/`Down` row of §6.1 — an alternate-scroll
notch over the job's own target cancelling it and deleting its file, in both
cursor modes, with the local-wheel and other-pane controls beside it — and the
forwarded-mouse-report row that advances with it**; a contended lock refusing
rather than overshooting. **Does not
ship:** TIFF (a named debt with its notices work), promises, drops.

### T-PASTE-3 — the drop door

**Size: L–XL**, with the Windows adapter, the macOS adapter and the batch routing
separately reviewable.

**It opens with PROBE 5**, and the probe is bounded and does **not** presuppose
swizzling: an application-owned destination `NSView` first, a narrow upstream
extension second, class-wide replacement last and only with a written per-window
lifetime contract. Then the Windows adapter (own OLE initialisation, a target on
every HWND, teardown against winit's unconditional revoke, effects, coordinates);
the macOS adapter (the full destination contract including `draggingUpdated:`,
per-window teardown, AppKit-to-physical conversion); the landing from the live
point; the terminal-centre cell in `row_verb` and the OS drop routed through it;
batch admission; the ghost suppressed; the drop admitted-type matrix sharing
T-PASTE-2's lane for raw pictures; PROBE 6.

**Gates:** the drop matrix of §6.2 including 65 files, mixed kinds, the
file-versus-folder cells on Preview and Files centres, a second window and a
fullscreen transition — and, for any raw-picture landing it admits, the **typed
destination** its completion needs (ruling ㉖): a delayed Preview retarget, a
delayed **seat-edge** split with its captured side, a delayed **root-rim** split
on its captured edge, and the invalidation fixtures for all three — including the
changed-root cancellation that must not be repaired into a seat. **A raw picture
landing on a terminal centre is a `TerminalInsertion`**, so this ticket also
inherits T-PASTE-2b's input-generation gates **including the wheel-to-`Up`/`Down`
row and its local-wheel and other-pane controls** (§4.2, fifth review finding 1).
**Does not
ship:** the *reading* of promises (they are registered only to
be refused), strip verbs for OS drops.

**If PROBE 5 finds no acceptable shape, T-PASTE-3 ships Windows only.** That is
the ticket's stated fallback rather than an open end: macOS drops stay exactly as
they are today — nothing happens — and the README and `docs/features.md` say so
in both languages, which is the same "name the lane you do not ship" rule every
other ticket here follows. A probe whose failure has no product answer is not a
bounded probe.

**If 0.4.1 has to shrink**, T-PASTE-1 alone is a release: the K144 repair plus
file-clipboard pasting on the shells whose rows §6.2 measured. **Unmeasured
*agent* recipients inherit Codex's spelling with the inheritance recorded**, the
same as in a full release — the earlier sentence here said they refuse, which
contradicted the ticket's own gate two paragraphs above, and it is struck. What
does refuse in a shrunken release is what refuses in any release: a `nu` row
without a measured baseline, a `%` path in a `cmd.exe` pane, and everything else
§7.2 lists.

---

## 9. Probes and open ends

### 9.1 What still needs a machine

| | What | What it decides | If it is not run |
| --- | --- | --- | --- |
| **PROBE 1** | source × gesture × advertised formats × chosen rung, versions recorded, **including WSLg, `clip.exe` and RDP** | nothing — it is a **fixture matrix to record**, not a blocker on a ruling; ruling ② is implemented the same way whatever it says | T-PASTE-1 does not ship: the matrix has to exist, but no ruling waits on its content |
| **PROBE 2** | PowerShell 5.1 and 7: which code points close a single-quoted string, and whether doubling each yields one character | how wide the `PowerShell` refusal is | refuse the whole documented quote class (§2.3) |
| **PROBE 3** | Claude Code and Copilot CLI: spaces, apostrophes on both platforms, a Windows drive root and a UNC root, two paths, `file:` URI (Codex is **measured** by source, not probed) | whether the six inheriting rows need arms of their own | record the inheritance as a known risk — **at every release size**, T-PASTE-1-only included |
| **PROBE 4** | Git Bash, MSYS and native consumers, `MSYS2_ARG_CONV_EXCL` unset and set | ruling ⑭ | the row sets `"paste_paths_as": "windows-slash"` (§2.5) and gets `'D:/Demo/a.txt'`; nothing global changes |
| **PROBE 5** | macOS drop destination: own view / upstream / replacement | T-PASTE-3's macOS half | **T-PASTE-3 ships Windows only** and says so in both languages |
| **PROBE 6** | what Safari, Chrome and Firefox offer for a dragged image, by version | ruling ㉑'s browser row | the registered types decide; a promise-only drag is refused visibly |
| **PROBE 7** | real `CF_DIBV5` payloads: headers, masks, actual alpha | ruling ㉔'s supported layouts | refuse the layout |
| **PROBE 8** | the **distribution** of delayed-render latency for common Windows sources | whether the transaction must move off the event loop (§9.2's debt) | the transaction stays on the loop, with no Folio deadline, with the `ClipboardRead` station |
| **PROBE 9** | **whether any TCC prompt appears on the paths this lane writes** | §4.1 | **the lane ships enabled on macOS on a recorded assumption** — the package is unsandboxed and `$TMPDIR` is this account's own — and an actual denial at write time is a refusal with a toast, never a fallback copy elsewhere. An unrun experiment cannot answer a per-request access decision, so the unrun state is "assume, and handle the error", not "disable" |
| **PROBE 10** | nushell, on a version this document names: (a) maximum raw-string fence width, (b) whether a raw string is accepted in a builtin's and in an external command's argument position | whether the `Nushell` arm exists at all, and how wide it goes | **the arm does not ship**: a `nu` row refuses every path and says the lane is unmeasured. Half (a) alone does not enable it |
| **PROBE 11** | `cygwin1.dll` / `msys-2.0.dll` beside the resolved **eligible** program and beside `..\usr\bin\`, on a real Cygwin, a real MSYS2 and the Folio-shipped Git Bash wrapper | ruling ⑫a's classifier | **every outcome keeps today's answer, the positive match included, and `"paste_paths_as": "cygwin"` is refused with the unmeasured-spelling message** — an unrun probe enables no new match by either route — so T-PASTE-CYG's arm ships dark |

**PROBE 9 was narrowed, and its old half is gone from the body as well.** Its
first half — can a child of a Folio row read a file under `$TMPDIR` — is not an
open question: the Mac package is unsandboxed
(`packaging/macos/entitlements.plist:20`), children inherit `TMPDIR`, and this
program **already depends on that answer** for its per-run socket
(`PRIVACY.md`, "Elsewhere"). Presenting it as unknown would have hidden a
dependency the design already has. §7.3 used to ask it anyway; that sentence is
struck there too, so the probe's scope is stated in one place. What is open is
the TCC half, and its unrun state is the table's: ship, assume, and treat a real
denial as a refusal.

**An unrun probe is not evidence either way.** Nothing in this document may be
implemented as though a probe had answered — but every probe now has a shipped
behaviour if it is not answered, which is the difference between a bounded probe
and an open end.

### 9.2 Named debts

TIFF on macOS, with its feature, lockfile and notices work. **Reading** file
promises on both platforms. An OS drop on the tab strip. Real WSL mount facts in
place of the default-mount assumption, and real Cygwin mount facts — a cygdrive
prefix read from `/proc/cygdrive` rather than assumed — in place of ruling ⑫a's.
A `nu` or `fish` row reached through `wsl.exe -e` without an override. **A
grammar for a language REPL** — Python's and JavaScript's string literals — which
is what ruling ⑩'s best-effort default stops short of, and which `"paste_as"`
would carry. **The clipboard transaction on a worker thread of its own**, in
either of the two shapes ruling ④ names: a message-pumping owner window for that
thread, or a read-only path in `bt-platform`'s helper that opens with a null
`HWND` — opened if PROBE 8's distribution says the loop cannot carry it.

**The owner's §2.5 ruling is no longer a debt**: it was taken on 2026-09-15, the
standing derivation stands, and the only thing that follows from it in the
detector is T-PASTE-CYG, which is a ticket rather than an open end.

### 9.3 Sources

Every claim about this codebase is cited in place. The outside ones:

* microsoft/terminal **#16627** — pasting a file copied in Explorer pasted its
  path in 1.18, stopped in 1.19, PR #16634.
* microsoft/terminal **#15646** and PR **#16214** — `$hello.txt` dropped into a
  WSL tab expanded as a variable; the fix is single quotes.
* microsoft/terminal **#18006** — a path containing `'` dropped into a WSL tab
  was single-quoted without escaping.
* microsoft/terminal **#8109** — the argument for always quoting a dropped path.
* Windows Terminal's `_translatePathInPlace` and its **`pathTranslationStyle`**
  profile setting (`none` / `wsl` / `cygwin` / `msys2` / `mingw`).
* *about_Quoting_Rules* and *about_Parsing* (PowerShell 7.x) — the Unicode
  quotation characters, and native-argument passing.
* *Parsing C command-line arguments* (MSVC) — the `2N`/`2N+1` backslash rules,
  and their scope: **that parser's consumers, not `cmd` itself**.
* The fish language reference on quoting; and the Nushell book,
  *Working with strings* — "Raw strings behave the same as a single quoted
  string, except that raw strings may also contain single quotes… Additional `#`
  symbols can be added to the start and end of the raw string to enclose one less
  than the same number of `#` symbols next to a `'` symbol in the string" — which
  states no maximum fence width and does not cover argument position, which is
  PROBE 10.
* MSYS2's *Filesystem paths*, for argument conversion and `MSYS2_ARG_CONV_EXCL`.
* Microsoft's *WSL configuration*, for `automount` and its root.
* Codex `normalize_pasted_path` and its paste consumer, at revision
  `a8964cb1bad67bc26a826fb07d1bef99c6a3f008`
  (`codex-rs/tui/src/clipboard_paste.rs`, `…/bottom_pane/chat_composer.rs`).
* Claude Code's common-workflows documentation (prose paths and `@`), and GitHub
  Copilot CLI's attachment documentation (`@`).
* Apple's *Accepting drags* destination contract, the file-promise sample, and
  `NSTemporaryDirectory`.
* `RegisterDragDrop`, `OpenClipboard` — which accepts a null `HWND` — and
  *Clipboard Operations* (delayed rendering), which requires the owner to fulfil
  `WM_RENDERFORMAT` **without opening the clipboard** because the requester holds
  it open; and `GetClipboardSequenceNumber`, whose own contract says that "if
  clipboard rendering is delayed, the sequence number is not incremented until
  the changes are rendered" — the sentence that withdrew ruling ④'s equality
  test. The documented system behaviour for an owner that never renders — a wait
  of about thirty seconds and then `NULL` to the requester — is the backstop
  ruling ④ names as not being a responsive bound.
* `CreateDirectoryW`, for the two facts ruling ㉓ turns on: the security
  descriptor it is handed applies **only to a directory it creates** (an existing
  one returns `ERROR_ALREADY_EXISTS` untouched), and it has effect only on a
  filesystem that keeps persistent ACLs.
* Microsoft's *Security Descriptor Control* and *ACE Inheritance Rules* — the
  pair that reversed ruling ㉓'s protection flag. `SE_DACL_PROTECTED` **prevents
  the descriptor's DACL being modified by inheritable ACEs**, which is the
  boundary this lane wants against its temp parent; what propagates a vetted ACE
  *down* to new children is `OBJECT_INHERIT_ACE` / `CONTAINER_INHERIT_ACE`. The
  third revision had the first of these doing the second's job and forbade it.
  And `SECURITY.md:39`–`:45`, cited for its **fail-closed** rule alone — its
  principal is the **logon** SID, deliberately, which is not the **user** SID
  these files need.
* Rust's `std::env::temp_dir` platform behaviour — Darwin's system-provided
  directory and Windows' environment-based selection, with `TMP` ahead of `TEMP`
  — which is why §4.1 calls the `/tmp`-on-empty-`TMPDIR` rule **Folio's own**
  (`instance.rs:331`–`:337`) rather than `std`'s, and calls the Windows answer
  configured-then-vetted rather than per-account.
* The Cygwin user's guide on `/cygdrive` — the prefix is **configurable**, with a
  stable `/proc/cygdrive` route to its current value — which is why ruling ⑫a
  calls `/cygdrive` a default-mount assumption. And Git for Windows' own
  packaging, which installs a `compat-bash.exe` as `<Git>\bin\bash.exe` while the
  runtime sits under `usr\bin\`, which is why ruling ⑫a's classifier does not
  look beside Folio's shipped candidate.
* The PNG specification's animated-PNG chapter, and `image`'s own decoder
  distinguishing a full animation from the default image
  (`image-0.25.10/src/codecs/png.rs:140`–`:161`) — the pair that replaced §4.2's
  first-frame rule. And `image`'s `bmp` feature gate
  (`image-0.25.10/src/lib.rs:245`–`:246`, `Cargo.toml:70`), which
  `BmpDecoder::new_without_file_header` sits behind and which this workspace does
  not yet enable (`Cargo.toml:107`).
* Python's lexical-analysis rules for escape sequences in ordinary string
  literals — the reason ruling ⑩'s Windows default is labelled best-effort rather
  than proven for a REPL.
* kitty's **OSC 5522**; ghostty-org/ghostty discussion **#10517**, which is about
  image paste **over SSH**; and an unverified WezTerm community recipe writing
  into `/tmp/wezterm-clipboard-images/`.
* The Nushell book's raw-string section, and `docs/plans/shell-matrix-2026-09-07.md:365`
  — the owner's 2026-09-07 ruling on `printed_path_namespace`, which the owner
  confirmed on 2026-09-15 and which §2.5 keeps in both directions.
* winit **0.30.13**, read in `~/.cargo/registry`:
  `platform_impl/windows/drop_handler.rs:83`, `:109`, `:137`;
  `platform_impl/windows/window.rs:1167` (the `OleInitialize` + `RegisterDragDrop`
  gate) and `:1432` (`COM_INITIALIZED`, winit's own `CoInitializeEx`);
  `platform_impl/windows/event_loop.rs:1262` (the unconditional `RevokeDragDrop`);
  `platform_impl/macos/window_delegate.rs:367` and `:666`;
  `platform/windows.rs:493` ("winit may still attempt to initialize COM API
  regardless of this option") and `:497`.
* `image` **0.25.10**: `src/codecs/bmp/decoder.rs:534`
  (`new_without_file_header`, "for decoding the `CF_DIB` format directly from the
  Windows clipboard") and `:752`; `Cargo.toml:120` for the `tiff` feature.

---

## 10. Review ledgers

### 10.1 First review — `paste-paths-review-2026-09-15.md`, 24 findings

Every one is answered below: **accepted** means the design changed, and the
section says where.

**These rows are a historical record of what the *first* revision did, not a
statement of current policy** (fourth review finding 12). Three later reviews
moved several of them, and a reader who took an accepted row for a live
instruction would implement a withdrawn rule. Where that is the case the obsolete
clause is **struck in place** and the current ruling is named beside it; the rest
of the row stands. §10.2, §10.3, §10.4 and §10.5 are the later decisions, and
where they disagree with anything below, they win.

| # | Verdict | What changed |
| --- | --- | --- |
| **1** | accepted | §2.3, §2.4. The universal safe set is **gone**: there is no bare form, every grammar always quotes. POSIX backslash is no longer assumed inert; PowerShell's Unicode quote characters are doubled and **PROBE 2** must prove the doubling before the arm ships, with refusal as the fallback. Non-ASCII whitespace is no longer assumed lexically inert. §6.1 asserts argument identity by running a lexer over the output, and the table carries Unicode delimiter attacks beside CJK and emoji. |
| **2** | accepted | §2.4 is a new section: a representability gate ahead of quoting. `to_str()` failure, any C0/C1 control, DEL or line terminator → **visible refusal**; `to_string_lossy` is banned including in the repaired K144; the false sentence "a quoted path has no control character and no newline" is struck (§5.1); the invalid-UTF-8 acceptance promise is amended from "pastes correctly" to "refuses visibly and names the file" (§2.4, §6.2). Ordinary text sanitisation is untouched. |
| **3** | accepted | §2.3's `cmd` row rewritten. Interpreter and CRT consumer separated; `2N` trailing backslashes, not one extra; the `"D:\"` description corrected (a literal quote after the consumed backslash, not `D:`); builtins named as not following CRT rules, with §6.2 asserting the opened file for both; `%` **refused**; `!` refused when the row's own args turn delayed expansion on; `^` explicitly not escaped; "universal cmd safety" dropped. |
| **4** | accepted | §2.3 gains a `Fish` arm (`\`→`\\`, `'`→`\'`) and a `Nushell` arm (raw string with a growing fence). The "`'\''` happens to be correct in fish" reasoning is withdrawn as reasoning from one sequence to an encoder; the "nu cannot spell an apostrophe path" claim is withdrawn as false; the "every shell an account can be set to" claim is narrowed. §6.1 tests combined backslash-and-apostrophe cases. |
| **5** | accepted | §2.2 ruling ⑦ states a **spawn-time default**, not a recipient guarantee, and says in as many words that the foreground program is not inferred from the screen. Derivation is from the **resolved** launch and keeps `PowerShellSeven` (ruling ⑧). Ruling ⑨ adds a `profiles.json` `"paste_as"` override for wrappers and unknowns. `shell_integration.rs:248`'s WSL login-shell `exec` is cited as the reason a WSL row's reader is unknowable. §6.1 tests `wsl.exe -e nu` and an override row, not only builtin ids. |
| **6** | accepted, then **superseded by second-review finding 3, and closed by the owner on 2026-09-15** | The first revision derived the namespace from the program and re-pointed the detector. That reversed the owner's 2026-09-07 ruling without citing it, so the second revision withdrew it into a proposal. **The owner then ruled: the standing `(paths, integration)` derivation is kept for both directions and the program-keyed alternative is rejected** (§2.5). The integration-off Git Bash gap this row was answering is a **disclosed residue** with a per-row remedy (`"paste_paths_as"`), not a change; the Cygwin half is adopted separately as T-PASTE-CYG. |
| **7** | accepted | §2.5 ruling ⑬ rewritten: lexical inverse separated from mount facts; the unmounted-drive case is a disclosed **default-mount assumption**, not a translation; `\\wsl$\` read; drive case normalised; the `\\wsl.localhost\…\mnt\d\x` non-identity named; UNC and foreign-distro fallbacks stated as having no inverse; `~` never emitted because a quoted one does not expand. §6.1 splits the round trip into a **supported-identity** suite and a **known-non-identity** suite. `$(wslpath …)` remains forbidden (red line 12). |
| **8** | accepted | §2.5 ruling ⑭: MSYS2's own argument conversion is named, `MSYS2_ARG_CONV_EXCL` is named, quotes do not disable it, §6.2 carries MSYS and native consumers with the variable unset and set (**PROBE 4**), the forward-slash Windows spelling is the documented per-row fallback through the override, and red line 12 forbids changing the reader's environment. |
| **9** | accepted — the earlier ruling is **reversed**, and this row was corrected twice more | §2.3. Codex's `normalize_pasted_path` at revision `a8964cb1…` was read: it strips one surrounding quote pair, tries a `file:` URL, tries a Windows recogniser, and only then requires exactly one shlex token — so a **bare spaced POSIX path attaches nothing** and the quoted one works. **`Agent` is the POSIX single-quoted form for a POSIX path and the double-quoted form for a Windows path** (second review, finding 4); this row's earlier sentence "the POSIX single-quoted form on both platforms" was left standing after that change and is now **struck**, which is what third-review finding 7 asked for. The "quotes make agents seek apostrophe-prefixed names" claim is withdrawn. Prose references and automatic attachments are separated; Claude Code's prose/`@` and Copilot CLI's `@` are recorded as documented behaviour that says **nothing** about escaped apostrophes; **multi-file automatic attachment is not guaranteed**, stated without claiming every branch returns `None`, since a Windows multi-path line can reach the recogniser before shlex. Shlex identity is asserted for POSIX outputs only. **PROBE 3** measures spaces, apostrophes, roots and multiples per agent, with versions. |
| **10** | accepted | §1.2 ruling ② now states the order as a **preference with a named loss**, not a deduction about intent; the "screenshot tools offer no text at all" claim is struck. The losing case gets a **Paste picture** verb (menu row plus bindable command) rather than a second setting. §6.2 carries the versioned source/gesture/format/result matrix (**PROBE 1**). No ShareX default is claimed — the product is not cited at all, only the mixed-workflow *case*. The files-versus-text order is stated once, in ruling ①, and the contradictory sentence in the setting's rationale is rewritten (§4.6). |
| **11** | accepted | §1.2 ruling ③ defines **Absent / Present / Unreadable**, with empty-`HDROP` as absent and empty text as present, and `Unreadable` stopping the ladder; encoding-level fallback is allowed only *within* the picture rung. ~~Ruling ④ adds a coherent snapshot with `GetClipboardSequenceNumber` / `changeCount` before and after.~~ **Superseded by the third revision (§10.3 row 1):** the Windows before-and-after equality test is withdrawn — the number is read once, before the open, as diagnostic context only, and coherence there is the single open interval; macOS keeps `changeCount` on its own justification. §1.2 ruling ④ is the current rule. §1.3 extracts a shared, log-free file-URL decoder beneath `paths_on` rather than reusing the service helper. §1.1 corrects the `Result`/`Option` descriptions. WSL/RDP bridged formats are covered by PROBE 1's "record what is advertised"; no file identity is inferred from arbitrary text. |
| **12** | accepted | §5.2 is a new section: the promise is limited to a **fresh argument boundary**, and inside-token, mid-quote and wrapped-column-zero behaviour is documented as outside it. zsh `bracketed-paste-magic`, PSReadLine and `$PSNativeCommandArgumentPassing` are named as later stages; §6.2 asserts the argument received and the file opened, on 5.1 and 7, default and custom zsh, bracketing on and off. Bracketed inheritance is kept and is explicitly not disabled to bypass hooks (§5.1). |
| **13** | accepted | §4.2 ruling ㉖. A job carries window, tab, leaf, **session incarnation** and a globally unique **request sequence** (`CONVENTIONS.md:154`), revalidates target, modal gate and setting at completion, cancels on ownership or setting change and deletes its file through the owned path. Ordering is removed as a problem by ruling ㉕'s one-in-flight rule. The same applies to delayed drops. |
| **14** | accepted | §3.1 rebuilt. The first draft's "winit's content-view class" is **corrected**: `NSDraggingDestination` is implemented on `WindowDelegate` (`window_delegate.rs:367`) and registration is on the window (`:666`); the methods are entered/prepare/perform/conclude/exited with **no `draggingUpdated:`**, which is why winit's route cannot feed a following highlight. **PROBE 5** now prefers an application-owned destination `NSView` first and a narrow upstream extension second, with class-wide replacement last and only with a written per-window lifetime contract; it must verify hit testing, responder/IME, the `CAMetalLayer` and the web panes, and specify the full method set, native ABI return types, teardown and the AppKit-to-physical conversion. |
| **15** | accepted | §3.1's Windows half. `window.rs:1167` gates `OleInitialize` **and** registration together, so Folio owns the STA initialisation and its balance; a target is registered on **every** HWND, with `DRAGDROP_E_ALREADYREGISTERED` named as the failure of a missed constructor; teardown is written against `event_loop.rs:1262`'s unconditional `RevokeDragDrop`; payloads are copied before release; effects are `COPY`/`NONE` only, never `MOVE`; coordinates, DPI, source masks, multi-window teardown, cancellation and re-entrancy are specified. |
| **16** | accepted | §3.2. The strip row is **removed from the table** and described accurately: `row_verb`'s strip arm is unreachable (`main.rs:28995`) and the strip's verbs live on `row_strip_landing` (`:29555`) and its two commits, which are untouched; only an *OS* drop on the strip is refused, at the strip's own routing. Ruling ⑱ defines batch admission — many on a terminal centre, one-item verbs refuse a multi-item drop **while hovering**, mixed kinds, a 64-item cap, partial failure. Ruling ⑲ makes drop insertion target-specific and focus-free while **preserving K144's focus move** (`main.rs:76921`), which the first draft misdescribed. |
| **17** | accepted; the promise half **superseded by second-review finding 2** | §3.4 ruling ㉑ gives drops their own admitted-type matrix: raw picture types are registered explicitly, picture drops on preview and edge are defined, and decoders and insertion are shared. The unconditional Safari claim is **withdrawn** and replaced by **PROBE 6**. The first revision left promises *unregistered* while still promising a visible refusal; they are now registered **only so that the refusal can be drawn**. |
| **18** | accepted | §4.2. The synthesised `BITMAPFILEHEADER` is **withdrawn** in favour of `image`'s audited `BmpDecoder::new_without_file_header` (`decoder.rs:534`, written for `CF_DIB`). **TIFF is removed from 0.4.1** and becomes a named debt with its feature, lockfile and notices work, since `tiff` is a separate feature (`image-0.25.10/Cargo.toml:120`) the workspace does not enable. Ruling ㉔ makes alpha a property of compression and mask, not of the format id, covers premultiplication and undefined legacy alpha, refuses unsupported layouts, and adds known-pixel fixtures and **PROBE 7**. |
| **19** | accepted | §4.2 ruling ㉕. Native length checked **before** copying (`GlobalSize`), with the macOS residue stated; dimensions checked before allocation; the cap expressed in decode bytes rather than an RGBA8 multiplication; a 512 MiB aggregate directory quota with oldest-first eviction; **one job in flight**, a second refused; write/flush/close failures delete the partial file; disk pressure refuses; full-decode validation with original-byte write replaces "header check"; ~~first-frame policy stated~~; ~~**PROBE 8** gives acquisition a measured latency contract~~. **Two clauses superseded:** the first-frame policy is **struck** by the third revision (§10.3 row 18) — an animated PNG is written whole and validated frame by frame (§4.2); and PROBE 8 gives acquisition **no** latency contract (§10.3 row 2) — it measures a distribution and decides where the transaction runs, while Folio sets no deadline at all (§1.2 ruling ④, §4.2 ruling ㉕). |
| **20** | accepted | §4.1 ruling ㉓. The socket precedent is replaced by the **directory** precedent `instance.rs:360`, including **repair of an existing mode**; a same-owner world-readable directory no longer passes; both Folio-made levels are vetted, so an intermediate link cannot redirect a non-link leaf; vetting runs **before every operation**, not once per run; files are owner-only; operations are anchored to a verified handle with no-follow on Unix, with the Windows residue stated; Windows gets reparse-point refusal and an owner-only DACL as the uid counterpart **— rewritten twice since: by the third revision into owner/DACL/inheritance checks at both levels (§10.3 row 12), and by the fourth into one principal (the token's user SID), a *required* `SE_DACL_PROTECTED` and explicit `OI`/`CI` propagation (§10.4 row 4, §4.1)**; ~~temp discovery is `std::env::temp_dir()`~~ **— superseded (§10.3 row 11): Windows is `std::env::temp_dir()` *configured, then vetted*, while Unix is Folio's own `instance::runtime_directory()` policy, `folio-<uid>` under `$TMPDIR` or `/tmp` (§4.1)** — and macOS's `/var` alias is explicitly not rejected; failures are closed and sanitised. |
| **21** | accepted | §4.3 ruling ㉗. The "seven days exceeds any session" and "preserves history" claims are **struck**. Retention is best-effort with a real maximum from the quota; the sweep runs at startup **and hourly**; the system may remove files sooner; the exact owned-name grammar excludes `clip-family.png`; age is mtime; active writes are excluded; failures are reported once. A path in history is explicitly not a promise the file exists, and how to keep a picture permanently is stated. The text says the sweep is not clipboard watching. |
| **22** | accepted | §4.6 ruling ㉙ makes the switch cover **clipboard and drop** pictures and cancel pending jobs; inserting an existing file's path is explicitly outside it. Red line 3 carries the **named storage exception**; red line 1 is narrowed to feature-initiated requests, with UNC and network-volume I/O described as the reader's own gesture; §7.3 separates `BT_PTY_DUMP` (opt-in, already disclosed at `PRIVACY.md:186`) from this feature's storage. §4.5 discloses drops, original PNG metadata, cleanup limits, redirected `%TEMP%`, and that deleting the directory does not undo history, agent records or recipient copies. No sandbox blocker is invented — `entitlements.plist:20` says the package is unsandboxed — and ~~child access and~~ TCC become **PROBE 9** with refusal, not a fallback copy, if it fails. **The child-access half is superseded** by the second and third revisions (§10.2 row 18, §10.3 row 15): it is not an open question — the package is unsandboxed, children inherit `TMPDIR`, and the per-run socket already depends on that answer — so PROBE 9 is the TCC half alone (§7.3, §9.1). |
| **23** | accepted | §8. T-PASTE-1 is **L**; T-PASTE-2 is **L–XL** and splits into 2a acquisition/decoding and 2b storage/delivery/settings; T-PASTE-3 is **L–XL** with three separately reviewable parts and opens with a bounded probe that does not preselect swizzling. §6.1 no longer claims to be the whole specification, and §6.2 adds native argv, recipient versions, async ownership, storage attacks and decoder fixtures. The picture rows move to T-PASTE-2 — **but the i18n row does not move, and this sentence was corrected in the second pass**: every ticket that adds a string keeps an i18n row, and T-PASTE-1 ships at least six refusal toasts. Each ticket states the lane it does not ship and its gates. K144 stays in T-PASTE-1 and its Windows defect is written out (§2.1). |
| **24** | accepted | §1.4 and §9.3. The adoption inference from WT's report timing is removed; Ghostty #10517 is narrowed to **SSH image paste**; the WezTerm recipe is marked **unverified**; WT's history is cited as examples rather than as proof of the grammar table; the Codex citation carries its revision. **On the `usershell` anchor**: the sentence carrying the wrong `profiles.rs:1541` was rewritten away by ruling ⑦, so there is no anchor in the body to correct — `USER_SHELL_ID` is at `profiles.rs:1512` and this row records that rather than pointing at a line the body no longer has. |

**Nothing is declined.** Two findings were answered by reversing an earlier
ruling rather than by adjusting it — **9** (agents get a quoted single token) and
**1** (there is no bare form at all) — and two by withdrawing a claim the design
could not support: the Safari offer (**17**) and the universal round trip
(**7**).

### 10.2 Second review — `paste-paths-review-2-2026-09-15.md`, 23 findings

An Opus reviewer standing in for Codex, whose limit resets 2026-09-19. Its Part 1
re-checked all 24 rows above against the body and found none ledger-only; its
Part 2 raised 23 new findings. Every one is answered below. **One is declined in
part, on evidence**, and it is marked as such.

**Each cell here is the *second* revision's answer, and several were themselves
superseded by the third or the fourth** (fourth review finding 12). Where a cell
ends in a **Closed by the third revision** sentence, that sentence is the current
decision and the text before it is the historical record of how the row got
there; where an obsolete instruction was still written in the present tense, it is
now **struck in place** with the current ruling named beside it.

| # | Verdict | What changed |
| --- | --- | --- |
| **1** | accepted | §1.2 ruling ④ rewritten. The transaction is **two steps** — survey the advertised type list (which renders nothing), then fetch **one** rung — so the exposure is no longer multiplied by five. The thread is named (winit's event loop, `lib.rs:5927`, `main.rs:96290`), ~~a delayed render is stated to be **unbounded** with no timeout and no cancellation~~ — **superseded (§10.3 row 2): Folio imposes no responsive deadline, while the *system* has a documented ~30-second give-up-and-return-`NULL` backstop; "unbounded" and "bounded" are both withdrawn (§1.2 ruling ④)** — and it gets a new `hang_watch` station **`ClipboardRead`** beside `PtyResize` / `WebPage` / `WebRetire`. **PROBE 8 is redefined** from "measure a bound" to "measure the distribution and decide whether the transaction must move off the loop", and the worker-thread variant is written down in §9.2 with its real cost — a second message-pumping clipboard owner window, ~~because `OpenClipboard` wants a window of the *calling* thread (`lib.rs:5838`)~~ — **superseded (§10.3 row 2): `OpenClipboard` accepts a null `HWND`; the window is *this codebase's helper* (`lib.rs:5903`–`:5919`), not a Win32 law about reading, so the debt has two shapes (§9.2)**. **Closed by the third revision.** The measured bound this row left standing in §4.2's acquisition bullet is struck there as well as in ruling ④; Windows coherence is no longer sequence equality across `GetClipboardData`, macOS keeps `changeCount` on its own justification, and the survey is candidate selection — so "exactly one rung" and the empty-`HDROP` rule stopped contradicting each other (third review, findings 1–3). |
| **2** | accepted | §1.2 gains a fifth variant, **`ClipboardPayload::Refused(UnsupportedKind)`**, set when a kind is advertised and deliberately not read; only it raises a toast, `Nothing` stays silent, so `Ctrl+V` on an empty clipboard says nothing. On the drop side the decision is taken and stated: **promise types are registered after all**, purely so the destination is offered the drag and can trace the refusal box, with `performDragOperation:` never entered and `receivePromisedFiles` never called. §3.4's matrix carries the row. **Closed by the third revision.** §5's crate map carries all five payload values and names the two report channels apart, and T-PASTE-1's unimplemented picture rung is silent `Nothing` rather than the "nothing to paste" message §1.2 forbids for it (third review, finding 4). |
| **3** | accepted | §2.5's ruling ⑫ is **withdrawn into a proposal**. The standing 2026-09-07 derivation is kept and cited (`docs/plans/shell-matrix-2026-09-07.md:365`, `profiles.rs:3193`), T-PASTE-1 calls `printed_path_namespace` as it stands, the detector is **not** re-pointed, and the change is marked **OWNER RULING NEEDED** with both sides written out. The document's own scope paragraph now names that marker. **Cygwin** gets ruling ⑫a — a `Cygwin { home }` arm spelling `/cygdrive/<letter>`, detected by the runtime DLL beside the program, **PROBE 11** — and the section states that the Cygwin hole exists under the **standing** rule too, in the detector, so it is not something the proposal introduced. MinGW is explicitly out, with the reason: it is separator style, not a namespace. **Closed by the third revision.** The owner ruled on 2026-09-15 — the standing derivation stands, the program-keyed alternative is rejected and recorded as rejected — and §2.5, §5, §8, §6.1 and all three ledgers now say that one thing. The Cygwin half became **T-PASTE-CYG** with a classifier table complete over its six outcomes, a dark unrun state, and no undefined `home` field (third review, findings 5–6). |
| **4** | **accepted in part, declined in part — on the parser's own text** | §2.3's `Agent` arm rewritten with the four branches quoted and the revision pinned. **Declined:** the POSIX half is not wrong. `normalize_windows_path` returns `None` unless the string starts with a drive letter or `\\`, so for `'/Users/ann/John'\''s Papers/a.png'` the URL and Windows branches both decline, control reaches `shlex::Shlex::new(pasted)` over the **original**, and one token comes back carrying the true name. The quote-strip branch does not "win" for a path the recogniser rejects. **Accepted:** the Windows half is wrong and worse than the first revision admitted — `'C:\a'\''b.png'` strips to something that *does* start with a drive letter, so the recogniser returns `C:\a'\''b.png` verbatim. That is not a suspicion to probe; it is what the code does. **The `Agent` literal is now double-quoted for a Windows path and POSIX single-quoted for a POSIX path**, which both branches read correctly and which is additionally *Copy as path*'s own spelling. PROBE 3 keeps the apostrophe row on both platforms as the check on this reading. **Closed by the third revision.** §4.4's flat "single-quoted" and first-ledger row 9 now carry the platform split; shlex identity is scoped to POSIX outputs, because a Windows root `"C:\"` is not a shlex input; and the multi-path claim is narrowed to what the recipient's branch order actually supports (third review, finding 7). |
| **5** | accepted | §2.2 ruling ⑩ and §2.3. The **encoder** and the **interpreter** are separated: `Cmd` is the CRT-quoting encoder, and `%` / `!` refusal is a property of the **`cmd.exe` row specifically**. A `python` or `node` row on Windows gets CRT quoting with no expansion refusals, and §6.2 gains a row asserting that `D:\Data\100%\report.csv` **works** there rather than refusing. **Closed by the third revision.** Ruling ⑩'s unknown-program encoding is labelled a best-effort default with **no literal-identity guarantee**, the "works perfectly in those panes" sentence is struck, and §6.2's Python/Node acceptance row is replaced by a defined CRT command-line consumer (third review, finding 8). |
| **6** | accepted | §1.2. The picture rung is fetched as an **ordered list** — every advertised encoding copied inside the one transaction — so `bt-app` can fall to `CF_DIB` after a failed PNG decode without a second snapshot. The earlier within-rung fallback was unreachable exactly as the finding says. `Unreadable` is narrowed to an **acquisition** verdict and "malformed content" is struck from its definition, since the layer that reports it does not decode. The list's total size sits inside ruling ㉕'s bound, and a decode failure past a truncated list is terminal and says so. |
| **7** | accepted | §4.2 ruling ㉕ and §4.3. The quota gains a **24-hour eviction floor** — never evict a file younger than that, refuse the write instead — so eviction cannot reach a path sitting in an unsubmitted command line. The read-evict-write sequence is **serialised across windows and processes** by an owner-only `clipboard/.lock` taken with `create_new`, ~~with a 10-second staleness rule; a lock it cannot take means the write proceeds without evicting and the quota may overshoot, stated rather than pretended away.~~ **Superseded (§10.3 row 10): the staleness rule, the age steal and the bypass-on-contention are all withdrawn — the lock is a crash-released OS handle, contention waits off the loop for two seconds and then *refuses*, and 512 MiB is a real bound (§4.2 ruling ㉕).** §4.3's disclaimer is widened from history to "a path on your screen", naming the unsubmitted line. **Closed by the third revision.** Ruling ㉕ now carries **one** quota contract: a crash-released OS lock held over the whole accounting-and-write sequence, contention waited off the loop with a two-second limit and then refused, no age stealing and no bypass. The 24-hour floor became a **retention floor on every eviction path**, and §4.3's implied protection of every live prompt is withdrawn (third review, finding 10). |
| **8** | accepted | Red line 2 is rewritten as **one rule with one positively stated permission** — the *type list* may be read when the terminal's right-click menu is raised; the *content* only on a paste, a Paste picture or a drop. A failed type-list read **disables** the row rather than hiding it. **Paste picture** is moved to the terminal context menu only and explicitly kept out of the macOS menu bar, because `validateMenuItem:` fires on menu opening and key-equivalent dispatch at moments the application does not schedule (§5.3). §4.6 states that with the switch off the row is **disabled**, not enabled-then-apologising. |
| **9** | accepted | §4.1 takes `instance.rs:331`'s **naming** as well as its vetting: on macOS the directory is `folio-<uid>/clipboard` under `$TMPDIR` **or `/tmp`** — the `/tmp` fallback is now stated, where the earlier draft asserted `$TMPDIR` and called it per-account. The squat-then-fail-forever path is named as the reason. Windows keeps `%TEMP%\Folio\clipboard`, whose temp is already per-account. **Closed by the third revision.** `folio-<uid>` reaches the vetting, the sweep and `PRIVACY.md`; the `/tmp`-on-empty-`TMPDIR` rule is named as **Folio's own policy** (`instance.rs:331`–`:337`) rather than as `std`'s; Windows temp is described as configured-then-vetted, with `TMP` ahead of `TEMP`; and the uid suffix is explicitly **not** offered as proof against squatting (third review, finding 11). |
| **10** | accepted | §8. T-PASTE-1's scope **keeps an i18n row** (it ships six refusal toasts), and first-ledger row 23 is corrected in place rather than left to disagree. The PROBE 3 gate is restated as what it is: Codex is the measured recipient, the six others **inherit** its spelling, and the gate is *measure, or record the inheritance as a known risk* — not a refusal the grammar table does not carry. **Closed by the third revision.** §8's shrink paragraph no longer contradicts T-PASTE-1's own gate: unmeasured agent recipients inherit Codex's spelling with the inheritance recorded, **at every release size** (third review, finding 15). |
| **11** | accepted | §2.3's nushell paragraph. **One spelling** — `r` + *n* `#` + `'…'` + *n* `#` — replaces the two the section carried; the growth rule is stated; the Nushell book's raw-string sentence is quoted in §9.3; and the totality claim is **withdrawn** as the same error just admitted for fish. **PROBE 10** covers the maximum fence width and, separately, whether a raw string is accepted in *argument* position, which the book does not document. ~~Unprobed, a `nu` pane refuses anything needing a wider fence than one.~~ **Superseded (§10.3 row 14): until a named nushell version answers *both* halves of PROBE 10, the whole `Nushell` arm does not ship and a `nu` row refuses every path — a width-one arm answers half (a) and says nothing about argument position (§2.3).** **Closed by the third revision.** The `Nushell` arm does not ship at all until a named nushell version answers **both** halves of PROBE 10, the grammar table's row carries a literal and the construction formula instead of an ellipsis, and the fence-growth test is gated on the measured maximum (third review, finding 14). |
| **12** | accepted | §3.1's Windows half names winit's **third** COM owner: `platform/windows.rs:493`'s own warning that "winit may still attempt to initialize COM API regardless of this option", and `window.rs:1432`'s `thread_local! { COM_INITIALIZED }` with its `CoUninitialize` in a thread-local destructor. The ordering rule is written: Folio's `OleInitialize` before any window, and its `OleUninitialize` before winit's destructor or deliberately not at all. §6.2 gains a **fullscreen transition with a target registered**. |
| **13** | accepted | §3.4's matrix now carries §3.2's **four landings plus `Placeholder`** (a real `SeatKind`, `crates/bt-layout/src/tree.rs:26`) and names every cell with §3.2's own verb — `Insert` / `Retarget` / `Split` / `Refused` — instead of paraphrasing. The rim cell says `Split`, not "open the one file". §6.2 gains a `Placeholder` drop row. **Closed by the third revision.** §3.4's file-URL row is split into a **file** row and a **folder** row, so Preview centre no longer carries two verbs for a folder, and the split governs the hover as well as the commit (third review, finding 13). |
| **14** | accepted | PROBE 1's own list gains the three bridged-clipboard rows — a file copied in a WSLg GUI file manager, `clip.exe` text from a WSL shell, and a file and an image copied inside an RDP session — so the ledger's claim and the matrix agree. |
| **15** | accepted | §4.1. The representability gate runs on the **directory**, once, as part of vetting, and refuses the whole lane with one toast before any picture is written. And a job that somehow completes with an unspellable path **deletes its own file through the owned path**, as a cancellation does; §6.1 asserts it. |
| **16** | accepted | §4.3. The sweep runs on **the picture lane's worker**, never on the event loop — it is a listing plus `symlink_metadata` plus unlinks against a `%TEMP%` the design itself says may be a network share. It shares the worker with the encode, so it is never concurrent with this process's own write, and anything of it that touches the loop shares ruling ④'s station. §5's map carries both the worker and the station. |
| **17** | accepted | §8. PROBE 5's all-fail outcome is a **shipped behaviour**: T-PASTE-3 ships Windows only, macOS drops stay as they are today, and the README and `docs/features.md` say so in both languages. §9.1's table gives every probe an "if it is not run" column for the same reason. |
| **18** | accepted | (a) PROBE 2's fallback is now a **concrete set** — `U+0027`, `U+2018`, `U+2019`, `U+201A`, `U+201B` — refused by an unprobed build, with the probe able only to narrow it; a code point found outside the list stops the ticket. (b) **PROBE 1 is retitled** a fixture matrix to record: it blocks the ticket's shipping, not a ruling. (c) **PROBE 9 is narrowed** to the TCC half, and the child-read half is written down as a fact the per-run socket already depends on. **Closed by the third revision.** PROBE 9's child-read half is struck from §7.3 as well as §9.1, and its unrun state is stated operationally — the lane ships on a recorded assumption and an actual denial is a refusal, because an unrun experiment cannot answer a per-request access decision (third review, finding 15). |
| **19** | accepted | The scope paragraph points at **§9.1** and says eleven probes. It named one pending owner ruling; the third revision replaced that sentence, because the ruling was taken — the paragraph now says there is **no** open ruling and summarises the decision. |
| **20** | accepted | §2.4 quotes all five arms of `sanitize_paste` (`input.rs:718`–`:728`) exactly, including that `'\r'` keeps the CR and swallows a following LF, and that **TAB is kept, not dropped** — and then says so as the strongest of the arguments for having a gate at all, since a kept TAB would otherwise survive into the argument. |
| **21** | accepted | §3.1 names all three discard sites — `DragEnter` `:83`, `DragOver` `:109`, `Drop` `:137` — and says the entry callback carries no coordinate either. |
| **22** | accepted | First-ledger row 24 now says the sentence carrying the wrong anchor was **removed** by ruling ⑦, and records `profiles.rs:1512` as the fact rather than as a correction a reader could go and check in the body. |
| **23** | accepted | §4.1 stops naming `GetTempPath2W` and says what the design actually needs — a temp directory the platform chose. **The third revision corrected the second half of that sentence**: "per-account" was a claim about `std`'s discovery that `std` does not make, so §4.1 now says *configured, then vetted*, and the privacy of the directory rests on ruling ㉓ rather than on the name the environment handed back (third review, finding 11). |

**One partial decline, and it is the only one across the first two reviews.**
Finding 4's POSIX half does not hold: `normalize_windows_path` requires a drive
letter or a UNC prefix, so a quoted POSIX path reaches the shlex branch and comes
back correct. The finding's Windows half was right, and more sharply than the
first revision had it, so the arm changed there. Everything else in both reviews
is accepted. The third review read the same parser and **agreed** with that
decline on the same evidence, which is why the POSIX arm still stands.

### 10.3 Third review — `paste-paths-review-3-2026-09-15.md`, 20 findings

Seven blocking, thirteen should-fix, and a ledger of the second review's 23 rows
in which twelve were closed and eleven partially closed. §10.2 now carries what
closed each of those eleven. Every finding below has a decision and a place in
the body; **one is accepted in part**, and it is marked as such with the reason.

**These rows are a historical record of what the *third* revision did, not a
statement of current policy** (fifth review finding 5) — the same convention
§10.1 and §10.2 carry. The fourth revision moved several of them, and a reader
who took an accepted row for a live instruction would implement a withdrawn rule.
Where that is the case the obsolete clause is **struck in place** and the current
ruling is named beside it; the rest of the row stands. Two rows are superseded in
substance and say so: **row 8** is finished by **§10.4 row 3** (the CRT consumer
is reached through `CreateProcessW` with no `cmd` row in front of it) and **row
17** by **§10.4 row 6** (the split anchor is two-valued, and the captured
recipient moved onto the one destination that has a shell). §10.4 and §10.5 are
the later decisions, and where they disagree with anything below, they win.

| # | Decision | What changed, and where |
| --- | --- | --- |
| **1** (blocking) | accepted | §1.2 ruling ④. Coherence is spelled **per platform**: macOS keeps `changeCount` equality on its own justification; Windows **withdraws the sequence-number equality test across `GetClipboardData`**, because the documented contract says a delayed render does not bump the number until it is rendered — so a successful first fetch is itself a cause of a change, and the test could not tell the feared event from the requested one. What carries coherence on Windows is the single open interval plus the rule that a rendering owner must not open the clipboard. The number is read once, before the open, as diagnostic context only. A **delayed-render success fixture** — first paste must succeed — is added to §6.1's Windows fake and §6.2's source matrix, ~~beside a real-replacement row~~ — **that half is superseded (§10.4 row 2): a legitimate competing copier cannot replace the clipboard inside the open interval, so the Windows row asserts the *exclusion* instead; macOS keeps its replacement refusal**. The forbidden repair (a silent reopen) is stated. |
| **2** | accepted | §1.2 ruling ④ and §4.2 ruling ㉕. The measured bound is removed from the body — the acquisition bullet that still asked PROBE 8 for a number included. **"Everywhere" was not true when this row was written**: §10.1 row 19 still gave PROBE 8 "a measured latency contract", which the fourth revision struck there (§10.4 row 12). The flat "there is no timeout" is also struck: Folio imposes no responsive deadline and no cancellation, and the *system*'s documented ~30-second give-up-and-return-`NULL` backstop is named as a failure mode rather than a bound. The worker-thread owner window is redescribed as **a cost of this codebase's helper, not a Win32 law about reading** — `OpenClipboard` accepts a null `HWND`, and `lib.rs:5832`–`:5840` explains the window in terms of `EmptyClipboard`/`SetClipboardData`, while `:5903`–`:5919` is the helper that insists on one. §9.2 carries both shapes of the debt. |
| **3** | accepted | §1.2. The survey is redefined as **candidate selection**: candidates are fetched in rung order until one is `Present` or `Unreadable`, and a second rung is fetched only after acquired content proved the first `Absent`. The empty-`HDROP` and all-non-file-URL cases are named as the two `Absent` verdicts a type list cannot reach. "Exactly one rung" is restated as the ordinary case, and §6.1 tests **empty `HDROP` beside readable text** by name. |
| **4** | accepted | §5's crate map now carries the **five-state** payload, the per-platform coherence rules and the acquisition-error channel, and a paragraph beneath it states that `Refused` is the only *payload* value that toasts while `Unreadable` and the picture lane's failures report separately (§1.2 says the same, scoped). §8's T-PASTE-1 no longer asks for a "nothing to paste" message on an unimplemented picture rung: an absent rung with nothing else makes the payload `Nothing`, which is **silent**. |
| **5** (blocking) | **accepted in part** | **Accepted:** the contradiction is gone. The owner ruled on 2026-09-15, §2.5 records the rejection of the program-keyed alternative as a rejection, and §5 (no `derive_namespace`), §8 (not "a two-direction namespace change", no `Cygwin` arm inside T-PASTE-1), §6.1 and all three ledgers were made consistent with it. **One sentence of that §6.1 change was overbroad and the fourth revision corrected it** (§10.4 row 11): "the suite asserts that nothing reads a program" would have forbidden `derive_grammar(&ProgramSource)`, which the same ticket ships. The assertion is scoped to the **namespace** resolver; grammar keeps its program input. Cygwin is named in both of its configurations — automatic `BashInitFile` → `Msys`, explicit `None` → `Windows` — as the review's assessment asked, and is scoped as a separate change. **Declined in part, with the reason:** the correction also asks for "a specified resolution table with an explicit detector migration" for the alternative, and for the design to "give the owner two concrete scopes". That is a request to keep the choice open; the owner has taken it. Writing a full input-to-namespace table for a derivation this document is not going to build would be documenting a rejected design as though it were still a candidate, which is the opposite of what the rest of this finding asks for. What the body keeps instead is a short paragraph on why the alternative was considered and why it lost. |
| **6** | accepted | §2.5. The sibling DLL is demoted to an **installation hint**; the shipped Git Bash candidate `<Git>\bin\bash.exe` is handled explicitly as the wrapper `profiles.rs:1119`–`:1137` says it is, with Git for Windows' `compat-bash.exe` and its `usr\bin\` runtime cited; the classifier becomes a table complete over six outcomes — shipped candidate, Cygwin DLL only, MSYS DLL only, both, neither, unreadable — with **one** positive row, and **with PROBE 11 unrun every row keeps today's answer**, so no unverified positive match is enabled. `Cygwin { home }` loses its undefined field and becomes `Cygwin`; home-based detection is declined. `/cygdrive` is stated as the documented **default** prefix and an assumption, `/proc/cygdrive` is named and explicitly not read at paste time, and no mount fact is inferred for the detector. §6.2 gains the wrapper, integration-off, both-DLL and moved-prefix rows. |
| **7** | accepted | §2.3, §4.4 and first-ledger row 9. The platform split reaches the picture path and the ledger; **shlex identity is asserted for POSIX outputs only**, because `"C:\"` is a legal Windows literal that shlex would mis-lex; the multi-path claim becomes "multi-file automatic attachment is not guaranteed" rather than "every branch returns `None`", since a Windows combined line can reach the recogniser first; and Claude Code's and Copilot CLI's documentation is recorded as saying nothing about escaped apostrophes — an **unverified inheritance**, measured by PROBE 3. §6.2's agent rows are asserted against the recipient's branch order and gain a drive root and a UNC root. |
| **8** | accepted | §2.2 ruling ⑩ and §6.2. Unknown-program Windows encoding is labelled a **best-effort default with no literal-identity guarantee**: the CRT convention is process creation, not the recipient's interactive language, and a Python REPL reads `\n` and `\t` out of the same string. The "works perfectly in those panes" sentence is struck. The Python/Node acceptance row is **replaced** by a defined CRT command-line consumer — ~~an argv printer launched from a `cmd` row~~ — **superseded (§10.4 row 3): launching it from a `cmd` row put the encoder behind the very interpreter the row exists to exclude, and §2.3 refuses a `%`-bearing path in a `cmd` profile anyway, so the encoder row hands its line *straight to `CreateProcessW`* with no `cmd.exe` between and the `cmd`-profile refusal is asserted separately (§6.2); the fifth revision then pinned the harness to an MSVC CRT `wmain` consumer with the tested literal in `argv[1]` (§10.5 row 4)** — and a REPL grammar is booked as a debt in §9.2 rather than assumed. |
| **9** (blocking) | accepted | §2.5 defines **`"paste_paths_as"`**, a per-row *spelling* override taking `windows` / `windows-slash` / `wsl` / `msys` / `cygwin`, insertion-only, detector-untouched, environment-untouched; §2.2's ruling ⑨ points at it. The exact configuration is written out — `{"id": "gitbash", "paste_paths_as": "windows-slash"}` → `'D:/Demo/a.txt'`, POSIX quotes around a forward-slash Windows path — and carried into ruling ⑭, PROBE 4's table row, §6.1 and §6.2. One key, four existing namespace names plus the one spelling no namespace produces; no settings-page question. |
| **10** (blocking) | accepted | §4.2 ruling ㉕ picks **one** contract: a hard 512 MiB bound serialised across processes by a **crash-released OS lock** — `flock` on an open fd on Unix, a no-sharing handle (or `LockFileEx`) on Windows — held over accounting, eviction, reservation, write and the quota sweep. The ten-second staleness rule, the age steal and the bypass-on-contention are all **withdrawn**; contention waits **off the loop** for at most two seconds and then **refuses**. `.lock` is created once and never deleted, is outside the owned-name grammar and outside the quota. The 24-hour floor becomes a **retention floor on every eviction path** — quota, startup sweep, hourly sweep — and §4.3 drops the implication that it protects every live prompt. §6.1's lock tests are rewritten. |
| **11** | accepted, **and one half of it was not actually done until the fourth revision** | §4.1, §4.3 and §4.5. `folio-<uid>` was used in the selected path, the sweep, the privacy text and the delete command — but **the vetting recipe still said `Folio`** at the Unix level, so this row's "used in creation, vetting" was false when written and the fourth revision repaired it against `runtime_directory()`, both levels (§10.4 row 8); the `/tmp`-on-empty-`TMPDIR` rule is identified as **Folio's own policy** in `instance::runtime_directory()` (`instance.rs:331`–`:337`) and the false "which is what `std::env::temp_dir()` answers there" is struck; Windows temp is **configured, then vetted**, with `TMP` ahead of `TEMP` and no per-account claim; and the uid suffix is explicitly **not** proof against squatting, with the fail-closed limitation kept. |
| **12** (blocking) | accepted | §4.1 ruling ㉓'s Windows arm is rewritten: `CreateDirectoryW` applies a descriptor **only to a directory it creates**, so both existing levels are checked for **owner SID**, **DACL** and **inheritance to children**; a directory the user owns is repaired, one owned by another SID is **refused, never repaired**; storage that cannot keep persistent ACLs is **refused** rather than written to under a privacy promise it cannot keep; and the created file's own DACL is **read back** instead of assumed from the parent. §6.2 gains the broad-DACL, foreign-owner, no-inheritance, ACL-incapable and file-read-back rows beside the reparse fixture. |
| **13** | accepted | §3.4's `file URLs` row is split into a **file** row and a **folder** row, matching `row_verb` (`main.rs:28988`–`:28993`): Preview centre is `Retarget` for a file and `Refused` for a folder, Files centre the mirror. The distinction governs the **hover** as well as the commit, and §6.2 gains the held-folder-over-Preview row and its mirror. `Placeholder` and edge/rim cells are unchanged. |
| **14** | accepted | §2.3. PROBE 10's two halves are stated as independent, and the unrun fallback becomes **the whole arm not shipping**: without a named nushell version answering both width and argument position — builtin and external — a `nu` row refuses every path and says the lane is unmeasured. Narrowing the fence answers only half (a). The grammar table's row gains a literal and the construction formula instead of the repeated ellipsis, and §6.1's fence-growth test is gated on the measured maximum. |
| **15** | accepted | §2.3, §7.3, §8 and §9.1. The agent policy is **the same at every release size**: T-PASTE-1's gate and the shrink paragraph both say "inherit, and record the inheritance", and the shrink paragraph's contradicting "refusing rather than guessing" is struck. PROBE 9's obsolete child-read question is removed from §7.3 as well, and its unrun state is defined operationally — the lane ships on a recorded assumption and an actual TCC denial is a refusal — since an unrun experiment cannot answer a per-request access decision. PROBE 1 stays a mandatory shipping fixture, distinct from these fallbacks. |
| **16** (blocking) | accepted | §4.2 ruling ㉖ adds a **per-target input generation** that advances on submitted input, typing, an ordinary paste (`main.rs:96320`–`:96322`), a K144 insertion, a drop insertion and any other path insertion. A completion whose generation moved is invalidated: nothing is delivered, the file is cleaned up through the owned path, and a toast says the line moved on. The alternative contract — insertion into the then-current line — is named and **rejected**, with the reason. §6.1 gains Enter-before-completion and text-paste-before-completion fixtures, plus an unchanged-generation fixture so the check is not one-sided. |
| **17** (blocking) | accepted | §4.2 ruling ㉖ gives a job a **typed destination**: `TerminalInsertion` with session incarnation and input generation, `PreviewRetarget` with the pane and a content generation, ~~`LayoutSplit` with the captured anchor **seat**, the captured side and the tab's layout generation~~ — **superseded (§10.4 row 6): a raw picture is admitted on a *root rim*, which has no seat (`DropLanding::RootRim { edge }`, `main.rs:28787`), so the anchor is two-valued — `SeatEdge { seat, side }` and `TabRoot { side }` — each with its own revalidation, and a rim job is never repaired into a seat job (§4.2 ruling ㉖)** — the landing revalidated rather than recomputed, so a delayed split lands where the hand let go. Cancellation and file cleanup are stated for each target change. §6.2 gains delayed Preview and delayed edge fixtures, and T-PASTE-3's gates name them. `LeafSession` being a Terminal leaf's own PTY (`main.rs:10283`–`:10292`) is the cited reason a Preview target cannot carry an incarnation. **And the captured recipient this row put on the common job moved onto `TerminalInsertion` alone** (§10.4 row 6), the fifth revision adding the encoder configuration that travels with it (§10.5 row 3). |
| **18** | accepted | §4.2. The policy chosen is **preserve the complete animation**: original bytes are written, so an animated PNG stays animated, and validation decodes the default image **and** every animation frame under ruling ㉕'s caps applied to their sum — a truncated animation refuses, an over-cap one refuses rather than being flattened, and the toast reports that the file is animated. The first-frame rule is **struck**, along with the GIF example, which named a container neither platform's rung list offers. §6.1 gains an APNG fixture whose default image differs from animation frame one. |
| **19** | accepted | §4.1. Representability and **encodability** are kept apart: the directory gate stays as it is, and the job's **captured recipient** is used to run the prospective output through that pane's namespace and grammar **before the write**, so a `%`-bearing redirected `%TEMP%` or an unmeasured-grammar pane refuses without creating a file. Every post-write failure to deliver — encoder refusal, revalidation, cancellation, switch off, unspellable path — deletes the file through the owned path. §6.2 was said to gain the redirected-temp `%` row and the unmeasured-grammar row — **it did not, and the fourth revision added them**, each paired with a recipient that encodes the same directory successfully and each asserting that no file was created (§10.4 row 9). **And the mandatory common preflight this row created was wrong for two of the three destinations**, which the fourth revision moved onto `TerminalInsertion` (§10.4 row 6). |
| **20** | accepted | §8. T-PASTE-2a's scope now **enables `image/bmp`**: `BmpDecoder::new_without_file_header` is gated on that feature (`image-0.25.10/src/lib.rs:245`–`:246`, `Cargo.toml:70`) and this workspace enables only `gif, jpeg, png, webp` (`Cargo.toml:107`), so the decoder the design names cannot be imported as the manifest stands. The dependency, lockfile and notices impact is checked **for `bmp` specifically** rather than inherited from TIFF's, and the whole DIB lane is made contingent on the feature: without it, a DIB-only clipboard is `Absent`. §9.3 cites the gate. |

**One accepted in part, nothing refused.** Finding 5's request to keep two scopes
open for the owner is the only thing this revision does not do, because the owner
closed the question; everything else in the third review is accepted.

**And the closure claim this paragraph made is recomputed** (fourth review finding
12). It said the blocking seven were "resolved in the body rather than in this
table". The fourth review verified the twenty rows above and found **eight closed
and twelve partially closed**. So the honest statement is this: **the third
revision moved every one of the twenty in the body, and eight of them all the
way; the other twelve needed the fourth revision to finish, and §10.4 is where
each of those landed.** No row of this table was refused, and none is reopened.

**Two things that paragraph ran together are separated here** (fifth review
finding 5). A **row status** in the fourth review's ledger says how far the third
revision's answer got; a **finding severity** says how badly the fourth review
judged what it found. They are different lists and the fourth revision's text
mapped one onto the other. The fourth review's four **blocking** findings are its
numbers **1, 4, 5 and 6** — the grammar × spelling composition contract, the
reversed `SE_DACL_PROTECTED` rule, the generation list that omitted every editing
input, and the common encoder preflight two of three destinations could not
satisfy — and §10.4 carries each. The concurrent-copy fixture (finding 2) and the
missing preflight fixture rows (finding 9) were **should-fix**, not blockers,
however prominently the partial rows they came from figure above; the mapping
from those rows to the findings that finished them is row 1 → §10.4 row 2, row 12
→ §10.4 row 4, row 16 → §10.4 row 5, and rows 17 and 19 → §10.4 rows 6 and 9.

**And 8/12 is history, not a current certification.** It is the fourth review's
verified count of these rows, kept because it records what that pass found. The
standing count is the **fifth** review's assessment of the fourth review's own
twelve rows — **eight closed and four partially closed** — which §10.5 carries.

### 10.4 Fourth review — `paste-paths-review-4-2026-09-15.md`, 12 findings

Four blocking, eight should-fix, and a ledger of the third review's 20 rows in
which eight were closed and twelve partially closed, plus eleven second-ledger
closure sentences of which six were complete. §10.1, §10.2 and §10.3 now carry
what the fourth revision changed in each of the rows it touched. **Every finding
below is accepted**, and every one of them lands in the body — §10.4 records
where, it does not stand in for the change. Nothing is declined and nothing the
owner settled is reopened: the standing `(paths, integration)` namespace
derivation, the rejection of the program-keyed alternative, `"paste_paths_as"` as
an insertion-only spelling override, and Cygwin as T-PASTE-CYG dark until PROBE
11 are all taken as given below rather than re-argued.

| # | Decision | What changed, and where |
| --- | --- | --- |
| **1** (blocking) | accepted | §2.5 gains **ruling ⑫b, the composition contract** — a new subsection after the override's definition. Five rules carry it: **A**, the spelling runs first and produces one string and the grammar then encodes *that string* and reads nothing else; **B**, a table giving each spelling its emitted **path kind** and the namespace context it needs, with `wsl`'s distribution sourced from `wsl_distribution(index)` (`profiles.rs:3237`–`:3243`) — the row's own `-d`, else the machine default from `wsl::facts()` — so a `wsl` spelling on a non-WSL row is **accepted** rather than invented, `None` leaving a distribution share untranslated, and with `home` explicitly never needed because ruling ⑬ never emits `~`; **C**, four refusals that are properties of the emitted string (PowerShell's quote class; a `"` under `Cmd` and `Agent`-Windows; `%` and `!` under a **named** `cmd` grammar only; the unmeasured `Nushell` arm); **D**, the fallback string is the host path and the grammar reads its kind, which is how §2.3's **`Agent` branch is selected — by the emitted string, not the host path** (§4.4 corrected to match); **E**, the key is Windows-only and is refused at the row on a Unix host, once. The **6 × 5 product table** states the cells, and §2.3's `cmd` paragraph now refuses `"` as an **encoder** rule instead of asserting that no filename contains one. §6.1 gains the combination fixtures — apostrophes, drive and UNC roots, an embedded `"`, `paste_as: cmd` over an `msys` spelling, and the Unix-host refusal — and T-PASTE-1's gates name them. |
| **2** | accepted | §6.1's **Coherence, per platform** paragraph and §6.2's delayed-rendering rows are rewritten to assert what `OpenClipboard` guarantees. The Windows fixture now asserts four things: a competing `OpenClipboard` **fails** while Folio holds the clipboard, Folio's snapshot **completes**, the other process's copy succeeds after the close, and the **next** gesture reads it. Delayed-render first-paste success is kept; macOS keeps its separate `changeCount` replacement refusal, because AppKit makes no exclusion promise. **No sequence equality and no ordering assertion anywhere**, and the paragraph says that a future real Windows invalidation enters as a named observable failure rather than as a restored equality test. |
| **3** | accepted | §6.2's `Cmd`-encoder row becomes **two rows**. The encoder row hands the encoded command line **straight to `CreateProcessW`** with the argv printer as the image and **no `cmd.exe` between** — trailing backslashes, a space, an apostrophe, a lone `%` and **a paired `%NAME%`** — and the acceptance row keeps §2.3's **percent refusal in a `cmd` profile**, for both the lone and the paired form. §2.2's sentence that sent the encoder row through a `cmd` row is corrected at its source. No REPL guarantee is restored, and any future non-cmd interactive row must name a recipient whose input protocol consumes the grammar under test. |
| **4** (blocking) | accepted | §4.1 ruling ㉓'s Windows arm is rewritten. **`SE_DACL_PROTECTED` is required, not forbidden** — it is the boundary against inheritable ACEs from the `%TEMP%` parent chain — and propagation to children is stated as the separate mechanism it is, `OBJECT_INHERIT_ACE` + `CONTAINER_INHERIT_ACE`, both checked. **One principal at every level**: the token's **user SID** with `FILE_ALL_ACCESS`, no administrative identities, spelled `D:P(A;OICI;FA;;;<user SID>)` for the two directories and `D:P(A;;FA;;;<user SID>)` for the file — so creation and read-back ask the identical question, which the third revision's two lists did not. The **user SID is kept distinct from the logon SID**, with the reason (files must open after the next logon), and `SECURITY.md:39`–`:45` is cited for **one** thing, its fail-closed rule. The take-ownership and `SeBackupPrivilege` residue is stated. §6.2 gains a **protected-directory success** row and a **permissive-parent inheritance** row beside the existing ACL fixtures, plus a logon-SID refusal row; T-PASTE-2b's gates name the first two. |
| **5** (blocking) | accepted | §4.2 ruling ㉖'s generation list is replaced by a rule over the input's **origin**: the generation advances on **every user-originated byte this window puts into that target's PTY** — navigation, editing, control keys, history recall, Tab, IME commits, pastes and insertions alike. The boundary is named and cited: `input::keyboard_bytes` (`input.rs:530`–`:632`, arrows and `Home`/`End`/`Delete` at `:567`–`:582`, `Enter`/`Backspace`/`Tab`/`Escape` at `:619`–`:622`, `Ctrl+C` at `:540`–`:544`, the control alphabet at `:558`–`:563`) is the encoder, and the **target-specific** call is `note_user_typing(seat)` (`main.rs:85381`), whose own header already scopes it to "the four doors a person's own input reaches a shell through" and excludes replies, mouse forwarding and the resize-repair chord (`main.rs:85368`–`:85375`); the drop insertion becomes the fifth door. Terminal replies (`main.rs:35630`), mouse forwarding (`main.rs:18590`–`:18606`), the PSReadLine chord (`:83223`), a restored pane's replayed line (`:35702`) and local-only UI actions are **excluded deliberately**, each with its reason. §6.1 gains Left/Home, `Up` history recall, Backspace/Tab, `Ctrl+C`/`Ctrl+U` and an IME commit, plus three negatives — unchanged generation, a terminal reply, and the same keys in **another pane**. T-PASTE-2b's gates name them. |
| **6** (blocking) | accepted | §4.2 ruling ㉖ moves the **captured recipient off the common job** and onto `TerminalInsertion { leaf, incarnation, input_generation, recipient }`, which is the only destination with a shell; §4.1 scopes the pre-write encoder check to it. `PreviewRetarget` and `LayoutSplit` get the check that is meaningful for them — **native file-open/path validation**: absolute, below the vetted directory, inside the platform's length limit, no character the file API refuses. Security, size, cancellation and cleanup stay common. **A surrogate recipient is never chosen**, said in as many words. The split anchor becomes two-valued — `SeatEdge { seat, side }` and **`TabRoot { side }`** — because §3.4 admits a raw picture on a root rim and `DropLanding::RootRim { edge }` has no seat (`main.rs:28787`, mapped to `seats::LayoutAim::Rim(edge)` at `:28848`, `aimed_at` answering `None`); each variant gets its own revalidation, and a rim job is **never** repaired into a seat job. §6.1 gains **delayed root-rim success** and **changed-root cancellation** beside the Preview and edge fixtures, and a row proving the two nonterminal destinations complete with every terminal on the machine a `cmd.exe` row. T-PASTE-3's gates name them. |
| **7** | accepted | §2.5's classifier gains **eligibility before evidence**: only a resolved program whose lower-cased stem (read as `derive_integration` reads it, `profiles.rs:826`–`:829`) is one of `bash`, `sh`, `dash`, `zsh`, `ksh`, `mksh`, `tcsh`, `fish` is classified at all — everything else keeps today's answer **with no directory read**. Evidence from the executable's directory and its `..\usr\bin\` sibling is **aggregated as a union**, which makes the rule order-independent; a location that does not exist contributes nothing, while one that exists and cannot be read is a **partial read that stops the classification**. Only a union of exactly `cygwin1.dll` is positive. The **unrun-probe policy covers the explicit override too**: while PROBE 11 is unrun, `"paste_paths_as": "cygwin"` is refused with an unmeasured-spelling toast — one gate, not two, because an arm live for the override is not dark — and the reader's remedy is `"windows"`, which Cygwin tools accept. Fixtures for non-shell, cross-level mixed, partial-read and unrun-override cases in §6.1 and §6.2. **All of it inside T-PASTE-CYG**, whose gates name the new rows; T-PASTE-1 is untouched. |
| **8** | accepted | §4.1's Unix vetting recipe names `runtime_directory()`'s **`folio-<uid>`** — `$TMPDIR` when set and non-empty, else `/tmp` (`instance.rs:331`–`:337`) — and its `clipboard` child, **both levels vetted**, with `Folio` identified as the Windows name that appears at no Unix level. `prepare_runtime_directory` (`instance.rs:360`) is cited as the recipe being applied at both levels rather than borrowed for one, and the fail-closed squatting limitation is kept. §10.3 row 11's false "used in creation, vetting" is corrected in place, and §6.2's Unix storage rows say the two levels by name. |
| **9** | accepted | §6.2 gains the two picture-job rows the third ledger claimed: a **valid `%TEMP%` redirected to a path containing `%` with a `cmd.exe` recipient**, and a **valid directory with an unmeasured `nu` recipient** — each **refused before the write** with **no file created**, and each **paired with a recipient that encodes the same directory successfully** (`pwsh` and `bash`), so a directory refused for everybody cannot satisfy the test by accident. §4.1's own sentence gains the pairing requirement, and **T-PASTE-2b's gates name all four rows**. |
| **10** | accepted | §7.2's routing sentence is split. A **promise-only clipboard is the payload's `Refused(Promise)` value** — §1.2's and §5's answer, not the lane's error channel; an **acquisition** failure is the rung's `Unreadable`; and every **later** gate, decode, storage, quota, vetting, encoder or delivery failure uses the picture lane's error path. All three reach the same toast surface for different causes. No required toast changes and silent `Nothing` is unchanged. |
| **11** | accepted | §6.1's blanket "the suite asserts that no derivation reads a program" is **struck** and replaced by three scoped assertions: the **namespace** resolver is `printed_path_namespace` and no program-keyed `derive_namespace` is called because none exists; **`derive_grammar` does read the row's resolved program** and the test asserts that it does (`PowerShellSeven` without a file name, `FirstOf` off its first candidate, `served_by` never consulted); and the spelling override affects insertion only. §5's crate map carries the same distinction, and §10.3 row 5's overbroad closure sentence is corrected in place. No program-keyed namespace design is added. |
| **12** | accepted | §10.1 gains a lead-in saying its rows are the **first** revision's historical record, and five superseded clauses are **struck in place** with the current ruling named: row 11's before-and-after sequence check, row 19's first-frame policy **and** its "measured latency contract" for PROBE 8, row 20's `std::env::temp_dir()` Unix discovery **and** its owner-only-DACL sentence, and row 22's child-read half of PROBE 9. §10.2 gains the same lead-in and strikes three: row 1's "unbounded" delayed render and its calling-thread-window claim, row 7's ten-second staleness rule with its age steal and quota bypass, and row 11's width-one `nu` arm. §10.3's four overstated closure claims — rows 2, 5, 11 and 19 — are corrected in place, and its closing paragraph is **recomputed** against the fourth review's own count: eight closed, twelve partially closed, four of them blocking, each named with the §10.4 row that finished it. |

**Nothing is declined, and nothing is refused.** Two findings were answered by
reversing a rule the previous revision had stated backwards — **4**, where
`SE_DACL_PROTECTED` is the protection rather than the hole, and **2**, where the
API's own exclusion guarantee replaced a refusal it makes impossible — and two by
moving a check to where it can actually be satisfied: **6**, the shell preflight
onto the one destination that has a shell, and **5**, the generation from a list
of keys to the origin of the bytes. The four blocking findings are resolved in
§2.5, §4.1, §4.2 and §6.1–§6.2 respectively, not in this table.

### 10.5 Fifth review — `paste-paths-review-5-2026-09-15.md`, 6 findings

One blocking — and blocking **T-PASTE-2b and T-PASTE-3's terminal picture
delivery only**, not T-PASTE-1, which the review declares ready to open — four
should-fix and one nit, together with a ledger of the fourth review's 12 rows in
which **eight were closed and four partially closed**. Those four are rows 1, 3,
5 and 12, and the findings below are what finishes each: row 1 by findings 2 and
3, row 3 by finding 4, row 5 by finding 1, row 12 by finding 5. **Every finding
below is accepted**, and every one lands in the body — this table records where,
it does not stand in for the change. Nothing the owner settled is reopened: the
standing `(paths, integration)` namespace derivation, the rejection of the
program-keyed alternative, the insertion-only spelling override and the dark
Cygwin arm are taken as given.

| # | Decision | What changed, and where |
| --- | --- | --- |
| **1** (blocking for T-PASTE-2b) | accepted | §4.2 ruling ㉖. The picture job's input generation stops being an alias for `note_user_typing` and becomes **its own per-target boundary**: it advances at that call's five doors **and** on `WheelRoute::ArrowKeys` delivery to the job's target, which builds its `Up`/`Down` bytes from the very same `input::keyboard_bytes` (`main.rs:95117`–`:95136`, `input.rs:456`–`:468`) and is kept distinct from `MouseReport` and `Local` by the route decision itself (`main.rs:110810`–`:110823`). **Forwarded mouse reports now advance it too**, conservatively, and the unproven "a program in mouse tracking is not at a prompt" sentence is **struck** — the cited enum names a door, not the recipient's state, and the narrower supported-recipient contract is rejected because this document cannot enumerate it. Local scrolling, terminal replies, the PSReadLine repair chord and a restored pane's replayed line stay excluded, each on the same test. `note_user_typing`'s command-history meaning and its four doors are untouched. §6.1 gains the **wheel-to-`Up`/`Down`** row — cancellation, no insertion, owned-output deletion, both cursor modes — and the forwarded-report row, with **local-wheel** and **other-pane** controls beside them; the old mouse-report negative row is replaced. T-PASTE-2b's gates name them, and T-PASTE-3 inherits them for a raw picture landing on a terminal centre. |
| **2** | accepted | §2.5's product fixtures, §6.1's composition rows and §6.2's shell matrix. A cell whose rule has a **state** is written as a **pair**, and a pure fixture **selects the probe policy explicitly** instead of inheriting the build's: `D:\John's Archive\a.txt` × `wsl` × `PowerShell` is a **C1 refusal with no insertion** while PROBE 2 is unrun, and `'/mnt/d/John''s Archive/a.txt'` once the recorded result narrows the quote class. The third revision asserted only the second half unconditionally, against §2.3's own shipped fallback. §6.2's matrix gains the same distinction: a refused name is a **refusal observation** — the toast, and no bytes in the pane — not a line to run and a file to open, so narrowing a refusal set later changes which half of a pair is exercised rather than turning the matrix red. |
| **3** | accepted | §2.5 rule A and §4.2 ruling ㉖'s `TerminalInsertion`. Rule A's "the grammar reads nothing else" is kept for **path-shaped inputs** and corrected for policy: spelling produces the string a **configured** encoder consumes, and the configuration is captured at the gesture or spawn boundary — the **`Cmd` origin** (named versus ruling ⑩'s unknown-program default), the row's **delayed-expansion flag**, and the **applicable probe policy** for C1, C4 and the `cygwin` column. Bare `ShellGrammar::Cmd` plus a string cannot express what C3 and C4 decide on, which is why the fourth revision's sentence was unimplementable as written. The encoder stays pure — no live pane, no profile row, no original host path — and the captured recipient carries that configuration plus the effective insertion spelling and its context, so preflight and write encode the same string. §6.1 gains paired `%` and `!` fixtures across **both** `Cmd` origins, the `!` pair run with and without `/v:on`. |
| **4** | accepted | §6.2's `CreateProcessW` encoder row. The harness is specified to the argument index: an **MSVC CRT `wmain`** consumer, `lpApplicationName` pointing at it, and a **mutable** `lpCommandLine` holding a quoted program-name token, a space, then the exact tested literal — asserting `argc == 2` and `argv[1]` equal code unit for code unit. The fourth revision's command line was the encoded path alone, which would have placed the literal in **`argv[0]`**, where the backslash and quote conventions under test do not apply and where `CreateProcessW` does not supply the image name as an argument. The trailing-backslash, space, apostrophe, lone-`%` and paired-`%NAME%` rows are retained, and the separate `cmd`-profile refusal row is untouched; no interpreter and no REPL returns. |
| **5** | accepted | §10.3. It gains the same **historical-record lead-in** §10.1 and §10.2 carry, with row 8 pointed at §10.4 row 3 and row 17 at §10.4 row 6; both rows are then **struck in place** where they still read as current — row 8's argv printer "launched from a `cmd` row", row 17's seat-only `LayoutSplit` anchor. The closing paragraph's blocker attribution is repaired: a **row status** and a **finding severity** are different lists, the fourth review's blocking findings are exactly **1, 4, 5 and 6**, and its findings 2 and 9 were should-fix however prominent the partial rows they came from. The verified 8/12 count is kept as the fourth review's history rather than as a current certification; the standing count is this review's **8 closed, 4 partially closed** over §10.4. |
| **6** (nit) | accepted | Three sentences narrowed. §6.1's namespace assertion is scoped to **variant selection** by `(paths, integration)`, leaving the chosen variant's own context population — the WSL distribution, the MSYS home — exactly as the resolver already does it. §7.2's later-failure list reports through the **application's own refusal-reporting path**, with the picture lane as one caller, since T-PASTE-1 ships path refusals before any picture exists. And T-PASTE-CYG's scope says the classifier has **seven** outcomes, the count the table has carried since eligibility was added. |

**Nothing is declined.** The one blocking finding was answered by making a
boundary its own rather than borrowing another feature's, and by taking the
conservative side of the mouse question rather than asserting something about
recipients this document cannot check. Two more were answered by writing down
inputs that were always being used but not named — an encoder's policy, a probe's
state — and the rest by saying which tense a ledger row is written in.
