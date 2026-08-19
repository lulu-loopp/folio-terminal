# OSC 133 + OSC 7 shell integration

Folio treats FinalTerm Command Status (FTCS) `OSC 133` markers as the authoritative
prompt/input/output boundary for each terminal screen that emits them:

- `133;A`: prompt starts.
- `133;B`: command input starts.
- `133;C`: command input ends and output starts.
- `133;D[;<exit-code>]`: command output ends.

Every row written between B and C is command input, including the shell's visible command echo.
It never receives an image or formula decoration while live, and retains that identity after it
migrates through staging into the frozen transcript. Rows between C and D are output and use the
normal decoration pipeline. Registered candidates and asynchronous worker completions both recheck
the same region ownership, so a decoration cannot survive by racing a marker.

Region endpoints use Folio content anchors. Normal scrolling migrates them atomically from
live grid to staging to transcript. Before a resize, a live region also captures its exact displayed
command text; after vendor reflow, that content witness re-seats its endpoints on the new physical
rows. Resize therefore changes only projection, not input/output ownership.

## OSC 7: the authoritative working directory

The same script also emits `OSC 7` once per prompt, immediately before `133;A`:

```
ESC ] 7 ; file:///<percent-encoded $PWD> BEL
```

This is the standard Windows Terminal / iTerm convention and it is the **only** way Folio
learns where a session's output is being printed from. It exists to resolve relative image path
text (`./shot.png`, `../a/b.svg`, and bare references carrying a separator such as
`local-images/sunset.svg`) — see `docs/M2-preview-matrix-and-verbs.md` §6.3. A session that
never receives OSC 7 leaves relative paths undetected rather than guessing a directory, exactly as
a screen that never emits OSC 133 keeps the cursor/WRAPLINE heuristics.

The authority is empty (the file-URI spelling of "this host"); Folio also accepts
`localhost` and this machine's own name, and rejects every other authority as a remote share. The
path is percent-encoded minimally: UTF-8 byte by byte, keeping RFC 3986 unreserved characters,
sub-delims, `:`, `@` and `/`. The directory is stored per session and survives primary/alternate
screen switches, because a working directory belongs to the shell process and the full-screen TUI
it launched inherits it.

A location on a non-filesystem provider (`HKLM:`, `Cert:`, …) emits an **empty** report, which
retracts the previous directory. An unresolvable report — a remote share, a malformed URI, a
truncated one — clears the stored directory for the same reason: leaving a stale directory to
answer for a place the shell has left is the guess the ruling forbids.

## Which spelling of a directory each profile reports

OSC 7 carries a path, and a path only means something in a namespace. Each profile declares the one
its shell stands in (`profiles::PathNamespace`), and its script reports in that namespace and no
other:

| profile | namespace | what OSC 7 carries | how the script gets it |
|---|---|---|---|
| PowerShell / Windows PowerShell | Windows | `file:///D:/src` | `$PWD.ProviderPath` |
| Git Bash | Windows | `file:///D:/src` | `pwd -W`, the MSYS builtin for the Win32 spelling |
| WSL | WSL | `file:///mnt/d/src`, `file:///home/weiyi` | `$PWD` |
| Command Prompt | Windows | `file:///D:\src`, `file:///C:\Program Files` | `$P`, in the `PROMPT` variable |

