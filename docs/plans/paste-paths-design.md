# A file on the clipboard, a file on the pointer, and a picture with no name

Design for 0.4.1 — GitHub issues #1 and #2. 2026-09-15, branch
`docs/paste-paths-design` off `main` at `76ca0788`. **Docs only**: nothing here
is built, no crate is touched, and every claim about this codebase carries a
`path:line` so that the ticket which implements it can check the claim before it
trusts it.

Two requests from one outside reader, and they are one feature:

* **#1** — a file copied in Explorer or Finder should paste into a pane as its
  path, and a file dragged onto the window should insert its path.
* **#2** — when the clipboard holds a picture, pasting should write it to a
  temporary file and paste *that* file's path, which is how a person hands a
  screenshot to a command-line agent.

They are one feature because after the second sentence of #2 there is a file on
the disk and a path to put at a prompt, which is #1. Everything below is
arranged around that: **one ladder that turns whatever the clipboard or the
pointer is carrying into a list of paths, and one function that spells a path
for the program standing in the pane.**

---

## 0. The short answer, before the reasons

**Five rulings carry the whole design.**

1. **The clipboard is asked in a fixed order — files, then text, then a
   picture — and the first rung that answers wins.** Text beats a picture:
   every source that offers both means the text, and the only sources #2 is
   about (a screenshot tool, a snip) offer no text at all.
2. **A path is spelled for the program the pane's profile starts, and the
   profile already knows which program that is.** No new setting. PowerShell
   gets `'…'` with `''` for a quote; `cmd` gets `"…"`; bash, zsh, sh and Git
   Bash get `'…'` with `'\''` for a quote; a WSL pane gets `/mnt/d/…` and a Git
   Bash pane `/d/…`, by the same mount rule this window already reads in the
   other direction; and an **agent** pane gets the path bare, because Claude
   Code reads a line of prose, not a command line.
3. **A drop is a gesture of this window's drag system, not a winit event.**
   winit 0.30.13 throws the drop point away on Windows and reads macOS paths
   through an `NSString`, so `bt-platform` grows a drop door of its own and the
   drop lands in the verb table that already decides what a dragged row means
   (`row_verb`) — which gains exactly one row: **a file on a terminal pane's
   centre inserts its path.**
4. **A picture becomes a PNG in `…/Folio/clipboard/`, named `clip-<stamp>.png`,
   never overwriting, swept at startup at seven days**, and from that point it
   is an ordinary file taking the ordinary quoting. It is the one part of this
   feature that writes to the disk, so it is the one part with a switch and a
   paragraph in `PRIVACY.md`.
5. **The gesture is plain `Ctrl+V` / `⌘V` / `Shift+Insert`.** No chord, no mode.
   What is on the clipboard is the clipboard's business; the reader's gesture is
   *paste*.

---

## 1. What the clipboard is asked for, and in what order

### 1.1 What is there today

`bt_platform::clipboard_text` is two implementations of one sentence. On Windows
(`crates/bt-platform/src/lib.rs:5925`) it opens the clipboard through the owner
window M1-9 gave it, asks `IsClipboardFormatAvailable(CF_UNICODETEXT)`, locks
the handle and copies UTF-16 out. On macOS (`lib.rs:11319`) it is
`NSPasteboard::generalPasteboard` and `stringForType(NSPasteboardTypeString)`,
and that file's own header already names the gap this design fills: a pasteboard
holding only a picture answers `None`, "the same *no Unicode text* the Windows
arm answers when `CF_UNICODETEXT` is absent", and **"a richer pasteboard is a
product decision nobody has taken."** This is that decision.

One reusable thing is already written. `macos_services::paths_on`
(`crates/bt-platform/src/macos_services.rs:181`) reads local file paths off a
pasteboard for the Finder Service — `readObjectsForClasses`, `absoluteString`
rather than `-[NSURL path]` (because a path is bytes, not text), the reader's
own item order preserved because "a selection of three folders is three tabs and
the reader chose which is first", and a non-file URL dropped with a line instead
of failing the lot. The file rung on macOS is that function with a second
caller.

### 1.2 Ruling — the rungs, and their order

**One door, `bt_platform::clipboard_payload()`, answering one enum:**

```text
ClipboardPayload::Files(Vec<PathBuf>)      // the source said: these are files
ClipboardPayload::Text(String)             // what clipboard_text() answers today
ClipboardPayload::Picture(PictureBytes)    // Png | Dib | Tiff, and the bytes
ClipboardPayload::Nothing
```

| Rung | Windows | macOS |
| --- | --- | --- |
| 1 — files | `CF_HDROP` | `readObjectsForClasses:[NSURL]`, file URLs only |
| 2 — text | `CF_UNICODETEXT` | `NSPasteboardTypeString` |
| 3 — picture | the registered `"PNG"` format, then `CF_DIBV5`, then `CF_DIB` | `public.png`, then `public.tiff` |

**Ruling ①: files win over everything.** A file list is the one format whose
meaning is not in doubt — the source did not offer a *representation* of
something, it named objects on the disk. And on macOS the order is not a
preference but a correctness requirement: a Finder copy also puts the file's
**leaf name** on the pasteboard as plain text, so a text-first ladder would
paste `report.pdf` for a file the reader selected at
`/Users/ann/Papers/report.pdf`. Asking for URLs first is the only order that
answers the gesture.

**Ruling ②: text wins over a picture.** This is the question the ticket asked to
settle, and the argument runs in one direction only. Every source that puts a
picture on the clipboard *beside* text means the text — a spreadsheet range, a
browser selection, an IDE, a word processor all offer a bitmap as a courtesy to
a program that cannot take text. Every source #2 is about — Snipping Tool,
`⌃⇧⌘4`, a screenshot utility — offers **no text at all**. So taking the picture
only when there is no text costs the picture case nothing, while taking the
picture when there is text costs the text case everything: a copied sentence
would become a PNG on the disk and a path the reader never asked for.

