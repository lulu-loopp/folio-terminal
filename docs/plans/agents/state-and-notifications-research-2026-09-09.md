# Agent state, notifications per source, and an identity for a pane — research for 0.3

2026-09-09. Branch `docs/agent-state-research` off `main` at `5973d81`. This is
research, not a design: no product code is touched and nothing here is decided.
Three things are on the table for 0.3 — **(A) agent state v1**, **(B)
notification management per source**, and **(C) an identity for a long-lived
pane** — and each gets findings and a proposal. Every external claim carries a
source; the list is §8.

---

## 0. What the window already has

The second pillar shipped in 0.1.0 and has been extended four times since. The
parts the new work stands on:

* **An episode ledger, not a bell.** `attention.rs` mints a strictly increasing
  generation per producer and records an answer as a watermark, so `generation >
  watermark` is an unanswered request and a withdrawal never winds a watermark
  back. Four wait kinds are closed: `Permission`, `Elicitation`, `Agent`, `Quota`.
* **A mapping catalogue, as data.** `attention_map.rs` holds 18 rows for Claude
  Code, 3 for Codex, 5 for Copilot CLI and a turn-end row for pi. A row declares
  family, upstream event name, kind, identifier source, and either a wait tier or
  a clear class. Adding a family is adding rows.
* **A capability wire.** `attention_wire.rs`: `folio attention <family>:<event>
  --json <payload>` over a per-pane named pipe addressed by 128 unguessable bits.
  **The payload never crosses** — only the identifier a row names and at most 80
  characters of the sentence a turn ended on, quoted by `attention_words.rs` from
  the tail of a Claude Code transcript, skipping `isSidechain` entries.
* **A four-rung reach.** `notify::desktop_reach(tab_is_active, place)` answers
  `Nothing` / `Marks` / `Flash` / `Toast`, and `Interruption` has three arms
  because a delivery can only ask three things of the window.
* **Two switches, routed by who spoke.** `NotificationSwitches::admits(via)`
  sends `Osc9 | Osc777 | Osc99` to Settings ▸ Terminal ▸ `Notifications` and every
  turn-end transport to Settings ▸ Agents ▸ `Turn finished`.
