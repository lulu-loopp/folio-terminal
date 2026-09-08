# The five shells, measured — and what Command Prompt got

2026-09-07. `main` at `3c7b1b9`, branch `feature/shell-matrix-and-cmd-integration`,
debug build for the functional rows and `--release` for the latency ones, driven
in the real window on this machine (Windows 11 26200, 1400×1000 logical, 96 DPI).

Every window was opened with an isolated `APPDATA`/`LOCALAPPDATA` and its own
`BT_PTY_DUMP`, and every one of them was closed. No real input was injected: the
window was driven entirely by **posted** messages
(`scripts/dev/post-probe.ps1`, which gained `type`, `key`, `chord`, `wheel` and
`shot` verbs for this run) so that the keyboard stayed with the person using the
machine. One consequence is recorded under §5: a **posted chord does not reach
winit**, because winit reads modifier state from the real keyboard rather than
from the messages it is handed, so `Ctrl+Shift+…` could not be pressed at all.
Where a chord was needed it was reached by writing a plain key into the isolated
`keybindings.json` instead, which is the product's own mechanism.

Screenshots are in the session scratchpad under
`…\scratchpad\matrix\shots\` and `…\scratchpad\matrix\burst\`; the file names
are given beside each finding.

---

## 1. The matrix

| | 1 marks + jump | 2 clear verbs | 3 cwd follows `cd` | 4 printed path clickable | 5 `$$` / `$…$` | 6 card | 7 card latency | 8 odd |
|---|---|---|---|---|---|---|---|---|
| **PowerShell 7** | ✓ | ✓ | ✓ folder, ✗ **tab title** | ✓ `C:\…` | ✓ / ✓ | full | **tracks every row** | conda prompt wrapped cleanly |
| **Windows PowerShell 5.1** | ✓ | ✓ | ✓ folder, ✗ **tab title** | ✓ `C:\…` | ✓ / ✓ | full | as above | — |
| **Command Prompt** | ✗ **→ now ✓** | ✓ | ✓ incl. tab title | ✓ `C:\…` | ✓ / ✗ *(expected ✗)* | full | **misses rows, ≈1.5–2.5 s behind** | CJK typed and drawn correctly |
| **Git Bash** | ✓ | ✓ | ✓ (tab shows the shell's own `MINGW64:/d/Demo`) | ✓ `D:\…` and `D:/…`; ✗ `/d/…`, ✗ `/c/…` | ✓ / ✓ | not measured (other line) | not measured; sends `C`/`D`, so T-5's clock applies | — |
| **WSL · Ubuntu-24.04 (bash)** | ✗ **on the first WSL pane of a process**, ✓ from the second on | ✓ | ✗ / ✓ on the same terms | ✗ `/mnt/d/…`, ✗ `/home/…` | ✓ / ✗ then ✓ | not measured | **worst case by T-5** — a first pane reports neither `133` nor `OSC 7` | the race is T-2 |
| **WSL · zsh** | n.a. | n.a. | n.a. | n.a. | n.a. | n.a. | n.a. | no `zsh` in Ubuntu-24.04 on this machine |

"Expected ✗" means `docs/DESIGN.md` and `docs/shell-integration.md` already say
the capability is not there. There is exactly one: **inline `$…$` in a Command
Prompt pane**, which needs `OSC 133;C` to name a row as command output, and `C`
is the marker `PROMPT` has no moment to send. Everything else marked ✗ is a
finding, and each has a ticket in §4.

### Row by row

**1 — marks on the strip, and the jump.** `pwsh`, `powershell.exe` and Git Bash
draw a tick per command on the rail at the pane's right edge, and a posted click
on one scrolled the pane so that the clicked command's *prompt row* sat at the
top (`all-02-pwsh.png` → `all-03-tickjump.png`: the rail's top tick put the
`Write-Output` prompt on the second visible row). Command Prompt drew nothing
before this branch and now draws one tick per prompt — that is Part 2, §2, and it
was checked in the real window on the release build of this branch:
`cmdcard-02-ticks.png` has four ticks on a `cmd` rail after three commands, and a
posted click on the top one scrolled 805 lines back to put `echo one`'s own prompt
row at the top of the pane (`cmdcard-05-jump.png`).

**2 — the two clear verbs.** Both live on the terminal's own right-click menu,
not on the pane chevron (`cmd-07-panemenu.png` has no clear verb;
`cmd-09-ctx.png` has `Clear screen`, `Clear scrollback…` and `Restart shell…`).
They are terminal-side operations and do not consult the shell:
`Clear screen` on a `cmd` pane emptied the grid and left the prompt
(`cmd-clearmenu.png` → `cmd-cleared.png`).

**3 — the working directory.** The *folder* half is right everywhere the shell
reports `OSC 7`: `cd /d D:\Demo` in a `cmd` pane moved the files column to
`D:\Demo`, the pane head to `D:\Demo` and the tab to `Demo`
(`cmd-06-filecol.png`). The *tab title* half is not:

* a **PowerShell** tab is called `PowerShell 7` / `Windows PowerShell 5.1` for
  the life of the pane and never shows the folder (`winps-02.png`,
  `winps-03-files.png`: prompt reads `D:\Demo>`, tab still reads
  `Windows PowerShell 5.1`). Ticket T-1.
* a **Git Bash** tab is called `MINGW64:/d/Demo` — Git for Windows' own
  `PS1` title, in the MSYS spelling — rather than `Demo` (`gitbash-01.png`).
  Same cause as T-1, opposite symptom.
* a **WSL** tab is called `weiyi@LAPTOP-…: /mnt/d/Demo`, Ubuntu's own default
  title, for the same reason.

**4 — a printed path.** Windows-spelled paths are found and drawn as links in
every shell, including inside the command echo, and including the
forward-slash spelling `pwd -W` prints (`gitbash-03.png`: `D:\Demo\figure.png`
and `D:/Demo` are both underlined). No MSYS or WSL spelling is:
`/d/Demo/figure.png`, `/c/Windows/System32/drivers/etc/hosts`,
`/mnt/d/Demo/figure.png` and `/home/weiyi` were all left as plain text
(`gitbash-03.png`, `wsl-01.png`). Ticket T-3.

**5 — mathematics.** A `$$…$$` block is typeset in **every** shell, with or
without shell integration — it is not gated on `OSC 133` at all
(`cmd-04.png` shows the Gaussian integral rendered in a `cmd` pane that has
never sent a marker). An inline `$…$` is typeset only inside a command's own
output region, which is `C..D`, so it renders under both PowerShells and Git
Bash and does not render under Command Prompt or under a first-pane WSL. That
is the design (`bt_term::…::only_the_command_output_region_names_a_live_row_inline_eligible`),
and it is the one place the matrix's ✗ is the documented answer rather than a
defect.

**6 — the sidebar card.** The card is the tab card in `Cards` (focus mode,
`Ctrl+Shift+Z`). It is a **full** projection, not a summary and not a stale
snapshot: after `1..400 | ForEach-Object {…}` the card carried the last twelve
rows of the pane and the prompt beneath them, in the pane's own order and
colours (`burst\b-04337.png`). The same held for a `cmd` pane
(`cards-02-bash.png`, which is a Command Prompt tab in spite of its name) and
the card followed a `Clear screen` in step (`cmd-cleared.png`).

**7 — card latency.** Measured on the release build by photographing the window
every ~80 ms from the moment `Enter` was posted (the camera's own floor is a
frame every ~175 ms) and diffing the card rectangle against the pane rectangle.

*A command that finishes at once* looks the same in every shell — the card lands
within one sample of the pane, because the prompt printed afterwards repaints the
chrome:

| shell, workload | pane settles | card settles |
|---|---|---|
| `pwsh`, one line of output | 362 ms | 362 ms (`burst-*.png`) |
| `pwsh`, 400 rows at once | 242 ms | 411 ms (`burst-*.png`) |
| `cmd`, 400 rows at once | 189 ms | 356 ms (`burst\c-*.png`) |

*A command that prints slowly* is where the shells separate, and this is the
user's report reproduced. Six rows, one every ~1.4 s, with `Cards` up:

| shell | frames at which the **card** changed | rows the card missed |
|---|---|---|
| `pwsh` (`burst-*.png`) | 309, 1712, 3133, 4531, 5947 ms — one per row | **none** |
| `cmd` (`burst\d-*.png`) | 710, 1638, 3396, 5912, 8867 ms, against pane changes at 1638, 3396, **4869**, 5912, **7406**, 8867 | **two of six**, each waiting ≈1.5–2.5 s for whatever repainted the chrome next |

The mechanism is T-5, and it is read off the code rather than inferred from these
numbers. **Part 2 does not change it**: the clock is `DualPlaneSession::working`,
which only `133;C` sets and only `133;D` clears, and `cmd` sends no `C` and no `D`
that follows one.

**8 — anything odd.** Nothing beyond the tickets. CJK typed into a `cmd` pane
draws at the right width (the user's own earlier report, unchanged here);
PowerShell's prompt survives being wrapped by `folio.ps1` under conda
(`(base) PS D:\…>` with the markers intact, `all-02-pwsh.png`); no prompt-redraw
glitch was seen in any shell at this window size.

---

## 2. Command Prompt integration (Part 2)

### What shipped

`Integration::CmdPrompt` already existed and carried `OSC 7` alone. It now
carries the two `OSC 133` markers that describe the one moment `cmd.exe` expands
`PROMPT` at:

```
PROMPT=$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\<whatever PROMPT was>
```

`$e` is ESC and `$e\` is ST; `$P` is the drive and path. `D` ends the command the
previous prompt started, `A` opens this one, and the report sits between them —
`folio.bash`'s own order, so both doors report at the same point of the cycle.

Three changes make that safe and useful, and each is pinned:

1. **`bt_term::command_marks`** opens a record at `A` rather than at `B`. A `B`
   that follows reclaims the blank draft, which is the same reclaim a redrawn
   prompt already relied on, so a shell that sends both is byte for byte where it
   was: one record per command, `start` at `B`, `prompt` at `A`.
2. **Authority is narrowed.** `shell_integration_is_authoritative` — whose job is
   to retire the cursor-line heuristic — now asks whether a *region-building*
   marker (`B` or `C`) has been seen on that screen, recorded in the new
   `shell_region_screens`. A screen carrying only `A` and `D` keeps the
   heuristic. `shell_integration_seen()`, which an offer-to-install retracts
   itself on, still answers to any marker at all and is now a separate reading.
3. **`133;B` is still not sent**, and that is the half of the 2026-08-16
   measurement that has not expired — see below.

### The decision on a pre-existing `PROMPT`

**Prefixed, never replaced, and the markers go in front with the report.** A
`PROMPT` in the environment is a prompt somebody wrote with `setx`, and a
terminal that overwrote it would have taken their prompt away in exchange for a
capability they did not ask for. All three of `133;D`, `OSC 7` and `133;A` are
printed *before* the row the cursor ends on, so a user prompt of any shape —
including one ending in `$_`, a newline — keeps its appearance exactly.

There is therefore no "respect it, so no marks" branch: the marks are invisible,
they cost the user's prompt nothing, and refusing them would be refusing a
capability for a reason that does not exist. The one case that *is* skipped is
**a `PROMPT` that already carries the report**: a `cmd` pane exports `PROMPT` to
everything it starts, so a Folio launched from a `cmd` pane inherits a string
that already reports, and prefixing again would double the report and go on
doubling. That check was already there and is unchanged.

An **empty** `PROMPT` is not a prompt somebody chose; it is what `cmd` reads as
"use the default", so it gets `cmd`'s own documented default `$P$G` spelled out —
also unchanged.

### What `cmd` still does not get, and why

* **No exit code.** `PROMPT` has no substitution that reads `ERRORLEVEL`, so `D`
  is sent bare. A tick coloured from a number nobody reported would be worse than
  a tick with no colour.
* **No `133;C`, and therefore no inline `$…$` and no command text on a tick.**
  There is no pre-execution moment in this shell.
* **No `133;B`.** `B` opens an input region whose only closers are `C` and the
  *next* `A`. Without `C` every row a `cmd` command printed would sit inside the
  region that means "this is what the reader is typing" — losing the display
  mathematics and image previews that `cmd` panes have today, and owing the
  post-resize `InvokePrompt` chord to a shell with no such binding. This is the
  half of the old pin that is still true, and it is why the ticks come from `A`
  rather than from `B`.
* Clink would supply `C` and a real exit code both, and is still not required.
  Detecting it and upgrading the row is booked, not done.

### Settings

The Profiles page's integration line for Command Prompt reads
**"Prompt marks, directory and hyperlinks; no exit codes"** (and, where the
hyperlink declaration has been overridden away, "Prompt marks and directory; no
exit codes, no hyperlinks"). It is derived from the same `Integration` value the
spawn path uses, so it cannot drift from what the pane gets.

**Both halves of the Command Prompt row were rewritten**, English and Chinese,
because two shipped gates forbid an untranslated string
(`no_entry_ships_the_english_word_as_its_own_translation` and
`every_chinese_entry_carries_at_least_one_han_character`), so leaving the English
standing in the Chinese column was not an option this table has:

| id | English | 中文 |
|---|---|---|
| `Text::CapCmd` | Prompt marks, directory and hyperlinks; no exit codes | 命令标记、当前目录、链接；没有退出码 |
| `Text::CapCmdNoLinks` | Prompt marks and directory; no exit codes, no hyperlinks | 命令标记、当前目录；没有退出码、没有链接 |

---

## 3. Red tests and their pre-fix failures

Each was run against the production code mutated back to its pre-change shape,
one mutation at a time.

| test | mutation | failure |
|---|---|---|
| `bt_app::shell_integration::…::command_prompt_marks_its_command_boundaries_and_reports_its_directory` | `cmd_prompt` prefixes `CMD_OSC7` alone | `left: "$e]7;file:///$P$e\\$P$G"` vs `right: "$e]133;D$e\\$e]7;file:///$P$e\\$e]133;A$e\\$P$G"` |
| `bt_app::shell_integration::…::a_prompt_the_user_already_set_is_kept_and_reported_in_front_of_exactly_once` | same | `left: "$e]7;file:///$P$e\\$T$S$P$G"` vs the marked string |
| `bt_term::session::…::a_prompt_only_shell_gets_its_ticks_and_keeps_the_cursor_heuristic` (half one) | `note_prompt` remembers the anchor without opening a record | `the first prompt is one command's worth of record… left: 0, right: 1` |
| the same test (half two) | `shell_integration_is_authoritative` reads `shell_phases` again | `a shell that reports prompts has not said where the typed line ends, so the heuristic that guesses it stays` |
| `bt_term`'s `shell_integration_cmd::command_prompt_marks_every_prompt_and_reports_its_working_directory` | ledger mutation, through a **real `cmd.exe`** | `one record per prompt: 0 records for three prompts` |