**Ruling ③: the rungs are tried in order and the first one that answers wins —
there is no merging.** A paste is one gesture and it puts one thing at the
prompt. A clipboard carrying three files and a line of text is a clipboard whose
source said "files", and the text on it is that source's caption.

**Ruling ④: a promise is not read.** Windows `FileGroupDescriptorW` +
`FileContents` (an Outlook attachment), macOS `NSFilesPromisePboardType` and
`com.apple.NSFilePromiseItemMetaData` (a drag out of Photos or Mail) are
deliberately **not** rungs. A promise means *tell me where to put it and I will
write it*, which is a file this window would have to create in a place it chose
— the picture lane's problem with none of the picture lane's guarantees (no
format it can name, no size it can bound, and a write performed by another
process). `paths_on`'s own rule applies: what this window cannot open, it drops
with a line rather than guessing at.

**Ruling ⑤: the old door stays.** `clipboard_text()` is not replaced and not
re-pointed. The Markdown editor's paste (`crates/bt-app/src/main.rs:60428`), the
palette and search fields, the settings text fields (`main.rs:47228`) and the
web address bar are asking for **text**, on purpose, and a field that answered a
copied file with a path would be answering a question nobody asked. The new door
is beside the old one, and only the terminal's paste walks through it.

### 1.3 Cited behaviour

Windows Terminal reads `CF_HDROP` and pastes the path; it did in 1.18, lost it
in 1.19 as a regression, and issue #16627 (PR #16634) is the whole story — which
is worth knowing for a second reason: the feature is quiet enough that it took a
version and a half for anyone to file it, so the acceptance rows below are
written to be *checked* rather than assumed. No terminal in the survey reads a
clipboard **picture** into a file: kitty defines OSC 5522 so the *program* can
ask for typed clipboard data, WezTerm's answer is a user's Lua recipe writing
into `/tmp/wezterm-clipboard-images/`, Ghostty has a request and no
implementation, and the working answer people actually use is a third-party
daemon that watches the clipboard and rewrites it. **Folio will not watch the
clipboard** (§7); it will answer the paste.

---

## 2. One path, spelled for the program standing in the pane

### 2.1 What this window already knows, and the defect it is hiding

There is a path-to-shell-text function in the product today, and it is wrong on
one of the two platforms Folio now ships on. `inserted_path_text`
(`crates/bt-app/src/main.rs:8824`) is what `Insert path into terminal` (K144)
puts at the prompt, and its own doc comment states the reason for its quoting:

> Quoted with `"` rather than `'` because this window's shells are Windows
> shells, where `'` is a literal character to `cmd` and a *different* kind of
> quote to PowerShell — and because a Windows path cannot itself contain a `"`,
> the character is illegal in a file name, so there is nothing to escape.

Every clause of that was true when it was written and the first one stopped
being true at 0.4.0. In a zsh pane on a Mac, `"/Users/ann/$HOME notes/a.txt"`
expands `$HOME`, and a backtick or a backslash in the name is an escape. **K144
is a defect on macOS today**, and this ticket does not add a second quoting
function beside it — it absorbs it. The two rules around the quoting are right
and are kept verbatim: a space in front unless the cell left of the cursor is
already blank (`input_line_needs_a_space_first`, `main.rs:8850`, which reads the
*grid* because the shell will not tell us what the input line holds), and a
space after, always.

The namespace half is already written too, in the other direction.
`PrintedPathNamespace` (`crates/bt-transcript/src/paths.rs:61`) is
`Windows | Msys { home } | Wsl { distro, home }`, built for a pane by
`profiles::printed_path_namespace` (`crates/bt-app/src/profiles.rs:3201`) off
the pair the profile row already carries, and `to_local_path` (`paths.rs:134`)
turns `/mnt/d/Demo` and `/d/Demo` into `D:\Demo` so that a printed path can be
clicked. **What this design needs is that function run backwards.**

### 2.2 Ruling — two axes, two homes, and neither is a new setting

A path becomes text by answering two independent questions, and they are kept
apart because they have different owners:

* **Which spelling** — `D:\Demo\a.txt`, `/mnt/d/Demo/a.txt`, `/d/Demo/a.txt`.
  That is `PrintedPathNamespace`'s question, and the answer goes **on that type**
  as `to_pane_spelling(&self, local: &Path) -> Option<String>`, the inverse of
  `to_local_path` and sitting against it in the same file. One place, two
  directions, and a round-trip test (§6) that makes it impossible for them to
  drift. A second module that knew the `/mnt` rule would be the start of two
  answers to one question, which is the thing `paths.rs`' own header refuses.
* **Which grammar** — how a string becomes one word to the program reading it.
  That is `ShellGrammar` — `PowerShell | Cmd | Posix | Plain` — and it is read
  off the row by `profiles::shell_grammar(index)`, written as
  `derive_integration`'s twin (`profiles.rs:799`): the program's **file stem**,
  lower-cased, matched against a list, plus one check of `AGENT_IDS`
  (`profiles.rs:1746`).

**Ruling ⑥: the grammar comes from the program the row starts, never from the
integration choice.** `served_by` can answer `Integration::None` for a perfectly
ordinary `pwsh` whose owner turned shell integration off, and quoting has
nothing to do with whether a startup script was installed. `derive_integration`
reads the program; so does this.

**Ruling ⑦: no per-profile setting, and Windows Terminal is the reason.** WT
added `pathTranslationStyle` (`none` / `wsl` / `cygwin` / `msys2` / `mingw`,
defaulting to `none` except on WSL profiles) because a WT profile is a command
line and nothing else — it has no idea what it starts, so it has to ask the
reader. A Folio profile row already carries `paths: PathNamespace`, an
integration and a program source, and `printed_path_namespace` already derives a
three-way answer from them for the *reading* direction. Deriving the writing
direction from the same row costs the reader no question. The setting gets added
the day a real row is measured wrong, and not before.

