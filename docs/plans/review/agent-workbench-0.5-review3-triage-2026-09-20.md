# Review 3 triage — raw material for revision 4

Input: Codex 22 findings (C1–C22), Kimi 22 findings (K1–K22), the agent survey, and the shipped
code. Every code claim below was re-verified against the repository on `main`
(the reviewers cited a worktree of the design branch; line numbers here are this
tree's). Precedence read as instructed: §12 > §11 > §1–10.

Classes: **R** = I can rule (pure function/engineering). **O** = owner must rule (one question,
§G). **D** = defer, with the ticket named.

---

## A. The six claims called out by name

| Claim | Verdict | Evidence in this tree |
|---|---|---|
| **C3** — §11.5's answer to stuck-Waiting is factually wrong | **CONFIRMED** | `UserPromptSubmit` is a `Clear{Boundary, All, begins_turn:true}` (`crates/bt-app/src/attention_map.rs:198-213`) — but approving a tool, walking a permission menu or answering an elicitation is not a prompt submission. The code says so itself: `PostToolUse` is `ClearClass::Receipt / ThisKind` (`attention_map.rs:253-268`), `ElicitationResult` likewise (`:269-280`), and the receipt gate only retires credentials **already acknowledged** (`attention.rs:1289-1296`). Separately, six user gestures acknowledge — `Keyboard, Ime, Paste, FilesRow, MouseButton, MouseWheel` (`attention.rs:541-553`, `main.rs:19606-19625`) — so an arrow key acknowledges a standing question. §11.5's "nothing new is needed" must be withdrawn. |
| **C4 / K1** — the one-second forward clear eats a genuine wait | **CONFIRMED, blocker** | `UserPromptSubmit` carries `ClearScope::All` (`attention_map.rs:209`), so the rule as written deletes a `PermissionRequest` minted 100 ms after a submit — the ordinary first-tool-call case, not a race. K1's diagnosis is the sharper one: the inversion the rule was written for is `Stop`-before-`PermissionRequest`, and `begins_turn` is already the column that distinguishes the two. |
| **K3** — Working after `kill -9`; the pane's root is the shell | **CONFIRMED** | There is no process walker anywhere in `crates/` (zero hits for `CreateToolhelp32Snapshot`, `Process32*`, `EnumProcesses`, `NtQuerySystemInformation`); the only handle is the pane's root child (`crates/bt-pty/src/lib.rs:2063`). §11.8 drops the process rung deliberately, so a killed grandchild agent is unobservable. The shipped ledger has **no** Working state and no reaper — its only expiry is `WAIT_TTL = 600 s` for waits (`attention_wire.rs:77`, `WaitClock` `:307`). |
| **K4** — a denied permission leaves Waiting standing | **CONFIRMED** | Deny executes no tool, so no `PostToolUse` fires; and `PostToolUse`'s receipt is gated on acknowledgement anyway (`attention_map.rs:253-268`, `attention.rs:1289-1296`). The wait stands until `Stop` or the 10-minute TTL. **But it is only a word defect**, not an attention defect — see the derivation rule R-1, which removes it without new machinery. |
| **K5** — the floor table promises a row its own rule forbids | **CONFIRMED** | §11.8 row: a hookless, OSC-silent vendor gets "the mark and the title"; two bullets later, "Nothing else lists an agent." A zero-signal agent hand-typed into a shell can produce neither of the two recognition contracts. The only consistent reading is a Folio-launched dedicated pane, which §11.8 never says. |
| **K12** — `sanitize_paste` maps `\n`→`\r`; bracketing is conditional on 2004 | **CONFIRMED** | `sanitize_paste` normalises `\r\n` and bare `\n` to `\r` (`crates/bt-app/src/input.rs:811-832`); `paste_bytes(text, bracketed)` wraps only when told (`:796-809`); the caller passes the child's own mode — `input::paste_bytes(text, session.bracketed_paste_mode())` (`main.rs:118847-118858`), which also clears the selection and scrolls the view to the bottom. Into an unbracketed child a multi-line reply submits itself line by line. |

## B. Where the reviewers are wrong, or overstate

| # | Claim | Verdict | Why |
|---|---|---|---|
| W1 | **K16** — "most-used first has no store; quota readings have no home" | **PARTLY WRONG** | §7 already rules the store: *"Folio stores `{percent, resets_at, fetched_at}` per account-window"*. What is genuinely missing is only (a) that it persists across restart and (b) a use counter. One sentence, not a new subsystem. |
| W2 | **C13** — source and float "alternately send `CSI I/O`"; "both answer a cursor-position query" | **WRONG for today's code; a ticket-7 risk only** | Terminal focus is one boolean per session, reconciled once per drain turn — `leaf.session.set_keyboard_focus(holds_the_keyboard)` (`main.rs:37283`, inside `drain_leaf_pty` `:37165`) — and every protocol reply drains to the one PTY through `take_pty_writes`. A second view cannot become a second protocol actor unless ticket 7 adds one. The design should *state the invariant*, not invent a mechanism. Kimi reaches the same conclusion (K13's closing note) and is right. |
| W3 | **K15** — `folio --uninstall-cleanup --purge` may not exist (*flagged unverified*) | **PARTLY** | Correct that it is absent from `crates/`. But it is a **ruled 0.4.3 deliverable** — `docs/plans/design/clean-uninstall-2026-09-20.md:40` and ticket T-C at `:62` — i.e. a forward reference to a ticket that lands before 0.5, not an invention. Revision 4 should spell it as such. |
| W4 | **C2** — the wire "does not carry general session/turn identity" | **CONFIRMED but under-stated** | `Message` is five fields — `capability, family, event, id, text` — with **no sequence number and no timestamp** (`attention_wire.rs:85-101`), and `id` is `None` for every shipped row. So there is no ordering evidence at all, which is why C4/K1's time window cannot be repaired by a tolerance of any width. |
| W5 | **C8** — "the hook inbox drops oldest entries beyond 256" offered as a notification bound | **TRUE but irrelevant** | `INBOX_BOUND = 256` (`attention_wire.rs:84`) bounds the *pipe*, not delivery. The real per-pane bounds are one toast per turn (`attention.rs:1644-1647`) and 64 frames/pane/second (`attention.rs:1698`). Neither is application-wide — C8's conclusion stands on different evidence. |
| W6 | **K13** — "a preview float whose source tab closes is not swept" | **PARTLY** | `preview_tab_index` does fall back to the active tab (`main.rs:56587-56591`), and `close_pane`/`close_tab` never touch the float host. But `preview_surfaces` derives from the float host itself and `sweep_preview_panes` retires what stopped existing, so a preview float is not dangling — it is *mis-addressed*. The distinction matters: with a terminal tenant it becomes a wrong-pane bug with a process attached, exactly as K13 says. |

