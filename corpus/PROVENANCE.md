# Where the files in `corpus/` came from

**This file is a frame, not an answer. The rows are deliberately empty.**

The corpus is the one directory in this repository whose contents are *records of
a real machine doing real work* rather than something written here — and two of
its recordings were confirmed in the 2026-08-27 readiness review to contain a
real person's name, e-mail address, organisation name, account name and install
paths. Until every fixture has been re-recorded or rewritten clean, and until the
claim in `corpus/README.md` that "the source/command for every fixture and any
environment substitution is recorded" is true of every row below, this file
cannot say what it is meant to say.

Filling it in belongs to the corpus line of the release plan
(`docs/plans/release/plan.md`, gate 0). The table below is the shape the answer
has to take; the machine-path gate (`scripts/check-machine-paths.ps1`) currently
**skips `corpus/`**, and that exclusion is meant to be deleted in the same commit
that fills this table in.

## What each row has to say

For every tracked file: **own / upstream / generated**, the licence if it is
anyone else's, and — for a recording — the exact command that produced it, the
shell and its version, the ConPTY source and version, and what was substituted
out of the environment before it was committed.

| File | own / upstream / generated | Licence | How it was produced | What was substituted |
|---|---|---|---|---|
| `README.md` | | | | |
| `cargo-build-flood.btcr` | | | | |
| `cjk-width-cases.json` | | | | |
| `claude-code-session.btcr` | | | | |
| `claude-interactive.plan` | | | | |
| `daily.ps1` | | | | |
| `dollars.ps1` | | | | |
| `editor-alt-screen.btcr` | | | | |
| `editor-alt-screen.ps1` | | | | |
| `math-expressions.jsonl` | | | | |
| `pwsh-daily.btcr` | | | | |
| `resize-redraw.ps1` | | | | |
| `resize-sequence.btcr` | | | | |
| `shell-dollars.btcr` | | | | |
| `tui-redraw.btcr` | | | | |
| `tui-redraw.ps1` | | | | |

## The two known-dirty ones

`claude-code-session.btcr` and `cargo-build-flood.btcr` are recordings of a real
session on the author's machine. `rg -a` over them finds a personal name, a
personal e-mail address, an organisation name, a Windows account name and two
install paths. `docs/spikes/01-corpus.md` describes the first as a genuine
recording of a real interactive tool, not a synthesised fixture. They must be
re-recorded on a clean profile or rewritten byte-for-byte before this repository
is public — and `git grep` will not tell you when that is done, because it skips
files Git calls binary. Use `rg -a`, or the gate.

## Why the recordings are not "just test data"

A `.btcr` is a byte-exact transcript of everything a pseudoconsole emitted. That
is what makes it valuable as a regression fixture and what makes it dangerous as
a public file: it captures whatever was on the screen, including prompts, paths,
branch names and anything the tool being recorded chose to print.
