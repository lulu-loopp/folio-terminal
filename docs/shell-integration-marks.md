# Account integration marks, schema 2

Owner: `shell_integration`. `integration-marks.json` lives in Folio's resolved
data directory. Paths are absolute, resolved when a mark is written, not rebuilt
from a Documents-folder convention. PowerShell itself supplies its profile path.

```json
{
  "version": 2,
  "powershell_state": { "state": "enabled" },
  "powershell_profiles": [],
  "powershell_scripts": [],
  "psreadline_module_roots": [],
  "agent_config_roots": { "claude": [], "codex": [], "copilot": [] },
  "profile_refusals": []
}
```

Schema 1 is read as enabled and upgraded to 2 on the next write. Schema 2
requires `powershell_state`: either `{ "state": "enabled" }` or
`{ "state": "off", "by": "user", "at": "2026-09-20T12:34:56Z" }`.
The UTC timestamp is the first Off request, retained across removal retries.
Off is written before discovery/removal, even when a profile later refuses.
An explicit On or installation clears Off. Locations and other integrations'
fields survive every transition. Unknown states/actors, invalid dates and missing
schema-2 state are refused, just like unknown schema versions.

Each root/path array contains strings. `profile_refusals` contains objects with
`path` and `reason` strings: the most recent failed profile operations, retried
and replaced by the next operation. The other fields are historical locations,
including locations already cleaned. They are discovery candidates, not proof
that a mark still exists. Only exact file contents establish that fact.
`psreadline_module_roots` is read and never written since 2026-10-17: Folio no
longer installs PSReadLine into Documents, and the roots an earlier Folio
recorded there are where the Terminal page's removal row and the uninstall look
for its copy.
Discovery alone never adds a profile to `powershell_profiles`: migration/removal
records an exact owned mark before editing it; inaccessible candidates are kept
in `profile_refusals` for retry. A hand-written installation is left untouched
and suppresses the offer, but is not a Folio-written mark. Old records remain
readable and their historical paths remain discovery candidates, not ownership.
`enabled` permits upkeep; it is not evidence that Folio installed a mark. Startup
that finds only unowned profiles does not create a record merely for discovery.
An explicit Off still records the user decision, even with nothing to remove.

`powershell_scripts` supplies the exact literal operands older Folio versions
could write. It does not grant ownership of arbitrary lines mentioning a script.
The canonical APPDATA legacy line and its BetterTerminal relocation-failure
variant are always recognised. Literal legacy lines
are compared against recorded script paths and the current resolved script path;
there is no safe way to recover an unrecorded historical environment override
by guessing from a user's code.

T-B/T-C1 should extend these typed fields, retaining fields owned by other
integrations. Unsupported versions, unknown fields, malformed JSON, relative
paths, read-only files and reparse points are refused without rewriting them.
Use `profile_marks::lock` across read/modify/write; it is an OS file lock on
`integration-marks.lock`, released on close or process exit. There is no owner
count or executable identity for profile marks: they belong to the account.

Writes use `bt_persist::atomic_write`, as settings do. Installation/migration
records intent before changing a profile. A crash may leave an extra candidate,
but cannot leave a successfully written mark whose location was never recorded.
Refusals are written afterward. A record failure refuses the operation.

`shell_integration::remove_shell_integration` is the synchronous worker/CLI door
for T-C1. It returns each path and outcome, plus an exit code (0 done/unchanged,
1 refusal). It has no GUI, handoff, purge or running-instance policy. T-C1 owns
those outer policies. The settings switch uses the same function on a worker.