### 2.3 The grammar table

| The row | Grammar | Quote | Escape inside it |
| --- | --- | --- | --- |
| `pwsh`, `winps`, any `powershell`/`pwsh` stem | `PowerShell` | `'…'` | `'` → `''` |
| `cmd` | `Cmd` | `"…"` | nothing to escape; a trailing `\` is doubled |
| `gitbash`, `wsl`, `bash`, `zsh`, `sh`, and every other stem on a Unix host | `Posix` | `'…'` | `'` → `'\''` |
| the seven `AGENT_IDS` rows, and an unheard-of program on Windows | `Plain` | none | none |

**PowerShell takes single quotes.** Inside them PowerShell expands nothing — no
`$`, no backtick, no subexpression — and the only character needing anything is
the quote itself, doubled. Double quotes would expand `$`, and `$` in a Windows
path is not exotic: `$RECYCLE.BIN` is on every volume and `$` is a legal file
name character. This is the same bug WT shipped in the other grammar and fixed
in PR #16214 after issue #15646, where `$hello.txt` dropped into a WSL tab became
a variable.

**`cmd` takes double quotes, and there is nothing to escape** — `"` is illegal
in a Windows file name, which is the one clause of K144's old comment that
survives intact. One addition: **a path ending in `\` has that backslash
doubled** before the closing quote (`"D:\\"`). The C runtime's argument parser —
which is what every program `cmd` starts uses to split its command line — reads
`\"` as a literal quote, so `"D:\"` hands the program `D:` and an unterminated
word; `"D:\\"` hands it `D:\`, and cmd's own builtins read a doubled separator
in a path as one. Correct on both sides of the fork, which is why it is a rule
and not a special case.

**bash, zsh, sh and Git Bash take single quotes with `'\''`** — close the quote,
write an escaped quote, open it again. WT issue #18006 is precisely the bug of
not doing this: `D:\John's Archive` became `'/mnt/d/John's Archive'`, where the
quoting ends at the apostrophe and the space after it is bare. WT's own
`_translatePathInPlace` does the `'\''` escape now, and this design copies the
escape and not the plumbing. The same three characters also happen to be correct
in **fish** — outside quotes fish reads `\'` as a literal quote, so the sequence
concatenates the same three pieces — which is checked rather than assumed,
because `usershell` on a Mac can be pointed at anything. **nushell is the known
exception** and is named here rather than discovered later: a nu single-quoted
string has no escape at all, so a path containing `'` cannot be spelled for it.
A `nu` row gets `Posix` and that one path gets it wrong; adding a `Nushell`
grammar is a ticket for the day somebody runs one.

**An agent row gets the path bare, and this is the most important row in the
table.** `claude`, `codex`, `copilot`, `kimi`, `pi`, `hermes` and `opencode`
(`profiles.rs:1746`) are not shells. Claude Code reads a **line of prose** and
opens the file whose path it finds in it; handed
`'C:\Users\ann\clip-20260915-140233.png'` it looks for a file whose name begins
with an apostrophe. Since handing a picture to an agent is the entire stated
purpose of #2, a design that quoted for an agent would fail at the only thing it
was asked to do. And the derivation is the same shape as everything else here:
the table already knows which rows are agents, and the pane already knows which
row it is.

**The unheard-of row splits by platform, and the split is a fact about the two
tables rather than a preference.** On Windows every shell Folio ships a row for
is in the list above, and a row pointed at something else is a row pointed at a
*program* — the seven agent rows are the shipped examples of exactly that — so
an unheard-of Windows program gets `Plain`. On a Unix host the unheard-of row is
`usershell`, which is the account's own `$SHELL` (`profiles.rs:1541` and the
note above it), and every shell an account can be set to takes POSIX single
quoting — so it gets `Posix`. The cost of being wrong points the same way in
both cases: a bare path handed to a shell is a path the reader can quote by
hand, while a quoted path handed to a program is a path that names nothing.

### 2.4 When a path is quoted at all

**Ruling ⑧: a path is written bare when every one of its characters is safe, and
quoted otherwise.** The safe set is ASCII letters, ASCII digits, and
`_ . - / \ :` — **and every non-ASCII character**, because none of PowerShell,
`cmd`, bash, zsh or fish gives any non-ASCII code point syntactic meaning. That
last clause is why `D:\文档\报告.md` and `~/Pictures/スクショ.png` come out bare
and readable instead of wearing quotes for no reason, which matters on a product
whose second language is Chinese.

Everything else quotes: the space (WT's own threshold), `~` (which expands in
all three grammars — a leading one would be enough, but a `~` elsewhere in a
path is rare enough that the simpler rule costs nothing), `'`, `"`, `$`,
backtick, `%`, `!`, `#`, `&`, `;`, `,`, `*`, `?`, `(`, `)`, `[`, `]`, `{`, `}`,
`<`, `>`, `|`, `^`, `=`, `+`, `@`. The rule is a *set*, not a list of
metacharacters per shell, because a per-shell list is a second copy of each
grammar and the copy that is out of date is the one that bites.

**Two things this window cannot make safe, stated rather than papered over.**
① `cmd` expands `%NAME%` **inside** double quotes and there is no command-line
escape for `%` (the `%%` form is a batch-file rule), so a file called
`%USERPROFILE% backup.txt` pasted into a `cmd` pane will be mangled by `cmd` and
no quoting fixes it. ② PowerShell treats `[` and `]` as wildcard characters in
the path parameters of most cmdlets even inside single quotes, so
`Get-Content 'C:\a[1].txt'` needs `-LiteralPath` and the reader has to supply
it. Both are the shell's grammar and not this window's bug; the document names
them so that a report about either can be answered in one line.

