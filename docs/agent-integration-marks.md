# Agent integration marks (T-B)

The adapters decode ownership at `attention_hooks::hook_owner`,
`attention_codex::notify_owner`, and `attention_copilot::entry_owner`.
The verb/family identifies a Folio entry; its executable operand identifies the
copy. `attention_ownership::other_live` uses `explorer_menu::same_path` and
filesystem metadata. Only `NotFound` means a dead owner; access errors refuse.
Existing symlinks and short names for executables are compared through the
operating system.

**Ownership is per entry.** An entry Folio cannot decode is not Folio's: it is
left exactly where it is and it has no say over the entries beside it, so one
hand-written `"mytool attention claude-code:Stop"` no longer freezes a whole
file against installation and removal alike. The document's own shape is a
different fact and still refuses: a file this build cannot read is not an entry
it cannot claim. Copilot's `folio.json` is written whole, so an entry that is
not Folio's there still makes the file not Folio's, under its own sentence.

**Only Folio's own name is Folio's.** The verb and the family say *a* Folio
wrote an entry; they do not say which program runs. `folio_operand` decides
that, and it is the one place the rule is stated: the operand's own file name
must be one Folio has ever installed itself under — `folio.exe` on Windows
(`bt_platform::msix::PACKAGE_EXECUTABLE`) or `folio` in the macOS bundle and on
Linux, folded for case except on `OtherUnix`. There is no legacy name: the
product was named on 2026-08-14 and these installers were written on
2026-08-25. All three families ask it, through `legacy_path`, `direct_path` and
Codex's `notify_owner`, and they ask it before judging an entry's shape, so a
stranger's argument list is never this build's business. A copy the user
renamed is not recognised either: its marks are left alone, which is the safe
direction.

**An operand that names no file names no copy.** A relative, placeholder or
control-character operand — having passed the name test — cannot be resolved to a
copy on this machine, so it is not another live Folio: it is nobody's, and
nobody's is removable as a mark "naming a file that does not exist" (design
§6.2). 0.4.2 wrote a bare `folio.exe` whenever `current_exe()` failed, and such a
machine could otherwise neither install nor uninstall for ever. A user's own
`"mytool" attention claude-code:Stop` is not this: it never was a copy, so it is
never nobody's.

`attention_ownership::stable_executable` is the common per-copy gate on what may
be *written*, and it is unchanged by the two rules above. An absent, relative,
non-Unicode, control-character, `${…}` placeholder or App Translocation
executable path refuses. Translocation is decided by the `AppTranslocation` path
component alone, through the pure predicate `attention_ownership::translocated`:
macOS reports the same executable as `/var/folders/…` and as
`/private/var/folders/…`, so a prefix was a spelling rather than the fact. The
UI passes a failed `current_exe()` through this gate rather than inventing a
bare executable name.

## Linked configuration files

Folio never writes through a link it did not resolve itself, and **one
operation resolves once**. `attention_hooks::Config::resolve` is the only door
and `editable_target` is private behind it; the value it returns carries the
resolved target, and the read, the dated copy, the replace and Copilot's removal
all take that value. Nothing below an operation's entry is handed the
configuration path, so a link repointed between the read and the write — a
dotfiles `stow`, a profile switch — cannot make Folio write a file it never read.

The decision itself is `attention_hooks::linked_target` over injected facts:
**the target must be a regular file inside the resolved directory the
configuration path names.** That target is read, backed up and replaced through
`Config::land`'s atomic writer — metadata-preserving where a file is being
replaced rather than created. A target that leaves the folder, a link to a
directory and a link to nothing are refused, and the file is left
byte-identical. Copilot's whole-file removal takes the resolved target and then
the name that led to it, because leaving either would leave Folio's hooks firing
or leave upstream a name that loads nothing.

