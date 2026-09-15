# A file on the clipboard, a file on the pointer, and a picture with no name

Design for 0.4.1 — GitHub issues #1 and #2. 2026-09-15, branch
`docs/paste-paths-design` off `main` at `76ca0788`. **Revised twice**: against
`docs/plans/review/paste-paths-review-2026-09-15.md` (24 findings, 7 blocking)
and then against `docs/plans/review/paste-paths-review-2-2026-09-15.md` (23
findings, 4 blocking). §10 carries both ledgers, and the rulings below are the
twice-revised ones. **Docs only**: nothing here is built, no crate is touched,
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
are collected in **§9.1**. One thing is marked **OWNER RULING NEEDED** (§2.5),
because it would reverse a ruling the owner took on a dated branch and that is
not a decision a design document may take for itself.

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
   owner's standing 2026-09-07 derivation** until the owner rules otherwise
   (§2.5). This window does not know what program is reading the line *now* and
   will not guess from the screen.
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
that is one case, a promise (ruling ⑤) — and only it routes to a toast.
`Nothing` stays silent, so `Ctrl+V` at a fresh login says nothing at all.

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

**Ruling ④: availability is surveyed first, exactly one rung's bytes are
fetched, the fetch runs on the event-loop thread, and a delayed render is
UNBOUNDED.** This ruling was rewritten: the first draft said the read was
"bounded" and booked a probe to find the number, and no Win32 call on the path
takes a timeout.

* **Two steps, not one.** First the *advertised type list* is surveyed —
  `IsClipboardFormatAvailable` per format on Windows, `NSPasteboard.types` on
  macOS. **Surveying renders nothing**: it is a cheap, local question. Only then
  is the winning rung fetched. The earlier draft fetched up to five formats
  inside one open and therefore multiplied the exposure by five; this fetches
  **one** — or, for the picture rung, the encodings of that one rung (§1.2's
  list), which is the single place where more than one fetch happens and is
  bounded by ruling ㉕.
* **One transaction.** Survey and fetch happen inside one `OpenClipboard` …
  `CloseClipboard` pair — the shape `clipboard_text` already has — with
  `GetClipboardSequenceNumber` read before and after, and on macOS the
  pasteboard's `changeCount` before and after. If the number moved the whole
  read is discarded and the paste is refused with a toast rather than delivering
  a mixture of two clipboards. Delayed rendering means the owner runs code while
  we hold the clipboard open, and that code can legally replace it.
* **The thread is winit's event-loop thread**, because that is where
  `register_clipboard_owner`'s window lives (`lib.rs:5845`, `:5927` — "all calls
  run on winit's event-loop thread") and because the paste is a keystroke
  (`main.rs:96290`).
* **Therefore it can hang, and the document says so.** `GetClipboardData` on a
  delayed format sends `WM_RENDERFORMAT` to the owning process and blocks until
  that process's thread answers. There is no timeout and no cancellation. A
  browser mid-GC, an Office process on a stalled network drive, or a
  remote-desktop clipboard bridge can freeze this window's loop — no frame, no
  keystroke, no resize — for as long as it takes. **The mitigation is honesty
  plus a station**: the acquisition runs inside a new `hang_watch` station,
  `ClipboardRead`, beside the other synchronous cross-process calls this loop
  makes (`hang_watch.rs`: `PtyResize`, `WebPage`, `WebRetire`), so a hang report
  names it instead of pointing at the event loop in general.
* **PROBE 8 is redefined.** It no longer promises a bound, because a number
  measured from five sources is not a bound on an arbitrary one. It measures the
  **distribution** — how long a delayed render takes from the common Windows
  sources — and its output decides a *design* question, not a constant: whether
  the transaction must move off the loop.
* **The escape hatch is named, not implied.** If the distribution is bad, the
  whole transaction moves to a worker thread of its own, which on Windows costs
  that thread its **own message-pumping clipboard owner window** —
  `OpenClipboard` associates the clipboard with a window of the *calling* thread
  (`lib.rs:5838`), so the existing owner cannot be borrowed. That is a different
  `clipboard_payload` and a different ticket; it is booked in §9.2 as a debt with
  its cost written down rather than assumed into this one.

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

**Ruling ⑨: one `profiles.json` key, and no settings-page question.** A row may
carry `"paste_as": "powershell" | "cmd" | "posix" | "fish" | "nu" | "agent"`, and
a row that carries it is believed. It exists because inference provably fails for
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
`D:\Data\100%\report.csv` is a legal name that works perfectly in those panes,
and refusing it there would be trading a right answer for a visible wrong one.

### 2.3 The grammar table

Every grammar **always quotes**. There is no bare form and no "is this path
simple enough" predicate — that predicate was the whole of finding 1, and the
cost of removing it is quotation marks a reader can see, against a benefit of
never emitting a path that a shell re-lexes into something else. Windows
Terminal's own issue #8109 makes the same argument for drops.

| Row | Grammar | Literal | Escape inside |
| --- | --- | --- | --- |
| `pwsh`, `winps`, `powershell`/`pwsh` stems, `PowerShellSeven` | `PowerShell` | `'…'` | every character PowerShell reads as a single quote, doubled — **PROBE 2** |
| `cmd` | `Cmd` **+ the `cmd.exe` interpreter rules** | `"…"` | trailing backslashes doubled (2N); `%` refused; `!` refused when the row's own args turn delayed expansion on |
| `bash`, `zsh`, `sh`, `dash`, `ksh`, `wsl`, `gitbash` | `Posix` | `'…'` | `'` → `'\''` |
| `fish` | `Fish` | `'…'` | `\` → `\\`, `'` → `\'` |
| `nu` | `Nushell` | `r#…#'…'#…#` — one spelling, §2.3's growth rule | none; the fence grows — **PROBE 10** |
| the seven `AGENT_IDS` rows, **Windows path** | `Agent` | `"…"` | nothing — `"` is illegal in a Windows name |
| the seven `AGENT_IDS` rows, **POSIX path** | `Agent` | `'…'` | `'` → `'\''` |
| anything else | `Posix` on Unix, the `Cmd` **encoder only** on Windows | as above | as above — **no `%` or `!` refusal** |

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
consumers.** `"` is illegal in a Windows filename, so nothing inside the quotes
needs escaping for `cmd` itself. What needs care is everything else:

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
  reason. Quoting
  it would hand the reader a mangled name that looks like it worked, and this
  product's standing rule is that a visible refusal beats a silent wrong answer.