### 2.5 The spelling, per pane

**Ruling ⑨: the mount rule, not `wslpath`.** A WSL pane is handed
`/mnt/d/Demo/a.txt`, derived by the inverse of `drive_mount_to_local_path` — the
function `to_local_path` already uses — and a Git Bash pane is handed
`/d/Demo/a.txt` by the MSYS arm of the same. `wslpath` is refused for three
reasons and the first is enough: running it means spawning a process on every
paste, and this window's paste is measured in the same latency budget as a
keystroke. The second is that the alternative — writing `$(wslpath …)` into the
reader's command line — puts a command in their history that they did not type.
The third is that it would be a **second** source of truth for a rule this
codebase already implements in one place, and `paths.rs`' own header is about
not having two.

The known limit is the same limit the reading direction already lives with, and
is therefore not a new belief: a distribution whose `/etc/wsl.conf` moves
`automount.root`, or an MSYS installation with a custom mount table, is spelled
wrong — **in both directions, identically**. One wrong answer that agrees with
itself is better than two that disagree, and a reader who has moved their
automount root sees the same spelling in a printed path they click and in a path
they paste.

**Ruling ⑩: a distribution-internal path goes back through the share.** A file
at `\\wsl.localhost\Ubuntu-24.04\home\ann\x` dropped into a pane of that same
distribution is pasted as `/home/ann/x`, which is `distro_path_to_local_path`
run backwards and the exact mirror of the 2026-09-07 ruling in `DESIGN.md`
§7.30. A share naming a *different* distribution than the pane's is not
translated.

**Ruling ⑪: a path with no translation is pasted in this machine's spelling,
quoted.** A UNC path (`\\server\share\x`), a drive a distribution has not
mounted, an MSYS path outside the drive mounts. `to_local_path` refuses rather
than guesses, and that is right for *reading*, because a wrong underline points
at a file nobody named; here refusing means pasting **nothing**, which is worse
than pasting a path the shell cannot open. The reader sees the path they
dragged, quoted so it is inert, and decides what to do with it.

### 2.6 Several files

**Ruling ⑫: space-separated, each spelled and quoted on its own, in the order
the source gave them.** The order is load-bearing and is already ruled so —
`paths_on`'s header says a reader's selection has an order and the pasteboard
keeps it, and `CF_HDROP` keeps Explorer's. K144's leading and trailing space
rules apply to the whole run rather than per path: one space in front if the
cell left of the cursor is not blank, one space at the end.

`cat 'a b.txt' 'c d.txt' ` is what a person would have typed, and it is the same
sentence in an agent pane without the quotes.

---

## 3. The pointer's half: drag and drop

### 3.1 Nothing handles a drop today, and winit's events cannot carry one

`DroppedFile`, `HoveredFile` and `HoveredFileCancelled` appear nowhere in this
workspace — the grep is empty — and `with_drag_and_drop` is never called, so
winit's Windows drop target is registered by default and its events are thrown
away. Turning them on is the obvious first move, and it does not survive reading
the backend:

* **The drop point is discarded.**
  `winit-0.30.13/src/platform_impl/windows/drop_handler.rs` takes
  `_pt: *const POINTL` in both `DragOver` and `Drop` and uses neither.
  `DroppedFile(PathBuf)` carries a path and nothing else. A window that draws
  every pane itself cannot route a drop it cannot locate.
* **`DragOver` reaches the application not at all.** `HoveredFile` fires once,
  from `DragEnter`. There is no event while the hand moves across the window, so
  there is nothing to drive a landing highlight with — and a highlight is not
  optional here (§3.3).
* **A multi-file drop arrives as N events with no batch marker.** Both backends
  loop over the file names sending one event each. Ten files are ten
  `DroppedFile`s, and nothing says where the tenth is.
* **macOS reads the paths through an `NSString`.**
  `platform_impl/macos/window_delegate.rs:369-429` uses the deprecated
  `NSFilenamesPboardType` with `propertyListForType`, which is exactly the round
  trip `macos_files.rs:53` forbids at length — "a name that is not UTF-8 comes
  back as a *different name*, with `U+FFFD` where its bytes were, and the file
  that then opens is not the file the reader pointed at". A file dragged off an
  SMB or exFAT volume is the case that file is written about.

**Ruling ⑬: `bt-platform` grows a drop door of its own, and winit's is switched
off.** On Windows: `with_drag_and_drop(false)` on the window attributes and our
own `IDropTarget` via `RegisterDragDrop`, which hands us the screen point in
`DragEnter`/`DragOver`/`Drop`, the whole `IDataObject` — so the picture rung of
§1 works for a drag exactly as it does for a paste — the full file list in one
call, and control of the `DROPEFFECT` the cursor shows. On macOS: the three
dragging selectors on winit's content-view class, re-registered for
`NSPasteboardTypeFileURL` and reading with `readObjectsForClasses`, which is
`paths_on` again, with `draggingLocation` for the point.

This is the largest single piece of work in the feature, and it is why drag is
its own ticket. Two honesties about it.

First, the macOS half is **replacement, not addition**. `class_addMethod`
answers no for a selector the class already implements, and winit's view
implements all three, so this is `class_replaceMethod` — a different and heavier
thing than the M3-1 route `macos_app.rs` established for the *delegate*, whose
header is careful to say it only adds selectors winit leaves null. The
difference is defensible — a view carries no `is_kind_of` assertion the way
`NSApp.delegate` does, and winit's implementations only queue events nothing
consumes — but it is a claim about another crate's internals, and **T-PASTE-3
opens with a Mac probe** in the shape of `docs/plans/port/probe-x*.md`, not with
an implementation.