`shell_integration_cmd::the_prompt_this_test_runs_is_the_prompt_the_product_sets`
is the anti-drift gate: `bt-app` is a binary crate with no library target, so the
integration test spells the `PROMPT` again and then reads
`crates/bt-app/src/shell_integration.rs` to require the three constants verbatim.

---

## 4. Tickets

### T-1 — a PowerShell tab is never named after the folder it is in

**Evidence.** `winps-02.png` / `winps-03-files.png`: the pane is standing in
`D:\Demo`, the files column and the pane head both say so, the tab says
`Windows PowerShell 5.1`. Same for `pwsh` (`all-02-pwsh.png`).

**Cause, read off the code.** A tab's name is
`manual > program title (OSC 2) > cwd leaf (OSC 7) > profile title`
(`main::resolve_title`). `folio.ps1` writes `OSC 0` naming the edition —
`PowerShell 7` / `Windows PowerShell 5.1`, deliberately equal to the profile's
own title — and `LeafSession::announced_title` only drops a title equal to the
resolved **program path**, which that is not. So the announcement wins the stack
forever and the folder layer is never reached.

`main::pane_head_title` already applies the missing rule, against
`profiles::announcement_set`, and its own doc states the argument: "a shell that
reports the very name its profile already goes by has announced nothing — it has
agreed with the launcher". The tab does not apply it.

