# Security

Folio is a terminal emulator for Windows. It runs shells, hosts a web preview,
writes into three other tools' configuration files when you ask it to, and listens
on one named pipe. This file says what each of those is bounded by, and — more
importantly — what it is **not** bounded by.

## Reporting a vulnerability

Use this repository's **Security** tab → **Report a vulnerability**. That opens a
private GitHub security advisory, visible only to you and the maintainers. Please
do not open a public issue for something you believe is exploitable.

If you are not sure whether what you found is in scope, report it privately
anyway; deciding that is our job, not yours.

## The threat model, in one paragraph

Folio defends the boundary between **your logon session and everything outside
it**: other users on the machine, other logon sessions of the same user, the
network, and content that arrives from outside — a page loaded in the preview, a
byte stream printed by a program into a pane.

Folio does **not** defend against a hostile process already running as you in your
own session. Such a process can read your files, read Folio's memory, set the
environment Folio starts with, and write the same configuration files Folio
writes. Nothing below claims otherwise, and where a mechanism might read as if it
did, it is called out.

## The attention pipe

`crates/bt-platform/src/attention_pipe.rs`. One named pipe per Folio process; the
`folio attention` verb is the other end. A program running inside a pane uses it
to raise or lower that pane's attention mark.

- **The pipe carries a security descriptor Folio writes itself**, never the
  default. The default descriptor on an unnamed-descriptor pipe lets Everyone —
  anonymous logons included — open it.
- The descriptor is `D:P(A;;GA;;;<logon SID>)`: a **protected** DACL with exactly
  one entry. Protected, because inheritance is how a permissive entry arrives
  without anyone writing one. One entry, because the only principal in the design
  is the logon session — `Everyone`, `ANONYMOUS LOGON` and `NETWORK` are excluded
  by not being named.
- The principal is the **logon session**, not the user. A second session of the
  same user — a service, another desktop — is outside it.
- **If the process's token carries no logon SID, the endpoint is not opened at
  all.** It does not fall back to a default descriptor. Fail closed.
- `PIPE_REJECT_REMOTE_CLIENTS`: nothing off this machine can connect.
- `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a squatter that got to the name first cannot
  be mistaken for Folio.
- One client at a time, a bounded message, a bounded rate, a deadline.

**What this does not do: there is no authentication of the producer inside your
logon session.** The DACL answers "which session", not "which program". A caller
names the pane it means by presenting that pane's capability — 128 unpredictable
bits, handed to the pane's child process in its environment — and anything running
as you that can read that environment holds it. Making that untrue would need a
non-transferable inherited handle or a broker with an identity model of its own,
and neither is being promised here.

What is bounded instead is the blast radius. The worst a stolen capability buys is
one pane's attention mark, raised or lowered. There are no pane coordinates in the
message format at all, so a caller cannot name a pane whose capability it does not
have. It cannot type into a pane, open one, or read a transcript.

## The hooks installers

Folio can install a notification hook into three other tools, from a switch in
Settings. Each writes exactly one file:

| Tool | File | Directory from |
| --- | --- | --- |
| Claude Code | `settings.json` | `CLAUDE_CONFIG_DIR`, else `%USERPROFILE%\.claude` |
| Codex | `config.toml` | `CODEX_HOME`, else `%USERPROFILE%\.codex` |
| GitHub Copilot CLI | `hooks\folio.json` | `COPILOT_HOME`, else `%USERPROFILE%\.copilot` |

Copilot's own `settings.json` beside that directory is **read** — to see whether
you have switched all hooks off, so the settings row can say so rather than
letting you discover it by watching nothing happen — and never written.

- **The upstream environment variables are honoured, and that means the write can
  be redirected.** `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `COPILOT_HOME` are the
  variables those tools themselves read; Folio asks the same question they do
  rather than composing a path from a user name, because a machine with a
  redirected home is a machine where a composed path is wrong. Anyone who can set
  the environment Folio starts with can therefore choose the directory. That is
  disclosed here and in the settings row, and it is not a privilege boundary: the
  same actor could write the file directly.
- **Nothing is ever discovered or written in a working directory or a
  repository.** This is upstream's own security note: in a non-interactive
  session, hooks committed in a repository's `.claude/settings.json` run in a
  folder you never trusted. Folio writes user-level files only.
- **A file Folio cannot parse is never written over.** An unreadable
  `settings.json`, an unparseable `config.toml`, a `folio.json` holding somebody
  else's entry, or a Codex `notify` program of your own: each is a refusal with a
  reason, and the file is left exactly as it was.
- **A copy is kept before the first write of each day**, named
  `<file>.bak-<YYYYMMDD>`. Only the first copy of a day is kept, because what is
  worth having is the file as it stood before Folio touched it. **If the copy
  cannot be written, the install is refused and nothing is written.**
- **The write is atomic**: a temporary file in the target's own directory,
  `write_all`, `sync_all`, then a single-operation replace. A process killed
  mid-write cannot leave your configuration half a document.
- **Uninstalling gives the file back.** Folio's entries are removed and yours are
  left; for the Copilot hook file, which Folio created whole, the file is removed
  and the directory is left alone.