* **A ruled seven-state taxonomy that is only partly built.** `DESIGN.md`
  §7.1.5b: busy (breathing) | progress (ring, OSC 9;4's four phases) |
  unread·done | unread·failed | awaiting | bell | dead, with **one dot one
  assertion** and a tab taking its members' highest severity. What runs is
  `StatusClaim` — `Silent`, `Unread`, `Bell`, `Failed`, `Awaiting`. **Busy and the
  tool being run have no producer at all.** An eighth mark, `Attached`, was
  proposed in the attention plan §3.5 and never ruled.

Feature (A) is therefore not a new taxonomy. It is the missing producer for two
rungs of one that was ruled on 2026-07-18, plus two things the taxonomy has no
vocabulary for: **which tool**, and **the sub-agents**.

---

## 1. What each agent can tell us

### Claude Code

The documented core is `SessionStart`, `SessionEnd`, `UserPromptSubmit`,
`PreToolUse`, `PostToolUse`, `Notification`, `Stop`, `SubagentStop`, `PreCompact`;
the live reference also lists `PermissionRequest`, `SubagentStart`, `StopFailure`,
`Elicitation`/`ElicitationResult`, `PostToolUseFailure`,
`TaskCreated`/`TaskCompleted` and more [S1] — six of which are already rows in
`attention_map.rs`, corroborating the same page read in August.

Every event carries `session_id`, `transcript_path`, `cwd`, `hook_event_name`,
`permission_mode`, and — on sub-agent events — `agent_id` and `agent_type`.
`PreToolUse`/`PostToolUse` carry `tool_name`, `tool_input`, `tool_use_id`;
`PostToolUse` adds `tool_response`. **`Stop` carries `last_assistant_message` as
a field** [S1], which the transcript reader predates and could retire.
`Notification` fires with a `notification_type`: `permission_prompt` at about six
seconds, `idle_prompt` about sixty seconds after Claude stops with no typing
since [S1][S2]. A second channel this project has not used is `statusLine` and
**`subagentStatusLine`, which receives a `tasks[]` array of `{id, name, type,
status, contextWindowSize, tokenCount}`, polled per refresh tick** [S3] — a
sub-agent list delivered without tailing anything, but a poll, so a good source
for a list and a poor one for an edge.

Claude Code emits **OSC 8** and **OSC 9;4** progress but **not OSC 133** — three
requests for it were closed unplanned [S4]. Its notification channel is
`preferredNotifChannel`: `auto`, `terminal_bell`, `iterm2`, `iterm2_with_bell`,
`kitty`, `ghostty`, `notifications_disabled` [S5], where `auto` reaches the
desktop only in iTerm2, Ghostty and kitty. **Folio is not in that table.**
Title-setting over OSC 0 is reported in issues and not documented; treat it as
unconfirmed [S6]. A hook may return `hookSpecificOutput.terminalSequence`, an
allowlisted escape string [S1] — an escape hatch, not a protocol.

### Codex CLI

`notify` is unchanged since the August survey: an argv array, one JSON argument
appended, a single event type `agent-turn-complete` carrying `thread-id`,
`turn-id`, `cwd`, `input-messages`, `last-assistant-message` [S7]; a request for a
turn-*start* event is still open, which is the proof no second type ships [S8].
The hooks engine is now documented: `SessionStart`, `SessionEnd`, `PreToolUse`,
`PostToolUse`, `PermissionRequest`, `PreCompact`, `PostCompact`,
`UserPromptSubmit`, **`SubagentStart`, `SubagentStop`**, `Stop`, `Interrupt`, in
`~/.codex/hooks.json` or `[hooks]` in `config.toml`, carrying `session_id`, `cwd`,
`model`, `transcript_path`, `permission_mode` and, on sub-agent events,
`agent_transcript_path` [S9]. Codex has a first-class sub-agent concept [S10].
`tui.notification_method` (`auto|osc9|bel`) picks OSC 9 for Ghostty, iTerm2,
kitty, Warp and WezTerm — Warp added by an explicit pull request [S11][S12].
Folio is not on that list, which is why panes get a bare bell. The rollout JSONL
under `~/.codex/sessions/` is documented as a location [S13] but its schema is
not; the only account is a community reverse-engineering that found the source
comments and the real output disagreeing [S14]. Not a source to build on.

### GitHub Copilot CLI

Hooks are official: `sessionStart`, `sessionEnd`, `userPromptSubmitted`,
`userPromptTransformed`, `preToolUse`, `postToolUse`, `postToolUseFailure`,
`agentStop`, **`subagentStart`, `subagentStop`**, `errorOccurred`, `preCompact`,
`permissionRequest`, `notification` [S15] — five are already rows here. There is
also an ACP server (`copilot --acp`, JSON-RPC over NDJSON) streaming tool
execution and permission requests [S16], and a session log at
`~/.copilot/session-state/<id>/events.jsonl` that is explicitly unstable [S17].

### The rest

OpenCode serves SSE at `GET /event` carrying `session.idle`, `session.status` and
`message.part.updated`; `session.status` omits `parentID`, so sub-agent
attribution is weak [S18]. Aider has only `--notifications-command`, fired when it
is done and waiting [S19]. Gemini CLI gained hooks in 0.26 (`BeforeTool`,
`AfterTool`, `BeforeAgent`, `AfterAgent`, `SessionStart`, `Notification`) [S20]
and has experimental OSC 9 notifications [S21]. Cursor's `cursor-agent` has hooks
including `subagentStart`/`subagentStop` with `parent_conversation_id` plus
`--output-format stream-json` [S22], and no idle or waiting-for-user hook.
Amazon Q's context hooks only inject context in [S23].

### Derivability

| | idle | working (which tool) | waiting | done | sub-agents | last message |
|---|---|---|---|---|---|---|
| Claude Code | `Notification.idle_prompt`, ~60 s | `PreToolUse.tool_name` → `PostToolUse` | `PermissionRequest` / `Elicitation`, 0 s | `Stop` | `SubagentStart`/`Stop`, or `subagentStatusLine.tasks[]` | `Stop.last_assistant_message`, or transcript tail |
| Codex | — (no idle event) | `PreToolUse`/`PostToolUse` | `PermissionRequest` | `Stop`, `notify` | `SubagentStart`/`Stop` | `notify.last-assistant-message` |
| Copilot CLI | `notification.agent_idle` (background agents) | `preToolUse`/`postToolUse` | `notification.permission_prompt` / `elicitation_dialog` | `agentStop` | `subagentStart`/`Stop` | — (transcript format unquoted) |
| OpenCode | `session.idle` | `tool.execute.before/after` | `permission.evaluate` | `session.idle` | weak (no `parentID`) | `message.part.updated` |
| Gemini CLI | — | `BeforeTool`/`AfterTool` | `Notification` | `AfterAgent` | — | — |
| Cursor agent | — | `preToolUse`/`postToolUse` | — | `stop` | `subagentStart`/`Stop` | `stream-json` |
| Aider | — | — | — | `notifications_command` | — | — |

Three ways of hearing, by how far they can be trusted: **a hook the user
installed** (an edge, exact, and the only one that reports a tool); **a file the
agent writes** (a poll, and only Claude Code's transcript has a format quoted
here); **the bytes in the pane** (universal, coarse). A fourth — **sniffing the
title**, Codex's `Action Required`, Gemini's `✋` — is a reconfigurable string with
no version, filed at the lowest credibility by the attention plan §3.4, and it
stays refused.

---

## 2. How others show it

**Warp** is the most complete. An **Agent Management Panel** at top right lists
every agent across sessions; a separate bell opens a Notification Mailbox with
All / Unread / Errors filters. Per-pane state is a small circular badge on the
vertical tab row — magenta clock in progress, green check done, red triangle
error, grey stop cancelled, yellow stop blocked — plus an accent unread dot that
clears on focus, kept deliberately separate from state. Notifications are three
categories (Complete, Request, Error) delivered as toast (non-focused tab only, at
most two visible), mailbox entry, tab badge and OS notification; for a group of
agents only the parent notifies [S24][S25][S26].

**VS Code** puts sessions in a sidebar (since 1.107 behind
`chat.agentSessionsViewLocation`) and in an Agents window of its own; the
title-bar indicator, gated by `chat.agentsControl.enabled`, shows an **unread
sessions badge** and an **in-progress sessions badge**, each a filter when
clicked, and there is no documented colour vocabulary for state and no per-session
notification setting [S27][S28]. Its notification centre is the status-bar bell,
with Do Not Disturb (`notifications.toggleDoNotDisturbMode`, and `…BySource` per
extension) hiding everything but errors and **keeping everything in the list**
[S29].

**Zed** has an Agent Panel plus a Threads Sidebar grouped by project, each row
with "a status indicator" whose icons are not documented; its settings are the
crispest of the five — `agent.notify_when_agent_waiting` = `primary_screen |
all_screens | never` and `agent.play_sound_when_agent_done` = `never | when_hidden
| always`, both firing on the same two triggers [S30][S31]. **Cursor** has Draft /
Running / Needs attention / Done, groupable by status, with no colour coding and
no first-party desktop notification [S32][S33]. **JetBrains** shows "Thinking…" /
"Planning…" in the tool window, and its only crisp three-state list — "Working…"
/ "Awaiting input" / "Ready" — is in the Junie CLI's `/history` [S34][S35].

**Terminals.** Ghostty parses OSC 9, 777 and 99 into one notification, its
`bell-features` are `system | audio | attention | title | border`, it drops
`RequestAttention` as unimplemented, and its maintainers rejected a
running/waiting/error indicator as infeasible [S36][S37]. WezTerm's
`notification_handling` = `AlwaysShow | NeverShow | SuppressFromFocusedPane |
SuppressFromFocusedTab | SuppressFromFocusedWindow` is the same focus gate this
window computes [S38]. kitty separates three things cleanly — bell (`bell_on_tab`,
default 🔔), activity (`tab_activity_symbol`) and command-finished
(`notify_on_cmd_finish` = `never | unfocused | invisible | always`, with a
duration threshold) [S39][S40]. iTerm2 has a per-profile **Filter Alerts** panel
toggling output, idle, bell, session-close and escape-code triggers
independently, plus "Suppress alerts from active session" [S41]. tmux is the most
granular: `monitor-bell` / `monitor-activity` / `monitor-silence`, each with its
own action, visual choice, style and status flag — `!`, `#`, `~` [S42]. Windows
Terminal's `bellStyle` is `all | audible | window | taskbar | none` and gained
`"notification"` — a real toast, throttled to five seconds and suppressed when the
pane is focused [S43][S44].

**The tmux plugins are the closest prior art to feature (A)**, and they agree:
every one that genuinely distinguishes working from waiting from done reads Claude
Code's **hooks** into a small per-session status file, not the statusline
contract. `tmux-claude` has eight states with emoji; `tmux-agent-status` draws a
sidebar tree with per-agent glyphs coloured yellow working / cyan waiting /
magenta ask / green done; `tmux-agent-indicator` colours the pane border. The one
exception polls `claude agents --json` [S45][S46][S47][S48].

---

## 3. Protocols and conventions

`OSC 9;<text>` is iTerm2's one-field notification [S49].
`OSC 9;4;<state>;<progress>` is ConEmu's progress — 0 clear, 1 normal, 2 error, 3
indeterminate, 4 paused — documented by Microsoft as a tab ring and a taskbar bar
[S50]; Ghostty implements only sub-command 4 of the twelve and times the state out
after 15 s [S51]. `OSC 777;notify;<title>;<body>` is urxvt's, split at the first
`;` only. `OSC 99` is kitty's, with `i` identifier, `d` done, `p` payload type,
`a` action, `u` urgency, `e` encoding, `w` auto-close, `c` close-notify, `o`
when-to-show [S40]. `OSC 133` A/B/C/D-with-exit-code is implemented by Windows
Terminal, WezTerm, kitty, iTerm2, Ghostty and VS Code [S52][S53] — and by no agent
CLI. `OSC 1337` carries `RequestAttention=yes|no|once|fireworks` and, separately,
`SetUserVar=<key>=<base64>`, an existing "the program tells the terminal a
variable" channel that iTerm2 status-bar components render [S49][S54].

**There is no agent-state protocol.** The freedesktop terminal-wg has an open
notifications track and has finalised nothing; Ghostty's stated position is to
support OSC 9, 99 and 777 all three rather than wait [S55]. The one real
precedent is proprietary: **Warp layers structured agent-session state onto OSC
777 as a `warp://cli-agent` payload**, wired for Claude Code and Gemini CLI, with
an open proposal to widen it [S56]. The August survey's finding stands unchanged:
no agent CLI emits `OSC 1337;RequestAttention`, and none emits OSC 133.

**Recommendation: Folio does not define a state OSC for 0.3.** Defining one buys
nothing while there is no producer, and the hook lane already carries more than an
escape sequence could. Worth doing instead: one issue per upstream asking to be
added to a table that already exists — Codex's `notification_method=auto` list and
Claude Code's `preferredNotifChannel`. If a sequence is ruled anyway, it should
**not** be a new OSC number but one key in the space already parsed:
`OSC 1337;SetUserVar=agent=<base64 of {"s":"working","t":"Bash","n":2}>` — `s`
one of `idle|working|waiting|done`, `t` an optional tool name, `n` an optional
sub-agent count. iTerm2 already implements the carrier, every other terminal
ignores it silently, and it costs one row in `attention_map.rs`.

---

## 4. Notification management patterns

**Android** is the deepest: app → optional channel group → channel → a five-level
importance (`NONE`, `MIN`, `LOW`, `DEFAULT`, `HIGH`), set once by the app and
thereafter changeable only by the user. Importance gates *interruption*, never
*persistence* — every level still lands in the drawer [S57]. **iOS** is two-level:
per-app allow plus three independent placements, then a per-Focus allow/silence
list, with Time Sensitive as the one override that breaks through [S58][S59].
**Slack** is the shape a terminal should copy: a global default (Everything /
Mentions), a per-channel override where it differs, mute as a separate boolean,
and a schedule that is a time window rather than a level [S60][S61]. **macOS**
makes Do Not Disturb one Focus among several with a duration picker, and
Notification Center keeps everything suppressed during it [S62][S63]. **Windows**
Do Not Disturb suppresses toasts but still logs them, with a user-curated Priority
list as the exception; an app can *read* `ToastNotificationMode` (`Unrestricted` /
`PriorityOnly` / `AlarmsOnly`) but not set it, and no OS flag means "toast only if
hidden" — that stays the app's own job [S64][S65], which is what `desktop_reach`
does.

Three rules are unanimous: **two tiers, not five**; **defer, never drop** (all
five keep a retrievable history through DND); and **a timer on DND**. Two are
worth skipping: Android's channel-per-notification-type, which only pays when an
app has many kinds — a pane has one — and iOS's summarisation and reordering.

---

## 5. An identity for a long-lived pane

### 5.1 What exists today, and what it is not

`profiles.json` holds `id`, `display_title`, `hidden`, `program`, `args`, `env`,
`starting_dir`. That is a **template**: it says how to start something, and every
start is a new copy. It carries no icon, no colour, no hotkey and no notification
level, and nothing links two panes started from it. `TabV1.pinned` is a hint
about tab-strip order, not an identity. The quake terminal (§7.54) is the one
thing with singleton semantics — one companion window, one hotkey, summoning
toggles it — and it is bound to **a window**, not a pane, and there is exactly one
of them. Cards is a grid of a window's tabs; the palette (§7.55) has a fixed
`actions → tabs and panes → commands → files → settings` order whose
tabs-and-panes rows already jump by `activate_tab` + `focus_seat`, and it excludes
tabs in other windows **because a window has no name to say which one**.

The gap is precise: there is no **named, long-lived instance**. Nothing survives a
restart under a name a person chose, nothing can be summoned by that name, and
there is nowhere for a per-pane level or a notification history to hang that
outlives the pane.

### 5.2 How others model it

| | the named thing | persists as | summoning by name |
|---|---|---|---|
| Windows Terminal | profile (`guid`, `name`, `icon`, `tabColor`, `startingDirectory`) [S66] | `settings.json`; one implicit layout autosave via `firstWindowPreference` [S68] | `wt -w <name>` is attach-or-create, but the name dies with the window; `-p` always opens a new one [S67] |
| Warp | launch configuration (YAML), tab config (TOML) [S70][S71] | files in a config dir | always a fresh copy — a maintainer confirms it "is always starting a new Window" [S70] |
| tmux | session name | nothing natively; tmux-resurrect snapshots, tmux-continuum autosaves every 15 min [S73][S74] | `new-session -A -s <name>` = attach or create |
| zellij | session name | built in, `session_serialization` on by default, re-runs each pane's command [S75] | `attach --create <name>` = attach, **resurrect an exited one**, or create |
| VS Code | `.code-workspace` (folders + its own settings) [S79][S80] | one file | reuses the recent window, **no singleton check** — the same workspace twice gives two windows [S81] |
| Zed | the project path | `restore_on_startup` = `last_session / last_workspace / none` [S83] | reuses the current window; no registry |
| iTerm2 | profile; named window arrangements [S84] | preferences | arrangements stamp out fresh windows; the **dedicated hotkey window** toggles the same one [S85] |
| iOS / Android | the app icon | the OS | tapping foregrounds the running app, never a second [S86] |

Four details carry over. **Windows Terminal has never put profiles in the Windows
jump list** — a request open since 2019 whose draft spec never shipped [S69].
Named *templates* stay separate from instances everywhere: Warp's Workflows are
commands [S72], zellij's layouts are KDL files [S76], VS Code's terminal profiles
carry `icon`, `color` and `overrideName` per template and never per instance
[S82]; tmuxinator, sesh and tmux-sessionizer reduce a project name to a session
name and attach-or-create [S77][S78]. On phones the **Home Screen is a curated
subset and the App Library is everything** [S87], quick actions are template verbs
[S88], and **widgets** are a persistent glanceable status surface no terminal
surveyed offers [S89]; Android **pinned shortcuts** are the user promoting one
thing to its own icon [S90].

### 5.3 What a stable identity would add

A **named instance** is a row: `name`, `profile_id`, a starting folder, a colour,
an optional hotkey, a notification level. It survives a restart because it is
authored rather than snapshotted. It is a **singleton**: summoning it switches to
the pane already running it and starts one only when nothing is. It is what the
per-pane level in §6.4 and the notification centre in §6.3 hang off, so that "this
one asked twice while I was out" survives the pane being closed. And it is a row
in the agent panel whether or not anything is running in it — the property that
makes a home screen a home screen.

**Surfaces that need no new metaphor:** **Cards is the home screen** — already a
grid of a window's tabs, and a named instance is a card that outlives its pane,
with a name and a start verb. **The palette already has the section**: `tabs and
panes` gains rows for instances that are not running, run by the same summon verb,
and its own note that cross-window rows are excluded "because a window has no name
yet" is answered by the same field. **The Windows jump list** is one shell API,
registered lazily beside the AUMID the toast path already writes (§7.6) — also the
surface Windows Terminal has been asked for since 2019 and never shipped [S69],
and the only one here visible when Folio is not. And **the notification centre
groups by instance**, which is what makes a history readable: five entries from
one named pane are one group.

**Out of scope**: a window per instance (an instance is a pane, and Cards already
answers "show me all of them"); a custom icon library or user-supplied image files
(a colour and the profile's own glyph, per §7.4's rule about not shipping image
files); an application model inside the terminal — instances start programs, they
do not sandbox, install, update or own them.

---

## 6. A proposal for the owner to rule on

### 6.1 The state model

```
AgentState = Idle | Working { tool: Option<Name> } | Waiting { kind: WaitKind }
           | Done { ok: bool } | Gone
SubAgent   = { id, name, state: AgentState, since }
```

Each variant is already a rung of §7.1.5b: `Working` is **busy**, `Waiting` is
**awaiting**, `Done { ok }` is **unread·done** / **unread·failed**, `Gone` is
**dead**; `WaitKind` is the ledger's existing four. Nothing new is drawn — agent
state v1 is the missing *producer* for busy and the missing *vocabulary* for which
tool, and the tool name lives in the tooltip and the panel, never as a mark.
`Waiting` is **read from the ledger, not stored twice**.

Sub-agents are **one level deep**: Claude Code, Codex, Copilot and Cursor all
expose exactly `SubagentStart`/`SubagentStop` and nothing upstream reports a tree.
A "tree" in the panel is a parent row with children indented under it.

### 6.2 Sources, ranked

1. **The hook already installed** (Claude Code, Codex, Copilot). A third lane in
   `attention_map.rs` beside wait and turn-end: rows whose action is `State { … }`.
   `PreToolUse` → `Working{tool_name}`, `PostToolUse` → `Working{None}`, `Stop` →
   `Done{ok}`, `SubagentStart`/`Stop` → the child list. This needs `tool_name` to
   cross the wire — a **third declared field**, bounded and alphabet-restricted
   like the association key, never a free payload.
2. **`Notification.idle_prompt`** for `Idle` on Claude Code, at its own ~60 s.
3. **The transcript tail**, already implemented, for the last message and for
   sub-agent presence via `isSidechain`. Claude Code only.
4. **OSC 9;4** for the progress ring, which §7.1.5b rules and which pi and Copilot
   CLI emit today.
5. **Nothing.** A pane with no adapter shows a name and no state, as it does now.

Title sniffing is refused at every rank.

### 6.3 Surfaces

* **Pane header** — the ruled `stateIcon`: breath for `Working`, ring for
  progress, one dot at the highest severity, tool name in the tooltip.
* **Tab dot** — unchanged, `Awaiting > Failed > Bell > Unread`.
* **Focus card** — state, tool and elapsed on one line, the last message's lede
  under it. Cards is also the home screen of §5.3.
* **One panel, two sections**, a sibling of the Git panel: **Agents**, a row per
  agent pane across every tab of this window with kind, pane name, state, lede and
  elapsed, sub-agents indented; and **Asked**, the history — who asked what and
  when, grouped by named instance. A click routes exactly as a clicked toast does
  today. **The history is a projection of the ledger, not a second store** — an
  entry clears when the watermark passes its generation, the same fact that clears
  the dot. `Ctrl+Shift+A` keeps its meaning.

### 6.4 Levels, inheritance, DND

Four levels, because they are exactly the reach that already exists:

| level | ceiling |
|---|---|
| Silent | no mark at all |
| Dot | `Reach::Marks` |
| Dot + taskbar | `Reach::Flash` |
| Everything | `Reach::Toast` |

**A level is a clamp on the computed reach, not a second gate** — the door that
decides whether an interruption is owed stays where it is (red line 12).
Inheritance is Slack's: a default per **profile** (`profile_id` is already on the
leaf, and the seven agent profiles are the natural rows on the Agents page), then
a named instance's own level, then a per-pane override, each absent by default.
**Do not disturb** is global, has a shortcut and a timer (30 min / 1 h / 2 h /
until I turn it off), and clamps every pane to `Marks`; nothing is dropped, the
panel keeps what it swallowed. The two existing switches stay as they are —
`Turn finished` and `Notifications` say *whether a class of thing speaks at all*,
a level says *how loudly this pane does*.

