# The remote seam: one session authority, one wire, three clients — design note, 2026-09-27

Design note. Docs only; it rules nothing and moves no owner. It says where the
line falls between the part of Folio that *is* a session and the parts that
*look at* one, what one versioned protocol carries across that line for the three
clients the plans name (the 0.5 tool face, the 0.6 remote desktop client, the
mobile app), and what 0.5 must lay so that neither 0.6 nor the phone reworks it.
It is the design row R2 of `docs/plans/roadmap-0.5-2026-09-27.md` (§2.R) begun
early, against the minimum of that plan's §5.2 (a)–(e).

**Sources, by short name.** RM — `docs/plans/roadmap-0.5-2026-09-27.md` (rows
C1, L1–L3b, T1–T4, B4, R1, R2; §5, §5.2). AR — `docs/ARCHITECTURE.md` (§2
processes, §4 ownership, §5.1 lanes, §12). OC — `docs/plans/design/ownership-census-2026-09-25.md`
(§2's rows, cited as "census row n"). WB — `docs/plans/design/agent-workbench-0.5-2026-09-20.md`
(§6, §11.7–§11.9, §13.2). RS — `docs/plans/remote/research-2026-09-10.md` (§3,
§8, §9). TF — the tool-face design note of 2026-09-24 and the owner's rulings at
its end (the coordinator's records; RM §0.1). MC — the mobile design project's
context file (RM row R1). Outside sources are cited by URL in §1.

**The owner's rules this note is held to.** Folio collects nothing; its only
network traffic is what the person opens and the update check (MC §1).
Extensibility is outside programs driving Folio through a small versioned
interface, the `folio` CLI and MCP (WB §6). The phone shows messages and
information, redesigned for the phone, and is never a projection of the desktop
(MC §4). 0.5 is the workbench, 0.6 is remote, mobile after both (RM §1 item 5).

---

## 1. What production systems already do

The seam and the wire below are drawn from systems that have carried remote and
detachable terminals for years, read from their own papers, documentation and
source. Each row gives the pattern, how it fails in practice, and a verdict for
Folio. Where a row rests on a skim or a search summary rather than a reading,
its source cell says so.

| system and source | the pattern | failure modes in practice | verdict for Folio |
|---|---|---|---|
| **mosh** — the SSP paper (Winstein and Balakrishnan, USENIX ATC 2012, <https://mosh.org/mosh-paper.pdf>, §2–§3) | The server runs a terminal emulator and holds "the authoritative state of the terminal"; SSP synchronizes *state objects*, each Instruction an idempotent diff "between a numbered source and target state", sent at a frame rate set from the RTT (at least half the smoothed RTT apart, capped at 50 Hz, after an 8 ms collection interval), so a flood never fills the network and Control-C keeps working; roaming is the server re-targeting to the source of any authentic datagram with a higher sequence number (AES-128-OCB, key handed out over SSH); the client predicts echo in epochs and underlines unconfirmed guesses | By the paper's own words, state sync "causes trouble for a task like 'cat'-ing a large file … where the user might rely on having accurate history on the scrollback buffer": no scrollback. UDP only, so a network that passes only TCP breaks it. Prediction is a guess about a program the client does not run | **Adapt.** State sync, not byte sync, for every client that does not render a terminal: the ledger and the phone's text view are latest-value objects sent as numbered, idempotent diffs, paced by the link (§3, §4.3). **Avoid** prediction (RS §8.6 already) and UDP (the tailnet carries TCP) |
| **tmux** — the manual (<https://man.openbsd.org/tmux>), *Getting Started* and *Control Mode* (<https://github.com/tmux/tmux/wiki/Getting-Started>, <https://github.com/tmux/tmux/wiki/Control-Mode>); **GNU screen** — the manual, `-x` (<https://www.gnu.org/software/screen/manual/screen.html>) | "tmux keeps all its state in a single main process, called the tmux server"; clients attach over a socket, and a session "will survive accidental disconnection … or intentional detaching". Control mode (`-C`, `-CC`) is a line protocol for programs: commands answered inside `%begin`/`%end`/`%error` guards carrying a command number; asynchronous `%output %pane` notifications with the pane's bytes; flow control by `pause-after`, `%pause`, `%continue`; existing content fetched with `capture-pane`; a control client's size set by `refresh-client -C`. screen's `-x` attaches a second display to an attached session ("multi-display mode") | Control clients do not see the output of tmux's own modes; the command set is tmux's whole command set, so the protocol is as wide as tmux; a window has one size for all its clients | **Adopt** the server/client split, the numbered command/answer guard, and pause-then-resync as the flow-control answer (§3's `cmd`/`result`, §4.3's bounded queue). **Adapt** `capture-pane` + `%output` into RS §8.2's checkpoint + bytes |
| **iTerm2's tmux integration** (<https://iterm2.com/documentation-tmux-integration.html>) | A native GUI over `tmux -CC`: tmux windows become native windows and tabs, a split is `split-window`, a window resize is sent as the client's size; when the connection drops tmux keeps running and `tmux -CC attach` restores the layout | "A tab with a tmux window may not contain non-tmux split panes"; tmux's one size per window leaves "empty" gray areas for differently sized clients; scrollback and search are weaker than in a native window | **Adapt.** A native client over a server's session model works, and the desktop remains one: the host owns sessions, the desktop owns layout, and no tab mixes two ownership models. One PTY, one size (WB §11.7.4): a phone never proposes a size |
| **WezTerm multiplexer** — docs (<https://wezterm.org/multiplexing.html>); source `mux/src/domain.rs`, `wezterm-client/src/domain.rs`, `codec/src/lib.rs` (<https://github.com/wezterm/wezterm>) | A `Domain` owns panes, tabs and windows; the GUI has its own domain (`LocalDomain`, always attached, not detachable) and attaches to others through a `ClientDomain`, which keeps `remote_to_local` maps for windows, tabs and panes, resyncs from `ListPanes` by mark-and-sweep, and forwards each local operation ("translate the local ids …, resync the changed structure, and then translate the results back"). The codec: a `leb128` length whose high bit marks compression, a serial, an ident, the body; `CODEC_VERSION` 45; PDUs such as `GetPaneRenderChanges` (dirty lines, cursor, `seqno`, `input_serial`), `GetLines`, `WriteToPane`, `SendPaste`, `Resize`, `NotifyAlert`, `PaneRemoved`, `SetFocusedPane`, `GetTlsCreds`. Transports: a unix socket, SSH (start the server, then its socket), TLS with credentials bootstrapped over SSH | The docs call multiplexing "a young feature"; WSL 2 has no AF_UNIX interop; the client proxies server-computed render state, so every GUI feature needs a PDU (RS §8.2 records the maintainer's own objection); forty-five codec versions show a wire that moves with the internals | **Adapt** — the closest structure to Folio. Take the domain shape: the desktop's host is its own in-process local domain (§2); a remote host is mirrored through an id map, never by sharing ids. Take the frame header (a capped length, a serial, a kind) and render changes with a sequence number for the text projection. **Avoid** a protocol of internal operations (AR §12.3): Folio's wire carries domain commands, so it need not move with every internal change |
| **Eternal Terminal** (<https://eternalterminal.dev/howitworks/>) | SSH authenticates; the server mints a per-session passkey, starts its own process and closes SSH; each side's `BackedWriter` keeps "an encrypted buffer of the last N bytes sent and the sequence number" and, on reconnect, resends what the other side has not acknowledged — resumable TCP | A disconnection longer than the buffer loses bytes; it carries bytes, not a screen, so a client has nothing to rebuild from beyond the buffer | **Adopt** for the byte stream and the command stream: offsets and operation ids make a reconnect resume, not restart (RS §8.2's offset; §3's `op` with retained outcomes). **Adapt** its bootstrap for a headless host (RS §8.3) |
| **Zellij** — docs: web client (<https://zellij.dev/documentation/web-client.html>), session resurrection (<https://zellij.dev/documentation/session-resurrection.html>); the author's account (<https://poor.dev/blog/building-zellij-web-terminal/>) | A server process owns sessions, panes and PTYs; clients send input and receive "render instructions" as ANSI bytes; the web client is xterm.js over two websockets (terminal and control). Login tokens are shown once and stored hashed; the browser keeps a session token in an HTTP-only cookie; read-only tokens exist. Resurrection serializes layout and commands every second and re-runs a command only behind "Press ENTER to run…" | TLS is "a hard requirement" off localhost; the server "does not provide its own rate-limiting"; command rediscovery "can be inaccurate"; a browser terminal is a second terminal implementation | **Adopt** show-once, stored-hashed pairing secrets and a read-only class of client (§3, *Pairing*; §7 Q12), and resurrection's rule that nothing re-runs without the person, which S1 already states. **Avoid** a browser client, and any listener that is not bounded and rate-limited |
| **VS Code Remote** (<https://code.visualstudio.com/api/advanced-topics/remote-extensions>) | "Workspace Extensions" run in a remote extension host beside the files; "UI Extensions" run locally; clipboard, `openExternal`, webviews and URI handlers always act on the person's own machine; the server's version must match the client's exactly | Code placed remotely "can cause the application to launch on the wrong side"; native modules must be built for both sides | **Adopt** the placement rule for the ledger: a fact lives where its sources live — the ledger beside the sessions on the host; presentation, clipboard, hand-offs and notification delivery on each client (§2). **Avoid** the exact-version lock; the handshake negotiates (§3) |
| **Blink Shell** (<https://docs.blink.sh/advanced/advanced-mosh>; its background notes from a search summary only) and **Termius** (<https://docs.termius.com/help-center/faq/how-can-i-keep-termius-sessions-alive-in-the-background-on-ios-ipados>) | iOS stops a backgrounded app's activity "within 20 to 30 seconds" (Termius); both offer location tracking as the way to stay alive, and Blink leans on mosh to survive suspension | Location tracking is a battery and privacy cost paid for a side effect; sessions still die; neither can bring an event to a sleeping phone without a push service | **Avoid** background keep-alive tricks. **Adopt** the conclusion: a phone holds no live connection; push wakes it and it re-synchronizes on open (§5) |
| **Tailscale** — SSH (<https://tailscale.com/kb/1193/tailscale-ssh>), `whois` (<https://tailscale.com/kb/1080/cli>), the LocalAPI client (<https://pkg.go.dev/tailscale.com/client/local>) | Tailscale SSH authenticates "over WireGuard, using Tailscale node keys"; the server "already knows who the remote party is"; `whois` returns the node and the user behind an IP or IP:port, and the LocalAPI serves the same (`/localapi/v0/whois`) over the daemon's local socket | Policy (ACLs, check mode) lives in the tailnet's admin, outside the application; how the LocalAPI is reached differs by platform build (the Go client falls back beyond the socket on macOS) — a research item | **Adopt** tailnet identity as the network-level authentication (§3). **Adapt**: add an application-level device key, because not every device of a tailnet user is a paired device |
| **APNs** (<https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns>, <https://developer.apple.com/documentation/usernotifications/modifying-content-in-newly-delivered-notifications>); **FCM** (<https://firebase.google.com/docs/cloud-messaging/customize-messages/set-message-type>, search summary only) | Sending takes the app's provider token or certificate; 4 KB payloads; `apns-collapse-id`. A Notification Service Extension needs `mutable-content`, has "only about 30 seconds", and if it does not finish "the system displays the original contents"; decrypting server-encrypted data is a stated use. FCM data messages carry up to 4096 bytes; high priority is lowered for apps that do not show a notification | The provider credential cannot ship in an open-source desktop build; an extension that fails shows the placeholder | **Adopt** encrypted payloads opened by the extension, over a content-free placeholder (§5) |
| **ntfy** (<https://docs.ntfy.sh/config/>) and **UnifiedPush** (<https://unifiedpush.org/developers/intro/>) | A self-hosted ntfy server reaches iOS only through an upstream holding the APNs credential; the upstream request "contains only the message ID … and the SHA256 checksum of the topic URL", and the phone fetches the message from the person's own server. UnifiedPush (Android, Linux) lets the person choose the distributor and the push server; payloads are encrypted per RFC 8291 | iOS cannot be self-hosted end to end; the upstream still sees timing and a topic hash | **Adopt** ntfy's upstream shape as the relay's floor (a request carrying no content) and UnifiedPush as Android's fully self-hosted road (§5, §7 Q3) |

**Ideas listed for comparison only, not as models.** Dinotty and VelaTerm are
AI-written projects; Oxide is design documents with no implementation. Their
ideas stand in this note only where a production system above establishes the
same thing independently:

| idea | where it appears | established instead by |
|---|---|---|
| a server-side VT model; a snapshot, then increments | Dinotty | mosh's authoritative screen; WezTerm's render changes; tmux's `capture-pane` then `%output` |
| a one-time code for a new device | Dinotty | Zellij's show-once login token; the tailnet's node identity |
| terminal-primitive MCP tools; "what the user is looking at" first | Dinotty, Oxide | TF's verbs and the owner's rulings (T4) |
| debounced, subscribable events | Oxide | mosh's collection interval and frame rate; tmux's `pause-after` |
| phone push with QR pairing | VelaTerm | APNs and FCM with an extension; ntfy's upstream relay |
| a conversation view | VelaTerm | the owner's own ruling of 2026-09-26 (S2) |

---

## 2. The seam

**Who is authoritative today.** One process, one thread. The `folio` process's
window thread owns every fact a session has: `LeafSession` holds the PTY (census
row 169), the terminal model `DualPlaneSession` (row 170, a hub written from 12
modules), the typing queue (row 167) and the attention ledger's per-pane state
(row 162); `TabState.sessions` (row 158) is the container, and PTY bytes are
drained inside the event loop (`drain_pty`, rows 103 and 127). AR §12 states the
consequence: session state interleaved with chrome state on the window thread
**disqualifies** a backend that owns sessions while clients view them. A pane
closing retires its PTY (`close_pane`, row 169's writers), so a session cannot
outlive its only view; AR §4.1's first sentence is the rule that is not yet true.

**What becomes the host.** A set of owners that link with no window — the same
test RS §1 and §8.1 apply to a headless server — called **the host** here:

| owner | holds | arrives by |
|---|---|---|
| the session registry | identity, incarnation, lifecycle, the view bindings | C1, A3, A4 (0.4.7) |
| the terminal, per session | the one VT state machine that is the **reply authority**, the **size authority** and the **snapshot source** | RS §8.2 |
| input and size order, per session | one admission queue for every byte typed for someone, one ordered resize | C2 (0.5.2), A9 (0.5.6) |
| the attention ledger | the five facts per session, keyed by session identity; the word derived | L1 (0.5.0) |
| credentials and grants | every principal that may call, and what each may do | T2 (0.5.3) |

The shape is tmux's server and WezTerm's domain (§1): sessions belong to a
process that outlives any view. The desktop window is **the host's first
client, in-process** — WezTerm's GUI with its own always-attached local domain —
calling the host's API directly and never speaking the wire to itself. The wire
(§3) is for other processes. The placement rule is VS Code's: a fact lives where
its sources live, so the ledger sits with the sessions, and clipboard, hand-offs,
presentation and notification delivery stay with each client.

**One state machine, two stream shapes.** RS §8.2 chose, for a Folio client,
*a checkpoint plus the session's original bytes*, parsed again by the client's
own `DualPlaneSession` so marks, math, images and reflow are unchanged code. That
choice stands. A phone or an MCP tool runs no `bt-term`, so for them the same
state machine publishes a **text projection**: mosh's principle (the server's
screen is authoritative; numbered, idempotent diffs of state, at a rate the link
sets) in WezTerm's form (changed lines under a sequence number, older lines on
request), derived from the host's model and never from pixels. Mosh's known
cost, no scrollback, does not bind here: a Folio client that wants history takes
the byte stream, and the phone does not want it.
Where the host is the desktop itself (0.5, and the phone's case), its
presentation model *is* the state machine — one parse; the metrics-free
canonical copy of RS §8.2 is built only where no window draws the session (a
headless host, 0.6). The seam is the interface, not the number of parsers.

**The facts that move, or gain a second reader** (AR §4.1's three owners; census
rows as OC §2 numbers them):

| fact | today | after | change |
|---|---|---|---|
| `LeafSession.pty` (169, LIFE) | `panes` topic | the registry's session record | **moves**; a view's close no longer retires it (AR §4.1) |
| `LeafSession.session` (170, HUB) | `terminal` topic | the host's terminal | **moves**; new readers: the text projection (T4's `pane text`, the phone), the checkpoint (0.6) |
| `LeafSession.pending_typing` (167, LIFE) | `terminal` | C2's admission queue | **moves**; new writers are remote principals, through admission only |
| resize proposals; `pending_psreadline_resize_reanchor` (166) | `dpi` | the session's size authority | **moves** (AR §12.2 decision 1; A9) |
| `LeafSession.attention` (162, LIFE) | `deliver_attention` / `settle_attention` | the ledger in `bt-workbench`, keyed by session (L1, A4) | **moves**; new readers: the serializer (L3b), the push sender (§5) |
| `TabState.sessions` (158, HUB) | `panes` | the registry owns sessions; the tab holds view → session bindings | **splits** (A3) |
| `TabState.focused_leaf` (138, VIEW) | `panes` | stays the view's | **second reader**: the presence observation (§4.5) and T4's "what the user is looking at"; only the session it names crosses, never the address |
| `WindowRuntime.attention_next_place` (38, LIFE) | the ledger, lent by five modules | stays a window's queue | no outward reader |
| `pty_coalesce` (103), `unpainted_pane_output` (127) (PROJ) | `terminal` | stay the view's | their input becomes the session's output stream instead of the PTY rings |
| `projection` (168), `frame_image_references` (163), `last_presented_frame` (75) (PROJ) | `frame` | stay the view's | **never serialized** (§3, *What is never sent*) |
| `pending_paste` (165), `integration_offer` (164) | `clipboard`, `terminal` | stay the desktop's surfaces | a confirmed paste enters C2 |
| `App.session_store` (31, DUR) | `windows` | stays the transaction owner of `session.json` | **second writer of desired state**: the registry supplies session ids and vendor resume ids (C1, S1) through the store's own door |
| `seats` (157), `tabs` (121), `active_tab` (35), `tab_ids` (34) | view hubs | stay | never outward |

Nothing in AR §4.4's table (the update's facts) moves or gains a reader.

---

## 3. The wire

**One grammar, three clients.** One portable crate owns the grammar (RS §8.1
names it `bt-remote`): no window, no platform call, bounded readers, the
attention wire's discipline — declared fields only, everything bounded, failure
silent and counted (`attention_wire` header; RS §8.5).

| client | when | carriage | principal | may |
|---|---|---|---|---|
| the tool face (`folio <verb>`, `folio mcp`) | 0.5.3 | the tool endpoint, local only (TF "the door": a named pipe with `PIPE_REJECT_REMOTE_CLIENTS`; a `0700` Unix socket with a peer check) | a tool credential (§4.4) | TF §3's tiers and the owner's "operate Folio" |
| the remote desktop client | 0.6 | the tailnet carriage; RS §8.3's `ssh` stdio for a headless host | a paired device, class *desktop* | attach, read, type into panes it holds an input grant for |
| the phone | after 0.6 | the tailnet carriage | a paired device, class *phone* | the ledger, seen, dismiss, show, reply (§5) |

**Framing** (WezTerm's header, without its compression bit). A `u32` length,
checked against the family's cap **before** a byte is reserved (RS §8.5 item 1),
a serial and a kind, then a JSON body; a pane-bytes frame appends one raw segment
after its JSON header. JSON because the tool face and MCP speak it already and
the phone parses it for free; the byte segment keeps pane output out of it. First frame each way: `hello {protocols, product,
client_kind, proof}` / `welcome {protocol, host, boot, families, limits}`; a
version with no overlap is refused and said (RS §8.3).

**Families.** Each publication declares its kind, AR §4.2's fifth rule:

| family | client → host | host → client | kind |
|---|---|---|---|
| `ledger` | `subscribe {since?}`, `unsubscribe` | `snapshot`, `delta` | latest value; coalesced per row |
| `notice` | `subscribe`, `expand {notice}` | `raised`, `handled`, `expanded` | events with identity; never coalesced; loss answered by a snapshot |
| `session` | `subscribe`, `list` | `born`, `incarnated`, `exited`, `closed` | receipts |
| `pane.text` | `subscribe {session, incarnation, lines}` | `snapshot`, `delta` (settled lines appended, screen rows replaced) | latest value |
| `pane.stream` (0.6, Folio clients) | `attach {session, incarnation}` | `checkpoint`, `bytes {offset}` | loss-free up to a bound, then a re-checkpoint (RS §8.2) |
| `presence` | `presence {foreground, looking_at?}` | — | observation |
| `cmd` | `cmd {op, verb, target, expect_rev, args}` | `result {op, outcome, rev}` | receipt |

Commands are answered as tmux control mode answers them — one guarded reply
per numbered command, asynchronous publications between — and resumed as Eternal
Terminal resumes: `op` is the client's number, and a reconnecting client that
repeats an `op` gets the retained outcome instead of a second effect.
`outcome` is one of `accepted`, `refused {reason}`, `stale`, `not_executed`
(TF review item 3: a timeout before admission is *not executed*; after admission
the outcome is retained under `op`). Every command names its target as
`{session, incarnation}` and the revision it read; a stale target or revision is
refused, never retried onto a replacement (AR §12.3's first breakage).

**Capability tokens.** A connection's principal is fixed at `hello`; each
command is checked against that principal's tier and its live grants, and the
check is repeated at effect admission (TF review item 1). On the local endpoint
each request also carries the caller's `cap`, because many agent processes share
it (TF's `{v, cap, op, verb, args}`). A grant is never a bearer token handed to
another principal.

**The tailnet carriage.** Off by default: one switch, *allow paired devices*
(the fourth configuration entrance, A6, is where it is declared). On, the host
listens **only on the tailnet interface's address** — never on every interface,
never on a LAN address — and admits a connection only when (i) the local
Tailscale daemon's `whois` for the peer address names a paired device's node
(during pairing: a node of the host's own tailnet login), and (ii) the device
proves its key by signing the host's nonce. Confidentiality and integrity are
the tailnet's WireGuard; the protocol adds no second encryption in v1 (§7 Q8).
Tailscale absent or down: the switch says so and nothing listens anywhere else.

**Pairing** (Zellij's show-once token, over the tailnet's identity). The desktop shows a QR code
and a six-digit code, valid 120 s, single use, five attempts, then a new code.
The QR carries the host's tailnet name and port, the host key's fingerprint and a
128-bit pairing secret, shown once and kept only as a hash; the typed code stands in for the secret when there is no
camera. The code's only job is to prove that the hand holding the phone can see
the desktop; the tailnet already gives confidentiality and node identity, so no
PAKE is needed. The phone sends its public key (kept in the Secure Enclave or the
Android Keystore), a name, its class and, optionally, a push route (§5). The
desktop asks once, *Pair "<name>"?*, and lists the device under Settings with a
Revoke.

**What is never sent.**

1. **The desktop screen.** No frame, capture, pixel, layout or chrome; census
   rows 75, 163 and 168 have no serializer. A phone gets data it lays out itself.
2. **Bytes into a pane without an explicit grant** held by that principal for
   that session's current incarnation. A phone types only as a reply to an agent
   row, through C2 (§5); a desktop client only into sessions it attached with an
   input grant, shown on the host's pane while it stands.
3. **Credentials** — `FOLIO_ATTENTION`, the tool credential, device keys,
   pairing secrets — and grant identities to any other principal.
4. **Hook payloads** (the attention wire's rule), and the question text of a wait
   (not carried today; §7 Q6).
5. **Runtime internals** (AR §12.3): `AppEvent`, `Runtime`, window handles,
   `Instant`, rendered labels, positional indices, diagnostic prose.
6. **Plaintext to anyone outside the tailnet.** The relay of §5 receives
   ciphertext only.

---

## 4. The minimum 0.5 lays (as field lists)

Each list is what the owning contract must carry on the day it lands, so the
wire can serialize it later without a migration. *Absent is absent* (MC §3): an
optional field is omitted, never `null`, zero or `—`.

**4.1 Session identity** (C1's four things, plus the host).

| field | type | minted by | lifetime | crosses the wire |
|---|---|---|---|---|
| `host` | 128-bit random | the first start on a data directory | the data directory | yes |
| `boot` | 128-bit random | each start of the host | one process | yes (revisions restart with it) |
| `session` | 128-bit random | the registry, at a pane's birth | until the pane is closed; kept in `session.json` when restore brings the pane back | yes |
| `incarnation` | `u32`, +1 | the registry, at every new child process in the session | one child | yes, in every command |
| `agent_epoch` | `u32`, +1 | the ledger, when an agent's lifetime begins inside the incarnation (WB §13.2) | one agent run | yes |
| `vendor_session` | `{vendor, id}`, observed | a hook's declared id field (`IdSource`) | as the vendor keeps it | read-only; never an address |
| `view` | `{window, tab, seat}` | the desktop's view owners | one binding | **never** (a view is a client's) |
| `program` | `{kind: shell \| agent, mark}` | profile or recognition (L4) | the incarnation | yes |
| `exit` | `{code, at}` | the registry | once | yes |

Invariants, C1's: a move changes `view` only; a restore keeps `session` and
bumps `incarnation`; a resume never resurrects a grant; every credential and
grant dies with its incarnation.

**4.2 The ledger's shape** (L3a's contract, serialized by L3b; states per MC §3).

`snapshot {host, boot, seq, at_ms, rows[], accounts[]}` and
`delta {boot, seq, prev_seq, upsert[], remove[{session, agent_epoch, reason}], accounts[]}`.

A row: `session`, `incarnation`, `agent_epoch`, `rev` (the `seq` of its last
change), `agent {mark, name}`, `title` (bounded), `folder` (display text),
`state` (the A2A value — `working`, `input-required`, `completed`, `failed`,
`x-folio/idle`, `x-folio/limited` — **derived at serialization and stored
nowhere**, so no client derives its own), `unread` (derived from the
acknowledgement watermark), and the facts, each present only with a source:
`turn {phase, since_ms}`, `waits[{id, kind, since_ms}]` (`kind` is
`WaitKind`: `permission`, `elicitation`, `agent`, `quota`), `outcome {kind,
at_ms, lede}` (lede ≤ 80 characters, `attention_words`' rule), `notice {id,
raised_ms, muted, handled}`, `context {percent}`, `model`. An account:
`{company, account, windows[{name, left_percent, resets_at_ms, fetched_at_ms}]}`
(Q1–Q4).

**4.3 The subscription.** `subscribe {since: {boot, seq}?}`. The host keeps a
bounded run of deltas (a count and an age); a `since` inside it gets deltas, any
other gets a snapshot. Rows are latest values, coalesced per row over a short
window (mosh's collection interval); notice events are never coalesced. Each subscriber
has a bounded queue; overflow drops the queue, counts it, and the next frame is a
snapshot — tmux's pause-then-resync, never an unbounded buffer. With no change the host sends nothing (WB §6's quiescent budget); the
tailnet carriage pings only while a connection is open.

**4.4 The credentials** (T2, RS §8.5 item 7). The tool credential: `cap` (128
bits, in the pane's environment as `FOLIO_TOOL_CAP`, never in argv, the log or
a setting), `holder {session, incarnation}`, `minted_at`, default scope *read,
own tab* (the tab found through the holder's current view binding). A grant:
`{grant, holder: principal, target: {session, incarnation} | tab | all, tier:
read_other | send | operate | input | reply, generation, granted_at,
granted_via}`, ended by incarnation end, revoke or restart. **`principal` is an
enum from day one — `tool {cap}` now, `device {id}` in 0.6** — so one grant table
serves every client. The tool credential is admitted only on the local
endpoint; the tailnet carriage refuses it, so a leaked environment is not remote
access.

**4.5 Client presence** (AR §12.1's in-column; A7a). `presence {client, kind:
desktop_window | device, foreground, looking_at?: session, at_ms}`. The desktop
reports it from `seat_holds_the_keyboard` (WB §13.2) while it is the only client,
so AR §12.2's decisions 2 and 3 have their input before a second client exists.

---

## 5. Mobile v0: who is waiting for me

**The slice.** Three screens, no terminal. **The list**: every running agent
row, by severity (Waiting, Failed, Limited, Done unread) and then creation
order, each row mark · title · state word · age. **The item**: the same facts,
the lede, and on request the latest reply (`notice.expand`, V5's source: the
transcript tail the Stop hook names, else the screen tail). **A reply field**,
only where WB §11.7.3's predicate holds: no permission or quota wait, a turn
boundary observed, disarmed by any new event. One tap on a push opens the item;
*Show on desktop* is `session.show`, which the desktop answers as a person's
jump (`open_from_notification`).

**What it needs from the desktop, and where the plan puts each.** The ledger's
contract (L3a, 0.5.0); its serializer and subscription (L3b — RM §5 lands it in
0.5.3; §7 Q9 asks whether its read-only half moves to 0.5.1);
the notice stream and `expand` (V5, 0.5.0); C2's admission (0.5.2) and V11's
predicate (0.5.5) for a reply; the tailnet carriage and pairing (0.6); the push
sender (with the carriage). Nothing else: no pane text, no attach.

**Push needs a relay.** APNs and FCM accept sends only from a holder of the
app's provider credential, which cannot ship inside an open-source desktop
binary. So a small relay holds it, and the design keeps it blind:

| the relay | |
|---|---|
| **sees** | a route id, the device's push token, the arrival time, the sender's address, a ciphertext padded to a fixed size |
| **never sees** | agent, title, words, host name, tailnet identity, any count of anything |
| **keeps** | route id → token, until unpair or the push service reports the token dead; no per-send record, no log line naming a route |
| **does** | checks the route's HMAC, rate-limits it in memory, forwards |

At pairing the phone registers with the relay (route id, route secret) and hands
the desktop, over the tailnet, the route, its secret and a 256-bit payload key.
The desktop sends `AEAD(key, {notice, session, state, title, lede})`; on iOS a
Notification Service Extension decrypts it and shows the text (a generic
*Folio* line if it cannot), on Android a high-priority data message is decrypted
by the app. **Self-hosted-optional, honestly:** the relay is a published program;
the project would run the default one; a person may run their own, but only with
an app built under their own push credentials (on Android, UnifiedPush through a
self-hosted distributor removes even that). **A floor below that**, ntfy's
upstream shape: the push carries no content at all, only an opaque id, and the
app fetches the notice over the tailnet when it wakes — at the cost that an
offline phone shows only *Folio*. **The no-telemetry rule holds**: the
relay is a delivery pipe the person turns on by pairing with push; nothing about
use is collected; the desktop sends to it only for a notification the ledger
raised, and the update check stays the only other traffic Folio starts.

**Who interrupts** (AR §12.2 decision 3), proposed: the host decides once per
notice. A focused desktop window with recent input keeps it on the desktop; a
phone in the foreground gets it in-app; otherwise the push. *Handled anywhere,
gone everywhere* (WB): opening the item on the phone is a seen observation
(§7 Q7), and `handled` travels as a notice event.

**Offline.** Without the tailnet the push still arrives (the relay is on the
internet) and the item shows what the push carried; every action is disabled and
the state line reads *Offline*.

---

## 6. The stack for the phone

| | native SwiftUI + Kotlin/Compose | Flutter | React Native | **Rust core + native UI** |
|---|---|---|---|---|
| one person, with agents | two small UIs, and the protocol, pairing and crypto written twice | one UI in Dart; the protocol a third implementation | one UI in TypeScript; native modules for push and the key store anyway | two small UIs; **the protocol, pairing, crypto and ledger model are the desktop's own `bt-remote`** through UniFFI |
| push and background | first-class; the iOS extension is native | the iOS extension is still a native target; plugins sit between | as Flutter | first-class; the extension links the same core to decrypt |
| terminal text later | SwiftTerm on iOS, nothing equivalent shared | a Dart terminal widget, a second VT | a web-view terminal (xterm.js) | the host already sends text lines; a real VT later can be `bt-term`'s |
| binary size (order of magnitude, to be measured) | smallest | engine adds several MB | runtime adds several MB | native plus a few MB of core |
| store review | ordinary | ordinary | ordinary; over-the-air code pushes draw questions | ordinary |
| drift from the desktop | a hand-kept copy of the grammar | a hand-kept copy | a hand-kept copy | none: one crate, one set of bounded-reader tests |

Every column shares two review facts: App Review needs a demo mode, because a
reviewer cannot pair with a desktop, and the app must say plainly that it
requires Tailscale and a running Folio.

**Recommendation: the Rust core plus native UI, one platform first.** The phone's
UI is three screens and is where native wins (push, the extension, the system's
own materials, which MC §5 asks for); the part that must never drift — framing
caps, stale-revision refusal, the ledger's derived state, pairing — is the part
the desktop already owns and tests. It also forces the discipline 0.6 needs
anyway: `bt-remote` links with no window. Kotlin Multiplatform is the fallback if
the core outgrows UniFFI; Flutter with a Rust bridge is the fallback if the
owner wants both platforms from one UI on day one.

---

## 7. Open questions for the owner

Each with alternatives and a recommendation.

1. **Which phone first?** iOS · Android · both at once. *Recommend:* the one the
   owner carries; the second after v0 proves the protocol.
2. **The carriage.** (a) the tailnet only; (b) the tailnet, plus RS §8.3's `ssh`
   stdio for a headless Linux host; (c) a relay that carries sessions.
   *Recommend (b):* one grammar, two carriages; (c) never — a relay that carries
   terminal traffic is a server that sees it.
3. **The push relay.** (a) the project runs a blind relay carrying an
   encrypted payload, self-hosting for self-builders and UnifiedPush on Android;
   (b) the same relay, content-free (ntfy's upstream shape); (c) no push in v0,
   the list refreshes when opened; (d) hand notices to the person's own ntfy
   server, with no Folio app at all. *Recommend (a)* with (b) as the person's
   switch; (d) is worth a look as a desktop-only setting before the app exists.
4. **Who interrupts** (AR §12.2 decision 3). (a) §5's rule; (b) always both;
   (c) the phone only while the desktop is locked. *Recommend (a).*
5. **Replying from the phone.** (a) paste and submit, only at a free-text wait,
   only into a pane holding a *reply* grant, granted per profile on the desktop
   and shown on the pane; (b) paste only (useless away from the desk); (c) no
   reply in v0. *Recommend (a)*; permission answers stay out (WB §11.7.3).
6. **The question text of a wait.** (a) keep it off the wire; (b) carry it,
   bounded, to paired devices only. *Recommend (b)* once a vendor gives it through
   a declared field — "who is waiting" without "for what" is half an answer.
7. **Is opening the item on the phone *seen*?** (a) yes, it clears the dot
   everywhere; (b) no, only the desktop pane's focus counts. *Recommend (a)* —
   one notification is one object.
8. **A second encryption layer over WireGuard?** (a) no; (b) TLS with the pinned
   host key. *Recommend (a)* for the tailnet; (b) becomes necessary only if a
   carriage outside the tailnet is ever ruled in.
9. **When does the ledger's subscription ship?** (a) 0.5.3 with L3b, as RM §5
   has it; (b) pulled to 0.5.1 read-only, so the tool face and a phone prototype
   exercise it early. *Recommend (b)* if 0.5.1's capacity allows — it is small
   and it is the contract everything else depends on.
10. **Where the desktop's host runs in 0.6.** (a) inside `folio` (sessions end
    when Folio quits, as today); (b) a separate host process the window attaches
    to (sessions survive a restart and an update). *Recommend (a) first*; the
    seam of §2 keeps (b) possible, and the headless host of RS §8.1 is (b) on
    another machine.
11. **Tailscale as a requirement.** (a) accept it; (b) build our own NAT
    traversal. *Recommend (a)*: Folio then opens no internet listener at all.
12. **Device classes.** (a) two, *desktop* and *phone*, with fixed
    capabilities; (b) per-device capability lists. *Recommend (a)*.

---

## 8. This note's own architecture impact

**(a) Facts touched:** none — a document. It proposes, for the tickets that
implement it, the moves and second readers of §2's table: census rows 169, 170,
167, 166 and 162 move to the host's owners; 158 splits; 138 and 31 gain a
reader or a desired-state source; 75, 163 and 168 are named never-serialized.

**(b) Doors:** none opened. It names the doors its tickets would need, each on
AR §6's admission rule and §5.1's ingress lane: the tool endpoint (TF, 0.5.3);
the tailnet listener and the local Tailscale `whois` query (0.6); an HTTPS
`POST` to the relay beside `bt_platform::http`, which today has `https_get` and
`https_download` only; the device-key store. New argv doors are TF's `folio
<verb>` and `folio mcp` (AR §2.1); a headless host (§7 Q10 (b)) would add one
more, and a thread or two on the ingress lane (AR §0.1's counts change then).

**(c) Debt:** none added or repaid. It depends on D-1 and D-54 (the registry and
the session-named `Site`), D-57 (the ledger's crate), D-43 and D-44 (PTY birth
and resize off the window thread), by their current versions.

**(c′)** None.

**(d) Ownership change:** no — this note. Each implementing ticket that moves a
row of §2's table changes an owner and carries its own Codex-reviewed note
(CONVENTIONS rule 11); C1/A3 already has one owed (RM §2.C).
