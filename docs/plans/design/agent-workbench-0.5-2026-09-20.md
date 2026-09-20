# The agent workbench: Folio 0.5

Status: **total design note, 2026-09-20.** This theme is 0.5; remote is 0.6 (owner, 2026-09-18). The structure and the interaction rules below are settled and are the input to ticket writing. The visual design of every surface named here is **not** settled and is not settled by this note.

## 1. Status, and how to read it

Over one long session the owner ruled on an interactive mock, twelve rounds. His closing statement (2026-09-20) is the reason this note exists: the **interactions** are settled; what is still unsatisfying is **visual** — the icons, the way information is laid out, the buttons — and the mock does not look like the real product, so polishing it further buys nothing.

Therefore:

> **The mock is frozen. It is the reference for STRUCTURE, INFORMATION ARCHITECTURE and INTERACTION RULES only.
> It is NOT a visual specification.**

The look of each surface is decided **when that surface is built in the real GPU-rendered product and reviewed on a real window** — the house practice since 2026-08: a new surface gets its look ruled at birth, in front of the owner, on the thing itself. Everything the mock says about pixels — a radius, a colour, a mark, a gap — is a measurement of an HTML page, not a ruling about Folio.

Two markers are used throughout and never mixed:

- **RULED (structure/interaction)** — decided. Written as a testable statement. A ticket may not quietly change it.
- **OPEN (visual, decided at birth)** — deliberately undecided. The ticket that builds the surface carries the question to the owner, on the real window.

**Where the mock lives.** Outside the repository, by design: a working copy in the session scratchpad and the owner's copy at `D:\Developer\trace\design05\wireframe\` (`mock.html`, `icons-compare.html`, and `NOTES.md`, which records all twelve rounds with the owner's words). It is a 185 KB single page with no external resources; it will not be committed, it will not be maintained, and it will drift. **This note is the record.** Where this note and the mock disagree, this note wins; where this note is silent, the question is open, not delegated to the page.

**Evidence authority for this theme** (CONVENTIONS §十): this note for structure and interaction; the owner on a real window for anything visual; `docs/design/ui-mockup.html` and `crates/bt-render/src/{theme,scheme}.rs` for the existing design language, with the code winning over the master where they disagree.

## 2. The problem, and the position

The bottleneck of working with several agents is not the agents. It is **the attention of the one person in the middle** (owner, 2026-09-20). Five sessions can run; one person can answer. Every minute he spends finding out *who needs him* is a minute not spent answering.

**Folio's job is attention routing.** Not scheduling, not delegating, not deciding: making sure that every glance the person spends lands on the one thing that actually needs him now. Concretely, and these are the only claims this theme makes:

1. Order by **who is waiting for what from me**, not by when a session was created.
2. Show **what is being asked** where he is already looking, and let him **answer on the row** without switching.
3. Summarise a finished turn in three lines, so five finished agents are not five long reports.
4. Let him **pre-authorise the judgements that can be written as rules** ("read-only commands always allowed"), have Folio answer them for him, and **leave a trace of every such answer**.
5. **Count the interruptions** — waits, wait durations, his own response delay. That number is the only way to know whether any of this helped.

**What Folio is not**, and these are refusals, not omissions:

- **Not an orchestrator.** No built-in controller agent, no task graph, no queue, no deployment pipeline. Folio shows live sessions, not tickets. The kanban screenshot the owner brought (209 cards, 27 awaiting review, a "bulk approve" button) is a photograph of the bottleneck, not a cure — "bulk approve" is the rubber stamp that backlog forces, and it is the direction to avoid.
- **Not an IDE.** No editor core. No VS Code extension compatibility: the API surface presumes an editor and a workbench Folio does not have, it needs a Node extension host, and the marketplace terms restrict extensions to Microsoft's own products (owner ruled, 2026-09-20).
- **Not a plugin host.** No in-process extension points, no dynamic loading, no public API promise beyond the one named in §6. Extensibility means **outside programs driving Folio through a small, versioned, public interface** — the `folio` CLI and MCP surface. In a terminal, every command-line program is already the plugin.

The owner's own words on the payoff, worth keeping because it is also the pitch: *"folio 命令以及 mcp 真的很棒……folio 整个也都是可以被 agent 所操控的"* — the whole of Folio can be driven by an agent.

## 3. Vocabulary

Four things are routinely confused and must not be:

- **Agent program** — the thing running in the pane: Claude Code, Codex, Kimi CLI, Copilot CLI, OpenCode, Hermes, pi. This is the first-level identity. **RULED: the mark on a row is the agent program's mark.**
- **Model** — what the program is talking to: Opus 5, GPT-6, DeepSeek-V4, Qwen3-Max, Hermes 4. Second level only.
- **Vendor** — who owns the model, and separately who owns the program. A third-party shell wears its own mark and names the model's vendor at second level, with a dim `via <shell>` **only when the model is not the shell's own** (Hermes running Hermes 4 carries no `via`).
- **Account** — which configuration directory a session runs under (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `KIMI_CODE_HOME`). Quota belongs to the account, not to the vendor and not to the session.

Three more terms this note uses as defined here:

- **Session** — one agent program's conversation in one pane. **Proposed invariant: the primary key of every list in this theme is the PANE, not the session** — one pane, one row, showing the live session; dead sessions go to the pane menu's `Resume ▸`, which already exists. Without this, a pane that has been restarted three times grows three rows. *(Open — §10 Q3.)*
- **Tab = workspace.** A tab is the unit of work and the unit of reach. An agent's default reach through Folio's tools is its own tab.
- **Row** — the two-line component that represents one pane-with-a-session. One component, three hosts (§4.2).

