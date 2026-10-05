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

## What the exit code in `133;D` means

PowerShell answers "did that fail?" in two variables and neither is the answer. `$?` is the last
statement's success, and it is overwritten by the *next* statement — including any statement a
prompt runs. `$LASTEXITCODE` is the exit code of the last **native** command, and it is a
session-wide leftover: a cmdlet that fails never touches it, so it keeps answering for a program
that ran three commands ago. bash has neither problem — `$?` there *is* the status, and
`folio.bash` reads it as its first statement and puts it back for the user's own hook — so this
section is PowerShell's alone.

`folio.ps1` reads both as the first two statements of `prompt`, records what `$LASTEXITCODE` was
when the line was submitted, and derives the code in this order:

1. **The line was refused by the parser.** Nothing ran; the host printed a syntax error and that
   error is the whole of the command's output. Reported as `1`, because neither variable moved and
   neither can speak for it.
2. **`$LASTEXITCODE` moved since the line was submitted, and moved to something non-zero.** A
   native command in this line wrote it. This is the one signal no prompt code can launder, so it
   is consulted before `$?`, and it is why the baseline is taken in `PSConsoleHostReadLine` — the
   last moment before the host runs the line, and after any helper a prompt customizer shells out
   to while the user is still typing.
3. **`$?`.** True is `0`: the shell's own verdict that the command succeeded, including a command
   that succeeded behind an older native failure whose code is still lying in `$LASTEXITCODE`.
4. **`$?` is false.** The code reported is `$LASTEXITCODE` when that is non-zero, and `1` when it
   is zero or unset — a failing cmdlet, a failing pipeline, a terminating error, a command stopped
   with Ctrl+C. Ctrl+C is the one line of the table below that a harness cannot press, because
   stopping a pipeline needs a real console; it reaches this tier by the rule rather than by
   measurement, and Ctrl+C at an *empty* prompt leaves PSReadLine returning the empty string, which
   is the "not a command" row.

`(Get-History -Count 1).ExecutionStatus` is not in that list because it cannot be: it reads
`Completed` for a failing native command, a failing cmdlet and a `Write-Error` alike. History
records what was typed, not how it went.

| what happened | `133;D` |
|---|---|
| native command exits 3 | `3` |
| native command exits 0 | `0` |
| the same failing native command run twice | `3`, then `3` |
| native command exits 3 inside a pipeline | `3` |
| cmdlet fails, nothing native ran this session | `1` |
| **cmdlet fails after a native command exited 0** | **`1`** |
| cmdlet fails after a native command exited 3 | `3` — the residual below |
| cmdlet succeeds after a native command exited 3 | `0` |
| failing pipeline, `Write-Error`, `throw` | `1` |
| assignment or any statement that runs nothing native | `0` |
| a line the parser refuses | `1` |
| Enter on an empty line, a line of whitespace, a line that is only a comment | *no `C`, no `D`* |
| the prompt drawn again mid-line (a resize) | *no `D`* |

The last two rows are boundaries of the marker pair rather than of the arithmetic. `C` opens an
output region that only `D` closes, so it is owed only by a line that will actually run something:
the submitted text is handed to PowerShell's own parser, and an input that parses cleanly into no
statements gets no markers at all instead of a zero-length command in the ledger wearing the
previous command's status. And `D` is written only for a `C` that is still open, so PSReadLine
redrawing the prompt — which `InvokePrompt` does on every resize, at Folio's own request — reports
no status where it has none to report.

### Folio's prompt has to be the outermost one

Everything a prompt customizer runs overwrites `$?` before an inner prompt could read it. Conda is
the plain example: its `prompt` writes `(base) ` with `Write-Host` and then calls the prompt it
renamed out of the way, so a Folio nested inside it reads the success of `Write-Host` and reports
every command as `0`. Which way the two nest is decided by nothing better than which profile file
each was written into — `conda init powershell` writes its hook into the **all-hosts** profile,
which loads *before* the per-host file that usually carries the `.` of this script.