---

## C. The issues, clustered (44 findings → 27 issues)

### C.1 The state model

| # | Issue | Findings | Verdict | Smallest change | Class |
|---|---|---|---|---|---|
| I-01 | One state word is asked to carry asserted facts, acknowledgement and outcome at once | C1, C3, K4, K6(part) | CONFIRMED | **Rule R-1** (below). The shipped ledger already separates the two: credentials live in `strong`, acknowledgement is a watermark, and `answer()` moves the watermark **without removing the credential** (`attention.rs:1407-1423`), which is what makes `State::Acknowledged` a fixed point (`:1040-1050`). Derive the word and the dot from those existing fields; store no new state. | **R** |
| I-02 | The one-second forward clear deletes live waits | C4, K1, K2 | CONFIRMED — blocker | **Rule R-2**: forward tolerance is a property of the clear's existing `begins_turn` column. `begins_turn:false` (Stop / StopFailure / SessionEnd) forgives waits minted within 1 s after it; `begins_turn:true` (UserPromptSubmit) forgives nothing — a submit causally precedes its own turn's waits. No new field; the column exists (`attention_map.rs:211, 225, 238, 250`). | **R** |
| I-03 | Working never ends: `kill -9`, or a `begins_turn` landing after its own `Stop` | K2, K3, C5(part) | CONFIRMED | **Rule R-3**: Working is *evidence with a clock*, not a latch. It ends on a turn-end clear, on OSC 133 command-end where shell integration is on, or at `WAIT_TTL` (the same 600 s the waits use — one number, not two). Add one honest line to the floor: without shell integration a killed agent's row holds Working until the pane closes. | **R** |
| I-04 | Denied permission leaves the word "Waiting" | K4 | CONFIRMED | Dissolved by R-1: Waiting is shown only while a wait is **asserted and unacknowledged** — exactly the shipped `Grounds::AwaitingInput` (`attention.rs:984, 1000-1006`). Deny is a keystroke → acknowledged → the row falls back to Working. Record the residual (Folio cannot tell deny from approve) in the floor. | **R** |
| I-05 | "Focused" has no application-level definition | C6, K6, K8(part) | CONFIRMED | **Rule R-4**: effective pane focus is the shipped four-factor predicate `seat_holds_the_keyboard(window_focused, tab_is_active, seat_is_focused_leaf, owner_is_a_shell)` (`main.rs:18735-18742`), with the fourth argument read as "the keyboard's owner is this pane's terminal view". Background tabs and minimised windows have none. Note the shipped `Settle{active, focused}` is **window** focus + active tab (`main.rs:25412-25414`) — K6 is right that it is not the pane rule, and right about the fix. A Folio composer taking the keyboard therefore makes *no* pane focused, which is also the answer to K8's 1004 flip. | **R** |
| I-06 | The floor table contradicts its own recognition rule | K5, C5(part) | CONFIRMED | One sentence: a zero-signal vendor gets a row **only in a Folio-launched dedicated pane** (known by construction). Hand-started, hookless, OSC-silent: as invisible as WSL, said in the same breath as the table row. | **R** |
| I-07 | "All states" is a vendor claim, not a per-install evidence contract | C2 | CONFIRMED | Replace the floor table with a **per-install matrix**: each cell names the event that actually installed, read back from the config file (`attention_hooks.rs:34-36` already reads installed rows back from disk rather than remembering them). "No event yet" is **unknown**, not Idle. Expired evidence removes the assertion; it never manufactures another state. | **R** |
| I-08 | Pane identity ≠ agent lifetime; and a moved pane's ledger | C5, C15, K21 | CONFIRMED | **Rule R-5**: one address type for everything that names a pane, and it is the shipped `PasteTarget` — `{tab: TabId, seat: SeatId, incarnation}` (`main.rs:835-839`, validator `:804-811`). The ledger's `Site{tab: usize, seat}` is a **positional index** (`attention.rs:91-94`) and must not survive into 0.5: tabs are dragged and seats are renumbered. Add an *agent lifetime* epoch under that key; retire on SessionEnd / OSC 133 end / incarnation change; events from a retired lifetime are dropped. A moved pane's ledger state travels with its leaf, re-sited, same capability. | **R** |

