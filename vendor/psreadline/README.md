# PSReadLine for Windows PowerShell panes

Folio pins Microsoft's PowerShell Gallery package of PSReadLine as the line editor of its
Windows PowerShell 5.1 panes (T-INTEGRATION-INJECT-2-BUILD; the design is
`docs/plans/design/integration-inject-2-2026-10-15.md`, the owner's ruling Q1 of 2026-10-15):

- Package: `PSReadLine.2.4.5.nupkg`
- Package version: `2.4.5`, the newest stable PSReadLine on the gallery (published 2025-10-22)
- Source asset: <https://www.powershellgallery.com/api/v2/package/PSReadLine/2.4.5> (the package
  page's "Download the raw nupkg file" link), which redirected to
  <https://cdn.powershellgallery.com/packages/psreadline.2.4.5.nupkg>
- Fetched: 2026-10-17, once, by the person who set the pin. Nothing fetches it again: not a build,
  not a test, not `package.ps1`. The hash below proves the bytes unchanged, and a release that
  re-fetched would depend on the gallery being up.
- Package SHA-256: `cb9390e9733208456c234a7971d1ec4a917886c239502aab68f4b71aa4bba235`
  (234,360 bytes). Our hash of the downloaded package is the pin, as it is for the ConPTY
  package; the gallery's own feed hash is not relied on.
- Upstream repository: <https://github.com/PowerShell/PSReadLine>
- License: BSD 2-Clause, `Copyright (c) 2013, Jason Shirk`, the package's `License.txt`. It is
  taken out of the package with the binaries and reproduced in the repository's
  `THIRD-PARTY-NOTICES.md`.

It is a published binary artefact, not source. No upstream source is vendored and no file in the
package is modified, so `scripts/check-vendor-notices.ps1` (Apache-2.0 §4(b), modified vendored
crates) does not apply to it and has no row for it.

## What is taken out of it

`crates/bt-app/build.rs` verifies the package and entry hashes, then extracts only these entries,
for a Windows target and whatever the build host is (`crates/bt-app/src/psreadline_artefact.rs`,
through `bt_winres::pinned`, the reader the ConPTY sidecar is taken out with). The executable
embeds them, with Folio's own manifest `assets/psreadline/PSReadLine.psd1` beside them, and writes
them into `%LOCALAPPDATA%\Folio\psmodules\cb9390e97332\PSReadLine\2.4.5\` for a Windows
PowerShell pane. Nothing is added to the release archive.

| Package entry | SHA-256 | Bytes | Authenticode |
| --- | --- | ---: | --- |
| `Microsoft.PowerShell.PSReadLine.dll` | `f8e3a5b7e3e8cad2130ce10647564a2a0ea15d98db8a0cc8d589f80154c108e2` | 339,528 | Valid; `ProductVersion` 2.4.5 |
| `Microsoft.PowerShell.Pager.dll` | `451994c3d3e38d939b4fa5f8594d0207840f45e7a210aa75f81aef85aa17c592` | 16,784 | Valid |
| `netstd/Microsoft.PowerShell.PSReadLine.Polyfiller.dll` | `2e63dc86d9240243ccfb07da5d0b4fce2bc5ce5f9952658a7ce68c26bcd3e14c` | 16,928 | Valid |
| `net6plus/Microsoft.PowerShell.PSReadLine.Polyfiller.dll` | `6d33ea289b405c64bf5daa9c59e459596cac59ece399d0d215ee9f3d46afd675` | 14,920 | Valid |
| `License.txt` | `25c2fdfcdc653f65629233f5ef217a83287733d3c0299c207b0bca865508c463` | 1,324 | — |

Each DLL's signer subject is `CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond,
S=Washington, C=US`, issued by `Microsoft Code Signing PCA 2011`, with a Microsoft Time-Stamp
Service countersignature (`Get-AuthenticodeSignature`, 2026-10-17). The package also carries a
NuGet repository signature at `.signature.p7s`.

**Not taken**, and why: `PSReadLine.psd1`, `PSReadLine.psm1` and `PSReadLine.format.ps1xml` —
the module Folio writes is binary-only, because execution policy governs a `.psm1` and a
`.ps1xml` and not a manifest or an assembly, and under `Restricted` or `AllSigned` the signed
`.ps1xml` stops a 5.1 startup at the untrusted-publisher prompt. Folio's manifest names the
assembly as its `RootModule` instead, and the one function upstream's `.psm1` defines,
`PSConsoleHostReadLine`, is defined by Folio's shell-integration loader. `Changes.txt` is not
needed to run the module. `SamplePSReadLineProfile.ps1` is an entry the earlier fork never carried:
a sample profile for a person to read, which Folio does not run. The NuGet package's own records
(`PSReadLine.nuspec`, `_rels/.rels`, `[Content_Types].xml`, the `.psmdcp` core properties and
`.signature.p7s`) describe the package and are not part of the module.

## The gate a pin change passes

`scripts/ci/check-psreadline-artefact.ps1` (run by CI's `licensing` job) holds this file, the
package and the build script's table to one another: the package's SHA-256 equals the one here and
the table's; every entry the table names is in the package with its pinned SHA-256 and in the table
above with the same one; each extracted DLL's Authenticode signature is `Valid` with signer subject
`Microsoft Corporation`; and Folio's manifest names the table's version as its `ModuleVersion`,
the assembly as its `RootModule`, and no `NestedModules`, `FormatsToProcess` or `ScriptsToProcess`.
The build script's own refusal on a hash mismatch is the gate every build passes.

To move the pin: fetch the new raw `.nupkg` once into an empty directory, record its SHA-256, its
entries and each DLL's signature here, replace the package, update the table in
`crates/bt-app/src/psreadline_artefact.rs`, Folio's manifest and `psreadline::SHIPPED_VERSION`,
and run the check. The artefact's directory name follows the package's hash, so a build carrying
the new package writes a sibling directory and the old one is removed at the next launch that
finds nothing holding it.
