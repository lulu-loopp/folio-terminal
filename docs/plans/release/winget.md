# Publishing Folio to winget

Research only — no manifest has been submitted, no `package.ps1` change has
been made. This is what a submission would need to say, and where the current
release shape (a signed portable zip, `folio.msix` as a sparse identity riding
inside it) does and does not fit the winget community repository's model.

## 1. Does a portable zip fit the model?

Yes, and the fit is closer than it looks at first. Since Windows Package
Manager 1.5, `InstallerType: zip` is a real installer type; it requires
`NestedInstallerType` and `NestedInstallerFiles` to say which file inside the
archive is actually run, and `NestedInstallerType: portable` is one of the
allowed nested types.
[Create your package manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest)
[installer.md](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)

`NestedInstallerFiles` normally holds exactly one entry, but the schema lifts
that limit specifically when `NestedInstallerType` is `portable`:

> This field can only contain one nested installer file unless the
> NestedInstallerType is 'portable'

which does not help us directly — we still only name one file
(`folio.exe`) — but confirms the archive-portable path is the one meant for
exactly this shape of thing.

**The part that decides whether this works at all is what winget does with
the *other* eight files in the zip**, and that took real digging — it is not
in the current docs page, only in a resolved GitHub issue.

By default, `winget install` on a zip+portable package extracts the archive
into `%LOCALAPPDATA%\Microsoft\WinGet\Packages\<...>` and creates a **symlink**
to the one named file inside `%LOCALAPPDATA%\Microsoft\WinGet\Links`, which is
what's actually on `PATH`. That breaks exactly our case: `folio.exe` looks for
`conpty.dll` and `OpenConsole.exe` beside `current_exe()`
(`vendor/conpty/portable-pty/src/win/psuedocon.rs`, per
`scripts/release/package.ps1`'s own doc comment), and a program launched
through a symlink in a different folder does not reliably see its siblings —
this was reported and confirmed against real multi-file zips (nginx-, php-,
Paint.NET-shaped packages) in
[microsoft/winget-cli#2711](https://github.com/microsoft/winget-cli/issues/2711):

> The problem here is that `.dlls`/other files the package needs can't be
> referenced when the package is being invoked through the symlink and thus
> the package won't work correctly.

**This was fixed**, not worked around, by
[microsoft/winget-cli#4816](https://github.com/microsoft/winget-cli/pull/4816),
which added an installer-manifest field, `ArchiveBinariesDependOnPath`,
carried in manifest schema **1.9.0** onward:

> Added a new boolean field to the installer manifest called
> ArchiveBinariesDependOnPath. This field only applies to nested portables in
> an archive. When set to true, the portable installer will skip creating a
> symlink and automatically add the install directory to path. … Verified
> with an E2E test to make sure that the install directory path is added
> instead of the symlink directory.

The schema doc for the field, current as of 1.12.0:

> Optional indication to add the install location directly to PATH. Only
> applies to an archive containing portable packages. … Specifying `true`
> will add the install location directly to the `PATH` environment variable.
> Specifying `false` or leaving the value unset will use the default
> behavior of adding a symlink to the `links` folder.
[installer.md §ArchiveBinariesDependOnPath](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)

With `ArchiveBinariesDependOnPath: true`, winget extracts the whole zip into
one real folder and puts *that folder* on `PATH` — no symlink, no
indirection. Every file `package.ps1` puts in the archive
(`conpty.dll`, `OpenConsole.exe`, `folio.msix`, the two licences, the notices,
`folio-here.cmd`) lands beside `folio.exe` in that folder exactly the way it
does when a person unzips the release by hand. `folio.msix` still names that
same folder as its external content location (`docs/RELEASING.md` §"The
sparse MSIX package"), so the Explorer-menu row a user turns on afterwards
still resolves. **This is the one field that makes the whole plan work**, and
it did not exist before winget-cli 1.9 — any manifest we write has to declare
`ManifestVersion: 1.12.0` (or at least 1.9.0) and set it explicitly; leaving
it unset reintroduces the symlink and a `folio.exe` that cannot start ConPTY.

One more nuance the archive shape forces: `package.ps1` zips the *folder*, not
its contents, so that extracting produces one directory rather than nine loose
files (`ZipFile]::CreateFromDirectory(..., $true)`). `RelativeFilePath` inside
the manifest therefore has to include that folder —
`folio-<version>-windows-x64/folio.exe`, not `folio.exe` — or `NestedInstallerFiles`
names a path that is not in the archive.

## 2. Identifier, manifest files, and the fields that carry real content

A multi-file submission needs at minimum three YAML files under
`manifests/<first-letter-lowercase>/<Publisher>/<Package>/<Version>/` in a
fork of `winget-pkgs`: a **version** file, a **default locale** file, and an
**installer** file. A `locale.zh-CN` file is optional and adds nothing beyond
`ShortDescription` in Chinese if we want one — `PackageName`/`Publisher` are
inherited from the default locale when unset.
[Create your package manifest — multiple manifest files](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest)

**Identifier.** The MSIX identity already published in
`packaging/msix/AppxManifest.xml` is `Name="WeiyiShi.Folio"`,
`Publisher="CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US"`. A winget
`PackageIdentifier` is an unrelated namespace (GitHub's repo path, not
Windows's package-family system), so there is no technical requirement to
reuse the string — but Microsoft's own packages do exactly this on purpose:
`Microsoft.WindowsTerminal` names the winget package and
`Microsoft.WindowsTerminal_8wekyb3d8bbwe` is the MSIX package family name for
the *same* product, same prefix on both. Reusing `WeiyiShi.Folio` verbatim as
the `PackageIdentifier` (`Publisher: Weiyi Shi`, `PackageName: Folio`,
`Moniker: folio`) is the version of "wise, not confusing" that matches that
precedent, and a repository search turned up no existing `WeiyiShi` publisher
folder and no `folio` moniker or `PortableCommandAlias` collision in
`winget-pkgs` today — this would be a first submission with a clear identifier.
Flagged as **open question 1** below rather than decided here, because it is
a naming call in the same family as the product-name ruling
(`product-name-folio.md`) and belongs to the user.

**Fields with real content to fill in**, from
`defaultLocale.md` and `installer.md` (1.12.0), matched against what this
repository already states elsewhere:

| field | value | source |
| --- | --- | --- |
| `Publisher` | `Weiyi Shi` | certificate subject / `LICENSE-MIT` |
| `PackageName` | `Folio` | `product-name-folio.md` |
| `License` | `MIT OR Apache-2.0` | `LICENSE-MIT`, `LICENSE-APACHE` |
| `LicenseUrl` | link to one licence file in the repo | dual-licensed; pick one (open question 4) |
| `Copyright` | `Copyright (c) 2026 Weiyi Shi and Folio contributors` | `LICENSE-MIT` |
| `ShortDescription` | one line, no marketing words (`docs/plans/ui-style/copy-guide.md`'s rule applies in spirit even though this file is outside its checked list) | — |
| `PackageUrl` | `https://github.com/lulu-loopp/folio-terminal` | repo |
| `PublisherSupportUrl` | `https://github.com/lulu-loopp/folio-terminal/issues` | repo |
| `PrivacyUrl` | `https://github.com/lulu-loopp/folio-terminal/blob/main/docs/PRIVACY.md` | repo |
| `ReleaseNotesUrl` | the tag's release page, e.g. `.../releases/tag/v0.2.2-preview` | `docs/plans/release/release-note-v0.2.2-preview.md` is the source text |
| `ReleaseDate` | the date `gh release create` was run for that tag | RFC 3339 date, per `installer.md` |
| `Tags` | up to 16, e.g. `terminal`, `console`, `cli`, `conpty`, `pty`, `windows-terminal` | `defaultLocale.md` caps this at 16 |

**Signature verification: `InstallerSha256`, and no `SignatureSha256`.**
`SignatureSha256` exists only for `msix`/`appx` installer types — it is "the
sha256 of signature file inside appx or msix"
(`AppxSignature.p7x`), used for streaming installs of packaged apps. Our
`InstallerType` is `zip`; the field that matters is `InstallerSha256`, "SHA
256 hash for the installer... used to confirm the installer has not been
modified" — the hash of the whole `.zip`. `package.ps1` already writes this:
it is the `folio-<version>-windows-x64.zip` line in `SHA256SUMS.txt`, produced
after signing. Winget does not itself verify `folio.exe`'s Authenticode
signature from the manifest for a zip installer — that check happens on our
side (`smoke.ps1 -ExpectSigned`) and on winget's side only as part of the
community repo's own security scanning of the submitted binary, not as a
manifest field we set.
[installer.md §InstallerSha256/SignatureSha256](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)

## 3. Pre-release handling

**Schema-wise, `0.2.2-preview` is legal.** The `PackageVersion` JSON-schema
pattern only excludes filesystem-illegal characters
(`^[^\\/:\*\?"<>\|\x01-\x1f]+$`) — a hyphenated suffix passes it fine, and the
field's own doc says versions are sometimes "date driven" or carry
"package-specific meaning" rather than clean semver.
[version.md](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/version.md)

**Convention-wise, Microsoft's own repository does not do that.** The
precedent already in `winget-pkgs` for a pre-release channel is a *separate
PackageIdentifier*, not a suffixed version: `Microsoft.VisualStudioCode`
(stable) and `Microsoft.VisualStudioCode.Insiders` (pre-release) are two
package identities, each carrying a clean numeric `PackageVersion`
(`1.96.0`, no `-preview`/`-insiders`) — the channel lives in the identifier,
not the version string. Nothing forces us to follow that shape (a
`Folio.Preview`-style split identifier would be one manifest tree fully
separate from a future stable one, doubling maintenance for a product this
size), but it is the shape winget itself models for exactly this situation,
and it is why this plan does **not** propose `0.2.2-preview` as the
`PackageVersion` — see open question 2.

**`wingetcreate update` is the per-release flow**, once a package identifier
exists:

```
wingetcreate update <PackageIdentifier> --submit --token <PAT> \
    --urls <InstallerUrl>|<Architecture> --version <PackageVersion>
```

It fetches the existing manifest tree for that identifier, downloads the new
installer to recompute `InstallerSha256`, and opens (or updates) a PR with a
new `manifests/.../<Publisher>/<Package>/<NewVersion>/` folder — the old
version's manifest folder is left in place, versions accumulate rather than
being overwritten, exactly like this repository's own release archive.
`-r/--replace` exists for *editing* an already-open manifest folder in place
rather than adding one; the default is additive.
[winget-create update.md](https://github.com/microsoft/winget-create/blob/main/doc/update.md)

**Automating the PR**, once the shape is proven by hand once: the community
standard is the third-party GitHub Action
[`vedantmgoyal9/winget-releaser`](https://github.com/vedantmgoyal9/winget-releaser)
(Komac under the hood), triggered on a GitHub Release event. It needs:

- a fork of `microsoft/winget-pkgs` under the same GitHub account
  (`lulu-loopp`), created once;
- a **classic** PAT with `public_repo` scope (fine-grained tokens are not
  supported), stored as a repository secret (e.g. `WINGET_TOKEN`) — never in
  the workflow file itself;
- `identifier: WeiyiShi.Folio` (or whatever open question 1 settles on) and
  an `installers-regex` narrow enough to match only
  `folio-<version>-windows-x64.zip` among this release's assets — the same
  release also publishes `folio.msix`, `option-ext-<version>.crate`,
  `folio-<version>.cdx.json` and `SHA256SUMS.txt`, none of which is an
  installer winget should see.

The trigger has to be `release: types: [released]`, not a tag push:
`docs/RELEASING.md` is explicit that the tag-push workflow only builds an
unsigned rehearsal artifact and **never publishes anything** — the real
release is created by hand afterward, from the signed machine, sometimes as a
`--draft` first. A release-event trigger fires only once a human has actually
published the page with the real, signed asset attached, which is the
release-integrity story this repository has already committed to.

## 4. Validation and moderation

**Local, before opening a PR** — both are named directly in `winget-pkgs`'s
own PR template checklist:

```
winget validate --manifest <path-to-version-folder>
winget settings --enable LocalManifestFiles   # once, admin shell
winget install --manifest <path-to-version-folder>
```

`validate` checks the YAML against the schema; `install --manifest` actually
runs our `ArchiveBinariesDependOnPath` path end to end on a real machine —
the only way to find out locally whether `folio.exe` actually starts a shell
from the extracted, PATH-added directory before a PR proves it in front of
strangers.
[validate command (winget) — Microsoft Learn](https://learn.microsoft.com/en-us/windows/package-manager/winget/validate)

**`Tools/SandboxTest.ps1`**, checked into `winget-pkgs` itself, runs the same
install inside a disposable Windows Sandbox rather than the machine doing the
work — the closer analogue to what a first-time user's machine looks like,
and worth running once before the first submission given how easy the
symlink-vs-PATH failure mode above is to get subtly wrong.
[Tools/SandboxTest.ps1](https://raw.githubusercontent.com/microsoft/winget-pkgs/master/Tools/SandboxTest.ps1)

**Moderation.** The documented, common rejection reasons
([Troubleshoot.md](https://github.com/microsoft/winget-pkgs-submission-test/blob/master/Troubleshoot.md),
search results over closed PRs) are largely mechanical and largely already
satisfied by how this repository ships:

- **hash mismatch** between `InstallerSha256` and the actual asset — a
  process risk if the manifest is authored before the final signed zip
  exists; the fix is always regenerating the hash from `SHA256SUMS.txt` after
  `package.ps1 -Sign`, never hand-typing it;
- **the publisher/package folder must match the identifier** —
  `manifests/w/WeiyiShi/Folio/`, mechanical, satisfied by construction;
- **AV/security-scan false positives** on the binary — the standard
  first-timer failure for small independent publishers, and outside our
  control beyond what is already true (Authenticode-signed by a real
  certificate, not a self-signed one) — worth budgeting review-cycle time
  for, not something this plan can pre-empt;
- **no scripts as installers** (batch/PowerShell) is explicitly banned — not
  relevant here, we ship `zip`/`portable`, not a script;
- **silent install requirement** — a portable zip has no install UI at all,
  so this is satisfied trivially;
- nothing in the moderation docs found bans the word "preview" from
  `PackageName` or `ShortDescription` outright; the VS Code Insiders
  precedent above is the stronger signal that the convention is to keep
  `PackageVersion` clean and put the channel in the identifier instead, not
  a documented hard rule against the word appearing anywhere.

## 5. What `package.ps1` and `docs/RELEASING.md` would need

**`package.ps1`: nothing.** The zip it already produces —
`folio-<version>-windows-x64.zip`, all nine files inside one
`folio-<version>-windows-x64/` folder — is exactly the archive a
`zip`+`portable`+`ArchiveBinariesDependOnPath` manifest wants. No new asset,
no flag, no change to what ships. The only new artifact this plan produces
lives entirely in a fork of `winget-pkgs`, not in this repository.

**`docs/RELEASING.md`: one short new section**, added after "## What gets
published", documenting the per-release manifest update once a package
identifier exists — the `wingetcreate update` invocation above, where its
`InstallerSha256` comes from (`SHA256SUMS.txt`, not recomputed by hand), and
either the manual PR flow or a pointer at the `winget-releaser` workflow if
open question 3 lands on automating it. Not written here, since it documents
a real per-release step once one exists, and none exists yet.

## Checklist

- [ ] **Decide open questions 1–3** below (identifier, wait-for-non-preview,
      automate-or-not) — everything after this depends on them.
- [ ] Fork `microsoft/winget-pkgs` under `lulu-loopp` (or wherever the
      identifier's publisher segment points).
- [ ] Author the three manifest files for the chosen version, from the
      skeleton below, filled from `SHA256SUMS.txt` and the release page for
      that tag.
- [ ] `winget validate --manifest <path>` locally.
- [ ] `winget settings --enable LocalManifestFiles` (once), then
      `winget install --manifest <path>` locally — confirm `folio` on `PATH`
      starts a working shell (ConPTY, not the fallback path) from the
      installed directory, not merely that the install step exits 0.
- [ ] `Tools/SandboxTest.ps1` against the same manifest folder.
- [ ] Submit via `wingetcreate submit` (first time, by hand) or
      `wingetcreate update ... --submit` (subsequent versions).
- [ ] Watch the automated validation pipeline on the PR (hash check,
      AV/security scan, schema) and moderator review; budget for at least
      one round of feedback on a first submission from a new publisher.
- [ ] Only after a manual submission has cleared moderation once: wire
      `vedantmgoyal9/winget-releaser` on `release: types: [released]`, scoped
      to the zip asset only, with a classic PAT (`public_repo`) as a repo
      secret.
- [ ] Add the short new section to `docs/RELEASING.md` documenting the
      per-release manifest update.

## Manifest skeleton

Three files, `manifests/w/WeiyiShi/Folio/0.2.2/`, `ManifestVersion: 1.12.0`
throughout (required — see §1, this is the version that carries
`ArchiveBinariesDependOnPath`). Placeholders in `< >`; everything else is a
value already established elsewhere in this repository, not invented here.

`WeiyiShi.Folio.yaml` (version manifest):

```yaml
PackageIdentifier: WeiyiShi.Folio
PackageVersion: 0.2.2
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
```

`WeiyiShi.Folio.locale.en-US.yaml` (default locale manifest):

```yaml
PackageIdentifier: WeiyiShi.Folio
PackageVersion: 0.2.2
PackageLocale: en-US
Publisher: Weiyi Shi
PublisherUrl: https://github.com/lulu-loopp
PublisherSupportUrl: https://github.com/lulu-loopp/folio-terminal/issues
PrivacyUrl: https://github.com/lulu-loopp/folio-terminal/blob/main/docs/PRIVACY.md
PackageName: Folio
PackageUrl: https://github.com/lulu-loopp/folio-terminal
License: MIT OR Apache-2.0
LicenseUrl: <LICENSE-MIT or LICENSE-APACHE blob URL — open question 4>
Copyright: Copyright (c) 2026 Weiyi Shi and Folio contributors
ShortDescription: <one line, no marketing words>
Tags:
  - terminal
  - console
  - cli
  - command-line
  - conpty
  - pty
  - windows-terminal
ReleaseNotesUrl: https://github.com/lulu-loopp/folio-terminal/releases/tag/v0.2.2-preview
ReleaseDate: <YYYY-MM-DD, the date the release page was published>
ManifestType: defaultLocale
ManifestVersion: 1.12.0
```

`WeiyiShi.Folio.installer.yaml` (installer manifest):

```yaml
PackageIdentifier: WeiyiShi.Folio
PackageVersion: 0.2.2
Platform:
  - Windows.Desktop
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: folio-0.2.2-windows-x64/folio.exe
    PortableCommandAlias: folio
ArchiveBinariesDependOnPath: true
Installers:
  - Architecture: x64
    InstallerUrl: https://github.com/lulu-loopp/folio-terminal/releases/download/v0.2.2-preview/folio-0.2.2-windows-x64.zip
    InstallerSha256: <sha256 of folio-0.2.2-windows-x64.zip, from SHA256SUMS.txt>
ManifestType: installer
ManifestVersion: 1.12.0
```

`SignatureSha256` is intentionally absent — that field applies to `msix`/
`appx` installer types, and ours is `zip` (§2). No `LICENSE-CN`/`locale.zh-CN`
file is included in the skeleton; it is optional and adds one field
(`ShortDescription` in Chinese) if wanted later.

## Automation choice, recommended

Do the **first** submission by hand (`wingetcreate submit`, or a manually
authored PR from the skeleton above) — a new publisher's first PR is the one
most likely to hit an AV false positive or a moderator request neither of us
can predict from documentation alone, and there is no value in automating a
flow that has not yet been proven once. Once that PR has merged, wire
`vedantmgoyal9/winget-releaser` on `release: types: [released]` for every
release after — it is the community-standard action, needs only a repo
secret and an `installers-regex`, and matches this repository's existing
posture of "no CI step publishes anything a human did not trigger": the
release-event trigger still only fires after a person has run
`gh release create` by hand from the signed machine.

## Open questions for the user

1. **`PackageIdentifier: WeiyiShi.Folio`** — reuse the MSIX identity string
   verbatim (recommended above, matches the `Microsoft.WindowsTerminal` /
   `Microsoft.WindowsTerminal_8wekyb3d8bbwe` precedent), or pick something
   else? This is a one-time, effectively permanent choice — winget-pkgs
   treats the identifier as the package's unique key.
2. **Submit now, at `0.2.2`, with `-preview` confined to the tag/URLs and
   never in `PackageVersion` — or wait for a non-preview tag?** Nothing
   found in winget's schema or moderation docs blocks a preview build by
   name; the VS Code Insiders precedent (§3) argues for keeping
   `PackageVersion` clean regardless of channel, which this plan already
   does. Whether to publish a still-preview product to winget at all before
   `docs/plans/post-release-roadmap-2026-09-01.md`'s later milestones land
   is a product-maturity call, not a technical one.
3. **Automate on the first release, or only after the first manual PR
   clears** (recommended: the latter, §"Automation choice")?
4. **`LicenseUrl`** — point at `LICENSE-MIT` or `LICENSE-APACHE`? Winget
   allows exactly one URL for a dual-licensed project; either file states
   the same copyright line.
5. Is `Tags` (§"Manifest skeleton") the right list, and is the one-line
   `ShortDescription` placeholder something the user wants to write
   themselves, or drafted here after question 2 settles the "preview or
   not" framing it would need to reflect?

## Sources

- [Create your package manifest — Microsoft Learn](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest)
- [validate command (winget) — Microsoft Learn](https://learn.microsoft.com/en-us/windows/package-manager/winget/validate)
- [winget-pkgs installer manifest schema 1.12.0](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)
- [winget-pkgs defaultLocale manifest schema 1.12.0](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/defaultLocale.md)
- [winget-pkgs version manifest schema 1.12.0](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/version.md)
- [winget-pkgs doc/Authoring.md](https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md)
- [winget-pkgs doc/Policies.md](https://github.com/microsoft/winget-pkgs/blob/master/doc/Policies.md)
- [winget-pkgs PULL_REQUEST_TEMPLATE.md](https://github.com/microsoft/winget-pkgs/blob/master/.github/PULL_REQUEST_TEMPLATE.md)
- [winget-pkgs Tools/SandboxTest.ps1](https://raw.githubusercontent.com/microsoft/winget-pkgs/master/Tools/SandboxTest.ps1)
- [winget-cli issue #2711 — Handle DLLs for symlinked executables](https://github.com/microsoft/winget-cli/issues/2711)
- [winget-cli PR #4816 — ArchiveBinariesDependOnPath fix](https://github.com/microsoft/winget-cli/pull/4816)
- [winget-cli discussion #3382 — does the zip installer support multi-file programs?](https://github.com/microsoft/winget-cli/discussions/3382)
- [winget-create update.md](https://github.com/microsoft/winget-create/blob/main/doc/update.md)
- [vedantmgoyal9/winget-releaser](https://github.com/vedantmgoyal9/winget-releaser)
- [winget-pkgs-submission-test/Troubleshoot.md](https://github.com/microsoft/winget-pkgs-submission-test/blob/master/Troubleshoot.md)
- [Microsoft.VisualStudioCode.Insiders manifest, winget-pkgs](https://github.com/microsoft/winget-pkgs/blob/master/manifests/m/Microsoft/VisualStudioCode/Insiders/1.96.0/Microsoft.VisualStudioCode.Insiders.yaml) (pre-release-channel precedent, §3)
- `docs/RELEASING.md`, `scripts/release/package.ps1`, `packaging/msix/AppxManifest.xml` — this repository, for the current release shape §1–§2 is checked against.