### C.2 Notification, dot, badge

| # | Issue | Findings | Verdict | Smallest change | Class |
|---|---|---|---|---|---|
| I-09 | Which transitions raise a notification is never said | C7, K7 | CONFIRMED | Owner's ruling stands (*every* notification appears) — this only **bounds** it: name the raising set. Recommend Waiting + Failed always, Done under the shipped `Agents ▸ Turn finished` switch (`attention.rs:332-333, 356` — `NotificationSwitches.turn_end` already exists). | **O — Q2** |
| I-10 | No application-wide delivery arbiter: storm, coalescing, hand-over, cross-window | C8, K7, K9(a)(d) | CONFIRMED | **Rule R-6**: one app-owned arbiter keyed by `(NotificationRoute, episode)`. `NotificationRoute{window, tab, seat}` already exists with a strict parse and the ruled "a route naming something gone resolves to nothing" behaviour (`crates/bt-app/src/notify.rs:24-87`) — reuse it, do not invent. Coalesce **duplicate evidence for one route**, never two agents. Numeric visible/queued caps; overflow still represents every affected pane. Another window's arrival is presented by the foreground Folio window. Delivery is claimed once presentation begins; a foreground change routes only *subsequent* notices (`desktop_reach` already decides once, at arrival, `notify.rs:233-246`). | **R** |
| I-11 | Dismissal, acknowledgement, mute and resolution are conflated | C7, K9(b)(c) | CONFIRMED | **Rule R-7**: the notification has its own lifecycle, independent of the dot. Auto-dismissal never changes pane debt. Mute scopes to the notification only — never the dot, never the badge, never Failed. A closed or replaced target invalidates every action on that notification; the surface dies with the route. | **R** (mute+Failed → **O — Q6**) |
| I-12 | The expansion is not necessarily "the latest reply" | C9 | CONFIRMED | `attention_words` returns an **80-character lede**, not a reply (`attention_words.rs:61, 87`). Bind content to the event and the lifetime: Claude's expansion shows the sentence the event carried; everyone else's shows a bounded terminal snapshot, and the note must stop calling that a reply. A Failed or Waiting notice never borrows an earlier conclusion. | **R** |
| I-13 | Badge scope: this window's dots or the application's | K21 | CONFIRMED (unruled) | Recommend: the badge sums **its own window's** dots; the rail carries the other windows' (§11.3 already made the rail application-wide). | **O — Q7** |
| I-14 | Sticky Waiting row (§11.11 Q1 / §12.4.1) | K7(part), K22 | not a defect | Owner already recommended yes. Keep; no change. | — |