**Shape of the fix.** Give `resolve_title`/`display_title` the announcement
*set* rather than one profile title and filter the program-title layer through it,
exactly as `pane_head_title` does. Where no folder has been reported the tab
still falls through to the profile title, which is the same string, so nothing
regresses.

**Why it is not in this branch.** It changes what every PowerShell tab in every
screenshot and README is called, and the head's own doc frames the filter as a
head-specific ruling ("a head with a whole bar to fill"). Whether the tab adopts
it is the user's call, not an obvious one-file bug. Git Bash and WSL tabs are the
same defect wearing the shell's own title instead (`MINGW64:/d/Demo`,
`weiyi@…: /mnt/d/Demo`) and would need the same ruling — those titles *are*
announcements, so the filter would not touch them and a second question is owed:
does a tab prefer the shell's title or its folder?

### T-2 — the first WSL pane of every process gets no shell integration at all

**Evidence.** `wsl-01.png`: a WSL pane in `/mnt/d/Demo` with no ticks, no inline
mathematics, Ubuntu's own unwrapped `PS1`. The user's own report pairs WSL with
Command Prompt and not with Git Bash, which is this defect seen from the other
end: a first-pane WSL reports neither `OSC 133` nor `OSC 7`, so it is also the
worst case of T-5. Its `BT_PTY_DUMP` contains **no**
`OSC 133` and **no** `OSC 7` at all. The very next WSL pane in the same process
(`…\dumps\all.txt.3`) contains the full `A`/`B`/`C`/`D;0` cycle and
`ESC]7;file:///mnt/d/Demo`. The two spawns, read out of `Win32_Process`:

