# X-3 — Command, Option and IME routing

*2026-09-12. Ticket X-3 of `docs/plans/port/macos-plan-2026-09-12.md`, branch
`probe/macos-x3`. Run on the Mac mini in `~/folio-port/wt/x3` at `origin/main`
`4741ab9`, on the second build lane (`CARGO_TARGET_DIR=~/folio-port/target-x3`,
`nice -n 10 … -j 4`, debug only). Keys were posted with `CGEventPost` from a small
Swift helper, only after `NSWorkspace.frontmostApplication` named our own pid.*

## Verdict

**PASS on §3's wording — every cell has one answer, and §4 is the routing rule
that covers all of them.** No cell has two plausible answers: the one product
question, Option-as-text versus Option-as-Alt, was ruled in Q9, and the measurement
says only *how* to implement it. The matrix is nevertheless almost entirely red,
which is the point: M1-7 and M1-8 are unwritten, so a Mac runs the Windows dialect.

## The matrix, as it behaves today

The shell pane's program was `stty raw -echo; exec cat -v`, so every byte Folio
wrote to the child came back in caret notation and nothing else did; a second run
used an interactive `zsh` for the signals. Evidence: `BT_PTY_DUMP`, `BT_IME_TRACE`,
`screencapture`.

| Chord | Shell pane | Markdown being edited | Command palette | Focused web page |
|---|---|---|---|---|
| `⌘C` | nothing at all | types `c` | types `c` | NOT-CHECKABLE |
| `⌘V` | nothing at all | types `v` | types `v` | NOT-CHECKABLE |
| `⌘X` | nothing at all | types `x` | (as `⌘C`) | NOT-CHECKABLE |
| `⌘A` | nothing at all | types `a` | types `a` | NOT-CHECKABLE |
| `⌃C` | `0x03` | — | — | NOT-CHECKABLE |
| `⌃D` | `0x04`; EOF ends `cat` | — | — | NOT-CHECKABLE |
| `⌃Z` | `0x1a`; `zsh: suspended` | — | — | NOT-CHECKABLE |
| `⌘T` / `⌘W` / `⌘,` | nothing | nothing | nothing | NOT-CHECKABLE |
| `⌘Q` | **quits the process** | same | same | same |
| `⌥a` (Option as text) | `ESC` + `å` | nothing | nothing | NOT-CHECKABLE |
| `⌥e` then `e` | `é`, once | — | — | NOT-CHECKABLE |
| German `ü ä ö ß` | **nothing reaches the child** | — | — | NOT-CHECKABLE |
| Pinyin preedit | drawn at the caret | not drawn | — | NOT-CHECKABLE |
| Pinyin commit | `你好`, once | — | — | NOT-CHECKABLE |

The Windows dialect works unchanged here, the control for every red cell:
`Ctrl+Shift+V` pasted the pasteboard into the pane (M1-9's `NSPasteboard` door is
live), `Ctrl+Shift+N` opened a tab, `Ctrl+Shift+P` the palette, `Ctrl+Shift+B` the
files column, `Ctrl+F` the search capsule, and `Ctrl+S` saved the edited Markdown
page — its md5 changed — while `⌘S` only typed an `s` into it.

**Control is already the terminal's and needs nothing.** In a cooked shell,
`sleep 30` then `⌃C` printed `^C` and `OSC 133;D;130`; `⌃Z` printed
`zsh: suspended  sleep 30` and `D;146`; `⌃D` ended `cat` with `D;0`. Under German,
`Ctrl` follows the layout's **logical** letter rather than the physical key — the
key U.S. calls `z` gave `0x19`, the key it calls `y` gave `0x1a` — which is what
`control_byte` and xterm both do.

**The focused web page is NOT-CHECKABLE until M4-2**: no `WKWebView` host exists
here. Filled by reading: `webhost::claimable_chords` writes `command: false` into
every `WebChord` and `claim_for` matches on exact equality, so no Command chord can
ever be claimed back from a page; and X-2's twenty-requirement matrix contains no
keyboard hook at all, because WebKit has no `AcceleratorKeyPressed`.