### C.3 Reply, paste, and the second view

| # | Issue | Findings | Verdict | Smallest change | Class |
|---|---|---|---|---|---|
| I-15 | "Verifiably waiting for free text" proves absence of evidence, not readiness | C10, K11 | CONFIRMED — blocker | No described adapter asserts that the editor accepts ordinary text, is empty, and still belongs to that completion. Two further holes both reviewers found independently: `WaitKind::Elicitation` is **not** excluded by the two-kind test and MCP elicitation can be a choice list, not free text; and on the OSC lane a child's `Proceed? [y/N]` is invisible to the ledger. **State plainly: no adapter qualifies for auto-submit in 0.5.** | **R** (and Q1) |
| I-16 | Paste + Enter has no editor or transport contract | C11, K10, K12 | CONFIRMED — blocker | If any write path ships: **no Enter**; refuse multi-line when the child's 2004 is off rather than flattening silently (`input.rs:796-832`, `main.rs:118847`); never clear or replace an unseen draft; route through the shipped answer path (`AnswerKind::Paste`, `attention.rs:545`; `answer_attention`, `main.rs:97217`) so the ledger spends the wait; name `PasteTarget{tab, seat, incarnation}` and revalidate with `live_paste_target` immediately before enqueueing (`main.rs:101150`). | **R**, conditional on Q1 |
| I-17 | One PTY, one size does not define two interactive viewports | C12, C14, K13 | CONFIRMED | One parser/grid/mode owner (the session); each view owns its scroll anchor, selection and geometry; exactly one keyboard composition at a time, never migrating implicitly; pointer gestures pinned to the originating view and geometry generation; mouse reporting in canonical cells. PTY lifetime and size stay the source pane's; source retirement detaches every view. | **D — ticket 7 (cut from first 0.5)** |
| I-18 | A second view must not become a second protocol participant | C13, K13 | **PARTLY** (see W2) | Turn it into a stated invariant rather than a mechanism: terminal focus is computed **once**, from the one input owner across all attached views (`main.rs:37283`); only the canonical session answers queries and drains replies; protocol replies never traverse the answer path — which the shipped code already guarantees structurally (`main.rs:97210-97216` names the focus reports as out of reach by construction). | **D — ticket 7** |
| I-19 | Float lifetime, resize authority, render budget | C14, K13(lifecycle) | CONFIRMED | `float.rs` has two tenants and no terminal tenant (`float.rs:1289, 1326`); `focus_thumb` is a read-only text re-projection at 10 Hz (`focus_thumb.rs:103`); `close_pane`/`close_tab` never touch the float host; `preview_tab_index` falls back to the **active** tab (`main.rs:56587-56591`). These are ticket-7 obligations, not properties inherited from the chassis. | **D — ticket 7** |

### C.4 Protocol, log, quota

