# Building Folio from source

`README.md` is for people who download the archive. This file is for people who
build it.

## What you need

[rustup](https://rustup.rs/) and the MSVC toolchain — the Visual Studio Build
Tools with the C++ workload. Nothing else. `rust-toolchain.toml` pins the
compiler down to the patch number, and rustup installs that exact version on the
first build.

Windows only. Folio is written against ConPTY, Direct2D-era font APIs and the
Windows shell, and there is no cross-platform layer under it.

## Build

```powershell
git clone <this repository>
cd folio
cargo build --release
```

The binary is `target\release\folio.exe`. To run it from the checkout:

```powershell
cargo run --release
```

A debug build runs, but the terminal is fast because the renderer is optimised;
judge speed from `--release` only.

`cargo` will use every core it can find. On a machine you are also working on,
`cargo build -j 4`, or a `jobs` entry in your own cargo configuration, leaves you
a computer.

## The three gates

Every change passes all three, on Windows, before it is proposed. CI runs the
same three:

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

There is no advisory level: clippy runs with `-D warnings`, so a lint that warns
is a lint that fails.

Beside them are the script gates — the shortcut table against the source, the
third-party notices against the lock file, the public documents against the
forbidden-word list, and several more. `CONTRIBUTING.md` lists them and says what
each one holds together.

## Copied-in code

`vendor/alacritty_terminal` is a patched copy of the upstream VT engine, built as
a member of the workspace. Every difference from upstream is listed in
`vendor/alacritty_terminal/CHANGES-FOLIO.md`, with the reason for each.

`vendor/conpty/` holds the two ConPTY files that ship in the release archive —
`conpty.dll` and `OpenConsole.exe`.

`scripts/check-vendor-notices.ps1` refuses a copied-in dependency that has lost
its licence text.

## Packaging a release

`scripts/release/package.ps1` builds the archive that the releases page carries:
the executable, the two ConPTY files, `README.md`, both licence texts and
`THIRD-PARTY-NOTICES.md`, with `SHA256SUMS.txt` beside it.

## Where the decisions are written down

- `docs/DESIGN.md` — what the program is supposed to do.
- `CONVENTIONS.md` — how work is done here, in Chinese, naming the incident each
  rule was paid for.
- `CONTRIBUTING.md` — the short version of the same, and how a change is
  proposed.
- `docs/BT-ENVIRONMENT.md` — every `BT_*` environment variable the program reads,
  what each one switches on, and what can end up in a file it writes.
