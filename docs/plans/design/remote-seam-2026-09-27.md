# The remote seam: one session authority, one wire, three clients — design note, 2026-09-27

Design note. Docs only; it rules nothing and moves no owner. It says where the
line falls between the part of Folio that *is* a session and the parts that
*look at* one, what one versioned protocol carries across that line for the three
clients the plans name (the 0.5 tool face, the 0.6 remote desktop client, the
mobile app), and what 0.5 must lay so that neither 0.6 nor the phone reworks it.
It is the design row R2 of `docs/plans/roadmap-0.5-2026-09-27.md` (§2.R) begun
early, against the minimum of that plan's §5.2 (a)–(e).

**Revision (b), 2026-09-27, after the Codex review** (verdict *not yet*, 13
findings, 9 High; an owner-question triage; five missing owner questions; an
adoption checklist) **and the mobile design project's data contract** (its
`02-spec-from-mock.md` §3: 23 fields, a `mute` command, per-device preferences,
rows in creation order). The note is rewritten in place; §9 records each finding
and what changed. Where revision (a) and this text disagree, this text rules.
Changes in substance: a named, scheduled **session domain façade** (§2.2) is the
prerequisite of L1, L3a, C2, T1 and T2, and "a host with no window" is 0.6's,
not 0.5's; the wire has one publication envelope and one subscription state
machine (§3.3); authentication is two mechanisms and one authorization engine,
under a threat model with a mutually authenticated handshake (§3.6); the field
lists now carry C1's lifecycle, L1's fact-owner matrix, L2 and L3a's commands
(§4); the relay's metadata is stated, and automatic push is named as an
exception to the network rule that only the owner can grant (§5); the phone's
reply follows the settled paste-only ruling and waits for S2 (§5); §7 holds only
questions the sources do not settle.

**Sources, by short name.** RM — `docs/plans/roadmap-0.5-2026-09-27.md` (rows
C1, C2, A3, A4, A7a, A9, L1–L3b, S1, S2, T1–T4, V5, V11, B4, R1, R2; §3 row 21;
§5, §5.2). AR — `docs/ARCHITECTURE.md` (§2 processes, §4 ownership, §5.1
lanes, §12). OC — `docs/plans/design/ownership-census-2026-09-25.md` (§2's rows,
cited as "census row n") and its generated inventory
`docs/plans/design/ownership-census-inventory.tsv` (cited as "inventory
`<field>`"). WB — `docs/plans/design/agent-workbench-0.5-2026-09-20.md` (§6,
§11.7–§11.9, §13.2, §13.3). RS — `docs/plans/remote/research-2026-09-10.md` (§3,
§8, §8.8's tickets T1–T13, §9). TF — the tool-face design note of 2026-09-24 and
the owner's rulings at its end (the coordinator's records; RM §0.1). MC — the
mobile design project's context file (RM row R1); MS — the same project's
`design/02-spec-from-mock.md` (§3, its data contract). Outside sources are cited
by URL in §1.

**The owner's rules this note is held to.** Folio collects nothing; its only
network traffic is what the person opens and the update check (MC §1).
Extensibility is outside programs driving Folio through a small versioned
interface, the `folio` CLI and MCP (WB §6). The phone shows messages and
information, redesigned for the phone, and is never a projection of the desktop
(MC §4). 0.5 is the workbench, 0.6 is remote, mobile after both (RM §1 item 5).

---

## 1. What production systems already do

The seam and the wire are drawn from systems that have carried remote and
detachable terminals for years, read from their own papers, documentation and
source. Each row gives the pattern, how it fails in practice, and a verdict for
Folio. **Provenance is stated per row**: *read* means the primary text was in
hand; *read through a summariser* means the page or file was fetched and
answered by a summarising tool, so exact wording was not checked line by line;
*unsupported* means the claim was dropped.

