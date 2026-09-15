STATUS: COMPLETE — phase 4 complete; 23 findings (4 blocking, 15 should-fix, 4 nits); all 24 first-round findings verified against the revised text; citations checked.

# R2-PASTE-PATHS — second adversarial review, 2026-09-15

**This pass is by an Opus reviewer standing in for Codex**, whose usage limit
resets **2026-09-19**; Codex re-checks then. Treat every judgement below as a
stand-in reading, not as the Codex re-review the round was designed for.

Reviewed `docs/plans/paste-paths-design.md` at `08dc5c2f` against
`docs/plans/review/paste-paths-review-2026-09-15.md` at `6a245777` (24 findings,
7 blocking; the design it reviewed was `dee7d4b3`). `D:n` means line n of the
revised design. `R:n` means line n of the first review. Source shorthand:
`main.rs`, `input.rs`, `profiles.rs`, `shell_integration.rs`, `hang_watch.rs`
and `i18n.rs` are in `crates/bt-app/src/`; `paths.rs` is
`crates/bt-transcript/src/paths.rs`; `lib.rs`, `macos_files.rs`,
`macos_services.rs`, `macos_impl.rs`, `launch_pipe_unix.rs` and `instance.rs`
are in `crates/bt-platform/src/`. `registry/` is
`C:/Users/Weiyi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

Method: the whole 1411-line revised design read; every `path:line` in it opened
and compared with the code; winit 0.30.13 and `image` 0.25.10 read in the
registry; the house docs (`CONVENTIONS.md`, `docs/DESIGN.md`, `docs/PRIVACY.md`,
`README.md`, `packaging/macos/entitlements.plist`) and the standing plan
`docs/plans/shell-matrix-2026-09-07.md` read. Read-only tooling: `grep`, `sed`,
`python` over files. **No `cargo` of any kind, no build, no test, no app
launch, no clipboard or drag experiment.** No code and no design edit. Working
evidence: `target/review2-notes.md`.

## Verdict

**Fix these first. Do not open T-PASTE-1 yet.** The revision is real work, not a
ledger: all 24 findings land as changed text, and several — 2, 14, 15, 18, 19,
20, 21 — are closed thoroughly enough that the new sections are better than the
old ones were wrong. But four of the corrections open new holes of their own,
and two of those sit inside T-PASTE-1's own lane:

* **Rethink §1.2's rung transaction.** Ruling ④'s one-snapshot read and ruling
  ⑤'s promise refusal are each unimplementable as written — the first puts an
  unbounded cross-process render on the event-loop thread with no mechanism for
  the bound PROBE 8 promises, the second asks a four-variant enum to say a fifth
  thing (finding 1, finding 2).
* **Rethink §2.5's ruling ⑫.** It reverses a standing user ruling of 2026-09-07
  (`docs/plans/shell-matrix-2026-09-07.md:365`) without citing it or naming it a
  reversal, and the derivation it puts in place has no arm for Cygwin — a
  distinction the design's own cited source (`pathTranslationStyle`) makes
  (finding 3).
* **§2.3's `Agent` arm is wrong for apostrophe-bearing paths on both
  platforms**, by the design's own account of the Codex parser (finding 4).

## Part 1 — did the 24 corrections land?

Each row was checked against the revised text, not against the ledger. "Closed"
means the correction is in the document, complete, and does not contradict
another section. Where a row is not clean, the reason carries a **new finding
number** from Part 2.

| # | Landing | Note |
| --- | --- | --- |
| 1 | **closed** | The safe-set predicate is gone: D:300–302 "Every grammar **always quotes**. There is no bare form and no 'is this path simple enough' predicate". PowerShell's quote class is doubled (D:319, D:334–345); the `char::is_whitespace()` assumption is withdrawn (D:344). §6.1 asserts by running a lexer (D:1071). Residue: PROBE 2's fallback is not operational — **finding 18**. |
| 2 | **closed** | §2.4 (D:412–446) is new and complete: `to_str()` refusal, C0/C1/DEL/line-terminator refusal, `to_string_lossy` banned including in K144, the false "a quoted path has no control character" sentence struck and replaced at D:1012–1014, the acceptance promise amended at D:444–446 and D:1165–1167. The quotation of `sanitize_paste` is inexact — **finding 20**; and the gate's application to a picture's *own* path is undefined — **finding 15**. |
| 3 | **closed** | D:347–372 separates the interpreter from the CRT consumer, states `2N`, corrects the `"D:\"` description ("consumes the backslash, emits a literal `"` … and leaves the quote state flipped", D:355–357), refuses `%`, refuses `!` under a `/v:on` row, leaves `^` alone. Exported to a non-`cmd` recipient by ruling ⑩ — **finding 5**. |
| 4 | **closed** | `Fish` arm at D:307, D:381–386; `Nushell` at D:308, D:388–393; "every shell an account can be set to" withdrawn (D:391–393); §6.1 tests the combinations (D:1070). The nu fence is asserted, not cited — **finding 11**. |
| 5 | **closed** | Ruling ⑦ (D:250–266) says spawn-time default in as many words and cites `shell_integration.rs:248` — verified: `WSL_LOGIN_SHELL`'s `*) exec "$shell" -l` arm is at that constant. Ruling ⑨ adds `paste_as`. §6.1 tests `wsl.exe -e nu` (D:1090). |
| 6 | **closed, regressed elsewhere** | D:449–462 does re-derive and does change the reading direction. But it reverses a user ruling and has no Cygwin arm — **finding 3**. |
| 7 | **closed** | D:464–499 separates lexical rules from mount facts, discloses the default-mount assumption, reads `\\wsl$\`, normalises drive case, names the `\\wsl.localhost\…\mnt\d\x` non-identity, forbids `~`. §6.1 splits the round-trip suite in two (D:1082–1087). |
| 8 | **closed** | D:501–515 names the MSYS stage and `MSYS2_ARG_CONV_EXCL`, refuses to change the environment (red line 12, D:1211–1213), documents the forward-slash fallback through the override, and books PROBE 4. |
| 9 | **partially closed** | The reversal is there (D:395–411) and the bare-path claim is withdrawn. But the ruling's own justification is false for apostrophe paths — **finding 4** — and its gate contradicts its table — **finding 10**. |
| 10 | **closed** | Ruling ② (D:104–133) states the preference and the loss, adds **Paste picture**, books PROBE 1, and the contradictory setting rationale is rewritten (D:966–973). ShareX is not cited at all. The new verb collides with red line 2 — **finding 8**. |
| 11 | **partially closed** | Absent/Present/Unreadable is defined (D:135–160) and the `Result`/`Option` descriptions are corrected (D:66–77, verified against `lib.rs:5940`'s `return Ok(String::new())`). The snapshot is unimplementable — **finding 1**; promise-only has no variant — **finding 2**; the within-rung fallback is unreachable — **finding 6**; and the WSLg/RDP half the ledger says PROBE 1 covers is absent from PROBE 1's own list — **finding 14**. |
| 12 | **closed** | §5.2 (D:1015–1034) limits the promise to a fresh argument boundary and names the three cases outside it; §6.2 asserts "the argument the program received or the file it opened" (D:1124). |
| 13 | **closed** | Ruling ㉖ (D:851–877) carries window/tab/leaf, session incarnation and a sequence number, revalidates at completion, cancels and deletes through the owned path, and covers delayed drops. |
| 14 | **closed** | Verified in the registry: `winit-0.30.13/src/platform_impl/macos/window_delegate.rs:367` is `unsafe impl NSDraggingDestination for WindowDelegate`; the method set is `draggingEntered:` (369), `prepareForDragOperation:` (391), `performDragOperation:` (398), `concludeDragOperation:` (420), `draggingExited:` (426) — **no `draggingUpdated:`**; registration is `window.registerForDraggedTypes` at 666–669 with `NSFilenamesPboardType`; paths come back through `propertyListForType` (376, 405), i.e. `NSString`s. D:545–557 and D:568–600 say all of this correctly, and PROBE 5 now prefers an app-owned view. No outcome is stated if all three shapes fail — **finding 17**. |
| 15 | **closed** | Verified: `windows/window.rs:1167–1195` gates `OleInitialize` (1168) and `RegisterDragDrop` (1194) together behind `attributes.platform_specific.drag_and_drop`; `event_loop.rs:1260–1262` calls `RevokeDragDrop` unconditionally on `WM_DESTROY`. D:602–618 states ownership, every-HWND registration, `DRAGDROP_E_ALREADYREGISTERED`, teardown against the unconditional revoke, `COPY`/`NONE` only. It misses winit's second COM owner — **finding 12**. |
| 16 | **closed** | Verified: `main.rs:28995–29004` does document the strip arm as unreachable behind `layout_aim`, and `row_strip_landing` is at `29555`. D:632–644 describes it accurately and removes the row. Ruling ⑱ (D:653–674) defines batch admission with a 64 cap; ruling ⑲ (D:669–674) keeps K144's focus move (`main.rs:76921` confirmed: `set_files_keyboard` then "layout focus follows"). |
| 17 | **closed** | §3.4's matrix (D:697–706) registers raw picture types, does not register promises, and D:713–719 withdraws the Safari claim for PROBE 6. The matrix is incomplete for two of the four landings — **finding 13**. |
| 18 | **closed** | Verified: `registry/image-0.25.10/src/codecs/bmp/decoder.rs:534` is `new_without_file_header`, and the root `Cargo.toml:107` enables only `gif, jpeg, png, webp`. D:794–812 withdraws the synthesised header, D:830–839 removes TIFF to a debt, ruling ㉔ (D:813–828) makes alpha a property of compression and mask and refuses unsupported layouts. |
| 19 | **partially closed** | The acquisition bound, the decode-byte cap, the aggregate quota, one-in-flight, complete failures and PROBE 8 are all at D:841–850. But the latency contract has no mechanism — **finding 1** — and the quota is not serialised — **finding 7**. |
| 20 | **closed, regressed elsewhere** | Verified: `instance.rs:360` does create with an explicit mode, `symlink_metadata`, refuse a symlink/non-directory/foreign uid and repair the mode; `launch_pipe_unix.rs:470` is indeed a socket vetting. D:740–776 takes the directory precedent, vets both levels before every operation, adds the Windows reparse/DACL counterpart and states the handle residue. But it takes the precedent's vetting and drops its **naming** — **finding 9**. |
| 21 | **closed** | D:879–911 strikes both claims, adds the hourly sweep, states OS cleanup, gives the exact owned-name grammar and mtime clock, excludes active writes and says the sweep is not clipboard watching. The quota half of "across processes" is still open — **finding 7**; the sweep has no thread — **finding 16**. |
| 22 | **closed** | D:949–975 makes the switch cover drops and cancel pending jobs; red line 1 (D:1174–1180) is narrowed to feature-initiated requests; red line 3 (D:1187–1194) carries the named storage exception; §4.5 (D:923–947) discloses drops, PNG metadata, cleanup limits, redirected `%TEMP%` and downstream copies; §7.3 (D:1221–1233) separates `BT_PTY_DUMP` and cites `entitlements.plist:20` — verified, the file says "Not sandboxed. Developer ID distribution outside the App Store" at 20–21. |
| 23 | **closed** | §8 (D:1235–1303) sizes T-PASTE-1 **L**, splits T-PASTE-2 into 2a/2b at **L–XL**, makes T-PASTE-3 **L–XL** with three reviewable parts opening with a bounded probe, and each ticket names the lane it does not ship. D:1052–1055 strikes the "whole of the specification" claim. T-PASTE-1's scope sentence contradicts the ledger — **finding 10**. |
| 24 | **closed** | D:196–210: no inference from report timing, Ghostty narrowed to SSH, WezTerm marked unverified. §9.3 carries the Codex revision. The `usershell` anchor is not corrected so much as removed — **finding 22**. |

**Nothing is ledger-only.** Every row's claimed change is present in the body.
Two rows describe their change slightly better than the body delivers it (11,
24), and two rows describe a change that is complete but that breaks something
else (6, 20).

## Part 2 — new findings

### 1. Blocking — ruling ④'s one transaction puts an unbounded cross-process call on the event-loop thread, and PROBE 8's bound has no mechanism

**Evidence.** D:162–169: "All rungs are read inside one transaction — on Windows
one `OpenClipboard` … `CloseClipboard` pair … Delayed rendering means the owner
runs code while we hold the clipboard open". D:848–850: "the read is bounded and
a source that does not answer within it is `Unreadable` (ruling ③). **PROBE 8:
measure delayed-render latency for the common Windows sources** so the bound is a
number with evidence under it." The door is synchronous on the event-loop
thread: `paste_from_clipboard_into` (`main.rs:96290`) is called from key
handling, and `lib.rs:5927` says in as many words "all calls run on winit's
event-loop thread"; `open_clipboard_with_retry` (`lib.rs:5913–5923`) already
spends `std::thread::sleep` there.

**Trigger / breakage.** `GetClipboardData` on a delayed format sends
`WM_RENDERFORMAT` to the owner and blocks until the owner's thread answers.
There is no timeout parameter and no cancellation: a hung or slow owner (a
browser mid-GC, an Office process on a stalled network drive, a remote-desktop
clipboard bridge) freezes Folio's event loop — no frame, no keystroke, no
resize — for as long as it takes. A number measured from five common sources is
not a bound on an arbitrary source, so PROBE 8 cannot produce what the ruling
spends. Worse, the ruling *widens* the window: today `clipboard_text` reads one
format; ruling ④ reads up to five inside one open, so the exposure is per rung.

**Correction.** Say which thread the acquisition runs on and give it a
`hang_watch` station — every other synchronous cross-process call on this loop
has one (`hang_watch.rs:291` `WebPage`, `:301` `WebRetire`, `:284` `PtyResize`).
Either (a) state plainly that a delayed render is unbounded, that it blocks the
loop, and that the only mitigation is the hang watch's report — and delete
"bounded" and PROBE 8's promise; or (b) move the whole transaction to a worker
thread with its own clipboard owner window, which is a different design of
`clipboard_payload` and has to be written, not implied. Do not ship a sentence
that says the read is bounded when no Win32 call in the path takes a timeout.

### 2. Blocking — `ClipboardPayload` cannot say "there was a promise", so ruling ⑤'s visible refusal and §7.2's toast are unimplementable

**Evidence.** D:92–97 gives four variants: `Files`, `Text`, `Picture`,
`Nothing`. D:171–182 (ruling ⑤): "A pasteboard or a drag offering **only** a
promise is `Absent` at every rung and is refused visibly." D:1216–1220 lists "a
promise-only payload" among the refusals that are "a toast that names what was
refused and why". D:1256–1260 (T-PASTE-1): "A clipboard picture is `Absent` and
the paste says 'nothing to paste'."

**Trigger / breakage.** Absent at every rung yields `Nothing`, which is exactly
what an empty clipboard yields. The caller therefore cannot distinguish "you
have not copied anything" from "you copied something Folio refuses to read",
and the promised toast either never appears or appears on every paste with an
empty clipboard — which would be a toast on `Ctrl+V` at a fresh login. The same
conflation hits the drop side: D:701 says a promise is "refused while hovering",
but §3.4's registration rule (D:708–712, "A **promise** is not registered at
all") means the destination never sees the drag, so there is nothing to draw a
refusal box on either. Red line 11 (D:1206–1208) — "Answer a highlight with
nothing" — is satisfied only because *no* highlight appears, which is not the
same as the refusal §3.3 promises.

**Correction.** Give the payload a fifth state, e.g.
`Refused(UnsupportedKind)`, set when a rung's format family is advertised but
deliberately not read, and route only that state to the toast; keep `Nothing`
silent. On the drop side either register the promise types purely so the
destination can refuse them visibly (and then never call
`performDragOperation:`/`receivePromisedFiles`), or delete the "refused while
hovering" sentence and say the drag is simply not accepted, with no mark — and
say which, because §3.3 turns on it.

### 3. Blocking — ruling ⑫ reverses a standing user ruling without citing it, and its replacement has no Cygwin arm

**Evidence.** D:449–462: "the namespace is a fact about the **program**, and
integration is a fact about a startup script. `derive_namespace(&Profile) ->
PrintedPathNamespace` reads the program and the `paths` field,
`printed_path_namespace` becomes a caller of it, and **the reading direction
changes with it** … That is a strict improvement". The rule it replaces is a
user ruling: `docs/plans/shell-matrix-2026-09-07.md:365–372` — "**Fixed,
2026-09-07, on the user's ruling** (`fix/unix-path-spellings-and-cmd-hover`). The
recommended shape: … `printed_path_namespace` **derives it from the pair a
profile already carries** — Windows directories behind a bash init file is an
MSYS bash … **Nothing reads a profile id, and nothing guesses from the text**."
The code carries the same sentence: `profiles.rs:3193`, "it is read off the pair
the row already carries rather than off its id". The design cites neither.
Meanwhile D:1343, in the design's own source list, records Windows Terminal's
`pathTranslationStyle` as `none` / `wsl` / **`cygwin`** / **`msys2`** / `mingw`;
the word *cygwin* appears nowhere else in the document
(`grep -in 'cygwin\|cygdrive' docs/plans/paste-paths-design.md` returns one hit, D:1343).

**Trigger / breakage.** Two separate breakages. (a) **Process**: a design
document silently overturns a ruling the owner took on a dated branch. Whatever
the merits, that is the owner's call, and the ledger never surfaces it — §10's
row 6 says "Stated as a behaviour change outside the feature's surface", which
describes the *size* of the change and not the fact that it contradicts a prior
ruling. (b) **Substance**: the replacement keys MSYS-ness on the program rather
than on the init-file pair, and a Cygwin `bash.exe` row has the same program
stem and the same `paths: Windows`. Cygwin maps drives at `/cygdrive/d`, not
`/d`. Under the old rule such a row answered `Windows` and Folio translated
nothing — inert and safe. Under ruling ⑫ the same row both **pastes**
`/d/Demo/a.txt`, which Cygwin cannot open, and **detects** `/d/…` in its output
as a link to a file it is not, which is §7.1.5f's "a mark that answers hover but
not click" failure the design itself quotes at D:685–687. The design argues the
reading-direction change is "a strict improvement"; for Cygwin it is a strict
regression, and the regression is in the detector, i.e. outside this feature.

**Correction.** Do not decide this here. Surface it to the owner as a reversal
of the 2026-09-07 ruling, with `shell-matrix-2026-09-07.md:365` cited, and say
what the new derivation is keyed on. Whatever is decided, add a Cygwin arm or a
Cygwin refusal to the grammar/namespace table and to §6.1's derivation test, and
say which of MSYS2 / Cygwin / MinGW stems the derivation recognises — the
design's own cited precedent needs five values where the design has two.

### 4. Blocking — the `Agent` arm hands an agent a *different filename* for any apostrophe-bearing path, on both platforms

**Evidence.** D:395–401 describes the parser: "Codex's `normalize_pasted_path`
strips one surrounding `"` or `'` pair, tries a Windows-path recogniser, and
otherwise runs **shlex over the original string and requires exactly one
token**." D:402–404 concludes: "The POSIX single-quoted form is the one spelling
that satisfies both the quote-strip and the shlex paths, so `Agent` uses it on
**both** platforms." The `Agent` escape is `'` → `'\''` (D:313). D:409 scopes the
suspicion to Windows only: "an apostrophe-bearing **Windows** path is a suspected
wrong name".

