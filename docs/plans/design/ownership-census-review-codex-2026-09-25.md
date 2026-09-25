**Verdict: adopt with changes.** Adopt the investigation and the attention-first extraction direction. Do not adopt the census as an exhaustive ownership authority, the seven kinds as sufficient input-routing policy, or A–G as implementation-ready briefs. Move E out of the 0.4.6 sequence: the dirty-gate bypass is a confirmed code-path data-loss defect for 0.4.5.

Reviewed `docs/ownership-census` at `6e2a9ebbc5633a3a155b39f53d91d7d55a42f079`. The product-source diff from the note's `f7826bd4` baseline is empty. This was a static review: no build, test, GUI reproduction, or product modification. “Confirmed” below means the complete reachable source path establishes the consequence under the stated conditions; it does not claim a runtime experiment. Only this review file was written.

Read the design note and its TSV, all of `ARCHITECTURE.md` and `CONVENTIONS.md`, the requested structural-debt rows, the adversarial-review ledger, and the relevant declarations, runtime writers, attention implementation/adapters, input ladders, close and persistence paths. Code anchors below are symbols rather than unstable line numbers. Obligations R1–R7 are this review's identifiers; all remain open recommendations, not implemented repairs.

**P1 · R1 — An ordinary close bypasses an existing dirty gate and loses unsaved content; E must be a 0.4.5 defect ticket.**

Exact paragraphs: §4.1 V2, beginning “the dirty gate answers ‘busy’ with the word for ‘nothing to ask’,” especially “the path through `FolioApp::close` to the loss of the dirty buffer is not traced to the end”; §6's opening “All 0.4.6”; and ticket E's “True on BASE (static).”

**Definitive answer: yes.** A normal window containing an unsaved preview can close while its unsaved-changes gate is up, without receiving an answer, and the edited buffer is subsequently dropped. Neither the session snapshot nor Recent preserves its content. This is separate from the adversarial ledger's D2-2 failure/panic road: no error or crash is needed.

The complete sequence is:

1. In an ordinary, non-leaving window, edit a file-backed preview without saving. No application quit transaction is active. Ask to close its tab. `runtime/tabs::Runtime::close_tab` calls `raise_dirty_gate(GateRequest::CloseTab(index))` before removing anything. `Runtime::gate_dirty_names` reads that tab's dirty pool; `DirtyGate::open` stores the request; `raise_dirty_gate` returns `Ok(true)`. The tab and dirty buffer remain.
2. Without answering the gate, request that window's close through the OS, for example the taskbar's Close window. `FolioApp::window_event` accepts this existing window. Its early exits concern an absent window, application retirement, a leaving window, or failures flushing pending wheel/drop work. None rejects a close because `dirty_gate` is open. Choose a quiescent window with no pending wheel/drop work. The visible quake-window arm is inapplicable to this ordinary window.
3. The `WindowEvent::CloseRequested` arm calls `raise_dirty_gate(GateRequest::Shut)`. Its **first** branch sees the old open gate and returns `Ok(false)`, before even asking which buffers a Shut would lose. The handler sets `shutting = true`. The event's tail calls `self.close(window_id)`.
4. `FolioApp::close` checks existence, whether the window is already leaving, and whether `app.quit` exists. All three allow this case. There is **no second dirty-buffer gate**. It determines whether this is the last visible window and calls `Runtime::close_window(ending)`.
5. `close_window` finishes a rename, then either calls `mark_session_dirty` for the ending run or `vault_this_window` and `App::forget_window` for a nonfinal window. It does not call `quit_save`, `PreviewPool::save_dirty`, or a recovery-content writer. It calls `let_go_of_this_window`, which hides/tears down native resources and retires shells. `close` marks the window `leaving`; for the ending run it also calls `App::finish`.
6. `FolioApp::reap_leaving_windows` removes the window when its pages are gone or the page teardown deadline is reached. Removing its `WindowRuntime` drops the tabs, preview pools and edited buffers. With no remaining windows it exits the loop. A file-only reproduction needs no browser deadline to make progress.
7. Reopening cannot recover these edits: `TabState::preview_content` serializes pool entries as `path`, `name`, and `source`; `bt_persist::PreviewPoolEntryV1` contains no edited bytes. The nonfinal-window vault also records seeds/locations, not edited content. The original file still contains the last saved version.

