# Codex's adversarial review of the 0.5 functional design, revision 3 (2026-09-20)

**F1 — One state word is being asked to represent incompatible facts. — BLOCKS-DESIGN**

**Claim:** The ledger owns the seven states (§3, §11.5), while focus clears the pane’s dot (§12.2).

**Breaking scenario:** A Waiting pane gains focus without receiving an answer. Its dot must disappear, but its wait remains. A completed pane can likewise lose its dot without its last task ceasing to be completed. If focus changes the underlying state to Idle, the protocol loses the completion or wait; if it does not, the implementation needs more than one state variable. Account exhaustion introduces another independent fact.

**Smallest change:** Define ledger-owned facts separately: agent lifetime, observed turn phase/outcome, outstanding waits, account quota, and attention acknowledgement. Derive the single displayed word and dot from those facts. Focusing acknowledges attention; it does not resolve a wait, recover quota, or alter the task outcome exposed on the wire.

---

**F2 — “All states” is not a per-install evidence contract. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Claude Code has “all states,” and the other families have the stated degradation floors (§11.8).

**Breaking scenario:** Ticket 2 treats a mapped event as an installed event, a completion as proof of input readiness, or an ordinary Enter as a submitted agent prompt. Those are different claims. The current wire carries `capability`, `family`, `event`, optional `id`, and bounded `text`; it does not carry an input-ready assertion or general session/turn identity. See [Message and its encoding](crates/bt-app/src/attention_wire.rs:89).

**Smallest change:** Add a per-install capability matrix, with this minimum evidence boundary:

| Agent/lane | Working | Waiting | Done / Idle | Failed | Limited | Exited |
|---|---|---|---|---|---|---|
| Claude Code | Installed `UserPromptSubmit` | Installed permission, elicitation, agent-input and quota-wait events, preserving their different meanings | `Stop`; acknowledgement may remove unreadness, but does not prove an empty input editor | Installed `StopFailure` | Separate attributed quota source; quota-wait hook is not a percentage reading | `SessionEnd`, applicable shell command-end, or owned process death |
| Copilot CLI | `userPromptSubmitted` | Two mapped notification kinds | `agentStop`; same Idle limitation | No mapped terminal-failure evidence | No quota adapter specified | `sessionEnd` or applicable process/command boundary |
| Codex, notify install | No hook evidence | No hook evidence | `agent-turn-complete`; same Idle limitation | No distinct mapped failure evidence | Separate account rate-limit adapter | Applicable process/command boundary |
| pi | No mapped start | No mapped wait | `agent_settled`; same Idle limitation | No mapped failure evidence | No source specified | Applicable process/command boundary |
| Kimi, OpenCode, Hermes, GLM harnesses, Gemini CLI without hooks/OSC | None | None | None | None | Kimi’s proposed account reader only; no general source for the others | Applicable process/command boundary |
| Generic tty lane | OSC command execution is not necessarily an agent turn | `RequestAttention=yes`; withdrawal by `no` | BEL and text announcements alone do not prove successful task completion | No generic task-failure proof | None | Applicable OSC 133 command-end |

Initial Idle also needs evidence; “no event yet” is unknown. Missing or expired evidence must remove the assertion or mark it stale, not manufacture another state. Each installation must advertise what actually installed successfully.

---

**F3 — The answer to the earlier stuck-Waiting finding is factually wrong. — BLOCKS-DESIGN**

**Claim:** Answering Claude’s permission prompt submits a prompt, so `UserPromptSubmit` clears Waiting and “nothing new is needed” (§11.5).

**Breaking scenario:** The user approves a tool, navigates a permission menu, or supplies an elicitation response. These are not necessarily new main-thread prompt submissions. The code distinguishes `PostToolUse` and `ElicitationResult` receipts from prompt boundaries. Moreover, the current ledger acknowledges waits on keyboard input, paste, mouse buttons and mouse wheels—not just a completed answer. Pressing an arrow can therefore acknowledge a wait while the question still stands.

The distinction is explicit in [receipt clearing](crates/bt-app/src/attention.rs:1259) and [user-input classification](crates/bt-app/src/main.rs:19574).