**Trigger / breakage.** Take `/Users/ann/John's Papers/a.png`. The `Agent`
encoder emits `'/Users/ann/John'\''s Papers/a.png'`. The first branch strips the
outer pair and yields `/Users/ann/John'\''s Papers/a.png` — a name with four
extra characters in it, naming no file. Whether that branch *returns* is the
whole question, and the design's own sentence says the shlex branch runs
"otherwise", i.e. only if the earlier branches did not answer. So on the
design's own account the conclusion at D:402 is false for exactly the class of
path §2.4 exists to protect, and the suspicion at D:409 is scoped one platform
too narrowly. This is finding 2's failure mode — a different file — arriving
through the arm that was supposed to close finding 9.

**Correction.** Re-read the parser and state which branch wins for a quoted
string that is not a Windows path, with the revision pinned. If the quote-strip
branch wins, the POSIX single-quoted form is wrong for apostrophe paths and
`Agent` needs either a different spelling for them or a visible refusal. Until
that is read, mark the whole `Agent` row **PROBE 3** rather than a ruling —
which is what D:410–411 already half-says ("`AGENT_IDS` is a starting grouping,
not a finding") and what the table at D:313 contradicts.

### 5. Should-fix — ruling ⑩ exports `cmd`'s interpreter refusals to programs that are not `cmd`

**Evidence.** D:286–291 (ruling ⑩): "On Windows, `Cmd` — double quotes under the
C runtime rules, which is what a Windows program's own argv parser reads." The
`cmd` row's rules are at D:359–365: "**Ruling: a path containing `%` is refused
in a `cmd` pane**".

**Trigger / breakage.** `%NAME%` expansion is `cmd.exe`'s command-line
interpretation, not the C runtime's argument parsing. A Python REPL row, a
`node` row, a custom tool row — anything unheard-of on Windows — gets grammar
`Cmd` and therefore inherits a refusal that has no cause in it.
`D:\Data\100%\report.csv` and `C:\Users\ann\Documents\50% draft.docx` are legal
names that would be refused with a toast in a pane where they would have worked.
A visible refusal is better than a wrong answer; it is not better than a right
answer.

**Correction.** Split the grammar from the interpreter: keep `Cmd` as the
CRT-quoting encoder and make the `%` and `!` refusals a property of the
*`cmd.exe` interpreter row specifically*, not of the encoder. State that an
unheard-of Windows program gets CRT quoting with no expansion refusals, and say
why.

### 6. Should-fix — ruling ③'s within-rung fallback is unreachable for the failure it exists to cover, because ruling ㉚ puts validation above the layer that can fall back

**Evidence.** D:157–160: "The one fallback that *is* allowed is **within** the
picture rung — PNG unreadable, try `CF_DIBV5`, then `CF_DIB`". D:995–1000
(ruling ㉚): "`bt-platform` hands over bytes and what they are; **it does not
decode**. It carries no `image` dependency". D:797–803: the validation that
detects a broken picture is a **full decode**, and it lives in `bt-app` —
"those bytes are decoded end to end to prove they are a whole picture — an
`IHDR` that parses says nothing about a truncated or corrupt stream".

**Trigger / breakage.** A source that offers both a corrupt or truncated PNG and
a perfectly good `CF_DIB` — the exact shape ruling ③ names — gets a refusal. The
platform layer reports `Present(Png)` because the handle locked and the bytes
copied; the app layer then fails the full decode, and by then the clipboard
transaction is closed (ruling ④) and the `CF_DIB` bytes were never copied.
Re-opening to fetch them would be a second snapshot, which ruling ④ forbids.

**Correction.** Either have the platform layer copy *every* picture encoding it
finds inside the one transaction and hand up an ordered list, so the app layer
can fall back after a failed decode; or state plainly that the fallback covers
only acquisition failure (lock and render), that a decode failure is terminal,
and delete "malformed content" from `Unreadable`'s definition at D:145 — where
it currently sits, implying the platform layer can detect it.

### 7. Should-fix — the 512 MiB quota eviction is not serialised and can delete a file whose path is in an unsubmitted input line

**Evidence.** D:846–848: "A write that would exceed the directory quota first
removes the oldest `clip-*.png` files; if that cannot free enough, the write is
refused." D:849–850: "**One in flight.** At most one picture job **per
window**." D:903–907 protects active writes only on the age path: "a file being
written by *another* Folio is excluded by the `create_new` + age rule, since a
file younger than the cutoff is never a candidate."

**Trigger / breakage.** The age argument does not apply to the quota, which is
oldest-first regardless of age. Eight 64 MiB pastes in five minutes and the
ninth evicts the first — whose path the reader has sitting in a half-typed
command line they have not pressed Enter on yet. §4.3's disclaimer covers
*history* (D:898–901), not a live input line. Separately, "one in flight per
window" means two windows — and two Folio processes, since the directory is
shared — can each read the directory size, each conclude there is room, and both
write; nothing serialises the read-evict-write sequence across them.

**Correction.** Give the quota a floor: never evict a file younger than some
stated age, and refuse the write instead, so eviction cannot reach a path that is
plausibly still on a prompt — and say the number. State how the quota is
serialised across windows and processes, or state that it is best-effort and may
overshoot. Extend §4.3's "not a promise the file is there" sentence to cover the
unsubmitted line, not only history.

### 8. Should-fix — red line 2 contradicts itself over **Paste picture**, and macOS menu validation makes its carve-out hard to hold

**Evidence.** D:1181–1186: "**Read the clipboard except on the reader's own
gesture** … and **no read to decide whether a menu row is enabled**. `Paste`
stays always enabled. **Paste picture** is the one row whose enablement depends
on what is there, and it reads only the advertised *type list*, never the
content, and only while its menu is being built." D:1044–1046 makes it "a new
menu row and a bindable command with no default chord"; §5.3 notes macOS's
`Edit ▸ Paste` action (D:1040–1042).

**Trigger / breakage.** The prohibition and its exception are in one sentence,
and the exception is exactly the prohibited thing. On Windows, reading the
advertised type list means `OpenClipboard` + `EnumClipboardFormats`, which is a
clipboard open — it can fail while another process holds it, and it is not free.
On macOS an `Edit` menu row is validated by AppKit through `validateMenuItem:`
whenever the menu is opened *and* on key-equivalent dispatch, so "only while its
menu is being built" is not something the app controls for a menu-bar row; each
validation reads `NSPasteboard.types`. The rule as written cannot be pinned by a
test, because it forbids and permits the same call.

**Correction.** Rewrite red line 2 as one rule with one exception stated
positively: the clipboard's *type list* may be read when a menu that contains
**Paste picture** is opened, and its *content* only on a paste, a Paste picture
or a drop. Say what happens when the type-list read fails — row disabled, not
hidden. Decide whether the row lives in the macOS menu bar at all, given
validation; a terminal-context-menu-only row avoids the whole question. Also
state what **Paste picture** does when §4.6's switch is off — §4.6 (D:951–962)
is silent, and a row that is enabled and then toasts "turned off" is the second
kind of lie §7.1.5f is about.

### 9. Should-fix — §4.1 takes `instance.rs`'s vetting and drops its naming, which on the `/tmp` fallback is a squattable, permanently-fail-closed directory

**Evidence.** D:736–739: "`std::env::temp_dir()`, then `Folio`, then
`clipboard`. That is `GetTempPath2W` on Windows and `$TMPDIR` on macOS, which
is already per-account there". D:746–749 cites the precedent:
"`instance::prepare_runtime_directory`
(`crates/bt-platform/src/instance.rs:360`)". That precedent's sibling is
`instance.rs:331–338`:

```rust
pub fn runtime_directory() -> PathBuf {
    let base = std::env::var_os("TMPDIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let uid = unsafe { libc::geteuid() };
    base.join(format!("folio-{uid}"))
}
```

D:774–776: "**Fail closed, with a sanitised message.** A refusal is a toast and
no file".

**Trigger / breakage.** `std::env::temp_dir()` on Unix is `$TMPDIR` **or
`/tmp`**, and "which is already per-account there" is true only of the first.
`/tmp` is not per-account: a directory literally named `Folio` there is a name
any other local user can create first. Then §4.1's uid check refuses it, §4.1
fails closed, and the feature is dead for that user on that machine with no
recovery path in the design — a one-line local denial of service. The house
precedent solves this in one format string, and the design cites the function
ten lines below the one that does it.

**Correction.** Use the precedent's naming as well as its vetting —
`folio-<uid>`, or the platform's per-user temp on Windows, where
`GetTempPath2W` already is per-account — rather than a fixed `Folio`. State the
`/tmp` fallback explicitly instead of asserting `$TMPDIR`. If a fixed name is
kept for the `%TEMP%\Folio` symmetry the PRIVACY text wants (D:927), say what a
reader does when the vetting refuses; a permanent refusal with no remedy is not
"fail closed", it is fail-forever.

### 10. Should-fix — T-PASTE-1's scope sentence contradicts ledger row 23, and its PROBE 3 gate contradicts §2.3's table

**Evidence.** D:1249–1251 (T-PASTE-1): "**All of §6.1 except the picture, file,
sweep and job-identity rows**". §6.1's i18n row is D:1119–1121: "The new toast
strings, the setting's two lines, the `Text::ALL` count (660 today)" — verified,
`i18n.rs:4852` is `pub const ALL: [Self; 660] = [`. Ledger row 23 (D:1402) says
"The picture and **i18n** rows move to T-PASTE-2." D:1253–1256 (gates): "PROBE 3
answered **or the agent arm refused for the unmeasured recipients**", against
D:313, which gives all seven `AGENT_IDS` rows the `Agent` grammar.

**Trigger / breakage.** T-PASTE-1 adds toasts — D:1216–1220 lists at least six
refusals it ships — and therefore needs its own i18n row and its own `Text::ALL`
count bump; the ledger says that row moved away. And the gate, read literally,
means six of seven agent rows refuse every path paste when PROBE 3 is
unanswered: a product decision the grammar table does not carry and the "does
not ship" line does not mention.

**Correction.** Keep an i18n row in every ticket that adds a string, and say so
in T-PASTE-1's scope; correct ledger row 23 or the scope sentence, not both to
different answers. Restate the PROBE 3 gate as what it actually is — Codex is
the measured recipient, the other six inherit its spelling as the default, and
the gate is *measure, or record the inheritance as a known risk* — or, if
refusal is meant, put it in the table.

### 11. Should-fix — the nushell growing fence is asserted as a total function with no version-pinned citation, no probe, and two spellings

**Evidence.** D:308 (the table): Literal `r#'…'#`. D:388–390: "`r#'…'#` has no
escapes at all; the encoder chooses the smallest fence `r#…#'` whose closing
sequence does not occur in the path, which is a total function." §9.3's source is
"the Nushell book on strings and raw strings" (D:1350) — no URL, no version.
§6.1 tests it (D:1073, "the fence grows past a path containing `'#`"). There is
no PROBE for nu, while there is one (PROBE 2) for the same class of question in
PowerShell.

**Trigger / breakage.** Totality requires that nushell accept arbitrarily many
`#`s in the fence. If it accepts only one, the encoder has no answer for a path
containing `'#`, and the arm is not a total function — the same shape of error
the design has just withdrawn for fish ("reasoning from one sequence to a whole
encoder", D:386). Two spellings of the fence in one section (`r#'…'#` versus
`r#…#'`) is exactly the ambiguity a grammar spec cannot carry. The design also
does not say whether a raw string is accepted in *argument* position, which is
where every path this feature emits will land.

**Correction.** Pin the nushell version and cite the page and section for raw
strings; state the maximum fence width the language accepts and what the encoder
does at that width (refuse). Fix the fence spelling to one form. Fold it into
PROBE 2's shape — "which delimiters close a literal, and does doubling or
growing work" — or give nu its own probe; §6.2 already has a `nu` row to carry
it.

### 12. Should-fix — §3.1's Windows half names two winit call sites and misses the third COM owner on the same thread

**Evidence.** D:606–608: "Folio owns `OleInitialize`/`OleUninitialize` on the
event-loop thread, balanced over the process's life rather than per window."
`registry/winit-0.30.13/src/platform/windows.rs:493–496`, in the doc of the very
function the design calls: "Note that **winit may still attempt to initialize
COM API regardless of this option**. Currently only fullscreen mode does that,
but there may be more in the future." The mechanism is
`windows/window.rs:1432–1443`:

```rust
struct ComInitialized(...);
impl Drop for ComInitialized {
    fn drop(&mut self) { unsafe { CoUninitialize() }; }
}
thread_local! {
    static COM_INITIALIZED: ComInitialized = {
        unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32); ... }
    };
}
```

**Trigger / breakage.** Folio's `OleInitialize` and winit's `CoInitializeEx` are
both STA, so they nest rather than conflict — but the apartment's initialisation
count is now held by two owners with different lifetimes, and winit's is
released by a thread-local destructor at thread exit, in an order neither crate
controls. "Balanced over the process's life" is a claim about a count a second
crate also holds. `with_drag_and_drop(false)` does not remove this path; the doc
says so explicitly.

**Correction.** Name `platform/windows.rs:493` and `window.rs:1432` beside
`window.rs:1167` and `event_loop.rs:1262`, and state the ordering rule: Folio's
`OleInitialize` runs before any window is created, and its `OleUninitialize`
either runs before winit's thread-local destructor or is deliberately not run at
all in a process about to exit. Add the fullscreen path to §6.2's drop matrix —
enter and leave fullscreen with a drop target registered.

### 13. Should-fix — §3.4's admitted-type matrix has no column for two of the four landings, and calls the edge verb by the wrong name

**Evidence.** §3.4's matrix (D:697–706) has two target columns: "Terminal
centre" and "Preview centre / edge / rim". §3.2's table (D:624–630) has four
rows of landing, including "**centre** of a Files column". `SeatKind` has four
variants — `crates/bt-layout/src/tree.rs:26–36`: `Terminal`, `Files`, `Preview`,
`Placeholder`. §3.4's file-URL row says the preview/edge/rim cell is "open the
one file", while §3.2 gives the edge and the rim `Split` — "a new pane holding
it".

**Trigger / breakage.** A picture dragged out of a browser onto a **Files
column's centre**, or onto a **Placeholder** pane, has no stated result; §3.3
and red line 11 require every rectangle under a held payload to have a verb
decided *before* the highlight. "Open the one file" on a rim is not what the rim
does; a reader following §3.4 would implement `Retarget` where §3.2 says
`Split`, which is a different pane count.

**Correction.** Give §3.4's matrix the same four landings §3.2 has, plus
`Placeholder`, and name each cell with §3.2's own verb (`Split` / `Retarget` /
`Refused` / `Insert`) rather than a paraphrase. State the `Placeholder` answer
explicitly — `Refused` is fine, silence is not, because `row_verb`'s
`_ => RowVerb::Refused` arm (`main.rs:28996`) is where it currently falls, and a
reader should not have to find that out from the code.