```
wsl.exe --cd /mnt/d/Developer/bt-wt-shell-matrix
wsl.exe --cd /mnt/c/Users/… -- /bin/bash --init-file /mnt/c/…/folio.bash -i
```

**Cause.** `wsl::begin_login_shell_probe` is started *from the pane spawn* and
never waited for — deliberately, because it boots a virtual machine and §7.40 ②
refuses to make a frame wait for one. `shell_command_for` then needs
`wsl.integrated_login_shell()` and gets `None`, so it falls through to the bare
launcher. The answer lands seconds later and every WSL pane after it is served
in full. So the documented WSL row is true of the second pane onwards and false
of the first, which on a `--profile wsl` launch is the only one there is.

**Shape of the fix (recommended).** Stop asking a separate `wsl.exe` and let the
pane we are already starting answer for itself:

```
wsl.exe --cd <dir> -- sh -c 'shell=$(getent passwd "$(id -u)" | cut -d: -f7);
  case "$shell" in *bash) exec "$shell" --init-file "<script>" -i;;
                   *) exec "$shell" -l;; esac'
```

That boots no virtual machine for anyone who never opens a WSL pane — which is
the whole of what §7.40 ② was protecting — and makes the *first* pane correct,
which the current shape cannot. `WslFacts` is still wanted for the profile's
title, and that half already works from the registry.