### 6.5 Persistence and scope

Per-pane level: an additive optional field on `TermLeafV1` in `session.json`
(absent = inherit), a schema bump and nothing else. Per-profile defaults:
`profiles.json`. Named instances: a file of their own, because they are authored
and must not be lost when a session snapshot is. DND: **not persisted** — it has a
timer, and a terminal that comes back silent without saying so is broken. State is
never persisted; a restored pane has no agent in it yet.

**Out of scope**: sound of any kind; notification categories beyond the four kinds
the ledger has; per-tool rules; a second history store; mobile or cloud push; any
state read by sniffing a title or a screen; and everything named at the end of
§5.3.

---

## 7. Open questions

1. **Does agent state add a mark?** — **No.** Reuse §7.1.5b's seven; the tool name
   goes in the tooltip and the panel. A new mark reopens a taxonomy ruled row by
   row.
2. **One panel or two?** — **One, two sections.** The lists have the same rows and
   the same click; splitting them means answering "which one is it in?" forever.
3. **Sub-agent depth.** — **One level**, because that is all any upstream reports.
   A deeper tree would be invented.
4. **Four levels or three?** — **Four**, because they map one-to-one onto `Reach`,
   and a level that is not a reach is one nobody can predict.
5. **Where does the default live — profile or agent kind?** — **Profile**:
   `profile_id` is already the key on the leaf, and a WSL pane running Claude Code
   is a different row from a Windows one.