### 14. Should-fix — ledger row 11 says PROBE 1 covers WSLg and RDP formats; PROBE 1's own list does not

**Evidence.** Ledger row 11, D:1389: "WSL/RDP bridged formats are covered by
PROBE 1's 'record what is advertised'". PROBE 1's source list, D:1130–1137:
"Explorer one file / three files / a folder / *Copy as path*; Finder one file /
three files; Excel a cell range and copy-as-picture; Word text and an image; a
browser text selection and *Copy image*; Snipping Tool; `Win+Shift+S`; `⌃⇧⌘4`;
and a screenshot tool configured to offer a path beside the picture." No WSLg,
no RDP, no `clip.exe`. The first review's correction (R:71) asked for "Record
host-visible WSL formats".

**Trigger / breakage.** A bridged clipboard is the one place where the advertised
type list is *synthesised* by a bridge rather than by the copying application,
and it is where a file list and a text list most plausibly disagree — the exact
input rulings ① and ② are about. The ledger claims coverage the matrix does not
give, so an implementer reading the ledger will believe the case is booked.

**Correction.** Add the rows — a file copied in a WSLg GUI file manager,
`clip.exe` text from a WSL shell, and a file and an image copied inside an RDP
session, each with the bridge's version recorded. Or strike the claim from the
ledger and book it as a named debt in §9.2.