Command Prompt's report is spelled with **backslashes and unencoded spaces**, and that is forced
rather than chosen. `PROMPT` is not a hook, it is a format string: its whole alphabet is a dozen
`$`-substitutions, of which `$P` (the current drive and path) and `$e` (escape) are the two that
matter, and there is no operation in that language that could turn `\` into `/` or percent-encode
a space. So the report says the directory in the only spelling the shell can say it in. The decoder
takes it — a backslash is not a delimiter in a URI, so a Windows path survives segmentation as one
piece — and that acceptance is now a pinned decision rather than a happy accident
(`a_working_directory_may_be_spelled_the_way_a_windows_shell_can_spell_it`), because tightening the
URI parser is a reasonable-looking edit that would silently blank every Command Prompt pane's
directory with no symptom inside the crate that made it.

The one thing `PROMPT` cannot survive is a `%` in a directory name, which percent-decoding will
read as the start of an escape. That report is malformed, and a malformed report clears the stored
directory rather than leaving a stale one — the standing rule above, reached by the standing route.

Git Bash reports the **Windows** spelling although it prints `/d/src` at its prompt, and that is not
a translation for our convenience: the process's working directory *is* a Win32 directory, which is
what `CreateProcess` was handed and what Explorer opens. `/d/src` is a third namespace only that
shell understands, and adopting it would mean every existence check, every relative-image
resolution and every inheritance into another pane had to learn a spelling nothing else speaks.
`pwd -W` is a builtin and the answer is remembered against `$PWD`, so the subshell it needs happens
when you `cd`, not on every prompt.

WSL reports the POSIX path and does **not** convert. `wslpath -w` would answer
`\\wsl.localhost\<distro>\home\weiyi` for a Linux home — a UNC whose authority the rules above are
obliged to reject as a remote share — so converting would make the most common directory in WSL
unreportable. A POSIX directory is stored and displayed as it is, and relative image path text
beside it stays undetected, exactly as it does for a session that reported nothing: the resolution
gate is still "drive-rooted", and this is the standing rule for a directory this terminal cannot
resolve against rather than a new exception.

### Carrying a directory between profiles

A new tab opens where the shell you are looking at is standing, whenever the shell it starts can
name that place (`profiles::cwd_for_spawn`). Between the two namespaces the map is WSL's own drive
mounts, and it is not total:

| from → to | `D:\src` | `/mnt/d/src` | `/home/weiyi` |
|---|---|---|---|
| → Windows profile | `D:\src` | `D:\src` | *no answer* |
| → WSL | `/mnt/d/src` | `/mnt/d/src` | `/home/weiyi` |

The empty cell is the honest one: a directory inside the distribution's own filesystem has no
Windows spelling, so the new tab starts at its own profile's starting directory rather than at a
place that does not exist. Same rule, same reason as an unreported directory — never guess one.

## Injecting the script

PowerShell's script is **opt-in and manual**: you dot-source it into `$PROFILE` yourself, and this
product never writes there. The bash script is installed automatically, for one session at a time,
and the asymmetry is the shells':

* `pwsh` has one startup file at one well-known path and no argument that would source a second one
  after it, so the only automatic injection available would be editing a file that belongs to you.
* `bash --init-file <file>` names the startup file for one interactive shell and touches nothing on
  disk.

So a Git Bash profile is started as `bash --init-file <script> -i` and a WSL profile as
`wsl.exe [--cd <dir>] -- <login shell> --init-file <script> -i`, with `BT_SHELL_INTEGRATION=1` in
the environment. The script is written out to `%APPDATA%\Folio\shell-integration\` from a
copy compiled into the binary, so the two halves of the OSC 133 agreement always ship together.

**What `--init-file` costs, and how it is paid back.** It replaces `~/.bashrc`, and because bash
consults it only for a shell that is *not* a login shell, Folio also drops the `--login`
that Git Bash's own shortcut passes. The script therefore runs the startup chain itself, in bash's
documented order — `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login`,
`~/.profile` — before installing anything of its own. This is not cosmetic on Git for Windows:
`/etc/profile` is what puts `/mingw64/bin` on the path, so a shell that skipped it is a Git Bash
that cannot find git. The chain is a pinned test (`crates/bt-term/tests/shell_integration_bash.rs`),
and `PATH`, `MSYSTEM` and `command -v git` were verified byte-identical to a plain `--login` shell.

Everything the script finds, it keeps: your `PROMPT_COMMAND` is called rather than replaced, an
existing `DEBUG` trap is chained rather than overwritten, and `PS1` is wrapped rather than rebuilt —
re-wrapped on every prompt, because a theme that regenerates `PS1` in its own `PROMPT_COMMAND`
(starship, powerline, and most prompt kits) would otherwise drop the markers after the first line.
No `OSC 0`/`OSC 2` title is emitted: a title set by the shell outranks the working directory in the
name stack, so a pane that announced itself once would stop following `cd`.

**WSL and `WSLENV`.** `wsl.exe` forwards no environment variable it was not told to, so
`BT_SHELL_INTEGRATION` and the `TERM_PROGRAM`/`TERM_PROGRAM_VERSION`/`COLORTERM` declarations every
other child already receives are listed in `WSLENV` — appended to whatever is already there, never
replacing it.

**Which shell, and which distribution.** `wsl.exe --list --verbose` names the installed
distributions and marks the default with `*`; that distribution is then asked for its own name and
its user's login shell (`getent passwd`). The init file is offered **only** when that shell is a
bash — a distribution logging into zsh or fish keeps its shell and goes without markers, which is
the fallback path below rather than a broken shell. When more than one distribution is installed,
the profile is titled `WSL · <default>` so the row says which one it starts; a machine with one
needs no qualifier and keeps the bare `WSL`.

**Command Prompt has no script and no hook — its whole integration is `PROMPT`.** Folio
reads whatever `PROMPT` this process inherited, puts the `OSC 7` report in front of it, and hands
the result to `cmd.exe`. Prefixed and never replaced: a `PROMPT` in the environment is a prompt
somebody wrote with `setx`, and a terminal that overwrote it would have taken their prompt away in
exchange for a directory they cannot see. An unset `PROMPT` gets `cmd`'s own documented default,
`$P$G`, spelled out — the moment we set the variable at all we owe the whole of it. And because a
`cmd` pane exports `PROMPT` to everything it starts, a report already in the inherited value is
left alone rather than prefixed a second time.

## What each profile actually gets

The honest matrix. Every "no" below is a shell's own limit, and every one of them lands on the
fallback path described under **Authority and fallback** rather than on a guess.

| | `133;A` | `133;B` | `133;C` | `133;D` + exit code | `OSC 7` | `OSC 0` title | `FORCE_HYPERLINK` | ↑ history |
|---|---|---|---|---|---|---|---|---|
| **PowerShell** (7, script installed) | yes | yes | yes | yes | yes | `PowerShell` | script | PSReadLine |
| **Windows PowerShell** (5.1, script installed) | yes | yes | yes | yes | yes | `Windows PowerShell` | script | PSReadLine |
| **either PowerShell** (script not installed) | no | no | no | no | no | — | no | PSReadLine |
| **Git Bash** | yes | yes | yes | yes | yes | none, deliberately | yes | bash's own |
| **WSL** (bash login shell) | yes | yes | yes | yes | yes | none, deliberately | yes, via `WSLENV` | bash's own |
| **WSL** (zsh/fish login shell) | no | no | no | no | no | — | set, but not forwarded | that shell's own |
| **Command Prompt** | **no** | **no** | no | no | **yes** | refused — see below | yes | not promised |
| **a profile of the reader's own**, no door | no | no | no | no | no | — | yes | not promised |

Six rows need their reasons stated, because each looks like an omission and is not.

**The two PowerShells are two profiles** (user ruling 2026-08-11), which is Windows Terminal's own
arrangement and what a machine with both installed makes necessary: 7 and 5.1 are different
products with different language versions, and one row could only ever start one of them while
claiming to be both. `PowerShell` resolves to `pwsh.exe` and to nothing else — `BT_SHELL` still
overrides it (Q4), and a machine without an install gets that row **greyed** rather than quietly
started as 5.1. `Windows PowerShell` names `%SystemRoot%\System32\WindowsPowerShell\v1.0\
powershell.exe`, ships inside the OS, and is therefore the profile every fallback lands on:
`profiles::FALLBACK_PROFILE` moved to it, because a floor that can be greyed is a fallback chain
with a hole in the bottom.

They share the one PowerShell mark. The mock-up draws a single PowerShell symbol and inventing a
second would assert a visual distinction the family does not have; identity here is the mark *and*
the title (`docs/UI-UX.md` §126-137), and the titles already differ. What the script emits as its
`OSC 0` now differs too, and has to: Folio drops a title that only repeats the profile's
own name — a shell agreeing with its launcher has announced nothing — so `folio.ps1` names
the edition (`$PSVersionTable.PSEdition`), and a 5.1 session that still called itself `PowerShell`
would prefix every pane head in its tab with its own family name.

Sessions saved before the split keep the slug `"pwsh"` untouched: it meant "the user asked for
PowerShell" and still does, with resolution now stricter. Sessions old enough to have stored an
executable **path** (v1–v5) are split by that path — `pwsh.exe` → `pwsh`, `powershell.exe` →
`winps` — because the path is the surviving record of which of the two actually ran, and folding
both onto one slug would spend it.

**Command Prompt sends no OSC 133 at all**, and this overturns the ruling of 2026-08-11 (Q5) that
allowed it `A` and `B`. That ruling assumed `A` and `B` are two more facts and that missing `C`/`D`
costs only what `C`/`D` would have bought. They are not facts — they are a claim of *authority*,
and this implementation charges for it in `C`:

* `133;A` alone turns `shell_integration_is_authoritative` on for the screen, and that flag's job
  is to **retire the cursor-line heuristic** — the rule that the line under the cursor is probably
  still being typed and must not be decorated yet. Its replacement is the semantic input region,
  which only `B` and `C` build. A shell that sends `A` and stops has switched the protection off
  and put nothing in its place: a path typed at a `cmd` prompt would light up as a link mid-word.
* `133;B` opens an input region whose only closers are `C` and the *next* `A`. Without `C` it stays
  open across the command's entire run, so the resize gate reads the command's own output as an
  unsent buffer: the window cannot be resized for as long as anything is printing, and every resize
  that does land owes an `InvokePrompt` chord to a shell that has no such binding.

Both are strictly worse than sending nothing, and sending nothing is a documented, tested position.
The measurement is pinned at `a_prompt_that_can_never_send_c_must_not_send_a_or_b_either` — if that
test ever goes red the reason has expired and the decision should be revisited rather than
inherited. What `cmd.exe` cannot do at all is the `C`/`D` pair itself: `PROMPT` is expanded once,
just before a line is read, and the shell has no pre- or post-execution moment to be called at.
Clink would supply both and is **not** required (Q5's surviving half); detecting it and upgrading
the row is booked, not done.

**Command Prompt's `OSC 0` is refused rather than absent.** `cmd.exe` calls `SetConsoleTitle` with
its own image path on the way up, and ConPTY forwards it as `ESC ]0;C:\WINDOWS\System32\cmd.exe`.
A program title outranks the working directory in the name stack, so without intervention every
Command Prompt tab is *called* `C:\WINDOWS\System32\cmd.exe` and stays called that after `OSC 7`
finally gives it a real directory. A shell reading its own command line back has not named itself,
it has repeated what this terminal wrote — so a title equal to the pane's own resolved program is
dropped before the name stack sees it (`LeafSession::announced_title`). This is the rule the pane
head already applied to a title equal to the profile's *display name*, said in the other
vocabulary. `title Build` in that same pane is kept.

**`FORCE_HYPERLINK` is a fact about the terminal, and is now stated as one.** It is the
`supports-hyperlinks` convention that half the Rust CLI ecosystem asks before it will emit `OSC 8`,
and its default answer is a guess made from `TERM` and a list of known terminal names this one is
not on. It renders `OSC 8`, so the answer is yes. Until now the only thing that said so was line 16
of `folio.ps1`, which made a capability of the terminal a property of one profile's
*opt-in* script: links worked in a PowerShell whose owner had installed the script, and in no other
pane in the window. It is now declared by the profile system — which is exactly the "per-profile
environment override mechanism" the ruling at `docs/M2-persistence-schema-v1.md` §296-299 deferred
this to, so **R-d is settled**. PowerShell alone still gets it from its script, because saying it
twice would be two places to change and one silently redundant. Any inherited value is left alone,
`0` included: this is a declaration, not an override, and someone who set it has already answered.
Across the WSL boundary the name is listed in `WSLENV` whether or not this process set it, so the
user's own answer travels too — but only on the path that has an init file, so a distribution
logging into zsh gets the variable on the Win32 side and nothing in the distribution, which is the
same boundary `TERM_PROGRAM` already stops at.

**A profile's own environment is the last word, and it can change this table.** The three layers
are: what this window inherited, then what this terminal declares (`TERM_PROGRAM`,
`TERM_PROGRAM_VERSION`, `COLORTERM`, `TERM`, the `FORCE_HYPERLINK` declaration above, `PROMPT` for
`cmd` and `BT_SHELL_INTEGRATION` for a bash), then the rows of the profile's own `env` — which
therefore **win**, `TERM_PROGRAM` included. A profile's environment is the most specific sentence
anybody says about its sessions, and the rule this page already states about `FORCE_HYPERLINK` —
whoever set the variable has answered the question — is the same rule one layer up. `BT_SHELL`
surviving as a debugging back door says the same thing: this machine belongs to the person using
it. Two of those rows change a row of the matrix, and the settings page derives its sentence from
both rather than repeating a promise this build would not keep: `FORCE_HYPERLINK=0` takes the
hyperlink column away, and on either PowerShell a `TERM_PROGRAM` override does too, because
`folio.ps1` declares links only for a session whose `TERM_PROGRAM` it recognises as this
terminal's. An empty value **takes the variable away** from that profile's sessions, including one the window
itself inherited: Windows removes an environment-block entry whose value is empty rather than
binding the name to the empty string, so `FOO=` in a profile means "no `FOO` here". That is the
operating system's answer and is left to it rather than filtered, and it is also what a reader who
cleared a value box meant, so the storage needs no third state. A row with an empty *name* is not a
variable and never reaches a child.

**Which door serves a profile is derived from its program, and may be named outright.** `Auto` —
what every shipped profile carries — reads the program's file name: `pwsh`/`powershell` take the
PowerShell script, `bash`/`sh`/`zsh`/`wsl` take the init file, `cmd` takes the `PROMPT` variable,
and anything else takes no door at all. That derivation reproduces the five doors above exactly
(`auto_derives_the_door_every_shipped_profile_has_always_had`), which is why the rows carry the
rule rather than five constants standing beside it. A profile of the reader's own running an
arbitrary executable is the case the last answer exists for: `--init-file` handed to a program that
is not a bash is a filename it will try to open. Nothing about the degradation is new — a screen
that never sees OSC 133 keeps the cursor/WRAPLINE heuristics byte for byte, and a session that
never sees OSC 7 leaves the relative path undetected rather than guessing a directory.

**A WSL profile's own variables are listed in `WSLENV` so that they cross.** A variable set on
`wsl.exe` is set on a *Win32* process, and the distribution behind it sees nothing that was not
named in `WSLENV`; a stored row that never reached the shell it was aimed at would be honoured by
every check except the only one that matters. The names are listed `/u` — Win32 to WSL, value
carried verbatim — because that is what they are: values, and this terminal has no way to know that
one of them holds a path wanting translation. A reader who wants something else writes their own
`WSLENV` row, and the layering above lets it win. This is the reader's instruction and crosses
whatever the login shell turns out to be, which is why the zsh row above still says "set, but not
forwarded" about *this terminal's* five: those are listed by the install path alone and that has
not changed.

**"↑ history" is not ours to promise.** The `DESIGN.md` §7.1.4 wording is limited to a profile
where PSReadLine is detected with persistent history enabled. `cmd`'s recall is `doskey`'s and dies
with the process; bash's is its own `HISTFILE`. Neither is a mechanism this terminal controls, so
the UI does not promise either.

## Default shell and `BT_SHELL`

Ruling (2026-08-04, evidence-backed): PowerShell 5.1 ships PSReadLine 2.0.0 (2020), whose stale
render anchor corrupts an unsubmitted wrapped input line whenever the pane narrows — reproduced in
Folio itself and in Windows Terminal, while PowerShell 7's PSReadLine 2.4.5 is clean in
both. Modern terminals already default to `pwsh` when it is present, so Folio does too.

`PtySession::spawn_default` picks the shell to launch in this order:

1. **`BT_SHELL`**, if set to a non-empty value, wins outright. Its value is used verbatim as the
   child process's program — either a full path (`C:\Tools\pwsh-preview\pwsh.exe`) or a bare
   executable name (`pwsh`, resolved against `PATH` by the OS at spawn time, the same way any
   other program name is). The value is never checked for existence up front.
2. Otherwise, **PowerShell 7** (`pwsh.exe`) is used if it can be found: first by searching `PATH`,
   then at the traditional MSI/`winget` install location
   (`%ProgramFiles%\PowerShell\7\pwsh.exe`), then at the Microsoft Store app-execution alias
   (`%LocalAppData%\Microsoft\WindowsApps\pwsh.exe`). All three are real filesystem/`PATH` probes,
   not assumptions — PowerShell 7 can land through any one of those three install paths and only
   the first reliably ends up on `PATH`.
3. Otherwise, **Windows PowerShell 5.1** (`powershell.exe`) is the default, exactly as before this
   ruling.

Whichever program is picked is launched the same way `spawn_default` always has: `-NoLogo` plus
Folio's usual `TERM_PROGRAM`/`COLORTERM`/`TERM` declarations. If the resolved shell fails
to start — a bad `BT_SHELL` override, a `pwsh.exe` resolved from a stale `PATH` entry that no longer
exists, or a profile whose program has been uninstalled — Folio falls back to
`powershell.exe` once and **writes a one-line banner into the pane's own first line** instead of
failing the session outright, naming the program that would not start and the reason. The pane is
then that fallback profile in every respect: its mark, its name, and the `profile_id` written to
disk all say `pwsh`, because it is running one.

The banner is in the pane and not in the status line, which is what it used to be. That is
`docs/M2-restart-shell-contract.md` §3/§5#3's requirement — the substitution is never silent — and
§2's reading of what a banner *is*: content appended to the transcript rather than an event with a
channel of its own. It therefore scrolls with the shell's output, stays in the scrollback, and can
be copied; the status line was drawn on one frame of the focused pane and discarded, so a pane that
fell back while you were looking at another tab announced it to nobody.

If you are on Windows PowerShell 5.1 by choice (`BT_SHELL=powershell.exe`, or no PowerShell 7
install at all) and see stray/duplicated characters on a wrapped, unsubmitted input line right
after narrowing the window, that is the PSReadLine 2.0.0 defect above — `Install-Module
PSReadLine` (from the PowerShell Gallery) resolves it without changing anything else about your
profile.

## bash: Git Bash, WSL, and a hand-installed copy

`scripts/shell-integration/folio.bash` is what Folio injects, and it is also
installable by hand for any bash it does not start itself — a shell over ssh, a distribution whose
login shell you changed. Dot-source it as the last relevant line of `~/.bashrc`:

```bash
. "$HOME/folio.bash"
```

Loaded that way, `BT_SHELL_INTEGRATION` is unset and the script sources nothing on your behalf:
bash has already run your startup files, and running them again is exactly what the marker is there
to prevent. It requires bash (the `DEBUG` trap and `PROMPT_COMMAND` it installs are bash's), is
idempotent within one shell, and does nothing at all in a non-interactive one.

## PowerShell 7 and Windows PowerShell 5.1

The opt-in script preserves the prompt and PSReadLine implementation that exist when it is loaded,
then wraps them with standard A/B/C/D markers. Load prompt customizers first, and dot-source the
script as the final relevant line in `$PROFILE`:

```powershell
. 'D:\Developer\BetterTerminal\scripts\shell-integration\folio.ps1'
```

Restart PowerShell after editing the profile. The script requires PSReadLine and is idempotent within
one shell process. It works in both PowerShell 7 (`pwsh`) and Windows PowerShell 5.1
(`powershell.exe`). A `-NoProfile` shell, a profile blocked by execution policy, or a missing
PSReadLine installation does not emit markers and therefore uses fallback behavior.

### The old names still work

Both scripts were called `betterterminal.ps1` / `betterterminal.bash` before the product was named
Folio (2026-08-13). Those two filenames still exist beside the new ones, as one-line shims that
source their sibling, because the line that loads them lives in a file that belongs to you — your
`$PROFILE` or your `~/.bashrc` — and a rename is not a reason for somebody else's file to stop
working. They are transitional: point your own line at `folio.ps1` / `folio.bash` and the shims can
go. Nothing inside the terminal reads them; every injection path names the new file directly.

The environment variable moved with the name. `TERM_PROGRAM` is now `Folio` rather than
`BetterTerminal`, and the PowerShell script tests for the new value — a tool of your own that keyed
on the old string needs the same edit. The two halves are held equal by a test
(`shell_integration::tests::the_integration_script_knows_the_name_this_terminal_announces`), so they
cannot drift apart again.

The injection pattern follows the standard sequences documented by
[Windows Terminal](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration)
and the documented
[PSConsoleHostReadLine extension point](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psconsolehostreadline).

## Authority and fallback

Authority is scoped per screen. Once a primary or alternate screen emits any recognized OSC 133
marker, region markers replace the cursor/CUP/sticky-line heuristics on that screen. Malformed or
out-of-order markers recover conservatively: duplicate B does not move an open command start; A, C,
or D closes an unterminated command at that marker; C/D without B changes phase but invents no input
region. Both BEL and ST terminators and arbitrary PTY chunk boundaries are supported.

A screen that has never emitted OSC 133 is byte-for-byte on the existing fallback path. This is
important for Codex, Claude Code, and other nested/full-screen TUIs: an outer PowerShell B/C pair
describes only the lifetime of the TUI process, not its internal composer. When the TUI switches to
an unmarked alternate screen, Folio continues using the existing cursor/WRAPLINE/CUP
heuristics there.

## Accepted v1 trust boundary

OSC 133 is trusted terminal metadata, matching Windows Terminal and VS Code's FTCS compatibility
model. v1 does not add a nonce. A child process can forge markers, but the impact is limited to
decoration gating and command-region navigation metadata; it does not grant file, process, or shell
authority. A nonce or application-authenticated protocol remains a future hardening option.