6. **Does DND survive a restart?** — **No**, per §6.5.
7. **Does more of the payload cross the wire?** — `tool_name` must, for
   `Working{tool}` to exist. **One new declared field**, bounded and restricted
   like the association key, and nothing else: no free text, no session id, no
   path.
8. **Do we ask upstream to add Folio?** — **Yes**, one issue each to Codex
   (`notification_method` auto list) and Claude Code (`preferredNotifChannel`). No
   code here; Warp was added the same way [S12].
9. **What becomes of the unruled eighth mark, `Attached`?** — **Superseded for
   agent panes** and **left open** for everything else; its `alternate_screen`
   criterion never covered Codex anyway.
10. **Is an instance a pane or a tab?** — **A pane.** An agent lives in a pane and
    the level is per pane; an instance owning a tab would have to say what happens
    when it is split.
11. **Where do instances live?** — **A file of their own.** `session.json` is a
    snapshot this window rewrites; an instance is something a person wrote.
12. **Is a hotkey required to register one?** — **No.** It is one optional field;
    Cards and the palette summon by name without one, and the quake key is already
    spoken for.
13. **Do we build the Windows jump list?** — **Yes**, lazily, on the same terms as
    the toast AUMID: nothing written until a person registers an instance. One API,
    and the only surface here visible when Folio is not.