* **`!NAME!` expands only under delayed expansion**, which is off by default and
  which this window *can* read for the one case that matters: a row whose own
  `args` carry `/v:on` or `/v on`. A path containing `!` is refused in such a
  row and allowed elsewhere, with the residue stated — a reader who turned
  delayed expansion on from inside the shell is outside what the row can say.
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
encoder"). **PROBE 10: on a pinned nushell version, find the maximum fence width
the parser accepts, and confirm a raw string is accepted in *argument* position**
— the book documents raw strings under strings, not under command arguments, and
argument position is where every path this feature emits lands. Until it is
answered, a `nu` pane refuses any path that would need a fence wider than *n* = 1
and refuses nothing else; if the probe finds argument position does not take a
raw string at all, the arm is replaced or the row refuses, and §6.2's `nu` row is
where that is decided.

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
single-quoted for a POSIX path.** `"C:\a'b.png"` strips to `C:\a'b.png`, which
the recogniser returns unchanged and correctly; `"` needs no escape because it is
illegal in a Windows name; and the double-quoted form is additionally the
spelling Explorer's own *Copy as path* produces, which is what these tools are
most likely to have been built against. The bare form stays withdrawn: on macOS a
bare `/Users/ann/My Pictures/screen.png` lexes to two tokens and attaches
**nothing**.

Three consequences are stated rather than assumed:

* **Prose references and automatic attachments are different things.** Claude
  Code documents paths written in prose, which a *model* reads — quotes are read
  correctly there, and model interpretation is not deterministic filename
  parsing, so no promise is made about it beyond "the path is present and
  delimited." Copilot CLI documents `@` attachments, not a bare-path grammar.
  Codex is the only one of the three whose parser was read. **The "seven agents
  take a bare path" rule of the first draft had no evidence behind it for six of
  them.**
* **Several paths in one insertion are not several attachments.** Codex's parser
  returns `None` when shlex yields more than one token, so a two-file paste into a
  Codex pane attaches nothing. The design still inserts both, space-separated and
  each quoted, because that is the text the reader asked for and can edit; it
  **does not promise** that more than one becomes an attachment.
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
  carry.

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

**Ruling ⑫ (revised): the insertion namespace uses the owner's standing
derivation; the change the first revision made is withdrawn into a proposal.**

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
reversal**. That was the document overturning an owner's decision on its own
authority, which is not a thing a design may do.

**So: the standing derivation stands.** T-PASTE-1 calls
`printed_path_namespace` as it is, for insertion as well as for detection, and
the detector is not re-pointed.

**OWNER RULING NEEDED — the two sides, written out.**

*For changing it.* A `gitbash` row whose owner set integration to `None`
answers `Windows` (`profiles.rs:3219`), so that pane is handed
`'D:\Demo\a.txt'` while every path it prints is spelled `/d/…`. The shell is the
same shell either way; what the reader turned off was a startup script. Under
the standing rule the pasted path is *tolerable* — MSYS tools accept a Windows
path with backslashes — but it is not the spelling the pane speaks, and the
detector has the matching hole in the reading direction.

*For keeping it.* The pair is what the owner chose, and the ruling's own
sentence — "Windows directories behind a bash init file is an MSYS bash" — is a
*definition* of MSYS-ness, not a proxy for one. Keying on the program instead
means deciding what a `bash.exe` is by where it sits on the disk, which is the
next paragraph's problem, and it changes the **detector** — a surface outside
this feature, on every Git Bash pane, in a release whose headline is paste.

**Cygwin, which neither derivation has an arm for.** A Cygwin `bash.exe` row has
the same program stem *and* the same `paths: Windows` *and* the same
`BashInitFile` door as a Git for Windows row, so **both** the standing rule and
the proposed one answer `Msys` and spell `/d/Demo` — which Cygwin cannot open,
because Cygwin mounts drives at `/cygdrive/d`. The hole is therefore **not
introduced by the proposal**; it exists today, in the detector, where a Cygwin
pane's printed `/cygdrive/d/…` is not recognised and a printed `/d/…` would be
recognised as a file it is not — §7.1.5f's "a mark that answers hover but not
click". The design's own cited precedent needs five values where this codebase
has three: Windows Terminal's `pathTranslationStyle` is
`none` / `wsl` / `cygwin` / `msys2` / `mingw`.

**Ruling ⑫a: a `Cygwin { home }` arm is added, spelling `/cygdrive/<letter>`,
and it is recognised by the Cygwin runtime DLL sitting beside the program** —
`cygwin1.dll` beside a Cygwin `bash.exe`, against `msys-2.0.dll` beside Git for
Windows'. That is a file-existence question of exactly the kind the profile table
already asks to find a program at all, asked once when the row resolves.
**PROBE 11** confirms the sibling-DLL test on a real Cygwin and a real MSYS2
installation; until it is answered, a row with neither DLL beside it keeps
today's answer. **MinGW is deliberately not an arm**: `mingw` in WT's list means
the same Windows paths with `/` separators rather than a different namespace, and
a quoted Windows path already works there — so there is nothing for a namespace
to do.

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
is the documented fallback for that row — through the `paste_as` override, not by
guessing. **PROBE 4.**

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
| file URLs | yes | `Insert` (many) | `Retarget` (one) | `Retarget` for a folder, `Refused` for a file | `Refused` | `Split` (one) |
| raw picture (`public.png`; `CF_DIB`/`CF_DIBV5`/`"PNG"`) | yes | §4's lane, then `Insert` | §4's lane, then `Retarget` | `Refused` | `Refused` | §4's lane, then `Split` |
| a file **promise** | yes, **to refuse it visibly** | `Refused` | `Refused` | `Refused` | `Refused` | `Refused` |
| text | not registered | — | — | — | — | — |

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

* **Windows**: `std::env::temp_dir()`, then `Folio`, then `clipboard`. The
  platform's temp is already per-account here, which is the whole of what this
  design needs from it; the earlier draft named the Win32 function `std` happens
  to call, which is a `std` implementation detail that buys nothing and can go
  stale, and that sentence is struck.
* **macOS**: `folio-<uid>`, then `clipboard`, under `$TMPDIR` — **or under
  `/tmp` when `$TMPDIR` is unset or empty**, which is what `std::env::temp_dir()`
  answers there and which the earlier draft's "`$TMPDIR`, which is already
  per-account" quietly omitted. This is `instance::runtime_directory`'s own
  shape, ten lines above the vetting this section takes as its precedent
  (`instance.rs:331`), and its header gives the reason: "two users on one machine
  must not meet in one directory, and the one that got there first must not be
  able to decide what the second one finds." A fixed `Folio` under `/tmp` is a
  name any other local user can create first, after which §4.1's uid check
  refuses it, the lane fails closed, and the feature is **dead for that account
  on that machine with no remedy** — a one-line local denial of service. Taking
  the precedent's vetting without its naming was the mistake; both are taken now.

macOS's `/var/folders/…` form and its `/private/var` alias are **not** rejected:
they are the standard answer, and a check that refused them would refuse every
Mac.

**The directory is spelled once, at vetting, or the lane refuses.** §2.4's
representability gate runs on the **directory** as part of this vetting — before
any picture is written — so that a temp path that cannot be spelled as a shell
literal refuses the whole lane with one toast, rather than writing a file and
then refusing to name it. The failure is real: a redirected `%TEMP%` with an
unpaired surrogate, or a `$TMPDIR` with a non-UTF-8 byte. A job that somehow
reaches completion with an unspellable path deletes its own file through the
owned path, exactly as a cancellation does (ruling ㉖) — a picture on the disk
that nothing points at is the write the §4.6 switch was supposed to be the only
cause of.

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