Second, the cheap alternative was considered and rejected. Reading
`bt_platform::pointer_position()` — which exists on both platforms,
`lib.rs:7280` and `macos_impl.rs:613` — when a `DroppedFile` arrives would give
a point that is *probably* right; and on macOS, where winit **queues** the event
rather than sending it, probably-right is a guess about where the hand was. This
product's standing rule on marks is that pointing at the wrong thing is worse
than not pointing (`DESIGN.md` §7.1.5k), and a drop that lands in the wrong pane
types a path into the wrong shell.

### 3.2 Ruling — the drop joins the verb table that already exists

There is a table. `row_verb` (`crates/bt-app/src/main.rs`, called from
`commit_layout_drop` at `:88732`) decides what a **row dragged out of the files
column** means when it lands, and it reads three things: the payload's kind, the
landing, and the target pane's kind.

| Landing | File | Folder |
| --- | --- | --- |
| root rim / seat **edge** | `Split` — a new pane holding it | `Split` — a new column rooted there |
| **centre** of a Preview pane | `Retarget` — open it there | `Refused` |
| **centre** of a Files column | `Refused` | `Retarget` — re-root the column |
| **centre** of a Terminal pane | `Refused` → **`Insert`** | `Refused` → **`Insert`** |
| the tab strip | `Refused` | `Refused` |

**Ruling ⑭: an OS drop goes through this same table, and the table gains one
row — a file or a folder on a terminal pane's centre inserts its path.** Both
carriers, one table. Three reasons:

* **The rectangle must not have two answers.** A file dropped on the middle of a
  shell pane has to mean one thing whether it came from Explorer or from Folio's
  own column. Two carriers with two answers over one rectangle is the shape this
  product argues against wherever it has met it — the same object, two answers,
  and what differs is not the object but where it was pointed at.
* **The cell being changed is a refusal, so nothing is taken away.** A file on a
  terminal centre traces the box and does nothing today. It becomes the verb
  that surface actually has.
* **It makes the window's own column better for free.** Dragging a file out of
  the files column onto a shell to get its path is a thing people try, and today
  it is refused.

Everything else in the table is unchanged, and each unchanged cell now has a
reason it can be asked for:

* **On a preview pane** the drop opens the file, which is `open_preview_onto`
  (`main.rs:54632`) and therefore the same door as a double-click in the tree,
  with the pool lookup and the view memory coming along.
* **On a files column**, a folder re-roots the column — and this is a genuine
  gain from #1, because a folder dragged off the desktop can now point a column
  at it. A **file** on a column stays `Refused`, and the reason is that the only
  meaning it could have is *copy it here*, which is a write to the filesystem
  this window does not make on a drag. The traced box is the honest answer
  (M147).
* **At a pane's edge or the window's rim** the drop splits and opens the file in
  the new pane, which for a `.png` is the picture pane and for a `.md` the
  reader — unchanged.
* **On the tab strip** the drop is refused. Opening a new tab at a dropped
  folder is a reasonable verb, and it is a tab-creation decision rather than a
  paste one; naming it here would widen this feature by a surface. Noted as a
  debt, not designed.

### 3.3 Hover feedback

**Ruling ⑮: the existing drag chrome, minus the ghost.** The landing highlight
and the traced refusal box are driven by `DropLanding`, computed from the live
pointer — which is the second reason the door of §3.1 has to report `DragOver`
and not just the drop. Nothing new is drawn.

The **ghost is suppressed for an OS drag**: the system is already drawing the
dragged file's own image under the pointer, and a second phantom would be this
window drawing a thing that is already there.

The promise rule applies with its full force. `DESIGN.md` §7.1.5f: *a mark is a
promise, and a mark that answers hover but not click is this window lying about
what it drew.* A highlight under a hand holding a file promises that letting go
does something, so every rectangle that highlights must have a verb and every
rectangle that does not must trace the refusal box instead. That is what routing
the OS drop through `row_verb` buys — the box and the verb are read off one
table, so they cannot disagree.

### 3.4 The rest of the drop's rules

* **Multi-file: one drop, one insertion, one gesture.** The door hands over the
  whole list; §2.6 spells it.
* **A picture with no file behind it** — dragging an image out of a browser,
  where the drag carries `public.png` or `CF_DIB` and no file URL — goes down
  the picture lane of §4 and arrives as a path. The lanes are shared, so this
  costs nothing, and it is what a reader dragging a picture at a shell means.
* **A drop while a modal is open is refused** — the first-run card, a dialog, a
  menu, the palette. `paste_from_clipboard_into`'s neighbours already keep that
  predicate (`main.rs:96260`).
* **A drop into a pane that is not the focused one does not move the focus.** A
  paste into a named pane already answers that pane's attention without taking
  the keyboard (`main.rs:96320`), and a drop is the same gesture with a
  different starting point.

---

## 4. A picture becomes a file

### 4.1 Where it goes

**Ruling ⑯: the platform's own temporary directory, in a folder of this
product's — `%TEMP%\Folio\clipboard\` on Windows, `$TMPDIR/Folio/clipboard/` on
macOS.**

Not `%LOCALAPPDATA%` and not `~/Library/Caches`: a cache is a thing the program
manages for its own benefit and keeps until it decides otherwise, and this is a
thing the *system* should be free to sweep. `$TMPDIR` is already per-account on
macOS and is already where this program keeps its per-run socket (`PRIVACY.md`,
"Elsewhere"); `%TEMP%` is already where the panic log goes. Both are directories
a reader already understands as temporary, which is half of the privacy answer.

