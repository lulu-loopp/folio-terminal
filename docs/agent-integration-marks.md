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

**An operand that names no file names no copy.** A relative, placeholder or
control-character operand cannot be resolved to a copy on this machine, so it is
not another live Folio: it is nobody's, and nobody's is removable as a mark
"naming a file that does not exist" (design §6.2). 0.4.2 wrote a bare
`folio.exe` whenever `current_exe()` failed, and such a machine could otherwise
neither install nor uninstall for ever.

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

Folio never writes through a link it did not resolve itself. It resolves one,
once, in `attention_hooks::editable_target`, and the decision is
`attention_hooks::linked_target` over injected facts: **the target must be a
regular file inside the resolved directory the configuration path names.** Then
that target is what is read, backed up and replaced, through `land`'s atomic
writer — metadata-preserving where a file is being replaced rather than created.
A target that leaves the folder, a link to a directory and a link to nothing are
refused, and the file is left byte-identical. Copilot's whole-file removal takes
the resolved target and then the name that led to it, because leaving either
would leave Folio's hooks firing or leave upstream a name that loads nothing.

A `~/.claude` a dotfile manager has junctioned onto a managed folder is
therefore ordinary: it installs, and — the reason this rule exists — it
uninstalls. Hard links, read-only files and anything that is not a regular file
are still refused by the shared T-A predicate, on the resolved target.

The settings row says so. `state()` answers `Refused(reason)` for a file this
build will not edit, which is neither `Installed` nor `Absent`: the switch would
otherwise read `Off` over hooks that are firing and offer a press that cannot
happen. `row_state()` gives the row both facts from that one read, and the
sentence under the row is the filesystem predicate's own answer, said about an
agent's configuration file rather than about a `$PROFILE`
(`AgentConfigLink`, `AgentConfigHardLink`, `AgentConfigReadOnly`).

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

No shell, guard process, launcher, or startup migration is added. Exact legacy
Folio shell templates are decoded for ownership; an explicit hook install or
refresh upgrades eligible entries. Unrecognized Folio-shaped templates refuse.
Claude entries sharing a group with user hooks are edited individually.

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
atomic writer. Claude/Copilot JSON reserialization can reorder keys and change
formatting. Codex keeps `toml_edit` preservation. T-C1 release-note wording:
“Updating agent hooks may reformat their JSON settings; a dated backup is kept.”
Config read/modify/write is not a transaction with arbitrary external editors;
the existing narrow race with simultaneous external replacement remains.

The new English `AgentHooks*` and `AgentConfig*` strings are registered in
`Text::ALL` and `CHINESE_PENDING` for both platform columns. Chinese copy awaits
owner review.