An even shorter reproduction is two OS close requests: the first raises `GateRequest::Shut`, and the second takes the same erroneous busy branch. A gate opened for a git operation can expose the same close bypass if the window also contains dirty previews. A visible quake window's close hides it instead, and an active application quit makes `FolioApp::close` return; those exceptions narrow the finding, not refute it.

Fix: ticket E's explicit `Raised / NothingToAsk / Busy` result is appropriate. Only `NothingToAsk` authorizes proceeding; audit **every** caller, including non-keyboard callers. Keep the pending request on Busy. The gate's answer already calls `DirtyGate::take` before replaying the accepted operation, so it does not need the current “already open means proceed” exception. Add the proposed real-buffer/event-road regression, the two-close variant, Cancel preservation, and accepted Save/Discard replay tests. Check both final and nonfinal window retirement. Put E before the 0.4.5 tag, independently of A–D and the taxonomy ruling; add its defect-ledger entry in that implementation ticket. This review does not edit that ledger.

**P1 · R2 — The census has useful positive evidence, but its completeness and regeneration claims are unsound.**

Exact paragraphs: §1 Method items 2–3 (“every method name ... declares only with `&mut self`”; “153 ... stayed unresolved ... about ten real writes”), §1 “Limits, stated” (“None of these changes a fact from single- to multi-writer”), ticket A's “receiver typing exactly §1 item 3,” and §5.5's promise to read “only the listed writers.”

The four field counts are reproducible: **83 + 273 + 47 + 33 = 436**. The TSV contains **1,321 fact/module/function rows, 170 distinct facts and 1,702 counted sites**, and its class totals match the note. It contains **zero single-writer facts**. Thus it does not expose the claimed 223 single-writer facts, the other 43 fields, or all 2,099 sites. This is a selected multi-writer snapshot, not the complete backing census. The smaller site total is not itself an arithmetic contradiction; the missing population prevents reproducing the whole total and auditing the negatives.

The random check below found real omissions beyond the stated untyped-local problem:

- `Runtime::raise_dirty_gate` calls `self.window.dirty_gate.open(request)`. `DirtyGate::open(&mut self, ...)` changes both request and hover. The TSV has only `pointer_moved` and `answer_dirty_gate` for this field: the operation that opens it is absent.
- `Runtime::advance_rename_blink_if_due` calls `self.window.rename_blink.advance(now)`. `CursorBlink::advance(&mut self, ...)` changes visibility and its next deadline. Only the reset callers appear in that field's TSV rows.
- `FolioApp::transfer_tab` assigns `source.placeholder_tab = None` and removes `source.tabs[index]`; the former writer is absent entirely, and the latter row lists `push` and a lend but omits `remove`. These do fit the note's acknowledged local-resolution gap, but the missing-sites list is not supplied.

Method **spelling** is not receiver identity. For example `trace::Gate::open(&self)` and `DirtyGate::open(&mut self, ...)` coexist. Excluding a name because another type has a nonmutating method loses real writes on a fully explicit receiver. `AttentionLedger::apply` is another central mutator absent from `LeafSession.attention`'s write kinds. Conversely, `as_mut`, `get_mut` and `iter_mut` grant mutable access; their invocation does not establish a mutation. Calling them “mutating calls” while counting only explicit `&mut` syntax as lending makes the “148 other than lending” claim misleading. The `WindowRuntime.tabs` sample also counts mutation/access below contained fields as a write to the hub. That is legitimate access-footprint evidence under item 2, but not an independent write to the container's membership.

Failure sequence: move or add a mutation through an unresolved local, or call a mutator whose name also belongs to an immutable method elsewhere. The report remains unchanged; a fact can still appear single-writer or absent. A later ownership move uses §5.5 as its stopping rule and misses a live writer. Adding an unrelated immutable method can even change the global mutator-name set without changing the application behavior. A diff gate reproducing this heuristic would bless the omission.

