# A promise on the clipboard is pasted as files (0.4.10 plan row 81, Part B)

Beside `paste-paths-design.md`, whose Ruling ⑤ this supersedes for the clipboard on Windows
(owner, 2026-11-09). The drag half of Ruling ⑤ stands: dragging a promise onto a pane is plan
row 83. The decisions are the coordinator's rulings R1–R14 of 2026-11-09 in the row's report.

## What a promise is

A file copied from inside a program — a mail or chat attachment, a photo in Phone Link, a file
inside a zip folder shown by Explorer — reaches the clipboard as a *promise*: a descriptor
(`FileGroupDescriptorW`, a count and one 592-byte `FILEDESCRIPTORW` per entry) and the entries'
contents (`FileContents`), which the source renders only when asked, entry by entry, by its index
(`FORMATETC.lindex`). There is no path to paste until something writes the files.

## The rule

**The clipboard lane answers in a second or says why; anything that renders goes to a worker of
its own, as the picture save does** (R1). A render can take seconds — an attachment fetched from a
server, a file unpacked from an archive — and on the lane it would run past `PASTE_DEADLINE` and
hold every later copy and paste behind it.

## The rung

`read_payload`'s order is unchanged: files, text, picture, promise, nothing. The promise rung is
reached only when no picture rendered; `PromisedFiles::picture_passed_over` says whether one was
offered. On Windows the lane reads the descriptor inside its open interval (bounded copy,
`clipboard_promise::MAX_DESCRIPTOR_BYTES`) and answers `ClipboardPayload::Promise`; a descriptor
that is missing or does not read answers `Refused(Promise { picture_passed_over })`, as before.
macOS answers `Refused` for every promise (R6): AppKit's file promises are a drag facility, and the
pasteboard is read on the main thread, where nothing may render.

## The render

`Runtime::stage_clipboard_promise` takes one of the picture save's places
(`CLIPBOARD_PICTURE_JOBS`), withdraws the mailbox's generation and starts `clipboard-promise`. The
worker:

1. reserves `<staging>/<stamp>-<n>/` with `create_dir` (`clipboard_picture::reserve_staging_folder`;
   the staging folder is the window host's: `%TEMP%\folio\clipboard` in the product, a test's own
   folder headless);
2. asks the window's clipboard door to render into it (`ClipboardDoor::render_promised_files`;
   `PlatformClipboard` calls `bt_platform::render_clipboard_promise`, a worker-only door);
3. sweeps the staging folder after a success (`clipboard_picture::sweep_staging`);
4. lays its answer in the mailbox and wakes the window (`AppEvent::ClipboardPictureReady`).

`render_clipboard_promise` joins a single-threaded apartment (`OleInitialize`), asks
`OleGetClipboard` through the clipboard's one declared contention wait (5, 10, 20 and 40 ms on
`CLIPBRD_E_CANT_OPEN`, through `wait::sleep_within`), wraps the data object in `OleContents` and
runs the pure renderer, `clipboard_promise::render_promise`:

- **judged first** (`judge_promise`): at most 64 entries and 256 MiB declared (R2); every name
  split on either separator and every component a file name Windows accepts — not empty, not `.`
  or `..`, none of `< > : " | ? *` or a control, no trailing space or dot, no device name (with or
  without an extension, the superscript digits included), at most 255 UTF-16 units; and no two
  names one name without case;
- **identity** (R13): the descriptor is read again through OLE and must equal what the lane read —
  the sequence number cannot say, delayed rendering advances it on a read that succeeded;
- **written**: a folder entry is made and never asked for; a file entry's `FileContents` is asked
  for as `TYMED_ISTREAM | TYMED_HGLOBAL | TYMED_ISTORAGE` — a stream is copied in 64 KiB chunks with
  the thread's own messages let through between them, a global up to its declared size (a global
  is rounded up), a structured storage into a compound file at the entry's path (`StgCreateDocfile`,
  `CopyTo`, `Commit`; R5). Every byte is charged against the paste's 256 MiB as it lands;
- **pasted**: the distinct top-level entries, in the descriptor's order (R3);
- **any refusal removes the paste's folder** before it answers.

No clock is kept on the render: the window does not wait for it, a newer paste supersedes it by
generation, and a source that never returns holds one of the four places — a fifth staging paste is
refused `workers-busy`.

## What is pasted and said

`adopt_clipboard_picture` pastes the answer's paths through `paste_paths_into(..,
PasteFrom::Promise(sequence))` — the road a copied file list takes, so the spelling is the one a
copied file gets. A success shows nothing and writes
`clipboard promise saved: <n> files, <bytes> bytes, <ms> ms; sequence=<N>` — no names (R11). A
refusal is one `paste refused: promise (<why>[, picture passed over]); road=promise; sequence=<N>`
line and a notice: too many or too large says so; one beside a picture that did not render says the
picture could not be read; any other keeps "copy a saved file instead" (R7).

## Staging

Each promise paste's folder keeps its files under their own names. `clipboard_picture::ours` reads
`<stamp>-<n>` (a folder) as well as `<stamp>-<n>.png`; the one sweep keeps the newest 20 entries of
either, a retired folder removed with what is in it. A name without the picture's extension is
listed only when it is a folder, so a file or a link wearing that shape, and anything not Folio's,
is never touched.