| # | Issue | Findings | Verdict | Smallest change | Class |
|---|---|---|---|---|---|
| I-20 | Created-pane authority has no lifetime | C15 | CONFIRMED | Define it as a relation between a **source agent lifetime** and a **target lifetime**, subordinate to current scope. Target replacement, source retirement or grant revocation invalidates it; moving the pane requires scope revalidation. Everything else is the typing tier, whoever created it. | **D — ticket 8** |
| I-21 | A revision on "everything" is not a concurrency model | C16, K14 | CONFIRMED | Rule the three gaps in one sentence each: **scope** — in 0.5 the revision rule binds exactly the mutating verbs ticket 8 ships; **source** — a per-addressable-thing counter bumped on *state/content* change only, never on focus or a painted frame (`focus_thumb.rs:247` already distinguishes identity from revision); **shape** — refusal returns the current revision as a structured error in §6's list, checked atomically with the mutation on the owning executor. A Folio revision protects Folio-owned state only. | **D — ticket 8** |
| I-22 | The action log cannot support its promised metrics | C17, K15 | CONFIRMED | Two record classes (agent actions; payload-free Folio attention events); lifetime/episode/action ids; start and end causes; a **named number** for the bound (CONVENTIONS §十 rule 5) and whether it is per app or per window; enqueue recorded separately from completion — a PTY write is not proof an agent obeyed. Sequencing: §2.5's counts are queries over events produced in tickets 2/4/5, so **a minimal append sink rides with ticket 2** or the metrics claim comes out of the first 0.5. Spell `--uninstall-cleanup --purge` as the 0.4.3 deliverable it is (W3). | **R** |
| I-23 | Quota attribution invents an account | C18, K18 | CONFIRMED | Unknown attribution stays **unknown**; it never becomes "one account". Per-source adapter contract; say whether an identity is a *configuration* or a *provider account* before aggregating. A hookless agent has no row, so its account is never "in use" and never toasts — say that out loud (K18). | **R** |
| I-24 | Reset time and a new turn do not prove recovery | C19 | CONFIRMED | Model availability as available / exhausted / **unknown**, per limit bucket. Passing `resets_at` invalidates the old reading and schedules a refresh — it is not a recovered sample. A submission is evidence about a session, not about remaining quota. "All exhausted" is defined only over a non-empty, fully attributable set. Recovery toasts require an observed recovery. This supersedes §11.5's "or any later turn start for that account". | **R** |
| I-25 | Quota persistence and "most-used first" | K16 | **PARTLY WRONG** (W1) | §7 already names the store. Add two clauses: it survives restart, and it carries a use counter. | **R** |
| I-26 | Poll latency, and Done vs Limited on one row | K17 | CONFIRMED (unruled) | State the 5–10 min poll latency in the floor; extend §11.5's precedence: an **unspent Done outranks Limited** on the row, the quota fact living on the chip and in the panel. Otherwise unread debt hides behind a quota reading. | **R** |

### C.5 Tickets and scope

| # | Issue | Findings | Verdict | Smallest change | Class |
|---|---|---|---|---|---|
| I-27a | Recent view claims creators and editors it cannot witness | C20 | CONFIRMED | The wire discards tool payloads by construction (`attention_wire.rs:85-101`), so "file-write events from agent hooks" has no producer. Ship Recent with **evidenced mentions and the user's own opens**, relative paths resolved against the event-time directory, "who" absent when unknown. Add created/edited when a producer exists. | **R** |
| I-27b | The rail ticket calls itself read-only and contains Stop | C20 | CONFIRMED | §11.10 ticket 3 says "Read-only"; §11.3.5 gives the glance card *go there · stop*. A hand-started agent inside a shell has no process handle, and Ctrl+C ≠ killing the pane's tree. | **O — Q5** |
| I-28 | The marks pipeline lost its ticket | K19 | CONFIRMED | Old ticket 0 owned it; the §11.10 table dropped it while ticket 1 draws a mono mark per row. Fold the marks pipeline into ticket 1, its first consumer. **The survey supplies the source**: the ACP registry serves an official per-agent `icon.svg` with `license`/`license_url` at `cdn.agentclientprotocol.com/registry/v1/latest/<id>.svg` — a sanctioned, versioned, machine-readable answer to §4.9's problem. | **R** |
| I-29 | A second installer hides inside ticket 6 | K20 | CONFIRMED | The Anthropic statusLine shim has the install / uninstall / refuse-politely shape that `attention_codex.rs` and `attention_copilot.rs` each treat as a slice of their own. Split **6a** (statusLine lane: reader, wrap-or-refuse, account attribution) from **6b** (chip, panel, toasts, Codex/Kimi readers). | **R** |
| I-30 | The sequence has no acceptance gates and a too-large first release | C21, K22 | CONFIRMED | §D below. | **R** + Q1/Q3 |
| I-31 | Select-and-comment | C22, K§8 | agreed by both | Both reviews: capture is easy, **delivery is the unresolved part of §11.7.3**, so it cannot ship ahead of it. Both correctly find no conflict with copy-on-select: the selection persists after release (`main.rs:96317`, `:19875`), so a command hanging off the persisted selection never touches the release gesture. Shift-drag already forces local selection in mouse-reporting TUIs (`main.rs:19746`). | **O — Q4** |

---

## D. The structural rules (seven rules replacing ~25 patches)

CONVENTIONS §十 rule 6: the same obligation failed twice, so the answer is structural, not a third
patch. Every rule below is expressible in fields the shipped code already owns.

