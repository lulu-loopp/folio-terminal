# Account integration marks, schema 1

Owner: `shell_integration`. `integration-marks.json` lives in Folio's resolved
data directory. Paths are absolute, resolved when a mark is written, not rebuilt
from a Documents-folder convention. PowerShell itself supplies its profile path.

```json
{
  "version": 1,
  "powershell_profiles": [],
  "powershell_scripts": [],
  "psreadline_module_roots": [],
  "agent_config_roots": { "claude": [], "codex": [], "copilot": [] },
  "profile_refusals": []
}
```

Each root/path array contains strings. `profile_refusals` contains objects with
`path` and `reason` strings: the most recent failed profile operations, retried
and replaced by the next operation. The other fields are historical locations,
including locations already cleaned. They are discovery candidates, not proof
that a mark still exists. Only exact file contents establish that fact.

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
and currently probed profiles. Each resolved PowerShell executable is queried at
most once per process, shared with pane discovery. Queries use `-NoProfile`,
`-NonInteractive`, a hidden console and a five-second timeout. Documents paths
are never assembled. `BT_POWERSHELL_PROFILE` replaces the entire candidate set,
including recorded paths, and suppresses real PowerShell discovery.

Profile edits preserve UTF-8, UTF-8 BOM or UTF-16LE BOM and every existing line's
terminator. Removal consumes only owned lines and their own terminators; blank
lines and empty profile files remain. Changed profiles get a byte-identical
dated backup before atomic replacement. Symlinks/reparse points (including
ancestors), read-only/locked files and unsupported encodings are refused.

The only managed line written is:

```powershell
if (Test-Path -LiteralPath "$env:APPDATA\Folio\shell-integration\folio.ps1" -PathType Leaf) { . "$env:APPDATA\Folio\shell-integration\folio.ps1" } # Folio shell integration v1
```

The predicate is evaluated once and consumes its Boolean result. Missing files
never reach dot-sourcing, even with strict mode and terminating error preference.
`-LiteralPath` avoids wildcard interpretation of account-folder characters.
Installation refuses if the resolved data root is not `APPDATA\Folio`, because
writing a second managed spelling would break the exact-form contract.