**What is replaced is what was read.** `Config::land` reads the resolved target
again immediately before the replace and refuses (`AgentConfigChanged`, nothing
written and no copy kept) when it no longer matches the bytes the operation was
given — `shell_integration::replace_profile`'s rule, for its reason: an agent
edits its own configuration as readily as a person does. This is a narrow guard,
not a transaction: two writers can still interleave inside the replace itself.

A `~/.claude` a dotfile manager has junctioned onto a managed folder is
therefore ordinary: it installs, and — the reason this rule exists — it
uninstalls. Hard links, read-only files and anything that is not a regular file
are still refused by the shared T-A predicate, on the resolved target.

The settings row says so. `state()` answers `Refused(reason)` for a file this
build will not edit, which is neither `Installed` nor `Absent`: the switch would
otherwise read `Off` over hooks that are firing. `row_state()` gives the row both
facts from that one read; the sentence under the row is the filesystem
predicate's own answer, said about an agent's configuration file rather than
about a `$PROFILE` (`AgentConfigLink`, `AgentConfigHardLink`,
`AgentConfigReadOnly`), and the switch itself takes no press in either direction
while it stands there — `SettingsRow::option_enabled`, the rule every greyed
picker in the dialog keeps.

**Editing touches only Folio's own entries** (owner's rule, 2026-11-04; 0.4.10
plan row 67a; this replaces "Editing reformats", which described the writer
before it). Folio changes only what it can prove is its own, and every other
byte of an agent's file stays as it was: the person's hooks before and after
Folio's on the same events, every other key, their order, the indentation, the
line endings, a byte-order mark, the trailing newline, the spelling of numbers
and escapes.

- **Claude Code** (`attention_hooks`): the file is read as spans by a strict
  JSON reader (`splice`), and `edited` makes only three kinds of edit. It
  replaces an own entry where it stands, which is how a gone copy's entry is
  repaired among the person's own. It deletes an own entry with its comma, and
  the group, event or `hooks` that held nothing but Folio's goes with it. It
  appends a missing entry: a group at the end of its event, after the person's,
  an event at the end of `hooks`, or `hooks` at the end of the file's object,
  laid out like its neighbours. An own entry is one `hook_owner` reads as a
  Folio executable speaking `attention`. Nothing else is edited, moved or
  deleted. Before anything is written, `verified` reads the result back. It
  cuts Folio's entries, and the groups, events and `hooks` that hold nothing
  else, out of the file before and after, each with one separator, and
  requires what is left to be the same bytes: the byte-order mark, every piece
  of the person's, and the whitespace, commas and brackets between them. It
  also compares the two documents as values with Folio's entries stripped from
  both. Any difference refuses the write whole: "Folio could not change only
  its own lines in this file, so nothing was written". A file that is not
  strict JSON (comments, trailing commas) is never written, as before. A key
  named twice on the way down to Folio's entries, or inside an entry that reads
  as Folio's, is refused too, because the agent's parser decides which of the
  two it reads. The switch, the take-over and uninstall all go through this
  writer.
- **Codex** (`attention_codex`): the same rule on `config.toml`. `toml_edit`'s
  rendering ended every line in `\n` and dropped a byte-order mark, so the edit
  is now one line of text, placed by `ImDocument`'s spans. An own `notify` with
  other words has its value replaced, keeping a comment at the end of the line.
  A missing `notify` gets a line after the last top-level key-value, or at the
  top of a file of tables only. Removing takes the line and its line ending and
  keeps any comment above it (ruling R12). The result is read back and refused
  the same way if any other value reads differently.
- **Copilot**: `hooks/folio.json` is Folio's whole file. One entry that is not
  Folio's refuses the write, so no byte of anyone else's is touched.

One round trip is not the identity. The file cannot say who wrote an empty
`"hooks": {}` or an empty event array. Folio fills it on install, and on
removal it goes with Folio's entries, by the same rule that removes a `hooks`
Folio created. The same holds one level up: an object Folio's entries were the
only members of is written back as `{}`, so a file that was `{ }`, or `{` and
`}` on two lines, comes back as `{}`. Every other byte comes back. This is ruled, not owed (2026-11-04,
plan row 67a D1): an empty container holds no hook or setting of the person's,
and the rule protects their hooks and settings, so no record outside the file
is kept to tell who wrote one.

