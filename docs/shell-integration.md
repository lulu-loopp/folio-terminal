# OSC 133 + OSC 7 shell integration

BetterTerminal treats FinalTerm Command Status (FTCS) `OSC 133` markers as the authoritative
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

Region endpoints use BetterTerminal content anchors. Normal scrolling migrates them atomically from
live grid to staging to transcript. Before a resize, a live region also captures its exact displayed
command text; after vendor reflow, that content witness re-seats its endpoints on the new physical
rows. Resize therefore changes only projection, not input/output ownership.

## OSC 7: the authoritative working directory

The same script also emits `OSC 7` once per prompt, immediately before `133;A`:

```
ESC ] 7 ; file:///<percent-encoded $PWD> BEL
```

This is the standard Windows Terminal / iTerm convention and it is the **only** way BetterTerminal
learns where a session's output is being printed from. It exists to resolve relative image path
text (`./shot.png`, `../a/b.svg`, and bare references carrying a separator such as
`local-images/sunset.svg`) — see `docs/M2-preview-matrix-and-verbs.md` §6.3. A session that
never receives OSC 7 leaves relative paths undetected rather than guessing a directory, exactly as
a screen that never emits OSC 133 keeps the cursor/WRAPLINE heuristics.

The authority is empty (the file-URI spelling of "this host"); BetterTerminal also accepts
`localhost` and this machine's own name, and rejects every other authority as a remote share. The
path is percent-encoded minimally: UTF-8 byte by byte, keeping RFC 3986 unreserved characters,
sub-delims, `:`, `@` and `/`. The directory is stored per session and survives primary/alternate
screen switches, because a working directory belongs to the shell process and the full-screen TUI
it launched inherits it.

A location on a non-filesystem provider (`HKLM:`, `Cert:`, …) emits an **empty** report, which
retracts the previous directory. An unresolvable report — a remote share, a malformed URI, a
truncated one — clears the stored directory for the same reason: leaving a stale directory to
answer for a place the shell has left is the guess the ruling forbids.

## Default shell and `BT_SHELL`

Ruling (2026-08-04, evidence-backed): PowerShell 5.1 ships PSReadLine 2.0.0 (2020), whose stale
render anchor corrupts an unsubmitted wrapped input line whenever the pane narrows — reproduced in
BetterTerminal itself and in Windows Terminal, while PowerShell 7's PSReadLine 2.4.5 is clean in
both. Modern terminals already default to `pwsh` when it is present, so BetterTerminal does too.

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
BetterTerminal's usual `TERM_PROGRAM`/`COLORTERM`/`TERM` declarations. If the resolved shell fails
to start — a bad `BT_SHELL` override, or a `pwsh.exe` resolved from a stale `PATH` entry that no
longer exists — BetterTerminal falls back to `powershell.exe` once and shows a one-line notice in
the status line instead of failing the session outright.

If you are on Windows PowerShell 5.1 by choice (`BT_SHELL=powershell.exe`, or no PowerShell 7
install at all) and see stray/duplicated characters on a wrapped, unsubmitted input line right
after narrowing the window, that is the PSReadLine 2.0.0 defect above — `Install-Module
PSReadLine` (from the PowerShell Gallery) resolves it without changing anything else about your
profile.

## PowerShell 7 and Windows PowerShell 5.1

The opt-in script preserves the prompt and PSReadLine implementation that exist when it is loaded,
then wraps them with standard A/B/C/D markers. Load prompt customizers first, and dot-source the
script as the final relevant line in `$PROFILE`:

```powershell
. 'D:\Developer\BetterTerminal\scripts\shell-integration\betterterminal.ps1'
```

Restart PowerShell after editing the profile. The script requires PSReadLine and is idempotent within
one shell process. It works in both PowerShell 7 (`pwsh`) and Windows PowerShell 5.1
(`powershell.exe`). A `-NoProfile` shell, a profile blocked by execution policy, or a missing
PSReadLine installation does not emit markers and therefore uses fallback behavior.

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
an unmarked alternate screen, BetterTerminal continues using the existing cursor/WRAPLINE/CUP
heuristics there.

## Accepted v1 trust boundary

OSC 133 is trusted terminal metadata, matching Windows Terminal and VS Code's FTCS compatibility
model. v1 does not add a nonce. A child process can forge markers, but the impact is limited to
decoration gating and command-region navigation metadata; it does not grant file, process, or shell
authority. A nonce or application-authenticated protocol remains a future hardening option.
