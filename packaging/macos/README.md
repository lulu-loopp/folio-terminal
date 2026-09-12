# `packaging/macos`

What a Developer ID `Folio.app` is assembled and signed from. This is the
skeleton P-2 lays down; nothing here builds a bundle yet — M1 assembles the
first one and M5 signs, notarizes and ships it
(`docs/plans/port/macos-plan-2026-09-12.md` §4.5, §7.1).

| File | What it is |
|---|---|
| `Info.plist.in` | The template `Contents/Info.plist` is generated from. One substitution, `@VERSION@`. |
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
