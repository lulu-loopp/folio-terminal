# Focus follows a dropped path: round 3, 2026-09-17

Reviewed `3ab18954` against round 2 (`10c1f10e`), including `a556d6e3`.
App references mean `crates/bt-app/src/main.rs` at the reviewed commit.
**Verdict: merge with must-fixes; do not merge unchanged into 0.4.2.**
Bar (a), retained-address drop protection: passes. Bar (b), keyboard/composition:
incomplete. Round-2 P2: **partly closed**.

`:18338` rejects a commit naming another shell; `:99265` applies that ruling.
Cancellation preserves the address (`:99407`); empty preedit preserves it too.
Bare stale Commit and empty-preedit-then-Commit are closed while the address
survives. Non-empty preedit re-homes at `:18329`: the stated bound remains.

**P2: lifecycle notifications erase the barrier (`:99284`, `:99332`).**
Attack: preedit in A; drop/focus B; refused cancel; Disabled; Enabled;
empty preedit; stale Commit. Both notifications set `shell_composing = None`,
so `:18338` admits the commit and `:99317` writes to B. No non-empty preedit
at B is needed. Preserve stale ownership across this sequence, or establish
and test a backend guarantee that old text cannot cross that boundary.
This is not a native reproduction. Pinned winit 0.30.13
`src/platform_impl/windows/event_loop.rs:1587` emits
an end-composition result BEFORE Disabled; `:1530` rejects results while
disabled, but `:1513` re-enables them on STARTCOMPOSITION. Consequently an
ordinary end/blur alone does not prove this attack; the intervening Enabled
is essential. App Focused(false/true) itself does not clear the address.
Tab switch (`:39543`), pane close (`:54525`), and restart (`:75874`) do not
clear it either; tab/seat/incarnation comparisons reject changed destinations.

**P2: the owner ladder is routing, not a stale-commit barrier (`:99180`).**
Shell-to-editable-Preview bypasses the ruling at `:99232`, then inserts at
`:61197`; Shell-to-Search inserts at `:77296`. Conversely, start a composition
in Preview/Search with no prior shell composition, then drop a path into B:
even after pass-tail cancellation (`:99367`), shell ownership is still None,
and the refused-cancel commit reaches B. The ladder does not close either
direction; `composing` is cleared before routing (`:99176`). Carry/check the
origin across non-shell owners too, with regression coverage for both moves.

After the cancelled-shell handoff, discarding clears both ownership records
(`:99176`, `:99271`) without writing. Cancellation already cleared preedit,
reset the cursor throttle and destroyed the system caret (`:99408`). The next
non-empty preedit establishes B and Shell again; caret publication follows the
current owner (`:61413`, terminal frame `:66322`). No next-composition state
regression found in that sequence; native candidate placement was not exercised.
macOS cancellation calls discardMarkedText then client.unmarkText
(`crates/bt-platform/src/lib.rs:11376`). Winit `src/platform_impl/macos/view.rs:338`
clears marked text, queues empty preedit and enters Ground, preserving our
shell address; `:411` requires marked text before emitting a Commit. Thus a
plain post-unmark insertText emits no Commit; an already queued shell Commit
is guarded as above. The new test's macOS comment overstates that native path.

`git diff 10c1f10e 3ab18954 --stat`: two files, 379 insertions, 25 deletions
(main.rs 400 changed lines; DESIGN.md 4). All app hunks are IME state/routing,
IME tests, or the clippy table refactor. Drop/paste roads are unchanged.
Validation: `cargo test -p bt-app --bin folio <filter> -j 4` only; all passed:
`ime` 61, `focus` 114, `drop` 98; additionally `a_composition_left_behind` 1
(the new regression is missed by the requested filters). 274 test executions,
zero failures. Product code stayed read-only; no scratch tests, application
launch or process termination. Lifecycle/owner counterexamples are source traces.