The folder is created once, on the first picture paste of a run, and vetted the
way `launch_pipe_unix::vetted_endpoint` (`launch_pipe_unix.rs:470`) vets its
socket: `symlink_metadata` rather than `metadata`, refuse a symlink standing
where the folder should be, refuse a directory belonging to another uid, and on
Unix create it `0o700`. A refusal is a toast and no file — never a write
somewhere else.

### 4.2 The name, the format, the size

**Name — `clip-<yyyymmdd>-<hhmmss>.png`, in local time**, and on a collision
`clip-<yyyymmdd>-<hhmmss>-2.png`, `-3`, and so on. Local time, because the name
is for the reader, who is about to see it in a shell listing beside timestamps
from the same clock. The collision ladder exists because two pastes in one
second are ordinary, and it is resolved by **`File::create_new`** —
`CREATE_NEW` / `O_EXCL` — so that the collision is detected by the filesystem
rather than by a `stat` another process can invalidate between the look and the
write. Never `bt_persist::atomic_write`: replacing is exactly what must not
happen here.

**Format — PNG, always, and the source's PNG bytes are written through
unchanged.** If the clipboard offers the registered `"PNG"` format or
`public.png`, those bytes go to the file after a header check that confirms they
are a PNG and reports the dimensions; they are not decoded and re-encoded,
because a lossless round trip changes every byte for nothing and can drop
ancillary chunks such as a colour profile. A `CF_DIBV5` or `CF_DIB` — preferred
in that order, since V5 carries alpha and the older header does not — is
converted: a 14-byte `BITMAPFILEHEADER` is synthesised in front of the DIB, the
result handed to the `image` crate's BMP decoder, and the pixels re-encoded as
PNG. **This needs `bmp` added to the workspace's `image` features**
(`Cargo.toml:107` has `gif, jpeg, png, webp`), which is one decoder inside a
dependency already present, and a notices-drift check that is already automated.
A `public.tiff` takes the same road through the TIFF decoder.

One extension for every picture, because three readers depend on the name: the
agent, whose reader picks its decoder off it; Folio's own preview, if the reader
opens the path they just pasted; and the reader, who should be able to tell what
a file in that folder is without opening it.

**Size — refused above 64 MiB of source bytes, or above 256 MiB of decoded
RGBA** (which is, for instance, 8192 × 8192). The decoded bound is the real
cost, and the encoded one stops a decoder being handed a bomb. A refusal says so
in a toast; it is never silent, because a paste that appears to do nothing is
the failure mode §7.1.5f exists to prevent.

**The work does not run on the event-loop thread.** A DIB of a 4K screen is a
one-to-three-hundred-millisecond encode, which is a dropped frame on a product
that measures `event_to_present_us`. The gesture starts the work, the file
lands, and the path is inserted when it exists — re-asking
`input_line_needs_a_space_first` at that moment, which is exactly how K144
already decides its leading space, so a reader who typed a character in the
meantime still gets a well-formed line. A failed write is a toast and nothing
typed.

### 4.3 When the files go

**Ruling ⑰: swept at startup, at seven days. Never on exit, never never.**

Not on exit, because the path may be sitting in a shell's history, in an agent's
conversation, or in a command the reader is about to re-run with Up-Enter — a
terminal that deleted the file it had just named would be breaking the sentence
it wrote. Not never, because a screenshot is often of something private, and a
folder the reader does not know about should not accumulate them forever. Seven
days is longer than any session and longer than yesterday's history, and shorter
than any reasonable memory of what a picture was.

The sweep runs once at startup, which is the same shape and the same argument as
`diagnostics.log`'s rotation, already documented in `PRIVACY.md` as "checked
once at startup". It removes only **regular files** whose names match
`clip-*.png` in that one folder — not directories, not symlinks, not anything
else somebody put there — so it cannot be aimed at a file that is not ours.

### 4.4 The path is a path

**Ruling ⑱: once the file exists it is an ordinary file and takes §2 whole.**
Quoted by the pane's grammar, spelled in the pane's namespace — which means a
picture pasted into a WSL pane arrives as
`/mnt/c/Users/ann/AppData/Local/Temp/Folio/clipboard/clip-20260915-140233.png`
and into a `claude` pane bare. There is no second rule for pictures, and that is
the point of writing the file at all: after the write, #2 *is* #1.

### 4.5 What `PRIVACY.md` and `README.md` must say

A new row in `PRIVACY.md`'s **Elsewhere** list, in that section's voice, saying
all of:

* the two directories by name, `%TEMP%\Folio\clipboard` and
  `$TMPDIR/Folio/clipboard`;
* what is in them — *a PNG of whatever picture was on your clipboard when you
  pasted into a pane*, which is the plainest possible statement of the risk;
* that a file is written **only** by that gesture: no clipboard is watched, and
  nothing is written when you copy;
* that files older than seven days are removed at startup, and that you may
  delete the folder at any time;
* the delete command for each platform, as every other row there carries one;
* the switch that turns it off.

And one clause in the README's privacy paragraph (`README.md:87`). That
paragraph today accounts for the network and for settings, profiles and
sessions; a picture written to a temporary file is a **new kind of thing on the
disk**, and a paragraph that lists the kinds has to list it.

### 4.6 The one setting

**Ruling ⑲: one switch, and it is only for the picture.**
`Settings ▸ General ▸ Paste a picture as a file`, default **on**. Off, a
clipboard holding only a picture pastes nothing and says so in a toast.

The reason it exists: this is the only part of the feature that writes to the
disk, and a program that writes a file the reader did not name should let them
say no. The reason there is **no** switch for the path paste — the "paste plain
text instead" toggle the ticket asks about — is that it would have no case to
serve. When the clipboard holds files there is, in the overwhelming case, no
text on it at all: Explorer's `Ctrl+C` offers none, and *Copy as path* offers
text and is therefore already the text rung, quotes and all, unchanged. And when
a source offers both, ruling ② has already given the reader the text. A setting
is a question asked of every reader forever; ask it only when both answers are
real.