### 15. Should-fix — nothing says what happens when a written picture's *own* path fails the representability gate

**Evidence.** D:439–441: "The gate runs **before** the spelling and the grammar
… and is the same gate for a paste, a drop, **a picture's own path** and K144."
D:913–921 (§4.4): "once the file exists it is an ordinary file and takes §2
whole". The deletions in §4.2 are scoped to cancellation (D:873–875, "a
cancelled job's file is deleted through the owned path") and to write failure
(D:847, "A write, flush or close error deletes the partial file").

**Trigger / breakage.** A `%TEMP%` or `$TMPDIR` containing a byte sequence that
is not valid UTF-8 — legal on macOS, reachable on Windows through a redirected
`%TEMP%` with an unpaired surrogate — makes the gate refuse the very path the
job just created a file for. The reader gets a refusal toast, the picture is on
the disk, nothing points at it, and it survives until the age sweep. That is a
write the §4.6 switch was supposed to be the only cause of, happening after a
refusal.

**Correction.** Run the gate on the *directory* once, at §4.1's vetting, and
refuse the whole lane there with a toast saying the temp directory cannot be
spelled — before any picture is written. State that a job whose path fails the
gate deletes its own file through the owned path, the way a cancellation does.

### 16. Should-fix — the hourly sweep has no thread and no hang station