So the script does not depend on the order. Installing it wraps whatever `prompt` is; and a
customizer that renames that function afterwards and puts its own in front is noticed by the next
draw, which takes the name back and keeps the customizer in the chain — the same discipline
`folio.bash` already applies to `PS1`, for the same reason. Both prompts keep their output, in
their order; only the outermost one reads the status and writes the markers.
`scripts/shell-integration/tests/exit-status.ps1` runs the whole table above in a real PowerShell
of each generation, three times over: this script alone, conda installed before it, conda installed
after it. `conda-order.ps1` pins the re-hoist itself: after the first draw Folio owns the global
`prompt` name again, with conda's wrapper and the reader's original prompt kept in its chain, and each
of them runs once per draw.

**The residue, stated.** A cmdlet that fails while an older native failure is still in
`$LASTEXITCODE` is reported with that older code — `3` where `1` was meant. Nothing distinguishes
it from the same native command being run again, and of the two readings the one that keeps a
retried failure's own code is worth more than the one that renumbers it; either way the verdict is
right and only the digits are borrowed, which is exactly what the reader would see if they typed
`$LASTEXITCODE` themselves. A customizer installed after this script gets **one** prompt — the one
that discovers it — reported with its laundered status. And a customizer that replaces `prompt`
without calling the function it displaced takes the markers away entirely; there is nothing left of
Folio in that session to notice, and no status is better than a wrong one.

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
`\\wsl.localhost\<distro>\home\weiyi` for a Linux home — a UNC whose authority a `file:` report is
obliged to reject as a remote share — so converting would make the most common directory in WSL
unreportable. A POSIX directory is stored and displayed as it is, and the pane translates it on
this side, in the one place that knows which distribution the pane is standing in
(`bt_transcript::paths::PrintedPathNamespace`, §7.30). That is what makes a name printed beside it
resolvable: since 2026-09-07 a WSL pane standing in `/home/weiyi` measures `docs/a.md` from there,
and `/etc/hosts` printed into it is the file Windows opens at `\\wsl.localhost\<distro>\etc\hosts`.
The wire format did not move, and neither did the rule about authorities: a report that *carried*
one would still be refused.

### Carrying a directory between profiles

A new tab opens where the shell you are looking at is standing, whenever the shell it starts can
name that place (`profiles::cwd_for_spawn`). Between the two namespaces the map is WSL's own drive
mounts, and it is not total:

| from → to | `D:\src` | `/mnt/d/src` | `/home/weiyi` |
|---|---|---|---|
| → Windows profile | `D:\src` | `D:\src` | *no answer* |
| → WSL | `/mnt/d/src` | `/mnt/d/src` | `/home/weiyi` |

The empty cell is the honest one: a directory inside the distribution is not a place a Windows
shell stands in — a `cmd.exe` handed a UNC working directory falls back to `C:\WINDOWS` and says
nothing about it — so the new tab starts at its own profile's starting directory rather than
somewhere it was not put. Same rule, same reason as an unreported directory: never guess one. This
is a question about *starting a process*, and it is not the one §7.30 answers about *reading a
file*, where `\\wsl.localhost\<distro>\…` is exactly the name that works.

## Injecting the script

PowerShell's script is process-scoped and automatic when the row can be composed. Folio owns a
refreshed copy below its data directory and composes the one PowerShell argv at the leaf-spawn
seam. A row with no terminal host switch receives exactly `-NoExit -Command <loader>`.
For an existing `Command` or strict base64/UTF-16LE `EncodedCommand`, a below-normal preparation
worker first asks the row's resolved PowerShell executable to parse the exact command text. A valid
answer appends `CRLF` and the loader inside that same terminal command; Folio preserves an existing
`NoExit` and never adds one to a terminal command. The exact executable and argv own the process
cache entry, so an edited row must answer a new question. File, CommandWithArgs, stdin,
NonInteractive, unknown or invalid argv, an unavailable parse answer, and an overlong Windows
command line are left unchanged. A failed probe is an unavailable answer rather than an invalid
command: a later birth may schedule the next background attempt, up to three attempts for the
exact executable and argv in one process, and the birth never waits. The Settings capability
sentence reads that same answer.