## The routing rule M1-7 implements

**① Command is the application's, Control is the terminal's, and no verb wears
both.** `BINDINGS` keeps one row per verb and its third column stays `Scope`
(*where it works*); the dialect is a **new** column — one `Chord` per platform per
row — walked by `docs/shortcuts.md`'s generator, §4.7's twin rather than a fork.
One sentence generates the macOS dialect: a row whose Windows chord is
`Ctrl+Shift+<letter>` or `Ctrl+<punctuation>` becomes `Cmd+<letter>` (`Cmd+T`,
`Cmd+W`, `Cmd+N`, `Cmd+,`, `Cmd+P`, `Cmd+1…9`), and a bare `Ctrl+<letter>` in a
`Preview` scope — `Ctrl+S`, `Ctrl+Z`, `Ctrl+Y`, `Ctrl+F`, `Ctrl+L` — becomes
`Cmd+<letter>` and **stops being a control letter**. Discipline ① is retired here
by construction: Command never had a control code to take.

**② The clipboard predicates fix two cells at once.** `is_copy_shortcut` and
`is_paste_shortcut` (`crates/bt-app/src/input.rs`) are Control predicates with an
`Insert` arm. On macOS they test `super_key()` and drop the `Insert` pair, which no
Apple keyboard has; and `should_copy_selection`'s "Shift forces it" rule is
unnecessary there, because `Cmd+C` is not a control code and may copy
unconditionally while `Ctrl+C` stays an interrupt, selection or none.

**③ Every one-line field must test Super.** The loudest defect here, and not
macOS-specific. Six insert sites — the palette (`main.rs` `palette_key`), the
search capsule, the git branch prompt, the commit graph's search, the tab rename
and the settings field — guard `ctrl` and `alt` and never `super`, so a Command
chord types its letter into whatever box holds the caret; three were measured
(`helcv`, `abcv`, `Englixcvxsa…`). `keyboard_bytes` and the preview player's rung
already carry `!modifiers.super_key()`; M1-7 writes that predicate once.

**④ Option is text by default, and winit owns the switch.** macOS hands Folio
*both* halves today: winit reports the composed character **and** `alt_key()`, so
`meta_prefix` prepends `ESC` and the child gets `ESC å` (`1b c3 a5`) — neither
policy. M1-7 sets `OptionAsAlt::None` at both window constructors and exposes the
setting (`None` / `OnlyLeft` / `OnlyRight` / `Both`) through
`Window::set_option_as_alt`, so Option means something in one place. Dead keys need
nothing: `⌥e e` arrived as `Ime::Preedit("´")` then `Ime::Commit("é")` and reached
the pty as three exact UTF-8 bytes.

**⑤ Drop the `is_ascii()` half of the character arm.** `keyboard_bytes`'s
`(text.is_ascii() || modifiers.alt_key())` guard swallowed `ü ä ö ß` on a German
layout — zero bytes each — while `⌥q` there produced `ESC «` only because Alt
happened to be held. The guard exists to stop a composed CJK character being typed
twice, but on macOS a plain layout key is a `KeyboardInput` with text and *no*
`Ime` event, so what must gate the arm is the live composition, not the code point.
The `中` case in `main.rs` encodes the old policy and is the test M1-7 rewrites.

**⑥ `WebChord::command` is filled from the row's macOS chord**, and the host side
has no accelerator callback to fill it from: a `WKWebView` is an `NSView` in the
responder chain, so the window takes its chords back at `performKeyEquivalent:` on
the hosting view — every Command chord and nothing else, which turns W0′'s hazard
into a guarantee. M4-2 owns the call site, M1-7 the conversion.