**Evidence.** D:885–887: "Files are removed when they are older than **seven
days**, checked at startup **and hourly** while a Folio runs". D:766–769: "the
vetting runs before each create and before each sweep". Nothing in §4.3 or §5
says where the sweep runs. Every other synchronous filesystem or cross-process
call on this event loop carries a `hang_watch` station
(`hang_watch.rs:280–310`).

**Trigger / breakage.** A sweep is a directory listing plus per-level
`symlink_metadata` plus up to N unlinks, against a `%TEMP%` that may be
redirected to a network share — the design discloses exactly that at D:941–943.
On the event-loop thread that is a stall at an arbitrary moment once an hour,
with no station to name it in a hang report. §5's crate map (D:979–993) places
"picture lane + job identity" in `main.rs` and says nothing about the sweep's
thread either.

**Correction.** Say which thread the sweep runs on — the picture job's worker is
the obvious owner — and give it a `hang_watch` station if any part of it runs on
the loop. Put the sweep in §5's map beside the picture lane.

### 17. Should-fix — PROBE 5 has no stated outcome if all three shapes fail

**Evidence.** D:568–586 orders three candidates: an application-owned
destination `NSView`, "a narrow upstream extension", and class-wide replacement
"only if neither works, and then with a per-window lifetime contract written
down". D:1290–1291: "**It opens with PROBE 5**, and the probe is bounded".
§9.1's PROBE 5 row (D:1315) says it blocks "all of T-PASTE-3's macOS half".