### 3.1 The seven states

**RULED (display).** One word each, and the same seven in both languages:

| English | 中文 | Meaning | Master's visual vocabulary (existing) |
|---|---|---|---|
| Idle | 空闲 | Alive, nothing to do, waiting for your next prompt | no dot, no breathing |
| Working | 在跑 | A turn is running | `.ticon.working` breathes; **no dot** |
| Waiting | 在等你 | Stopped on a question only a person can answer | `.unreaddot.await` — `--warn`, pulsing |
| Done | 做完待看 | A turn finished and has not been looked at | `.unreaddot` — `--accent`, static |
| Failed | 出错 | The turn ended badly | `.unreaddot.fail` — `--err` |
| Limited | 额度用完 | The account's quota is exhausted | `.unreaddot.bell` — `--warn`, **static, never pulses**: you can do nothing, so nothing should flash |
| Exited | 已退出 | The process is gone; the session may be resumable | no dot |

**RULED: the accent colour means UNREAD in this product, not "needs you"; "needs you" is orange.** This is the master's existing semantics and it is the opposite of the first wireframe's. The wireframe lost.

**RULED: Idle is not an event.** It does not breathe, does not light any dot, and does not enter any count. Idle is rest.

**RULED (protocol).** The CLI/MCP surface reports A2A `TaskState` values where they exist, so a consumer need not know Folio: `working` · `input-required` (Waiting) · `completed` (Done — "unread" is Folio-local and does not go on the wire) · `failed` · `canceled` **only when the user ended it** (a process that exited on its own is not a cancelled task, and is reported with no A2A state). **Idle and Limited have no A2A equivalent and are namespaced Folio extensions** (`x-folio/idle`, `x-folio/limited`), documented as extensions so a consumer that does not know them can ignore them safely. Quota is a fact about an account, not about a task; that is why `Limited` cannot be standard.

### 3.2 The state machine, and who is allowed to say

**RULED: one fact, one owner.** The session ledger — today's `AttentionLedger`, extended — is the **owner** of every session's state. The tab dot, the rail row, the pane header, the attention badge, the glance card, the toast and the protocol are all **projections**. No surface derives state from terminal text on its own; no surface stores a second copy across frames. If two surfaces disagree, the ledger is right (CONVENTIONS §十 rule 3).

Transitions, and the evidence that produces each:

| To | From | Evidence |
|---|---|---|
| Working | Idle, Done, Failed, Waiting | turn-start hook; or the user's own submission in a pane Folio owns |
| Waiting | Working | a permission/confirmation hook carrying the question text |
| Done | Working | turn-end hook, with the first sentence of the last main-thread assistant message (§7.51) |
| Failed | Working | a turn-end-with-failure hook; **never quoted**, because a turn that did not finish has no conclusion |
| Limited | any | the account's quota reading crosses to exhausted |
| Idle | Done, Failed | the user has looked at the row (unread is cleared by looking, §5) |
| Exited | any | the child process is gone |

**RULED: what is not knowable is not shown.** Per program, today:

