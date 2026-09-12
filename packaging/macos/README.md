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