**⑦ `⌘Q` is the one cell that needs M3-2 too.** It already ends the process — winit
installs AppKit's default menu — but as `NSApplication terminate:` rather than as
Folio's `quit` verb. `Cmd+Q` is the `quit` row and the menu item dispatches it,
under X-4's rule about `terminate:` inside an `ApplicationHandler` callback.

## The IME findings M1-8 implements

**Preedit placement on a terminal is already right at backing scale 2.** With the
window at `515,135,960,600` points, `ni hao` was drawn underlined at the pane's
caret and the candidate list (`1 你好 2 👋 3 你好吗 …`) stood immediately beneath
that line. winit's `firstRectForCharacterRange:` reads `set_ime_cursor_area`, so
`apply_ime_cursor_area` needs no macOS arm and `ImeSystemCaret` has nothing to do
there — as `portable_impl.rs`'s own note predicts. M1-8 deletes that door here.

**The preview seat does not publish a caret.** Composing into the Markdown page put
the candidate window at the **window's top-left corner** and drew no preedit at the
caret. `ime_caret_source(ImeOwner::Preview)` already answers `Field` and
`preview_ime_cursor_area` already exists, so this is a call-order question on the
macOS path, not a missing design.

**Cancellation is clean**: `Esc` mid-composition gave `Ime::Preedit("")`, no
`Commit`, no byte to the child, nothing left on the glass.

**There is no duplicate commit.** `你好` arrived as six bytes (`e4 bd a0 e5 a5 bd`)
exactly once, `é` as three; the winit-on-macOS hazard did not reproduce on 0.30.13
/ macOS 26.6. The detector to keep is this probe's pair: a duplicate is an
`Ime::Commit(t)` followed by a `KeyboardInput` whose text is `t`, with the payload
twice in `BT_PTY_DUMP`. M1-8 carries a case asserting one write per commit.

**The one real failure is a focus change mid-composition.** `Ctrl+Shift+P` over a
live composition committed the **raw reading** `nihao` into the shell and the
palette never opened — only the key-hint card came up, so the method had eaten the
letter and passed the modifiers through. §7.1.5a″ says cancel, never commit
elsewhere, and Folio cannot obey: the keyboard owner never changed, so
`settle_composition_owner` never ran, and `bt_platform::cancel_composition()`
answers `false` off Windows. M1-8 gives it a macOS arm — `discardMarkedText` on the
view's `NSTextInputContext`, `unmarkText` on the view — called **before** the chord
is dispatched: by the time the application sees the key, the method has committed.

## Not checkable, and two venue facts

Only the focused web page. `TISSelectInputSource` was allowed: German was enabled,
selected, measured, deselected and **disabled again**, and the enabled-source list
at the end is identical to the one recorded at the start.

`open` cannot set the environment of a bundle already running — it activates the
running copy instead — so two runs of one bundle may never overlap; one was lost to
this and repeated. And a Folio from **another ticket's** worktree takes the
keyboard: M3-3's appeared mid-run, and one IME measurement was redone with the
frontmost pid asserted before every posting.

## Numbers, and the process ledger

`cargo build -p bt-app` (debug, cold `target-x3`, `-j 4`): **79.85 s wall, 244.16 s
user, peak RSS 2.187 GB**, binary 231,339,064 bytes, target 3.5 GB — **deleted** at
the end; free space ended at 38 GiB.

**Input source**: recorded as `com.apple.keylayout.US` with
`TISCopyCurrentKeyboardInputSource` before anything was selected, and restored to
it. The pasteboard was empty at the start, held a fixture, and is empty again.

**Processes, and how each ended.** Five `folio` processes from this worktree's
bundle — 64692, 65416, 66467, 66843, 67189 — each ended by `kill` on the pid this
probe's own script wrote down (66467 ended itself on `⌘Q` first); one launcher
shell, 66304, the same way; the `swiftc` invocations exited on their own. Nothing
was matched by name or window title. M3-3's Folio (67736, later 68246) was not
touched, and no event was posted at any window that was not this bundle's own.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VNAfpT6VRgLihW5Fp3EU74