* **Unix.** `DirBuilder::mode(0o700)` for `Folio` **and** for `clipboard` —
  both, because an intermediate directory of ours redirected by a link
  redirects the final one that is not a link. Then, for each level:
  `symlink_metadata`, not a symlink, is a directory, `uid == geteuid()`, and mode
  repaired to `0700` if it is anything else. A same-owner world-readable
  directory does **not** pass. Files are created `0600`.
* **Windows.** There is no uid; the equivalents are stated rather than left
  implied: refuse a **reparse point** at either level, and create both
  directories with a DACL granting the current user only — the same
  owner-restricted footing `SECURITY.md` records for the launch pipe. Files
  inherit it.
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

**Multi-frame sources take the first frame and say so** in the toast. A copied
GIF is a plausible thing to paste; silently keeping one frame without saying so
is not.

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
* **The quota has a floor, and it is 24 hours.** A write that would exceed the
  directory quota removes the oldest `clip-*.png` files — but **never one
  younger than 24 hours**, and if the older ones cannot free enough the write is
  **refused** instead. Without the floor, eight 64 MiB pastes in five minutes
  evict the first, whose path may be sitting in a half-typed command line the
  reader has not pressed Enter on. §4.3's disclaimer covers *history*; it does
  not cover a line that is still on the prompt, and eviction must not be able to
  reach one.
* **The read-evict-write sequence is serialised across windows and processes**,
  because the directory is shared and "one job in flight per window" says nothing
  about two windows or two Folios. The sequence takes an exclusive
  `clipboard/.lock`, created with `create_new` and owner-only and removed at the
  end, with a 10-second staleness rule for a lock left by a process that died.
  **If the lock cannot be taken, the write proceeds without evicting** and the
  quota may overshoot until the next sweep — stated here rather than pretended
  away, because refusing a paste because another window is busy would be worse
  than a directory briefly over its cap.
* **One in flight.** At most one picture job per window. A second picture paste
  while one is pending is **refused with a toast**, not queued — a queue would
  need an ordering contract for a gesture that has no meaningful order.
* **Failures are complete.** A write, flush or close error deletes the partial
  file and reports; disk pressure is a refusal, not a truncated PNG.
* **Acquisition has a latency contract.** A delayed-rendered format means the
  *owner* runs code before we get bytes; the read is bounded and a source that
  does not answer within it is `Unreadable` (ruling ③). **PROBE 8: measure
  delayed-render latency for the common Windows sources** so the bound is a
  number with evidence under it.

**Ruling ㉖: the delayed insertion carries identity, and it is revalidated at
completion.** Encoding a 4K DIB is a one-to-three-hundred-millisecond job and
must not run on the event-loop thread. That makes the insertion **asynchronous**,
and the first draft's single re-check of the cell left of the cursor solved none
of the real problems: a reader can switch tabs, restart or close the shell, open
a modal, press Enter, or paste again while it runs. So a job carries:

* **window, tab and leaf**, plus the **session incarnation** of that leaf, so
  that a restarted shell in the same seat is a different recipient and a reused
  `SeatId` cannot be mistaken for the original;
* a **request sequence number**, globally unique in the way `CONVENTIONS.md:154`
  requires of worker addresses, with one owner for the answer.

At completion the job **revalidates** the original target's existence and
incarnation, the modal gate, and the setting of §4.6 — and if any has changed, it
does not insert. Cancellation is explicit when the pane closes, the shell
restarts, the window closes or the setting is turned off; a cancelled job's file
is deleted through the owned path rather than left for the sweep. Ordering is
unnecessary because only one job is ever in flight (ruling ㉕), which is a second
reason for that rule. The leading-space rule is re-asked at insertion, as K144
already does — but as the *last* step, not the whole of the contract. The same
applies to a delayed picture **drop**.

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
* The directory is additionally bounded at **512 MiB** (ruling ㉕), oldest first.
  Those two together are the maximum retention this design offers.
* **The system may remove them sooner.** A temp directory is swept by the
  platform, and Folio does not prevent it.
* **A path on your screen is not a promise that the file is there** — not in
  shell history, and **not in a command line you have typed but not submitted**.
  The 24-hour eviction floor (ruling ㉕) is what makes the second case unlikely
  rather than impossible; nothing makes it impossible. A reader who wants to keep
  a picture copies it somewhere of their own, and the toast that reports a
  written file says where it is, which is the moment to do that.

**The sweep runs on the picture lane's worker, not on the event loop.** It is a
directory listing, a `symlink_metadata` per level, and up to N unlinks, against a
`%TEMP%` that §4.5 explicitly discloses may be redirected to a network share —
which on the event-loop thread would be an arbitrary stall once an hour with
nothing to name it in a hang report. It is the same worker the encode runs on, so
it is never concurrent with a write of this process's own. Anything of it that
does touch the loop shares ruling ④'s `hang_watch` station.

The owned-name grammar is exact — `clip-` + 8 digits + `-` + 6 digits +
optional `-` + digits + `.png` — because `clip-family.png` is a file a reader
could have put there and the sweep must not own it. Only **regular files** in
that one directory are considered: not directories, not symlinks, not anything
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
and into an agent pane single-quoted (§2.3). There is no second rule for
pictures, which is the point of writing the file at all: after the write, #2 *is*
#1.

### 4.5 What `PRIVACY.md` and `README.md` must say

A new row in `PRIVACY.md`'s **Elsewhere** list, in both languages, in that
section's voice, saying all of:

* the two directories by name, `%TEMP%\Folio\clipboard` and
  `$TMPDIR/Folio/clipboard`, and that the files are readable only by you;
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
bt-platform     clipboard_payload()  -> Files | Text | Picture(bytes + what they are) | Nothing
                                        each rung: Absent | Present | Unreadable
                                        one snapshot, change-count checked
                the drop door:       register / continuous point / payload / effect
                    windows_impl:    own OleInitialize + IDropTarget on every HWND
                    macos:           PROBE 5 — destination view, upstream, or replacement
                the file-URL decoder shared beneath macos_services::paths_on
                (clipboard_text() unchanged, and still what every text field reads)

bt-transcript   PrintedPathNamespace::to_pane_spelling()   — beside to_local_path
                derive_namespace()   — one derivation, both directions

bt-app          shell_literal.rs     representability gate + ShellGrammar + encoders  [pure]
                profiles::grammar()  — derive_integration's twin, plus "paste_as"
                main.rs              paste routing, drop landing, picture lane + job identity
                                     inserted_path_text (K144) absorbed and repaired
                the picture worker   decode, encode, write — and the hourly sweep
                hang_watch           one new station: ClipboardRead (ruling ④)
```

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
backslashes survive as two; an apostrophe becomes `\'`. `Nushell`: the fence
grows past a path containing `'#`. `Agent`: the result is one shlex token, which
is asserted **by running a shlex** over the output rather than by eye.

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
is `Windows` for spelling**, which is the standing 2026-09-07 derivation this
document keeps (ruling ⑫), and the test carries that expectation *with the
owner-ruling note beside it* so that whoever changes it knows what they are
changing. A **Cygwin** row spells `/cygdrive/d` (ruling ⑫a), and a row with
neither runtime DLL beside it keeps today's answer. `wsl.exe -e nu` without an
override is `Posix` and **the test says so, naming it as the spawn-time
default**; a `"paste_as"` row is believed.