| system and source | the pattern | failure modes in practice | verdict for Folio |
|---|---|---|---|
| **mosh** — the SSP paper (Winstein and Balakrishnan, USENIX ATC 2012, <https://mosh.org/mosh-paper.pdf>, §2–§3; read) | The server runs a terminal emulator and holds "the authoritative state of the terminal"; SSP synchronizes *state objects*, each Instruction an idempotent diff "between a numbered source and target state", sent at a frame rate set from the RTT (at least half the smoothed RTT apart, capped at 50 Hz, after an 8 ms collection interval), so a flood never fills the network; roaming is the server re-targeting to the source of any authentic datagram with a higher sequence number (AES-128-OCB, key handed out over SSH); the client predicts echo in epochs and underlines unconfirmed guesses | By the paper's own words, state sync "causes trouble for a task like 'cat'-ing a large file … where the user might rely on having accurate history on the scrollback buffer": no scrollback. UDP only | **Adapt.** State sync, not byte sync, for every client that does not render a terminal: the ledger and the phone's text view are latest-value objects sent as numbered, idempotent diffs, paced by the link (§3.3). **Defer** prediction until measured (RS §8.6's measurement, for an interactive desktop client only; tools and phones never predict). **Avoid** UDP (the tailnet carries TCP) |
| **tmux** — the manual, `window-size` and `destroy-unattached` (<https://man.openbsd.org/tmux>; read), *Getting Started* and *Control Mode* (<https://github.com/tmux/tmux/wiki/Getting-Started>, <https://github.com/tmux/tmux/wiki/Control-Mode>; read through a summariser); **GNU screen** — the manual, `-x` (<https://www.gnu.org/software/screen/manual/screen.html>; read) | "tmux keeps all its state in a single main process, called the tmux server"; a session "will survive accidental disconnection … or intentional detaching", and `destroy-unattached` decides whether the last detach ends it. A window has one size: `window-size` is `largest`, `smallest`, `manual` or `latest` ("the size of the client that had the most recent activity"). Control mode: commands answered inside `%begin`/`%end`/`%error` guards carrying a command number; `%output %pane` notifications; flow control by `pause-after`, `%pause`, `%continue`, after which the client refetches; existing content via `capture-pane`; a control client's size via `refresh-client -C`. screen's `-x` attaches a second display to an attached session | Control clients do not see the output of tmux's own modes; the command set is tmux's whole command set; a smaller client pans or pads | **Adopt** the server/client split, the numbered command/answer guard, pause-then-refetch as the flow-control answer (§3.3, §3.5), and an explicit last-detach rule (§4.2). **Adapt** `capture-pane` + `%output` into RS §8.2's checkpoint + bytes. Size policy is the owner's (§7 Q2) |
| **iTerm2's tmux integration** (<https://iterm2.com/documentation-tmux-integration.html>; read through a summariser) | A native GUI over `tmux -CC`: tmux windows become native windows and tabs, a split is `split-window`, a window resize is sent as the client's size; when the connection drops tmux keeps running and `tmux -CC attach` restores the layout | "A tab with a tmux window may not contain non-tmux split panes"; one size per window leaves "empty" gray areas for differently sized clients; scrollback and search are weaker than in a native window | **Adapt.** A native client over a server's session model works, and the desktop remains one: the host owns sessions, the desktop owns layout, and no tab mixes two ownership models. A phone never proposes a size |
| **WezTerm multiplexer** — docs (<https://wezterm.org/multiplexing.html>; read through a summariser); source `mux/src/domain.rs`, `wezterm-client/src/domain.rs`, `codec/src/lib.rs` (<https://github.com/wezterm/wezterm>; read through a summariser, and the review's independent inspection confirmed the maps, the mark-and-sweep resync, the framing and the render-change sequence fields) | A `Domain` owns panes, tabs and windows; the GUI has its own domain (`LocalDomain`, always attached, not detachable) and attaches to others through a `ClientDomain`, which keeps `remote_to_local` maps for windows, tabs and panes, resyncs from `ListPanes` by mark-and-sweep, and forwards each local operation ("translate the local ids …, resync the changed structure, and then translate the results back"). The codec: a `leb128` length whose high bit marks compression, a serial, an ident, the body, with a `CODEC_VERSION` constant checked at connect; PDUs such as `GetPaneRenderChanges` (dirty lines, cursor, `seqno`, `input_serial`), `GetLines`, `WriteToPane`, `SendPaste`, `Resize`, `NotifyAlert`, `PaneRemoved`, `SetFocusedPane`, `GetTlsCreds`. Transports: a unix socket, SSH (start the server, then its socket), TLS with credentials bootstrapped over SSH | The docs call multiplexing "a young feature"; WSL 2 has no AF_UNIX interop; the client proxies server-computed render state, so every GUI feature needs a PDU (RS §8.2 records the maintainer's own objection); the PDU set mirrors internal operations, so the wire is coupled to the internals | **Adapt** — the closest structure to Folio. The desktop is its own in-process local domain (§2); a remote host is mirrored through an id map, never by sharing ids; the frame header (a capped length, a serial, a kind) and render changes under a sequence number shape §3. **Avoid** a protocol of internal operations (AR §12.3): Folio's wire carries domain commands |
| **Eternal Terminal** (<https://eternalterminal.dev/howitworks/>; read through a summariser) | SSH authenticates; the server mints a per-session passkey, starts its own process and closes SSH; each side's `BackedWriter` keeps "an encrypted buffer of the last N bytes sent and the sequence number" and, on reconnect, resends what the other side has not acknowledged | A disconnection longer than the buffer loses bytes; bytes, not a screen, so nothing to rebuild from beyond the buffer | **Adopt** acknowledged offsets for the byte stream and operation ids with retained outcomes for commands (§3.3, §3.5). **Adapt** its bootstrap for a headless host (RS §8.3) |
| **Zellij** — docs: web client (<https://zellij.dev/documentation/web-client.html>), session resurrection (<https://zellij.dev/documentation/session-resurrection.html>); the author's account (<https://poor.dev/blog/building-zellij-web-terminal/>); all read through a summariser | A server process owns sessions, panes and PTYs; clients send input and receive "render instructions" as ANSI bytes; the web client is xterm.js over two websockets (terminal and control). Login tokens are shown once and stored hashed; the browser keeps a session token in an HTTP-only cookie; read-only tokens exist. Resurrection serializes layout and commands every second and re-runs a command only behind "Press ENTER to run…" | TLS is "a hard requirement" off localhost; the server "does not provide its own rate-limiting"; command rediscovery "can be inaccurate"; a browser terminal is a second terminal implementation | **Adopt** show-once, stored-hashed pairing secrets and a read-only class of grant (§3.6), and resurrection's rule that nothing re-runs without the person, which S1 already states. **Avoid** a browser client, and any listener that is not bounded and rate-limited |
| **VS Code Remote** (<https://code.visualstudio.com/api/advanced-topics/remote-extensions>; read through a summariser) | "Workspace Extensions" run in a remote extension host beside the files; "UI Extensions" run locally; clipboard, `openExternal`, webviews and URI handlers always act on the person's own machine; the server's version must match the client's exactly | Code placed remotely "can cause the application to launch on the wrong side"; native modules must be built for both sides | **Adopt** the placement rule for the ledger: a fact lives where its sources live — the ledger beside the sessions; presentation, clipboard, hand-offs and notification delivery on each client (§2). **Avoid** the exact-version lock; the handshake negotiates (§3.2) |
| **Termius** (<https://docs.termius.com/help-center/faq/how-can-i-keep-termius-sessions-alive-in-the-background-on-ios-ipados>; read through a summariser); **Blink Shell** (<https://docs.blink.sh/advanced/advanced-mosh>; establishes only that Blink uses mosh — revision (a)'s claim that Blink offers location tracking is **unsupported and removed**) | iOS stops a backgrounded app's activity "within 20 to 30 seconds" (Termius); Termius offers location tracking as the way to stay alive; Blink leans on mosh to survive suspension | Location tracking is a battery and privacy cost paid for a side effect; sessions still die; nothing brings an event to a sleeping phone without a push service | **Avoid** background keep-alive tricks. **Adopt** the conclusion: a phone holds no live connection; push wakes it and it re-synchronizes on open (§5) |
| **Tailscale** — SSH (<https://tailscale.com/kb/1193/tailscale-ssh>), `whois` (<https://tailscale.com/kb/1080/cli>), the LocalAPI client (<https://pkg.go.dev/tailscale.com/client/local>); read through a summariser | Tailscale SSH authenticates "over WireGuard, using Tailscale node keys"; the server "already knows who the remote party is"; `whois` returns the node and the user behind an IP or IP:port, and the LocalAPI serves the same (`/localapi/v0/whois`) over the daemon's local socket | Policy (ACLs, check mode) lives in the tailnet's admin, outside the application; node keys rotate; how the LocalAPI is reached differs by platform build — a research item | **Adopt** tailnet identity as the network-level authenticator. **Adapt**: an application-level device key and a host key, mutually proven, because the tailnet does not know which of a user's devices were paired, and does not survive a tailnet admin's policy change unchanged (§3.6) |
| **APNs** (<https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns>, <https://developer.apple.com/documentation/usernotifications/modifying-content-in-newly-delivered-notifications>; read); **FCM** (<https://firebase.google.com/docs/cloud-messaging/customize-messages/set-message-type>, <https://firebase.google.com/docs/cloud-messaging/android-message-priority>; read — upgraded from revision (a)'s search summary) | APNs: a provider token or certificate; 4 KB payloads; `apns-collapse-id`. A Notification Service Extension needs `mutable-content`, has "only about 30 seconds", and if it does not finish "the system displays the original contents"; decrypting server-encrypted data is a stated use. FCM: "Maximum payload for both message types is 4096 bytes"; "While the connection to FCM is encrypted, it is not end-to-end encrypted"; high-priority messages that do not produce user-visible notifications "may be deprioritized to normal priority" | The provider credential cannot ship in an open-source desktop build; an extension that fails shows the placeholder; provider-side metadata is visible to the provider | **Adopt** encrypted payloads opened on the device, over a generic placeholder, and a visible notification for every high-priority push (§5) |
| **ntfy** (<https://docs.ntfy.sh/config/>) and **UnifiedPush** (<https://unifiedpush.org/developers/intro/>); read through a summariser | A self-hosted ntfy server reaches iOS only through an upstream holding the APNs credential; the upstream request "contains only the message ID … and the SHA256 checksum of the topic URL", and the phone fetches the message from the person's own server. UnifiedPush (Android, Linux) lets the person choose the distributor and the push server; payloads are encrypted per RFC 8291 | iOS cannot be self-hosted end to end; every upstream still observes delivery metadata | **Adopt** ntfy's upstream shape as the relay's content-free floor, and UnifiedPush as Android's self-hosted road (§5, §7 Q5) |

### 1.1 Considered, and rejected or deferred

| design | what it is | verdict for Folio |
|---|---|---|
| **VS Code Remote Tunnels** (<https://code.visualstudio.com/docs/remote/tunnels>; read through a summariser) | "Both hosting and connecting to a tunnel requires authentication with the same Github or Microsoft account on each end"; both ends make outbound connections to a service hosted in Azure and "VS Code doesn't set up any network listeners"; an SSH connection runs over the tunnel "to provide end-to-end encryption"; usage limits on tunnels and bandwidth | **Rejected as the default, recorded as the real alternative.** It removes the listener and the Tailscale dependency, at the price of an account with a third-party service that relays every byte and sees every connection's metadata — against "Folio collects nothing" and "no service to run". It is §7 Q10's alternative (b) |
| **OpenSSH multiplexing** (<https://man.openbsd.org/ssh_config>, `ControlMaster`, `ControlPersist`; read) | "Enables the sharing of multiple sessions over a single network connection"; `ControlPersist` keeps the master "open in the background … after the initial client connection has been closed" | **Rejected, in one sentence**: a carriage optimisation, not session persistence or resynchronisation, and the Windows client cannot use it (RS §8.3, Q12); Folio multiplexes inside its own framing on one long-lived `ssh.exe` for the headless-host carriage |
| **Termius Vault** (<https://termius.com/vault>; read through a summariser) | Hosts, keys, passwords, snippets and port forwards synchronised across devices through a cloud vault "on Termius servers" (AWS), encrypted "before it leaves your device", with a local copy on each device | **Rejected explicitly.** Folio syncs no credential, device key, pairing record or configuration through any cloud. Pairing is device-local: a new or restored phone pairs again; a lost phone is revoked from the desktop. This is the answer to multi-device and recovery expectations, stated so no later feature assumes a vault |
| **tmux-resurrect** (<https://github.com/tmux-plugins/tmux-resurrect>; read through a summariser) | Saves sessions, windows, panes, layout, each pane's working directory and "programs running within a pane" from a conservative list; optionally pane contents; does not preserve live processes | **Adopt as a warning for S1**: persisted desired state and vendor resume ids restore *what may be started*, never replay commands; Folio starts an agent only as S1 states and sends it nothing (§4.2's restore row) |

### 1.2 Ideas listed for comparison only, not as models

Dinotty and VelaTerm are AI-written projects; Oxide is design documents with no
implementation. Their ideas stand in this note only where a production system
above establishes the same thing independently:

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

### 2.1 Who is authoritative today

One process, one thread. The `folio` process's window thread owns every fact a
session has: `LeafSession` holds the PTY (census row 169), the terminal model
`DualPlaneSession` (row 170, a hub written from 12 modules), the typing queue
(row 167) and the attention ledger's per-pane state (row 162);
`TabState.sessions` (row 158) is the container; PTY bytes are drained inside the
event loop (`drain_pty`, rows 103 and 127). AR §12 states the consequence:
session state interleaved with chrome state on the window thread
**disqualifies** a backend that owns sessions while clients view them. Closing a
pane retires its PTY (`close_pane`, among row 169's writers), so today a session
cannot outlive its only view.

### 2.2 The session domain façade — the 0.5 step, and the 0.6 host

The shape is tmux's server and WezTerm's domain (§1): sessions belong to an owner
that outlives any view. It arrives in two steps, and revision (a) wrongly spoke
of the second as if it were the first.

**Step 1 (0.5): the session domain façade — a semantic owner, on today's
thread.** Proposed as a new row, **SD**, with its own Codex-reviewed note
(CONVENTIONS rule 11), landing after A3/A4 (0.4.7) and **before L1, L3a, C2, T1
and T2**, which consume it. It owns, behind one API whose callers never touch
`LeafSession` fields:

- **lifecycle**: create, view bind and unbind, child exit, explicit close,
  retirement — the registry's (C1, A3) transitions of §4.2, with the PTY
  (row 169) held by the session record rather than the view;
- **the terminal**: every read and write of `DualPlaneSession` (row 170) goes
  through the façade; replies the parser mints are written by the façade;
- **transport**: the PTY drain, exit detection and close go through the façade,
  which publishes output to its views (rows 103 and 127 become consumers);
- **order**: one admission queue for typed input (row 167, C2's) and one
  ordered resize (§2.4's resize set, A9's);
- **a window-thread adapter**: while the mechanics still run on the window
  thread, the adapter drives them there; moving them to the session transport
  lane (AR §5.1) is A9's and later work, and changes no caller.

Without SD, L1 and L3a key lifecycle to a view and migrate again, C2 writes a
view-owned queue, T2 grants attach to the current child instead of `{session,
incarnation}`, and L3b can serialize view identity. With SD, those consumers are
written once. **Roadmap §5.2's sentence stands as written** — A9 (PTY birth and
resize off the window thread) is still not a prerequisite for 0.6 — and this
note adds that **SD is**, and is a prerequisite of the 0.5 rows above. In 0.5
the façade is a domain boundary, **not a windowless host**.

**Step 2 (0.6): the host with no window.** The no-window link test (RS §1,
§8.1) becomes true with RS §8.8's **T4** (the canonical terminal, headless, in
a library) and **T5** (the broker owning sessions): the same façade API,
implemented by a process that links no window. On the desktop, where a window
does draw the session, the façade keeps one parse (§2.3); where no window does,
T4's metrics-free canonical copy is the terminal.

The desktop window is the façade's **first client, in-process** — WezTerm's GUI
with its own always-attached local domain — and never speaks the wire to itself.
The wire (§3) is for other processes. The placement rule is VS Code's: the
ledger sits with the sessions; clipboard, hand-offs, presentation and
notification delivery stay with each client.

### 2.3 One state machine, two stream shapes

RS §8.2 chose, for a Folio client, *a checkpoint plus the session's original
bytes*, parsed again by the client's own `DualPlaneSession` so marks, math,
images and reflow are unchanged code. That stands. A phone or an MCP tool runs
no `bt-term`, so for them the same state machine publishes a **text
projection**: mosh's principle (the server's screen is authoritative; numbered,
idempotent diffs of state, at a rate the link sets) in WezTerm's form (changed
lines under a sequence number, older lines on request), derived from the
façade's model and never from pixels. Mosh's known cost, no scrollback, does not
bind: a Folio client that wants history takes the byte stream, and the phone
does not want it. Where a window draws the session, its presentation model *is*
the state machine — one parse.

### 2.4 The facts that move, split, or gain a reader

Census rows as OC §2 numbers them; the other `LeafSession` fields from the
generated inventory, which the census rows do not list because they have one
writer each. Every `LeafSession` field is placed; three are left for the SD note
to classify, and say so.

| fact | today | after SD | change |
|---|---|---|---|
| `LeafSession.pty` (169, LIFE) | `panes` topic | the session record | **moves**; a view's close no longer retires it by itself (§4.2 decides when a session ends) |
| `LeafSession.session` (170, HUB) | `terminal` topic | the façade's terminal | **moves**; new readers: the text projection (T4's `pane text`, the phone), the checkpoint (0.6) |
| `LeafSession.pending_typing` (167, LIFE) | `terminal` | C2's admission queue | **moves**; remote principals write only through admission |
| **the resize set**: inventory `LeafSession.conpty_grid`, `pending_pty_resize`, `pending_psreadline_resize_reanchor` (166, LIFE) | `dpi`, `panes`, free functions | the session's size authority | **moves** (AR §12.2 decision 1; A9); the exhaustive list of resize writers is A9's own inventory |
| inventory `LeafSession.grid` | panes | the view's measured size | **stays** the view's; it becomes a *proposal* to the size authority, not the PTY's size |
| `LeafSession.attention` (162, LIFE), inventory `attention_clock`, `attention_capability`, `bell_reported` | `deliver_attention` / `settle_attention` | the ledger in `bt-workbench`, keyed by session (L1, A4); the attention credential with the session record | **moves**; new readers: the serializer (L3b), the push sender (§5) |
| inventory `incarnation`, `wake`, `profile`, `program`, `spawn_place`, `integration`, `output_revision`, `last_finished_command` | set at birth or by the drain | the session record | **move** with it (identity, launch facts, output revision, the mark ledger's last command) |
| inventory `paste_recipient`, `last_seen_revision`, `card_skip` | various | — | **for the SD note to classify** (each could be the session's or a view's) |
| `TabState.sessions` (158, HUB) | `panes` | the registry owns sessions; the tab holds view → session bindings | **splits** (A3) |
| `TabState.focused_leaf` (138, VIEW) | `panes` | stays the view's | **gains a reader**: the presence observation (§4.5) and T4's "what the user is looking at"; only the session it names crosses the wire |
| `App.session_store` (31, DUR) | `windows` | stays the transaction owner of `session.json` | **gains a desired-state source, not a reader**: the registry supplies session ids and vendor resume ids (C1, S1) through the store's own door |
| `WindowRuntime.attention_next_place` (38, LIFE) | the ledger, lent by five modules | stays a window's queue | no outward reader |
| `pty_coalesce` (103), `unpainted_pane_output` (127) (PROJ) | `terminal` | stay the view's | consume the façade's output publication instead of the PTY rings |
| `projection` (168), `frame_image_references` (163), `last_presented_frame` (75), inventory `presented_metrics`, `metrics`, `text_scale`, `has_rail`, `thumb_awake`, `column_awake` | `frame`, view topics | stay the view's | **never serialized** (§3.8) |
| `pending_paste` (165), `integration_offer` (164) | `clipboard`, `terminal` | stay the desktop's surfaces | a confirmed paste enters C2 |
| `seats` (157), `tabs` (121), `active_tab` (35), `tab_ids` (34) | view hubs | stay | never serialized; not an authority in any grant (§4.7) |

Nothing in AR §4.4's table (the update's facts) moves or gains a reader.

---

## 3. The wire

### 3.1 One grammar, three clients

One portable crate owns the grammar (RS §8.1 names it `bt-remote`): no window,
no platform call, bounded readers, the attention wire's discipline — declared
fields only, everything bounded, failure silent and counted (`attention_wire`
header; RS §8.5). Clients negotiate the families they use; none needs all.

| client | when | carriage | authenticator | families |
|---|---|---|---|---|
| the tool face (`folio <verb>`, `folio mcp`) | 0.5.3 | the tool endpoint, local only (TF "the door": a named pipe with `PIPE_REJECT_REMOTE_CLIENTS`; a `0700` Unix socket with a peer check) | the tool credential (§3.6) | `ledger`, `session`, `pane.text`, `cmd` (TF §3's tiers and "operate Folio") |
| the remote desktop client | 0.6 | the tailnet carriage; RS §8.3's `ssh` stdio for a headless host | a paired device, class *desktop* | all, `pane.stream` included |
| the phone | after 0.6 | the tailnet carriage | a paired device, class *phone* | `ledger`, `notice`, `session`, `presence`, `cmd` (§5) |

### 3.2 Framing and compatibility

WezTerm's header, without its compression bit. Every frame:
`u32 total_len | u32 serial | u16 kind | u32 json_len | json | raw`, where
`total_len` covers everything after itself, `raw` is `total_len − 10 − json_len`
bytes (empty except in `pane.stream`), and **a global cap (1 MiB) is checked on
`total_len` before a byte is reserved**; after `kind` is read, the family's own
narrower cap applies (RS §8.5 item 1). Before authentication the cap is 4 KiB
and a connection may send at most one `hello` (§3.6).

- **Negotiation.** `hello {protocols: [major…], families: [{name, minor}], client_kind}`
  → `welcome {protocol, families: [{name, minor}], host: {id, name, os, version}, boot, limits}`.
  The host picks the highest common major; a family with no common minor is
  absent from `welcome`; no common major is refused with `error
  {code: "version", supported}` and the connection closes (RS §8.3). A client
  never downgrades on its own; it offers the list it speaks.
- **Unknown data.** Unknown JSON fields are ignored. An unknown enum value is
  kept as `{"unknown": "<value>"}` by the reader and never acted on; an unknown
  `kind` frame is dropped and counted. Minor versions only add optional fields,
  enum values and families; anything else is a major version.
- **Errors.** One shape: `error {code, family?, op?, detail?}`, `code` from a
  closed list (`version`, `auth`, `limit`, `malformed`, `unknown_target`,
  `stale`, `busy`, `internal`); `detail` is bounded text for people and is never
  parsed.

### 3.3 One publication envelope, one subscription state machine

Every publication, in every family, is:

`pub {family, scope, boot, gen, seq, kind: snapshot | delta | event | reset, body}`

- `scope` names what is subscribed (`ledger`; a `session`; `{session,
  incarnation}` for pane families). `boot` is the host's process identity;
  `gen` is the scope's stream generation, bumped whenever the host cannot
  continue from the previous history (a new incarnation, a lost history, an
  overflow); `seq` is strictly increasing within `(boot, gen)`.
- A **snapshot** states the whole scope at `seq`; a **delta** carries `prev_seq`
  and applies only on top of exactly that state (idempotent: mosh's numbered
  source and target); an **event** is a receipt with its own identity; a
  **reset** tells the client its cursor is void and a snapshot follows.

**Subscription:** `subscribe {family, scope, since?: {boot, gen, seq}}`. States,
host side: *idle → syncing* (a snapshot, or deltas from `since` if the host
still holds that history) *→ live → paused* (the queue passed its bound) *→
syncing* again (reset, then snapshot). The client acknowledges with `ack
{family, scope, seq}` at least every *window* publications; the host keeps at
most *window* unacknowledged publications per subscription (credit-based
backpressure; *window* is in `welcome.limits`). A subscription whose client
stops acknowledging is paused, not buffered — tmux's `%pause`, followed by the
client's refetch.

**Mutation cases**, each a test the L3b brief carries:

| case | the client sees | the client does |
|---|---|---|
| gap (`prev_seq` ≠ its `seq`) | a delta it cannot apply | drops it, sends `resubscribe` with its cursor; the host answers with deltas or a reset + snapshot |
| duplicate (`seq` ≤ its `seq`) | an already-applied publication | ignores it |
| reorder | cannot happen on one ordered carriage; treated as a gap | as a gap |
| boot change | `boot` ≠ its `boot` | discards every cursor and every retained `op` expectation; resubscribes without `since` |
| queue overflow | `reset` | discards the scope's state; applies the snapshot that follows |
| stale incarnation | `pane.*` publication with an older `incarnation`, or `reset` for a new one | discards the pane state; resubscribes to the new `{session, incarnation}` if it still wants it |

### 3.4 The families, and what each retains

| family | scope | snapshot | delta / event | retention on the host | reconnect |
|---|---|---|---|---|---|
| `ledger` | the host | every row and account (§4.6) | rows upserted or removed; accounts upserted; coalesced per row over a short window (mosh's collection interval) | the last 256 deltas or 10 minutes, whichever is less | `since` inside the retained run → deltas; else snapshot |
| `notice` | the host | every notice not yet handled or expired | `raised`, `handled`, `expanded` events, never coalesced | open notices, and handled ones for 10 minutes as tombstones | `since` inside → events; else snapshot of open notices |
| `session` | the host | every session: `{session, incarnation, program, folder, title, exit?}` | `born`, `incarnated`, `exited`, `closed` events | closed sessions as tombstones for 10 minutes | as `notice` |
| `pane.text` | `{session, incarnation}` | the screen's rows and the last *N* settled lines (N ≤ 1,000), each line `{id, text}` in UTF-8, the cursor `{row, col, visible}` | `lines {append[], replace[{row, text}], scroll_off}` under `seq` | the current screen only | always a snapshot (the screen is small; history on request with `lines.get {before_id, count ≤ 500}`) |
| `pane.stream` (0.6, Folio clients) | `{session, incarnation}` | RS §8.2's checkpoint at `offset` | `bytes {offset, len}` raw, and `resize {epoch, cols, rows}` in order with the bytes | the journal's run from the last checkpoint (RS §9 Q3) | `since` offset inside the run → bytes; else a new checkpoint (a new `gen`) |
| `presence` | the connection | — | client → host only, a lease (§4.5) | the lease | re-sent on connect |

`ledger` and `notice` overlap on purpose: a row's `notice` field is the latest
notice's state (a latest value, for the list); the `notice` family is the
stream of notice events (for delivery and the lock screen). A client that shows
only the list subscribes to `ledger` alone.

### 3.5 Commands and their outcomes

`cmd {op, verb, target: {session, incarnation}?, expect_rev?, args}` →
`result {op, outcome, rev?}`, answered as tmux control mode answers — one reply
per numbered command. **Operation identity is `(principal_id, boot, op)`**: a
client that repeats an `op` after a reconnect under the same `boot` gets the
retained outcome, never a second effect (Eternal Terminal's resend, applied to
commands). The host retains the last 256 outcomes or 10 minutes per principal;
older repeats get `error {code: "stale"}`.

Outcomes, terminal and closed:

| outcome | means |
|---|---|
| `done` | the effect is committed: for `seen`, `mute`, `dismiss`, `show` the ledger changed; for typed input, **C2 wrote the bytes to the session's transport** — not that the agent read them |
| `admitted` | typed input only: queued in C2 at a position; a later `result` for the same `op` brings `done` or `failed` |
| `refused {reason}` | not permitted or not applicable now: `no_grant`, `not_waiting_for_text`, `muted`, `unknown_target`, `limit` |
| `stale` | the target's incarnation or the row's revision is not the one named; nothing happened |
| `not_executed` | cancelled or timed out before admission; nothing happened |
| `failed {reason}` | admitted and then could not complete (the PTY closed, the child exited); nothing further will happen |

### 3.6 Authentication and authorization

**Two authentication mechanisms, one authorization engine.** The tool face
authenticates with a local bearer credential; a paired device with the tailnet's
node identity *and* its own key. Both resolve to a `principal` (an opaque id,
§4.7), and one engine decides every command from the principal's grants. The
attention credential (`FOLIO_ATTENTION`) stays a third, attention-only
mechanism outside this engine (TF review item 1).

**Threats, and what answers each.**

| threat | answer |
|---|---|
| an unpaired node joins the tailnet (or is shared into it) | `whois` must name a node **bound to a paired device**; outside pairing, nothing else is admitted |
| a tailnet admin or policy change | the device key is still required; a node that changes owner fails the binding check (the pairing bound node id *and* user) |
| a paired phone is compromised or lost | revoke from the desktop ends its principal and every grant at once; phones hold only *read*, *seen/mute/dismiss/show* and per-session *reply* grants (§4.7), so the damage is reading the list and pasting text at a free-text wait |
| a local tool credential is stolen | it is valid only on the local endpoint, only for its incarnation, only within its tiers; the tailnet carriage refuses it |
| replay of a handshake or a command | handshake proofs sign fresh nonces from both sides and the transcript; commands are bound to `(principal, boot, op)` and a connection |
| downgrade | the transcript signed by both keys includes both offered protocol lists and the chosen one |
| host impersonation or host key change | the phone pins the host key at pairing; every connection proves it; a changed key refuses the connection and says so; re-pairing is the only road |
| resource exhaustion by a client | §3.2's pre-auth cap, one `hello`, a 5-second handshake deadline, a cap on connections per node (4) and in total (16), per-subscription credit, per-principal command rate |
| node key rotation (Tailscale) | the binding is to the node's stable id and user, not its key; a rotated key under the same stable id stays paired |

**The handshake, mutually authenticated.** After TCP accept on the tailnet
address: (1) the host asks `whois` for the peer and holds `{node_stable_id,
user}`; (2) `hello` carries the client's nonce `nc` and its device id; (3)
`welcome` carries the host's nonce `nh` and a host signature over `T = hash(nc,
nh, host_id, boot, client offer, chosen protocol, node_stable_id, user, device
public key)`; (4) the client verifies it with the pinned host key and replies
with its device signature over the same `T`. Only then is the principal
resolved. The keys are the application's (P-256, a curve both the Secure Enclave
and Android Keystore sign with in hardware); the transport is still the
tailnet's WireGuard.

**Pairing** (Zellij's show-once token, over the tailnet's identity). The desktop
starts a pairing window of 120 s with a **host-wide budget of five attempts**,
consumed atomically and never reset by reconnecting; the window ends on
success, on the budget, or on time. The QR carries the host's tailnet name and
port, the host key's fingerprint and a 128-bit secret; the six-digit code
stands in for the secret when there is no camera, **in which case the phone
shows the host key's short fingerprint and the desktop shows the same, and the
person confirms they match**. The pairing proof is a MAC keyed by the secret (or
the code) over the handshake transcript `T`, so a code cannot be moved to
another connection. The desktop keeps only a hash of the secret, asks once —
*Pair "<name>"?* — and lists the device under Settings with a Revoke. What the
phone registers: its device public key, name, class, the node binding `whois`
returned, and optionally its push route (§5).

### 3.7 Secrets and identifiers: where each goes

| item | made by | goes to | stored | logged | ends |
|---|---|---|---|---|---|
| tool credential `FOLIO_TOOL_CAP` | the façade, per tool-enabled incarnation | the pane's environment; presented only to the local tool endpoint | the host keeps a hash | never | incarnation end, revoke, restart |
| host key (private) | the host, per data directory | nowhere | the host's key store | never | reset of the data directory |
| host key (public) / fingerprint | the host | the phone, at pairing | the phone's pin | may be | re-pair |
| device key (private) | the phone, in secure hardware | nowhere | the Secure Enclave / Keystore | never | unpair, app removal |
| device key (public), device id | the phone | the host, at pairing | the host's device list | device id only | revoke |
| pairing secret / code | the host | the phone (QR or eyes) | a hash, for 120 s | never | the pairing window |
| relay route id and route secret | the relay | the phone, then the host over the tailnet | both | route id may be; secret never | unpair (deleted at the relay by both ends) |
| payload key and key version | the phone | the host over the tailnet | both | never | rotation (§5), unpair |
| push token | APNs / FCM | the phone, then the relay | the relay | never | token refresh, unpair |

No private key or bearer secret is ever published or forwarded; an
authenticator is presented only to the endpoint that authenticates it.

### 3.8 What is never sent

1. **The desktop screen.** No frame, capture, pixel, layout or chrome; census
   rows 75, 163 and 168 and §2.4's view fields have no serializer. A phone gets
   data it lays out itself.
2. **Bytes into a pane without a grant** held by that principal for that
   session's current incarnation (§4.7). A phone's bytes are only a reply at a
   free-text wait, in C2's paste-only mode (§5).
3. **Private keys and bearer secrets**, as §3.7 states.
4. **View identity or authority**: window, tab, seat, positional indices;
   "own tab" is resolved to sessions on the host at authorization time, and the
   wire never names a tab.
5. **Hook payloads** (the attention wire's rule), and the question text of a
   wait until a vendor gives it in a declared field (§7 Q9).
6. **Runtime internals** (AR §12.3): `AppEvent`, `Runtime`, window handles,
   `Instant`, rendered labels, diagnostic prose.
7. **Notice content outside the tailnet in plaintext.** Content that leaves the
   tailnet leaves only as the push ciphertext; its metadata is not hidden, and
   §5 states what is seen.

---

## 4. What 0.5 lays, as field lists to cut briefs from

*Absent is absent* (MC §3): an optional field is omitted, never `null`, zero or
`—`. Times: **ordering and expiry use the host's monotonic clock**; every
`*_ms` on the wire is the host's wall clock for display only; a client's own
time is never trusted — the host stamps what it receives.

### 4.1 Session identity (C1)

| field | type | minted by | lifetime | on the wire |
|---|---|---|---|---|
| `host` | 128-bit random | the first start on a data directory | the data directory | yes |
| `boot` | 128-bit random | each start of the process that holds the façade | one process | yes |
| `session` | 128-bit random | the registry, at a session's creation | until the session ends by §4.2; kept in `session.json` while restore owes it | yes |
| `incarnation` | `u32`, +1 | the registry, at every new child process in the session | one child | yes, in every command |
| `agent_epoch` | `u32`, +1 | the ledger, when an agent's lifetime begins inside the incarnation | one agent run | yes |
| `vendor_session` | `{vendor, id}`, observed | a hook's declared id field (`IdSource`) | as the vendor keeps it | read-only; never an address; not sent to phones (MS §3.3) |
| `view` | `{window, tab, seat}` | the desktop's view owners | one binding | **never** |
| `program` | `{kind: shell \| agent, mark}` | profile or recognition (L4) | the incarnation | yes |
| `exit` | `{code, at}` | the registry | once per incarnation | yes |

**C1's "incarnation (agent lifetime)" is two numbers here, and the C1 note must
rule it.** An agent typed into a running shell starts and ends without a new
child process, so one number cannot be both. WB §13.2 already has both
(`incarnation` plus "an agent-lifetime epoch"): `incarnation` guards typed input
and credentials (a replacement shell must refuse old input); `agent_epoch` keys
ledger rows and notices (a new agent run must not inherit an old row's waits).

**Persistence.** The registry writes `session` and `vendor_session` into the
session document as desired state through `SessionStore`'s door (row 31);
"stored" means the store's landed receipt (AR §4.2, durability). A restored
`session` that collides with a live one (possible only with a copied data
directory) is re-minted and the event is logged.

### 4.2 C1's lifecycle transitions

| event | session | incarnation | grants and credentials | views |
|---|---|---|---|---|
| create (a pane opens a shell or agent) | minted | 1 | a tool credential if tool-enabled | one bound |
| bind another view (drag-out float V10, a 0.6 client) | unchanged | unchanged | unchanged | +1 |
| move (tab, window, tear-off) | unchanged | unchanged | unchanged | rebound (AR §4.1) |
| unbind a view that is not the last | unchanged | unchanged | that view's principal's input grant ends | −1 |
| **unbind the last view** | **ends in 0.5** (today's behaviour: the child is retired) — whether it survives is §7 Q3 | — | all end | 0 |
| child exits | stays, with `exit` | — | every grant and credential of the incarnation ends | unchanged; the pane shows the exit |
| restart the child (restart shell, resume an agent) | unchanged | +1 | minted afresh; nothing resurrected | unchanged |
| explicit close (the person closes the pane or kills the session) | ends | — | all end | all unbound |
| Folio quits, updates or crashes | ends (0.5: the process owns the children); restore owes it | — | all end | — |
| restore after a start | the persisted `session`, if restore brings the pane back | +1 | minted afresh | one bound |

tmux's `destroy-unattached` is the precedent for making "last view gone" an
explicit rule rather than an accident of the code; tmux-resurrect's is for
restore restoring what may be started, never replaying commands.

### 4.3 L1: each fact, its owner, its expiry

| fact | owner (producer) | source | expiry | precedence | invalidated by |
|---|---|---|---|---|---|
| agent lifetime (`agent_epoch`, present) | the ledger, on a recognition event (L4) | hook credential from the pane, or an OSC row from its tty | the incarnation's exit; an explicit end event | the first recognition event opens it; nothing else does | child exit, a new incarnation |
| turn phase | the ledger | hook turn-start/turn-end; OSC 133 command end where a vendor has no hook | turn end; the agent's end | a hook beats an OSC inference for the same epoch | a new turn start; agent end |
| outstanding waits | the ledger | hook waits with a `WaitKind`; `OSC 1337;RequestAttention` | the producer's clear; `WAIT_TTL` (`attention::expiry`) | each wait is keyed by its own id; a clear clears only its own | agent end; a new incarnation |
| acknowledgement watermark (seen) | the ledger | `seen` from a client with presence (§4.5); the desktop's focus rule (WB §13.2) | never expires; moves forward only | the highest acknowledged `seq` wins | a new `agent_epoch` resets it |
| last outcome and lede | the ledger | hook turn end with its declared words; OSC 9/777/99 | replaced by the next outcome | the latest by host receive order | agent end removes the row |
| account quota | the quota owner (Q1–Q4) | statusLine lane; the vendors' own binaries | `fetched_at` + the vendor's window; shown greyed with its age when stale (MC §3) | the newest fetch | account removed |
| mute (per agent) | the ledger | `mute` from any client (MS §3.5) or the desktop | the agent's end | the latest by host receive order | agent end |

The row's state word and dot are **derived** from these every time and stored
nowhere (WB §13.2); so is `unread` and so is `reply_open`.

### 4.4 L2: the action log

WB §11.9 and RM row L2, unchanged and restated so the brief has it here: one
append-only store in Folio's data folder; one line per action taken through the
tool face or a paired device — `when · principal id · session · agent · verb ·
outcome`; **no payload, no typed text, no terminal output, no secret**;
size-bounded with oldest-first eviction; removed by `folio --uninstall-cleanup
--purge`. Interruption counts (WB §2.5) are queries over it. A phone's `reply` is
logged as a verb and an outcome, never its text.

### 4.5 L3a: the commands and observations the domain takes

| input | from | effect | never |
|---|---|---|---|
| `presence {foreground, looking_at?: session}` | every client, as a **lease** of 60 s renewed while foreground; a disconnect ends it at once | the host stamps it on receipt; decides who gets an interruption (§5) | a lease never outlives its connection; an expired foreground lease counts as background |
| `seen {session, agent_epoch, through_seq}` | a client with presence, or the desktop's focus | moves the watermark; clears `unread` | seen never answers a wait (WB §13.2) |
| `answered` | only the producer (a hook's clear, an OSC `no`) | clears that wait | no client command answers a wait; a reply is typed input, not an answer |
| `mute {session, agent_epoch, on}` | any client | per-agent mute: silences the notification only, never the dot and never Failed (RM V5) | — |
| `dismiss {notice}` | any client | marks the notice handled everywhere | does not touch the dot or the wait |
| `show {session}` | a device | the desktop opens that session as a person's jump (`open_from_notification`) | not an interruption of the desktop (it is the person's own act) |
| `reply {session, incarnation, notice?, text}` | a device holding a *reply* grant | C2, **paste-only** (RM §3 row 21; WB §13.3.1), only while `reply_open` | never Enter; never at a permission or quota wait |
| `device.prefs {notifications, done}` | the phone about itself (MS §3.7) | the host sends no push the phone would drop | — |

Interruption (AR §12.2 decision 3) is decided on the host per notice from
presence leases; the rule is §7 Q7's.

### 4.6 L3b: the row, the account and the host, with the mobile contract

The ledger row, with MS §3.8's 23 additions folded in (marked ⁺):

`session`, `incarnation`, `agent_epoch`, `rev`, `born_seq`⁺ (creation order),
`agent {mark, name}`, `title` (≤ 120), `folder` (≤ 260, display text),
`branch`⁺ (≤ 120), `state` (the A2A value — `working`, `input-required`,
`completed`, `failed`, `x-folio/idle`, `x-folio/limited` — derived, absent with
no state facts), `state_since_ms`⁺ (derived with `state`), `unread`, `muted`⁺
(per agent), `turn {phase, since_ms}`, `waits[{id, kind, since_ms, question⁺
(≤ 300, only per §7 Q9), resets_at_ms⁺ (quota waits)}]`, `outcome {kind, at_ms,
lede ≤ 80}`, `notice {id, raised_ms, muted, handled}`, `context {percent}`,
`model`, `account⁺ {company, account}`, `reply_open`⁺ (WB §11.7.3's predicate,
evaluated on the host), `activity⁺ {verb, object}` (from S2, 0.5.5).

An account: `{company, account, mark⁺, kind⁺: subscription | api, windows[{name,
left_percent, resets_at_ms, fetched_at_ms}], balance⁺ {amount, currency,
fetched_at_ms}, in_use_by⁺ [session], readable⁺: false}`.

The host, in `welcome`: `{id, name⁺, os⁺, version⁺}`.

The push body (§5): `{v, notice, host⁺, session, agent_epoch⁺, state, agent⁺,
title, raised_ms⁺, lede, question⁺, resets_at_ms⁺}`, and the retraction
`{v, handled⁺}`.

**Rows are ordered by `born_seq`, creation order**, as the desktop rail (WB
§11.3) and the mobile mock keep them; revision (a)'s severity sort is
withdrawn. Severity colours the badge, not the order.

### 4.7 Principals, credentials and grants (T2, and the 0.6 device)

- **Principal**: `{principal_id (opaque, 128-bit), kind: tool | device}`. A tool
  principal points at a credential id and its holder `{session, incarnation}`;
  a device principal at a device id. **The raw credential is never an identity**
  and never appears in a record or a log.
- **Tool credential**: `{credential_id, verifier (hash of the cap), holder
  {session, incarnation}, minted_at}`; dies with the holder's incarnation, on
  revoke, and on restart (TF, owner Q3).
- **Device**: `{device_id, public_key, name, class: desktop | phone, node_binding
  {node_stable_id, user}, paired_at, push_route?}`; lives until revoked or the
  data directory is reset.
- **Grant**: `{grant_id, principal_id, target: {session, incarnation} | all,
  tier: read | read_other | notify | send | operate | input | reply |
  ledger_read | ledger_act, generation, granted_at, granted_via}`. A grant whose
  target is a session **dies with that incarnation**; a grant whose target is
  `all` (a device's `ledger_read` and `ledger_act`, the owner's "operate
  Folio") lives with its principal until revoked. "Own tab" is not a target: the
  host resolves it to sessions at authorization time from the holder's current
  view binding.
- **Device classes** are product presets of grants, not authority: a *phone*
  receives `ledger_read` and `ledger_act` (seen, mute, dismiss, show) and, per
  session, `reply`; a *desktop* receives, per attached session, `input`.
  Effective capability is always the grants.

### 4.8 When each lands

Unchanged from the roadmap: L3a in 0.5.0; **L3b, T1 and T2 in 0.5.3** (RM §5).
Revision (a)'s question about moving L3b is withdrawn — the plan settles it.
Early exercise needs no reschedule: an **in-process subscriber** built against
L3a's contract in 0.5.0/0.5.1 (a test harness and the mobile project's simulator
feed) runs the §3.3 state machine and its mutation cases before the endpoint
exists.

---

## 5. Mobile v0: who is waiting for me

**The slice.** Three screens, no terminal, no pane attach, no permission
answers. **The list**: every running agent row **in creation order**
(`born_seq`), each row mark · title · state word · age, the badge counting by
severity. **The item**: the same facts, the lede, and on request the latest
reply (`notice.expand`: V5's source, the transcript tail the Stop hook names,
else the screen tail). **A reply field, conditional**: shown only while the
host's `reply_open` is true, and it sends a **paste-only** reply — the text lands
on the agent's input line without Enter, the agent's draft preserved (RM §3 row
21; WB §13.3.1). That is the settled 0.5 ruling; a phone reply that submits would
need the owner to supersede it (§7 Q8). *Show on desktop* is `show`. Mute is per
agent.

**The dependency chain, and three stages.**

| stage | needs | what it proves |
|---|---|---|
| read-only prototype, against a simulator feed | L3a (0.5.0), L3b's serializer (0.5.3), V5's expand (0.5.0) | the list, the item, the subscription state machine |
| conversation and reply | S2's model (0.5.5), C2's paste-only mode (0.5.2), V11's predicate (0.5.5) | the item's conversation segment, the reply |
| real devices | the tailnet carriage, pairing and the push sender (0.6) | everything, on a phone |

**Push needs a relay, and the relay is not blind to metadata.** APNs and FCM
accept sends only from the holder of the app's provider credential, which
cannot ship inside an open-source desktop build. So a small relay holds it.

| | the relay |
|---|---|
| **sees, per push** | the route id; the device's push token; the arrival time; the sender's IP address; a ciphertext of fixed size. From these it can **count** pushes per route, see their **timing and cadence**, and **correlate** a route with an IP address and a token |
| **keeps** | route id → `{token, route-secret verifier, platform}` until unpair or the provider reports the token dead; an in-memory rate-limit bucket per route (a counter, discarded after its window); nothing else |
| **never keeps** | a per-send record; a log line naming a route, token or address; any aggregate |
| **infrastructure** | the host's own access logs are off; any hosting-provider logs are outside the relay's control and are named in the published privacy text |
| **the providers** | Apple and Google see the app's topic or project, the token, the time, the relay's address and the size; FCM states that it is not end-to-end encrypted, which is why the body is |
| **abuse** | an HMAC over each request with the route secret; a route that exceeds its rate is refused for the window; a route with a dead token is deleted |
| **retention, at most** | route mappings for the life of the pairing; nothing about a send beyond the rate window |

"No telemetry" therefore means **no analytics, no profile and no durable
per-send record**, not "the relay observes nothing". And **automatic push is new
network traffic**: Folio today starts only the update check and what the person
opens. Pairing with push is consent, but it does not make the owner's sentence
literally true — **only the owner can extend that rule** (§7 Q4, blocking).

**The payload.** ChaCha20-Poly1305 (CryptoKit on iOS; the platform cipher on Android 9
and later) with a 256-bit key from the phone; nonce = `key_version
(32 bits) ‖ counter (64 bits)`, the counter persisted by the host per route and
never reused; AAD = `{v, route_id, key_version, counter}`; the phone refuses a
counter it has already seen and a `raised_ms` older than 24 hours. The key
rotates on re-pair and whenever the phone sends a new `key_version` over the
tailnet; token refresh is the phone's business with the relay and does not touch
the host. Plaintext is padded to 1,024 bytes; with the tag, the header and base64
the provider payload stays under 1.6 KB, inside both 4 KB limits. On iOS the
Notification Service Extension decrypts; the `aps.alert` it replaces is a
generic *Folio*, which is what shows if it cannot. On Android a high-priority
data message is decrypted and **always** shown as a notification, so FCM does not
deprioritize it. A retraction `{v, handled}` removes a delivered notification
(*handled anywhere, gone everywhere*). **Content-free** is the floor (ntfy's
upstream shape): the push carries only an opaque id and the app fetches over the
tailnet — at the cost that an offline phone shows only *Folio* (§7 Q5).

**Self-hosting, honestly.** The relay is a published program; the project would
run the default one; a person may run their own only with an app built under
their own push credentials. On Android, UnifiedPush through a self-hosted
distributor removes the relay's provider dependency.

**Offline.** Without the tailnet the push still arrives and the item shows what
the push carried; every action is disabled and the state line reads *Offline*.

---

## 6. The stack for the phone

Weighted as the owner weighs it: one person maintaining it with agents first,
then push and background reliability, then binary size, then store review.
Terminal rendering is **not** a criterion for v0 — the phone presents
information and does not project a terminal (MC §4).

| | native SwiftUI + Kotlin/Compose | Flutter | React Native | **Rust core + native UI** |
|---|---|---|---|---|
| one person, with agents | two small UIs; the protocol, pairing and crypto written twice | one UI in Dart; the protocol a third implementation | one UI in TypeScript; native modules for push and key store anyway | two small UIs; the protocol, the subscription state machine and the ledger model are the desktop's own `bt-remote` through UniFFI; the UniFFI wrappers, lifecycle glue and extension packaging are still per platform and can still drift |
| push and background | first-class; the iOS extension is native | the extension is still a native target; plugins between | as Flutter | first-class; whether the Rust core fits the extension's launch and memory budget is **the spike's first question** |
| binary size (to be measured) | smallest | an engine | a runtime | native plus the core |
| store review | ordinary | ordinary | ordinary; over-the-air code pushes draw questions | ordinary |

For every column, Guideline 2.1 (<https://developer.apple.com/app-store/review/guidelines/>)
asks for demo account information and a live back end "if your app includes a
login", or a built-in demo mode "with prior approval by Apple": the app must
**provide a reviewable demo or a working review setup**, since a reviewer
cannot pair with a desktop.

**Recommendation, conditional on a one-week spike: the Rust core plus native
UI, one platform first.** The spike passes if: the core links into the app and
into the iOS Notification Service Extension (or the Android messaging service);
§3.3's mutation cases run on the device; key operations stay native (Secure
Enclave / Keystore signing is called through native APIs, never as key bytes in
Rust); a payload decrypts within the extension's time and memory limits; a
TestFlight or internal-track build installs; the binary size is measured. If the
core does not fit the extension, the extension decrypts natively and the core
stays in the app. Kotlin Multiplatform is the fallback if the core outgrows
UniFFI; Flutter with a Rust bridge if the owner wants both platforms from one UI
on day one.

---

## 7. Open questions for the owner

Only questions the sources do not settle. Each has alternatives and a
recommendation. **Settled by the sources, and not asked:** L3b's version (0.5.3,
RM §5; §4.8); a second encryption layer (the threat model of §3.6 requires
application-level mutual authentication and signed transcripts; a second
transport encryption inside the tailnet adds nothing it needs); device classes
(presets of grants, §4.7; the owner is asked only if a class should offer
different actions); pairing budgets, key rotation, token refresh and revocation
mechanics (engineering, §3.6, §5); the reply mode (settled paste-only; Q8 is a
labelled request to supersede, only if the owner wants it).

1. **Which phone first?** iOS · Android · both at once. *Recommend:* the one the
   owner carries; the second after v0 proves the protocol.
2. **PTY size and simultaneous input, before any second interactive view**
   (A7a decision 1, due 0.5.1). The 2026-09-10 ruling says two clients attached
   to one session both see it live and both may type (WB §11.7.4); this carries
   it forward. (a) *latest*: the size follows the interactive client with the
   most recent input, as tmux's `latest`; others show the session at that size,
   letterboxed or scaled, never reflowed; (b) a size **leader** by explicit
   take-over, released on disconnect to the next most recent; (c) *smallest*.
   Input from several clients is totally ordered by host arrival on C2's queue;
   a resize is ordered with the bytes (RS §8.2's resize epoch). Phones and tools
   are never size candidates. *Recommend (a)*, with the leader shown on each
   view, and leadership passing on disconnect.
3. **How long does a session live?** Last view closed · Folio quits · update ·
   crash · reboot. (a) as today: every one ends it; (b) the last view's close
   leaves it running (a headless session in the window, reattachable), quit ends
   it; (c) a separate host process keeps sessions across quit and update, not
   reboot. *Recommend (a) for 0.5* (§4.2 records it), *(b) as 0.6's first
   step*, and (c) later, where RS §8.1's broker already points.
4. **May pairing authorise Folio to send automatic push traffic?** (blocking) —
   the owner's network rule names only what the person opens and the update
   check. (a) yes, opt-in per device, off until the person turns it on, with
   §5's metadata and retention statement published; (b) yes, on by default once
   paired; (c) no: the phone learns only when it is opened. *Recommend (a).*
5. **What a push carries.** (a) the encrypted notice; (b) nothing but an opaque
   id, the app fetching over the tailnet (ntfy's shape); (c) the person picks
   per device. *Recommend (c)*, default (a).
6. **The lock screen.** (a) title and lede shown by default; (b) generic
   *Folio* until unlock, text on opt-in; (c) follow the phone's own "show
   previews" setting. *Recommend (c)*: the platform setting is where people
   already decide this.
7. **Who is interrupted** (A7a decision 3). (a) the host decides per notice
   from presence leases: a desktop window focused with input in the last few
   minutes keeps it; else a foreground phone gets it in-app; else the push;
   (b) always both; (c) the phone only while the desktop is locked. *Recommend
   (a)*, and opening an item on the phone counts as **seen** everywhere (A7a
   decision 2) — never as answered.
8. **(A request to supersede, only if wanted.)** The notification reply is
   ruled paste-only (RM §3 row 21). From a phone, paste-only lands text the
   person must still submit at the desk. (a) keep paste-only; (b) allow
   paste-and-submit from a paired phone, only at a free-text wait, only into a
   session holding a *reply* grant. *Recommend (a) for v0*; revisit after S2
   shows how often a phone reply is wanted.
9. **The question text of a wait**, once a vendor gives it in a declared field
   (it is off the wire today). (a) keep it off; (b) send it, bounded, to paired
   devices. *Recommend (b)* when such a field exists; nothing to rule before.
10. **The carriage and its trust.** (a) the tailnet (Tailscale required) plus
    `ssh` for a headless host; (b) an outbound tunnel service, as VS Code
    Remote Tunnels (§1.1): no listener and no Tailscale, but an account with a
    service that relays every connection; (c) no remote access beyond the
    tailnet. *Recommend (a)*; building our own NAT traversal is not a near-term
    alternative.
11. **What each paired phone may receive.** Folder, title, latest reply,
    account quota, model. (a) everything the row carries; (b) per-device
    switches for folder, latest reply and quota; (c) per-session opt-in.
    *Recommend (b)*, defaults on, set on the desktop's device page.

---

## 8. Architecture impact

**This commit:** (a) facts touched: none — a document; (b) doors: none;
(c) debt: none added or repaid; (c′) none; (d) ownership change: no.

**What this note proposes, if adopted.** It is **not** the rule-11 note for any
move: each still needs its own Codex-reviewed note (CONVENTIONS rule 11) — SD's
(§2.2), C1's (owed, RM §2.C), A9's.

- (a) Facts: census rows 169, 170, 167, 166 and 162, and the inventory fields
  of §2.4, move to the session façade and the ledger; row 158 splits; row 138
  gains a reader; row 31 gains a desired-state source; rows 75, 163 and 168 and
  the view fields of §2.4 are named never-serialized; three inventory fields
  are left for SD's note to place.
- (b) Doors, each on AR §6's admission rule and §5.1's ingress lane: the tool
  endpoint (TF, 0.5.3); the tailnet listener and the local Tailscale `whois`
  query (0.6); an HTTPS `POST` to the relay beside `bt_platform::http`, which
  has `https_get` and `https_download` only; the host key store and the device
  list. New argv doors are TF's `folio <verb>` and `folio mcp` (AR §2.1); a
  headless host (§7 Q3 (c)) would add one more, and threads on the ingress lane
  (AR §0.1's counts change then).
- (c) Debt: it depends on D-1 and D-54 (the registry and the session-named
  `Site`), D-57 (the ledger's crate), D-43 and D-44 (PTY birth and resize), by
  their current versions; SD is a new row the roadmap would gain, not debt.
- (d) Ownership changes: yes, as listed in (a), each through its own note.

---

## 9. Revision record

### Revision (b), after the Codex review (2026-09-27)

| # | finding | what changed |
|---|---|---|
| 1 | High — "host without a window" had no landing row; sequencing ahead of the roadmap | §2.2: a named row **SD**, the session domain façade (lifecycle, terminal access, drain/reply/exit/close, input and resize order, a window-thread adapter), prerequisite of L1, L3a, C2, T1, T2; "windowless host" reserved for 0.6, made true by RS §8.8 T4 and T5; roadmap §5.2's A9 sentence kept and reconciled |
| 2 | Medium — the resize set; row 31 | §2.4 adds inventory `conpty_grid`, `pending_pty_resize` (with row 166) and places `grid` as the view's proposal; every `LeafSession` field placed, three left to SD's note; row 31 "gains a desired-state source, not a reader"; §8 split into this commit's impact and the proposed architecture, and says this note is not the rule-11 note |
| 3 | High — not a resumable protocol | §3.2 (length covering both parts, the raw segment's length, a global pre-allocation cap, negotiation, unknown data, one error shape); §3.3 (one envelope with `boot`/`gen`/`seq`, snapshot/delta/event/reset, credit-based acks, the state machine, the mutation-case table); §3.4 (per-family snapshot, retention and reconnect); §3.5 (`(principal_id, boot, op)`, retention bounds, a closed outcome vocabulary separating `admitted` from `done`) |
| 4 | High — two credentials, not one | §3.6 "two authentication mechanisms, one authorization engine"; §4.7 opaque principal ids, hashed verifiers, separate lifetimes for tool credentials, devices and grants; `tab` removed as a grant target; §3.7–§3.8 reworded |
| 5 | High — threat model and mutual host authentication | §3.6 threat table; a handshake signed by both keys over one transcript binding nonces, protocol offers and choice, host, boot, node identity and device key; pairing with a host-wide atomic budget, a MAC over the transcript, fingerprint confirmation on the code path, key-change and node-key rotation rules, pre-auth bounds |
| 6 | High — field lists not sufficient for C1, L1–L3 | §4.1 identity with the incarnation/agent-epoch split stated for C1's note; §4.2 lifecycle table; §4.3 L1 fact-owner-expiry matrix and the time rule; §4.4 L2; §4.5 L3a commands and observations; §4.6 L3b shapes; §4.8 keeps 0.5.3 and adds an in-process subscriber for early exercise |
| 7 | High — the relay learns counts; push is a network exception | §5: what the relay sees, keeps and never keeps; what the providers see; abuse handling; maximum retention; "no telemetry" defined; the cipher, nonce, AAD, replay, rotation, token refresh, unpair, size budget and fallback; §7 Q4 (blocking) and Q5 |
| 8 | High — reply mode contradicted row 21; S2 omitted | §5: reply is paste-only and conditional on `reply_open`; S2 (0.5.5) in the chain; three stages; §7 Q8 is a labelled request to supersede |
| 9 | Medium — survey corrections | §1: per-row provenance; mosh "defer prediction until measured"; tmux `window-size` and `destroy-unattached` read and quoted; WezTerm's version-count inference removed, provenance stated; Blink's location claim removed; FCM upgraded to primary pages |
| 10 | Medium — four missing designs | §1.1: VS Code Remote Tunnels (rejected as default, kept as Q10's alternative), OpenSSH multiplexing (rejected in one sentence), Termius Vault (cloud sync rejected explicitly), tmux-resurrect (a warning adopted for S1) |
| 11 | Medium — "never sent" not literal | §3.7 data-flow table; §3.8 rewritten: secrets presented only to their authenticating endpoint, no view authority, metadata acknowledged separately from content |
| 12 | Medium — stack certainty and weighting | §6: weighted as the owner weighs it; terminal rendering removed as a criterion; drift admitted in the wrappers; recommendation conditional on a one-week spike with pass criteria; Guideline 2.1 quoted instead of "needs a demo mode" |
| 13 | High — PTY size leadership and simultaneous input missing | §7 Q2, carrying the "both may type" ruling forward, with the size policy, input order, leadership on disconnect |
| — | the owner-question triage and five missing questions | §7 rewritten: settled items listed and not asked; new Q2 (size and input), Q3 (session lifetime), Q4 (push as a network exception), Q6 (lock screen), Q11 (information scope) |
| — | the mobile data contract (MS §3) | §4.6: the 23 fields; §4.5: `mute` and `device.prefs`; rows ordered by `born_seq` (creation order), revision (a)'s severity sort withdrawn |