- **Claude Code** — rich: turn start, tool use, turn end with quotable words, permission requests; plus `statusLine` for model, context %, cost and rate limits. All seven states reachable.
- **Codex** — `notify` fires **only at turn end** and carries `last-assistant-message`. Working can be inferred from the user's own submission through a pane Folio owns; Waiting is **not** observable. The app-server gives account, plan and rate limits. A Codex row is honestly poorer than a Claude Code row.
- **Copilot CLI** — `agentStop` exists; its transcript format has no quotable upstream text, so no words (§7.51's "we only write a row we have evidence for").
- **Kimi CLI, OpenCode, Hermes, pi, GLM through any harness** — nothing but process liveness today.

**RULED: a row degrades to state only.** A program with no hooks shows its mark, its title, `Working`/`Exited` from process liveness, and nothing else — no empty ring, no blank todo line, no `—`. **Absent is absent** (§7).

**OPEN (visual):** whether a state-only row should look visibly thinner than a rich one, or keep the same two-line rhythm with the second line carrying only the state word. Decided at birth, on a rail holding both kinds at once.

## 4. The surfaces

### 4.1 The Agent rail

**Purpose.** One flat list of every agent session, across the tabs of this window, ordered so the person can answer. The owner's formulation: the tab strip says *where things are*; the Agent rail says *who needs me*.

**RULED.**

- A **second section of the side rail**, below the tab list, behind a drag handle. `flex: none`, capped at **50 % of the side-rail height** or wherever the user drags it. Expansion happens inside its own scroller, **so the tab list above never moves and never shrinks.**
- **One component, three hosts**: vertical = the rail's foot; cards = the foot of the card column; horizontal = a panel dropped from the tab strip by the attention badge. **The row is identical in all three** — two implementations of a row is how two surfaces end up three pixels apart and a hand never learns where to reach.
- **Hand-started agents are first-class.** A session started by typing `kimi` into an ordinary PowerShell pane appears exactly like a dedicated agent pane; `pane.kind` and `pane.session` are decoupled, so that pane's header stays a **shell header** with the session appended. Recognition, in order: the agent's own hook (reports however it was started, and is the reliable one) → shell integration's view of the command line → process information. **If unsure, do not list it.**
- **Ordering is CREATION ORDER and does not change when a state changes** — a row must not jump out from under the pointer. (§4.4's list is the one that sorts by urgency; opening it *is* the question "who needs me".)
- **Clicking a row switches to that tab AND puts focus on that pane.** Allow/Deny act in place and do not navigate.
- **Collapse is a button** at the head of the section, not the whole header row (§5).
- The side rail widens **220 → 280 px** when the agent section is present, for the master's own reason: a column of names and a column of "a question plus two buttons" are two different problems. Measured at 220, title, tab name and question all truncate past reading.

**OPEN (visual):** the section header's typography and counts; the drag handle; whether the rail reads as one surface with the tab list or as a separate panel; the empty state's wording and height.

**Open (structure, §10 Q1/Q3/Q4):** this window or the whole application; the pane-vs-session key; sub-agents that block.

### 4.2 The agent row

**RULED.**

- **An agent row is a tab row that is two lines tall.** It shares every property that makes a row a row with `.vtab`: height rhythm, padding, radius, hover and selected fills, icon position, title position, the `⌄` at the right.
- **Line one** = `mark · title · [context ring + %] · [one small measure] · ⌄`. The mark is the **agent program's** (§3, §4.9); the model is not on line one. The one small measure is `⑂N` when there are sub-agents, else the clock for **this turn** while Working, else nothing.
- **The context ring is drawn whenever the vendor gives the number** — no threshold; three bands (dim < 60 %, `--warn` 60–85 %, `--err` > 85 %); a row with no number draws nothing. **When the row narrows, the PERCENTAGE yields first and the ring never yields** — a ring can still say "nearly full", a number with nowhere to stand says nothing. (Measured: at 280 px the title floor is ~101 px; at 220 px the percentage collapses and the title returns to ~69 px.)
- **Line two** = state word, then the activity or the place, dimmed, indented to the title's left edge: `Idle · <folder> · <branch>` (idle yields the space to *where*) · `Working · reading bt-app/src/tabs.rs` (busy, the activity wins and the place goes) · `Waiting · <truncated question>` plus inline **Allow / Deny** · `Done` · `Failed · 3 of 44 tests did not pass` · `Limited · 19:00` · `Exited` with an inline `Resume`. A known todo count is right-aligned as a dim `3/7`.
- **RULED: the state word never truncates.** It is `flex: none`; every missing pixel comes out of the activity and the ellipsis falls there. Measured, both languages: state words truncated = 0.
- **The `⌄` is always present on an agent row** — resting ~.45 opacity, hover .8, pressed/open 1, **24 × 24 hit box**. It is the only way into the second level and may not be a hover secret. *(This was the mock's worst defect: it matched no rule that unhid it, so it was `display: none` with a 0 × 0 hit box through four rounds of "passing" tests — the owner's report was "没法展开也没有 hover" (can't expand, and there's no hover). Acceptance therefore needs a **reachability witness**: a real pointer, a measured hit box, a topmost-element check, then a real click.)*
- **`in <tab>` moves into the tooltip**, and returns to line two only when **two session titles collide**.

**OPEN (visual):** the two-line proportion; where the state word's colour comes from; the inline Allow/Deny buttons' shape and weight — they are the only buttons in a scrolling list and they are the thing the owner reaches for; the `⑂N` glyph; the ring's stroke; the `3/7`'s placement.

### 4.3 The glance card (second level)

**RULED: the second level is a glance card, not an expander and not a detail bar.** Every tab in this window already has a glance card; an agent row is a kind of row, so it gets one. In-place expansion and the detail bar were both built and both withdrawn: in-place expansion put a 522 px panel inside a ~370 px frame and made the list grow under the pointer; the detail bar took ~220 px off the terminal and sat too far from its row.

- It is the **`.file-peek` family, not `.layout-peek`** — it carries Allow / Deny / Go to / Stop / Resume, and **a card with something to reach for must be enterable** (the master ruled `.file-peek` enterable on 2026-08-14).
- **Opens on hover after 350 ms; the `⌄` pins it.** Pinned ignores hover until Esc / ← / a click elsewhere / the `⌄`.
- **Corridor**: row + gap + card is one corridor; leaving it starts a **220 ms** grace; while the pointer is inside the card, other rows do not steal it. Both numbers are the master's own.
- **Placement is the master's, and is one function shared with the tab card**: beside the row, 10 px right, flipped left when it does not fit, top-aligned, clamped 8 px from the screen edge. **Two cards placed by two similar-looking functions end up three pixels apart; there is one `placeBesideRow`.**
- **RULED: two cards are never on screen at once.** Opening one closes the other.
- **Contents, in order** — and **any line whose field is unknown does not appear; there is no empty slot**: ① full path · branch ② model + **the model vendor's own small mark** + `via <shell>` when applicable · reasoning effort · account (only under §4.7) · permission mode · **Folio scope** (§6) ③ context ring with its number · this turn · session duration ④ todo ⑤ sub-agents, each with its own state word ⑥ files changed ⑦ the last reply. ⑧ Actions: **Go to · Stop · Resume** — exactly three.
- **RULED: ④ ⑥ ⑦ are each clamped to one summary line plus `more →`.** Uncapped, the rich card measured 522 px against the poor card's 104 px; the clamp is what keeps a poor session's card small, which is correct.
- **RULED: `Reopen with another account ▸` does not exist** anywhere — not in the pane menu, not in any `Resume ▸` submenu, not in the card's actions. A running session cannot change account: "another account" can only mean *a new session under another configuration directory*, and history does not follow across configuration directories, so the row would promise a continuity Folio cannot deliver. **A menu must not lie.** The front door to a second account is **Profiles** — one profile = one agent program plus its environment; "use the other account" = "new tab, that profile".

**OPEN (visual):** everything about the card's interior — the order is ruled, the layout is not; the pill row ② is the densest thing in the design and is exactly the kind of thing the owner said was laid out wrongly; the three action buttons; the `more →` affordance.

**Noted, needs the owner's judgement (§10 Q6):** for **the session you are currently looking at**, ⑥ and ⑦ are strictly worse than the pane itself — they are `git status` and the last few lines of that terminal. The card earns its keep for sessions in *other* tabs.

### 4.4 The attention badge and its list

**RULED.**

- **One badge in the title bar: a dot and a number** — the app-icon badge, nothing more.
- **The number is the TOTAL**, not the count of the most urgent class. The owner's reason, and it is the better one: a badge is a badge; it does not get a second algorithm.
- **The colour is the most urgent class present**, in the ruled order `Waiting → Failed → Limited → Done` (`--warn` / `--err` / `--warn` / `--accent`). So the two marks answer two questions: the number answers *how many are waiting on me*, the colour answers *is any of them blocking me*.
- **Nothing to show = nothing drawn.** Not a grey zero.
- Hover 350 ms opens **the list**, on the same paper as the glance card; the pointer may enter, a click pins it. **The list is sorted by urgency**, because opening it *is* the question. Rows are the same two-line component, Allow / Deny are in the row, clicking the row goes there.
- **In the vertical layout, while the Agent rail is present, the badge is not drawn** — the rail already is that list, and one answer is not given twice. (It appears in horizontal, in cards, **or** when the side rail is collapsed.)
- **RULED: Waiting and Done are never summed into one number.** One control and one list, yes; one number, no — "is something blocking me" and "how many finished" are different urgencies, and adding them dilutes the only real question. The badge's total counts *items*; its colour keeps the classes apart.

**OPEN (visual):** the badge itself — the owner's complaint about an earlier form was *"这个图标的位置以及圆角这些都 不对"* ("the position of this icon and the rounded corners are wrong"), and the fix came from the master's own two-month-old note on `.attn-chip`: *quiet chrome, loud dot* — the tinted pill body read as foreign furniture; the dot is the signal, the rest is ordinary caption-area chrome that behaves like the gear beside it. That correction is a **ruling of the master's**, and the new badge must be born under it, measured on a real title bar: shared centre line with the gear and the caption buttons, no resting fill, only the glyph takes the band colour.

### 4.5 Quota: the chip, the panel, the toasts

Quota is a fact about an **account**, which is a fact about the **application** — not about a window and not about a tab. It was drawn in the window header, in the tab strip, in the rail head and in a rail footer, and withdrawn from each.

**RULED — the chip.**

- **A chip, not an icon button** (§5): a gauge glyph in the title bar, left of the gear.
- **There is no number on the chip.** A percentage in the title bar is a number nobody reads until it matters, and it matters exactly three times — so the chip keeps only the part worth a permanent pixel: **its colour**. The numbers are delivered by a toast, once, and then leave.
- **Only accounts Folio is CURRENTLY RUNNING A SESSION ON may set the chip's colour or raise a toast.** An idle account at 93 % says 93 % in the panel and nothing anywhere else: it will not interrupt anything.
- **Within that set, an already-exhausted account no longer decides the colour.** Otherwise one burnt weekly window holds the title bar red for days, and a permanent red is a red nobody reads — the account quietly climbing to 85 % would then arrive unannounced. "Used up" is already said three times: once by the toast, continuously by that session's row (`Limited · 19:00`), and by the panel's `--err` bar with its reset time.
- So the chip answers one question: **among the accounts you are using and can still use, how tight is the tightest.** Dim normally, `--warn` at ≥ 80 %, `--err` **only when every in-use account is exhausted** (for a single-account user that is simply "used up"). No sessions at all: dim — there is no work to interrupt.

**RULED — the panel.** Hover 350 ms; pointer may enter; click pins; Esc closes; opens directly under the chip.

- **Every known account is listed**, in use or not: the panel exists to help decide *what to use next*.
- **Ordered most-used first** (not emptiest-first, which was drawn and changed). In-use accounts are marked `in use`, because that is what gives them the right to raise an alarm.
- Grouped by company, then account. One line per window (5-hour / weekly / 7-day) with the percentage, the absolute reset time **and** a dim relative one — `resets 19:00 · in 2h`: the absolute answers "can I schedule around it", the relative answers "should I wait". Both. A dim `read 2 minutes ago` at the foot.
- **Vendors that cannot be read are listed as "cannot read", last, by name** — a row that is simply absent reads as fine.
- **A "resets soon" badge was drawn and withdrawn** — the owner: *"标记就是多此一举"* (a marker is superfluous). The two times are the facts; a badge on top is decoration. The two thresholds proposed with it went with it.

**RULED — the toasts.** The master's existing toast (`#toast-host` / `.toast`): same surface, same position, same motion, same duration. A quota notice is not a new kind of popup and must not grow into one.

1. **One account, one crossing, one toast.** 81 % for two hours is one toast, not one per poll: it reports a *crossing*, and falling back below the line re-arms it.
2. **Three lines only: 80 %, exhausted, recovered.** Recovery is reported **only if a session was actually Limited** — a quota nobody hit coming back is not worth interrupting anyone.
3. **3.4 s**, the master's constant. One duration per card in the whole product beats an extra second of reading.
4. **Hover stops the clock**; leaving restarts it.
5. **Clicking it opens the quota panel, pinned, under the chip** — the toast is the summons, the panel is the answer, and this teaches the chip's location as a side effect.
6. **It never takes keyboard focus** and is not in the tab ring. You are typing in a pane; a notice that steals the caret costs a command.
7. **It never covers the input line.** The host is pinned below the title bar at the top right, furthest from any prompt; a pane's last line is the one rectangle in this window no transient may cover.
8. **Only in the focused window.** Elsewhere your quota is not news; the badge and the chip remember it quietly. (System-level notifications are a separate question, not answered here.)
9. **Two at once stack, they do not merge** — the master's host is a flex column, 8 px gap, append, each with its own clock. Merging needs a new component; stacking is what this product already does.

**Two defects of the master, to be written back** (found by reusing it, not introduced here): `#toast-host` is `pointer-events: none` and neither `.toast` nor `.toast-act` re-enables it — **the master's own Undo button cannot be clicked**. And the master's `.attn-chip` has a chip's rounded body but a click that means "go there", which is neither an expansion nor a button (§5).

**OPEN (visual):** the gauge glyph. The honest finding from the mock: *"the same stroke weight as the gear" cannot be taken literally* — the gear is a solid 24-grid path with no stroke at all, while the rest of that row (`i-panel` 1.1, `i-file` 1.15, `i-pin`, `i-chev` 1.2) is 16-grid, 1.1–1.2 stroke. The gauge should join the house, and **the gear is the outsider that should eventually be redrawn**. Both decided at birth, on the real title bar.

### 4.6 Pane header and tab changes

**RULED (pane header, 30 px — the master's height since 2026-08-12).** It carries the facts that belong to *this pane*: vendor · account · session title · state word · duration · sub-agent count, dropped in this order as it narrows: maximise → duration → sub-agent count → state word → account to one letter → vendor name becomes its mark → account → **the session title only ever truncates, it never drops**. Never dropped: the state dot · the kind icon · `⌄` · `✕` · the Waiting pill · the ask strip. **Narrow headers tell vendors apart by MARK, not by a `CC`/`CX` abbreviation.**

**RULED: the ask strip** — a 30 px strip under the pane header with a 2 px `--warn` left edge, carrying the **full** question and Allow / Deny. It has no equivalent in the master and is new. It stays, for one reason: you are already looking at this pane, being sent to a list in the corner to read what it is asking is worse, and the strip is the only place that shows the question **untruncated** (every list truncates).

**RULED: there are exactly TWO authoritative places to answer** — the ask strip of the pane you are looking at, and the row (in the rail or in the attention list). A third was drawn in a tab menu and deleted.

**RULED (tabs).** A tab keeps its **aggregate state dot** and, horizontally, its pending-count badge: at rest the horizontal layout collapses the rail, and that dot is the only entrance. **The unread dot is display-only** (§5). The tab's second line for a question is **removed** — the rail does the same job and does it better, since it also says which tab and sorts by urgency.

**OPEN (visual):** the whole pane header at agent density — six things in 30 px. The owner named it, with the vertical window header, as one of the two surfaces currently under-used. The drop thresholds above are measurements of a mock; the real ones are measured on the real header.

### 4.7 Accounts on screen

**RULED: an account label appears only when that vendor actually has more than one account on this machine.** One gate (`vendorAccounts(vendor) > 1`), six call sites: the card's pill, the pane header, the row's tooltip, the pane menu's session strip, the tab menu and the tab card's session block. With one account, the word "account" does not occur anywhere in the window. **A constant is noise.**

**RULED: multi-account is not a Folio feature.** A second configuration directory works, but history does not follow it. So: no switcher, no account picker, no "reopen with another account". What survives is **bookkeeping, not interface**: Folio must know **which configuration directory each session runs under**, read from that pane's environment, or a quota number cannot be attributed to the right account — one vendor with two accounts is two ledgers. The only visible consequence is the quota itself.

### 4.8 The Recent view

**RULED.** Not a new panel: a **third view of the files column**, beside Tree and Git, per tab. (`FilesView` is `{ Files, Git }` today, and its own comment already anticipates a third page — the switch's measuring and painting walk one list precisely so that the day a third page arrives is not the day the reserved width and the drawn word become two lists.)

- Sources: paths recognised in terminal output; file-write events from agent hooks; the user's own opens.
- Each row: **who** touched it, **what** happened (created / edited / mentioned), an **unread dot**.
- Click opens the preview.
- Optional: **the preview follows the agent's latest file.**
- **Needs no protocol, so it can come first** (§8). The owner also said that opening a preview is laborious today and must be fixed in 0.5; this view is one of the two fixes.

**OPEN (visual):** the view switch now that it has three pages; the row (three facts and a dot in a 220 px column); how "who" is shown — a mark, a name, or nothing when it was the user.

### 4.9 Marks

**RULED: every agent wears its vendor's OFFICIAL mark**, Claude included — not a hand-drawn approximation. The shipped `#p-claude`, the master's own eight-ray hand-drawn burst, **is replaced when this lands**; do not fix only the new surfaces and leave the old one.

**RULED: monochrome marks follow the theme's text colour; colour marks are never recoloured**, because recolouring is what brand guidelines most commonly forbid. The master's rule "a mark carries its own colour and does not flip with the theme" was written for colour marks and needed the refinement: **a monochrome brand has no colour to carry**, so it must follow `currentColor` or it disappears on one of the two themes. (Measured: a near-black `#171717` official OpenAI file is nearly invisible on Folio Dark; Kimi's official `#fff` K vanishes entirely on Folio Light.) **The escape hatch for a colour mark that fails contrast is the vendor's OWN monochrome version** — never an adjusted colour.

**RULED: status vocabulary lives outside the vendor path.** Breathing is on the wrapper, the dot is overlaid at the corner; no vendor's geometry is animated, so swapping any vendor's asset costs nothing in motion.

**Third-party trademarks, used only to identify their products.** `TRADEMARK.md` exists; **each vendor's brand guidelines must be read before shipping** (minimum size, clear space, colour, whether monochrome or any modification is permitted). One modification is already on the books and needs explicit confirmation: Kimi's K recoloured to `currentColor` while its blue dot keeps the vendor's blue.

**OPEN (visual), the carried checklist:**
- Contrast against `--panel` (dark `#252525`, light `#F7F7F5`), 3:1 floor for non-text: **Claude's `#D97757` measures 2.91 on Folio Light** — below the line. Unresolved (§10 Q7).
- **Hermes' official mark is 39 × 78** and cannot enter a 15 px square slot; the mock used a geometric substitute. The general question — how a tall mark enters a square slot — is unanswered.
- **pi's three colour blocks** are three small squares at 15 px and are essentially unidentifiable in monochrome.
- The owner on the existing pane-menu icons: *"有点丑而且看不懂是什么功能"* — ugly, and you cannot tell what they do; he named zoom pane, split-and-run, duplicate pane, move-pane-to-new-tab/new-window/to-window (the last three are a small square with an arrow and are nearly indistinguishable). What he wants is *"简洁干净,而且描述清晰"* — simple and clean, and clearly descriptive.
- Whether a mixed set (official for most, a redrawn one for the two or three where redrawing genuinely wins) is acceptable, or whether one source must be used throughout for consistency.

## 5. House rules this theme establishes

1. **Click = icon button. Hover-to-unfold = rounded chip.** Two intentions, two faces; a thing that unfolds may not look like a thing that opens a dialog.
2. **Tabs, rows and panes are PLACES, not controls.** Clicking one goes there; hovering one gives a glance card. Rule 1 does not govern them.
3. **Two constants, never mixed.** `CARD_OPEN_MS = 350` for glance cards — the master calls it *the tab-peek constant* and every card in the window uses it (tab card, agent card, attention list, quota panel). `MENU_OPEN_MS = 250` for `⌄` menus (the 2026-08-20 ruling stands). The difference is not speed, it is intent: **a menu opens under a pointer that is aiming at a control; a card opens under a pointer that has stopped.**
4. **The unread dot is display-only.** A 7 px circle is too small to be a target, and it no longer needs to be one: the tab's `⌄` carries the menu and stopping on the tab gives the card.
5. **A section header is collapsed by a chevron BUTTON at its head**, never by clicking the header text. A row of plain text with no clickable face must not be a switch. (The side rail's tab grouping already does this.)
6. **A constant is noise.** A field that cannot vary in this installation is not drawn — the account label when there is one account; the percentage on the quota chip, whose colour is the part that changes what you do next.
7. **A badge is a badge.** Its number is a count of items; it does not get a second algorithm.
8. **Absent is absent** (§7). No empty slot, no `—`, no zero standing in for "unknown".
9. **One list, one place.** A second surface answering the same question is deleted, not synchronised.
10. **Two cards are never on screen at once**, and both are placed by the same function.

## 6. The protocol: `folio` CLI + MCP

This is the extensibility mechanism and the sole public surface. It is designed from day one as a **public protocol**: a version number, capability discovery, structured errors, stable verb names — so that if in-process extension points are ever built they call the same verbs over a different transport. *One interface, two transports.*

**RULED — scope, and what the words must not imply.**

- **This is the scope of FOLIO'S TOOL SURFACE. It is not a sandbox.** The agent's shell can already touch the whole machine. **No string in this product may imply containment.** The tiers say what the agent may do *through Folio*.
- **Identity is a per-pane token placed in that pane's environment.** Scope follows the pane to whatever tab it is in.
- **Three tiers**, plus one that stands apart:
  1. **Read its own tab** (default).
  2. **Act in its own tab** — open a preview, split, open a page.
  3. **The whole of Folio** — explicit, visible, granted per session, and expiring with it.
  - **Typing into another terminal pane is its own, higher tier, even inside the tab.** An agent typing at an agent is an injection chain and is never covered by tiers 1–3.
- **Every action taken through the tool surface leaves a visible record** — who, what, when, and in which pane, readable after the fact. This is also what makes pre-authorised rules (§2.4) safe: Folio answering for the user leaves a trace that can be reviewed and undone.

**RULED — order and shape.** Read-only verbs first (list panes, read a pane's state, read output, read the session ledger); acting verbs after (open a file at a line, open a page, split, answer a permission prompt); typing into another pane last and separately. States on the wire are §3.1's A2A values with the two namespaced extensions.

**RULED — deliberately excluded**: dynamic loading; in-process plugins; a plugin directory; any promise about internal APIs. Folio's internal **registries stay internal** — preview renderers (file type → renderer), link and path recognition/opening, the agent adapter table (per-vendor hook formats, quota, session resume), commands with stable ids, themes. They are seams so that new features register instead of adding a `match` arm; they are not a public surface. The only setting a user must ever turn on is *allow external programs to control Folio* — which this theme needs anyway.

**Budget.** With no agent sessions and no panel open, this subsystem is **quiescent**: zero polls, zero timers, zero work per frame.

## 7. Data sources, and their honesty

**RULED: what is absent is absent.** A field with no evidence is not drawn — never an empty slot, never a `—`, never a zero. A stale value is shown greyed with its age, or not at all; never a guess.

**RULED: Folio never touches an OAuth token, a session token, an API key or a credentials file.** Every quota reading comes from the vendor's own binary, over stdio or loopback, speaking under its own identity exactly as it does when the user runs it. Nothing leaves the machine because of the quota panel. Folio stores `{percent, resets_at, fetched_at}` per account-window — never a token, never an e-mail, never transcript content.

Per vendor, as measured (2026-09-19; **V** = verified on the machine, **D** = vendor-documented, **I** = inferred):

| | State / activity | Todo / sub-agents | Model | Context | Quota | Cost |
|---|---|---|---|---|---|---|
| **Claude Code** | rich hooks **V** | from hooks **V** | statusLine **D/V** | `context_window.used_percentage` **D/V** | `rate_limits.five_hour` / `.seven_day` / `.spend_limit`, each `{used_percentage, resets_at}`, **pushed, free** **D/V** | `cost.total_cost_usd` **D/V** |
| **Codex** | **turn end only** **V** | — | app-server **V** | — | `account/rateLimits/read`, `usedPercent` + `windowDurationMins` + `resetsAt`, plus a pushed sparse update **V** | — |
| **Kimi CLI** | — | — | — | — | local server child, `usedRatio` 0–1 per window, plan-dependent window set **V** | — |
| **Copilot CLI** | stop hook, no quotable words **D** | — | — | — | — | — |
| **GLM / Z.ai** | — | — | — | — | **nothing officially** **D**; the route third-party tools use is undocumented and cookie-oriented — **must not be used** | — |
| **DeepSeek** | — | — | — | — | balance only, and only with a key the user explicitly pastes for that purpose — opt-in, off by default **D** | — |
| **OpenCode / Hermes / pi** | — | — | — | — | — | — |

Rules that follow, each a refusal:

- **Read a window's KIND from its duration, never from its key name.** `primary` / `secondary` are positional.
- **Absent ≠ 0 %.** No Anthropic number when `rate_limits` is missing (API-key accounts, before the first response of a session); no Codex number for an `apiKey` account; no GLM percentage from an undocumented route; no figure derived by counting tokens in a transcript, by extrapolating between updates, or by carrying a value past its `resets_at`; never one merged number across two accounts of one vendor.
- **Cadence / budget.** Anthropic: pushed and free — **never poll**. Codex: on panel open and ~5 min, plus the pushed update. Kimi: a server child, so panel-open and at most every ~10 min. Zero when there is no session and no panel.
- **The statusLine conflict (Anthropic only).** The user may already have a statusLine. Folio **must not overwrite it**: offer either (a) leave it alone and show nothing, or (b) wrap it — Folio's shim keeps `rate_limits`, forwards stdin verbatim to the previous command and prints its output unchanged. Anything else silently breaks the user's own HUD.
- **Quota is measured in USED, not remaining**, everywhere, so that a longer bar is always worse. Ruled by consistent use through twelve rounds; recorded here so it is not re-opened per surface.

## 8. Sequence

The owner's order, with the reason each position is what it is.

| # | Ticket | Why here |
|---|---|---|
| 0 | **Foundations** — the session ledger as the single owner of state (§3.2); the row component; reuse of the `.file-peek` chrome and `placeBesideRow`; the motion constants 350 / 250; the marks pipeline and `TRADEMARK.md` review | Nothing visual may be born before these exist, or the second surface re-implements the first |
| 1 | **Protocol + identity** — the `folio` verb set, versioning, the per-pane token, the three tiers + the typing tier, the visible record | Everything that acts depends on it, and its shape is hardest to change later |
| 2 | **Read-only tools** | Proves the protocol against a real consumer at the lowest risk |
| 3 | **The Recent view** — *may come earlier*, and probably should | Needs no protocol at all, and answers the owner's "opening a preview is laborious" complaint on its own |
| 4 | **The Agent rail, the row, the glance card, the badge and its list** | The visible core; depends on 0 and on the ledger, not on the protocol |
| 5 | **Quota: chip, panel, toasts** | Independent of 4 but shares the title-bar geometry, so it follows it |
| 6 | **Comment-to-agent** — select an element in a preview or a page, send it to an agent's input | Needs 1 and 2; the Markdown path is cheapest (block → source-line mapping already exists, so the comment carries `file:line`); the page path needs the picker; PDF is page + rect + text. **Delivery is a bracketed paste into the agent's input and is NEVER auto-submitted**; several comments may be batched; page content is untrusted and arrives quoted and truncated |
| 7 | **Watching an agent drive a page** — last | The most expensive and the least reversible. Its rules are already ruled: the agent has its own visible cursor; the user touching input takes over; **closing the pane means BACKGROUND, not end** — the row must then read `driving a web page · <domain>`, be clickable back, and have Stop; never invisible. **A separate browser profile by default**, with no user cookies; sharing the user's session is an explicit grant. Cost note: WebView2 has CDP, WKWebView does not, so the Mac side is a second implementation |

## 9. Earlier rulings this theme touches

The owner's principle (2026-09-20): *rulings are not law; cite the date and the premise, and judge whether the premise still holds.* Conflicts are flagged, not silently resolved.

| Ruling | Date | Status here |
|---|---|---|
| The `⌄` menu opens on hover 250 ms and on click | 2026-08-20 | **Stands**, and is now one of a pair: 250 for menus, 350 for cards (§5.3) |
| The pane `⌄` menu has separate rows for *Move to new tab* / *Move to new window* / *Move to window ▸* | 2026-08-20 | **Partly re-ruled.** The owner ruled the two window rows merge into one `Move to window ▸` (new window, then existing windows). Merging the tab rows in as well was *proposed, not ruled* — §10 Q9 |
| Spring-loaded 250 ms dwell drags a pane into a tab | 2026-08-21 | **Stands**, and the same dwell governs tab-into-tab. Two caveats travel with it: it was ruled "ordinary mode only, not the cards column", and whether the gesture works on the **vertical** tab list is **unverified** |
| `.file-peek` may be entered by the pointer | 2026-08-14 | **Stands**, and is the reason the agent card is that family (§4.3) |
| `.attn-chip`: *quiet chrome, loud dot* | 2026-07-18 | **Stands**, and governs the new badge and chip (§4.4) |
| The pane header is 30 px | 2026-08-12 | **Stands** (§4.6) |
| The attention block owns tab status icons and notifications | 2026-08-25 | **Stands, re-sourced**: they and the rail are projections of one ledger (§3.2) |
| Web preview: one preview per tab, no tabs inside it, heavy browsing returns to the system browser | 2026-08-19 | **Needs re-ruling.** Premise changed 2026-09-18: the browser becomes its own pane class, closer to a real browser, and agent-drivable. Not resolved here — §10 Q10 |
| No tab groups for now; "a group is a project" deferred | 2026-09-20 | **Still deferred**, and it is the same question as "does a tab have a root folder". Nothing in this note depends on it |
| The files column and the preview already support several per tab | 2026-09-20 | **Stands**; the Recent view is a third *view*, not a fourth panel |
| Minimum sizes are law to the program and advice to the user | — | **Stands**; the rail's 280 px and the drop orders are inputs to the solver, not exceptions to it |
| Extensibility: no VS Code compatibility, no in-process plugins, internal registries stay internal | 2026-09-20 | **Stands**; §6 is its first delivery |

## 10. Open questions for the owner

1. **Is the Agent rail this window's, or the whole application's?** Per-window means the title must say so and carry a dim "N more in other windows"; application-wide means clicking a row raises another window.
2. **With no agent sessions at all, does the rail disappear entirely, or keep one header line?** It was argued both ways while quota lived in the rail head; quota has since moved to the title bar, which removes that argument.
3. **Is a row keyed by PANE or by SESSION?** Pane (recommended): one pane one row, a restart replaces the contents, dead sessions live in `Resume ▸`. Session: a restart adds a row and the list grows without bound.
4. **A sub-agent that blocks on its own question**: does the flat list grow a second level (the same aggregation rule the tab dot uses over panes), or does a blocked sub-agent surface as the parent row waiting?
5. **Does the rail freeze its order while the pointer is inside it**, or is creation order (§4.1) enough on its own?
6. **For the session you are looking at right now, should the glance card drop "files changed" and "last reply"?** They duplicate the terminal in front of you; the card earns its keep for other tabs.
7. **Claude's `#D97757` measures 2.91:1 on Folio Light, under the 3:1 floor.** (a) accept it; (b) use the vendor's own monochrome version for that one mark on that one theme; (c) use monochrome marks everywhere and give up colour as an identification channel.
8. **Should the state word or the activity yield first in English?** The state words were shortened to `Waiting` / `Done` / `Limited · 19:00` / `Exited`, which brought English to the same number of truncations as Chinese (2 vs 2), 12 px apart. Closing the last 12 px means changing wording again.
9. **The brief records "`Move to ▸` merges tabs and windows" as standing.** What the owner ruled was that the two **window** rows merge; folding the tab rows into the same submenu was my proposal and is unruled. Which is it? (My position: keep *Move to new tab* separate — splitting into a tab is frequent and should not gain a level.)
10. **The browser pane's multiple pages.** Recommended: visible tab strip, and *opening a link never replaces the page you are using* (loading, pinned, agent-occupied, or holding an unsubmitted form) — it opens a new page in the same pane; the preview keeps its dropdown switcher because documents are cheap to reopen and nearly stateless.
11. **Select-and-comment and web driving are ruled in structure but were never drawn.** Are they in scope for a first 0.5, or does 0.5 ship the rail, Recent, the protocol and quota, with comment and driving following in 0.5.x?
12. **Does the Recent view's row show "who" with the agent's mark?** That puts vendor marks into the files column, which has never carried one.
13. **One divergence between the brief and the mock's record, flagged rather than resolved.** The brief proposed a house rule of the form *"in this window a number appears only when it changes what you do next"*. NOTES supports two narrower rules and not that generalisation: **a constant is noise** (the account label, §5.6) and **a badge is a badge** (§5.7); the quota chip lost its number for a stated, specific reason, not under a general law. §5 carries the narrow pair. Adopt them as they stand, or rule the general form and let the narrow ones follow from it?