Its description is two sentences in the settings voice — written statement,
reader's perspective, no English mode names inside the Chinese, at most two
lines, pinned by the budget test — and it names where the file goes.

---

## 5. Where the feature lives

```text
bt-platform     clipboard_payload()  -> Files | Text | Picture(bytes + what they are) | Nothing
                the drop door:       register / point / payload / effect
                    windows_impl:    IDropTarget, RegisterDragDrop, with_drag_and_drop(false)
                    macos:           the three dragging selectors + paths_on
                (clipboard_text() unchanged, and still what every text field reads)

bt-transcript   PrintedPathNamespace::to_pane_spelling()   — beside to_local_path

bt-app          shell_literal.rs     ShellGrammar + shell_literal() + the line builder  [pure]
                profiles::shell_grammar(index)             — derive_integration's twin
                main.rs              paste routing, the drop landing, the picture lane
                                     inserted_path_text (K144) absorbed
```

**Ruling ⑳: `bt-platform` hands over bytes and what they are; it does not
decode.** `bt-platform` carries no `image` dependency and should not grow one —
a door's job is to cross the boundary. `bt-app` already depends on `image`
(`crates/bt-app/Cargo.toml:64`), and the conversion, the size bound and the file
write live there, above the platform line, where they are the same code on both
platforms.

**Bracketed paste: yes, unchanged, and by doing nothing.** The built line is
ordinary text and goes through `paste_text` → `input::paste_bytes`
(`main.rs:110689`, `input.rs:696`), so it is bracketed exactly when the shell
asked for bracketing and not otherwise. `sanitize_paste` is a no-op on it — a
quoted path has no control character and no newline — and a multi-file paste is
one lump inside one pair of brackets, which is what it is. This matters most in
an agent pane, where bracketed paste is how the agent knows a block arrived as
one piece.

**The keyboard: nothing new.** `is_paste_shortcut_on` (`input.rs:299`) already
answers `Ctrl+V`, `Ctrl+Shift+V` and `Shift+Insert` on Windows and `⌘V` on
macOS, and every one of them lands in `paste_from_clipboard_into`
(`main.rs:96290`) — as do the terminal menu's `Paste` row (`main.rs:73388`) and
macOS's `Edit ▸ Paste` action. One door, so the feature arrives at all of them at
once. A separate chord was considered and rejected: the reader's gesture is
*paste*, and asking them to know in advance what is on the clipboard in order to
choose a key is asking them to do the work this feature exists to do.

---

## 6. What is tested

### 6.1 Pure, and therefore the whole of the specification

**`shell_literal`, one table per grammar.** A bare safe path; a space; `'`; `"`;
`$`; a backtick; `%`; `!`; `#`; `&`; `;`; `(`; `[`; a leading `~`; a `~` in the
middle; CJK; an emoji; a name that is entirely unsafe characters; a path ending
in `\`, which is where `Cmd` doubles it and the other two do not; the drive root
`D:\`; and a path containing both `'` and a space, which is the combination WT
#18006 got wrong.

**`to_pane_spelling`, one table per namespace.** `D:\Demo\a.txt` →
`/mnt/d/Demo/a.txt` and `/d/Demo/a.txt`; `C:\` → `/mnt/c/`;
`\\wsl.localhost\Ubuntu\home\a\x` → `/home/a/x` in that distribution's pane and
untranslated in another's; `\\server\share\x` untranslated; a path on a drive
with no mount; a `Windows` namespace, which translates nothing.

**The round trip, which is the strongest pin in the set.** For every path in the
corpus and both foreign namespaces, `to_local_path(to_pane_spelling(p)) == p`.
The two directions live in one file so that they cannot drift; this is the test
that says so.

**Format precedence.** A fake clipboard described by *which rungs answer* —
every combination of {files, text, png, dib} — and an assertion on which rung is
chosen. Pure over the description rather than over a real clipboard, so it runs
on every platform including CI's.

**The temp file.** The name from a fixed timestamp; the collision ladder to
`-3`; the sweep predicate — which names are old enough, that a name not matching
`clip-*.png` is untouched however old, that a directory is untouched, that a
symlink is untouched.

**The line put at the prompt.** A leading space only when the cell left of the
cursor is not blank (the existing shape at `main.rs:152531`); a trailing space
always; three files space-separated in the source's order; the same three in an
agent pane, bare.

**The grammar derivation.** Every shipped row id on both seed platforms → its
grammar, as a table, so that adding a row without deciding its grammar is a red
test. Plus: a `pwsh` row with integration turned off is still `PowerShell`
(ruling ⑥ as an assertion), and every `AGENT_IDS` row is `Plain`.

**i18n.** The new toast strings and the setting's two lines, the `Text::ALL`
count (660 today), and the description's two-line budget.

### 6.2 By hand, on both machines, because none of the above touches a clipboard

**Windows.** Explorer `Ctrl+C` on one file, on three files, on a folder, and on
a file whose name has a space, a `'`, a `$`, a `%` and CJK — pasted into `pwsh`,
`winps`, `cmd`, `gitbash`, `wsl` and a `claude` pane, six panes in one tab, and
each line then *run*, to prove the shell opened the file. *Copy as path* pasted
into each, unchanged and not double-quoted. Snipping Tool and `Win+Shift+S` into
a `claude` pane and into `pwsh`. A drag from Explorer onto a terminal centre, a
terminal edge, a preview pane, a files column, the tab strip and the window
chrome — and out of the window and back, so that the box un-traces. A drop while
a menu is open. Fifty files at once.

