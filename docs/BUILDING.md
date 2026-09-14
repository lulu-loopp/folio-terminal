# Building Folio from source

[`install.md`](install.md) is for people who download a build. This file is for
people who make one.

How a change gets proposed is [`CONTRIBUTING.md`](../CONTRIBUTING.md); a
security problem does not go in an issue, and [`SECURITY.md`](../SECURITY.md)
has the private channel for it.

## What you need

**On Windows:** [rustup](https://rustup.rs/) and the MSVC toolchain — the Visual
Studio Build Tools with the C++ workload. Nothing else.

**On macOS:** rustup and the Xcode command line tools,
`xcode-select --install`. That is where `codesign`, `dsymutil`, `sips` and
`iconutil` come from, and it is enough to build, bundle and run. Xcode itself is
needed only to *release* — `notarytool` and a Developer ID certificate are
`docs/RELEASING.md`'s subject, not this file's. An Apple silicon Mac running
macOS 14 or newer; there is no Intel build.

`rust-toolchain.toml` pins the compiler down to the patch number, and rustup
installs that exact version on the first build.

### The pin names a version, and a host triple that only Windows can use

The channel in that file is `1.94.1-x86_64-pc-windows-msvc`. **The version is
what is pinned.** The triple settles one question — whether a Windows machine
builds against the MSVC host or the GNU one, which once selected differently on
two workstations and failed at link time — and everywhere else there is exactly
one host to pick from.

So rustup on a Mac refuses that channel by name, and the answer is not to edit
the file. Install the same version for this machine's own host and point cargo
at it:

```sh
rustup toolchain install 1.94.1-aarch64-apple-darwin --component rustfmt --component clippy
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin
```

Without `RUSTUP_TOOLCHAIN` every later `cargo` reads `rust-toolchain.toml` again
and asks for the Windows channel a second time. `.github/actions/toolchain` does
exactly this, reading the version out of the file and the host out of
`rustup show`, which is why no workflow carries a second copy of the number.
`docs/DESIGN.md` §13.5 is where the decision is written down.

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

On macOS the same build writes `target/release/folio`, and a bundle is assembled
from it:

```sh
cargo build --release -p bt-app
scripts/release/macos/bundle.sh --out target/macos
```

That writes `Folio.app` and, beside it and never inside it, `Folio.app.dSYM` —
the debug information a crash report is turned back into file names and line
numbers with. `--no-dsym` skips it for a local run; a release must not.
`Info.plist` is rendered rather than copied
(`cargo run -q -p bt-winres --bin render-info-plist`), so the version in the
bundle is the workspace's one line and not a second copy of it.

**Run it from the bundle, not from `target/release/folio`.** Notifications,
the web preview's data store and Finder's **Open in Folio** all key on the
bundle identifier, and a loose executable has none.

### Signing a bundle you built yourself

A local `Folio.app` is signed ad-hoc — a signature that identifies the bundle to
this machine and names no developer:

```sh
codesign --force --sign - --options runtime \
    --entitlements packaging/macos/entitlements.plist target/macos/Folio.app
codesign --verify --strict --verbose=2 target/macos/Folio.app
```

Two things follow from it, and both are the reason a release is signed properly
instead. An ad-hoc bundle that has been through a browser or a download carries
the quarantine attribute, and Gatekeeper refuses it in the same words the README
tells a reader to distrust — one you built and never sent anywhere has no
quarantine attribute and runs. And **every signature is a new identity**, so the
notification permission, and any other permission the system keys on one, is
asked for again after every re-sign.

## The three gates

Every change passes all three, on the platform it was written on, before it is
proposed. CI runs the same three on Windows, and compiles the portable crates
for macOS and Linux beside them:

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

There is no advisory level: clippy runs with `-D warnings`, so a lint that warns
is a lint that fails.

### The eleven targets that need a window

`crates/bt-platform` declares ten test targets with `harness = false`, and
`crates/bt-app` one more — `macos_glyph_surface`, `macos_sheet`,
`macos_app_delegate`, `macos_services`, `macos_compose`, `macos_menu_bar`,
`macos_notifications`, `macos_webview`, `video_playback`,
`macos_window_restore` and `macos_pointer_route`. Each is its own `main` because
AppKit wants the main thread and libtest never hands a test case that thread.

**They ask before they open a window on somebody's desk.** `BT_MAC_GUI=1` is
that consent: without it each of them prints one line and exits, so
`cargo test --workspace` costs nothing on a machine nobody is watching, and off
macOS they have no body at all. `BT_MAC_GUI_SHOT=<dir>` names a directory for
the pictures they take of their own windows — optional for most, required for
`macos_glyph_surface`, which judges what it drew. `docs/BT-ENVIRONMENT.md` has
both.

Three of them ask for something else instead, and the reason is the same
reason the bundle exists: `macos_notifications` and `macos_webview` need to be
running **inside a `.app`**, because the notification centre refuses a process
with no bundle identifier and the website data store is keyed on one; and
`video_playback` needs a real window rather than a variable, because the player
never leaves its first state without one.

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
`scripts/release/macos/bundle.sh` is the other half, and it is the same script
a local build uses.

Signing is a step of its own on both platforms, because on both it is the step
that needs a person rather than a machine — signed in to Azure on Windows, and
holding an unlocked keychain on a Mac. `docs/RELEASING.md` has how each is set
up, how a release is signed and notarized, and why the time stamp is not
optional.

## The tree

Nine directories at the root, and every one of them is what its name says:

- `assets/` — files the build reads and the executable carries: the application
  icon, the emoji font, the colour schemes, the PSReadLine module Folio repairs,
  the MiTeX specification the formula engine imports, and the README's boards.
- `crates/` — the product, cut into compilation units. `bt-app` is the window.
- `docs/` — the design record, the plans behind each block of work, the spike
  reports, the screenshots the README shows, and the prototypes in `docs/design/`.
- `licenses/` — upstream licence texts, reproduced as their licences require.
- `packaging/` — the MSIX manifest and logos, the winget manifest, and in
  `packaging/macos/` the `Info.plist` template and the entitlements a Developer
  ID signature is given.
- `scripts/` — the gates, the release steps, the shell integration, the
  development probes, and in `scripts/ci/` the two helpers the workflows call.
- `tests/` — fixtures, not test code: `tests/corpus/` holds the recorded terminal
  sessions replayed byte for byte, `tests/assets/` the documents, images and
  videos the preview tests open.
- `vendor/` — copied-in code, patched and declared. See above.
- `.github/` — the workflows.

Everything else at the root is a file: the two READMEs, the licences, the
changelog, this project's conventions, and the cargo, clippy and rustfmt
configuration.

## Where the decisions are written down

- `docs/DESIGN.md` — what the program is supposed to do.
- `CONVENTIONS.md` — how work is done here, in Chinese, naming the incident each
  rule was paid for.
- `CONTRIBUTING.md` — the short version of the same, and how a change is
  proposed.
- `docs/BT-ENVIRONMENT.md` — every `BT_*` environment variable the program reads,
  what each one switches on, and what can end up in a file it writes.