**Smallest change:** Withdraw §11.5’s explanation. Separate “the user interacted” from “the producer resolved the request.” Define resolution evidence per wait kind, including receipt ambiguity and expiry. Expiry means the evidence became stale; it does not establish Working or Idle. The ten-minute clock only covers strong credentials, and repeated arrivals can renew it; it is not a universal correctness guarantee for every wait.

---

**F4 — The one-second clearing rule suppresses genuine requests. — BLOCKS-DESIGN**

**Claim:** Any `ClearScope::All` also clears waits minted during the following second (§11.5).

**Breaking scenario:** `UserPromptSubmit` arrives, then the agent requests permission 100 ms later. Both belong to ordinary execution, yet the proposed rule removes the genuine permission request. Conversely, an old request delayed by 1.1 seconds survives. A delayed Stop can also clear a newer turn’s request.

**Smallest change:** Remove the time-window rule. Correlate events by agent lifetime and source-supported turn/request identity where available. Where the upstream contract cannot distinguish a duplicate from a new request, document that ambiguity and retain conservative level semantics. Receipt time cannot establish causal order. Require replay cases for a fast new permission, a late old permission, duplicate Stop, and Stop crossing the next submission.

---

**F5 — Pane identity does not solve agent lifetime or discovery. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** One running agent per pane makes the ledger key settled; rows disappear on exit; hookless vendors can show their mark and title (§11.3, §11.5, §11.8).

**Breaking scenario:** Agent A exits and agent B starts in the same shell pane. The pane credential remains valid, so A’s delayed hook can change B’s row. A hooked agent that crashes without `SessionEnd`, inside a shell without OSC 133, is no more observable than the hookless case §11.5 acknowledges; its Working assertion can remain indefinitely. Separately, a hand-started hookless program emitting no OSC meets neither of the two recognition contracts, so Folio cannot produce its promised marked row.

**Smallest change:** Keep pane identity as the row key but add a replaceable agent-lifetime identity and retirement rules. Reject events from retired lifetimes where attribution permits it; otherwise expose uncertainty. State explicitly that silent, hand-started hookless agents are unlisted. If Folio-launched profiles may establish identity, name that as an additional authoritative source. Generic OSC may establish a generic signalling process, not a known agent vendor.

---

**F6 — “Focused pane” needs one application-level definition. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** The focused pane has no dot; other panes do; tab and badge project those dots (§12.2).

**Breaking scenario:** Every tab remembers a selected pane, including tabs in background or minimised windows. Treating those remembered selections as focus clears unseen notifications. Conversely, focusing pane A must not clear its tab’s dot if sibling B still owes attention. Opening a notification editor also takes the keyboard away from the terminal while the selected pane remains unchanged.

The existing input predicate already distinguishes window focus, active tab, selected terminal and keyboard owner: [seat_holds_the_keyboard](crates/bt-app/src/main.rs:18692). The shipped attention pass does not simply implement the new pane rule unchanged.

**Smallest change:** Define effective pane focus from the authoritative keyboard owner, including future float views. Background tabs and minimised windows have none. Record acknowledgement against the arriving event generation; moving focus during expansion must not acknowledge a newer event accidentally. A tab’s dot is the aggregate of its remaining pane dots, and the application badge counts each pane once.

---

**F7 — Notification dismissal, acknowledgement, mute and resolution are still conflated. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Every arrival appears, notices leave automatically, mute is per agent, and unhandled notifications accumulate in the badge (§12.1–§12.2, §11.7.5).

**Breaking scenario:** A focused agent finishes: it receives a notification and no dot. The notification expires, so it cannot subsequently “accumulate” in a badge defined exclusively by dots. A muted Working agent then fails: nothing specifies whether failure bypasses mute. A pane closes while its notification or reply draft remains open; an eventual click must not target a replacement pane.

**Smallest change:** Give notifications an explicit lifecycle independent of dots. Define:

