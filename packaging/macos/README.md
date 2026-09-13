# `packaging/macos`

What a Developer ID `Folio.app` is assembled and signed from. This is the
skeleton P-2 lays down; nothing here builds a bundle yet — M1 assembles the
first one and M5 signs, notarizes and ships it
(`docs/plans/port/macos-plan-2026-09-12.md` §4.5, §7.1).

| File | What it is |
|---|---|
| `Info.plist.in` | The template `Contents/Info.plist` is generated from. One substitution, `@VERSION@`. Rendered by `crates/bt-winres/src/plist.rs`. |
| `entitlements.plist` | The entitlements `codesign -o runtime` is handed. The hardened-runtime baseline, with every exception explicitly `false`. |
| `.gitignore` | Keys, identities and release artifacts refused at the place they would land. |

The plan's §4.5 calls the second file `Folio.entitlements`; this is that file
under the name P-2 fixed, and the rest of the tree should use this one.

## `@VERSION@`

Filled at bundle time from `[workspace.package] version` in the workspace
`Cargo.toml`, into both `CFBundleShortVersionString` and `CFBundleVersion`.

That is the same one line that
`bt_app::version::tests::the_version_is_the_manifests_and_nothing_elses`
already holds `folio --version`, the PE `VERSIONINFO` block and the header of
every diagnostic file to. Keeping
the plist a *template* is what keeps the count at one: a literal version written
in here would be a fifth place, agreeing with the other four right up until the
next release moves them. M5-5 extends that gate over the two generated fields.

A release channel suffix (`-preview`) belongs to the tag and never reaches the
manifest — see `docs/RELEASING.md` — which is also why `CFBundleVersion`, which
accepts only dotted integers, can be filled from the same string.

### How the bundle script gets the file

```sh
cargo run -q -p bt-winres --bin render-info-plist > "$app/Contents/Info.plist"
```

That prints the rendered plist on stdout and nothing else; anything it will not
render goes to stderr and a non-zero exit, so a script with `set -e` stops
before `codesign` rather than signing a plist with `@VERSION@` still in it. The
version it fills in is `bt-winres`'s own `CARGO_PKG_VERSION`, which is the
workspace line every crate here inherits — the script does not read, parse or
repeat that line, because a second reader of it is a second thing to be wrong at
a release.

`bt-winres` is the crate with no dependencies, so this is a few seconds of
compilation on the bundle machine and no network. It is that crate rather than
`bt-app` for two reasons: `bt-app` is a binary with no library, so reaching a
function in it from a second binary means compiling the whole window first; and
`bt-winres` is already where a version meets a container that can refuse it —
`FileVersion::parse_semver` is that refusal for the Windows `VS_FIXEDFILEINFO`
numbers and `plist::render` is it for `CFBundleVersion`.

A path may be given as the single argument to render some other template; with
none, the file rendered is this directory's.

### What is refused

`plist::render` stops rather than producing a plist when either can happen:

- **a version `CFBundleVersion` cannot carry** — anything that is not one to
  three dotted integers, which is what a `-preview` or `+build` suffix would
  make it. The suffix has nowhere to go in a plist, and dropping it silently
  would put `0.3.0` on a bundle built from `0.3.0-rc.2` with nothing anywhere in
  it saying which one it is;
- **a placeholder nothing fills** — any `@…@` other than `@VERSION@`. An
  unfilled one fails no build, no `codesign` and no notarization: it ships, and
  the first reader of it is a user.

## The decisions this file records

**Taken 2026-09-12 by the merger, on the owner's standing authorization**, so
that P-2 could be dispatched without another round trip. They answer §8 Q4 and
Q6 of the plan, where they are also recorded.

- **Bundle identifier `io.github.lulu-loopp.folio`** — reverse DNS under the
  project's GitHub home, which the owner controls. Permanent: TCC keys every
  granted permission on it.
- **The signing team is the owner's Developer ID team.** The team id is read
  from the certificate subject at release time
  (`security find-identity -v -p codesigning`) and is deliberately **not**
  written anywhere in this repository.
- **Minimum macOS 14.0**, stated as the deployment target rather than inherited
  from whatever the build machine is running.
- **arm64 only** for the preview.
- **Not sandboxed**: Developer ID distribution, hardened runtime, and the
  minimal entitlements in `entitlements.plist` — no exceptions.
- Category `public.app-category.developer-tools`; display name `Folio`;
  executable `folio`.

## The four scripts that read this directory

