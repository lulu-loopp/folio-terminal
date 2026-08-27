# Contributing

Thank you for looking. This file is the short version of how work is done here.
`CONVENTIONS.md` is the long version — it is written in Chinese, it names the
incidents each rule was paid for, and it is worth reading before a first change of
any size. `docs/DESIGN.md` says what the program is supposed to do; this file says
how a change to it gets made.

Security problems do not go in an issue. `SECURITY.md` has the private channel.

## The three gates

Every change passes all three, on Windows, before it is proposed:

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

There is no advisory level. CI runs clippy with `-D warnings`, so a lint that
warns is a lint that fails. `rust-toolchain.toml` pins the exact compiler,
including the patch number, because a lint set moves between releases and "clippy
is clean" is otherwise a statement about one machine on one day.

`cargo` will take every core on the machine. `cargo test -j 4`, or a `jobs` entry
in your own cargo configuration, leaves you a computer to work on.

Beside the three there are gates that run as scripts, each one guarding a file
that would otherwise drift:

| Script | What it holds together |
| --- | --- |
| `scripts/check-shortcuts-table.ps1` | `docs/shortcuts.md` and the shortcut table in the source |
| `scripts/check-doc-words.ps1` | `README.md`, `CHANGELOG.md` and the rest, against the forbidden-word list |
| `scripts/check-notices.ps1` | `THIRD-PARTY-NOTICES.md` and the lock file |
| `scripts/check-vendor-notices.ps1` | every copied-in dependency and its licence text |
| `scripts/check-adapter-boundary.ps1` | the terminal adapter and the policy it must not import |
| `scripts/check-machine-paths.ps1` | no tracked file naming a person, an address or a checkout path |

## Write the failing test first

Not as ceremony — as evidence. This repository has shipped an assertion that could
not fail, a comment that said the opposite of the code under it, and a lint table
that inherited into nothing and passed every run. All three looked like working
guards.

So:

- **A fix names the test that was red before it.** "Fixed" without that is
  recorded as not fixed.
- **A new guard proves it fires.** Plant the violation, watch it go red, put the
  file back. CI does this to itself in two places — it appends a `todo!()` to a
  crate and demands clippy refuse it, and it plants a forbidden import and demands
  the boundary check refuse that.
- **Ask what makes an assertion red.** If there is no answer, it is decoration.

Two habits that come out of real failures:

- **A default value hides bugs.** A test that only ever passes row `0`, or leaves
  every field of a key at its default, is testing one path and claiming a family.
  Each field of a multi-field key needs a test that can break it alone.
- **A timeout measures silence, not elapsed time.** Reset the budget on every byte
  received and put an absolute ceiling above any honest cost. A total-seconds
  timeout measures how busy the machine is. And `sleep` is not a synchronisation
  primitive: wait for the thing to have happened, not for time to have passed.

## Working on the source

- **Derive from the specification, not from the reproduction steps.** A
  reproduction can be wrong, and this project has twice paid for a local repair
  made to satisfy one. Go back to `docs/DESIGN.md`, to DEC STD 070, or to the
  upstream source, work out the correct behaviour, and fix that. If the
  reproduction then still fails, say so.
- **No heuristics.** Guessing which rows a resize removed by comparing grids
  passes its tests and is still wrong; it was only never falsified by the sample
  in front of it.
- **A comment tells the truth or is deleted.** Change the behaviour, change the
  comment in the same edit. Find a comment lying, fix it where you stand.
- **Edit source with an editor.** Do not rewrite `.rs` files with stream editors
  or generated patches: a regular expression that reshapes code cannot reshape the
  paragraph above it, and that is exactly how a file comes to describe something
  it no longer does.
- **No placeholders in product code.** `todo!()` and `unimplemented!()` are denied
  by clippy, and CI asks the test harness for ignored tests and refuses any. An
  unfinished thing is reported as unfinished.
- **A value the specification pins is a named constant**, documented with where it
  came from. One quantity, one definition — the same number defined twice in two
  crates is a regression waiting for one of them to move.
- **Do not deviate quietly.** If the specification and reality disagree, open a
  section in your description saying which clause, what you did instead, why, and
  what the alternatives cost. Code that departed from the specification while the
  description said it had not is worse than the departure.

### The copied-in terminal engine

`vendor/alacritty_terminal` is upstream's VT engine with a small patch on it, and
it stays a member of the workspace: its own test suite is the line that catches a
patch of ours breaking VT semantics, and it only runs while it is a member. It is
held to upstream's formatting and upstream's lints, not ours. Every difference is
listed in `vendor/alacritty_terminal/CHANGES-FOLIO.md`, and every difference is
declared in the file it lives in.

Policy does not go in there, and it does not go in the adapter in front of it
either. The adapter answers what the engine did; what to do about it belongs
further up.

## Words on the screen

`docs/plans/ui-style/copy-guide.md` governs anything a user reads: a row name is a
noun or a verb phrase, a description says what happens when you turn a thing on
and then what happens when you turn it off, and never how it is implemented. The
vocabulary this codebase uses about itself stays out of the window — there is a
forbidden-word gate over both language columns, and a second gate that reads a
half-width comma after a Chinese character as the defect it is.

The same list covers the documents a reader arrives at first.
`scripts/check-doc-words.ps1` reads the prose of `README.md`, its Chinese half,
`CHANGELOG.md`, this file and the screenshot list, and leaves fenced examples,
code spans and comments alone — a path is the reader's own word for a thing they
will type.

Both languages are edited together. Every string is in `crates/bt-app/src/i18n.rs`
with its English and its Chinese side by side, so a new sentence arrives in two
languages or not at all.

## Commits

One narrative sentence, in English, lower case, saying what the change does to the
program rather than which files moved:

```
a files column keeps a shallow watch on every folder it is showing, and a re-read that says the same thing is not news
```

Longer bodies are welcome and often necessary — the reasoning, the measurements,
what was tried and rejected. Keep the subject a sentence a reader can act on.

## Licensing contributions

Folio is licensed under either the MIT License or the Apache License,
Version 2.0, at the recipient's option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms
or conditions.

Do not submit code, assets, or other material that you do not have the
right to license. Identify any third-party material and its license in
the pull request.