- **A link is followed.** If the target path is a symlink or a junction, it is
  resolved and the file it names is what gets replaced, with the copy landing
  beside that real file. This is what `std::fs::write` did before the write became
  atomic, and it is what a `~/.claude` kept in a dotfiles repository needs — an
  atomic replace that did not resolve the link would leave a regular file where
  you had a link. Refusing to install through a link would break that setup while
  stopping nothing, for the reason given above: whoever could plant the link
  already runs as you. When a link changes the destination, the resolution is
  written to the attention trace.

## The web preview

`crates/bt-platform/src/webview.rs`, `crates/bt-app/src/webhost.rs`. Folio hosts
Microsoft Edge WebView2 to show a page beside a terminal.

**Every navigation goes through one gate**, in `crates/bt-app/src/webnav.rs`. An
address typed into the address bar, restored from a session, loaded from
`pins.json`, chosen from the switcher, or arriving from the page itself as a link,
a redirect or a script all reach the same rule. `http` and `https` pass;
`javascript`, `data`, `blob` and `vbscript` are refused; `file` is refused unless
the seat was minted for exactly that file; `view-source`, `devtools`, `edge`,
`chrome`, `chrome-extension`, `about` and `ms-browser-extension` are refused; any
other scheme — that is, anything the shell would launch a program for — is refused
outright rather than confirmed. Addresses with userinfo (`user@host`) are refused,
as are control characters and addresses with no host.

**Four more surfaces refuse before anything can happen**, each registered on the
WebView2 instance itself before the first navigation, so a failure to register is a
failure to open the preview rather than a page loaded without them:

- a window the page tries to open is handled and not opened;
- a download is cancelled — the address is then handed to the machine's browser or
  refused visibly, which is Folio's decision on its own turn, not the engine's;
- a permission request is answered `DENY`;
- a launch of an external URI scheme is cancelled.

**Two engine settings are switched off rather than inherited**:
`IsGeneralAutofillEnabled` and `IsPasswordAutosaveEnabled`. WebView2 defaults
general autofill on, and Folio's WebView2 profile is persistent — it lives in
`%LOCALAPPDATA%\Folio\WebView2` and outlives the window and the session. A default
left alone would file names, addresses, phone numbers and card details typed into a
previewed page onto disk, and Folio has no verb for reviewing or clearing that.
Password autosave's default has moved between runtime versions, so it is set
explicitly rather than inherited. Also off: web messages, host objects and the
status bar. Developer tools are on, because the head carries a verb for them.

All of that is applied in the same step that installs the four refusal surfaces,
before anything navigates, and every call in it propagates its failure — including
the cast to the settings interface the two autofill switches live on. A runtime
too old to answer it is a preview that does not open, not a preview that opens
with a form-filling profile nobody asked for.

**Subframes are not run through the navigation gate, and this is deliberate.**
Folio attaches `NavigationStarting`, which fires for the top-level document only.
It does not attach `FrameNavigationStarting` or `WebResourceRequested`.

The reason is that the gate is a rule about **what this seat was asked to show**,
and a subframe is part of how a page is composed rather than a place the seat was
sent. An `<iframe>` with no `src` starts at `about:blank`; a local page opened from
the files column may frame a sibling file; `data:` and `blob:` frames are ordinary
parts of viewers. Every one of those is refused by the top-level rule, so running
subframes through it would not tighten a boundary — it would refuse the page's own
structure and call it security.

What actually bounds what a subframe can do to your machine is the four refusal
surfaces above, and every one of them is registered on the WebView2 instance rather
than on a frame: a subframe cannot open a window, start a download, obtain a
permission, or launch an external scheme. Beyond that, a subframe is subject to the
same origin policy the engine enforces for any browser.

The boundary this leaves open, stated plainly: **a page Folio shows you can load
subresources and subframes from any `http`/`https` origin it likes, and Folio does
not see or filter those requests.** That is the behaviour of a web view; it is not
a proxy or a content blocker. If that matters for what you are previewing, do not
preview it.

## Diagnostics

- `%APPDATA%\Folio\diagnostics.log` receives `stdout` and `stderr` for a run that
  did not keep its console. **The size is checked once, at startup**: if the file
  is already 4 MiB or larger it is moved to `diagnostics.prev.log`, replacing the
  previous generation. There is no throttling during a run, so a single run's log
  can grow past 4 MiB; what is bounded is the history kept between runs, which is
  two files.
- `%APPDATA%\Folio\hang-reports\hang-<timestamp>.txt` is written when the window
  thread stops answering. It records the instruction and stack pointers, how many
  bytes of stack were scanned, how many modules were mapped, and each candidate
  return address **as a module name and an offset**. The stack bytes themselves are
  read but not written to the file. Addresses and run state are still in there, so
  read a report before you attach it to anything.
- `%TEMP%\bt-app-panic.log` is appended to by the panic hook.

## Environment variables

Folio reads no environment variable of its own unless you set one. Several
`BT_*` switches make it write terminal content — up to and including every byte of
every pane — to a path you name. They grant no privilege: setting one requires
already being able to run programs as you. **`docs/BT-ENVIRONMENT.md` lists every
one of them**, what it writes, and what can end up in the file. That list is
checked against the source by a test.

## What Folio does not do

- No telemetry, no analytics, no crash reporting to anyone.
- No network client of its own. The only thing that reaches the network is a page
  you asked the preview to open, fetched by WebView2.
- No update check.
- Nothing is read from, or written to, a working directory or a repository as
  configuration.
