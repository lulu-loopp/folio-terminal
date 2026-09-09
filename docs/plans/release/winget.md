# Publishing Folio to winget

Written as research before the first submission. Since 2026-09-07 the 0.2.2
manifests under `packaging/winget/manifests/` are submitted as
microsoft/winget-pkgs#431006 (pipeline passed, waiting for a manual review); no
`package.ps1` change was needed. The rest of this note is what a submission
has to say, and where the current
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
at least `ManifestVersion: 1.9.0` and set the field explicitly; leaving it unset
reintroduces the symlink and a `folio.exe` that cannot start ConPTY.

**Which schema version to declare is not "the highest one that exists".**
`winget-pkgs` publishes schema folders up to 1.28.0 (`doc/manifest/schema/` on
`master`; `https://aka.ms/winget-manifest.installer.1.28.0.schema.json` answers
and 1.29.0 does not, checked 2026-09-07), but nothing in the repository is
written against them: ten consecutive merges sampled on 2026-09-07 all declare
**1.12.0**, and the pull-request template's own checklist asks whether the
manifest "conforms to the 1.12 schema". 1.12.0 carries
`ArchiveBinariesDependOnPath` — it has been there since 1.9.0 — so it is what
the manifests in `packaging/winget/` declare. Declaring a version the community
pipeline is not yet written against would buy nothing and risk a mechanical
refusal.

One more nuance the archive shape forces: `package.ps1` zips the *folder*, not
its contents, so that extracting produces one directory rather than nine loose
files (`ZipFile]::CreateFromDirectory(..., $true)`). `RelativeFilePath` inside
the manifest therefore has to include that folder, and **the folder is not
called what the archive is called**: `package.ps1` stages into
`folio-<version>` and then names the zip `folio-<version>-windows-x64.zip`
(`$folder = "folio-$Version"`, `"$folder-windows-x64.zip"`), so the path is
`folio-<version>\folio.exe` and a manifest that repeats the archive's own name
here points at a path that is not in the archive. Read out of the built zip on
2026-09-07 rather than out of the script: the nine entries of
`folio-0.2.2-windows-x64.zip` are all under `folio-0.2.2/`.

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
  release also publishes `folio-<version>.cdx.json` and `SHA256SUMS.txt`,
  neither of which is an installer winget should see.

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

- [x] **Decide open questions 1–5** below — answered 2026-09-07, see
      "The rulings".
