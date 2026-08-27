# corpus provenance

Everything in this directory is first-party: recorded, scripted or generated in this repository.
Nothing here is vendored, copied from a third party, or carries terms of its own, so every row below
reads "repository terms" — whatever licence the repository ships under covers it. The repository has
no top-level LICENCE file yet; adding one is a release gate item, and it will settle these rows too.

The `.btcr` recordings are raw ConPTY byte streams captured on a Windows development machine, so
they carried the recording account's identity until it was scrubbed on **2026-08-27**. The scrub is
length-preserving — the BTCR format frames every output payload behind a `u32` length prefix, and
the recordings carry ConPTY's baked-in line wrapping, so every substitution keeps the byte count and
the column alignment of what it replaced. Replay is byte-for-byte the same stream with different
letters in it. `no_recording_carries_a_person` in `crates/bt-corpus/tests/corpus_privacy.rs` holds
the line for any recording added later.

Scrub classes, without the original values:

| class | what replaced it |
|---|---|
| account holder's given name | a placeholder given name of the same length |
| account holder's mail address | a same-length address at `example.com` |
| account banner naming the holder | a same-length neutral sign-in line |
| Windows home root above the user directory | a same-length neutral directory name |
| repository parent directory on the recording machine | a same-length neutral directory name |
| organisation label derived from the mail address | a same-length neutral account label |

The project's own name, crate names, tool versions and every control sequence are untouched: they
identify the software, not a person.

## Recordings

| file | source | licence | scrubbed |
|---|---|---|---|
| `pwsh-daily.btcr` | first-party recording of `corpus/daily.ps1` under ConPTY (100×24) | repository terms | 2026-08-27 — repository parent directory |
| `cargo-build-flood.btcr` | first-party recording of a verbose `cargo build` of this workspace under ConPTY (120×20) | repository terms | 2026-08-27 — given name, home root, repository parent directory |
| `claude-code-session.btcr` | first-party recording of a real interactive Claude Code 2.1.210 session under ConPTY (100×28), driven by `corpus/claude-interactive.plan` | repository terms | 2026-08-27 — given name, mail address, account banner, organisation label, home root, repository parent directory |
| `editor-alt-screen.btcr` | first-party recording of `corpus/editor-alt-screen.ps1` under ConPTY (60×12) | repository terms | not needed — carried no identity |
| `tui-redraw.btcr` | first-party recording of `corpus/tui-redraw.ps1` under ConPTY (60×10) | repository terms | not needed — carried no identity |
| `shell-dollars.btcr` | first-party recording of `corpus/dollars.ps1` under ConPTY (100×18) | repository terms | not needed — carried no identity |
| `resize-sequence.btcr` | first-party recording of `corpus/resize-redraw.ps1` under ConPTY (80×18) with four scheduled resizes | repository terms | not needed — carried no identity |

`editor-alt-screen.ps1` and `tui-redraw.ps1` are labelled stand-ins: the recording machine had no
`vim` and no `htop`/`top`, so the scripts reproduce the alt-screen and continuous-redraw behaviour
those programs exercise. They are not recordings of those programs.

## Inputs and generated fixtures

| file | source | licence | scrubbed |
|---|---|---|---|
| `daily.ps1`, `dollars.ps1`, `editor-alt-screen.ps1`, `resize-redraw.ps1`, `tui-redraw.ps1` | first-party recording scripts | repository terms | not needed — carried no identity |
| `claude-interactive.plan` | first-party timed input plan (`MS:HEX`) for the Claude Code recording; the hex decodes to prompts written for this spike | repository terms | not needed — carried no identity |
| `cjk-width-cases.json` | first-party width table; every row cites its UAX #11 rule and the measured `alacritty_terminal` behaviour | repository terms | not needed — carried no identity |
| `math-expressions.jsonl` | first-party generated LaTeX expressions | repository terms | not needed — carried no identity |