**Trigger / breakage.** The three candidates have real ways to fail: an overlay
view that must not take mouse events from the panes beneath it, against a
`CAMetalLayer` and embedded `WKWebView`s, is not obviously constructible;
upstream is not on Folio's clock; class-wide replacement may be judged
unacceptable, which is why it is third. "Blocks all of T-PASTE-3's macOS half"
is a consequence, not a plan — the ticket would be open with an unspecified
deliverable.

**Correction.** State the fallback as a shipped behaviour: if PROBE 5 finds no
acceptable shape, T-PASTE-3 ships **Windows only**, macOS drops stay as they are
today (nothing), and the README and features text say so in both languages —
the same "state the lane it does not ship" rule §8 applies to every other ticket
(D:1237–1239). A probe whose failure has no product answer is an open end, not a
bounded probe.

### 18. Should-fix — PROBE 2's fallback is not operational, and two probes are not real unknowns

**Evidence.** D:340–345: "**PROBE 2** … Where (b) is false for a code point, a
path containing it is **refused visibly** in a PowerShell pane". D:1253: "PROBE 2
answered **or the affected PowerShell paths refused**". D:1310 (PROBE 1):
"Blocks — ruling ②". D:1318 (PROBE 9): "can a child of a Folio row read
`$TMPDIR`; any TCC prompt on these paths", against D:1224–1226, "The Mac package
is **not sandboxed** (`packaging/macos/entitlements.plist:20`)" — verified, the
file says "Not sandboxed. Developer ID distribution outside the App Store".

