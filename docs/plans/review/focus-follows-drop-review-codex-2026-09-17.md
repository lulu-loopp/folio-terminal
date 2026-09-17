# Focus follows a dropped path — narrow review, 2026-09-17

Reviewed `a5938a228f1a949ffb6e6f8fda360c6ee6a61c7c` against `d1db4abd`.
All references are at a5938a22; `main.rs` means `crates/bt-app/src/main.rs`.
**Verdict: merge with must-fixes; do not merge this commit unchanged into 0.4.2.**
Bar (a): the internal drag's addressed-write protection is preserved; the external
modal/covered-point fallback below prevents an unconditional all-drops sign-off.
Bar (b): fails even after a successful write and successful native window activation.

## Must-fixes

**P1 — Moving the seat does not necessarily take the keyboard.**
`main.rs:54889` clears only files focus; `:54859` changes the shell/tree focus.
Keep a preview float focused, then drop an external file on an uncovered terminal.
The path reaches that terminal, but `:55662` still finds the focused float,
`:55688` still selects it, and `:98331` routes the next key to the preview.
A live preview editor (`:60424`) and focused search field (`:98343`) also survive.
The ordinary pointer-focus road explicitly blurs floats (`:54835`); this road does not.
Fix: relinquish the competing nonmodal keyboard owners after an accepted write,
and test the actual next Enter/argument routing, including a drop onto the current seat.

**P1 — A modal can keep a drop, raise the window, and take its next Enter.**
`main.rs:114014` blocks retiring quit state, not the asking quit card.
Collection (`:96666`) and flush (`:96599`) have no quit/settings/dirty-gate refusal.
The hit test (`:96723`) checks floats and rails, not these modal surfaces.
Thus a drop over a modal can write into the terminal geometrically underneath it;
the modal remains, and Enter answers quit (`:97581`) or settings (`:97649`).
The write admission is inherited; the new focus/raise does not repair it.
Fix: refuse modal-covered drops before writing, rechecking admission at flush.
Also preserve a truthful aim: the inherited covered/chrome/missing-point fallback
at `:114705` selects `focused_leaf`, contrary to the no-shell comment at `:96625`.

**P2 — `true` does not mean that a PTY accepted bytes.**
`main.rs:98678` calls `write_pty_input`; `:18618` returns success for no PTY and
`:18624` logs `PtyError::InputRefused` then also returns success. A backed-up input
ring can refuse an entire path (`crates/bt-pty/src/lib.rs:1178`), yet `:98696`
reports `true`: both drop roads move focus, and external drops raise the window.
This is newly observable through the bool, although the swallowing wrapper predates it.
Fix: propagate accepted/refused status to drop callers; preserve clipboard behavior.
Acceptance means queued input, not proof that the child has consumed it.

**P2 — A live shell preedit is not settled by a shell-to-shell focus move.**
`main.rs:54859` neither commits nor cancels it. `composing` records only an
`ImeOwner` category (`:98864`), and `:18211` considers Shell-to-Shell unchanged.
Even the turn-tail settlement (`:102637`) therefore keeps that composition;
a subsequent IME commit writes through the newly focused shell (`:98977`).
This is an inherited focus-helper limitation exposed by the new caller, not a native
reproduction. Fix: settle composition using the old pane identity before changing
keyboard ownership; test a live preedit and a commit immediately after batch flush.

## Checks that passed, and limits

The complete `paste_offer_kept` (`main.rs:90569`) and `live_paste_target` (`:98630`)
method bytes are identical to d1db4abd. Internal order is retained address (`:90650`),
addressed write (`:90663`), then focus (`:90664`). The write indexes the captured
active tab and target seat (`:98674`), never a focus-selected replacement PTY.
The live check requires the target tab/incarnation at write time. Through the
successful write's bookkeeping/repaint, files-focus cleanup and `focus_seat`, no
event dispatch, await, tab switch or handover occurs. The bare seat still names
that same active tab at focus time; no same-numbered seat in another tab is reached.
Native foreground activation follows this sequence, not the address or the write.

All actual `Ok(false)` exits (`:98586`, `:98654`, `:98666`, `:98674`) skip both
focus and raise. Unspellable paths can show the refusal toast (`:98658`) only.
Post-write presentation errors still propagate before focus; bool is not a durable
delivery receipt (`:98695`, `:98702`). K144 (`:78185`) is byte-identical and uses
`paste_text` directly. Keyboard clipboard paste ignores the bool (`:98534`);
clipboard-picture completion discards it with `map(drop)` (`:98831`): unchanged.

Two windows cannot exchange batches: event WindowId selects its runtime (`:114031`,
`:110324`), collection stores in that window (`:96680`), and flush takes that same
window's batch (`:96600`). The receiving window's native handle is raised (`:54925`).
Internal row drags leaving Folio use the broker, not native file export: Away refuses
(`:30568`), and another window receives only strip operations (`:112639`). They do
not re-enter `DroppedFile`; a native file payload from any source would use the same
receiving-window path, with the same findings above, not a source-window raise.
The raise is once per batch and restores minimization first (`:54921`). Windows
reads foreground back (`crates/bt-platform/src/hotkey.rs:1389`); macOS activates the
app and makes the window key (`:1932`). Failure is merely logged (`main.rs:54928`),
so an absolute OS-wide next-keystroke guarantee remains best effort even after fixes.
Attention is already answered by paste (`:98684`): clearing the waiting dot is
acceptable when the user actually arrives. Files focus/ring clear together
(`:81737`, `:10357`); the retained row selection is not a leftover keyboard ring.

Validation at a5938a22: all eight `cargo test -p bt-app --bin folio <filter> -j 4` commands passed: **229 test executions, 0 failures**.
Round-3 filters: `clipboard_path_tests` (12), `row_splits` (2), `a_rows_centre_says` (1), `a_pane_offers_one_middle_and_four_bands_at_every_scale` (1), `a_content_drop_into_a_window_below_its_minimum_is_refused` (1), `a_stale_aim_is_refused_however_it_went_stale` (1).
Additional filters: `focus` (114), `drop` (97). Passing source/pure tests do not establish native event behavior or close the findings above.
Product code stayed read-only. No application launched, no process terminated,
no scratch tests added, and no broader test suite or heavier command run.