- Which transitions notify, including Failed, Limited and unclassified announcements.
- Mute’s scope and lifetime; whether failure is included; no retrospective replay on unmute.
- Automatic dismissal leaves pane debt unchanged.
- Focused arrivals can expire without entering the badge; outstanding waits remain discoverable independently of unreadness.
- Closing/replacing the target invalidates every action. Preserve an existing draft as selectable text, and never redirect it.

---

**F8 — “Every notification appears” has no bounded delivery or hand-over policy. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Notifications appear in Folio, or through the system when Folio is not foreground (§12.1).

**Breaking scenario:** Fifty agents finish together. Unbounded stacking covers the application; a capped stack silently drops arrivals; serial display can present obsolete completion notices minutes later. A background-window agent can also generate both an OS notice from its owning window and an in-app notice in the foreground Folio window. If Folio loses foreground while a card expands, independent delivery decisions can duplicate or lose the notice.

Today the hook inbox already drops oldest entries beyond 256, and turn-end delivery has its own deduplication latch: [inbox bound](crates/bt-app/src/attention_wire.rs:79), [turn-end gate](crates/bt-app/src/attention.rs:1570).

**Smallest change:** Add one application-owned delivery arbiter with event identity, numeric visible/queued limits, fairness, coalescing rules and an overflow representation that retains every affected pane. Coalesce duplicate evidence, not independent agents’ needs. Route other windows’ arrivals into the foreground Folio window. Define one hand-over rule—preferably delivery is claimed once presentation begins, and foreground changes route subsequent notices only. OS activation must resolve the original live identity and restore its window/tab/pane; a retired target is a no-op.

---

**F9 — The expanded content is not necessarily “the latest reply.” — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Expansion shows the agent’s latest reply, using Claude’s transcript or everyone else’s terminal tail (§11.7.2).

**Breaking scenario:** A Claude permission notice arrives before Stop and expands the previous turn’s conclusion. Codex’s terminal tail contains a status footer or half-typed prompt despite its notify payload providing assistant text. An alt-screen TUI redraws the tail between arrival and expansion. A Failed notice can display an earlier successful conclusion.

The existing Claude reader returns an 80-character lede, not a full expandable reply: [attention_words](crates/bt-app/src/attention_words.rs:61).

**Smallest change:** Bind content to the event and agent lifetime. Distinguish an authoritative assistant message from a terminal snapshot; never describe the latter as a reply. Use Codex’s supplied text. Specify a separately bounded full-message path if full Claude replies are required, including missing files, later transcript growth and absent matching messages. Failed and Waiting notices must not borrow an unrelated conclusion.

---

**F10 — The proposed free-text guard proves absence of evidence, not readiness. — BLOCKS-DESIGN**

**Claim:** Idle/Done plus no Permission/Quota wait makes free-text input “verifiably” safe and excludes permission prompts (§11.7.3).

**Breaking scenario:** Codex emits completion and later shows an approval dialog without a wait hook. Claude’s permission notification is delayed, lost or unavailable in that installation. An elicitation or background-agent wait is present but passes the two-kind exclusion. A local keypress starts another turn before its hook arrives.

None of the described adapters exposes a current, authoritative assertion that the editor accepts ordinary text, is empty, and still belongs to that completion.

**Smallest change:** State that **no currently described adapter qualifies for guaranteed safe auto-submit**. A strict “never for permission” guarantee requires an agent-side semantic submission operation that validates the expected input/request generation when accepting the reply. Until that exists, offer a retained draft and “go there”; do not enable auto-submit from a negative wait test.

---

**F11 — Paste plus Enter needs an editor and transport contract. — BLOCKS-DESIGN**

**Claim:** Notification reply is equivalent to typing, and any new event disarms it (§11.7.3).

**Breaking scenario:** The terminal already contains `Please delete…`; the notification appends a different answer and submits the combined text. Without bracketed-paste mode, embedded newlines can execute before the final Enter. With bracketing, a TUI may stage the paste or interpret Enter differently. The agent can change prompts after the last local check but before queued PTY bytes arrive.

The existing helper conditionally brackets, normalises newlines to carriage returns, clears selection and returns the viewport to the bottom: [paste_text](crates/bt-app/src/main.rs:118739), [paste_bytes](crates/bt-app/src/input.rs:796).

