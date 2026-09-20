# Terminal coding agents: a survey for Folio 0.5 adapter priority

Compiled 2026-09-20. Every cell comes from a primary source — vendor docs, the project's
own source or release notes, the GitHub / npm / PyPI APIs — with URLs in §9.
**Anything not confirmed from a primary source is written `unverified`, never guessed.**

This supersedes the popularity and mechanism columns of
`docs/plans/attention/evidence-cli-survey-2026-08-25.md`, which surveyed eight candidates
chosen by recall. That file's *measured* findings (the four Codex recordings, the Claude
Code recordings) still rule and are cited where they do. **§8 lists five places where the
August file is now wrong**; two of them change the 0.5 priority.

---

## 1. Popularity, measured

Stars from `api.github.com/repos/*`, downloads from `api.npmjs.org` and `pypistats.org`,
all read 2026-09-20. npm figures are the week 2026-09-13 – 2026-09-19.

| Agent | Repo | Stars | npm package | npm/wk | PyPI/wk | China vs global |
|---|---|---|---|---|---|---|
| **OpenAI Codex CLI** | `openai/codex` · Apache-2.0 | 125,493 | `@openai/codex` | **15,573,038** | — | global |
| **Claude Code** | `anthropics/claude-code` | 146,988 | `@anthropic-ai/claude-code` | **9,435,666** | — | global — **and the de-facto client for the GLM, DeepSeek and Kimi subscriptions sold in China (§6)** |
| *OpenClaw* — not a coding agent (§7) | `openclaw/openclaw` | **390,144** | `openclaw` | 2,982,033 | — | global |
| **OpenCode** | `anomalyco/opencode` · MIT *(moved from `sst/`)* | **208,823** | `opencode-ai` | 1,823,402 | — | global |
| **pi** | `earendil-works/pi` · MIT | **107,665** | `@earendil-works/pi-coding-agent` | 1,721,146 | — | global |
| **GitHub Copilot CLI** | `github/copilot-cli` | 11,188 | `@github/copilot` | **1,078,216** | — | global |
| **Gemini CLI** | `google-gemini/gemini-cli` · Apache-2.0 | 107,096 | `@google/gemini-cli` | 287,529 | — | global |
| *Hermes Agent* — not a coding agent (§7) | `NousResearch/hermes-agent` · MIT | **247,399** | — | — | `hermes-agent` 35,113 | global |
| **Grok Build** (xAI) | closed | — | `@xai-official/grok` | 55,501 | — | global |
| **Qwen Code** | `QwenLM/qwen-code` · Apache-2.0 | 28,013 | `@qwen-code/qwen-code` | 53,199 | — | **China-centric** |
| **Cline CLI** | `cline/cline` · Apache-2.0 | 68,867 | `cline` | 50,091 | — | global |
| **CodeBuddy Code** (Tencent) | not on GitHub (`cnb.cool`) | n/a | `@tencent-ai/codebuddy-code` | **43,352** | — | **China** |
| **Amp** | closed *(left Sourcegraph 2025-12)* | n/a | `@ampcode/cli` 24,663 + legacy `@sourcegraph/amp` 14,217 | **≈38,880** | — | global |
| **Kimi Code** | `MoonshotAI/kimi-code` · MIT | 7,534 | `@moonshot-ai/kimi-code` | 26,743 | — | **China-centric** |
| **Auggie** (Augment) | closed | n/a | `@augmentcode/auggie` | 23,030 | — | global |
| **Kilo** | — | — | `@kilocode/cli` | 21,001 | — | global |
| **Aider** | `Aider-AI/aider` · Apache-2.0 | 49,081 | — | — | `aider-chat` 62,447 | global — **no push since 2026-05-22** |
| **Kimi CLI** | `MoonshotAI/kimi-cli` · Apache-2.0 | 11,411 | — | — | `kimi-cli` 9,652 | **China-centric** |
| **Droid** (Factory) | `Factory-AI/factory` is **docs-only**, 24★ | n/a | `droid` | 7,351 | — | global, enterprise |
| **Qoder CLI** (Alibaba) | closed | n/a | `@qoder-ai/qodercli` | 7,163 | — | **China** |
| **MiniMax Code** | closed | n/a | `@minimax-ai/code` | 7,048 | — | **China** |
| **Crush** | `charmbracelet/crush` | 28,203 | `@charmland/crush` | 6,034 | — | global |
| **Goose** | `aaif-goose/goose` · Apache-2.0 *(moved from `block/`)* | 54,497 | — (curl installer) | — | — | global |
| **Continue CLI** | — | — | `@continuedev/cli` | 3,221 | — | global |
| **Cursor CLI** | closed, curl-only | n/a | — | — | — | global |
| **iFlow CLI** | `iflow-ai/iflow-cli` — last push **2026-03-20** | 5,092 | `@iflow-ai/iflow-cli` | **213** | — | **China only — shutdown announced for 2026-04-17, users migrated to Qoder** |
| **Trae / trae-agent** (ByteDance) | `bytedance/trae-agent` · MIT — last push **2026-02-05**, **zero releases ever** | 12,108 | **no npm or PyPI package exists** | — | — | China; dormant |

ACP adapter packages, for scale (§5): `@agentclientprotocol/claude-agent-acp`
**1,500,582/wk**, `@agentclientprotocol/codex-acp` **1,390,905/wk**,
`@agentclientprotocol/sdk` **6,047,359/wk**, `pi-acp` 32,230/wk.

**What the numbers say.** Two agents are an order of magnitude ahead — Codex 15.6M and
Claude Code 9.4M weekly npm installs. A second tier sits at 1–3M (OpenCode, pi, Copilot
CLI). Everything else is ≤ 300k.

Two results contradict the working assumptions behind today's adapter list:

- **GitHub Copilot CLI is not a minority agent.** At 1.08M npm/wk it is the third-largest
  real coding agent by installs, roughly **3.7× Gemini CLI**. Whatever prompted the
  suspicion, the download data does not support dropping it.
- **iFlow CLI is the one clear trap.** 213 downloads/week, no push since March, and its
  own forum announced a 2026-04-17 shutdown with users migrated to Alibaba's Qoder.

---

## 2. Process identity, for the detection floor