Fix before A is an authoritative gate:

- Commit the complete field inventory, with zero/single/multiple status, actual mutation versus mutable access/escape, and the distinction between a field and mutation beneath it. Separate declared storage from the authority being proposed.
- Emit all unresolved sites with containing item identity and reason; classify each as outside the target types, supported, or explicitly unresolved. “About ten” from a sample is not an error bound. Unknown target writes must prevent a completeness claim; new unknowns must fail the gate.
- Resolve direct `Runtime::{app, window}` first and only then its actual `Deref<Target = TabState>`. The rule is correct for `self.files` in `seat_a_files_column` and `close_pane`; it must not blindly map every `self.F` or nested receiver to `TabState`. Test explicit receivers, local aliases, indexed elements, helpers returning references, same-named fields/methods, inner mutability, and item relocation.
- State the supported analysis subset. `syn`/`bt_source::Index` supplies syntax and item identity, **not Rust semantic type inference**. Receiver/call resolution needs a specified bounded analysis with explicit unknowns, or stronger compiler information. Do not describe inference from names as “resolved by type.”
- Keep the triggers/effects as approximate annotations. Reachability and one-hop proximity neither prove a write causes an effect nor prove effects are absent. Manual class/owner decisions need a separately versioned annotation input; they cannot be regenerated from the six lexical rules.

**Ticket A is the right place to use `bt-source`, but its proposed implementation is not yet the right instrument.** A declared package universe, item identities, product/test classification and supported syntax queries honor the tripwire's purpose. A PowerShell copier that reads only generated output is also appropriate. Rebuilding the scratch token scanner over `Index` text, silently dropping unknowns, or encoding `main.rs` partitions as physical-file identity would only evade the letter while retaining the shrinking-universe failure. Require a fixture that moves a writer into a newly declared module with unchanged semantic coverage, alongside the new-writer mutation test. No new standalone Python source reader is recommended.

**P1 · R3 — Kind is not enough to generate the routing policy, and the seven kinds do not yet cover their own inventory consistently.**

Exact paragraphs: §4's “The smallest set of kinds that covers the twenty-eight”; the Gate, Instrument, Notification and Hint rows; “Enforcement ... one declaration ... surfaces with their kind”; and ticket D's “surfaces × kind × scope.”

Concrete counterexamples from the current code:

- Settings is an Instrument that takes the whole window's keyboard and pointer. Search is an Instrument that keeps standing when focus leaves. Menus suppress shell typing while allowing application shortcuts, and use `popup_takes_the_key(self.popups_up())`. Rename and git prompts own text input. These cannot derive one input rule from `Instrument`, even with a window scope.
- `websheet` includes four persistent failure states that **are** the pane's content and cannot be dismissed, as well as a dismissible download sheet. They are not all tools summoned on purpose; a dead WebView produces a failure state. Generic Instrument “Esc puts it away” would expose the empty web hole the module explicitly prevents.
- A key-hint card is dismissed by a nonmodifier key/press, not uniformly by moving the pointer away. A timed focus nudge is not necessarily summoned by resting or holding. Their current admission/dismissal rules do not follow the Hint row.
- `Runtime::toast_with_verb` and `press_toast` implement Undo for profile deletion and checkout. Inventory row 16 records only `×` as answerable. An optional Undo does not turn a toast into a compulsory question, but omitting it loses a real action and its lifetime/identity contract.
- The Notification row uses pane-attention episode rules for all long-lived news. Worker failure, a disk conflict, and a program awaiting input are different obligations. The existing ledger's seen/answered and delivery rules should not be manufactured for unrelated operation results merely to make them fit.

Failure sequence: implement D by giving every Instrument one modality, priority and Escape rule. Either Settings starts leaking keys/presses, or the search/menu family blocks actions that currently work; treating all web failure cards as dismissible exposes an empty pane. A generated document and three consumers can agree perfectly about that wrong rule.