**Size.** Changes the WSL spawn contract and the two pinned tests around it. Not
a one-file fix.

**Fixed, 2026-09-07, on the user's ruling** (`fix/wsl-first-pane-integration`).
The recommended shape, with one correction the measurement above did not reach:
the launcher is handed the question with **`-e`, not `--`**. `wsl.exe --` joins
everything after it into a single command line and gives that to the login shell,
which re-parses it — a question full of spaces, quotes, `$`, `|` and `;` arrives
in pieces and `$1` is empty. `wsl.exe -e` executes argv for argv. The probe is
gone rather than deferred: `wsl::begin_login_shell_probe` and `ask_login_shell`
are deleted, `crates/bt-app/src/wsl.rs` reads the registry and starts no process
at all, and `BT_SHELL_INTEGRATION` moved out of `WSLENV` into the one branch of
the question that reads the init file. Verified in the real window with a fresh
isolated `APPDATA`: `folio --profile wsl`, one WSL pane and no other, and its
`BT_PTY_DUMP` carries `ESC]7;file:///home/…` and `133;A`/`B`/`C`/`D;<code>` from
the first prompt on, with the rail carrying one tick per command coloured by its
exit code.

### T-3 — an MSYS or WSL path is never a link

**Evidence.** `gitbash-03.png` and `wsl-01.png`: `D:\Demo\figure.png` and
`D:/Demo` are underlined; `/d/Demo/figure.png`,
`/c/Windows/System32/drivers/etc/hosts`, `/mnt/d/Demo/figure.png` and
`/home/weiyi` are not.

