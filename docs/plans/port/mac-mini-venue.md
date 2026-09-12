# The Mac mini as a build venue

**P-1 of `macos-plan-2026-09-12.md`, done 2026-09-12.** The macOS plan puts
thirty-odd tickets on a Mac mini that is also the owner's personal machine, and
this is what an agent needs to know before it opens an ssh session. The same
facts live on that machine as `~/folio-port/README-agents.md`, which is the copy
an agent working there reads; this one is for an agent working on Windows and
deciding what a Mac ticket will cost.

## The corner of the machine we get

Everything lives under `~/folio-port`. Nothing else in the owner's home is ours,
and the machine runs an alpha production daemon of theirs that no ticket may go
near.

```
~/folio-port/
  repo/                 base checkout: branch `main`, pinned at 9acd482
                        (the v0.3.0-preview release commit). Never built in.
  wt/<ticket>/          one git worktree per ticket, added from `repo`.
  target/               the single shared CARGO_TARGET_DIR for every worktree.
  launchers/            the shell scripts long commands run from.
  logs/                 one log per launcher run.
  README-agents.md      the rules, in full.
```

`main` there is pinned to the release commit and deliberately does not track its
remote, so `git status` will report it behind and that is the intended state.

## The rules a ticket inherits

* Nothing outside `~/folio-port` is written, with two unavoidable exceptions:
  `~/.rustup` for a toolchain install and `~/.cargo` for the registry cache.
* No `sudo`, no package manager (that machine has none), no keychain or
  developer-tools change. Those are the owner's, and §5 of the plan says so.
* No `killall`, no `pkill`, no matching a process by name or window title. End
  the process id you wrote down yourself, and nothing else.
* One cargo at a time, always `nice -n 10 … -j 6`. Ten cores, six of them ours.
* Long commands go in a script, get copied over, lose their carriage returns,
  run under `nohup` with the log in `logs/`, and are watched by polling that log
  for a marker the script prints last. Never inline over ssh — the Windows shell
  that builds the command expands a `$` or a process pattern locally, on the
  wrong machine.

## The toolchain file names a channel that machine cannot install

`rust-toolchain.toml` pins `channel = "1.94.1-x86_64-pc-windows-msvc"`. The host
triple is there on purpose — it settles which of the two Windows hosts a Windows
machine picks — and rustup on a Mac refuses the channel, because it cannot host
that triple. CI already has the answer, in `.github/actions/toolchain`: off
Windows, split the version out of the channel, install it against the runner's
own default host, and export `RUSTUP_TOOLCHAIN` so that no later `cargo` reads
the file again. A Mac ticket does exactly that, which means
`RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin` in front of every command, and a
bare `cargo` in a worktree failing is the expected behaviour rather than a
broken venue.

## What P-1 found

The machine was already fit for the plan's preflight: the developer tools are
pointed at Xcode 26.6 and licensed, and `security find-identity -v -p
codesigning` answers `<sha1> "Developer ID Application: … (TEAMID)"`,
**1 valid identity found** — both of the things §5 asks the owner for, done.
`codesign` has no `--version`; it prints its usage instead, and its version is
Xcode's. Two of the plan's preflight facts are therefore already retired, and
the appendix's "0 valid identities found" is out of date.

The 0907 spike had left 13 GiB of build products in the checkout's own
`target/`, plus its log and output directories. Those were removed and the
checkout reset onto the release commit as a base for worktrees; free space on
the volume went from **48 GiB to 60 GiB**, with the first worktree and a warm
shared target accounting for the difference between that and 61.

Then, in a worktree at 9acd482 with a cold shared target, the venue was proved
by running what CI's `core-macos` job runs:

| Command | Result |
|---|---|
| `cargo check --locked --all-targets -j 6` over the job's fourteen crates | **green**, 34.0 s wall, 134 s user, peak RSS 855 MB, 127 units |
| `cargo test --locked -j 6 -p bt-persist -p bt-transcript` | **green**, 14.2 s wall, peak RSS 523 MB, 261 tests passed |

Two things worth carrying forward. `bt-platform` and the whole of wgpu 30 come
in as dependencies of `bt-render` and check clean, so the Metal-side dependency
graph resolves on that machine today. And the check is green while carrying
dead-code warnings that are all one shape — `bt-platform` 7, `bt-render` 3 lib
and 18 lib-test, `bt-pty` 15 lib-test, `bt-transcript` 1 — Windows-only
constants, functions and imports that no macOS arm reaches. `cargo check`
tolerates them; a lint gate with `-D warnings` will not, so the ticket that
extends the lint line to this platform is buying that cleanup with it.

The whole shared target after both commands was 767 MB, which is the number to
budget from: the plan's estimate of 5 GiB is a release build, not a check.
