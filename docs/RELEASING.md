# Releasing

## The tag

`v<version>` or `v<version>-preview`, and the version is the one in
`[workspace.package]`. The workflow refuses anything else, because the tag and
the manifest are one claim: the archive is `folio-<version>-windows-x64.zip` and
`folio.exe --version` answers `<version>`, so a tag naming a version the tree
does not carry would put three different numbers in front of the same reader.

**`-preview` is a release channel and not a second claim.** Every release so far
has been tagged that way over a manifest with no suffix — `v0.1.0-preview` over
`0.1.0`, `v0.1.1-preview` over `0.1.1` — and the suffix says who the build is
for, not what it is. The manifest does not carry it; nothing in the archive
carries it; only the tag and the release page do.

## The workflow

`.github/workflows/release.yml` has one job, `archive`, and two ways in.

**A tag push** is the real one: it builds, runs the licensing gates against the
tree it is building, writes the bill of materials, packs the archive, starts the
executable it just packed, and files a **draft** release for a person to read and
publish. Nothing is ever published without that person.

**A manual run** — Actions → Release → Run workflow, or
`gh workflow run release.yml --ref <branch>` — does every one of those steps
except the last. It drafts nothing, because the draft step asks whether this run
is of a tag and a manual run is not; the branch it runs is the branch you point
it at. It takes one optional input, `tag`: give it `v0.2.0-preview` and the
tag-versus-manifest check runs exactly as it would on the real tag, which is how
a tag that would be refused is found out about before it is pushed; leave it
empty and nothing is claimed, so the run is just a rehearsal of the archive.

Use it before every release, and after touching anything the job depends on.
This workflow ran red on `v0.1.0`, `v0.1.1` and `v0.2.0-preview` — three tags,
the same failure each time, seconds into the run — because several of its steps
exist nowhere else and the only way to exercise them was to tag something. The
failure itself was that this file installed the compiler its own way and that way
had stopped working; both files now call `.github/actions/toolchain`, which reads
the channel out of `rust-toolchain.toml`, so CI and the release build cannot
disagree about the compiler again. The release job passes it `cache: false`: what
this job produces is what people run, and a cache is a set of files from another
run that nothing here verifies.

## The three scripts

The release workflow runs three scripts in this order, and each of them can be
run by hand exactly as it runs there:

| script | what it produces |
| --- | --- |
| `scripts/release/sbom.ps1` | the bill of materials, written into the output directory |
| `scripts/release/package.ps1` | `folio.msix`, `folio-<version>-windows-x64.zip` with the package and the executable both in it, the MPL-2.0 crate archive, and `SHA256SUMS.txt` over everything beside them |
| `scripts/release/smoke.ps1` | starts the executable that was built and checks the seven things a green build can still be broken about |

Everything below is about the one step that is not in that workflow, because it
needs a person: signing.

## Signing

Folio is signed by Microsoft's **Artifact Signing** service — the service that
used to be called Trusted Signing. There is no `.pfx` anywhere in this project
and there is not going to be one. The service holds the key, issues a
certificate that is valid for **three days**, and signs on request for whoever
Azure says may use the certificate profile.

`scripts/release/sign.ps1` is the whole of the integration.
`scripts/release/package.ps1 -Sign` calls it.

### What signs, and what is only checked

`folio.exe` and `folio.msix` are signed, in one call to `sign.ps1` — `signtool`
signs a package with the command line it signs an executable with. `conpty.dll`
and `OpenConsole.exe` are Microsoft's, and they arrive from Microsoft's own
package already signed by Microsoft; putting our signature over theirs would
replace a statement Windows already trusts with a newer and weaker one.
`package.ps1 -Sign` checks that the signature they came with is still valid and
still time stamped, and signs neither. The five text files in the archive — the
two licences, the notices, the trademark note and `folio-here.cmd` — carry no
signature because no text file can.

The package needs its signature more than the executable needs its own. An
unsigned `folio.exe` is a program Windows warns about and then runs; an unsigned
`folio.msix` **cannot be registered at all**, so a release that ships one has an
Explorer menu row that fails for everybody who turns it on.