At start, the carry pass (`carry_pass::run_carry_steps`, 0.4.10 plan row 68)
brings Folio's own old `$PROFILE` lines forward: its last member,
`shell_integration::run_profile_lines_step` (row 68c), runs once per process,
and never before the agents' hooks have had their first real answer, so nothing
the window adopts waits for a PowerShell. The order at a start: the start's own
round comes before the program walk has answered, so the hooks have nothing to
read and this member runs nothing (it asks nothing, and stays owed); the walk's
answer — or the program lane's word that no walk will answer: a walk that
failed, a worker that ended, a request refused with no walk standing; never a
timer — begins the pass again, and that round runs the hooks, publishes them,
and then runs this member, whose answer is published on its own. Whether the
hooks had their answer is read as a round begins, so a walk that lands during a
round leaves this member to the round it begins. A walk still out has neither an
answer nor a refusal: the hooks and this member wait for it, as the hooks have
since row 67b, and its answer is used whenever it lands — the lane has no
deadline, so there is no timer. Nor does it start while another round is asked
for (a Copilot probe's answer, which the first-run card waits on; an Agents-page
open; a press): that round runs next and this member yields to it, starting in a
later round with nothing pending, so no round anybody waits for waits for a
PowerShell. During an update's trial it is
held back (`update_trial::Writer::ProfileMigration`) and runs at the commit. On
the pass's below-normal worker it observes each installed PowerShell (the
observation the Profiles page and the launch menu read; `-NoProfile`,
`-NonInteractive`, a hidden console and a five-second timeout, each executable
queried at most once per process and shared with pane discovery), then visits
the recorded profiles and the paths those editions named that are there. A start
that finds none — every Mac's — takes no lock, makes no
`integration-marks.lock`, and says nothing, not even the round's line; one that
finds a profile reads the record again under the record lock (which leaves an
empty `integration-marks.lock` in the data folder) and carries there. Each line
in one of the six forms Folio has written — the `$env:APPDATA` line of
0.1.0-0.4.2 under either root name, the double- and single-quoted literal lines
naming a recorded or the current script, and the two guarded lines — that is not
the guarded line for the account's own root is replaced by it. Recognition is by
whole lines, not by parsing PowerShell: a line that is exactly one of the forms
inside a `<# ... #>` block comment or a here-string is carried too, which in a
here-string changes the text of that string. Nothing else in the file changes,
no backup copy is made, and the record remembers the profile before it is
written. Each file the carry changed or left is said once each start in
`diagnostics.log` (`BT_SHELL_PROFILE <path>: ...`), for as long as it stays as
it is. Left as they are, and said: a line naming `folio.ps1` in no form of
Folio's; a profile Folio may not edit that holds one of its lines; every line of
an account whose record says Off (the person's decision); and every line when
the data root is outside the account's `APPDATA\Folio` and
`APPDATA\BetterTerminal`. The account's `APPDATA` is the one the PowerShells
were asked in (the account's current logon environment), never Folio's own: a
Folio started with another `APPDATA` — the release smoke's sealed run — has a
data root no line the account's PowerShell expands can name, and is refused (the
two roots are compared as one path whatever their case). A refusal is never
recorded, and discovery alone makes no record. Documents paths are never
assembled. `BT_POWERSHELL_PROFILE` replaces the entire candidate set, including
recorded paths, and suppresses real PowerShell discovery. A test build's member asks no PowerShell and reads no
account file.

Profile edits preserve UTF-8, UTF-8 BOM or UTF-16LE BOM and every existing line's
terminator. A BOM-less file that is not UTF-8 (a profile in the system's ANSI code
page, as Windows PowerShell's `Set-Content` writes one) is held byte for byte: no
code page is guessed, a line is Folio's when its bytes are the UTF-8 of one of its
forms, and every other byte is written back as it was (row 68c, R7). Removal
consumes only owned lines and their own terminators; blank lines and empty
profile files remain. Changed profiles get a byte-identical
dated backup before `bt_persist::atomic_replace_preserving`. Windows uses
`ReplaceFileW` without ignoring the replacement's own failure, and stages the
original Hidden/Archive/System flags before and after replacement (ReplaceFileW
sets Archive even when it was clear). **The flag steps are best effort on both
sides**: content is guaranteed and what the file carried is not, so a successful
replacement is never rolled back over an attribute. The recovery sibling is
retired on every exit except the one where it is the only copy of the original —
then it is kept and the reported error names it. After
`ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` the original is restored to its own name by
rename, falling back to a hard link where a rename is refused. Unix carries uid, gid,
mode and every extended attribute (quarantine, Finder tags, `user.*`) onto the
replacement before the rename; handing a file to another user is a privileged
call, so a refused `chown` leaves the replacement this process's rather than
failing a write, and an attribute that cannot be set is skipped. New files
use ordinary `atomic_write`. Hard-linked files (`nlink > 1`), symlinks/reparse
points (including ancestors), read-only/locked files and unsupported encodings
are refused. A concurrent external path replacement is still a narrow race;
this is not an exclusive transaction with arbitrary user programs.

The managed template has exactly two admissible roots, `Folio` and
`BetterTerminal`. The resolved account data root selects which is written:

```powershell
if (Test-Path -LiteralPath "$env:APPDATA\Folio\shell-integration\folio.ps1" -PathType Leaf) { . "$env:APPDATA\Folio\shell-integration\folio.ps1" } # Folio shell integration v1
```

The predicate is evaluated once and consumes its Boolean result. Missing files
never reach dot-sourcing, even with strict mode and terminating error preference.
`-LiteralPath` avoids wildcard interpretation of account-folder characters.
Installation and migration refuse a data root outside `APPDATA\Folio` and
`APPDATA\BetterTerminal`. The fallback root substitutes `BetterTerminal` for
both `Folio` path operands in the template above; the marker stays unchanged.
Both exact managed spellings are recognised and removed. Migration also updates
an owned managed line to the selected account root (for example after relocation).
No other root, quoting style, or appended comment is admitted.

T-B now writes the three `agent_config_roots` lists through
`attention_ownership::record`, under this same lock. Agent installation records
intent before updating its config, preserves `powershell_state`, and refuses
when the record cannot be safely extended. See [agent integration marks](agent-integration-marks.md)
for adapter ownership, take-over, and the T-C1 removal API.

## `integration-profile-files.json`, version 1

Owner: `shell_integration::profile_marks::ProfileFiles`. Beside `integration-marks.json` in the
same data directory, read and written under the same lock (`integration-marks.lock`). It records
what Folio's own `$PROFILE` writes brought into existence, written **before** each write it
describes, so a power loss leaves an entry naming something that is not there — which the next
removal clears — and never a file nobody knows is Folio's.

```json
{
  "version": 1,
  "profiles": [
    {
      "profile": "C:\Users\me\Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1",
      "edition": "windows_powershell",
      "created_file": false,
      "created_folders": [],
      "backup": "C:\Users\me\Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1.bak-20261005",
      "backup_sha256": "<64 hex digits>"
    }
  ]
}
```

- `edition` — `windows_powershell` or `powershell_7`: the edition that named this file its
  `$PROFILE.CurrentUserCurrentHost`, so a removal finds the file without asking that edition.
- `created_file` — no file stood there before Folio's write.
- `created_folders` — the folders Folio created for it, outermost first.
- `backup` and `backup_sha256` — the one copy of a file that was there, taken before Folio's first
  write into it, and the SHA-256 of the bytes it holds.

Every path is checked on reading, as `integration-marks.json`'s are: the profile is an absolute
`.ps1` with no `..`; each folder is an ancestor of it; the copy stands beside it and is named
`<profile>.bak-…`. A record that fails a check, an unknown field, or any version but 1 is refused,
and the removal that read it refuses with it, touching no profile. A removal that takes Folio's line
out (or finds none) deletes the copy while it still holds the recorded bytes, and deletes a file
Folio created when nothing but whitespace is left in it, then each created folder that is empty; an
entry left with nothing but its edition keeps that, and one with no edition is dropped.

**Why a file of its own**: `integration-marks.json` is read by the previous version as well, with
unknown fields refused (RULES §41: every mark keeps a format the previous version reads), so a field
added there would make an update's rollback refuse the record and leave the old build unable to
remove its own line. The previous version ignores this file; after a rollback it removes the line as
it always did, and the file and copy this record names are taken by the next removal of a version
that reads it.