14. **What happens when a registered instance's process exits?** — **The row stays
    and summoning restarts it.** That is the difference between a home screen and a
    task list, and it is zellij's answer too [S75].
15. **Does an instance's level override the profile's?** — **Yes**, three tiers
    total: one more than Slack's, and the smallest number that lets a named pane be
    quieter than its own template.

---

## 8. Sources

[S1] code.claude.com/docs/en/hooks · [S2] code.claude.com/docs/en/hooks-guide ·
[S3] code.claude.com/docs/en/statusline · [S4]
github.com/anthropics/claude-code/issues/26235, /22528, /32635 · [S5]
code.claude.com/docs/en/terminal-config, enum corroborated by
github.com/anthropics/claude-code/issues/67220 · [S6]
github.com/anthropics/claude-code/issues/47397, /21409, /15082 (unconfirmed) ·
[S7] learn.chatgpt.com/docs/config-file/config-advanced and
`codex-rs/hooks/src/legacy_notify.rs` · [S8] github.com/openai/codex/issues/8455 ·
[S9] learn.chatgpt.com/docs/hooks · [S10]
learn.chatgpt.com/docs/agent-configuration/subagents · [S11]
learn.chatgpt.com/docs/config-file/config-reference · [S12]
github.com/openai/codex/pull/17174 · [S13]
learn.chatgpt.com/docs/developer-commands · [S14] dev.to/milkoor, "Reverse
engineering Codex CLI rollout traces" (community) · [S15]
docs.github.com/en/copilot/reference/hooks-reference · [S16]
docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server · [S17]
github.com/github/copilot-cli/issues/3551 · [S18] opencode.ai/docs/server,
opencode.ai/v2/docs/build/plugins · [S19] aider.chat/docs/usage/notifications.html ·
[S20] geminicli.com/docs/hooks/reference · [S21]
geminicli.com/docs/cli/notifications · [S22] cursor.com/docs/hooks,
cursor.com/docs/cli/reference/output-format · [S23]
docs.aws.amazon.com/amazonq/latest/qdeveloper-ug/command-line-context-hooks.html ·
[S24] docs.warp.dev/agents/using-agents/managing-agents · [S25]
docs.warp.dev/agents/capabilities/agent-notifications · [S26]
docs.warp.dev/terminal/windows/vertical-tabs · [S27]
code.visualstudio.com/docs/agents/run/agents-window · [S28]
code.visualstudio.com/docs/agents/run/sessions/manage-sessions · [S29]
code.visualstudio.com/updates/v1_69 and
`src/vs/workbench/browser/parts/notifications/notificationsCommands.ts` · [S30]
zed.dev/docs/ai/agent-panel · [S31] zed `assets/settings/default.json` · [S32]
cursor.com/docs/cloud-agent · [S33] forum.cursor.com/t/…/168083 · [S34]
jetbrains.com/help/ai-assistant/junie-agent.html · [S35]
junie.jetbrains.com/docs/junie-cli-worktrees.html · [S36]
ghostty.org/docs/config/reference · [S37]
github.com/ghostty-org/ghostty/discussions/10786 · [S38]
wezterm.org/config/lua/config/notification_handling.html · [S39]
sw.kovidgoyal.net/kitty/conf · [S40] sw.kovidgoyal.net/kitty/desktop-notifications ·
[S41] iterm2.com/documentation-preferences-profiles-terminal.html · [S42]
man.openbsd.org/tmux.1 and tmux `options-table.c` · [S43]
learn.microsoft.com/windows/terminal/customize-settings/profile-advanced · [S44]
github.com/microsoft/terminal/pull/20011 · [S45]
github.com/smilovanovic/tmux-claude · [S46] github.com/samleeney/tmux-agent-status ·
[S47] github.com/accessd/tmux-agent-indicator · [S48]
github.com/craftzdog/tmux-claude-session-manager · [S49]
iterm2.com/documentation-escape-codes.html · [S50]
learn.microsoft.com/windows/terminal/tutorials/progress-bar-sequences · [S51]
ghostty.org/docs/vt/osc/conemu · [S52]
learn.microsoft.com/windows/terminal/tutorials/shell-integration · [S53]
wezterm.org/shell-integration.html · [S54] iterm2.com/python-api/statusbar.html ·
[S55] github.com/ghostty-org/ghostty/discussions/10998 and
gitlab.freedesktop.org/terminal-wg/specifications · [S56]
github.com/warpdotdev/warp/issues/11466 · [S57]
developer.android.com/develop/ui/views/notifications/channels · [S58]
support.apple.com/guide/iphone/iph7c3d96bab · [S59]
support.apple.com/guide/iphone/iph21d43af5b · [S60]
slack.com/help/articles/201355156 · [S61] slack.com/help/articles/360056534254 ·
[S62] support.apple.com/guide/mac-help/mchl999b7c1a · [S63]
support.apple.com/guide/mac-help/mchl2fb1258f · [S64] Windows 11 Settings ▸
System ▸ Notifications · [S65] learn.microsoft.com
`ToastNotificationManager.GetToastNotificationMode` · [S66]
learn.microsoft.com/windows/terminal/customize-settings/profile-general · [S67]
learn.microsoft.com/windows/terminal/command-line-arguments · [S68]
learn.microsoft.com/windows/terminal/customize-settings/startup · [S69]
github.com/microsoft/terminal/issues/6165, /576, /7641 and pull/1357 · [S70]
docs.warp.dev/terminal/sessions/launch-configurations and
github.com/warpdotdev/warp/issues/8833 · [S71]
docs.warp.dev/terminal/windows/tab-configs · [S72]
docs.warp.dev/features/warp-drive/workflows · [S73]
man7.org/linux/man-pages/man1/tmux.1.html · [S74]
github.com/tmux-plugins/tmux-resurrect and tmux-continuum · [S75]
zellij.dev/documentation/session-resurrection.html · [S76]
zellij.dev/documentation/layouts.html · [S77] github.com/tmuxinator/tmuxinator ·
[S78] github.com/joshmedeski/sesh, github.com/ThePrimeagen/tmux-sessionizer ·
[S79] code.visualstudio.com/docs/editing/workspaces/workspaces · [S80]
code.visualstudio.com/docs/configure/settings · [S81]
github.com/microsoft/vscode/issues/246776 · [S82]
code.visualstudio.com/docs/terminal/profiles · [S83] zed.dev/docs/getting-started ·
[S84] iterm2.com/documentation-one-page.html · [S85]
iterm2.com/documentation-hotkey.html · [S86]
developer.apple.com/documentation/uikit/uiapplicationdelegate/applicationwillenterforeground(_:) ·
[S87] support.apple.com/en-us/108324 · [S88]
developer.apple.com/design/human-interface-guidelines/home-screen-quick-actions ·
[S89] developer.android.com/develop/ui/views/appwidgets/overview · [S90]
developer.android.com/develop/ui/views/launch/shortcuts.

**Inside this repository** — `docs/plans/attention/plan.md` (§3.4, §3.5, §10.7,
§11.1, §11.7) · `docs/plans/attention/evidence-cli-survey-2026-08-25.md` ·
`docs/plans/attention/evidence-copilot-cli-2026-08-26.md` · `docs/DESIGN.md`
§7.1.5b, §7.6, §7.51, §7.54, §7.55 ·
`crates/bt-app/src/{attention,attention_map,attention_wire,attention_words,attention_hooks,notify}.rs` ·
`crates/bt-persist/src/{profiles,session,layout,settings}.rs`.