- [ ] Fork `microsoft/winget-pkgs` under `lulu-loopp` (or wherever the
      identifier's publisher segment points).
- [x] Author the three manifest files for the chosen version, filled from
      `SHA256SUMS.txt` and the release page for that tag —
      `packaging/winget/manifests/w/WeiyiShi/Folio/0.2.2/`.
- [x] `winget validate --manifest <path>` locally.
- [x] `winget settings --enable LocalManifestFiles` (once), then
      `winget install --manifest <path>` — on the clean Windows 10 machine
      rather than on the machine doing the work, confirming that `folio` on
      `PATH` is the extracted folder and not a link, not merely that the
      install step exits 0.
- [ ] ~~`Tools/SandboxTest.ps1`~~ — Windows Sandbox is not enabled here and
      turning it on needs an administrator and a restart. The clean Windows 10
      machine above is the substitute, and it is the older of the two Windows
      versions this product supports.
- [ ] Submit by hand: fork, branch `WeiyiShi.Folio-0.2.2`, copy the folder in,
      commit as `New package: WeiyiShi.Folio version 0.2.2`, open the PR
      (`wingetcreate update ... --submit` for subsequent versions).
- [ ] Watch the automated validation pipeline on the PR (hash check,
      AV/security scan, schema) and moderator review; budget for at least
      one round of feedback on a first submission from a new publisher.
- [ ] Only after a manual submission has cleared moderation once: wire
      `vedantmgoyal9/winget-releaser` on `release: types: [released]`, scoped
      to the zip asset only, with a classic PAT (`public_repo`) as a repo
      secret.
- [x] Add the new section to `docs/RELEASING.md` documenting the per-release
      manifest update — `## winget`, at the end of that file.

## The manifests, as written

The skeleton this section used to hold has been replaced by the files
themselves, so that there is one copy of them and it is the one that was
validated:
`packaging/winget/manifests/w/WeiyiShi/Folio/0.2.2/` — `WeiyiShi.Folio.yaml`,
`WeiyiShi.Folio.installer.yaml`, `WeiyiShi.Folio.locale.en-US.yaml`, all three
at `ManifestVersion: 1.12.0` (§1). The directory shape is `winget-pkgs`' own,
so the `0.2.2` folder is copied into a fork at the identical path rather than
rearranged.

Four things in them are worth naming here because they are the ones a later
version gets wrong:

- `RelativeFilePath: folio-0.2.2\folio.exe` — the folder inside the archive, not
  the archive's name (§1).
- `ArchiveBinariesDependOnPath: true` — without it the install is a symlink in
  `WinGet\Links` and `folio.exe` has no ConPTY beside it (§1).
- `InstallerSha256` is the `folio-0.2.2-windows-x64.zip` line of the release's
  `SHA256SUMS.txt`, upper-cased:
  `5510BDE154972B927A6590D0A29DB111B69126F6604A854D43EF369C2AFB4F74`. Downloaded
  and re-hashed on 2026-09-07; the release asset's own `digest` field agrees.
- `MinimumOSVersion: 10.0.17763.0` — Windows 10 1809, which is what both READMEs
  already say the archive needs.

`SignatureSha256` is intentionally absent — that field applies to `msix`/`appx`
installer types, and ours is `zip` (§2). There is no `locale.zh-CN` file; it is
optional, and a Chinese `ShortDescription` is a separate piece of writing.

`Author` is absent, and that is not an oversight. It is optional in the
`defaultLocale` schema, and `scripts/check-machine-paths.ps1` refuses the
author's name on any tracked line that is not a copyright notice, a sentence
about who signed the release, or a package-identity string. `Publisher: Weiyi
Shi` passes that rule (the word "Publisher" is one of the words it looks for);
a bare `Author: Weiyi Shi` line does not, and it would say nothing that
`Publisher` has not already said. Neither the gate nor the manifest was bent to
fit the other.

## The rulings (2026-09-07)

The five open questions this plan ended on are answered. What follows is the
answer, not the argument for it.

1. **`PackageIdentifier: WeiyiShi.Folio`**, `Publisher: Weiyi Shi`,
   `PackageName: Folio`, `Moniker: folio` — the MSIX identity string reused
   verbatim, as recommended.
2. **Submit now, at `PackageVersion: 0.2.2`.** The version is pure numeric; that
   the build is a preview is stated in `Description` and on the release page the
   `ReleaseNotesUrl` points at, and nowhere in the version string.
3. **The first submission is a pull request opened by hand.** No
   `winget-releaser` workflow is set up until that one has cleared moderation.
4. **`LicenseUrl` points at `LICENSE-APACHE` at the tag**, as a raw URL;
   `License` is `Apache-2.0 OR MIT`, and `Copyright` is `Copyright (c) Weiyi
   Shi`.
5. **`Tags` and `ShortDescription` are written**, in the locale manifest. The
   short description is the first line of `README.md` said again; the tags are
   fifteen of the sixteen the schema allows.

## Validation, as run

- `winget validate --manifest packaging\winget\manifests\w\WeiyiShi\Folio\0.2.2`
  against winget-cli v1.29.290 — "Manifest validation succeeded", exit 0, no
  warnings (2026-09-07).
- `winget install --manifest` end to end on the **Windows 10 virtual machine of
  `clean-vm.md`**, reverted to its `clean` snapshot: a real install from the
  published URL, `folio` resolving on `PATH` to the extracted folder rather than
  to `WinGet\Links`, `folio --version`, the sibling files, and
  `winget uninstall`. The evidence is in `winget-install-test-2026-09-07.md`
  beside this file.

  The Windows 11 machine could not be used: it is encrypted to carry its vTPM,
  and `vmrun` refuses it without the encryption password, which §2.2 of
  `clean-vm.md` deliberately keeps out of this repository. Windows Sandbox is
  not enabled on the development machine and enabling it needs an administrator
  and a restart. The Windows 10 machine is the lower bound this product claims
  support for, so it is not a weaker test — but it did have to be given two
  things it does not normally have: its network adapter, which the gate-5 build
  turns off on purpose, and a current App Installer, since the clean image
  carries `Microsoft.DesktopAppInstaller 1.0.30251.0` and no `winget` at all.
  Both are undone by reverting to `clean`, which is what the run ends with.
- Two things that run turned up and that a reader should not misread —
  `winget install --manifest` refuses the archive on a malware scan that
  Windows Defender, asked directly about the same bytes on the same machine,
  does not; and `winget uninstall WeiyiShi.Folio` does not match a package
  installed from a local manifest. Both are artefacts of the `--manifest` path
  rather than of the archive or the manifest, and the evidence file works
  through why.

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