**Rung semantics.** A fake clipboard described by each rung's answer —
`Absent` / `Present` / `Unreadable`, and `Present`-but-empty — across every
combination, asserting the chosen rung, that `Unreadable` stops the ladder, that
an empty `HDROP` is `Absent`, that an empty text is `Present`, that a changed
change-count discards, that a promise-only clipboard is `Refused(Promise)` and
**not** `Nothing`, that `Nothing` raises no toast and `Refused` does, that the
survey does not fetch, and that a picture rung carrying `[Png, Dib]` falls to the
`Dib` when the `Png` fails to decode — and is terminal when the list was
truncated at the bound.

**The picture.** Known-pixel fixtures per supported DIB layout — top-down and
bottom-up, `BI_RGB` and `BI_BITFIELDS`, V5 with and without a real alpha mask,
palette forms — asserting the written pixels; an unsupported layout refuses; a
truncated PNG with a valid `IHDR` refuses; dimensions over the cap refuse before
allocation; a multi-frame source writes frame one and says so.

**The file and the sweep.** The name from a fixed stamp; the collision ladder to
`-3`; the owned-name grammar accepting exactly its own shape and rejecting
`clip-family.png`; age by mtime, on both sides of the cutoff; a directory, a
symlink and a foreign file untouched; the quota evicting oldest-first; **a file
younger than 24 hours never evicted, and the write refused instead**; a refused
write when eviction cannot free enough; a stale `.lock` taken after 10 seconds
and a live one causing a write without eviction rather than a refusal.

**Job identity.** A completion whose leaf is gone does not insert; whose
incarnation changed does not insert; whose modal opened does not insert; whose
setting was turned off does not insert and deletes its file; **whose own path
fails the representability gate does not insert and deletes its file**; a second
paste while one is pending is refused.

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

**The shell matrix.** A file whose name has a space, an apostrophe, `$`, `%`,
`!`, a PowerShell smart quote and CJK, into `pwsh`, `winps`, `cmd`, `gitbash`,
`wsl`, `zsh`, `bash`, a `fish` row, a `nu` row and **a Cygwin `bash.exe` row** —
each line then **run**, and the opened file compared. Under `cmd`, a builtin
(`cd`, `type`) *and* a native child (`findstr`, a tiny argv printer) for the
trailing-backslash rows, with `/v:on` and without; and **a non-`cmd` Windows row
(a `python` or `node` profile) with a `%` in the name, which must work rather
than refuse** (ruling ⑩). Under PowerShell, 5.1 and 7, with PSReadLine default
and with bracketing off. Under zsh, default and with `bracketed-paste-magic`.
Under `nu`, a path needing a wider fence and a path in argument position (PROBE
10). Under Git Bash, an MSYS consumer and a native consumer with
`MSYS2_ARG_CONV_EXCL` unset and set (PROBE 4).

**The agents (PROBE 3).** Claude Code, Codex and Copilot CLI, versions recorded:
a spaced path, **an apostrophe path on both platforms**, two paths in one
insertion, a `file:` URI — and for each, whether it attached, referenced or
ignored the file. The Windows apostrophe row is the one that §2.3's reading of
`normalize_windows_path` predicts, so it is also the row that checks the
reading.

**The drop matrix.** From Explorer and from Finder onto: a terminal centre, a
terminal edge, a preview centre, a files column, the tab strip and the window
chrome; out of the window and back, so the box un-traces; a drop while a menu is
open; 3 files; 64 files; 65 files (refused); mixed file-and-folder on a terminal
and on a preview (refused); **a promise-only drag, which must trace the refusal
box rather than pass through unseen**; and a drop on a `Placeholder` pane. A
browser image drag from Safari, Chrome and Firefox (PROBE 6). On Windows: a
second window and the summoned terminal, to prove every HWND registered; **a
fullscreen transition with a target registered, because that is the one path
winit still initialises COM on** (`platform/windows.rs:493`); a DPI-changing
monitor, to pin the coordinate conversion; and a source that would have accepted
a move, to prove the effect returned.

**The storage.** A world-readable existing directory is repaired; a symlink at
either level refuses; a Windows reparse point at either level refuses; a file
lands `0600` / owner-only ACL; a `%TEMP%` redirected to a share writes there and
is disclosed.

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
`%` in a `cmd` pane, an unsupported bitmap layout, an over-cap picture, a busy
job, a full quota, a vetting failure, a promise-only payload, an over-count drop
— is a toast that names what was refused and why, in both languages.

### 7.3 What is disclosed rather than forbidden

`BT_PTY_DUMP` writes every byte of a pane to a file the reader names, and
`PRIVACY.md:186` already says so; a pasted path is in that file like any other
byte. That is an opt-in recording, not this feature's storage, and §4.5 keeps the
two apart. The Mac package is **not sandboxed**
(`packaging/macos/entitlements.plist:20`), so no App Sandbox container blocker is
invented here; what does have to be measured is whether a child process started
by a Folio row can read a file under `$TMPDIR` and whether any TCC prompt appears
on the paths this feature touches. **PROBE 9**, and there is no fallback copy
into a second location if it fails — a refusal instead.

---

## 8. The split

Three tickets. Each is independently shippable, and each states the lane it does
**not** ship so that the refusal is explicit rather than a gap.

### T-PASTE-1 — the clipboard door, and one path as one argument

**Size: L.** It was sized M–L on the assumption that the encoder was a table;
it is six encoders, a refusal gate, a two-direction namespace change and a
recipient contract.

