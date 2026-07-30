# OSC 133 shell integration

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