**macOS.** Finder `⌘C` on one file and on three; `⌃⇧⌘4` into a `zsh` pane and a
`claude` pane; a drag from Finder onto each of the same surfaces; a picture
dragged out of Safari, which carries `public.png` and no file URL; a file whose
name contains `'`; a file with a CJK name; and **a file on an SMB or exFAT
volume whose name is not valid UTF-8**, which is the case `macos_files.rs` is
written about and the case winit's own handler gets wrong.

**Both.** The Markdown editor, the palette, the search field and the settings
fields still paste *text* when the clipboard holds a file — the regression this
feature is most likely to cause. `BT_PTY_DUMP` on for every window, and the
record kept.

---

## 7. Red lines

The feature must never:

1. **Touch the network.** Nothing here has an address.
2. **Read the clipboard except on the reader's own paste gesture.** No watcher,
   no timer, no read on focus, and **no read to decide whether a menu row should
   be enabled** — the terminal menu's `Paste` and macOS's `Edit ▸ Paste` stay
   always enabled, and a paste with nothing to paste says so afterwards. A
   terminal that polls the clipboard knows what you copied in another
   application; the third-party tools people currently use for #2 work exactly
   that way, and this one will not.
3. **Persist anything the clipboard held.** Not in `session.json`, not in
   `pins.json`, not in `diagnostics.log`, not in a `BT_*` trace. A diagnostic
   line for a paste may name the rung that answered and the number of paths; it
   may never name a path, a file name, or a byte of text.
4. **Resolve a symlink, or canonicalise.** The path pasted is the path the
   source named. The reader pointed at a name, and handing the shell a different
   name is the same lie a wrong underline is. On Windows there is a second
   reason for the same rule: `canonicalize` returns a `\\?\` prefix that half
   the shells in the matrix cannot open.
5. **Overwrite an existing file.** The picture is written with `create_new`
   only, in the one folder, and a collision takes the next name.
6. **Follow a link standing where the clipboard folder should be**, or write
   into a directory owned by somebody else.
7. **Press Enter.** A paste puts characters at a prompt and never runs them,
   whatever the clipboard held.
8. **Read a file promise**, or render a delayed format that would make another
   process write a file.
9. **Move, copy or delete a file the reader dragged.** A drop reads a name.
10. **Answer a highlight with nothing.** Every rectangle that lights under a
    held file has a verb; every rectangle that has none traces the refusal box.

---

## 8. The split

Three tickets, in order. Each ends green on its own and leaves the product
shippable.

### T-PASTE-1 — the clipboard door, and one path spelled for one shell

**Size: M-L** — the largest share of the pure code, and all of the argument.

`bt_platform::clipboard_payload` on both platforms (the files and text rungs;
the picture rung is declared in the enum and answers `Nothing` until T-PASTE-2).
`PrintedPathNamespace::to_pane_spelling`. `shell_literal.rs` with `ShellGrammar`
and the line builder. `profiles::shell_grammar`. The routing in
`paste_from_clipboard_into`. **K144's `inserted_path_text` absorbed**, which is
also the fix for its macOS defect (§2.1). All of §6.1 except the temp-file rows.

Ships: #1's clipboard half, on every shell, on both platforms.

### T-PASTE-2 — a picture becomes a file

**Size: M.**

The picture rung on both platforms. The folder, its vetting, the name, the
collision ladder, the startup sweep. DIB and TIFF conversion, PNG passthrough,
the `bmp` feature on `image`, the size bounds, the off-thread encode and the
late insertion. The `Settings ▸ General` row and its two lines in both
languages. `PRIVACY.md`'s new row and the README clause.

Ships: #2, whole. Depends on T-PASTE-1 for the spelling and the quoting.

### T-PASTE-3 — the drop door

**Size: L**, and it opens with a probe rather than an implementation.

A Mac probe first, in the `docs/plans/port/probe-x*.md` shape: can the three
dragging selectors be replaced on winit's content-view class without disturbing
winit's own view, and does a replaced `performDragOperation:` reach us with the
pasteboard and the location. Then the Windows `IDropTarget` and
`with_drag_and_drop(false)`; the macOS half; the landing computed from the drop
point; the one new row in `row_verb` and the OS drop routed through it; the
ghost suppressed; multi-file; and the picture-on-a-drag case sharing T-PASTE-2's
lane.

Ships: #1's drag half, and the window's own files column gains the same verb.
Depends on both tickets above.

---

## 9. Sources

Every claim about this codebase is cited in place. The outside ones:

* microsoft/terminal **#16627** — pasting a file copied in Explorer pasted its
  path in 1.18, stopped in 1.19, PR #16634.
* microsoft/terminal **#15646** and PR **#16214** — `$hello.txt` dropped into a
  WSL tab expanded as a variable; the fix is single quotes.
* microsoft/terminal **#18006** — a path containing `'` dropped into a WSL tab
  was single-quoted without escaping, ending the quoting at the apostrophe.
* microsoft/terminal **#8109** — the argument for always quoting a dropped path.
* Windows Terminal's `_translatePathInPlace`
  (`src/cascadia/TerminalControl/TermControl.cpp`) and the
  **`pathTranslationStyle`** profile setting — `none` / `wsl` / `cygwin` /
  `msys2` / `mingw`, defaulting to `none` except on WSL profiles.
* kitty's **OSC 5522**, the clipboard protocol with MIME types — the other
  answer to #2, which needs the program to ask rather than the terminal to act.
* The WezTerm community recipe writing clipboard pictures into
  `/tmp/wezterm-clipboard-images/`, and ghostty-org/ghostty discussion **#10517**
  — the state of the art for #2 outside this product, which is a Lua snippet and
  a feature request.
* winit **0.30.13**, read in `~/.cargo/registry`:
  `src/platform_impl/windows/drop_handler.rs`,
  `src/platform_impl/macos/window_delegate.rs:369-429`, and
  `src/platform/windows.rs:497` (`with_drag_and_drop`).