`clipboard_payload` on both platforms with the three-state rung semantics, the
snapshot and change-count check, and the shared file-URL decoder (the picture rung
is declared and answers `Absent`). The representability gate. The six encoders.
`derive_grammar`, `derive_namespace` and the `"paste_as"` override, with
`printed_path_namespace` **used as it stands** (ruling ⑫) plus the `Cygwin` arm.
`to_pane_spelling`. The routing in `paste_from_clipboard_into`, the
`ClipboardRead` hang station, and the **Paste picture** row's enablement rule
(the row itself is T-PASTE-2's). **K144 absorbed and repaired**, keeping its
focus move. **Its own i18n row** — it ships at least six refusal toasts, so it
adds strings, needs its `Text::ALL` count bumped and carries the two-line budget
test; every ticket that adds a string keeps an i18n row, and the first ledger's
row 23 is corrected to say so. The rest of §6.1 except the picture, file, sweep
and job-identity rows; the shell and agent matrices of §6.2; PROBEs 1–4, 10 and
11.

**Gates:** every §6.2 shell row asserts the opened file; PROBE 2 answered **or**
the documented quote class refused (§2.3's concrete fallback); PROBE 3 answered
**or** the inheritance of Codex's spelling by the other six agent rows recorded
as a known risk — not a refusal, which is a product decision the grammar table
does not carry; PROBE 10 answered or `nu` refusing anything past fence width 1;
PROBE 11 answered or a row with neither runtime DLL keeping today's answer.
**Does not ship:** pictures, drops. A clipboard picture is `Absent` and the paste
says "nothing to paste"; a drop does nothing, as today.

### T-PASTE-2 — a picture becomes a file

**Size: L–XL, and it splits in two reviewable halves.**

**2a — acquisition and decoding.** The picture rung on both platforms, the
acquisition bound, the `new_without_file_header` path, the alpha and layout
table, full-decode validation with original-byte write, the frame rule, the
fixtures, PROBEs 7 and 8.

**2b — storage, delivery and the switch.** The directory trust contract on both
platforms, the name, the collision ladder, owner-only files, the age-and-quota
sweep, the job identity and revalidation, the one-in-flight rule, the
`Settings ▸ General` row in both languages, `PRIVACY.md` and the README clause,
PROBE 9.

**Gates:** the storage rows of §6.2; a written picture compared pixel-for-pixel
against its source for each supported layout; a refused layout refuses. **Does
not ship:** TIFF (a named debt with its notices work), promises, drops.

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

**Gates:** the drop matrix of §6.2 including 65 files, mixed kinds, a second
window and a fullscreen transition. **Does not ship:** the *reading* of promises
(they are registered only to be refused), strip verbs for OS drops.

**If PROBE 5 finds no acceptable shape, T-PASTE-3 ships Windows only.** That is
the ticket's stated fallback rather than an open end: macOS drops stay exactly as
they are today — nothing happens — and the README and `docs/features.md` say so
in both languages, which is the same "name the lane you do not ship" rule every
other ticket here follows. A probe whose failure has no product answer is not a
bounded probe.

**If 0.4.1 has to shrink**, T-PASTE-1 alone is a release: the K144 repair plus
file-clipboard pasting on the shells whose rows §6.2 measured, with the unmeasured
recipients refusing rather than guessing.

---

## 9. Probes and open ends

### 9.1 What still needs a machine

| | What | What it decides | If it is not run |
| --- | --- | --- | --- |
| **PROBE 1** | source × gesture × advertised formats × chosen rung, versions recorded, **including WSLg, `clip.exe` and RDP** | nothing — it is a **fixture matrix to record**, not a blocker on a ruling; ruling ② is implemented the same way whatever it says | T-PASTE-1 does not ship: the matrix has to exist, but no ruling waits on its content |
| **PROBE 2** | PowerShell 5.1 and 7: which code points close a single-quoted string, and whether doubling each yields one character | how wide the `PowerShell` refusal is | refuse the whole documented quote class (§2.3) |
| **PROBE 3** | Claude Code and Copilot CLI: spaces, apostrophes on both platforms, two paths, `file:` URI (Codex is **measured**, not probed) | whether the six inheriting rows need arms of their own | record the inheritance as a known risk |
| **PROBE 4** | Git Bash, MSYS and native consumers, `MSYS2_ARG_CONV_EXCL` unset and set | ruling ⑭ | the forward-slash spelling through `paste_as` |
| **PROBE 5** | macOS drop destination: own view / upstream / replacement | T-PASTE-3's macOS half | **T-PASTE-3 ships Windows only** and says so in both languages |
| **PROBE 6** | what Safari, Chrome and Firefox offer for a dragged image, by version | ruling ㉑'s browser row | the registered types decide; a promise-only drag is refused visibly |
| **PROBE 7** | real `CF_DIBV5` payloads: headers, masks, actual alpha | ruling ㉔'s supported layouts | refuse the layout |
| **PROBE 8** | the **distribution** of delayed-render latency for common Windows sources | whether the transaction must move off the event loop (§9.2's debt) | the transaction stays on the loop, unbounded, with the `ClipboardRead` station |
| **PROBE 9** | **whether any TCC prompt appears on the paths this lane writes** | §4.1 | refusal, never a fallback copy elsewhere |
| **PROBE 10** | nushell, pinned version: maximum raw-string fence width, and whether a raw string is accepted in argument position | the `Nushell` arm's totality | refuse anything past fence width 1 |
| **PROBE 11** | `cygwin1.dll` / `msys-2.0.dll` beside the program, on a real Cygwin and a real MSYS2 | ruling ⑫a's detection | a row with neither DLL keeps today's answer |

**PROBE 9 was narrowed.** Its first half — can a child of a Folio row read a file
under `$TMPDIR` — is not an open question: the Mac package is unsandboxed
(`packaging/macos/entitlements.plist:20`), children inherit `TMPDIR`, and this
program **already depends on that answer** for its per-run socket
(`PRIVACY.md`, "Elsewhere"). Presenting it as unknown would have hidden a
dependency the design already has. What is open is the TCC half.

**An unrun probe is not evidence either way.** Nothing in this document may be
implemented as though a probe had answered — but every probe now has a shipped
behaviour if it is not answered, which is the difference between a bounded probe
and an open end.

### 9.2 Named debts

TIFF on macOS, with its feature, lockfile and notices work. **Reading** file
promises on both platforms. An OS drop on the tab strip. Real WSL mount facts in
place of the default-mount assumption. A `nu` or `fish` row reached through
`wsl.exe -e` without an override. **The clipboard transaction on a worker thread
of its own**, with the message-pumping owner window that Win32 requires of the
calling thread (`lib.rs:5838`) — opened if PROBE 8's distribution says the loop
cannot carry it. **The owner's ruling on §2.5's derivation**, and whatever
follows from it in the detector.

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
* `RegisterDragDrop` and *Clipboard operations* (delayed rendering) on MSDN.
* kitty's **OSC 5522**; ghostty-org/ghostty discussion **#10517**, which is about
  image paste **over SSH**; and an unverified WezTerm community recipe writing
  into `/tmp/wezterm-clipboard-images/`.
* The Nushell book's raw-string section, and `docs/plans/shell-matrix-2026-09-07.md:365`
  — the owner's 2026-09-07 ruling on `printed_path_namespace`, which §2.5 keeps.
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

| # | Verdict | What changed |
| --- | --- | --- |
| **1** | accepted | §2.3, §2.4. The universal safe set is **gone**: there is no bare form, every grammar always quotes. POSIX backslash is no longer assumed inert; PowerShell's Unicode quote characters are doubled and **PROBE 2** must prove the doubling before the arm ships, with refusal as the fallback. Non-ASCII whitespace is no longer assumed lexically inert. §6.1 asserts argument identity by running a lexer over the output, and the table carries Unicode delimiter attacks beside CJK and emoji. |
| **2** | accepted | §2.4 is a new section: a representability gate ahead of quoting. `to_str()` failure, any C0/C1 control, DEL or line terminator → **visible refusal**; `to_string_lossy` is banned including in the repaired K144; the false sentence "a quoted path has no control character and no newline" is struck (§5.1); the invalid-UTF-8 acceptance promise is amended from "pastes correctly" to "refuses visibly and names the file" (§2.4, §6.2). Ordinary text sanitisation is untouched. |
| **3** | accepted | §2.3's `cmd` row rewritten. Interpreter and CRT consumer separated; `2N` trailing backslashes, not one extra; the `"D:\"` description corrected (a literal quote after the consumed backslash, not `D:`); builtins named as not following CRT rules, with §6.2 asserting the opened file for both; `%` **refused**; `!` refused when the row's own args turn delayed expansion on; `^` explicitly not escaped; "universal cmd safety" dropped. |
| **4** | accepted | §2.3 gains a `Fish` arm (`\`→`\\`, `'`→`\'`) and a `Nushell` arm (raw string with a growing fence). The "`'\''` happens to be correct in fish" reasoning is withdrawn as reasoning from one sequence to an encoder; the "nu cannot spell an apostrophe path" claim is withdrawn as false; the "every shell an account can be set to" claim is narrowed. §6.1 tests combined backslash-and-apostrophe cases. |
| **5** | accepted | §2.2 ruling ⑦ states a **spawn-time default**, not a recipient guarantee, and says in as many words that the foreground program is not inferred from the screen. Derivation is from the **resolved** launch and keeps `PowerShellSeven` (ruling ⑧). Ruling ⑨ adds a `profiles.json` `"paste_as"` override for wrappers and unknowns. `shell_integration.rs:248`'s WSL login-shell `exec` is cited as the reason a WSL row's reader is unknowable. §6.1 tests `wsl.exe -e nu` and an override row, not only builtin ids. |
| **6** | accepted, then **superseded by second-review finding 3** | The first revision derived the namespace from the program and re-pointed the detector. That reversed the owner's 2026-09-07 ruling without citing it, so §2.5 now **keeps the standing derivation** and marks the change **OWNER RULING NEEDED**. The integration-off Git Bash gap this row was answering is recorded there as the argument *for* changing it, not as a change already made. |
| **7** | accepted | §2.5 ruling ⑬ rewritten: lexical inverse separated from mount facts; the unmounted-drive case is a disclosed **default-mount assumption**, not a translation; `\\wsl$\` read; drive case normalised; the `\\wsl.localhost\…\mnt\d\x` non-identity named; UNC and foreign-distro fallbacks stated as having no inverse; `~` never emitted because a quoted one does not expand. §6.1 splits the round trip into a **supported-identity** suite and a **known-non-identity** suite. `$(wslpath …)` remains forbidden (red line 12). |
| **8** | accepted | §2.5 ruling ⑭: MSYS2's own argument conversion is named, `MSYS2_ARG_CONV_EXCL` is named, quotes do not disable it, §6.2 carries MSYS and native consumers with the variable unset and set (**PROBE 4**), the forward-slash Windows spelling is the documented per-row fallback through the override, and red line 12 forbids changing the reader's environment. |
| **9** | accepted — the earlier ruling is **reversed** | §2.3. Codex's `normalize_pasted_path` at revision `a8964cb1…` was read: it strips one surrounding quote pair, tries a Windows recogniser, then requires exactly one shlex token — so a **bare spaced POSIX path attaches nothing** and the quoted one works. `Agent` is now the POSIX single-quoted form on both platforms. The "quotes make agents seek apostrophe-prefixed names" claim is withdrawn. Prose references and automatic attachments are separated; Claude's prose/`@` and Copilot's `@` are described as documented rather than as a seven-agent grammar; several paths in one insertion are stated **not** to be several attachments (Codex returns `None`); delimiters are preserved. **PROBE 3** must measure spaces, apostrophes and multiples per agent, with versions. |
| **10** | accepted | §1.2 ruling ② now states the order as a **preference with a named loss**, not a deduction about intent; the "screenshot tools offer no text at all" claim is struck. The losing case gets a **Paste picture** verb (menu row plus bindable command) rather than a second setting. §6.2 carries the versioned source/gesture/format/result matrix (**PROBE 1**). No ShareX default is claimed — the product is not cited at all, only the mixed-workflow *case*. The files-versus-text order is stated once, in ruling ①, and the contradictory sentence in the setting's rationale is rewritten (§4.6). |
| **11** | accepted | §1.2 ruling ③ defines **Absent / Present / Unreadable**, with empty-`HDROP` as absent and empty text as present, and `Unreadable` stopping the ladder; encoding-level fallback is allowed only *within* the picture rung. Ruling ④ adds a coherent snapshot with `GetClipboardSequenceNumber` / `changeCount` before and after. §1.3 extracts a shared, log-free file-URL decoder beneath `paths_on` rather than reusing the service helper. §1.1 corrects the `Result`/`Option` descriptions. WSL/RDP bridged formats are covered by PROBE 1's "record what is advertised"; no file identity is inferred from arbitrary text. |
| **12** | accepted | §5.2 is a new section: the promise is limited to a **fresh argument boundary**, and inside-token, mid-quote and wrapped-column-zero behaviour is documented as outside it. zsh `bracketed-paste-magic`, PSReadLine and `$PSNativeCommandArgumentPassing` are named as later stages; §6.2 asserts the argument received and the file opened, on 5.1 and 7, default and custom zsh, bracketing on and off. Bracketed inheritance is kept and is explicitly not disabled to bypass hooks (§5.1). |
| **13** | accepted | §4.2 ruling ㉖. A job carries window, tab, leaf, **session incarnation** and a globally unique **request sequence** (`CONVENTIONS.md:154`), revalidates target, modal gate and setting at completion, cancels on ownership or setting change and deletes its file through the owned path. Ordering is removed as a problem by ruling ㉕'s one-in-flight rule. The same applies to delayed drops. |
| **14** | accepted | §3.1 rebuilt. The first draft's "winit's content-view class" is **corrected**: `NSDraggingDestination` is implemented on `WindowDelegate` (`window_delegate.rs:367`) and registration is on the window (`:666`); the methods are entered/prepare/perform/conclude/exited with **no `draggingUpdated:`**, which is why winit's route cannot feed a following highlight. **PROBE 5** now prefers an application-owned destination `NSView` first and a narrow upstream extension second, with class-wide replacement last and only with a written per-window lifetime contract; it must verify hit testing, responder/IME, the `CAMetalLayer` and the web panes, and specify the full method set, native ABI return types, teardown and the AppKit-to-physical conversion. |
| **15** | accepted | §3.1's Windows half. `window.rs:1167` gates `OleInitialize` **and** registration together, so Folio owns the STA initialisation and its balance; a target is registered on **every** HWND, with `DRAGDROP_E_ALREADYREGISTERED` named as the failure of a missed constructor; teardown is written against `event_loop.rs:1262`'s unconditional `RevokeDragDrop`; payloads are copied before release; effects are `COPY`/`NONE` only, never `MOVE`; coordinates, DPI, source masks, multi-window teardown, cancellation and re-entrancy are specified. |
| **16** | accepted | §3.2. The strip row is **removed from the table** and described accurately: `row_verb`'s strip arm is unreachable (`main.rs:28995`) and the strip's verbs live on `row_strip_landing` (`:29555`) and its two commits, which are untouched; only an *OS* drop on the strip is refused, at the strip's own routing. Ruling ⑱ defines batch admission — many on a terminal centre, one-item verbs refuse a multi-item drop **while hovering**, mixed kinds, a 64-item cap, partial failure. Ruling ⑲ makes drop insertion target-specific and focus-free while **preserving K144's focus move** (`main.rs:76921`), which the first draft misdescribed. |
| **17** | accepted; the promise half **superseded by second-review finding 2** | §3.4 ruling ㉑ gives drops their own admitted-type matrix: raw picture types are registered explicitly, picture drops on preview and edge are defined, and decoders and insertion are shared. The unconditional Safari claim is **withdrawn** and replaced by **PROBE 6**. The first revision left promises *unregistered* while still promising a visible refusal; they are now registered **only so that the refusal can be drawn**. |
| **18** | accepted | §4.2. The synthesised `BITMAPFILEHEADER` is **withdrawn** in favour of `image`'s audited `BmpDecoder::new_without_file_header` (`decoder.rs:534`, written for `CF_DIB`). **TIFF is removed from 0.4.1** and becomes a named debt with its feature, lockfile and notices work, since `tiff` is a separate feature (`image-0.25.10/Cargo.toml:120`) the workspace does not enable. Ruling ㉔ makes alpha a property of compression and mask, not of the format id, covers premultiplication and undefined legacy alpha, refuses unsupported layouts, and adds known-pixel fixtures and **PROBE 7**. |
| **19** | accepted | §4.2 ruling ㉕. Native length checked **before** copying (`GlobalSize`), with the macOS residue stated; dimensions checked before allocation; the cap expressed in decode bytes rather than an RGBA8 multiplication; a 512 MiB aggregate directory quota with oldest-first eviction; **one job in flight**, a second refused; write/flush/close failures delete the partial file; disk pressure refuses; full-decode validation with original-byte write replaces "header check"; first-frame policy stated; **PROBE 8** gives acquisition a measured latency contract. |
| **20** | accepted | §4.1 ruling ㉓. The socket precedent is replaced by the **directory** precedent `instance.rs:360`, including **repair of an existing mode**; a same-owner world-readable directory no longer passes; both Folio-made levels are vetted, so an intermediate link cannot redirect a non-link leaf; vetting runs **before every operation**, not once per run; files are owner-only; operations are anchored to a verified handle with no-follow on Unix, with the Windows residue stated; Windows gets reparse-point refusal and an owner-only DACL as the uid counterpart; temp discovery is `std::env::temp_dir()` and macOS's `/var` alias is explicitly not rejected; failures are closed and sanitised. |
| **21** | accepted | §4.3 ruling ㉗. The "seven days exceeds any session" and "preserves history" claims are **struck**. Retention is best-effort with a real maximum from the quota; the sweep runs at startup **and hourly**; the system may remove files sooner; the exact owned-name grammar excludes `clip-family.png`; age is mtime; active writes are excluded; failures are reported once. A path in history is explicitly not a promise the file exists, and how to keep a picture permanently is stated. The text says the sweep is not clipboard watching. |
| **22** | accepted | §4.6 ruling ㉙ makes the switch cover **clipboard and drop** pictures and cancel pending jobs; inserting an existing file's path is explicitly outside it. Red line 3 carries the **named storage exception**; red line 1 is narrowed to feature-initiated requests, with UNC and network-volume I/O described as the reader's own gesture; §7.3 separates `BT_PTY_DUMP` (opt-in, already disclosed at `PRIVACY.md:186`) from this feature's storage. §4.5 discloses drops, original PNG metadata, cleanup limits, redirected `%TEMP%`, and that deleting the directory does not undo history, agent records or recipient copies. No sandbox blocker is invented — `entitlements.plist:20` says the package is unsandboxed — and child access and TCC become **PROBE 9** with refusal, not a fallback copy, if it fails. |
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

| # | Verdict | What changed |
| --- | --- | --- |
| **1** | accepted | §1.2 ruling ④ rewritten. The transaction is **two steps** — survey the advertised type list (which renders nothing), then fetch **one** rung — so the exposure is no longer multiplied by five. The thread is named (winit's event loop, `lib.rs:5927`, `main.rs:96290`), a delayed render is stated to be **unbounded** with no timeout and no cancellation, and it gets a new `hang_watch` station **`ClipboardRead`** beside `PtyResize` / `WebPage` / `WebRetire`. The word "bounded" is gone. **PROBE 8 is redefined** from "measure a bound" to "measure the distribution and decide whether the transaction must move off the loop", and the worker-thread variant is written down in §9.2 with its real cost — a second message-pumping clipboard owner window, because `OpenClipboard` wants a window of the *calling* thread (`lib.rs:5838`). |
| **2** | accepted | §1.2 gains a fifth variant, **`ClipboardPayload::Refused(UnsupportedKind)`**, set when a kind is advertised and deliberately not read; only it raises a toast, `Nothing` stays silent, so `Ctrl+V` on an empty clipboard says nothing. On the drop side the decision is taken and stated: **promise types are registered after all**, purely so the destination is offered the drag and can trace the refusal box, with `performDragOperation:` never entered and `receivePromisedFiles` never called. §3.4's matrix carries the row. |
| **3** | accepted | §2.5's ruling ⑫ is **withdrawn into a proposal**. The standing 2026-09-07 derivation is kept and cited (`docs/plans/shell-matrix-2026-09-07.md:365`, `profiles.rs:3193`), T-PASTE-1 calls `printed_path_namespace` as it stands, the detector is **not** re-pointed, and the change is marked **OWNER RULING NEEDED** with both sides written out. The document's own scope paragraph now names that marker. **Cygwin** gets ruling ⑫a — a `Cygwin { home }` arm spelling `/cygdrive/<letter>`, detected by the runtime DLL beside the program, **PROBE 11** — and the section states that the Cygwin hole exists under the **standing** rule too, in the detector, so it is not something the proposal introduced. MinGW is explicitly out, with the reason: it is separator style, not a namespace. |
| **4** | **accepted in part, declined in part — on the parser's own text** | §2.3's `Agent` arm rewritten with the four branches quoted and the revision pinned. **Declined:** the POSIX half is not wrong. `normalize_windows_path` returns `None` unless the string starts with a drive letter or `\\`, so for `'/Users/ann/John'\''s Papers/a.png'` the URL and Windows branches both decline, control reaches `shlex::Shlex::new(pasted)` over the **original**, and one token comes back carrying the true name. The quote-strip branch does not "win" for a path the recogniser rejects. **Accepted:** the Windows half is wrong and worse than the first revision admitted — `'C:\a'\''b.png'` strips to something that *does* start with a drive letter, so the recogniser returns `C:\a'\''b.png` verbatim. That is not a suspicion to probe; it is what the code does. **The `Agent` literal is now double-quoted for a Windows path and POSIX single-quoted for a POSIX path**, which both branches read correctly and which is additionally *Copy as path*'s own spelling. PROBE 3 keeps the apostrophe row on both platforms as the check on this reading. |
| **5** | accepted | §2.2 ruling ⑩ and §2.3. The **encoder** and the **interpreter** are separated: `Cmd` is the CRT-quoting encoder, and `%` / `!` refusal is a property of the **`cmd.exe` row specifically**. A `python` or `node` row on Windows gets CRT quoting with no expansion refusals, and §6.2 gains a row asserting that `D:\Data\100%\report.csv` **works** there rather than refusing. |
| **6** | accepted | §1.2. The picture rung is fetched as an **ordered list** — every advertised encoding copied inside the one transaction — so `bt-app` can fall to `CF_DIB` after a failed PNG decode without a second snapshot. The earlier within-rung fallback was unreachable exactly as the finding says. `Unreadable` is narrowed to an **acquisition** verdict and "malformed content" is struck from its definition, since the layer that reports it does not decode. The list's total size sits inside ruling ㉕'s bound, and a decode failure past a truncated list is terminal and says so. |
| **7** | accepted | §4.2 ruling ㉕ and §4.3. The quota gains a **24-hour eviction floor** — never evict a file younger than that, refuse the write instead — so eviction cannot reach a path sitting in an unsubmitted command line. The read-evict-write sequence is **serialised across windows and processes** by an owner-only `clipboard/.lock` taken with `create_new`, with a 10-second staleness rule; a lock it cannot take means the write proceeds without evicting and the quota may overshoot, stated rather than pretended away. §4.3's disclaimer is widened from history to "a path on your screen", naming the unsubmitted line. |
| **8** | accepted | Red line 2 is rewritten as **one rule with one positively stated permission** — the *type list* may be read when the terminal's right-click menu is raised; the *content* only on a paste, a Paste picture or a drop. A failed type-list read **disables** the row rather than hiding it. **Paste picture** is moved to the terminal context menu only and explicitly kept out of the macOS menu bar, because `validateMenuItem:` fires on menu opening and key-equivalent dispatch at moments the application does not schedule (§5.3). §4.6 states that with the switch off the row is **disabled**, not enabled-then-apologising. |
| **9** | accepted | §4.1 takes `instance.rs:331`'s **naming** as well as its vetting: on macOS the directory is `folio-<uid>/clipboard` under `$TMPDIR` **or `/tmp`** — the `/tmp` fallback is now stated, where the earlier draft asserted `$TMPDIR` and called it per-account. The squat-then-fail-forever path is named as the reason. Windows keeps `%TEMP%\Folio\clipboard`, whose temp is already per-account. |
| **10** | accepted | §8. T-PASTE-1's scope **keeps an i18n row** (it ships six refusal toasts), and first-ledger row 23 is corrected in place rather than left to disagree. The PROBE 3 gate is restated as what it is: Codex is the measured recipient, the six others **inherit** its spelling, and the gate is *measure, or record the inheritance as a known risk* — not a refusal the grammar table does not carry. |
| **11** | accepted | §2.3's nushell paragraph. **One spelling** — `r` + *n* `#` + `'…'` + *n* `#` — replaces the two the section carried; the growth rule is stated; the Nushell book's raw-string sentence is quoted in §9.3; and the totality claim is **withdrawn** as the same error just admitted for fish. **PROBE 10** covers the maximum fence width and, separately, whether a raw string is accepted in *argument* position, which the book does not document. Unprobed, a `nu` pane refuses anything needing a wider fence than one. |
| **12** | accepted | §3.1's Windows half names winit's **third** COM owner: `platform/windows.rs:493`'s own warning that "winit may still attempt to initialize COM API regardless of this option", and `window.rs:1432`'s `thread_local! { COM_INITIALIZED }` with its `CoUninitialize` in a thread-local destructor. The ordering rule is written: Folio's `OleInitialize` before any window, and its `OleUninitialize` before winit's destructor or deliberately not at all. §6.2 gains a **fullscreen transition with a target registered**. |
| **13** | accepted | §3.4's matrix now carries §3.2's **four landings plus `Placeholder`** (a real `SeatKind`, `crates/bt-layout/src/tree.rs:26`) and names every cell with §3.2's own verb — `Insert` / `Retarget` / `Split` / `Refused` — instead of paraphrasing. The rim cell says `Split`, not "open the one file". §6.2 gains a `Placeholder` drop row. |
| **14** | accepted | PROBE 1's own list gains the three bridged-clipboard rows — a file copied in a WSLg GUI file manager, `clip.exe` text from a WSL shell, and a file and an image copied inside an RDP session — so the ledger's claim and the matrix agree. |
| **15** | accepted | §4.1. The representability gate runs on the **directory**, once, as part of vetting, and refuses the whole lane with one toast before any picture is written. And a job that somehow completes with an unspellable path **deletes its own file through the owned path**, as a cancellation does; §6.1 asserts it. |
| **16** | accepted | §4.3. The sweep runs on **the picture lane's worker**, never on the event loop — it is a listing plus `symlink_metadata` plus unlinks against a `%TEMP%` the design itself says may be a network share. It shares the worker with the encode, so it is never concurrent with this process's own write, and anything of it that touches the loop shares ruling ④'s station. §5's map carries both the worker and the station. |
| **17** | accepted | §8. PROBE 5's all-fail outcome is a **shipped behaviour**: T-PASTE-3 ships Windows only, macOS drops stay as they are today, and the README and `docs/features.md` say so in both languages. §9.1's table gives every probe an "if it is not run" column for the same reason. |
| **18** | accepted | (a) PROBE 2's fallback is now a **concrete set** — `U+0027`, `U+2018`, `U+2019`, `U+201A`, `U+201B` — refused by an unprobed build, with the probe able only to narrow it; a code point found outside the list stops the ticket. (b) **PROBE 1 is retitled** a fixture matrix to record: it blocks the ticket's shipping, not a ruling. (c) **PROBE 9 is narrowed** to the TCC half, and the child-read half is written down as a fact the per-run socket already depends on. |
| **19** | accepted | The scope paragraph points at **§9.1**, and says eleven probes, and names the one owner ruling. |
| **20** | accepted | §2.4 quotes all five arms of `sanitize_paste` (`input.rs:718`–`:728`) exactly, including that `'\r'` keeps the CR and swallows a following LF, and that **TAB is kept, not dropped** — and then says so as the strongest of the arguments for having a gate at all, since a kept TAB would otherwise survive into the argument. |
| **21** | accepted | §3.1 names all three discard sites — `DragEnter` `:83`, `DragOver` `:109`, `Drop` `:137` — and says the entry callback carries no coordinate either. |
| **22** | accepted | First-ledger row 24 now says the sentence carrying the wrong anchor was **removed** by ruling ⑦, and records `profiles.rs:1512` as the fact rather than as a correction a reader could go and check in the body. |
| **23** | accepted | §4.1 stops naming `GetTempPath2W` and says what the design actually needs — a per-account temp directory the platform chose. |

**One partial decline, and it is the only one across both reviews.** Finding 4's
POSIX half does not hold: `normalize_windows_path` requires a drive letter or a
UNC prefix, so a quoted POSIX path reaches the shlex branch and comes back
correct. The finding's Windows half was right, and more sharply than the first
revision had it, so the arm changed there. Everything else in both reviews is
accepted.