**Cause.** The detector (`bt-transcript`'s path scanner) accepts drive-rooted
Windows paths and relative references and requires the target to exist on disk.
There is no namespace translation anywhere on that path, and there could not be
one inside that crate: `/c/…` means a drive only to an MSYS shell and `/mnt/c/…`
only to a WSL one, and which of the two a pane is standing in is
`profiles::PathNamespace`, which lives a crate away.

**Shape of the fix.** The pane already declares its namespace to the session for
`OSC 7`; the same declaration has to reach the detector, and the detector needs a
translation step before its existence check — `/c/x` → `C:\x` for
`PathNamespace::Windows` panes whose shell is an MSYS bash, `/mnt/c/x` → `C:\x`
for `PathNamespace::Wsl`. A WSL-internal path such as `/home/weiyi` has no
Windows spelling and must stay plain text, which is the same empty cell
`docs/shell-integration.md`'s "carrying a directory between profiles" table
already has.

**Size.** Crosses `bt-app` and `bt-transcript` and adds a per-pane parameter to
the detector. Not a one-file fix.

**Fixed, 2026-09-07, on the user's ruling** (`fix/unix-path-spellings-and-cmd-hover`).
The recommended shape: `bt_transcript::paths::PrintedPathNamespace` is the
per-pane parameter, `bt_app::profiles::printed_path_namespace` derives it from
the pair a profile already carries — Windows directories behind a bash init file
is an MSYS bash, `wsl.exe` is a distribution, everything else speaks this
machine's own spelling — and the spawn pushes it into the session beside
`set_spawn_directory`. Nothing reads a profile id, and nothing guesses from the
text: `/d/Demo` printed into a PowerShell pane is still prose. Two corrections
the shape above did not reach:

* `~` is expanded for MSYS (Git for Windows maps `$HOME` onto `%USERPROFILE%`,
  which is what the `gitbash` row's `StartingDir::WindowsHome` already acts on)
  and **not** for WSL, for the same reason `/home/alice` stays plain text.
* the fix carries a second one with it: a WSL pane reports `OSC 7` as
  `/mnt/d/Demo`, which `resolve_relative_reference` refuses as a base, so
  **relative** references in a WSL pane were dead too. The base is translated
  once, in `PrintedPathLinks::in_namespace`.

`\\wsl.localhost\<distro>\…` was **considered and not taken** in the fix above,
and it was the one piece of this ticket left open: a UNC target is a `file:`
authority `decode_file_uri` is obliged to refuse as remote, so admitting it means
widening `is_local_absolute_path`, `local_path_to_file_uri` and every
`Rooting::DriveOnly` call site — a ruling about what "local" means, with a
security-shaped edge (`\\server\share\…` printed in any pane becoming clickable
with it), rather than a translation.

**Taken the same day, on the user's ruling** (`fix/wsl-distro-paths-and-host-colon`).
It is one namespace-scoped translation and not a change to what "local" means:
`distro_path_to_local_path` sits beside `drive_mount_to_local_path`, is reached
only from `PrintedPathNamespace::Wsl { distro, home }`, and produces only this one
share. `is_local_absolute_path` admits that share as a **root**, `resolve_relative_reference`
takes it as one, `local_path_to_file_uri` writes it as RFC 8089's `file://wsl.localhost/…`,
and `preview::is_network_path` answers "no" for the same prefix through the same
function — while `decode_file_uri` still refuses every foreign authority and no scan
opens a candidate on a printed `\\` or `//`, so `\\server\share\…` is exactly as
unrecognised as it was, in every pane. The distribution comes from the profile's
own `-d` argument or the machine's default; `~` is the home the pane's shell
reported at its first prompt. §7.30 carries the ruling.

### T-4 — a posted chord cannot drive this window

Not a product defect; a limit of the probe, recorded so the next run does not
rediscover it. `post-probe.ps1`'s new `chord` verb posts `WM_KEYDOWN` for
`VK_CONTROL`/`VK_SHIFT` around the base key, and winit ignores them: it reads
modifier state from the real keyboard (`GetKeyState`), which a posted message
cannot move, and `SetKeyboardState` does not cross a process boundary. So
`Ctrl+Shift+B`, `Ctrl+Shift+Z` and the rest are unreachable without real input.
The way round, used here, is to write the wanted action into the isolated
`%APPDATA%\Folio\keybindings.json` against a plain key (`F9` for `focus-mode`)
and post that. The verb is kept because it costs nothing and would work against
any window that reads its modifiers from the message stream.

---

### T-5 — the card's refresh clock is a shell-integration clock

**The user's report** (2026-09-07, on `dist\next45`): the sidebar card refreshes
noticeably slower for Command Prompt and WSL than for PowerShell 7 and Git Bash,
which both feel immediate. Reproduced: §1's row 7 table.

**Cause, read off the code.** The card's own damage key is shell-agnostic —
`focus_thumb.rs:609` keys a terminal seat on `DualPlaneSession::screen_revision()`,
which `session.rs:2702` bumps on *any* non-empty chunk from the child. Nothing
about OSC 133 is in it.

But that key is only consulted from inside `Runtime::refresh_chrome()`
(`main.rs:34221` calls `refresh_focus_thumbnails`, which is where every card is
re-projected), and **nothing on the PTY output path calls `refresh_chrome()`**.
`publish_pty_drain_frame` publishes a new *pane* picture and re-presents the
chrome the renderer already held. So a card moves only when something asks for a
chrome rebuild, and on the output path there are exactly two such things, both
shell-integration-shaped:

1. **The tab-mark breath.** `DualPlaneSession::working` is set by `133;C`
   (`session.rs:3925`) and cleared by `133;D` (`session.rs:3962`) — those are its
   only two writers. While it is true, `session_is_breathing → fleet_working →
   mark_opacity → tab_owes_frame → advance_strip_animation` gives the window a
   `STRIP_ANIMATION_FRAME` = 16 ms tick that calls `refresh_chrome()` every time.
   An integrated shell therefore hands the card column a 62.5 Hz clock for the
   whole duration of every command, and the cards run at their own ceiling —
   `focus_thumb::MIN_INTERVAL` = 100 ms, a skip rather than a deferral.
2. **A rename.** `drain_pty` calls `refresh_chrome()` when `name_evidence`
   changed, which is the OSC 0/2 title or the OSC 7 directory (`main.rs:14180`,
   `main.rs:72976`). A shell that reports `OSC 7` once per prompt therefore
   repaints every visible card once per prompt — at the prompt, not during the
   command.

So the three tiers are exactly the three the user felt: a shell with `C`/`D`
tracks live; `cmd`, which reports `OSC 7` and no `C`, catches up at each prompt
and rides incidental UI activity in between; and a first-pane WSL, which reports
neither, has no output-driven path to a card repaint at all.

**Why Part 2 does not fix it.** `cmd` now sends `133;D` and `133;A`, and `working`
is set only by `C`. The rail's ticks are new; the card's clock is not.

**Shape of the fix.** The card column should contribute its own frame deadline,
the way the tab-mark breath already does — `strip_animation_deadline`
(`main.rs:73454`) returns `Some` while a mark is animating and that is what makes
`about_to_wait_inner` use `ControlFlow::WaitUntil`. The same rule for the rail:
while `focus_rail_geometry_now()` is `Some` and any visible seat's damage key has
moved, ask for a frame at the card's own `MIN_INTERVAL` rather than at 16 ms. The
existing gates then do the rest — gate 3 skips a card whose damage key is equal
and costs nothing, gate 4 caps the rest at 10 Hz — so the ceiling is what an
integrated shell already pays today, and it becomes the same for every shell.

**Why it is not in this branch.** It is a change to the window's frame cadence,
which is the one part of this program with a hang watch built around it
(`DESIGN.md` §7.49), and it wants its own measurement of what a card column costs
per frame at 10 Hz with several tabs open. That is a ruling and a measurement,
not a one-file bug.

## 5. What Part 3 fixed

**Nothing in code.** Each of the four findings above is a ruling or a
cross-crate change with a named shape, and none is a one-file bug with an obvious
cause. One documentation fix did land, because the page was making a promise this
machine measurably does not keep: `docs/shell-integration.md`'s WSL section now
says that the first WSL pane of a process is started without the init file, and
points at T-2.

*(T-2 was ruled on and fixed the same day, on the branch
`fix/wsl-first-pane-integration` — see the note under that ticket. That
documentation fix has been replaced by the description of what the pane now
does.)*