**Smallest change:** Specify per-agent paste/submission capabilities, multiline treatment, existing-editor handling and write-failure behaviour. Never clear or replace an unseen draft automatically. Unsupported multiline delivery must remain staged outside the PTY. Validate target lifetime and local input revision immediately before enqueueing; make an allowed paste-and-submit one ordered write with no interleaving. Explicitly state that this local check still cannot close the agent-side prompt race identified in F10.

---

**F12 — One PTY and one size do not define two interactive viewports. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** The float is a second interactive view, supported conceptually by existing floats and thumbnails (§11.7.4).

**Breaking scenario:** Scrolling the float changes the source pane’s scroll position; selecting in one clears selection in the other; pasting into the float snaps the source to the bottom. A scaled float forwards mouse coordinates from its own pixels instead of the terminal’s canonical cells. IME composition starts in one view and commits after focus moves to the other.

**Smallest change:** Define one terminal parser/grid/mode owner, with separate view identities owning scroll anchors, selection and geometry. Exactly one view owns a keyboard composition at a time; composition never migrates implicitly. Pin pointer gestures to their originating view and geometry generation. Mouse reporting uses canonical terminal coordinates; local selection and history scrolling use the view’s own projection. State what happens when the selected history is evicted.

---

**F13 — A second view must not become a second terminal-protocol participant. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Shared interactive views are the first instance of the future shared-session model (§11.7.4).

**Breaking scenario:** Source and float independently report focus, alternately sending `CSI I/O` for the same PTY. Both answer a cursor-position query. Opening a notification composer incorrectly leaves the terminal focused. A focus report sent through the user-input path acknowledges a standing wait—the exact class the existing code prevents.

The current owner performs focus reconciliation after feeding terminal bytes and before draining replies: [drain_leaf_pty](crates/bt-app/src/main.rs:37264).

**Smallest change:** Compute terminal focus once from the actual input owner across all attached views. Switching between two views of the same terminal does not create a terminal focus transition; entering a Folio composer does. Preserve repeated `?1004h` subscription handling. Only the canonical terminal actor answers queries and drains protocol replies; views never do, and protocol replies never traverse the user-answer path.

---

**F14 — Float lifetime and resize authority remain undefined. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** The float uses the pane’s dimensions, scaled or clipped (§11.7.4).

**Breaking scenario:** The source pane is resized, moved to another window, closed, or its agent restarts while the float remains. It is unclear whether closing the source kills the PTY, transfers ownership or leaves a dangling view. Dragging the float larger must not resize the PTY indirectly. Several floats can also multiply rendering work without a stated limit.

**Smallest change:** For the first implementation, keep PTY lifetime and size owned by the source pane. Float resizing changes only presentation; source resize updates the canonical size once and invalidates dependent hit geometry. Source retirement detaches/disables all views immediately; closing a float only detaches that view. Specify cross-window movement, maximum attached views and hidden/idle rendering budgets. These are ticket-7 obligations, not properties inherited from the float chassis.

---

**F15 — Tier 2’s repair leaves created-pane authority without a lifetime. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** An agent may type into panes it created; grants follow session and tab scope (§6, §11.9).

**Breaking scenario:** Agent A creates pane B. The user later starts an unrelated interactive program in B, moves B to another tab, or replaces A with another agent in the same source pane. A pane-level “created by” flag can now authorise the wrong session or bypass the tab boundary. “Predates the grant” also does not classify a human-created pane added after the grant.

**Smallest change:** Define created-target authority as a relation between a particular source agent lifetime and target lifetime, subordinate to current scope. Target replacement, source retirement and grant revocation invalidate it; movement requires scope revalidation. All targets outside that relation require the typing tier, regardless of creation time. Keep credential attribution separate from these grants.

---

**F16 — A revision on “everything” is not yet a concurrency model. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Reads return revisions; writes naming stale revisions are refused (§12.4.7).

**Breaking scenario:** A terminal spinner changes its screen revision continuously, starving legitimate writes. Alternatively, a revision tracking only screen content misses a user keystroke whose echo has not arrived. A pane moves tabs after authorisation but before execution. A closed object is replaced with another whose counter also starts at zero.