### One-time preparation

1. **A Windows SDK**, for `signtool.exe`. Any install of the SDK that includes
   the signing tools will do, as long as it is **10.0.22621.755 or newer** —
   `sign.ps1` picks the newest x64 one under `Windows Kits\10\bin` and refuses an
   older one by name. An older `signtool` does not fail loudly: it ignores the
   signing library, looks in the machine's own certificate store instead, and
   reports that it found no certificate there.

   The same install is where `makeappx.exe` comes from, and `package.ps1` finds
   it the same way — so a machine that can sign a release can also pack one, and
   a machine with no SDK is refused by both with a sentence naming the SDK.

2. **The .NET 8 runtime, x64.** The signing library is a .NET 8 assembly hosted
   inside `signtool`'s native process. Missing, it is the failure Microsoft's own
   troubleshooting page describes as "signing fails with no error code";
   `sign.ps1` checks for it first and says so instead.

3. **The Azure CLI**, `winget install -e --id Microsoft.AzureCLI`. It is not the
   only way to be signed in — the library asks `DefaultAzureCredential`, which
   also reads a service principal out of `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and
   `AZURE_CLIENT_SECRET` — but it is the way a person at a laptop does it.

   **A shell opened before the CLI was installed does not have it on PATH**, and
   that is not an error you get to read: the library runs `az` by name from
   inside `signtool`, and a `signtool` that cannot find it stops on a prompt
   nobody is watching until somebody kills the run. It happened on the first
   signature this project made. `sign.ps1` now looks in the CLI's standard
   install location when the name does not resolve, puts that directory in front
   of this process's PATH, checks that the name resolves afterwards, and says so.
   With no CLI on the PATH and none in that location it refuses and names what to
   install, rather than starting a run that will hang.

4. **Sign in, as the account that holds the role.** Signing is authorised by the
   **Artifact Signing Certificate Profile Signer** role on the certificate
   profile, granted in the Azure portal to whoever is going to sign. That account,
   and no other:

   ```
   az login --use-device-code
   az account set --subscription <the subscription the signing account is in>
   ```

   The second line is only needed when the account can see more than one
   subscription. Nothing about this sign-in is written into the repository: no
   token, no subscription, no address.

5. **Microsoft's signing library** is fetched by `sign.ps1` itself, from
   nuget.org, into `%LOCALAPPDATA%\Folio\artifact-signing\<version>\`. It is
   never committed — see `/tools/` in `.gitignore` — and the version it fetches
   is pinned in the script, so the tool that signed a release can be named later.

### Every release

```powershell
az login --use-device-code                       # once per few hours
cargo build --release
./scripts/release/sbom.ps1
./scripts/release/package.ps1 -Sign
./scripts/release/smoke.ps1 -Exe target/release/folio.exe -ExpectSigned `
    -Msix target/release-package/folio.msix
```

`package.ps1 -Sign` signs `folio.exe` where the build left it and `folio.msix`
where it packed it, *before* the archive is built and before `SHA256SUMS.txt` is
written, so the hash published beside the archive is the hash of the signed bytes
and the executable `smoke.ps1` starts afterwards is the executable that ships.

`-Msix` is needed on that last line and nowhere else. `smoke.ps1` looks for the
package beside the executable, because that is where it is for everybody who
receives one — the archive holds both files in one folder. Straight out of a
build they are two directories apart, `target/release` and
`target/release-package`, so the path is given rather than a file copied to make
a default true.

`-ExpectSigned` makes `smoke.ps1` refuse an executable that is not signed, is
signed by somebody else, or is signed without a time stamp, and refuse a package
that is unsigned, untimestamped, signed by a different certificate than the
executable, or declaring a `Publisher` that is not that certificate's subject.
Leave it off for an ordinary build, which is unsigned and is meant to be.

To sign something without touching the original — a build in `dist/`, say —
`sign.ps1` takes `-OutDir` and signs copies placed there:

```powershell
./scripts/release/sign.ps1 -Files dist\folio-next31.exe -OutDir target\signed
```

### What the signature looks like

The certificate is issued to the same holder the two licence files and the
executable's own `LegalCopyright` name:

```
CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US
```

`sign.ps1` prints that subject after every signature, and `smoke.ps1
-ExpectSigned` reads the holder out of the executable's `LegalCopyright` and
refuses a certificate that does not name them — so there is no second copy of
the name for the two to drift apart on.

### The time stamp is not optional

An Artifact Signing certificate is **valid for three days**. A signature made
without a countersigned time stamp verifies for those three days and then stops
verifying, on machines nobody here is sitting at. `sign.ps1` always passes
`/tr http://timestamp.acs.microsoft.com`, and both it and `smoke.ps1
-ExpectSigned` refuse a signature with no time stamper on it rather than
believing a signature that happens to be young.

This is also why the short certificate lifetime is not a reason to re-sign
anything: a time-stamped signature outlives the certificate that made it, and an
archive already published never needs signing again.

### Naming a different account

Three names decide where the request goes, and none of them is a secret. They
default to this project's, and move by parameter or by environment variable —
the parameter wins:

| variable | parameter | default |
| --- | --- | --- |
| `FOLIO_SIGN_ENDPOINT` | `-Endpoint` | `https://eus.codesigning.azure.net` (East US) |
| `FOLIO_SIGN_ACCOUNT` | `-Account` | `folio-sign` |
| `FOLIO_SIGN_PROFILE` | `-CertificateProfile` | `folio-public` |

The endpoint's region has to be the account's region. A mismatch is answered with
a 403 and not with a redirect.

### When it will not sign

`sign.ps1 -DryRun` resolves every tool, says which credential the library is
going to find, writes the metadata, prints the exact
`signtool` command it would run, and stops. Almost everything that can be
misconfigured is visible in that output without asking the service anything.

| what you see | what it is |
| --- | --- |
| `not signed in to Azure` | the CLI is here but nobody is signed in. The script prints the `az login` line to run. |
| `no Azure CLI and no service principal` | nothing here can authorise anything. Install the CLI and open a new shell. Refused rather than started, because the alternative is a run that hangs. |
| a run that hangs with no output | this is what the two rows above exist to prevent. If it still happens, `signtool` is waiting on a credential prompt: kill it, and check that `az` resolves by name in the same shell. |
| HTTP 401 | the sign-in expired. `az login --use-device-code` again. |
| HTTP 403 | the account is signed in but may not use this profile: check the role assignment, the account and profile names, and that the endpoint's region matches the account's. |
| `no certificates were found that met all the given criteria` | `signtool` never loaded the signing library and fell back to the local certificate store — an SDK older than 10.0.22621.755, or the wrong architecture. |
| nothing at all, and a failure | the .NET 8 runtime is missing. `sign.ps1` checks for it, so this only happens if the check is bypassed with `-DlibDir`. |

### Testing the integration without signing anything

`scripts/release/sign-tests.ps1` runs fourteen cases against `sign.ps1` — the
metadata it assembles, the flags it passes, that `-OutDir` never writes back over
what it was given, that verification passes a signed file and refuses a tampered
one, that an Azure CLI which is installed but not on the PATH is put there and
resolves by name afterwards, and that a run with no sign-in refuses early and
names the command to run. It reaches no network, signs nothing, and reads the
sign-in state from an empty `AZURE_CONFIG_DIR` so it says the same thing on a
machine somebody is signed in on. Run it after changing either script.

## The sparse MSIX package

`folio.msix` is packed by `package.ps1` out of `packaging/msix/`, and it ships
**inside the archive**, in the same folder as `folio.exe`.

### What it is, and what it is not

It is an identity. There is no program in it: no executable, no library, nothing
but `AppxManifest.xml` and three logos, which is what "sparse" means and why the
whole file is a few kilobytes. What that identity buys is one thing — a verb on
the **first** page of the Windows 11 right-click menu, which is a page only a
packaged application is allowed to put anything on.