Fix: retain the seven names as a **candidate vocabulary**, not a proved minimal exhaustive partition. Define semantic role separately from input policy and presentation. Each declared surface/state needs explicit activation, keyboard/IME/shortcut policy, pointer/wheel/drop policy, scope, priority, safe answer/Escape behavior, replacement/queue policy and lifetime. Share the applicable predicates/order across consumers; do not require identical lists for consumers asking different questions. Preserve exceptional routes such as Escape canceling an in-progress drag above the modal ladder. Split compound inventory rows where state changes the contract. Include persistent pane failure/status information and actionable toasts explicitly, then judge whether another semantic role is needed or “notification” should be broadened without imposing the attention ledger on every member.

D's “a surface not in the table cannot be drawn as an overlay” also needs a concrete registration/drawing boundary. Merely walking the table proves nothing about an undeclared renderer call. Test real routes with competing surfaces, shortcuts, composition, pointer presses and drops, rather than only iterating declarations.

**Disposition of V1–V5:** V1 is a real routing concern, but “keyboard-owning, pointer-transparent is neither ... an instrument” does not follow from the proposed Instrument definition; rename/search already demonstrate different keyboard and pointer policies. The restore card is absent from `keyboard_owner`, and its Enter/Escape handler is below `Shortcuts::lookup`/`run_shortcut`, so the underlying paste/shortcut exposure is real on this baseline. V2 is the confirmed independent defect above. V3 is a real unreadable-delivery problem: `publish_frame_inner` consumes each worker flag for one constructed frame; the next eligible publication can erase the message, and construction is not even a durable receipt of presentation. It is not a violation of an already-ruled Toast kind because the status line was never assigned one. V4 is unfinished ruled notification work; a strip's shape alone does not establish a semantic violation. V5 is duplicated routing policy and demonstrated omission, not itself a surface violating its kind. Keep these five records, but label defect, ruling gap, migration debt and enforcement debt separately.

**P2 · R4 — VIEW is a legitimate ownership axis, but the proposed field classes mix axes and promise the wrong frame invariant.**

Exact paragraphs: §2.1 “The three buckets outside A§4.2,” particularly VIEW's “one surface, one module, one state machine” and “A reader may assume the state is consistent with the surface drawn last frame”; §0's “The contract that removes all three is one.”

A view owner is real: selection, scroll, focus and transient interaction have a lifecycle distinct from documents/sessions. However, this is not a sixth mutually exclusive counterpart to the five cross-thread contract classes. View-owned state can also be an observation, lifecycle state, a projection or an asynchronous target. The same coarse field can contain several: `WindowRuntime.dirty_gate` holds both a pending destructive request and pointer hover, yet the census gives the whole field DUR. `App.drag_broker` is VIEW but spans windows; it is not one popup's state machine. `image_pick_pending` covers background images, profile programs and settings import, so its proposed `preview` ownership does not fall uniquely out of the surface-topic rule either.

Failure sequence: a pointer event changes hover/press state, but its next frame is deferred or presentation fails. The state now describes the new interaction while the last presented picture describes the old one. A consumer trusting the quoted invariant can hit-test or reason about input from the wrong revision. Moving fields into a surface module does not repair this publication boundary, nor does it make a cross-window drag a local state machine.

Fix: keep VIEW as a domain/lifetime label and state the transition contract precisely. Identify the owning instance, commands, current logical state, capture/focus rules, accepted presentation revision, cancellation on hide/blur/retirement, and seat/window relocation. Readers needing pixels must consult acknowledged presentation; readers needing current interaction consult the interaction owner. Keep the last successful frame separate. Treat router delegation, coordinated cancellation and whole-record relocation as three obligations with their own tests. Use narrower field/subfact rows where one struct contains different authorities. D-52 through D-55 remain open: classifying window-thread fields does not repay their cross-thread contracts.

**P2 · R5 — A does not repay D-18, so G's prerequisite is missing.**

Exact paragraphs: §1 “Why no script is committed” ending “that also discharges D-18”; ticket A's Docs/Architecture impact declaring D-18 repaid; and ticket G's “after A.”

D-18 is about **source-reader subject extraction**, not the set of application fields written. Its full ledger paragraph identifies `scripts/dev/bt-app-split-freshness.py::census`, which extracts subjects from literal text and can classify a migrated `method_body("Runtime", ...)` query as a file-bound consumer. The script still does that work. A new field-writer TSV neither changes that classifier nor retargets its output.