**Trigger / breakage.** (a) "The affected code points" is PROBE 2's *output*; an
unprobed build cannot know which set to refuse, so the fallback has no
definition. (b) PROBE 1 blocks nothing: ruling ② is implemented the same way
whatever the matrix says — it is a measurement to be recorded, and calling it a
blocker for a ruling devalues the word in a document whose §9.1 header is "no
ticket may assume their answer". (c) PROBE 9's first half is close to settled:
an unsandboxed process's children inherit `TMPDIR`, `/var/folders/<x>/<y>/T` is
per-user and is not a TCC-protected location, and the design already relies on
exactly that for its per-run socket (D:737–739 cites `PRIVACY.md`'s
"Elsewhere"). Presenting it as an unknown hides that the design is already
depending on the answer.

**Correction.** (a) Write PROBE 2's fallback as a concrete candidate set — the
Unicode quotation marks the design already names, plus `U+0027` — and say that
an unprobed build refuses any path containing one of them. (b) Retitle PROBE 1
as what it is: a fixture matrix that must be *recorded* before T-PASTE-1 ships,
not a blocker on a ruling. (c) Narrow PROBE 9 to the part that is genuinely open
— whether any TCC prompt appears on the paths this feature touches when the
picture lane writes and a child reads — and state the child-read case as the
fact the socket already depends on.

### 19. Nit — the document's own scope paragraph points at the wrong section

**Evidence.** D:26–28: "Nine things are marked **PROBE** and no ticket may
assume their answer: they are collected in **§9.2**." §9.2 is "Named debts"
(D:1324); the probe table is §9.1, "What still needs a machine" (D:1307).

**Correction.** §9.1.

### 20. Nit — §2.4's quotation of `sanitize_paste` is inexact in a section whose point is exactness