Existing render revisions have specific meanings, not universal write-precondition semantics; [focus_thumb’s damage keys](crates/bt-app/src/focus_thumb.rs:247) already distinguish identity from revision.

**Smallest change:** For each initial write verb, specify stable object identity plus lifetime, authoritative revision owner, exact invalidating changes and the complete read set. Check permissions and revisions atomically with the mutation on the owning executor, after any asynchronous wait. Use separate semantic/input/layout revisions where needed, rather than every painted frame. Return a structured conflict with current revisions. A Folio revision protects Folio-owned state; it cannot guarantee the state of a TUI or external page at eventual input consumption.

---

**F17 — The log schema cannot yet support the promised metrics or failure accounting. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** A bounded action log stores agents only, also records Folio notification/focus events, and supplies interruption counts and durations (§11.9, §12.4.7).

**Breaking scenario:** A wait begins before oldest-first eviction and ends afterwards; the duration query invents a start or omits it silently. Multiple waits share a pane, but the schema has no episode identity. Focus occurs without an answer, so “response delay” becomes an inaccurate name. Disk failure after an action leaves no record despite “every action” being recorded.

**Smallest change:** Define explicit record classes for agent actions and payload-free Folio attention events. Add lifetime/episode/action IDs, start/end causes and monotonic elapsed-time semantics. Name numerical record, queue and disk bounds, retention-window behaviour, and partial/censored metrics. Define whether writes are refused when audit recording is unavailable. Record enqueue/acceptance separately from actual completion; a PTY write is not proof that an agent obeyed it. Remove any surviving generic “undo” promise.

---

**F18 — Quota attribution still invents an account for the least observable vendors. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Hook-child environment identifies accounts; a vendor without hooks simply shows one account (§11.8).

**Breaking scenario:** Two hookless Kimi sessions use different configurations, while Folio’s quota child reads the default account. Showing one account does not make either session attributable to it. OpenCode or Hermes can use different model providers; program identity does not identify the quota vendor. Two configuration directories can also authenticate the same provider account.

**Smallest change:** Specify an adapter contract per source:

- Anthropic: attributed statusLine sample; unavailable when installation is declined.
- Codex: rate-limit reader bound to the reporting configuration/account, with sparse-update merge rules.
- Kimi: explicitly identified configuration for its server child; otherwise no session association.
- Copilot, pi, OpenCode, Hermes, GLM/Z.ai and Gemini: unavailable unless a separately evidenced adapter exists.
- DeepSeek: balance is not an exhaustion percentage; either specify the opt-in key exception or omit it from 0.5.

Unknown attribution must remain unknown, not become “one account.” Define whether the identity represents a configuration or an actual provider account before aggregating it.

---

**F19 — Reset time and a new turn do not prove quota recovery. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Limited clears at `resets_at` or any later account turn start; all exhausted in-use accounts make the chip red (§11.5, §11.9).

**Breaking scenario:** The five-hour window resets while the weekly window remains exhausted. Another session submits a prompt on that account and immediately hits the same limit. An Anthropic sample passes its reset time without a new push. One known account is exhausted while another in-use account is unreadable: neither “all exhausted” nor “usable” is established.

**Smallest change:** Model account availability as available/exhausted/unknown over the applicable limit buckets. A bucket is exhausted only on fresh evidence; passing reset invalidates its old reading and schedules an allowed refresh—it is not a recovered sample. A submission updates that session’s turn evidence, not the account’s remaining quota. Define the all-exhausted condition for a nonempty, fully attributable set and represent unknown coverage separately. Recovery toasts require observed recovery. Also define initial samples, out-of-order samples, bucket changes and re-arming across resets.

---

**F20 — The first two visible tickets still contain unsupported actions and data. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** Recent is independent and shows who created/edited files; the rail ticket is read-only but includes Stop (§4.8, §11.3.5, §11.10).

**Breaking scenario:** A printed pathname establishes a mention, not an edit or creator. Current hook messages discard tool payloads, so “file-write events” have no specified producer. A hand-started agent inside a shell has no identified process handle for a precise Stop operation; Ctrl+C and killing the pane’s process tree have different consequences.