Failure sequence: A lands and D-18 is marked repaid. G moves a method whose structural tests already follow an item. The freshness inventory still describes it as a file-bound subject needing retargeting, or fails to distinguish it from a genuinely file-bound reader. G's source-reader proof is still based on the old mechanism despite its prerequisite being marked complete.

Fix: leave D-18 open unless A explicitly includes migration of **that** reader-subject query and its consumer. Prefer a separate bounded ticket with a migrated-item-query versus file-needle fixture and a real relocation witness. G needs both a corrected ownership inventory and truthful source-reader coverage for its selected methods. Give G an explicit method list: §2 note 5 partitions 120 parsed function items including nested helpers, while G promises 35 moved methods against a 115-method baseline. A unique inferred field owner is useful navigation evidence, not an automatic placement rule for every helper in that count.

**P2 · R6 — B–C have a sound dependency direction, but their literal move and proof promises need repair.**

Exact paragraphs: §5.1 step 1 (“move whole”; “every call site by one line at its root”), §5.3's public-surface list, §5.5's expiry reading claim, and B's “the moved `attention/tests.rs` and `notify` tests green under `-p bt-workbench`.”

The executable core of `attention.rs` really is self-contained after reversing `WAIT_TTL`: its external production dependencies are `std`, `bt_layout::SeatId` and that constant. Its test module uses `super::*` and `Duration`. Move that implementation whole, including its private helpers and state; do not rewrite it. `WaitClock` likewise operates on attention vocabulary and supplied time. Keep the endpoint, hook installers, terminal-event normalization, capabilities and native notification delivery in `bt-app`. In particular `attention_map` consumes `bt_term` notification types; moving it wholesale would introduce a dependency the proposed guard correctly forbids. `attention_trace` and `attention_words` also remain adapters, even though §5.2 does not list them.

There are concrete boundary edits beyond the one import:

- `attention.rs` has rustdoc links to `crate::notify::{desktop_reach, WindowPlace::exposed}`. B's new crate has no `notify` module and C's later module is named `reach`; a literal move leaves broken links.
- `NotificationSwitches` has `pub(crate)` fields used by app-side construction, and several vocabulary methods are `pub(crate)`. Exporting only the named type declarations does not make those accesses compile. `WaitClock` needs its called methods and deliberate re-exports/imports; moving `attention_is_consumed` to a differently named function also needs a root alias or call-site changes. None requires changing behavior.
- `notify` tests mix reach/interruption assertions with `NotificationRoute` and `toast_title` adapter tests. They cannot all move under B, where the reach implementation has explicitly been deferred to C. Split by owned behavior in C and retain the adapter tests in `bt-app`.
- Expiry behavior is not proved solely by moved ledger tests. `attention_wire` currently owns the three `WaitClock` tests, and `deliver_attention`/`settle_attention` still arm, forget and spend the clock. Move the pure-clock tests with it and retain integration coverage of those callers. Changing the rule is local only when its arming/retirement semantics remain unchanged.

Failure sequence: implement B exactly as the listed whole-file move and visibility whitelist, then run app checks/doc checks and the promised `notify` test command. App-side field/method access or aliases fail, links no longer resolve, and the reach tests are still in the other crate. Moving the entire `notify` test module to make the command pass instead drags adapter concerns across the boundary.

Fix the brief's atomic import/export/test/doc inventory. Check the new library and the real app consumers, the pure clock/ledger/reach tests at their correct step, and app ingress/delivery/teardown tests. “Same filter count” alone is not evidence of the same exercised behavior. `Instant` as an in-process supplied-time argument is compatible with A§12.3; do not serialize it as a future protocol.

For C, give `Places` a private counter with the current initial value **0** and preserve monotonically issued places, surrender on transfer, and fresh ordering in the target window. It represents allocation history, not the live queue itself. A privacy compile-fail test is stronger than “no app item names the mutator”: making a mutator public does not add any call site, so that alternative source test would stay green under C's advertised mutation. The four-struct census will still see callers lending `WindowRuntime.attention_next_place`; hiding its scalar does not mechanically make its row single-writer under §1. Report the private authority and remaining lenders separately, and include the new owning type if the census is to show its internal writer.