| Rule | Statement | Replaces |
|---|---|---|
| **R-1** | The ledger owns **facts**, never a word. Five independent facts, each with one owner and one expiry: agent lifetime · turn phase · outstanding waits (asserted set) · acknowledgement watermark · account quota, plus last turn outcome. **The row's word and the pane's dot are derived every frame, stored nowhere.** Waiting is shown only while a wait is asserted **and** unacknowledged. Focusing acknowledges; it never resolves a wait, recovers quota, or changes an outcome on the wire. | I-01, I-04, C1, C3, K4, and §12.2's apparent conflict with §3 |
| **R-2** | Forward tolerance is a property of the clear's `begins_turn` column: `false` forgives 1 s after it, `true` forgives nothing. | I-02, C4, K1 |
| **R-3** | Every asserted fact carries the same clock — `WAIT_TTL`, 600 s. Expiry means the evidence went stale; it never establishes another state. | I-03, K2, K3 |
| **R-4** | Effective pane focus = `seat_holds_the_keyboard(window_focused, tab_is_active, seat_is_focused_leaf, keyboard_owner_is_this_terminal_view)`. One definition, application-wide, including future views and Folio's own composers. | I-05, C6, K6, K8 |
| **R-5** | One address for a pane everywhere: `{TabId, SeatId, incarnation}` (+ window id when it crosses windows), plus an agent-lifetime epoch. Positional indices never leave the frame. | I-08, C5, C15, K21 |
| **R-6** | One application-owned delivery arbiter, keyed by `(NotificationRoute, episode)`. Coalesce duplicate evidence for one route; never merge two agents. A retired route is a no-op everywhere. | I-10, C8, K9 |
| **R-7** | Notifications, dots and resolutions are three lifecycles. Dismissal touches only the first; focus touches only the second; the producer touches only the third. Mute scopes to the first alone. | I-11, C7, K9 |

---

## E. The revised first 0.5

### E.1 My view: is "appear + expand + take me there", without reply, coherent?

**Yes — and it is the only coherent first release.** §11.2 is the owner's own sentence: *Folio's
first job is notify, and take me there*, and its argument is that there is exactly **one**
authoritative place to answer — the agent's own TUI. The reply field re-opens the delivery path
that §11.2 had just closed, from the other end. Cutting it removes four findings at once
(C10, C11, K10, K11) plus their dependants — no input-readiness proof is needed, no 1004 flip
(R-4), no disarm race, no retained-draft question (§11.11 Q4 dissolves), no per-vendor paste
contract. What remains still delivers the headline scenario end to end, and it is testable
without a real agent.

Drag-out (ticket 7) goes with it: both reviews name it the cut candidate, nothing depends on it,
its chassis has no terminal tenant, and I-17/I-18/I-19 are five unruled functional questions
about input ownership that no earlier ticket establishes.

### E.2 Sequence, with one acceptance gate each — every gate runnable with no vendor installed

| # | Ticket | Acceptance gate (no real agent) |
|---|---|---|
| 1 | **Recent view** + the marks pipeline (I-28) | A fixture of synthetic path/open events produces exactly the ruled rows and eviction order; an event with no producer renders **no** "who"; every mark resolves from the registry file or the row draws none. |
| 2 | **The ledger**, keyed by `{TabId, SeatId, incarnation}` + lifetime epoch | One replay test over synthetic wire lines and a fake clock, asserting the derived word **and** dot per frame against a checked-in table, across: fast permission 100 ms after submit (survives, R-2); late `Stop` crossing the next submit; duplicate `Stop`; `begins_turn` after its own `Stop`; wait expiry at `WAIT_TTL`; deny (word returns to Working, R-1); lifetime replaced in one pane (old events dropped, R-5). |
| 2b | **Minimal append log sink** (rides with 2, I-22) | Each ledger transition appends one record; the bound is a named constant and eviction is oldest-first; the record type **has no payload field** (compile-time, not a filter). |
| 3 | **The rail and the row** — application-wide, one line, glance card | A fake multi-window registry incl. one minimised window: row order is stable across arrivals; clicking a retired row is a no-op; the badge equals this window's dots (Q7). |
| 4 | **Notification: appear · expand · go there** (no reply) | Fifty synthetic arrivals in one second: visible cap honoured, every affected pane still represented, no two agents coalesced; a pane closed mid-display kills its notification and its expansion; activating a retired route is a no-op; a focused-pane arrival appears and expires **without** entering the badge. |
| 5 | **The badge and its list** | A dismissed notification leaves the pane's dot; focusing that pane clears dot and tab dot together; the badge count equals the sum and its colour the most urgent class. |
| 6a | **The statusLine lane** (I-29) | Against a fake vendor child and a scratch `CLAUDE_CONFIG_DIR`: a pre-existing user statusLine is refused out loud and the file comes back byte-identical after uninstall. |
| 6b | **Quota: chip, panel, toasts** | Captured response fixtures + fake time: a bucket reads available / exhausted / **unknown**; crossing `resets_at` yields unknown, not recovered; a recovery toast fires only on an observed available sample; an unattributable reading never becomes "one account". |
| 8 | **Protocol, read verbs only** | Fake identities and scopes; every read returns a revision; **no mutating verb ships** until I-21's per-verb contracts exist. |
| — | **Cut to 0.5.x** | Reply in the notification; drag-out / second interactive view (7); mutating protocol verbs and the typing tier; select-and-comment delivery; web driving. |