**Smallest change:** Ship Recent initially with evidenced mentions and user opens, resolving relative paths against the event-time directory and leaving authorship absent when unknown. Add created/edited only with a defined producer. Define Stop’s target and semantics per launch shape, or omit it from the initial rail. The rail ticket should not call itself read-only while containing a process-control action.

---

**F21 — The sequence needs acceptance gates and a smaller first release. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** The eight-ticket sequence is independently shippable (§11.10).

**Breaking scenario:** Ticket 4 ships ephemeral notifications before ticket 5 supplies their durable discovery path. It bundles the unsafe reply mechanism into arrival delivery. Ticket 8 introduces instrumentation only after the events needed for earlier metrics have occurred. Ticket 7 relies on an input/view architecture not established by any preceding ticket.

**Smallest change:** Revise the sequence and its acceptance columns:

- **Recent:** synthetic path/open events, attribution and eviction fixtures.
- **Ledger:** recorded/synthetic hook payloads, fake clock, duplicates, reorderings, missing clears and lifetime replacement.
- **Rail:** fake application-wide registry, minimised windows and stale targets.
- **Notifications plus debt list:** one delivery arbiter, fake foreground/OS sink, mute, closure and storm tests.
- **Quota:** captured response fixtures and fake time; explicitly separate adapter acquisition from presentation.
- **Protocol reads:** fake identities and scopes; defer writes until verb-specific revision contracts exist.

Keep notification auto-submit, the interactive second view, mutating protocol tiers, select-and-comment delivery and web driving out of the first 0.5. Add the minimal attention instrumentation with the ledger if 0.5 claims metrics. All functional state/routing contracts should be testable without a real agent; final terminal-input compatibility still needs a terminal probe, and vendor readiness claims need vendor evidence.

---

**F22 — Select-and-comment does not conflict with copy-on-select, but it cannot inherit a safe send path yet. — MUST-DECIDE-BEFORE-TICKET**

**Claim:** The owner’s new request could reuse notification reply (§11.7.3); select-and-comment was deferred in §11.10.

**Breaking scenario:** Selection copies successfully, then another selection overwrites the clipboard before the comment action. Live output changes the selected screen region. An alt-screen agent owns mouse gestures. The selected text comes from an earlier agent lifetime, while the pane now runs a shell. A multiline insertion submits lines despite omitting a final Enter.

**Smallest change:** Adopt this functional model:

- Selection release continues to copy normally. Commenting is a separate explicit selection command, reachable through the terminal context menu and a keyboard command; release itself never sends.
- The command captures the selected **plain text directly from the selection**, together with source pane/lifetime and optional observed turn metadata. It never reads the clipboard as its source.
- Open a Folio-owned comment draft containing the frozen quotation and the user’s comment. Deliver a bounded, escaped structure such as `Quoted terminal text:` followed by quoted lines, then `Comment:`. Show any truncation before insertion.
- Terminal row numbers are not source-file line references. Include `file:line` only when independently established; otherwise use the quotation itself.
- In mouse-reporting TUIs, ordinary gestures stay with the agent. Use Folio’s existing Shift-selection override for local capture. Main-screen scrollback is selectable while retained; alt-screen capture is limited to the available screen/history, without promising a conversation transcript.
- Bind delivery to the selected agent lifetime. Agent replacement disables delivery. Capture survives output changes and scrollback eviction once the draft exists.
- **No auto-Enter.** Preserve any existing agent draft. Insert only through a supported paste/editor contract; otherwise retain the Folio draft and take the user to the pane. Multiline text without safe paste handling must not be written automatically.

Keep full select-and-comment delivery in **0.5.x**. Selection capture is straightforward; reliable insertion is the unresolved part of §11.7.3, so sharing that path does not remove the dependency.

**UI suggestions — advisory**

- Distinguish a terminal snapshot from an authoritative assistant reply.
- Keep disarmed drafts selectable, with an explicit route to their original pane.
- Give muted agents a discoverable indication without suppressing their state.
- Expose the selection-comment command without replacing copy-on-select.