Keep `bt-workbench → bt-layout` for typed `SeatId` as the documented temporary exception. Replacing it with `u64` removes a manifest edge without providing session identity. B–C are extraction, not completion of D-1/D-54: `Site` still identifies tab/seat context, and the ledger still lives in `LeafSession` held by a window. No remote/headless session owner is created by that move alone.

**P2 · R7 — F's proposed host can still erase a failure before its one-second acceptance check.**

Exact paragraphs: §4 Toast's “stays long enough to read (≥ 4 s); three per anchor, the fourth evicts the oldest,” and ticket F's “A worker that dies is still being told about a second later.”

`ToastHost::raise` immediately starts departing the oldest live card of a full anchor. Its own test `a_fourth_card_on_one_anchor_sends_that_anchors_oldest_away_at_once` pins this. The source also explicitly describes eviction of a card only 40 ms old. Four worker failures falling back to the window anchor, or a worker failure followed by ordinary toasts at that anchor, defeats F's unconditional one-second claim. This does not refute the benefit over a one-frame status line; it refutes the guaranteed dwell as written. Moving the global worker notice flags also requires deciding which window consumes each notice, not creating one toast per window/frame accidentally.

Fix: keep the existing cap/loss policy unless the owner rules a change. State dwell as nominal and eviction as an explicit exception, keep a diagnostic record, and make F test both ordinary visibility and overflow behavior. If a failure must always remain actionable, specify a persistent notification or aggregation policy through Q4 rather than silently changing toast admission. Consume each lane death once at a stated delivery point; do not introduce redraw recursion by raising a toast inside frame construction. Retain the hand-off completion's request/target association when choosing its anchor.

**Census spot-check record.**

Selection was pseudo-random without replacement from the TSV's 170 distinct fact names: PowerShell `Group-Object fact | Sort-Object Name`, then `[Random]::new(20260925).Next(170)`, retaining the first ten distinct indices. This seed was chosen before looking at the selected facts. Each listed module/function was located; `rg` searches were checked against its body and the relevant receiver/declaration. The sample covers **194 TSV writer rows**, including the large hub rather than resampling it away. The evidence is about the note's syntactic definition of writing/access, not a proof that every lent reference mutates.

| Selected fact | Listed writer rows checked | Result |
|---|---:|---|
| `WindowRuntime.rename_blink` | 9 | All listed reset callers present; `advance_rename_blink_if_due` missing. |
| `WindowRuntime.advanced_reveal_sample` | 2 | Assignments in `toggle_advanced_group` and `advance_strip_animation` present. |
| `App.drag_broker` | 6 | Assignments/mutable accesses in all listed functions present; `as_mut` is an access classification, not independently a write. |
| `WindowRuntime.placeholder_tab` | 5 | All listed assignments/take present; `FolioApp::transfer_tab` is an additional writer. |
| `WindowRuntime.tabs` | 140 | Listed container/descendant assignment, access and lend sites present; membership changes and contained-state accesses must not be conflated. `transfer_tab` additionally removes from the source. |
| `WindowRuntime.dirty_gate` | 2 | Hover and take present; `raise_dirty_gate`'s opening mutation missing. |
| `App.files_worker_running` | 2 | Both lend to `files::disable_files_worker_state`; it is the common writer, confirming this is not competing authority. |
| `WindowRuntime.file_menu` | 6 | Four floats-module creation/removal callers and keyboard/mouse mutable accesses present. |
| `WindowRuntime.image_pick_pending` | 4 | All listed assignments/take present; program/import/image purposes cross the proposed topic owner. |
| `TabState.files` | 18 | All listed writers/accesses present, including explicit indexed receivers, move helpers and `Runtime` Deref access. |