| Agent | Executable | Windows process | macOS process | Distribution |
|---|---|---|---|---|
| Claude Code | `claude` | `claude.exe` | `claude` | npm `@anthropic-ai/claude-code`; native installer |
| Codex CLI | `codex` | `codex.exe` (Rust) | `codex` | npm `@openai/codex` → standalone binary |
| **Gemini CLI** | `gemini` | **`node.exe`** (`bundle/gemini.js`, Node ≥20) | **`node`** | npm; brew/MacPorts wrap the tarball; a self-contained zip exists for **macOS only** |
| **Qwen Code** | `qwen` | **`node.exe`** — even the "standalone" installer bundles `node.exe` + a `.cmd` shim | **`node`** | npm `@qwen-code/qwen-code`; brew |
| **CodeBuddy Code** | `codebuddy-code` / `codebuddy` / `cbc` | **`node.exe`** (16 MB `dist/codebuddy.js`) + genuine native win32 N-API addons | `node` | npm `@tencent-ai/codebuddy-code`; a native-binary installer is in beta |
| Copilot CLI | `copilot` | `copilot.exe` | `copilot` | npm `@github/copilot` |
| Cursor CLI | **`agent`** (some material says `cursor-agent`) | native binary | native binary | `curl https://cursor.com/install \| bash`; Windows `irm 'https://cursor.com/install?win32=true' \| iex` |
| Amp | `amp` | **Bun-compiled single file** | same | npm `@ampcode/cli`; install.sh |
| Crush | `crush` | `crush.exe` (Go) | `crush` | npm `@charmland/crush`, brew, **winget `charmbracelet.crush`**, scoop, go install |
| Goose | `goose` | `goose.exe` (Rust) | `goose` | `download_cli.ps1` / `.sh`; `brew install block-goose-cli` |
| Droid | `droid` | native | native | `irm https://app.factory.ai/cli/windows \| iex`; npm `droid` |
| Cline CLI | `cline` | `cline.exe` — **native**, per-platform binaries via optional deps, no Node at runtime | `cline` | npm `cline` |
| iFlow CLI | `iflow` | `node.exe` (ink TUI) | `node` | npm `@iflow-ai/iflow-cli` |
| Trae | `trae-cli` | **`python.exe`** | `python3` | **none** — `git clone` + `uv sync` only |
| Kimi CLI *(legacy)* | `kimi` | `python.exe` (Python ≥3.13) | `python3` | PyPI `kimi-cli` |
| **Kimi Code** *(current)* | `kimi` | **`node.exe`** via npm; the official installer ships a **Node SEA** single-file build | `node` / SEA binary | npm `@moonshot-ai/kimi-code`; install script |
| OpenCode | `opencode` | **`opencode.exe`** — a Bun-compiled standalone; the npm `bin` is only a Node shim that spawns it | `opencode` | npm `opencode-ai`; brew/scoop/choco/pacman/nix/Docker |
| pi | `pi` | **`node.exe`** (`dist/bundle/cli.js`, Node ≥22.19) — Bun is build-time only | `node` | npm `@earendil-works/pi-coding-agent`; **`pi.dev/install.sh` is Unix-only, Windows must use npm** |
| Aider | `aider` | `python.exe` | `python3` | PyPI `aider-chat` (no npm — npm's `aider-chat` 404s) |
| Hermes | `hermes` | **`python.exe`** (`hermes_cli.main`); the Windows service variant uses `pythonw.exe` | `python3` | install.sh / install.ps1 + a desktop installer. **PyPI, Homebrew and AUR are documented as "Unsupported"**; the npm `hermes-agent` is an unofficial third-party wrapper |
| OpenClaw | `openclaw` | `node.exe` (`openclaw.mjs`, Node ≥24.16) | `node` | npm `openclaw`; install.ps1; a native WinUI "Windows Hub" app |

> **Five of the highest-value agents are invisible to process-name detection** — Gemini,
> Qwen, CodeBuddy, iFlow and OpenClaw all show as `node.exe`; Kimi, Aider and Trae show as
> `python.exe`. This independently vindicates §11.8's decision to drop the
> process-information recognition rung. The column is here for the **mark**, not for
> recognition.

---

## 3. The event surface an external program can subscribe to

`✔` documented and usable · `—` looked for, not found · `?` unverified.

| Agent | Hooks: file, format | Turn end | Waiting-for-permission | Error | Session start/end | Subagent | Blocking? | Exec form |
|---|---|---|---|---|---|---|---|---|
| **Claude Code** | `~/.claude/settings.json` → `hooks{}`, JSON | `Stop` | **`PermissionRequest`**; `Elicitation` (MCP input) | `StopFailure`, `PostToolUseFailure` | `SessionStart` / `Setup` / `SessionEnd` | `SubagentStart` / `SubagentStop` | **`async: true` available** | **direct exec** — `command` + `args[]` |
| **Codex CLI** | `~/.codex/hooks.json`, or `[hooks]` in `config.toml` | `Stop` | **`PermissionRequest`** | `Interrupt` | `SessionStart` / `SessionEnd` | `SubagentStart` / `SubagentStop` | permission hook is a **sync decision gate** | array |
| **Codex `notify`** | `notify = [...]` in `config.toml` | `agent-turn-complete` **only** | **no** | — | — | — | fire-and-forget | direct exec, JSON appended to argv |
| **Gemini CLI** | `~/.gemini/settings.json` → `hooks{}`, JSON | **`AfterAgent`** | **`Notification`, `notification_type:"ToolPermission"`** | — | `SessionStart` / `SessionEnd` | **none** | **hooks run SYNCHRONOUSLY in the agent loop**; `SessionEnd` best-effort; `Notification` advisory | **shell string** (`command`) — no `args[]` |
| **Qwen Code** | `~/.qwen/settings.json` → `hooks{}`, JSON, + `disableAllHooks` | `Stop`, `StopFailure` | **`PermissionRequest`, `PermissionDenied`, `Notification`, `Elicitation`** | `StopFailure`, `PostToolUseFailure` | `SessionStart` / `SessionEnd` / `SessionDelete` | `SubagentStart` / `SubagentStop` | ? | command / http / function / prompt |
| **CodeBuddy Code** | `~/.codebuddy/settings.json` → `hooks{}`, JSON | `Stop` | **`PermissionRequest`, `PermissionDenied`, `Elicitation`, `Notification`** | `PostToolUseFailure` | `SessionStart` / `SessionEnd` | `SubagentStart` / `SubagentStop` | ? | `{type:"command", command, timeout}` |
| **Copilot CLI** | `~/.copilot/hooks/*.json` (Folio writes `folio.json`), JSON `version:1` | `agentStop` | **`notification` → `permission_prompt`, `elicitation_dialog`** | `errorOccurred`, `postToolUseFailure` | `sessionStart` / `sessionEnd` | `subagentStart` / `subagentStop` | **`notification` is fire-and-forget, never blocks** | `exec` + `args[]` |
| **Cursor CLI** | `~/.cursor/hooks.json`, JSON `version:1` | **`stop`** | `beforeShellExecution` — a **blocking gate**, not a notification | `postToolUseFailure` | `sessionStart` / `sessionEnd` | `subagentStart` / `subagentStop` | permission hooks sync; session hooks fire-and-forget | `command` (shell string) |
| **Droid** | `~/.factory/hooks.json`, JSON | `Stop` | **`Notification` → `permission_prompt`, `idle_prompt`, `elicitation_dialog`** | — | `SessionStart` / `SessionEnd` | `SubagentStop` | **synchronous, blocks to completion or a 60 s timeout** | stdin JSON |
| **Goose** | `~/.agents/plugins/<name>/hooks/hooks.json`, JSON | `Stop` | — (`BeforeShellExecution` is a gate) | `PostToolUseFailure` | `SessionStart` / `SessionEnd` | — | ? | ? |
| **Kimi Code** *(current)* | `~/.kimi-code/config.toml` `[[hooks]]`, **20 events** | `Stop` | **`PermissionRequest`, `Notification`** | ? | `SessionStart` / `SessionEnd`, + `TurnStarted`, `SessionHeartbeat` | ✔ | ? | **shell string** |
| **Kimi CLI** *(legacy)* | `~/.kimi/config.toml` `[[hooks]]`, 13 events | `Stop`, `StopFailure` | — (carried by `Notification`) | `PostToolUseFailure` | `SessionStart` / `SessionEnd` | `SubagentStart` / `SubagentStop` | exit 2 blocks | `command` |
| **iFlow CLI** | `~/.iflow/settings.json` → `hooks{}`, 9 events | `Stop` | — (`Notification` only) | — | `SessionStart` / `SessionEnd` | `SubagentStop` | ? | command; **payload via env vars**, not stdin |
| **Cline CLI** | executables named for the event in `~/Documents/Cline/Rules/Hooks/` (the CLI README says `~/.cline/hooks` — **conflicting**) | `TaskCancel` / `TaskResume` | — | — | `TaskStart` | — | ? | stdin JSON |
| **Crush** | `crush.json` or `~/.config/crush/crush.json` | — | **`PreToolUse` — the only hook shipped** | — | — | — | sync gate (exit 2 block, exit 49 halt) | stdin JSON |
| **OpenCode** | **in-process JS plugin**: `opencode.json` `plugin[]`, `Hooks.event` / `api.event.on(...)` | **`session.idle`**, `session.status` | **`permission.ask` / `permission.asked` / `permission.replied`, `question.asked`** | `session.error` | ? | ✔ | in-process | **JS module, not an external process** |
| **pi** | **in-process TS extension**: `~/.pi/agent/extensions/*.ts`, `pi.on(...)` | **`agent_settled`** | — | — | — | — | in-process | **TS module. No settings-level notify key at all** — notification is purely something an extension author writes |
| **Amp** | **in-process TS Plugin API**: `session.start`, `agent.start`, `agent.end`, `tool.call`, `tool.result`; thread state `idle\|running\|awaiting-approval\|error` | `agent.end` | `awaiting-approval` | `error` | `session.start` | — | in-process | **TS module** |
| **Aider** | — | **`--notifications-command "<cmd>"`** | — | — | — | — | **SYNCHRONOUS** — `subprocess.run(..., shell=True)` | shell string, **no structured payload**: the message text is baked into the generated command |
| **Hermes** | **three tiers**: 27-event Python plugin hooks (`~/.hermes/plugins/`); no-code Gateway hooks (`~/.hermes/hooks/<name>/HOOK.yaml` + `handler.py`, events `session:start\|end\|reset`, `agent:start\|step\|end`); **pure-shell hooks in `config.yaml`** | `agent:end`, `on_session_end` | — | — | `session:start` / `session:end`, `on_session_start` | — | ? | shell |
| **Trae** | **none** — no hooks, no notify, no OSC | — | — | — | — | — | — | only a continuously-written trajectory JSON (`--trajectory-file`) |

### 3b. Escape sequences these agents actually write

| Agent | BEL | OSC 9 | OSC 777 | OSC 99 | **OSC 9;4** | OSC 1337 | OSC 133 | Title (OSC 0/2) carries state? |
|---|---|---|---|---|---|---|---|---|
| Claude Code | ✔ behind a **focus gate** | — | — | — | — | hooks' `terminalSequence` allowlist **names OSC 1337 as refused** | — | no — a zero-semantic spinner |
| Codex CLI | ✔ behind a focus gate | ✔ only for recognised terminals | — | — | — | — | — | **YES** — idle `…` / `⠴ …` busy / **`[ . ] Action Required \| …`** |
| **Gemini CLI** | fallback | **preferred** | selectable | — | — | — | — | **YES** — exact strings `◇ Ready (folder)`, `⏲ Working… (folder)`, **`✋ Action Required (folder)`** |
| **Qwen Code** | `general.terminalBell`, **default `true`** | ✔ iTerm2 | ✔ Ghostty/cmux | ✔ Kitty | **✔** | ✔ (inline Mermaid images) | — | **YES** — `◐` responding, **`✳︎` waiting for confirmation** ("mirroring Claude Code's tab status icons") |
| **CodeBuddy Code** | fallback | ✔ iTerm2 | ✔ Ghostty/**Windows Terminal**/VS Code | ✔ Kitty | **✔** (WT/iTerm2/Ghostty) | — | — | `✳` idle prefix; disable with `CODEBUDDY_CODE_DISABLE_TERMINAL_TITLE` |
| **Crush** | ✔ | ✔ | — | ✔ | — | — | — | **YES** — `⠋` working / `✳` idle / **`✋` blocked on permission** |
| Copilot CLI | `beep` — may be Win32 `MessageBeep`, not a tty byte (§8) | — | — | — | **✔ `terminalProgress`** | — | — | session identity only |
| **pi** | — | ✔ (example extension only) | ✔ (example extension only) | ✔ (example extension only) | **✔ core** — `\x1b]9;4;3\x07` re-sent every 1000 ms, `\x1b]9;4;0` at turn end — **but `showTerminalProgress` defaults to `false`** | — | — | no — app name + session, no busy/idle |
| OpenCode | — | ✔ | ✔ | ✔ (preference order 99 > 777 > 9) | **— (not implemented)** | — | **— (not implemented)** | **no** — route/session name only; `OPENCODE_DISABLE_TERMINAL_TITLE` |
| Aider | last-resort fallback only | — | — | — | — | — | — | — |
| Kimi CLI / Kimi Code | — | **— (none at all)** | — | — | — | — | — | unverified |
| iFlow CLI | — | — | — | — | — | — | — | no — static `iFlow - <workspace>`, `CLI_TITLE` overrides |
| **Hermes** | — | **✔** via `display.bell_on_complete` / `bell_on_prompt` | **✔ inside Warp** — a `warp://cli-agent` event carrying **`stop`** and **`permission_request`** | — | — | — | — | no mechanism found |
| OpenClaw | — | — | — | — | — | — | — | none documented |
| Goose | — (open FR #10280 to *add* OSC 0/2) | — | — | — | — | — | — | no |
| Trae | — | — | — | — | — | — | — | no — only in-TUI widget labels |
| Amp / Cursor | ? | ? | ? | ? | ? | ? | ? | Cursor sets a conversation name; unverified |

### 3c. Which terminal names the vendors' allowlists contain

This decides what Folio must claim in `TERM_PROGRAM`, and it is read from vendor source:

- **Codex** (`codex-rs/terminal-detection`) enables OSC 9 **only** for
  `Ghostty, iTerm2, Kitty, WarpTerminal, WezTerm`, and degrades
  `AppleTerminal, Alacritty, Dumb, GnomeTerminal, Konsole, VsCode, Vte, WindowsTerminal,
  Unknown` to a **bare BEL**. Detection order: `TERM_PROGRAM` → env fallbacks
  (`GHOSTTY_RESOURCES_DIR`, `WEZTERM_VERSION`, `ITERM_SESSION_ID`, `KITTY_WINDOW_ID`,
  `KONSOLE_VERSION`, `GNOME_TERMINAL_SCREEN`, `VTE_VERSION`, **`WT_SESSION`**) → `TERM`.
- **Gemini CLI** (`terminalNotifications.ts`): OSC 9 for iTerm2; **bare BEL** for
  Alacritty, Apple Terminal, VS Code and **anything with `WT_SESSION` set**;
  **OSC 777 for everything else**.
- **OpenTUI / OpenCode**: the selection lives in `packages/native/src/terminal.zig` — *not*
  `ansi.zig`, which only holds the string constants (the August file's file reference was
  wrong; the behaviour it described was right). Protocol chosen by terminal identity with
  preference **OSC 99 > OSC 777 > OSC 9**; two override env vars, both real:
  **`OPENTUI_NOTIFICATION_PROTOCOL`** and **`OPENTUI_NOTIFICATIONS`**.
- **Aider** does not use a terminal allowlist at all: when `--notifications` is enabled it
  picks an **OS notifier** — `terminal-notifier`/`osascript` on macOS,
  `notify-send`/`zenity` on Linux, and **a PowerShell MessageBox popup on Windows** —
  and only falls back to `print("\a")` if none is present. So Folio receives **no byte at
  all** from a notifying Aider on Windows.
- **Crush** fires only when the window is **unfocused and focus reporting is supported**.
- **Copilot CLI's `beep`** is alleged (issues #1458 / #3573 / #3748) to be Win32
  `MessageBeep` rather than a tty byte. **Aider's Windows behaviour above is the same
  failure mode, confirmed from source** — which makes the Copilot allegation considerably
  more plausible and is one more reason the adapter should stay on the hook.

> **Two engineering consequences, both already in the plan and both reconfirmed here.**
> (1) Folio must emit focus reporting (`\e[I` / `\e[O`) — at least four independent agents
> gate their signal on it. (2) Folio must claim a `TERM_PROGRAM` these lists contain
> (`WezTerm`, `ghostty` or `iTerm.app`) and **must not leave `WT_SESSION` set**, or Codex
> and Gemini both silently drop to the least informative signal available.

---

## 4. Context window, quota, config root, Windows

| Agent | Context % readable externally | Quota readable externally | Config-root env var | Windows |
|---|---|---|---|---|
| **Claude Code** | **YES** — `statusLine` stdin JSON: `context_window.used_percentage`, `.current_usage`, `exceeds_200k_tokens` | **YES** — `rate_limits.five_hour.{used_percentage,resets_at}`, `.seven_day.*`, `.spend_limit.*`, plus `cost.total_cost_usd`. Pushed and free | **`CLAUDE_CONFIG_DIR`** — replaces the whole `~/.claude` path | native |
| **Codex CLI** | — | **YES** — app-server `account/rateLimits/read`: `usedPercent`, `windowDurationMins`, `resetsAt`, plus a pushed sparse update | **`CODEX_HOME`** (default `~/.codex`) | native |
| **Qwen Code** | **YES** — `ui.statusLine = {type:"command", command}` stdin JSON: `context_window.{context_window_size, used_percentage, remaining_percentage, current_usage, total_input_tokens, total_output_tokens}`; the `Stop` hook also carries `context_usage`/`context_limit` | `metrics.models.<id>.api.{total_requests,total_errors}` only — no quota window | **`QWEN_HOME`** (default `~/.qwen`), plus `QWEN_RUNTIME_DIR` | native |
| **CodeBuddy Code** | **YES** — `statusLine.command` stdin JSON: `context_window.{used_percentage, remaining_percentage, total_input_tokens, total_output_tokens}`, `cost.total_cost_usd`, `model.{id,display_name}` | `rate_limits` fields **unverified** | **`CODEBUDDY_CONFIG_DIR`** (default `~/.codebuddy`) | native, with real win32 Job-Object addons |
| **Gemini CLI** | **no** — the footer % is hidden by default (`ui.footer.hideContextPercentage` default `true`) and **there is no statusline-command hook**. OTel exports raw token counts, no percentage | — | **`GEMINI_CLI_HOME`** — the *parent* of `.gemini`, not the dir itself | native (Windows 11 24H2+ listed) |
| **Copilot CLI** | — | — | **`COPILOT_HOME`** — replaces all of `~/.copilot`; old XDG paths migrate in | native |
| **Cursor CLI** | a `statusLine` exists in `cli-config.json`; **field names unverified** (reference page 404s) | ? | **`CURSOR_CONFIG_DIR`** (+ `XDG_CONFIG_HOME` on Linux) | native |
| **Kimi Code / Kimi CLI** | — (a statusline-JSON feature request is open and **unshipped**, `kimi-cli#2149`) | **Was** yes on legacy kimi-cli — a local server child, `usedRatio` 0–1 per window (Folio-measured, 0.5 design §7). **On kimi-code the `/coding/v1/usages` route has an open bug returning zeroed ratios (`kimi-code#3908`)** — do not rely on it without re-measuring | **`KIMI_CODE_HOME`** (default `~/.kimi-code`; legacy `~/.kimi`) | native, **but it requires a bundled Git Bash as its shell** (`KIMI_SHELL_PATH` overrides) |
| **Crush** | — | — | **`CRUSH_GLOBAL_CONFIG`** / `CRUSH_GLOBAL_DATA` | native — **winget + scoop** |
| **Goose** | auto-compacts at 80 % (`GOOSE_AUTO_COMPACT_THRESHOLD`, `GOOSE_CONTEXT_LIMIT`); no external field | — | **`GOOSE_PATH_ROOT`** | native recommended |
| **Droid** | `/context` and `/statusline` in-session; no external field verified | — | **none found** | native |
| **Cline CLI** | — (`-v` prints stats, no documented field) | — | **`CLINE_DATA_DIR`** | native — **but the file-hook feature is macOS/Linux only** |
| **Amp** | — | — | `AMP_SETTINGS_FILE` *(moderate confidence)* | **WSL only** — "macOS, Linux, and Windows through WSL" |
| **OpenCode** | SDK exposes per-message `tokens{…}` + `cost` and `Model.limit.context`, but **no ready-made percentage**; and it is an SDK, not a file | only for OpenCode's own hosted provider | **`OPENCODE_CONFIG_DIR`** (+ `OPENCODE_CONFIG`, `OPENCODE_CONFIG_CONTENT`) | **native** (`opencode-windows-x64`); WSL is *recommended*, not required |
| **pi** | **YES, but only in `--mode rpc`** — `get_state` returns `contextUsage:{tokens, contextWindow, percent}`. The interactive TUI a user types into exposes nothing | — | **`PI_CODING_AGENT_DIR`** (default `~/.pi/agent`) | npm only — the official install script has no Windows branch |
| **Hermes** | in-session `context_pct` | **YES — `hermes usage --json`**, a documented stable schema `windows:[{label, used_percent, resets_at}]`, explicitly designed for scripts and cron | **`HERMES_HOME`** (default `~/.hermes`; `%LOCALAPPDATA%\hermes` on Windows) | **tier-1 native** (x86_64 + aarch64); shells through bundled Git Bash "same as Claude Code" per its own docs |
| **OpenClaw** | **YES** — `context.{max_tokens, used_tokens, pct_used}`, `openclaw status --usage`, JSON-RPC `usage.cost` / `sessions.usage` | same surface | **`OPENCLAW_STATE_DIR`** (+ `OPENCLAW_HOME`, `OPENCLAW_CONFIG_PATH`) | native CLI + a signed native WinUI app |
| **iFlow CLI** | in-TUI only, undocumented format | — | **no home override** — only `IFLOW_CLI_SYSTEM_SETTINGS_PATH` | effectively native |
| **Aider** | **no machine-readable field** — token and cost are printed as terminal text; the nearest thing is opt-in `--analytics-log FILE` JSONL, not built for live polling | — | **none** — `.aider.conf.yml` is searched home → git root → cwd, with only a `--config <file>` override | native, with documented PATH friction |
| **Trae** | raw token counts in the trajectory JSON only | — | `TRAE_CONFIG_FILE` (a file, not a root) | **no official statement either way** |

**Quoting risk.** Every agent whose hook form is a **shell string** (Gemini, Cursor,
Aider) is a hazard for an installer writing a Windows path with spaces. Every agent with a
**direct-exec** form (Claude `command`+`args[]`, Copilot `exec`+`args[]`, Codex array,
CodeBuddy `{type:"command"}`) is safe. Folio already refuses shell templates it did not
write (`docs/agent-integration-marks.md`); **Gemini would be the first adapter with no
safe form available** — the installer must quote, and the ownership decoder must learn a
shell-string shape it cannot re-parse as reliably.

**Uninstall risk.** Droid has **no config-root env var at all**, so a Folio install there
cannot be discovered from a moved home. Under the standing rule that any "before
uninstalling, please first do X by hand" is a defect, that is a real reason to rank Droid
below agents that have one.

---

## 5. Agent Client Protocol — the one mechanism that could replace many adapters

Repo `agentclientprotocol/agent-client-protocol`, Apache-2.0, **4,294 stars**; Rust schema
crate at **1.9.1**. TypeScript SDK `@agentclientprotocol/sdk` — **6,047,359 npm
downloads/week**.

**Versions, stated precisely.** The **shipping wire version is 1**. A `v2` schema exists in
the same repo but its own Cargo manifest says: *"Protocol v2 is intentionally NOT part of
the `unstable` umbrella. It introduces a parallel `v2` module with a different wire
version, so it must be opted into explicitly"* (`unstable_protocol_v2`). Both matter to
Folio, and they differ in exactly the field Folio cares most about:

| Folio state | ACP **v1** (shipping) | ACP **v2** (opt-in) |
|---|---|---|
| Working | inferred — streamed `agent_message_chunk`, `tool_call` with `ToolCallStatus pending\|in_progress\|completed\|failed` | **explicit** — `session/update` → `state_update`, `state:"running"` |
| **Waiting** | **`session/request_permission`** (client method, with `title`, `description`, `options[]`); `elicitation/create` for typed input | same, **plus** `state:"requires_action"` — "foreground work is blocked on user action" |
| Done / Failed | the `session/prompt` **response's `stopReason`** | `state:"idle"` with an optional `stopReason` |
| **Context %** | **`session/update` → `usage_update` → `UsageUpdate {used, size, cost?}`** — present in **v1** | same |
| Todo / plan | `session/update` → `plan` (`PlanEntry {content, priority, status}`) | `plan_update` |
| **Quota** | **absent from the schema** | absent |

`StopReason` (identical in both): `end_turn` · `max_tokens` · `max_turn_requests` ·
`refusal` · `cancelled`. **There is no error stop reason** — a hard failure is a JSON-RPC
error response, so Folio's `Failed` would come from the transport, not the enum.

**Who implements the agent side.** The official registry (`agentclientprotocol/registry`,
398★; index at `cdn.agentclientprotocol.com/registry/v1/latest/registry.json`) lists **41
agents, each CI-verified to return valid `authMethods` in the handshake**:

> agoragentic · **amp** · antigravity (Google) · **auggie** · autohand · **claude** ·
> **cline** · **codebuddy-code (Tencent)** · **codex** · cortex-code · corust · crow-cli ·
> **cursor** · deepagents · **devin** · dimcode · dirac · **factory-droid** · fast-agent ·
> **gemini** · **github-copilot-cli** · glm-acp-agent · **goose** · **grok-build (xAI)** ·
> harn · junie (JetBrains) · kilo · kimchi · **kimi** · **minimax-code** · minion-code ·
> mistral-vibe · nova · **opencode** · **pi** · poolside · qoder (Alibaba) ·
> **qwen-code** · sigit · stakpak · vtcode

Native (the vendor's own binary has an ACP mode): Gemini CLI `--acp` /
`--experimental-acp`, Copilot CLI, Cursor `agent acp`, Droid
`droid exec --output-format acp`, Goose `goose acp`, Cline `--acp`, Qwen Code
(`--acp` plus a `@qwen-code/acp-bridge` package and `qwen serve`), CodeBuddy `--acp`,
**OpenCode `opencode acp`** (pins `@agentclientprotocol/sdk` in `package.json`),
**Kimi Code `kimi acp`**, **Hermes `hermes acp`**, **OpenClaw `openclaw acp`**.
Via an official wrapper that spawns the real agent underneath: Claude Code
(`@agentclientprotocol/claude-agent-acp`, 1.5M/wk — wraps the **Agent SDK**, not the
interactive TUI) and Codex (`@agentclientprotocol/codex-acp`, 1.39M/wk — starts a Codex
App Server subprocess). **Community bridges only — the registry entry is not the vendor's:**
Amp (`amp-acp`), **pi (`pi-acp`; the project's own ACP discussion #4444 is still open and
unresolved)**, Crush (unverified), and **`glm-acp-agent`, which is an individual's wrapper,
not a Zhipu product** (§6). *A registry listing therefore proves ACP reachability, not
first-party support — the distinction matters if Folio ever depends on one.*

**The registry also ships an official `icon.svg` per agent**, served at
`cdn.agentclientprotocol.com/registry/v1/latest/<id>.svg` and referenced by an `icon`
field in the index; every entry also carries `license` and `license_url`. **This is the
cleanest sanctioned, versioned, machine-readable source of official marks that exists**,
and it directly answers the problem §7 of the 0.5 design raises about showing official
marks for agents Folio does not ship.

### Why ACP nevertheless cannot replace Folio's adapters in 0.5

ACP is **client-spawns-agent** over JSON-RPC on stdio. The architecture document says it
outright: *"the editor boots the agent sub-process on demand, and all communication
happens over stdin/stdout."* It is the protocol for *being* the UI, not for *watching*
one. A Folio pane in which the user typed `claude` is running the vendor's interactive
TUI; that process has no ACP endpoint, and nothing in either method list lets a third
party attach to a session it did not create — `session/list` and `session/resume` operate
on the connection you already hold.

**One documented exception, and it does not change the conclusion.** Goose ships
`goose serve` — an ACP server over **HTTP and WebSocket** (`--host 127.0.0.1`,
`--port 3284`, endpoint `/acp`, requiring `GOOSE_SERVER__SECRET_KEY` unless
`--dangerously-unauthenticated`), used by goose Desktop and by remote Desktop instances
with TLS and certificate pinning. So the transport is **not** uniformly stdio across the
ecosystem — but it is still a server the user deliberately starts, behind a shared secret,
not a side channel onto a TUI someone is already typing into. No equivalent exists for
Claude Code, Codex or Gemini CLI.

So adopting ACP means Folio **stops being a terminal for that pane and becomes an agent
client**, rendering conversation, permission dialogs and diffs itself.

---

## 6. A2A, and the Chinese vendor paths

**A2A** (`a2aproject/A2A`, 25,868★) is remote, server-to-server agent interop: agents
publish an `AgentCard` at `/.well-known/agent-card.json` and exchange async `Task` objects
(`submitted, working, completed, failed, canceled, input-required, rejected, auth-required`)
over JSON-RPC / gRPC / HTTP+JSON with webhook push. There is **no locally-spawned
subprocess, no TTY, and no token-usage or context-window field**. No mainstream coding CLI
ships agent-side A2A; what exists are third-party bridges that wrap an already-ACP-capable
agent to expose it remotely — the reverse of what Folio needs. *(One exception found:
CodeBuddy Code ships an apparently undocumented `--a2a` flag accepting A2A JSON-RPC over
stdio.)* **Verdict: not a transport for Folio.** Its only value is the one the 0.5 design
already takes in §3.1 — borrowing its `TaskState` vocabulary so Folio's own CLI/MCP
surface speaks something a consumer may already know.

**Zhipu / GLM — ships no terminal agent CLI.** The official path is literally Claude Code:
`npm install -g @anthropic-ai/claude-code`, then `ANTHROPIC_BASE_URL` set to
`https://open.bigmodel.cn/api/anthropic` (mainland) or `https://api.z.ai/api/anthropic`
(international), plus `ANTHROPIC_AUTH_TOKEN`; a helper `npx @z_ai/coding-helper` writes
these into `~/.claude/settings.json`. Zhipu's only first-party product beyond that is
**ZCode**, a macOS **GUI** desktop app, not a CLI. **`glm-acp-agent`, which appears in the
ACP registry, is not Zhipu's** — the npm maintainer and GitHub owner is an individual
(`stefandevo`), and it is a community wrapper over GLM's OpenAI-compatible endpoint, the
same category as the community Amp bridge.

**DeepSeek — same shape, no CLI.** The documented path is
`ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic` plus a token, on the standard
`@anthropic-ai/claude-code` binary, with Claude model names transparently remapped.

**Consequence for adapters.** In both cases the running process is the literal
`@anthropic-ai/claude-code` binary reading the normal `~/.claude/settings.json`, and
`ANTHROPIC_BASE_URL` is an ordinary environment override layered onto the same settings
hierarchy. Hooks and `statusLine` are provider-agnostic settings-file mechanisms.
**Folio's existing Claude Code adapter covers GLM and DeepSeek with zero vendor-specific
code.** *(Caveat, stated honestly: no Anthropic sentence explicitly guarantees that a
custom base URL never affects hooks or statusLine; this is an absence-of-contradiction
inference from the docs' structure, not a quoted promise.)*

**Quota for these two.** DeepSeek documents `GET https://api.deepseek.com/user/balance` →
`{is_available, balance_infos:[{currency,total_balance,granted_balance,topped_up_balance}]}`
— usable, but only with a key the user deliberately pastes, which is the 0.5 design's
existing opt-in ruling. **Zhipu: no public balance or quota REST endpoint found** after
targeted searching of `docs.bigmodel.cn` and `docs.z.ai`; the 0.5 design's ruling that the
undocumented cookie-oriented route **must not be used** stands unchallenged.

---

## 7. Things that look like candidates but are not

- **OpenClaw** — 390k★, 2.98M npm/wk, and *not a coding agent*. Its own README calls it
  "an open-source AI assistant that runs on your own computer and meets you in the
  channels you already use" — a local gateway daemon bridging Discord, Slack, Teams,
  Telegram, WhatsApp, iMessage and 20+ others, with a Control UI, a CLI and a TUI. It does
  not own a pane the way `claude` does. Worth a mark if its TUI appears in a pane; not an
  adapter. *(Unrelated but worth knowing: Cline 2.3.0 shipped a supply-chain compromise
  whose payload installed something named "openclaw", so the word will surface in security
  contexts.)*
- **Hermes Agent** — 247k★, `hermes`, Nous Research, MIT. A general self-hosted agent with
  a TUI and a messaging gateway that *delegates coding to Codex* rather than being a coding
  agent itself. **But it is not the thin target the first pass suggested**: it has three
  separate hook tiers (27-event Python plugin hooks, no-code Gateway hooks, and pure-shell
  hooks in `config.yaml`), `HERMES_HOME`, tier-1 native Windows, ACP **and** a full A2A
  v1.0 plugin, and — uniquely outside Anthropic — **a documented quota command built for
  scripts: `hermes usage --json` → `windows:[{label, used_percent, resets_at}]`**. If
  Folio ever wants a *non-coding* agent in the rail, this is the one with the richest
  surface. It is still out of 0.5 scope because it is not what a coding pane runs.
- **`gh copilot`** (the old extension) has no session loop; only the standalone
  `@github/copilot` matters.
- **`hermes-cli` on npm** is a Brazilian travel-agency CLI. **`goose-ai` on PyPI** is an
  impostor package whose install prints an unrelated string. Neither is the agent of that
  name — do not use either as a detection or install signal.
- **iFlow CLI** — 213 npm/wk, no push since 2026-03-20, **shutdown announced for
  2026-04-17** with users migrated to Qoder. Genuinely a Gemini CLI fork (retained
  `Copyright 2025 Google LLC` headers, leftover `goo.gle/set-up-gemini-code-assist` URLs,
  an installer that runs `--install-extension google.gemini-cli-vscode-ide-companion`),
  but dying.
- **Trae / `bytedance/trae-agent`** — last push 2026-02-05, **zero releases ever**, and
  **no npm or PyPI package exists** (both registries 404; corroborated by its own issue #86
  "trae-cli command not found"). No hooks, no OSC, no notify, no ACP. Ignore.

---

## 8. Corrections to the 2026-08-25 evidence file

1. **Gemini CLI now has hooks — the "hardest to adapt" verdict is void.** August concluded
   「没有 hooks/事件回调面 — 这是六家里适配最难的一家」. Today `docs/hooks/` documents eleven
   events including **`AfterAgent`** (turn end) and **`Notification` with
   `notification_type:"ToolPermission"`** (waiting for permission), configured under
   `hooks{}` in `~/.gemini/settings.json`, with `session_id`, `transcript_path` and `cwd`
   in every payload. Two real caveats survive: hooks *"run synchronously as part of the
   agent loop"*, and `command` is a shell string with no `args[]` form.
2. **The Gemini-fork hypothesis is backwards, and this changes the priority.** The working
   assumption was that a Gemini adapter would sweep up Qwen Code, iFlow and the other
   forks. It does not. Qwen Code's own README says it forked Gemini CLI v0.8.2 and *"stopped
   syncing with upstream"* at v0.1, and its whole event layer — hooks, notification,
   statusLine, OSC — was **rebuilt post-fork on the Claude Code pattern**, its source
   comments saying so literally ("mirroring Claude Code's tab status icons"). Tencent's
   CodeBuddy Code is not a Gemini derivative at all but a **Claude Code fork**: its bundle
   still carries `CLAUDE_CODE_*` env-var fallbacks and `.claude/agents` path fragments, and
   its flags, hook schema and statusLine field names match Claude Code's almost 1:1 with
   `CLAUDE_*` → `CODEBUDDY_*` renaming. Even iFlow's hook event *names* are Claude-shaped.
   **A Claude-Code-shaped hooks reader covers more of the Chinese field than a Gemini-shaped
   one does; Gemini CLI is the outlier with its own `BeforeAgent`/`AfterModel` vocabulary.**
3. **Codex default drift — needs one re-measurement.** August read `tui.notifications`
   default **`true`** out of the binary's own schema; today's published config reference says
   **`false`**, and `tui.terminal_title` reads `["spinner","project"]` rather than
   `["activity","project"]`. Under this project's own rule — defaults come from the program
   or its `--help`, never from a docs table — **neither value may be quoted in 0.5 copy
   until the installed binary is re-read.** Flagged, not resolved.
4. **OpenCode `attention.enabled` — resolved in August's favour, and the rule vindicated
   again.** The current source (`packages/tui/src/config/index.tsx`) still defaults it to
   **`false`**; the docs *prose* agrees, but the embedded example JSON in `tui.mdx` shows
   `"enabled": true`. **That is the fourth time an upstream docs table or example has been
   wrong about a default in this project's favour-of-the-program rule.** The file
   reference needs one fix though: the protocol selection lives in
   `packages/native/src/terminal.zig`, not `ansi.zig`.
5. **pi's OSC 9;4 is off by default.** August recorded that pi emits OSC 9;4 from its core
   — true — but `showTerminalProgress` defaults to **`false`**. The claim that "the floor
   gets pi's context ring for free" is therefore **wrong**: it is free only for users who
   turned it on. Same for pi's OSC 9/777/99, which live in an *example* extension, not the
   product.
6. **Kimi CLI is being retired.** Its own README: "Kimi CLI is evolving into Kimi Code
   CLI… Installing Kimi Code CLI automatically migrates your configuration and sessions…
   will be gradually wound down." Any Kimi work should target **`kimi-code`**
   (`~/.kimi-code/config.toml`, `KIMI_CODE_HOME`, 20 hook events) — and the quota path
   Folio measured on the legacy product has an **open upstream bug returning zeroed
   ratios** on the new one.
7. **Hermes was badly under-rated** by the first pass — see §7.
8. **Homes moved.** Goose `block/goose` → **`aaif-goose/goose`** (donated to the Linux
   Foundation's Agentic AI Foundation), docs `block.github.io/goose` → **`goose-docs.ai`**.
   OpenCode `sst/opencode` → **`anomalyco/opencode`**. Amp left Sourcegraph and renamed
   `@sourcegraph/amp` → **`@ampcode/cli`**. Any stored URL in the repo should be refreshed.
9. **Aider is dormant, and the evidence is sharper than "no push since 2026-05-22":** 81
   new issues and 54 new PRs in the last 30 days, and **zero merges since May**. Active
   community, absent maintainer, no successor notice in the README. Its
   `--notifications-command` is also **synchronous** and carries **no structured payload**.
10. **Copilot's `beep` on Windows** (issues #1458 / #3573 / #3748 alleging Win32
    `MessageBeep` rather than a tty byte) is **still unverified** — but Aider's confirmed
    Windows behaviour (a PowerShell MessageBox, never a tty byte) shows the failure mode is
    real and common. Nothing of Folio's depends on it: the Copilot adapter uses the hook.

---

## 9. Sources

**Shipped adapters.**
Claude Code hooks https://code.claude.com/docs/en/hooks ·
statusLine fields https://code.claude.com/docs/en/statusline ·
`CLAUDE_CONFIG_DIR` https://code.claude.com/docs/en/env-vars ·
**brand + legal** https://code.claude.com/docs/en/legal-and-compliance ·
Codex config reference https://developers.openai.com/codex/config-reference ·
Codex lifecycle hooks https://github.com/openai/codex/blob/main/docs/config.md ·
Codex legacy notifier https://github.com/openai/codex/blob/main/codex-rs/hooks/src/legacy_notify.rs ·
Codex terminal detection `codex-rs/terminal-detection` in https://github.com/openai/codex ·
Copilot hooks https://docs.github.com/en/copilot/reference/hooks-reference ·
`COPILOT_HOME` https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference

**Gemini CLI.** hooks https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md ·
hook reference https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md ·
settings, `GEMINI_CLI_HOME`, `--acp` https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md ·
notifications https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/notifications.md ·
ACP mode https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md

**Qwen Code.** fork provenance + feature table https://github.com/QwenLM/qwen-code/blob/main/README.md ·
hooks https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/hooks.md ·
status line https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/status-line.md

**CodeBuddy Code.** npm https://www.npmjs.com/package/@tencent-ai/codebuddy-code ·
ACP listing https://zed.dev/acp/agent/codebuddy-code

**iFlow CLI.** hooks https://github.com/iflow-ai/iflow-cli/blob/main/docs_en/examples/hooks.md ·
shutdown notice https://vibex.iflow.cn/t/topic/4819

**Trae.** repo https://github.com/bytedance/trae-agent · roadmap https://github.com/bytedance/trae-agent/blob/main/docs/roadmap.md · ACP request issue #344

**Cursor.** install https://cursor.com/docs/cli/installation · hooks https://cursor.com/docs/hooks ·
config + `CURSOR_CONFIG_DIR` https://cursor.com/docs/cli/reference/configuration · ACP https://cursor.com/docs/cli/acp

**Amp.** manual https://ampcode.com/manual · plugin API https://ampcode.com/manual/plugin-api ·
CLI, WSL-only https://ampcode.com/docs/cli · streaming JSON https://ampcode.com/docs/cli/streaming-json ·
npm rename https://ampcode.com/news/npm-package-changes · press kit https://ampcode.com/press-kit ·
independence https://sourcegraph.com/blog/why-sourcegraph-and-amp-are-becoming-independent-companies

**Crush.** README https://github.com/charmbracelet/crush/blob/main/README.md ·
hooks https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md ·
`notifications` enum in source https://github.com/charmbracelet/crush/blob/main/internal/config/config.go ·
title glyphs https://github.com/charmbracelet/crush/pull/3887

**Goose.** repo https://github.com/aaif-goose/goose ·
hooks https://goose-docs.ai/docs/guides/context-engineering/hooks/ ·
ACP https://goose-docs.ai/docs/gdk/acp/ ·
`goose serve` https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/goose-cli-commands.md ·
remote server https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/remote-goose-server.md ·
ACP providers https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/acp-providers.md ·
config files https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/config-files.md ·
env vars https://goose-docs.ai/docs/guides/environment-variables/ ·
install / Windows https://goose-docs.ai/docs/getting-started/installation/ ·
move to AAIF https://goose-docs.ai/blog/2026/04/07/goose-moves-to-aaif/

**Droid.** hooks https://docs.factory.ai/cli/configuration/hooks-guide ·
notifications https://docs.factory.ai/guides/hooks/notifications · exec https://docs.factory.ai/droid-exec/overview

**Cline.** CLI https://cline.bot/cli · hooks https://cline.bot/blog/cline-v3-36-hooks ·
ACP https://docs.cline.bot/usage/acp · **brand https://cline.bot/brand** ·
CLI README https://github.com/cline/cline/blob/main/apps/cli/README.md

**Kimi.** wind-down notice https://github.com/MoonshotAI/kimi-cli ·
kimi-code hooks https://www.kimi.com/code/docs/en/kimi-code-cli/customization/hooks.html ·
config files https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/config-files.html ·
data locations https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/data-locations.html ·
`kimi acp` https://www.kimi.com/code/docs/en/kimi-code-cli/reference/kimi-acp.html ·
usages bug https://github.com/MoonshotAI/kimi-code/issues/3908 ·
statusline request https://github.com/MoonshotAI/kimi-cli/issues/2149 ·
no OSC emitted https://github.com/manaflow-ai/cmux/issues/898

**OpenCode.** config + env vars https://opencode.ai/docs/config/ ·
plugins https://github.com/anomalyco/opencode/blob/dev/packages/web/src/content/docs/plugins.mdx ·
ACP https://github.com/anomalyco/opencode/blob/dev/packages/web/src/content/docs/acp.mdx ·
Windows/WSL https://github.com/anomalyco/opencode/blob/dev/packages/web/src/content/docs/windows-wsl.mdx ·
`attention.enabled` default in `packages/tui/src/config/index.tsx`; protocol selection in
`packages/native/src/terminal.zig`

**pi.** extensions https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md ·
session format https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md ·
OSC 9;4 in `packages/tui/src/terminal.ts` · open ACP discussion https://github.com/earendil-works/pi/discussions/4444

**Hermes.** CLI https://hermes-agent.nousresearch.com/docs/user-guide/cli ·
repo https://github.com/NousResearch/hermes-agent

**OpenClaw.** repo https://github.com/openclaw/openclaw · docs https://docs.openclaw.ai ·
usage tracking https://docs.openclaw.ai/concepts/usage-tracking ·
Windows https://docs.openclaw.ai/platforms/windows

**Aider.** options https://aider.chat/docs/config/options.html ·
notifications https://aider.chat/docs/usage/notifications.html ·
PyPI https://pypi.org/project/aider-chat/

**ACP / A2A.** site https://agentclientprotocol.com ·
architecture ("the editor boots the agent sub-process on demand")
https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/get-started/architecture.mdx ·
v1 schema https://github.com/agentclientprotocol/agent-client-protocol/blob/main/schema/v1/schema.json ·
v2 schema https://github.com/agentclientprotocol/agent-client-protocol/blob/main/schema/v2/schema.json ·
v2 gating https://github.com/agentclientprotocol/agent-client-protocol/blob/main/agent-client-protocol-schema/Cargo.toml ·
registry https://github.com/agentclientprotocol/registry ·
index https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json ·
A2A https://github.com/a2aproject/A2A · spec https://a2a-protocol.org/latest/specification/

**Chinese vendor paths.** Zhipu https://docs.bigmodel.cn/cn/coding-plan/tool/claude ·
Z.ai https://docs.z.ai/scenario-example/develop-tools/claude ·
DeepSeek https://api-docs.deepseek.com/quick_start/agent_integrations/claude_code/ ·
DeepSeek balance https://api-docs.deepseek.com/api/get-user-balance/ ·
`glm-acp-agent` provenance https://github.com/stefandevo/glm-acp-agent

**Escape-sequence specs.**
OSC 9;4 — `ESC ] 9 ; 4 ; <state 0–4> ; <progress 0–100> BEL`, states 0 hide · 1 normal ·
2 error · 3 indeterminate · 4 warning —
https://learn.microsoft.com/en-us/windows/terminal/tutorials/progress-bar-sequences,
https://ghostty.org/docs/vt/osc/conemu ·
OSC 99 — `ESC ] 99 ; <colon-separated key=value metadata> ; <payload> ESC \`, keys
`i d p a o u c w e f g n s t`, **with a capability query `OSC 99 ; i=<id> : p=? ST`** —
https://sw.kovidgoyal.net/kitty/desktop-notifications/ ·
OSC 9 and `OSC 1337 ; RequestAttention=yes|once|no|fireworks ST` —
https://iterm2.com/documentation-escape-codes.html ·
OSC 777 `OSC 777;notify;<title>;<body> ST` — rxvt-unicode convention ·
OSC 133 — https://contour-terminal.org/vt-extensions/osc-133-shell-integration/

**Brand and marks.** Anthropic, quoted in full because it governs what Folio may draw:
> "You can accurately say, in plain text, that your product has Claude Code preinstalled
> or that it runs Claude Code. But you can't use the Claude Code or Anthropic names or
> logos as part of your own product, feature, or company name, in your own logo, or in a
> way that suggests Anthropic built, endorses, or is partnered with your product. Any
> other use of Anthropic's names or logos is governed by our Trademark Guidelines and
> requires our written permission."
> — https://code.claude.com/docs/en/legal-and-compliance

Trademark Guidelines https://www.anthropic.com/legal/trademark-guidelines ·
OpenAI https://openai.com/brand/ (use the mark as provided, never more prominently than
your own, permission revocable) · Cline https://cline.bot/brand ·
**OpenCode https://opencode.ai/brand** (assets, no usage-licence text found) ·
**Moonshot / Kimi https://moonshotai.github.io/Branding-Guide/** — "© 2025 KIMI. All
rights reserved", **no third-party licence; permission is requested at team@moonshot.ai** ·
Amp press kit https://ampcode.com/press-kit ·
**ACP registry icons** `cdn.agentclientprotocol.com/registry/v1/latest/<id>.svg`, each
under the agent's own `license` / `license_url` recorded in the index.
Aider ships a `logo.svg` under the repo's Apache-2.0 **code** licence, which grants no
trademark rights, and has no separate trademark statement — so its status is unresolved,
not permissive. pi's website repo (`earendil-works/pi-website`) is **archived**, so its
assets should not be assumed current.
**No brand-guidelines page found for:** Cursor, Charm/Crush, Goose/AAIF, Factory/Droid,
Qwen, CodeBuddy, iFlow, Trae, Hermes (`/brand` and `/press` both 404), OpenClaw.
Google's brand hub is partner-gated.

> **A ruling is needed before 0.5 draws any mark.** Anthropic's text permits a plain-text
> statement and forbids logo use "as part of your own product… or in a way that suggests
> Anthropic built, endorses, or is partnered with your product." Whether a vendor mark
> used as a *row identifier in a list of running processes* is nominative identification
> or product decoration is a judgement call, not a settled one. The conservative shape —
> and the one that also solves the eight vendors with no brand page — is **Folio's own
> neutral glyph per vendor, with the vendor's name in text**, upgrading to the ACP
> registry's official SVG only for agents whose registry entry carries a licence that
> permits it.

---

## 10. Recommended adapter priority for Folio 0.5

### Tier A — adapter ships in 0.5

| Agent | Reason |
|---|---|
| **OpenAI Codex CLI** | 15.6M npm/wk, the most-installed agent there is. Already shipped: `notify` + `PermissionRequest` hook + app-server quota. |
| **Claude Code** | 9.4M npm/wk, and the only agent that yields all seven states **plus** context % **plus** quota with no extra work. Already shipped. **Also covers GLM and DeepSeek users for free (§6).** |
| **GitHub Copilot CLI** | 1.08M npm/wk — **third-largest, not marginal**, 3.7× Gemini. Its `notification` event is the best-shaped wait signal any vendor offers: async, fire-and-forget, three named wait subtypes. Already shipped; the suspicion that it was a mistake is not supported by the data. |
| **Gemini CLI** | 107k★, 288k npm/wk, and **newly adaptable** — `AfterAgent` + `Notification/ToolPermission`, `GEMINI_CLI_HOME` for a clean uninstall, native Windows. The largest real gap in today's list. Accept two costs: synchronous hooks, and a shell-string command form. |

### Tier B — next, and cheaper than they look

| Agent | Reason |
|---|---|
| **Qwen Code** | 53k npm/wk and the owner's China half. Its hook set is **Claude-Code-shaped**, so it largely reuses the reader Folio already has; it is the **only agent besides Claude Code with a sanctioned external context-% path** (`ui.statusLine` → `context_window.used_percentage`); it has `QWEN_HOME`; it already emits **OSC 9;4**. High value per line of new code. |
| **CodeBuddy Code** (Tencent) | 43k npm/wk, **a Claude Code fork** — Claude-shaped hooks, Claude-shaped statusLine fields (`context_window.used_percentage`, `cost.total_cost_usd`), `CODEBUDDY_CONFIG_DIR`, native Windows. Probably the single cheapest adapter on this list, and it is a China-market agent the current list has no answer for. |
| **Kimi Code** (not `kimi-cli`) | 26.7k npm/wk; **20 hook events** including `Stop`, `PermissionRequest`, `Notification` and `TurnStarted`; `KIMI_CODE_HOME`; native Windows. **The "which product" question is now answered** — kimi-cli's own README says it is being wound down into kimi-code. Two costs: the hook `command` is a shell string, and the quota route Folio measured on the legacy product has an open zeroed-ratio bug on the new one, so quota must be re-measured before it is drawn. |
| **Cursor CLI** | `~/.cursor/hooks.json` with `stop` and `sessionStart/End`, `CURSOR_CONFIG_DIR`, native Windows. No public star or download signal exists (closed, curl-only), so the case rests entirely on the company's size — decide on judgement, not on this table. |

### Tier C — floor only, with a mark, no adapter

- **OpenCode** (1.8M/wk) and **pi** (1.7M/wk) — both large, both reachable **only through
  an in-process JS/TS plugin the user must install**, which is not a shape Folio's
  installer can own. Both are also poorer at the floor than they first looked: OpenCode
  implements **neither OSC 9;4 nor OSC 133** and its title carries no busy/idle, and pi's
  OSC 9;4 **defaults to off** (`showTerminalProgress`) with its OSC 9/777/99 living in an
  example extension rather than the product. pi does have a real context field
  (`contextUsage.percent`) but only in `--mode rpc`, which is not the mode a user types in.
- **Crush** — its *title* already carries `✋` for waiting, so the floor reads it better
  than most; its only hook is `PreToolUse`.
- **Amp** — 38.9k/wk, in-process TS plugins, and **WSL-only on Windows**, which puts it
  behind §11.8's WSL wall anyway.
- **Cline CLI** — 50k/wk, but its **file hooks are macOS/Linux only**.
- **Droid** — the best-shaped `Notification` payload after Copilot, but **synchronous
  hooks that block for up to 60 s** and **no config-root env var**, i.e. no clean uninstall
  story. Promote only if the blocking behaviour is proven harmless.
- **Goose** (54.5k★, emits nothing today) · **Aider** — its `--notifications-command`
  looked like a zero-cost hook-alike, but it is **synchronous, shell-string, and carries no
  payload**, the project has merged nothing since May, and on Windows a notifying Aider
  opens a PowerShell MessageBox rather than writing a byte Folio could see. Floor only ·
  Auggie · Grok Build · Qoder · MiniMax Code · Kilo · Continue.

### Tier D — ignore

**iFlow CLI** (213/wk, shutdown announced) · **Trae / trae-agent** (dormant, no package
published, no mechanism of any kind) · legacy `gh copilot` (no session loop) ·
**OpenClaw and Hermes as coding agents** — they are not; give them a mark if their TUI
appears in a pane and nothing more · **Zhipu/GLM and DeepSeek as adapters** — their
official path *is* a Claude Code process, so the existing adapter already covers them and
a separate one would be duplicate code. Ship their **marks**, attributed from the
endpoint, not new adapters.

---

## 11. The question asked directly: can one generic mechanism replace most per-vendor adapters?

**No OSC convention can, and none is coming.** There is no shared "agent state" escape
sequence, and no proposal for one: a search of the kitty, Ghostty, WezTerm and Windows
Terminal issue trackers on 2026-09-20 found nothing. The closest things are Claude Code's
own `terminalSequence` hook allowlist (a product convention, not a standard) and a closed
Windows Terminal request (microsoft/terminal#20403) for a generic session-automation
surface — terminal automation, not agent state. The five sequences that do exist each
carry strictly less than Folio needs:

- **OSC 9 / 777 / 99** are one-shot announcements — no state machine, no retraction, no
  way to say "still waiting".
- **OSC 1337 `RequestAttention=yes|no`** is the only *standing, retractable* wait signal
  in existence, and essentially nobody emits it — Claude Code's allowlist explicitly
  **refuses** it.
- **OSC 9;4** is a progress ring with no semantics (though it is a free context ring, and
  pi, Qwen, CodeBuddy and Copilot already emit it).
- **OSC 133** marks shell prompts, not agent turns.

Worse, the agents that do emit these **pick the protocol from a hard-coded list of terminal
names that does not contain Folio**, so the floor's yield is a policy decision by each
vendor rather than a capability Folio can earn. The one encouraging detail is that **OSC 99
has a capability query** (`OSC 99 ; i=<id> : p=? ST`); answering it would at least move
OpenCode and Crush onto their richest path, and claiming a `TERM_PROGRAM` from §3c's lists
(while not leaving `WT_SESSION` set) would move Codex and Gemini off bare BEL.

**ACP can — but only by changing what Folio is for that pane.** ACP v1 (shipping) already
carries `session/request_permission` (Waiting), `stopReason` (Done/Failed),
**`usage_update {used, size, cost}` (context window %)** and `plan` (todos), for **41
CI-verified agents including Claude, Codex, Gemini, Copilot, Cursor, Kimi, Qwen, CodeBuddy,
OpenCode, pi, Cline and Droid** — with official icons in the same registry. v2 adds an
explicit `state_update` of `running` / `idle` / `requires_action`, which is Folio's state
machine almost verbatim. But an ACP agent is a subprocess the *client* spawns and owns both
pipes of; no method attaches to an interactive session a user started in a pane. Adopting
ACP means Folio renders the agent itself instead of hosting its terminal.

**So the honest answer is a split, and it should be written into the plan as one:**

1. **0.5 keeps per-vendor adapters** for the four Tier-A agents — the pane is a terminal,
   and the user typed the agent's name into it. Those four cover ≈27M weekly installs. The
   Tier-B four are cheap because three of them are **Claude-Code-shaped**, which is the
   reusable "generic mechanism" that actually exists today: not a protocol, but a *schema
   family*. A hooks reader parameterised over `{root env var, settings file, event names,
   exec form}` covers Claude Code, Qwen Code, CodeBuddy Code, Copilot, Codex, Droid and
   Kimi — seven vendors — with Gemini as the one genuine outlier.
2. **0.5 takes the ACP registry today, for free.** It is the sanctioned, versioned,
   machine-readable source of official marks and vendor metadata for 41 agents, and it
   answers the §7 mark problem at the cost of one JSON fetch and an icon cache — subject to
   the brand ruling in §9.
3. **"Folio as an ACP client" belongs in 0.6/0.7, and should be named now.** It is the only
   path that yields context %, plans and permission dialogs for dozens of agents with no
   per-vendor code, and it is a genuinely different surface — better decided deliberately
   than discovered while writing the fifth adapter.

---

## 12. What this survey could not settle, and must be measured

Listed so nothing here is quoted in 0.5 copy as if it were established.

| Open item | Why it matters | How to close it |
|---|---|---|
| **Codex `tui.notifications` default** — binary said `true` in August, docs say `false` today; `tui.terminal_title` also drifted | An installer that assumes the wrong default either does nothing or double-notifies | One re-read of the installed `codex.exe` schema, the way August did it |
| **Kimi Code's quota route** (`/coding/v1/usages`, open zeroed-ratio bug) | 0.5 design §7 records the legacy reading as **V** (verified); that verification may not transfer | Re-measure against `kimi-code` before any Kimi number is drawn |
| **Copilot `beep` on Windows** — tty byte or `MessageBeep`? | Only affects the floor, not the adapter | Record a pane with `beep` on |
| **Gemini's synchronous hooks** — how long does the agent actually wait? | 0.5's engineering rule is that a signal hook must not stall the loop; Gemini gives no async form | Time a hook that sleeps, against a real session |
| **Droid's 60 s blocking hook** | Same rule; this is the reason Droid sits in Tier C | Same method |
| **Cursor's statusLine payload field names** | Decides whether Cursor can ever show context % | The reference page 404s; read the binary or ask |
| **Whether a vendor mark may be drawn at all** (§9) | Anthropic's wording is a judgement call, and eight vendors have no brand page | Owner's ruling; the safe default is Folio's own glyph + the vendor's name in text |
| **Amp, Cursor: OSC behaviour** | Both are `?` throughout §3b | A recording each, if either is ever promoted |

Two figures in §1 are large enough to be worth naming as unusual rather than smoothing
over: **Hermes at 247,399 stars in ~14 months** and **OpenClaw at 390,144 in ~10 months**.
Both were re-fetched directly from `api.github.com` and `registry.npmjs.org` rather than
through a summarising layer, and both came back consistent. No third-party corroboration
(star-history and similar) was available in this environment, so they are reported as the
APIs give them, with the growth rate flagged.