When a PowerShell row must instead be spawned as written (`File`, `CommandWithArgs`, stdin,
`NonInteractive`, or an unknown/invalid host line), Settings > Profiles offers one **Enable via
profile** button for that edition. One click uses the existing managed-line writer and backup to
add Folio's guarded line to `$PROFILE.CurrentUserCurrentHost`; no confirmation is interposed, and
the success toast offers Undo. The installed fact is per edition, so every such row for that
edition changes to the ordinary capability sentence together. The one existing **PowerShell
`$PROFILE` line** row remains the only lasting removal surface. No button is offered when the row
has any accepted `NoProfile` spelling, or when the execution policy that row would run under refuses
profile scripts. Nor is it offered before that edition's first profile/policy observation has
landed, or off Windows, where there is no profile probe. A missing profile file is created with only
the managed line.

The policy is read per row from the scope list the probe reports (`Get-ExecutionPolicy -List`), in
PowerShell's precedence order — MachinePolicy, UserPolicy, Process, CurrentUser, LocalMachine, then
the default — with the row's own `-ExecutionPolicy` (any spelling) as its Process scope. A refusing
policy is one of three cases, and the capability sentence says which:

* **Set by the user's own scopes** (CurrentUser, LocalMachine or the default decides, nothing above
  CurrentUser is set). `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, which needs no
  elevation, would decide the effective policy, so the row offers one **Copy** button in the place
  of the Enable button; it puts exactly that command on the clipboard and the window says what was
  copied. Folio never runs it.

Either button stands in the row only while the text column can spare it (at most the share of the
row a picker may take on any other page); at narrow dialog widths it is not drawn, and the row's
`⋯` menu, which offers the same verb first on every row that has one, is the way to it.
* **Set by the organisation** (MachinePolicy or UserPolicy, i.e. Group Policy). No command and no
  button: the user cannot change it.
* **Set for the process** (a refusing Process scope: the row's own `-ExecutionPolicy`, its last
  occurrence as both binders take it, or a `PSExecutionPolicyPreference` in the environment). It
  outranks CurrentUser, so the command would not help; the sentence says the policy set for this
  process refuses `$PROFILE`, with no button.

Two more cases offer nothing:

* **Where `$PROFILE` is stored.** `RemoteSigned` refuses an unsigned script from the Internet or
  Untrusted zone, and the two editions decide the zone differently (measured 2026-10-05 through
  real UNC paths and a Mark of the Web): PowerShell 7 keys on the file's Mark of the Web alone, so
  an unmarked profile on a redirected share loads; Windows PowerShell also maps the path's URL
  zone, so a profile on a share it places in the Internet zone (`\\host.example.com\…`) is
  refused. The probe asks the row's own edition for its answer — the zone function its
  authorization manager uses — so each row follows its edition's rule. When `RemoteSigned` is the
  policy that decides, or would decide once the command has run, and the edition says it would not
  load the profile from there (or gives no answer), the row says `$PROFILE` is stored where the
  policy may refuse it. A line already installed where the edition says it loads shows the
  ordinary "loaded via $PROFILE" sentence. The edition's zone function is internal; when an
  edition gives no answer, the same probe run's public facts decide what can still be known — an
  unmarked profile on a local fixed drive loads, so its row keeps the button; a marked file or a
  network or removable path gets the Location sentence — and diagnostics say once per edition
  that the fallback was used.
* **A row this build cannot read** (for example `-ep:Bypass`: the colon form is not an option to
  either binder) makes no claim about its policy and shows only the "not provided" sentence.

The default policy, used when no scope is set, is not a constant: the probe asks the edition for
its effective policy with the probe's own Process scope cleared.

After the user runs the command in any shell, the next visit to the Profiles page re-reads the
policy (the page-entry observation edge) and the row becomes the ordinary Enable offer; the Enable
click re-probes the policy for that row anyway. There is no polling. Opening the Profiles page starts the existing background
profile/policy observation once for that visit; leaving and returning starts another observation,
with no polling loop.

The PowerShell script, and only it, first verifies `TERM_PROGRAM=Folio`: Folio itself adds its line
to `$PROFILE`, and that line must be inert when the same profile is read in another terminal. A
nested PowerShell started inside a Folio pane inherits the declaration and is deliberately
integrated. The process-scoped launch paths set the same declaration, so this guard does not
distinguish automatic composition from the opted-in profile line. Known limit: a PowerShell session
on another host reached by ssh from a Folio pane, whose profile sources `folio.ps1`, is inert
unless the ssh connection forwards `TERM_PROGRAM` (`SendEnv` on the client and `AcceptEnv` on the
server); `TERM_PROGRAM` does not cross ssh by default.

The bash and zsh scripts are unchanged: they act wherever they are sourced. Folio never adds a line
to a bash or zsh rc file, and a hand-installed copy sourced from an rc file on a remote host reached
by ssh from a Folio pane, or in a `sudo -i` / `su -` shell inside a pane, must keep working there —
`TERM_PROGRAM` crosses neither ssh nor `sudo`'s `env_reset`. The sequences they emit are the
standard `OSC 133` and `OSC 7` that other terminals read or ignore.

The bash script is also installed automatically, for one session at a time:

* `bash --init-file <file>` names the startup file for one interactive shell and touches nothing on
  disk.

So a Git Bash profile is started as `bash --init-file <script> <the profile's own arguments, less
its login flag>`, with `BT_SHELL_INTEGRATION` in the environment naming the startup chain the
script owes. A WSL profile is started as

```
wsl.exe [--cd <dir>] -e sh -c '<the login-shell question>' folio /mnt/c/…/folio.bash /mnt/c/…/zdotdir
```

where the question is the script `bt_app::shell_integration::WSL_LOGIN_SHELL` holds: read the login
shell out of `getent passwd`; `exec` it with `--init-file` and `BT_SHELL_INTEGRATION=login` when it
is a bash; `exec` it with `-l` and `ZDOTDIR` pointed at the directory when it is a zsh, carrying the
reader's own `ZDOTDIR` in `BT_USER_ZDOTDIR`; and `exec` it with `-l` and nothing else when it is
anything else. Both paths travel as `$1` and `$2` rather than spliced into the text, so a reader
whose Windows account name has a space in it gets filenames the shell reads verbatim.

**The profile's own arguments are kept, and only the login flag is dropped.** `--init-file` names
the startup file of an interactive shell that is *not* a login shell, and a bash started with `-l`
does not read it at all — measured, not assumed — so the flag cannot travel beside it. Everything
else the row carries does. What stays undone is `shopt login_shell`, which is off in a pane whose
profile asked for a login shell: a startup file that branches on it takes the non-login branch.

**zsh has no `--init-file`, so zsh has a different door.** Folio writes `folio.zsh` under three
names — `.zshenv`, `.zprofile`, `.zshrc` — into `%APPDATA%\Folio\shell-integration\zdotdir\` and
points `ZDOTDIR` at it. Each of the three sources your file of the same name and nothing else, and
`.zshrc` hands `ZDOTDIR` back at the end of itself, so `.zlogin` and every zsh started from the
session read your own directory with no trace of the arrangement left in the environment. If you
keep your startup files somewhere other than `$HOME`, that directory reaches the script in
`BT_USER_ZDOTDIR`, because `ZDOTDIR` itself has already been taken by the time zsh reads a line.

**`sh` and `dash` get no door at all.** They accept `--init-file` and ignore it in silence, which is
the one failure indistinguishable from a shell that has no integration, so the profile says so
outright and the capability row reads `No shell integration`.

**Whether a row is a login shell is the row's own switch** (`login` in `profiles.json`,
`Login shell` in Settings > Profiles; 2026-09-26, issue #12). Folio spells it on the command line
the way each shell's manual does — `--login` for bash, `-l` for zsh, `sh`, `dash` and the
rest — in front of the row's own arguments, and never as a `-` in front of the program's name.
**On macOS the shipped `zsh`, `bash` and `sh` rows are login shells**, as in Terminal.app,
iTerm2, Kitty and WezTerm, because Homebrew's installer puts `PATH` in `~/.zprofile`, which
only a login shell reads. On Linux the shipped rows are not, which is what its terminals do. Each
row goes through its door as above: a login zsh is started `zsh -l` with `ZDOTDIR` pointed at
Folio's copy, whose `.zprofile` sources yours; a login bash trades `--login` for the init file
with `BT_SHELL_INTEGRATION=login`; a login `sh` is `sh -l`.

`-e` rather than `--`, and it is load-bearing: `wsl.exe --` joins everything after it into one
command line and gives *that* to the login shell, which re-parses it — a question full of spaces,
quotes, `$`, `|` and `;` comes apart on the way in. `wsl.exe -e` executes the program directly,
argv for argv, which is also what makes handing the init file over as an argument work. Measured on
Ubuntu-24.04, 2026-09-07.

The script is written out to `%APPDATA%\Folio\shell-integration\` from a
copy compiled into the binary, so the two halves of the OSC 133 agreement always ship together.

**What `--init-file` costs, and how it is paid back.** It replaces `~/.bashrc`, and because bash
consults it only for a shell that is *not* a login shell, Folio also drops the `--login`
that Git Bash's own shortcut passes. The script therefore runs the startup chain itself — **the one
the profile asked for**, which is what `BT_SHELL_INTEGRATION` names. `login` is bash's documented
order for a login shell: `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login`,
`~/.profile`, and never `~/.bashrc`. `interactive` is bash's order for a shell that is not one:
`~/.bashrc` and nothing else. Which is which is a fact about the profile's own arguments, and only
the Windows side can read them. This is not cosmetic on Git for Windows:
`/etc/profile` is what puts `/mingw64/bin` on the path, so a shell that skipped it is a Git Bash
that cannot find git. The chain is a pinned test (`crates/bt-term/tests/shell_integration_bash.rs`),
and `PATH`, `MSYSTEM` and `command -v git` were verified byte-identical to a plain `--login` shell.

Everything the script finds, it keeps: your `PROMPT_COMMAND` is called rather than replaced — as a
string when it is one, and as a *list* when it is bash 5.1's array, where the script's two halves
are prepended and appended rather than assigned over the first element — an existing `DEBUG` trap is
chained rather than overwritten, read at the first prompt because that is the only place bash will
answer `trap -p DEBUG` truthfully, and `PS1` is wrapped rather than rebuilt —
re-wrapped on every prompt, because a theme that regenerates `PS1` in its own `PROMPT_COMMAND`
(starship, powerline, and most prompt kits) would otherwise drop the markers after the first line.
No `OSC 0`/`OSC 2` title is emitted: a title set by the shell outranks the working directory in the
name stack, so a pane that announced itself once would stop following `cd`.

**WSL and `WSLENV`.** `wsl.exe` forwards no environment variable it was not told to, so the
`TERM_PROGRAM`/`TERM_PROGRAM_VERSION`/`COLORTERM`/`FORCE_HYPERLINK` declarations every other child
already receives are listed in `WSLENV` — appended to whatever is already there, never replacing it.
`BT_SHELL_INTEGRATION` is **not** among them: it is set inside the distribution, by the one branch of
the question that reads the init file, because it is the only one of these whose meaning depends on
which shell was started. A zsh session carrying it for its whole life would tell every nested `bash`
that somebody had already run its startup files.

**Which shell, and which distribution — two questions, answered in two places.** Which
distributions are installed and which one `wsl.exe` starts is a fact about *Windows*: it is read out
of `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss`, synchronously, and costs microseconds
(`DESIGN.md` §7.40 ②). Which shell that distribution logs the user into is a fact about a *Linux
user account*, held only by the distribution's own password database — so it is asked by the pane
that needs the answer, inside the distribution, in the same command line as the shell it decides.

**Every WSL pane is integrated, the first one of a process included** (2026-09-07). It was not
always: the login-shell question used to be put by a second `wsl.exe` armed from the pane spawn and
never waited for, so the first WSL pane of every run composed its command line before the answer
existed, was started as a bare `wsl.exe --cd <dir>`, and reported neither `OSC 133` nor `OSC 7` —
while every WSL pane after it in the same process was served in full. On a machine whose default
profile is WSL that first pane is the only one there is. Measured in
`docs/plans/shell-matrix-2026-09-07.md` T-2 and fixed by moving the question into the pane, which
leaves nothing in flight for a spawn to race. Each shell is offered **only** the door it has — the
init file to a bash, `ZDOTDIR` to a zsh, and nothing to a fish or a shell somebody built themselves,
which keeps its shell and goes without markers on the fallback path below rather than being replaced
by one the reader did not choose. All of that is decided where the answer is, rather than from an
answer that may not have arrived.

When more than one distribution is installed, the profile is titled `WSL · <default>` so the row
says which one it starts; a machine with one needs no qualifier and keeps the bare `WSL`.

**Command Prompt has no script and no hook — its whole integration is `PROMPT`.** Folio
reads whatever `PROMPT` this process inherited, puts the two command-boundary markers and the
`OSC 7` report in front of it, and hands the result to `cmd.exe`. The report sits immediately
before `133;A`, which is `folio.bash`'s own order, so the two doors report at the same point of the
cycle. Prefixed and never replaced: a `PROMPT` in the environment is a prompt
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
| **either PowerShell** (script not installed or argv declined, profile fallback off) | no | no | no | no | no | — | no | PSReadLine |
| **Git Bash** | yes | yes | yes | yes | yes | none, deliberately | yes | bash's own |
| **WSL** (bash login shell) | yes | yes | yes | yes | yes | none, deliberately | yes, via `WSLENV` | bash's own |
| **WSL** (zsh login shell) | yes | yes | yes | yes | yes | none, deliberately | yes, via `WSLENV` | zsh's own |
| **zsh** (on Windows, MSYS2 or similar) | yes | yes | yes | yes | yes | none, deliberately | yes | zsh's own |
| **WSL** (fish or another login shell) | no | no | no | no | no | — | yes, via `WSLENV` | that shell's own |
| **`sh` or `dash`** | no | no | no | no | no | — | yes | that shell's own |
| **Command Prompt** | **yes** | **no** | no | **yes, no code** | **yes** | refused — see below | yes | not promised |
| **a profile of the reader's own**, no door | no | no | no | no | no | — | yes | not promised |

Several rows need their reasons stated, because each looks like an omission and is not.

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

**Command Prompt marks its prompts and not its input** (2026-09-07). `PROMPT` is not a hook, it is
a format string, and `cmd.exe` expands it at exactly one moment: just before it reads a line. That
moment is the end of the last command and the start of this prompt at once, so `133;D` and `133;A`
are precisely the two markers it can carry, and Folio sets

```
PROMPT=$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\<the prompt you already had>
```

`D` carries no status: `PROMPT` has no substitution that reads `ERRORLEVEL`, and a tick coloured
from a number nobody reported would be worse than a tick with no colour. `133;C` has nowhere at all
to be emitted from — there is no pre-execution moment in this shell.

**`133;B` is still refused, and for the half of the 2026-08-16 measurement that has not expired.**
That measurement found `A` and `B` to be a claim of *authority* rather than two more facts, charged
for in a `C` this shell cannot send, and it was right about `B`: the marker opens an input region
whose only closers are `C` and the *next* `A`, so without `C` every row a `cmd` command printed
would sit inside the region that means "this is what the reader is typing" — no display mathematics
and no image previews. So `cmd` panes go on decorating their output exactly as they always have.

The third item that stood in this list — the post-resize `InvokePrompt` chord, owed to a shell with
no such binding — has been struck out, and the reason it went is a defect it caused elsewhere. The
chord is a key `folio.ps1` binds, and it was sent to any pane whose input region was open on the
reading that a shell without the binding would drop it. GNU readline does not: it decodes what it
recognises of `CSI 24;8~` and inserts the rest, so a single resize typed `;8~` into a Git Bash or
WSL prompt and the next command died on a syntax error. The chord is gated on the profile's door
now (`psreadline_resize_repaint_input`), so no shell but a PowerShell can be sent it whatever its
markers say. `B` is still refused here for the first reason alone.

What has expired is the other half — that `A` alone was worse than silence, because it retired the
cursor-line heuristic without building the region that replaces it. That is now false of the
implementation rather than argued around: authority to retire the heuristic is claimed by the
markers that build the region (`B`, `C`) and by no others, so a screen carrying only `A` and `D`
keeps the heuristic it always had. The old measurement named this as its own exit — "if that test
ever goes red the reason has expired" — and the new one is
`a_prompt_only_shell_gets_its_ticks_and_keeps_the_cursor_heuristic`, beside a round trip through a
real `cmd.exe` in `crates/bt-term/tests/shell_integration_cmd.rs`.

What it buys is the capability this profile's reader has no other way to get. Every other profile
can be handed a script; `cmd` cannot, and until now its command rail was empty however many commands
had been run — nothing to click, and `Ctrl+Shift+↑`/`↓` with nowhere to go. Clink would supply `C`
and a real exit code both, and is still **not** required (Q5's surviving half); detecting it and
upgrading the row is booked, not done.

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
user's own answer travels too — and since 2026-09-07 it travels whichever shell the distribution
logs into. It used to be listed only where this side had established the shell was a bash, which
made a fact about what this *terminal* renders a property of the reader's choice of shell: a zsh
pane drew the same hyperlinks as any other and told the programs in it that it did not. Which shell
answers is the pane's own business now, so the listing asks the question it was always really
asking — is this pane crossing into WSL.

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
whatever the login shell turns out to be — as, since 2026-09-07, do this terminal's own four. The
one WSL pane that lists nothing is the one handed no script at all, which is the pane that was never
injected into.

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

## zsh

`scripts/shell-integration/folio.zsh` is what Folio writes into the `ZDOTDIR` it hands a zsh pane,
and it is installable by hand for any zsh it does not start itself. Dot-source it as the last
relevant line of your own `~/.zshrc`:

```zsh
. "$HOME/folio.zsh"
```

Loaded that way it installs its `precmd` and `preexec` hooks through `add-zsh-hook`, so a prompt kit
already in those lists keeps running, and it sources nothing on your behalf. It requires zsh, is
idempotent within one shell, and does nothing at all in a non-interactive one.

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

The process loader preserves the prompt and PSReadLine implementation that exist when it runs, then
the owned script wraps them with standard A/B/C/D markers. The script is idempotent within one shell
process and works in both PowerShell 7 (`pwsh`) and Windows PowerShell 5.1 (`powershell.exe`). It
returns before doing anything unless `TERM_PROGRAM` is exactly `Folio`; nested PowerShell sessions
inside Folio inherit that value and are intentionally integrated. It
requires PSReadLine. PowerShell 7 may revive only its already-loaded in-box PSReadLine assembly when
execution policy prevented module registration; Windows PowerShell does not take that path. A
missing PSReadLine installation, non-FullLanguage session, refused argv, failed preparation or
unreadable owned script emits no marks and uses fallback behavior. No execution policy is changed.

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

Authority is scoped per screen, and it is claimed by the markers that build a region rather than by
any marker at all. Once a primary or alternate screen emits `133;B` or `133;C`, region markers
replace the cursor/CUP/sticky-line heuristics on that screen. A screen that has emitted only `133;A`
and `133;D` — which is every Command Prompt pane — has said where its commands begin and end and
nothing about where a typed line stops, so it keeps those heuristics: the claim and the machine that
honours it arrive together or not at all. Malformed or
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