Reproduction searches include `rg -n 'rename_blink|advanced_reveal_sample|drag_broker|placeholder_tab|dirty_gate|files_worker_running|file_menu|image_pick_pending' crates/bt-app/src/main.rs crates/bt-app/src/runtime -g '*.rs'`, plus function-scoped `rg` for `\.tabs\b` and `\.files\b`. Read the receiver chain and enclosing function; a bare spelling hit is insufficient. The requested `rg -n 'fn keyboard_input|fn mouse_input|menu_or_dialog' crates/bt-app/src` was also followed through the real ladders, `keyboard_owner`, `PopupsUp`, shortcut dispatch and restore handling.

**The requested five TSV-marked single-writer checks are impossible with this artifact:** no such rows are committed. I did not invent five classifications or silently treat absence as “single.” As an additional negative probe, I searched five absent fields across the crate, including aliases at the named writers:

| Absent field probed | Post-construction writers found | Second module found? |
|---|---|---|
| `WindowRuntime.modifiers_held` | `FolioApp::window_event` assignment | No. |
| `WindowRuntime.diagnostic_minimized` | `Runtime::window_is_iconic`, via `Cell::set` | No; illustrates inner mutability outside a simple `&mut self` mutator rule. |
| `LeafSession.thumb_awake` | `Runtime::wake_terminal_thumb` assignment | No. |
| `LeafSession.attention_clock` | `settle_attention` calls `due`; `deliver_attention` calls `arm`/`forget`, both `main[free fn]` | No under the note's module grouping; multiple functions nonetheless. |
| `LeafSession.last_presented_frame` | `Runtime::redraw` assigns both pane and focused-leaf frames | No; the many `WindowRuntime.last_presented_frame` hits belong to a different field. |

These supplementary searches do not validate the missing 223-row population or prove absence of all alias writes. Commit that population and the unresolved ledger to make the requested audit genuinely possible.

**Ticket order and independent mergeability.**

| Ticket | Recommendation and actual dependencies |
|---|---|
| E | First, as a standalone **0.4.5 defect repair**. No census, crate extraction, UI taxonomy or P19 dependency. |
| A | Independently mergeable once R2's output/unknown/annotation contract is specified. A field-writer query does not close D-18; split or explicitly include that separate work. |
| B | After P19/D-27, with the atomic boundary corrections in R6. It need not wait for A or a UI ruling. Pure extraction is independently reviewable. |
| C | After B and P19; preserve the existing place semantics and correct its privacy test/census assertion. Independently mergeable on that base without a session registry. |
| D | After the owner's revised table ruling and its promised ownership-change note/review. Preserve ticket 57's resulting behavior on the implementation base. Requires R3, not B/C. |
| F | After Q4 and a precise delivery/overflow policy. It need not wait for all of D if that policy is separately ruled. No new notification policy should be invented inside the implementation. |
| G | After corrected A **and** the real D-18 repair/selected source-reader preparation. Freeze explicit item identities/destinations and move with every affected reader gate atomically. No inherent B/C/D/F dependency. |

Use this dependency graph instead of one serial A→G queue. The “committed, CI green on the branch” stopping point and post-0.4.5 merge restriction can remain for the structural tickets; applying them to E would knowingly ship the demonstrated ordinary-use loss. D-11 is repaid only to the extent its ownership evidence is reproducible, D-8 only after allocation/input policy is ruled and enforced, and D-57 only after the extraction and its adapter contracts land. Do not report those as the repayment of the broader session or cross-thread ownership debts.

**One-line recommendations on the five owner questions.**

1. **Q1:** Keep seven names provisionally, but rule semantic role and per-surface input/lifetime policy separately; the current seven-row table is not yet complete or demonstrably minimal.
2. **Q2:** Prefer a full window-scoped restore gate for a durable restore decision; obtain the explicit pointer-policy reversal and coordinate it with ticket 57 rather than smuggling it into D.
3. **Q3:** Prefer persistent actionable notifications for optional first-run/setup invitations; changing today's modal behavior requires the owner's ruling and preservation of explicit install consent.
4. **Q4:** Yes to readable toasts for hand-off refusals and worker-death announcements, with explicit eviction/logging semantics and durable status where continued action depends on it.
5. **Q5:** Keep the typed `bt-layout::SeatId` edge as a shrink-only exception until D-1 provides real session identity; a plain number only hides the coupling.