## Current upstream schemas, checked 2026-09-20

- [Claude Code command-hook fields](https://code.claude.com/docs/en/hooks#command-hook-fields):
  `type: "command"`, executable in `command`, argument array in `args`, and
  `async: true`. Presence of `args` selects direct execution on every platform.
  Stdin arguments remain limited to mappings that consume a payload. Direct
  execution preserves spaces, apostrophes, dollar signs and backticks without
  shell tokenization. Async failure completion stays suppressed by default;
  verbose/debug output is not promised silent.
- [Copilot CLI hook syntax](https://docs.github.com/en/copilot/reference/hooks-reference#configuration-file-syntax):
  version 1, `exec` and `args`; never combined with `bash`, `powershell` or
  `command`. `type: "command"`, matchers and `timeoutSec: 5` are retained. The
  reference does not specify a general `async` field; none is invented. Copilot
  documents `notification` as fire-and-forget. Missing-executable behavior for
  all installed events remains a real-agent acceptance check.
- [Codex legacy notifier source](https://github.com/openai/codex/blob/main/codex-rs/hooks/src/legacy_notify.rs):
  the existing `notify` array remains `[exe, "attention",
  "codex:agent-turn-complete", "--json"]`. Codex appends the JSON payload,
  discards standard streams, and spawns without waiting. Failure is a continued
  hook failure, rather than a user-facing blocking decision.

No shell, guard process or launcher is added. Exact legacy Folio shell
templates are decoded for ownership; an install or refresh upgrades eligible
entries. Unrecognized Folio-shaped templates refuse. Claude entries sharing a
group with user hooks are edited individually.

## The start-time scan (0.4.10 plan row 67b)

Since 0.4.10 the start-time carry pass (`carry_pass`, on its own below-normal
worker, never the window thread) classifies each agent's file
(`agent_hooks_step`; each family's `scan_resolved` returns a `HooksFound`) and
writes in exactly two states:

- **Dead**: every other copy the file names is `NotFound` on a volume that is
  there (`other_live_by`, `DeadWhen::MissingOnAVolumeThatIsThere`). The volume
  is the one the path leads to once its links and junctions are followed
  (`volume_of_a_missing_path` resolves the deepest part that is there), asked
  only under this rule, never by a press or the uninstall; a path whose way
  cannot be followed is an owner that cannot be asked about. A copy
  on a drive or share that is not plugged in, and an operand that names no
  file, is `OwnerUnknown` and keeps its hooks. A press and the uninstall keep
  the older rule (`DeadWhen::Missing`): what is not there is gone.
- **Drifted**: only the ruled shapes. This copy's own entries in an older form
  (C1–C3, P1, P2), with arguments that are not today's, or with a spelling of
  the program that is not this copy's; or no Claude Code `Stop`, the event
  resume needs. Another event the person removed by hand stays removed unless
  a drift repair runs, which writes the full current set.

A person's edit of Folio's own entry is theirs, not drift: an `async: false`
they set stays, and so do a `timeoutSec` or `matcher` on Copilot's entry.
Ownership is by content, never by position: each wanted entry claims an own
entry under its event that speaks for the same request (`claude-code:<row>`,
`copilot:<row>`), wherever it stands, so an entry moved into the person's own
group stays there. Entries of Folio's no install claims, such as one under an
event Folio does not use, are the person's placing: kept by every install and
said on the row ("An entry of Folio's under <event> is one Folio did not put
there"). This copy's stay as they are; another copy's, moved or gone, are
pointed at this copy where they stand and keep the person's other keys. A
switch turned Off still takes every entry of this copy's out.

Both go through the switch's writer (`apply_resolved`, `Decision::Install`),
which asks every question again, records the root in `agent_config_roots` and
changes only Folio's own entries. Nothing else is written: a living other copy
keeps its hooks and the row names it (its take-over stays the press's), and an
unknown owner, an unreadable file, an unclassified Folio-like entry, somebody
else's `notify` and somebody else's hook file are said on the row. The pass
runs at the program walk's first answer and when a walk moves an agent's
folder, at each Agents page open, after a press, and at an update trial's
commit, and when the window's language changes (a refused repair's reason is
said again in the new language); during a trial the repair is held
(`Writer::AgentHooksRepair`). A press holds its family's latch, so a repair
never writes beside one; a press that finds the repair holding the latch
writes nothing and says "Folio is updating this file. Try again." A repair
reads its file once (`Config::with_one_read`), so a press that wrote after the
scan makes the repair's write refuse rather than undo it. A due repair that
was held or refused is said on the row with its reason, and the resume row
counts those hooks as missing; hooks in an old shape are never counted as this
copy's. Each repair writes a `BT_AGENT_HOOKS` diagnostics line; a dead owner's
also raises one notice per family, and only when the rescan finds the hooks
this copy's (otherwise the line says `repaired=lost`). A drift repair raises
none: it rewrites only what makes an entry old (an old form is replaced, a
direct entry gets this copy's program and the wanted arguments and keeps its
other keys), so the person's own edits stay. Copilot is repaired within its version gate: only after the
version question has been answered (the step asks it, and the answer runs the
pass again), and never for a copilot too old for the hooks.

## Take-over and the settings row

The existing settings switch reports its result through a toast. A foreign live
owner leaves the switch off for this copy and returns `TakeOverRequired` with
its executable paths. The toast names those paths and offers a second press
within 30 seconds. `Pending` binds that press to the same config path;
`Decision::TakeOver` binds it to the exact foreign owners shown. A changed
owner requires another prompt. Pending consent is consumed by the next action,
including removal, and expires. There is no automatic takeover.

Removal returns `LeftOther(paths)` when another live copy remains. Other owned
or dead entries can still be removed; the result states which owners were kept.
Codex's user-authored notifier continues to refuse replacement even with
`TakeOver`. No saved notifier is overwritten or chained.

## T-C1 contract

Each adapter exposes `apply_at(path, Decision, exe, data) -> Outcome`. The caller
supplies the trusted current executable, actual config file and Folio data root.
The ordinary settings entry point `apply` obtains today's agent config path.
T-C1 should enumerate historical `agent_config_roots` plus today's environment
and default candidates, deduplicate paths, then call `apply_at` with `Remove`.
Root-relative files are `settings.json`, `config.toml`, and `hooks/folio.json`.
Attach the config path to each outcome. `Refused` is an attempted refusal;
`LeftOther` is a deliberate retention, not evidence of removal. Running-instance,
exit-code, purge and CLI policies belong to T-C1.

Before successful installation, including an unchanged refresh of pre-record
hooks, the current agent root is added under the T-A record lock. Intent precedes
the config write; a failed later write can leave an extra discovery candidate.
The record is historical location evidence, never ownership evidence. Existing
PowerShell decisions and other integrations' fields survive. Removal leaves
history in place. An unreadable or unsupported record refuses installation; a missing record is created.

Existing dated backup names and first-backup-of-day behavior stay unchanged.
Copilot's wholly removable file is still deleted; partial removal uses the shared
atomic writer. Claude Code's and Codex's files keep every byte outside Folio's
own entries (see "Editing touches only Folio's own entries" above). The 0.4.3
release note's "Updating agent hooks may reformat their JSON settings" no longer
holds as of 0.4.10.
Config read/modify/write is not a transaction with arbitrary external editors;
the existing narrow race with simultaneous external replacement remains.

The new English `AgentHooks*` and `AgentConfig*` strings are registered in
`Text::ALL` and `CHINESE_PENDING` for both platform columns. Chinese copy awaits
owner review.
