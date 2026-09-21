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

Startup checks for a marks record or an existing installed PowerShell script
(the pre-record evidence of opt-in). With neither, it starts no migration worker
and launches no PowerShell query. Otherwise a below-normal worker visits recorded
and currently probed profiles. Before any discovery, script repair or record
write, the worker reads the account state under the record lock. Off returns
immediately: zero PowerShell processes, zero script writes, zero record writes.
Reading the record stays off the window thread; an Off account still has the
short worker needed to read that authoritative state. The lock also covers
script repair, so removal cannot be followed by a stale startup repair. Each resolved PowerShell executable is queried at
most once per process, shared with pane discovery. Queries use `-NoProfile`,
`-NonInteractive`, a hidden console and a five-second timeout. Documents paths
are never assembled. `BT_POWERSHELL_PROFILE` replaces the entire candidate set,
including recorded paths, and suppresses real PowerShell discovery.

Profile edits preserve UTF-8, UTF-8 BOM or UTF-16LE BOM and every existing line's
terminator. Removal consumes only owned lines and their own terminators; blank
lines and empty profile files remain. Changed profiles get a byte-identical
dated backup before `bt_persist::atomic_replace_preserving`. Windows uses
`ReplaceFileW` without ignoring metadata/ACL merge errors, and stages the original
Hidden/Archive/System flags before and after replacement (ReplaceFileW sets
Archive even when it was clear). If restoring flags fails, the original object
is restored from the recovery sibling; if rollback also fails, its path is
reported and retained. The same recovery sibling also retains the original
object if the native operation fails after moving it. Unix carries uid, gid,
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
