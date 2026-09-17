# Focus follows a dropped path: round 4, 2026-09-17

Reviewed `31dda7bb` against `3ab18954` and the round-3 review.
App references are `crates/bt-app/src/main.rs`; backend references are pinned
winit 0.30.13 `src/platform_impl/`. Source traces, not native reproductions.
**Verdict: merge with must-fixes; do not merge unchanged into 0.4.2.**
Bar (a), retained-address drop protection: passes; drop/paste roads unchanged.
Bar (b), keyboard/composition: old attacks closed; ordinary-cancel safety unproven.

**Round-3 lifecycle P2: closed** (`:99374`, `:99429`, `:99507`).
Replay: preedit A -> move B/refused cancel -> Disabled -> Enabled -> empty
preedit -> stale Commit. Origin stays A; Clears keeps it (`:18393`);
Commits rejects and forgets it (`:18400`). Window focus changes do not erase it.
**Round-3 owner-ladder P2: closed** (`:99284`, before routing at `:99296`).
Shell->Preview, Shell->Search, Preview->Shell B, Search->Shell B all reject
both bare stale Commit and empty-preedit-then-Commit; non-empty B preedit
adopts B and its commit succeeds. The `ime` tests rerun all four and lifecycle.

**P2: an honoured cancel can leave the barrier armed (`:18393`, `:18400`).**
Exact loss trace: Opens(A) -> click B/cancel succeeds -> Clears at B = Keep(A)
-> no cancellation Commit -> Enabled if needed -> punctuation Commit at B
(even preceded by another empty preedit) = discard + Forget. The next works.
This needs no adversarial IME and DOES include an empty preedit: bound (2)'s
"neither" condition and backend exclusion (`:18372`) do not describe the rule.
Cancellation before assignment (`:55287`, `:55289`) does not compare at A:
Windows `event_loop/runner.rs:215,231` buffers callbacks until the app returns;
macOS `app_state.rs:306` likewise queues them. Delivery reads B at `:99284`.
Windows: empty preedit is not unconditional. `event_loop.rs:1537` handles zero
flags; `:1587` emits an end result only when state is Preedit AND retrieval
succeeds. `windows/ime.rs:90` maps size 0 to `Commit("")`, which safely retires
A; a negative result emits no Commit. [Microsoft documents missing data](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcompositionstringw),
and [CPS_CANCEL clears composition, unlike CPS_COMPLETE](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immnotifyime).
Thus the loss is proven for the stated event trace, NOT established as a
common MS Pinyin trace or a regression every Chinese-IME user hits.
macOS: platform `lib.rs:11376` explicitly unmarks; `view.rs:338` queues empty
preedit after the move, retaining A. But `view.rs:411` requires marked text
for Commit; ordinary unmarked typing uses KeyboardInput (`:490`), bypassing
this rule. A fresh non-empty preedit re-homes. No macOS punctuation loss proven.
Must fix: cover honoured-cancel-after-handoff plus punctuation, establish a
backend-backed retirement boundary (or prove the supported native sequence
always retires A), and correct the bound; lifecycle clearing alone reopens P2.

Identity audit: `composition_origin_now` (`:60977`) consistently reads stable
LeafId/float epoch (`:3740`, `:56038`), not geometry or strip index. Reflow is
read-only (`seats.rs:1333`); tab reorder preserves TabId (`:90760`); same-tab
pane swaps carry IDs (`seats.rs:50784`). Cross-tab moves re-key the moved pane
(`:35923`); a preview grip press leaves its editor (`:92426`). No identity
churn found in an unmoved field; actual relocation is an address change.
Validation: allowed cargo filters only: `ime` 66, `focus` 114, `drop` 98; 278 executions, zero failures. Product code read-only; no scratch tests, app launch or process termination.