---

## F. Vendor coverage, folding in the survey

| Finding | Consequence for revision 4 |
|---|---|
| A family costs **rows plus a config template, and no code** — there is no `match` on a family name in the crate, and a test pins it (`attention_map.rs:1-8`) | The adapter cost is the **installer**, not the mapping. `attention_hooks.rs` is Claude-Code-shaped and Claude-Code-only today (`:1`); the four shipped families are `claude-code`, `codex`, `pi`, `copilot` (`attention_map.rs:102-108`). |
| The survey's §8.2: Qwen Code, CodeBuddy and iFlow rebuilt their event layers **on the Claude Code pattern**, not Gemini's; GLM and DeepSeek run the literal `@anthropic-ai/claude-code` binary | **Parameterise `attention_hooks.rs`** — config-root env var (`CLAUDE_CONFIG_DIR` / `QWEN_HOME` / `CODEBUDDY_CONFIG_DIR` / `KIMI_CODE_HOME` / …), file path, and event-name aliases. One reader then covers ~7 vendors: Anthropic + GLM + DeepSeek (zero marginal cost), Qwen Code, CodeBuddy, iFlow, and Kimi Code (same shape, TOML container). |
| **Gemini CLI is the real gap** | Its hook `command` is a **shell string** with no `args[]` form, its hooks run synchronously inside the agent loop, and its vocabulary is its own (`AfterAgent`, `Notification{type:"ToolPermission"}`). It is the first adapter with no safe exec form — `docs/agent-integration-marks.md` has Folio refuse shell templates it did not write. Name it as its **own ticket, after the first 0.5**, not as a row in a parameterised reader. |
| Five high-value agents are `node.exe` or `python.exe` | Independently vindicates §11.8's dropped process rung. Keep. |
| ACP is client-spawns-agent | Cannot watch a TUI someone is already typing into. Keep it out of 0.5 — but take its **registry** for the marks (I-28). |

---

## G. Owner questions

1. **Reply in the notification — out of the first 0.5?** Recommend **yes**: appear, expand, go there. (Alt: keep it paste-only, no Enter. Alt: keep Enter — then it must be gated on an "input box is empty" fact that no vendor exposes.)
2. **Which states raise one?** Recommend **Waiting and Failed always; Done under the existing *Turn finished* switch**. (Alt: all four. Alt: Waiting only.)
3. **Drag-out → 0.5.x?** Recommend **yes**. (Alt: keep it, and rule its five input-ownership questions first.)
4. **Select-and-comment — 0.5.x, arriving with the reply as one paste-only mechanism?** Recommend **yes**. (Alt: capture in 0.5, delivery in 0.5.x.)
5. **Stop — out of the first rail and glance card?** Recommend **yes**, keep *go there*. (Alt: keep it only for Folio-launched panes.)
6. **Mute silences the notification only — not the dot, not Failed?** Recommend **yes**. (Alt: mute also silences Done.)
7. **The badge counts this window's dots; the rail carries the others?** Recommend **yes**. (Alt: the badge is the application's.)
8. **The toast's third line** — `Anthropic · 19:00` alone, or one word? Recommend **one word**. (Still §11.11 Q6.)

Everything else in §C is ruled or deferred above and needs nothing from him.