**Evidence.** D:425–428: "`sanitize_paste` deletes controls and turns LF into CR
(`input.rs:711`): `'\n' => normalized.push('\r')`, `character if
!character.is_control() => push`, everything else dropped." The function's actual
arms (`input.rs:718–728`) are `'\r' => { … normalized.push('\r'); }` (719–724,
which *collapses* a following LF and keeps the CR), `'\n' =>
normalized.push('\r')` (725), **`'\t' => normalized.push('\t')` (726)**, then the
non-control arm (727) and `_ => {}` (728). TAB is kept, not dropped.

**Trigger / breakage.** The conclusion survives — §2.4 refuses TAB at the gate,
so the sanitiser never sees one — but §5.1 rests "the sanitiser is a no-op on our
text" (D:1012–1013) on this quotation, and a ticket told to "check the claim
before it trusts it" (D:7–9) will find the quotation does not match the code.

**Correction.** Quote the five arms as they are, and say explicitly that TAB is
*kept* by the sanitiser and refused by the gate — which is a stronger argument
for the gate, not a weaker one.

### 21. Nit — §3.1 understates where winit discards the drop point

**Evidence.** D:537–539: "`platform_impl/windows/drop_handler.rs` takes `_pt:
*const POINTL` in `DragOver` and in `Drop` and uses neither." The file discards
it in three places: `DragEnter` at `:83`, `DragOver` at `:109`, `Drop` at `:137`.

**Correction.** "in `DragEnter`, `DragOver` and `Drop`" — it strengthens the
point, since the entry callback has no coordinate either.

### 22. Nit — ledger row 24 describes a correction the body does not contain

**Evidence.** D:1405: "the `usershell` anchor is corrected to `profiles.rs:1512`
(`1541` is the zsh seed)". `grep -n '1541\|1512'
docs/plans/paste-paths-design.md` returns only that ledger line: the body no
longer cites a `usershell` anchor at all, because the sentence carrying it was
rewritten away by ruling ⑦. (`USER_SHELL_ID` is indeed at `profiles.rs:1512`.)

**Correction.** Say "the sentence carrying the wrong anchor was removed" rather
than "the anchor is corrected"; a reader checking the ledger against the body
will not find `1512` and will not know whether the row is stale.

### 23. Nit — "`std::env::temp_dir()` … is `GetTempPath2W` on Windows" is a std implementation detail asserted without a version

**Evidence.** D:736–738.

**Correction.** Either pin the Rust version in which `std` uses `GetTempPath2W`,
or say what the design actually needs — a per-account temp directory the platform
chose — and stop naming the API. The distinction matters only for a
SYSTEM-token process, which Folio is not, so the sentence buys nothing and can
go stale.

## PROBE audit

| | Real unknown? | Fallback if it fails? | Verdict |
| --- | --- | --- | --- |
| 1 | Partly — the formats are unknown, but no ruling turns on the answer | n/a | **Mislabelled** (finding 18b) |
| 2 | Yes | Stated but not operational | **Fix the fallback** (finding 18a) |
| 3 | Yes | Stated in §8, contradicted by §2.3's table | **Fix the contradiction** (finding 10); and the arm it defends is wrong (finding 4) |
| 4 | Yes | Yes — forward-slash spelling through `paste_as` | Sound |
| 5 | Yes | **None if all three shapes fail** | **Add one** (finding 17) |
| 6 | Yes | Yes — "possibly a refusal" | Sound |
| 7 | Yes | Yes — refuse the layout | Sound |
| 8 | Yes, but unanswerable as posed | The bound it feeds has no mechanism | **Rethink** (finding 1) |
| 9 | Half — the child-read half is already relied on | Yes — refusal, no fallback copy | **Narrow** (finding 18c) |

**Presented as fact that should be a probe:** the nushell growing fence
(finding 11); the claim that the quote-strip branch of Codex's parser does not
win for a quoted POSIX path (finding 4); and, mildly, `std::env::temp_dir()`'s
Windows implementation (finding 23).

**Presented as a probe that is close to fact:** PROBE 9's child-read half
(finding 18c), and PROBE 1's status as a blocker (finding 18b).

## What was verified and found correct

Recorded so the next pass does not re-do it. Every `path:line` in the revised
design was opened. All of the following match: `lib.rs` 5925 / 5940 / 11358;
`macos_services.rs:181`; `macos_files.rs:33`; `instance.rs:360`;
`launch_pipe_unix.rs:470`; `macos_impl.rs:613` (`pointer_position`); `main.rs`
8824 / 8840 / 8841 / 8861 / 76921 / 96257 / 96290 / 96312 / 110689 / 110700 /
73388 / 88732 / 28995 / 29555 / 60428 / 47228 / 152531; `input.rs` 299 / 696 /
711; `profiles.rs` 799 / 3201 / 3219; `shell_integration.rs:248`; `paths.rs` 61
/ 134 / 218 / 235; root `Cargo.toml:107`; `crates/bt-app/Cargo.toml:64`;
`CONVENTIONS.md` 34 and 154; `PRIVACY.md:186`; `README.md:87`;
`entitlements.plist:20`; `i18n.rs:4852` (`ALL: [Self; 660]`); `DESIGN.md`
§7.1.5f and §7.1.5k both exist; `image-0.25.10` `decoder.rs:534` and `:752`, and
`Cargo.toml:120` for the `tiff` feature; winit 0.30.13
`macos/window_delegate.rs:367` and `:666`, `windows/window.rs:1167`,
`windows/event_loop.rs:1262`, `windows/drop_handler.rs`, `platform/windows.rs:497`.
Rulings ①–㉛ are numbered without a gap or a duplicate.

## Remaining release gates

Unchanged from the first review, plus: the `cmd` builtin-versus-CRT rows, the
nushell fence width, the Codex quote-strip branch, a Cygwin row if one is
admitted, the WSLg and RDP clipboard rows, a fullscreen transition with a drop
target registered, and a cross-window quota race. None was executed here; this is
a docs-only pass, and an unrun probe is still not evidence either way.