They live in `scripts/release/macos/`, they are POSIX `sh`, and they take every
path on the command line — nothing under a home directory is assumed except the
notarization credentials, which is the one place Apple's own tools put theirs.
That is what makes them runnable from a GitHub macOS runner as well as from the
owner's Terminal.

| Script | What it does |
|---|---|
| `bundle.sh` | Assembles `Folio.app` from a `cargo build --release` output: the plist from the renderer, the executable, `Folio.icns` built out of `assets/app-icon/` with `sips` and `iconutil`, `PkgInfo`. Then `dsymutil` into `Folio.app.dSYM` **beside** the bundle. Prints the tree and the sizes. |
| `sign.sh` | `codesign --options runtime --timestamp --entitlements` in nested-code order, then `--verify --deep --strict`, the signature, the entitlements as signed, and what Gatekeeper says. `--identity` defaults to `-` (ad-hoc). |
| `notarize.sh` | `notarytool submit --wait`, keeps the log beside the artifact, `stapler staple` and `stapler validate`. `--dry-run` prints the commands and uploads nothing. |
| `dmg.sh` | Staging folder with the application and a link to `/Applications`, `hdiutil create`, sign, notarize, staple, and the `-t open` Gatekeeper assessment a download actually gets. `--dry-run` likewise. |

**There is no nested code in this bundle today** — one executable with every
Rust crate linked into it, an `.icns`, an `Info.plist` and a `PkgInfo`, and an
`otool -L` naming only `/System/Library/Frameworks` and `/usr/lib` — and
`sign.sh` prints that fact at every run. The
order it would use when there is any is inside out: nested code deepest first,
then the main executable, then the bundle, because sealing a bundle hashes the
signatures inside it and a signature replaced afterwards breaks the seal.

### The signing session is the owner's, and why

`codesign` needs the Developer ID private key out of a keychain, and an ssh
session does not have the login keychain open — it answers
`errSecInternalComponent`. So steps 3 to 6 below run in `Terminal.app` at the
machine (or in any session where that keychain is unlocked). Notarization is the
other half and it *is* headless: it authenticates with an App Store Connect API
key file, which is why `notarize.sh` never asks for a keychain.

### The release sequence, in order

```sh
# 0. Which identity, spelled the way codesign wants it.
security find-identity -v -p codesigning

# 1. Build. One release build at a time — fat LTO, about 5.3 GB.
cargo build --release --locked -p bt-app

# 2. Assemble the bundle and the debug information beside it.
scripts/release/macos/bundle.sh --out dist/macos

# 3. Sign. spctl says `rejected` with `source=Unnotarized Developer ID` here,
#    which is the expected answer before step 4 and the reason this step's own
#    exit code is the one to read rather than that line.
scripts/release/macos/sign.sh --app dist/macos/Folio.app \
  --identity "Developer ID Application: <name> (<TEAMID>)"

# 4. Notarize the application and staple its ticket.
scripts/release/macos/notarize.sh --path dist/macos/Folio.app

# 5. Ask Gatekeeper again. Now: accepted, source=Notarized Developer ID.
spctl -a -vvv dist/macos/Folio.app

# 6. The disk image, from the stapled application — it is signed, notarized and
#    stapled in its own right, and the last line is the assessment a downloaded
#    image gets.
scripts/release/macos/dmg.sh --app dist/macos/Folio.app --out dist/macos \
  --identity "Developer ID Application: <name> (<TEAMID>)"
```

Five things are in `dist/macos/` at the end. **`Folio.dmg` is the one that is
published.** `Folio.app.dSYM`, `Folio.app.notarylog.json` and
`Folio.dmg.notarylog.json` are archived with the tag and never published —
the `.dSYM` because it is the only thing that turns a crash report from that
build back into file names and line numbers and it exists only on the machine
that linked it, the two logs because they are the service's own statement about
each submission. `Folio.app` itself is inside the image and does not go up
separately.

**Never re-sign after stapling.** The ticket is written into the signed
artifact; signing again throws it away and `stapler validate` then fails on a
build that was notarized ten minutes earlier.

### What is never in this repository

The `.p8` App Store Connect key and any exported `.p12` identity. `.gitignore`
beside this file refuses both at the place they would land. `notarize.sh` reads
`~/.appstoreconnect/folio-notary.env` for the key id and the issuer id — which
are identifiers and not secrets — and hands `notarytool` the *path* of the key;
nothing in this tree opens it.