The program the manifest names lives at an **external location**, and that
location is the folder the recipient extracted the archive into. This is the
reason the two files travel together: a package registered against a folder with
no `folio.exe` in it names a path with nothing at it.

Nothing happens when somebody extracts the archive. The package is inert until a
user turns the row on in `Settings ▸ General ▸ First page of that menu`, and that
registration is per-user and needs no elevation — no administrator, no installer,
no service. **Nothing in this repository registers a package on the machine that
built it.** A build that registered its own output would be a build that changed
the developer's Explorer menu and left it changed, and it would test the
registration on the one machine where it cannot fail interestingly.

The classic `HKCU\Software\Classes` verb — the one that reaches the "Show more
options" page and Windows 10 — is not replaced by this and stays where it is.
`docs/DESIGN.md` §7.4a is where that decision is written down.

### `Publisher` is the certificate subject, character for character

`<Identity Publisher="…">` in `packaging/msix/AppxManifest.xml` reads:

```
CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US
```

which is the subject of the certificate the release is signed with, spelled the
same way. Windows compares those two strings when the package is registered, and
**refuses a mismatch with a message that names neither of them**: the user is
told that a package could not be registered, and there is nothing in front of
them to compare. Nothing earlier fails — not the build, not the packing, not a
signature check, not a smoke test that only starts the executable.

That is the failure `smoke.ps1 -ExpectSigned` exists to catch, and it catches it
on the artefact: it opens the packed `folio.msix`, reads `Publisher` out of the
`AppxManifest.xml` inside it, and compares it with the subject of the certificate
that actually signed that package — as a distinguished name rather than as a
string, so that the space Windows puts after a comma and the one the manifest
does not are the same name and not a failure to go and edit a correct file. It
then checks that the same certificate signed `folio.exe`.

Change the certificate and this file changes with it. There is no way to derive
one from the other at packing time — the certificate does not exist until the
service issues one, three days at a time — so the two are written down once and
checked against each other on every signed release.

### The version in the manifest is `0.0.0.0` on purpose

The checked-in manifest carries `Version="0.0.0.0"`, and `package.ps1` replaces
it in a copy at packing time with the workspace version from `Cargo.toml`, in the
four-part form a package identity is spelled in: `0.1.1` becomes `0.1.1.0`, and a
pre-release suffix is cut off first, so `0.2.1-preview` also becomes `0.2.1.0`.
The manifest in the tree is never edited by the build.

A real version number in that file would be a second place this product's version
is written, free to disagree with the binary it ships beside — the exact failure
the single-source rule in `Cargo.toml` exists to prevent. `package.ps1` refuses
to pack a manifest whose `Version` is anything but `0.0.0.0`, so the placeholder
cannot quietly become a value.

The substitution is done on the parsed XML and not with a search over the text,
because `0.0.0.0` appears twice in that file: once as the attribute and once in
the comment explaining why the attribute is a placeholder.

### `makeappx.exe`

`makeappx.exe` comes from the Windows SDK — the same install `signtool.exe` comes
from, at
`C:\Program Files (x86)\Windows Kits\10\bin\<sdk version>\x64\makeappx.exe`.
`package.ps1` takes the newest x64 one under `Windows Kits\10\bin`, prints the
full path of the one it chose, and refuses with a sentence naming the SDK when
there is none.

It is run as `makeappx pack /d <layout> /p <output>\folio.msix /o /nv`, and the
`/nv` is load-bearing rather than lax. Semantic validation checks that every file
a manifest names is in the package, and the point of a sparse package is that
`folio.exe` is not; without the flag makeappx refuses with

```
error: Manifest validation error: … The file name "folio.exe" declared for
element "…/Application" doesn't exist in the package.
```

which is a description of the design rather than a fault in it. Microsoft's own
instructions for granting identity with an external location pass it for this
reason. The manifest's structure is still validated: a misspelled element or an
undeclared namespace fails with the flag on.
