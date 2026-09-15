# Releasing

## The tag

`v<version>` or `v<version>-preview`, and the version is the one in
`[workspace.package]`. The workflow refuses anything else, because the tag and
the manifest are one claim: the archive is `folio-<version>-windows-x64.zip`, the
disk image is `Folio-<version>-macos-arm64.dmg`, and `folio --version` answers
`<version>` out of either — so a tag naming a version the tree does not carry
would put three different numbers in front of the same reader.

**`-preview` is a release channel and not a second claim.** Every release so far
has been tagged that way over a manifest with no suffix — `v0.1.0-preview` over
`0.1.0`, `v0.1.1-preview` over `0.1.1` — and the suffix says who the build is
for, not what it is. The manifest does not carry it; nothing in the archive
carries it; only the tag and the release page do.

**No public document carries a versioned download link, and that is deliberate.**
`README.md`, `README.zh-CN.md` and the two `docs/install` documents send a
reader to `/releases` and name the assets as
`folio-<version>-windows-x64.zip` and `Folio-<version>-macos-arm64.dmg`, with
the angle brackets standing where a number used to. A link to
`/releases/download/v<version>-preview/<asset>` was a second copy of the claim
the tag and the manifest already make, it went stale the same way, and a stale
one is worse than none because it works and hands somebody an old build. So
there is nothing to bump at release-prep time — check instead that no document
has grown a versioned link back.

**The link that does not go stale is `/releases/latest/download/<asset>`, and
from the next release every page carries the two assets it needs.** GitHub
resolves that address by asset *name*, so it can only find a name that is the
same in every release: `package.ps1` writes `folio-windows-x64.zip` beside
`folio-<version>-windows-x64.zip`, and `dmg.sh` writes `Folio-macos-arm64.dmg`
beside the image the rename gives the long name to. Each pair is one set of
bytes copied by the packaging step itself and covered by the same checksum file,
so the two names on a page cannot come apart. A download link written anywhere
outside this repository takes this form:

```
https://github.com/lulu-loopp/folio-terminal/releases/latest/download/folio-windows-x64.zip
https://github.com/lulu-loopp/folio-terminal/releases/latest/download/Folio-macos-arm64.dmg
```

**`/releases/latest` is the release GitHub calls latest, which is never a
pre-release.** Every release up to and including v0.4.0-preview was published
with `--prerelease`, so on that repository the address answered 404. **From
v0.4.1 on, a release is published without `--prerelease`** (owner ruling,
2026-09-15): the tag keeps its `-preview` suffix and the release note still
says preview, but GitHub marks the release *Latest*, the two links above
answer, and the repository's front page shows the newest release. The
`-preview` suffix itself stays until 1.0. `README.md` and the two
`docs/install` documents keep sending readers to `/releases`, which always
answered. The product page does not depend on the flag either way: its script
asks the API for the list of releases and takes the newest, which is the same
endpoint the update check reads — what the stable names buy there is a download
address it does not have to rewrite each time.

## The workflow

`.github/workflows/release.yml` has one job, `archive`, and two ways in.
**Neither of them publishes anything**, and the job holds no permission that
would let it. It builds, runs the licensing gates against the tree it is
building, writes the bill of materials, packs the archive, starts the executable
it just packed, and keeps what it made as a **workflow artifact**.

**A tag push** is the one that matters: it says that this commit — the one
somebody named — builds green and packs a complete archive. What it leaves
behind is not the release. The release is signed and the artifact is not, and
both carry the same file names, so the artifact stays inside the run where
nobody arrives at it by following a download link.

It used to draft the release and attach that artifact to it. On 0.2.1 the draft
that appeared on the tag held the runner's unsigned files under the exact names
the signed ones carry, and it was one button away from being published as the
real thing; it was deleted, and the release was rebuilt by hand from the signed
files. The step is gone rather than repaired, because there is no version of
"attach the unsigned build to the release page" worth leaving in a file.

**A manual run** — Actions → Release → Run workflow, or
`gh workflow run release.yml --ref <branch>` — does the same thing on the branch
you point it at. It takes one optional input, `tag`: give it `v0.2.0-preview` and
the tag-versus-manifest check runs exactly as it would on the real tag, which is
how a tag that would be refused is found out about before it is pushed; leave it
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
| `scripts/release/package.ps1` | `folio-<version>-windows-x64.zip`, with `folio.msix` and the executable both in it, a copy of it called `folio-windows-x64.zip`, and `SHA256SUMS.txt` over everything beside it — **after emptying the output directory**, apart from that bill of materials |
| `scripts/release/smoke.ps1` | starts the executable that was built and checks the seven things a green build can still be broken about, and refuses an output directory holding a file from another release |

## What gets published

**Every asset on a release page comes off the machine that signed it, and none
of it comes out of CI.** `target/release-package` on that machine is the whole of
it: what the three scripts leave there, after `package.ps1 -Sign` has signed the
executable and the package, is exactly what a reader downloads. `gh release
create` is handed that directory and no list is written down anywhere, so there
is no second naming of assets to disagree with it, and nothing is hand-picked out
of `dist/`.

**Which is why `package.ps1` empties that directory before it writes into it.**
Handing `gh` a directory makes it impossible to leave an asset out, and exactly
as impossible to leave one behind: on the 0.4.0 run the directory still held
0.3.0's archive and 0.3.0's `SHA256SUMS.txt`, and the upload was one command
away from carrying two releases. Everything under `target/release-package` is
generated, so everything goes — the one exception is this version's
`folio-<version>.cdx.json`, which `sbom.ps1` wrote minutes earlier and which
`SHA256SUMS.txt` then covers. **The macOS assets are fetched into that directory
after `package.ps1` has run**, and `smoke.ps1` is the second net: it refuses to
run at all if the directory holds a file whose name carries a version other than
the one in `Cargo.toml`.

The workflow builds the same directory on a runner and keeps it as a workflow
artifact. That copy is unsigned and its file names are identical, so it is never
uploaded anywhere a stranger can reach. It is there to be compared against — the
same file list, the same notices, the same version, from the same commit — and
then left where it is.

| asset | what it is |
| --- | --- |
| `folio-<version>-windows-x64.zip` | the nine files, in one folder |
| `folio-windows-x64.zip` | the same bytes, under the name `/releases/latest/download/` resolves |
| `SHA256SUMS.txt` | one line for each of the other three, in the format `sha256sum -c` reads |
| `folio-<version>.cdx.json` | the CycloneDX bill of materials `sbom.ps1` writes |

**Four, and `folio.msix` is not one of them.** It was an asset of its own up to
0.2.2, beside the copy of itself in the zip, and what that bought was people
downloading it on its own: a file called `folio.msix` on a release page reads as
an installer, and a package registered against the folder it was downloaded into
names a path with no `folio.exe` at it. It is in the archive, where the
executable it points at is, and `package.ps1` packs it in a working directory it
takes away again rather than leaving a second copy behind.

The MPL-2.0 crate archive is not an asset either. `option-ext` is reached
through `dirs` → `dirs-sys`, and section 3.2 asks that the Source Code Form be
available to recipients and that they be told how to get it, which
`THIRD-PARTY-NOTICES.md` does by naming the exact version, the crates.io address
it is served from, and the SHA-256 `Cargo.lock` records for those bytes.

`SHA256SUMS.txt` cannot carry its own hash, so it is three lines over the other
three files. **Two of those three lines carry the same hash**, because the
archive and the copy of it are the same bytes under two names: a reader who
fetched either can check what they have against the line that names it. A
`sha256sum -c` run in a folder holding one of them reports the rest as missing —
the bill of materials always was one of those — and says `OK` for the file that
is there.

Three more assets come from the Mac and are fetched into the same directory —
`Folio-<version>-macos-arm64.dmg`, the copy of it called
`Folio-macos-arm64.dmg`, and the `SHA256SUMS-macos.txt` beside them; see
**macOS** below. **Both checksum files carry bare file names**, the hash, two
spaces and the name of the file, so that `sha256sum -c` or `shasum -c` works in
the folder a reader downloaded into. That is what `package.ps1` writes and what
`scripts/release/macos/checksums.sh` writes; 0.4.0's macOS file was made by
`shasum` on a path instead, read `target/macos-package/Folio-…`, and had to be
rewritten by hand before the page went up.

`scripts/release/smoke-tests.ps1` is `smoke.ps1`'s own self-test, and it is
about the one part of that script a green release does not exercise: the paths
it is handed. It builds nothing, signs nothing and starts nothing — each case
runs `smoke.ps1` in a child shell that was started in the repository and then
walked into a scratch folder, which is the one arrangement under which a
relative path has two answers, and reads the path the refusal names. Three of
its cases are about the other question the door answers — whether the file
`-Msix` names is the package or the archive the package ships in — and they
build a zip of each shape rather than describing one. Run it after changing how
`smoke.ps1` reads its arguments.

Everything below is about the one step that is not in that workflow, because it
needs a person: signing.

## What signs what, on both platforms

Two chains over the same source, operated in two places in this document — the
Windows one immediately below, the macOS one in the macOS section further down.

| | |
| --- | --- |
| **Windows** | `folio.exe` and `folio.msix`, signed through Microsoft's Artifact Signing service. The service holds the key, there is no `.pfx` in this project, and every signature carries a countersigned time stamp because the certificate is valid for three days. The subject is `CN=Weiyi Shi, O=Weiyi Shi, L=Ann Arbor, S=mi, C=US`, the same holder the executable's own `LegalCopyright` names. |
| **macOS** | `Folio.app` and the disk image around it, signed with a **Developer ID Application** certificate issued to the same holder, with the hardened runtime, a secure timestamp and the entitlements in `packaging/macos/entitlements.plist` — every key in that file `false`, no exceptions taken. Both are then **notarized** by Apple and the ticket **stapled** to each, so a reader's machine can check the signature with the network off. `Folio.app.dSYM` is built beside the bundle and archived with the tag; it is not published and never goes inside the application. |

Neither chain puts a key, a password or a team identifier in this repository.
The Windows one holds the key in a service the owner signs in to; the macOS one
holds it in a keychain on the owner's machine, and `packaging/macos/.gitignore`
refuses a `.p12` and a `.p8` at the place they would land.

## Signing

**This section is the Windows chain.** Folio is signed by Microsoft's
**Artifact Signing** service — the service that
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
   az login --scope "https://management.core.windows.net//.default"
   az account set --subscription <the subscription the signing account is in>
   ```

   That opens a browser and asks for the second factor. **`--use-device-code` is
   not an alternative here**: this tenant refuses the device code flow, and what
   comes back from it is a sign-in that `az account show` answers for and that
   cannot get a token — which is the failure below. The second line is only
   needed when the account can see more than one subscription. Nothing about this
   sign-in is written into the repository: no token, no subscription, no
   address.

   **A sign-in lasts hours, not days.** When it lapses, `sign.ps1` says so and
   prints the pair of lines to run, with this machine's tenant already in them;
   it asks for a token before it starts `signtool` precisely because `signtool`
   does not report an expired sign-in, it waits on one.

5. **Microsoft's signing library** is fetched by `sign.ps1` itself, from
   nuget.org, into `%LOCALAPPDATA%\Folio\artifact-signing\<version>\`. It is
   never committed — see `/tools/` in `.gitignore` — and the version it fetches
   is pinned in the script, so the tool that signed a release can be named later.

### Every release

```powershell
az login --scope "https://management.core.windows.net//.default"   # once per few hours
cargo build --release
./scripts/release/sbom.ps1
./scripts/release/package.ps1 -Sign
./scripts/release/smoke.ps1 -Exe target/release/folio.exe -ExpectSigned `
    -Msix target/release-package/folio-0.4.0-windows-x64.zip
```

`package.ps1 -Sign` signs `folio.exe` where the build left it and `folio.msix`
where it packed it, *before* the archive is built and before `SHA256SUMS.txt` is
written, so the hash published beside the archive is the hash of the signed bytes
and the executable `smoke.ps1` starts afterwards is the executable that ships.

**`package.ps1` empties `target/release-package` before it writes**, keeping only
the `folio-<version>.cdx.json` that `sbom.ps1` just wrote, and it names every
file it clears away. So run `sbom.ps1` first — the order above is the order —
and do not put anything in that directory before this line: it is emptied, and
the macOS assets belong there after it, not before.

`-Msix` is needed on that last line and nowhere else, and on the release machine
it names the **archive**. `smoke.ps1` looks for the package beside the
executable, because that is where it is for everybody who receives one — the
archive holds both files in one folder. On the machine that packed it there is
no loose copy at all: the package is in the zip and nowhere else, so the zip is
what is named, and `smoke.ps1` takes the package out of it into `-Artifacts` and
checks those bytes. It settles which of the two it was handed by opening the
file rather than by reading its name, because an msix is a zip as well.

A relative path there is read from the directory the shell is standing in, and
so are `-Exe` and `-Artifacts`: all three are made absolute before anything
reads them. They have to be, because a relative path has two answers on Windows
— PowerShell resolves one against `$PWD` and .NET resolves it against the
process's own directory, which `Set-Location` never moves — and a shell that
walked into a second checkout gets one answer from the check that the file is
there and the other from the reader two hundred lines later. That is how the
line above failed in a worktree on the 0.2.1 run, naming a folder nobody typed.
A `-Msix` naming a file that is not there is now refused at the door rather than
carried on.

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
going to find, asks that credential for the two tokens a real run needs, writes
the metadata, prints the exact `signtool` command it would run, and stops.
Everything that can be misconfigured before a signature is asked for is visible
in that output, and the signing service is never asked for one. A dry run reports
the token and does not refuse on it: it answers questions and decides nothing.

| what you see | what it is |
| --- | --- |
| `not signed in to Azure` | the CLI is here but nobody is signed in at all. The script prints the `az login` line to run. |
| `no Azure CLI and no service principal` | nothing here can authorise anything. Install the CLI and open a new shell. Refused rather than started, because the alternative is a run that hangs. |
| `the Azure sign-in cannot get a token` | the CLI still has a profile, but it has expired — `az account show` answers and a token request does not. `sign.ps1` asks for one before it starts `signtool`, because `signtool` does not report this and waits instead. The refusal prints the two lines to run, with this machine's tenant already in them. |
| a browser sign-in that is asked for and never arrives | this tenant **refuses the device code flow**, so `az login --use-device-code` produces a sign-in that cannot get a token. Use `az logout` and then `az login --tenant <the tenantId az account show prints> --scope "https://management.core.windows.net//.default"`, which opens a browser and asks for the second factor. |
| `the signing service did not respond` | `signtool` was still running `-TimeoutSeconds` after it started — 180 seconds by default — and was killed. Nothing was signed. A signature that is going to be made is made in seconds, so this is a run that was waiting for something it was never going to get. |
| a run that hangs with no output | this is what the rows above exist to prevent, and the timeout bounds what is left of it. If it still happens, `signtool` is waiting on a credential prompt: kill it, and check that `az` resolves by name in the same shell. |
| HTTP 401 | the sign-in expired between the token check and the request. Sign in again with the `az login` line above. |
| HTTP 403 | the account is signed in but may not use this profile: check the role assignment, the account and profile names, and that the endpoint's region matches the account's. |
| `no certificates were found that met all the given criteria` | `signtool` never loaded the signing library and fell back to the local certificate store — an SDK older than 10.0.22621.755, or the wrong architecture. |
| nothing at all, and a failure | the .NET 8 runtime is missing. `sign.ps1` checks for it, so this only happens if the check is bypassed with `-DlibDir`. |

### Testing the integration without signing anything

`scripts/release/sign-tests.ps1` runs twenty cases against `sign.ps1` — the
metadata it assembles, the flags it passes, that `-OutDir` never writes back over
what it was given, that verification passes a signed file and refuses a tampered
one, that an Azure CLI which is installed but not on the PATH is put there and
resolves by name afterwards, that a run with no sign-in refuses early and names
the command to run, that a sign-in which can no longer get a token is refused
with this machine's own tenant in the command it prints, and that a `signtool`
which never answers is killed and reported.

**It signs nothing.** Three of the cases hand it a `signtool` that is not one — a
few lines of C#, compiled by the compiler that ships with the .NET Framework,
which writes down the arguments it was given and sleeps when it is told to. That
is what lets a case say "signtool was never started" and mean it, and what lets
the timeout be tested without waiting three minutes for a real one.

It reads the sign-in state out of an `AZURE_CONFIG_DIR` it writes itself, so it
says the same thing on a machine somebody is signed in on. The token cases do
reach the sign-in service, because asking for a token is the thing under test;
they reach nothing else, and a machine with no network reads them as a refusal,
which is the same answer.

### Making the release page

Last, and only from the machine that just signed: the release is created by hand,
out of the directory the signed files are in.

The whole order, both platforms, with the four things 0.4.0 was caught out by
written into the steps they belong to:

1. **The Mac lane** — build, `bundle.sh`, `sign.sh --no-spctl`, `notarize.sh`,
   `dmg.sh`, the rename, `checksums.sh`. Gatekeeper is asked **once**, by
   `notarize.sh` after it staples, and that assessment is the one that must
   pass; `sign.sh`'s own is informational and never fatal. `bundle.sh` refuses a
   binary that is not this version at this commit, and a bundle missing any of
   the four documents. `dmg.sh` writes `Folio-macos-arm64.dmg` beside the image,
   and the rename moves both. `checksums.sh` writes `SHA256SUMS-macos.txt` with
   **bare file names**.
2. **The Windows lane** — `sbom.ps1`, then `package.ps1 -Sign`, which
   **empties `target/release-package`** before it writes, then `smoke.ps1`.
3. **Fetch the three macOS assets** — `Folio-<version>-macos-arm64.dmg`,
   `Folio-macos-arm64.dmg` and `SHA256SUMS-macos.txt` — from the Mac into
   `target/release-package`. After 2, never before it: step 2 empties that
   directory.
4. **Look at the directory.** Seven files — the four from the Windows lane and
   the three from the Mac — and every version in a name is this release's.
   Re-run `smoke.ps1` if anything was moved in or out since it last ran: it
   refuses a directory carrying another release's version.
5. **`gh release create`**, below, over that directory. The body is **copied
   from the file in `docs/plans/release/`**, with its first line — the banner
   saying what the file is — dropped; see **Release note shape**.
6. **winget**, and **the Homebrew tap** — `cask.sh`, under **macOS** above. Both
   name the release page, so both come after it exists.

```powershell
# The body, which is the repo file with its first line — the banner saying what
# the file is — and the blank line after it taken off. See **Release note
# shape** below.
$note = 'docs/plans/release/release-note-v0.4.0-preview.md'
$body = Join-Path ([IO.Path]::GetTempPath()) 'folio-release-body.md'
[IO.File]::WriteAllText($body, (((Get-Content -LiteralPath $note) | Select-Object -Skip 2) -join "`n") + "`n")

$assets = @(Get-ChildItem target/release-package -File | ForEach-Object { $_.FullName })
$arguments = @('release', 'create', 'v0.4.0-preview') + $assets + @(
    '--draft',
    '--title', 'Folio 0.4.0',
    '--notes-file', $body)
& gh @arguments
```

The list is built and splatted rather than written on one line: `gh` is a native
command, and an array interpolated into one takes its own view of quoting the day
a path has a space in it. `--draft` because a person reads the page and presses
the button. No `--prerelease`: from v0.4.1 the release is the one GitHub calls
latest, so `/releases/latest` answers (see **The tag** above); the update check
reads the list endpoint and is unaffected either way.

**The assets are whatever is in that directory, and the directory is the signed
build.** Nothing is typed out, so nothing can be left out; nothing is fetched
from a workflow run, so an unsigned file cannot arrive under a signed file's
name. Compare the two if you like — the workflow artifact from the tag's run has
the same file list, the same notices and the same version — but upload the local
one.

## Release note shape

**From 0.4.1 the release body has a fixed shape, and the long prose lives in
`CHANGELOG.md`.** A release page is read on a phone, in a notification, and by
somebody deciding in four seconds whether this release is worth their afternoon;
0.4.0's body ran to several screens of paragraphs, and almost all of them were
the changelog entry again, one directory away. So the page carries the short
form and links to the long one, and nothing is written twice.

`docs/plans/release/TEMPLATE.md` is the skeleton, and
`docs/plans/release/release-note-v<version>-preview.md` is this release's copy of
it. **That file is the published body**, which is what its first line says:

```
> The body of the GitHub Release for this version, as published.
```

That line is the only thing in the file that is not published — it is dropped,
with the blank line after it, on the way to `--notes-file`. There is no draft
banner and no "not final yet" in the body, because the file is edited until it
is right and then published as it stands.

The shape, in order:

1. **`# Folio <version>`**, and nothing above it.
2. **The download line** — the zip, `(Windows 10 1809+ / 11, 64-bit)`, a middle
   dot, the dmg, `(macOS 14+, Apple silicon)`. **Both name this release's own
   assets at this release's tag, and not `/releases/latest/download/`**: the page
   is about one build, and a fixed-name link on it would hand a reader of an
   older page whatever shipped since. The fixed names are for pages that are
   about Folio rather than about a version. Then the Chinese download line,
   and a link to the Chinese note. Both Chinese lines are written by the
   translation lane; in `TEMPLATE.md` they are empty.
3. **`## Highlights`** — three to five bullets, one sentence each, about what a
   reader can now do. No file names, no flags, no crate names.
4. **`## Changes`** — `### Added`, `### Changed`, `### Fixed`, each a short list
   with one line per item. **No paragraphs.** An item that needs a paragraph to
   be honest has a paragraph in `CHANGELOG.md`, and the line here is the
   sentence that sends a reader there.
5. **One `<details>` block**, summarised
   `Install notes (SmartScreen, Gatekeeper, checksums)`, holding what used to be
   the *Download and verify* section: the asset tables, the first-run warnings
   for each system, and the commands that check a download against the checksum
   file. Collapsed, because it is read once by the people who need it and
   scrolled past by everybody else.
6. **The last line**: `Full changelog:` with a link into `CHANGELOG.md` **pinned
   to this release's tag**, at the version's own anchor, and a compare link from
   the previous tag to this one. Pinned to the tag rather than `main` for the
   reason no public document carries a versioned download link: a link that
   moves is a link that tells a reader of 0.4.1 what 0.6 changed.

A `## Known issues` list, when there is one, goes between **Changes** and the
`<details>` block, under the same rule as everything above it: short bullets,
one line each.

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
user sets `Settings ▸ General ▸ Explorer context menu` to `On the first page`,
and that registration is per-user and needs no elevation — no administrator, no installer,
no service. **Nothing in this repository registers a package on the machine that
built it.** A build that registered its own output would be a build that changed
the developer's Explorer menu and left it changed, and it would test the
registration on the one machine where it cannot fail interestingly.

The classic `HKCU\Software\Classes` verb — the one that reaches the "Show more
options" page and Windows 10 — is not replaced by this and stays where it is:
the answer above writes both. `docs/DESIGN.md` §7.4a is where that decision is
written down, and §7.4b is where the two rows became one.

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

## winget

`winget install WeiyiShi.Folio` is the same archive, fetched from the same
release page and checked against the same hash. Nothing new is built for it and
no new asset is published: what lives in this repository is three YAML files
under `packaging/winget/manifests/w/WeiyiShi/Folio/<version>/`, laid out in the
directory shape `microsoft/winget-pkgs` uses so that the folder can be copied
into a fork of that repository unchanged.

### The one field the whole thing rests on

`InstallerType: zip` with `NestedInstallerType: portable` would, by default,
extract the archive and put a **symlink** to `folio.exe` in
`%LOCALAPPDATA%\Microsoft\WinGet\Links`. That is the one arrangement this
product cannot survive: `folio.exe` looks for `conpty.dll` and
`OpenConsole.exe` beside `current_exe()` and nowhere else, and `folio.msix`
names the extracted folder as the program's external location. A link in a
different directory is a `folio.exe` with no ConPTY and a package that
registers against nothing.

`ArchiveBinariesDependOnPath: true` is what turns that off. With it, winget
extracts the whole archive into one real folder under
`%LOCALAPPDATA%\Microsoft\WinGet\Packages\` and puts **that folder** on the
user's `PATH`, so all nine files sit beside each other exactly as they do for
somebody who unzipped the release by hand. It arrived in winget-cli 1.9 and is
carried by manifest schema 1.9.0 onward; leaving it unset does not fail
validation, it just quietly reintroduces the symlink. Do not remove it.

`RelativeFilePath` names the folder **inside** the archive, which is
`folio-<version>\folio.exe` — `package.ps1` zips the staging folder rather than
its contents, and that folder is `folio-<version>`, while the archive itself is
`folio-<version>-windows-x64.zip`. The two names differ by four words and a
manifest that repeats the archive's name here points at a path that is not in
the archive.

### What to change for a release

**The version's folder is made at release time, out of the last one.** The
manifests in this repository are `0.2.2`'s. A new version's three files are a
copy of that folder under the new number, with the values below changed in the
copy — and one of those values, `InstallerSha256`, does not exist until
`package.ps1` has signed and hashed the archive, so a folder created at
release-prep time is a folder with a hash from the previous release in it or a
hash somebody will have to remember to come back for. Copy it after the release
page is published, when all six values can be filled in at once.

Four values, and three of them are the version:

| field | file | where it comes from |
| --- | --- | --- |
| `PackageVersion` | all three | the workspace version, without the tag's `-preview` |
| `RelativeFilePath` | installer | `folio-<version>\folio.exe` |
| `InstallerUrl` | installer | the release page's zip asset, at its versioned name under this release's tag — **never** `/releases/latest/download/`, which would make the manifest for one version hand out another |
| `InstallerSha256` | installer | the `folio-<version>-windows-x64.zip` line of `SHA256SUMS.txt`, in upper case |
| `ReleaseDate` | installer | the day the release page was published |
| `PrivacyUrl`, `LicenseUrl`, `ReleaseNotesUrl` | locale | the same URLs at the new tag |

**The hash is copied out of `SHA256SUMS.txt`, never typed and never recomputed
from a second download.** That file is written by `package.ps1` over the signed
bytes, so it is the hash of what was uploaded; a hash that disagrees with the
asset is the most common reason a submission is refused, and it is the one
failure mode that a copy-paste cannot produce and a retype can.

The version number carries no channel. `-preview` lives in the tag and in the
URLs the manifest points at, and the `PackageVersion` is `0.2.2`; winget's own
precedent for a pre-release channel is a second `PackageIdentifier`
(`Microsoft.VisualStudioCode.Insiders`), not a suffixed version. That the build
is a preview is said in the locale manifest's `Description`, where a person
reads it.

### Validating before submitting

```powershell
winget validate --manifest packaging\winget\manifests\w\WeiyiShi\Folio\<version>
```

and then, on a machine that is **not** the one doing the work — the Windows 10
virtual machine of `docs/plans/release/clean-vm.md`, or Windows Sandbox — the
install itself, because `validate` only reads the YAML and the failure this
manifest exists to avoid is a runtime one:

```powershell
winget settings --enable LocalManifestFiles              # once, elevated
winget settings --enable LocalArchiveMalwareScanOverride # once, elevated
winget install --manifest <the version folder> --ignore-local-archive-malware-scan
```

The second setting and the flag are not a way around anything a user meets.
`winget install --manifest` runs a malware scan of the archive that installing
from the winget source does not — winget's own help says the scan is "performed
as part of installing an archive type package **from local manifest**" — and on
the clean Windows 10 machine that scan refuses this archive while Windows
Defender, asked directly about the same bytes on the same machine, finds nothing
in it. Without the override the local test cannot be run at all; nobody
installing the published package is asked the question.

What that has to show, beyond exiting 0: `folio` resolves on `PATH` to the
extracted folder under `WinGet\Packages` and **not** to anything in
`WinGet\Links`; `folio --version` prints the version this release claims;
`conpty.dll`, `OpenConsole.exe` and `folio.msix` are in the same directory as
the `folio.exe` that `PATH` resolved to; and `winget uninstall WeiyiShi.Folio`
takes the folder and the `PATH` entry away again.

### Submitting

The first submission is a pull request opened by hand, so that a new
publisher's first refusal is read by a person rather than by a workflow:

```powershell
gh repo fork microsoft/winget-pkgs --clone --remote
git -C winget-pkgs checkout -b WeiyiShi.Folio-<version>
# copy packaging/winget/manifests/w/WeiyiShi/Folio/<version>/ to the same path
git -C winget-pkgs add manifests/w/WeiyiShi/Folio/<version>
git -C winget-pkgs commit -m "New package: WeiyiShi.Folio version <version>"
git -C winget-pkgs push -u origin WeiyiShi.Folio-<version>
gh pr create --repo microsoft/winget-pkgs --head <account>:WeiyiShi.Folio-<version>
```

The subsequent-version equivalent is `wingetcreate update WeiyiShi.Folio
--version <version> --urls <the zip>|x64 --submit`, which downloads the asset to
compute the hash itself and adds a folder rather than replacing one. Versions
accumulate in `winget-pkgs` the same way they accumulate on the releases page.

Before opening it, read the checklist in that repository's pull-request
template, and expect these to be what is asked about:

- the folder path matches the identifier, `manifests/w/WeiyiShi/Folio/<version>`,
  and every file in it repeats the same `PackageIdentifier` and `PackageVersion`;
- `InstallerSha256` matches the asset the URL serves;
- the installer is not a script — a `zip` of a portable is fine, a `.ps1` or a
  `.bat` is refused outright;
- the install is silent, which a portable archive is by having no installer at all;
- the manifests validate against the schema version they declare, and the
  automated pipeline runs `winget validate` again on the pull request;
- the binary passes an antivirus and security scan. This is the one that costs a
  new independent publisher time and is not in our gift beyond what is already
  true: `folio.exe` and `folio.msix` are Authenticode-signed by a real
  certificate rather than a self-signed one. Budget a review cycle for it.

Only after a submission has cleared moderation once is it worth automating the
rest with `vedantmgoyal9/winget-releaser` on `release: types: [released]`, with
an `installers-regex` narrow enough to match only the zip — the release page
also carries the bill of materials and `SHA256SUMS.txt`, and neither of those is
an installer. The trigger is the release
event and not a tag push, because a tag push here only builds an unsigned
rehearsal and the release page is made by a person from the signed machine.

## macOS

A macOS release is a short sequence on the Mac and one step of it needs a
person: the Developer ID private key is in a keychain, and a keychain has to be
opened by somebody who knows the password. Notarization is the half that does work
headlessly, because it authenticates with a key file rather than a keychain.

`scripts/release/macos/` holds those steps, as POSIX `sh` scripts, and the
order of this list is the order they run in.

| | |
| --- | --- |
| `bundle.sh --out <dir>` | Assembles `Folio.app` from `target/release/folio` — the rendered plist, the icon built from `assets/app-icon/folio.ico`, `PkgInfo`, the two licences, the third-party notices and the trademark notice — asks the executable whether it is this version at this commit, reads the tree back and refuses a bundle that is not exactly those eight files, and runs `dsymutil` into `Folio.app.dSYM` **beside** the bundle, never inside it. |
| `sign.sh --app <bundle> --identity <id>` | Signs with the hardened runtime, a secure timestamp and `packaging/macos/entitlements.plist`, inside out, then reads the signature back. Its Gatekeeper verdict is informational and never fatal, and `--no-spctl` skips it, which is what the recipes and the workflow pass. `--identity` defaults to `-`, which is ad-hoc: that is how the script is exercised on a machine with no certificate. |
| `notarize.sh --path <bundle or image>` | Submits, waits, keeps the notarization log beside the artifact, staples the ticket to it, and asks Gatekeeper the one question that has to be answered `accepted`, `source=Notarized Developer ID`. |
| `dmg.sh --app <bundle> --out <dir> --identity <id>` | Stages the application beside a link to `/Applications`, writes the compressed read-only image, and signs, notarizes and staples that too. Then copies the stapled image to `Folio-macos-arm64.dmg` beside it — the same bytes, under the name `/releases/latest/download/` resolves — and hashes the two against each other. |
| `checksums.sh --dir <dir>` | Hashes every file in the directory the release page is made of, from inside it, into `SHA256SUMS-macos.txt`, and reads the file back with `shasum -c`. |
| `cask.sh <version> <sha256>` | Prints the Homebrew cask for this release, so the tap is a copy rather than two fields somebody retypes. |

`packaging/macos/README.md` carries the same sequence with every flag and what
each step's output should say; it is beside the scripts so that the two cannot
drift. What follows here is what that file does not decide: who unlocks the key,
what the lane needs, what is kept, and what the owner has to see before the tag
is published.

### The one-time preparation, and the part only the owner can do

1. **Point the developer tools at Xcode**, once per machine:
   `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer` and
   `sudo xcodebuild -license accept`. `xcode-select -p` must then print the
   Xcode path.
2. **Create the Developer ID Application certificate** — Xcode ▸ Settings ▸
   Accounts ▸ the team ▸ Manage Certificates ▸ **+** ▸ Developer ID
   Application. `security find-identity -v -p codesigning` must name a
   `Developer ID Application: … (TEAMID)` line and end `1 valid identities
   found`. The team id is read from that line at release time and is not
   written into this repository.
3. **Create the App Store Connect key** for notarization, and put it at
   `~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8`, mode `600`, with the
   key id and the issuer id beside it in
   `~/.appstoreconnect/folio-notary.env`. Those two ids are identifiers rather
   than secrets; the `.p8` is the secret. Nothing of this goes in the
   repository, and `packaging/macos/.gitignore` refuses a `.p8` and a `.p12` at
   the place they would land.

**The signing session must have the keychain open, and an ssh session does not
have one.** Measured: `xcrun notarytool history --keychain-profile folio` over a
non-interactive ssh answers `keychainLocked`, and `codesign`'s reach for the
private key fails the same way, with `errSecInternalComponent`. So the signing
step is run at the machine, or from a session where the owner has unlocked a
keychain for the duration of the release:

```sh
security unlock-keychain ~/Library/Keychains/folio-signing.keychain-db   # asks for the password
security list-keychains -d user -s ~/Library/Keychains/folio-signing.keychain-db login.keychain-db
# … the release …
security lock-keychain ~/Library/Keychains/folio-signing.keychain-db
```

A dedicated signing keychain rather than the login one is what keeps the
unlocked window the length of a release instead of the length of a login.

### Every release

```sh
export RUSTUP_TOOLCHAIN=1.94.1-aarch64-apple-darwin     # docs/BUILDING.md says why
cargo build --release -p bt-app
scripts/release/macos/bundle.sh --out target/macos
scripts/release/macos/sign.sh   --app target/macos/Folio.app \
    --identity "Developer ID Application: … (TEAMID)" --no-spctl
scripts/release/macos/notarize.sh --path target/macos/Folio.app
scripts/release/macos/dmg.sh    --app target/macos/Folio.app --out target/macos \
    --identity "Developer ID Application: … (TEAMID)"

mkdir -p target/macos-package
mv target/macos/Folio.dmg target/macos-package/Folio-<version>-macos-arm64.dmg
mv target/macos/Folio-macos-arm64.dmg target/macos-package/Folio-macos-arm64.dmg
scripts/release/macos/checksums.sh --dir target/macos-package
```

`notarize.sh` and `dmg.sh` both take `--dry-run`, which prints every command
with the real paths filled in and touches nothing.

**Gatekeeper is asked once, and after notarization.** `--no-spctl` on the
signing line is that decision, and it is the flag the workflow's macOS lane
already passes: asked between signing and notarization the answer is `rejected`
with `source=Unnotarized Developer ID`, which is true of everything that has not
been notarized yet and says nothing about this signature. The assessment that
decides is `notarize.sh`'s, made **after** the ticket is stapled, on the bundle
and again on the image; a refusal there — or an `accepted` whose source is not
`Notarized Developer ID` — stops the release.

Should you run `sign.sh` without `--no-spctl`, it prints that verdict and exits
**0** on it. It used to exit 1, and the 0.4.0 run took that for a failure and
stopped one step before the step that fixes it; a script that fails on the
expected answer is a script that cannot be put in a sequence.

**`bundle.sh` asks the executable what it is.** `--version` has to answer
`Folio <version> (<commit>)` with the version the plist was just rendered with
and the short hash of `HEAD`, or the bundle is refused: the plist is written
from this checkout and the binary is copied from wherever the build left it, and
nothing else in the lane compares the two. Build the tree you are bundling.

**`checksums.sh` runs from inside the directory, so the lines it writes are
bare file names.** `shasum -a 256 <dir>/<file>` writes the path it was given
back out, and a reader who has put the image and the checksum file in one folder
is then told there is no `target/macos-package/` there — which is how 0.4.0's
`SHA256SUMS-macos.txt` came to be rewritten by hand. The file covers everything
in the directory except itself, in the same format `package.ps1` writes on the
Windows side.

**The two licences, the notices and the trademark notice ship inside the
application.** `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md` and
`TRADEMARK.md` — the same four the Windows archive carries among its nine, for
the same reasons: MIT and Apache-2.0 both ask that the notice accompany the
distribution, and `THIRD-PARTY-NOTICES.md` is how `option-ext`'s MPL-2.0 §3.2
obligation is met. On macOS the only thing that survives the drag to
`/Applications` is the bundle, so `bundle.sh` copies all four into
`Contents/Resources/` — before signing, so the seal covers them — and refuses to
produce a bundle that does not hold them. That is why the disk image carries no
licence file of its own: the application carries them. **Through 0.4.0 neither
macOS download carried any of it**, which is the one thing in this section that
was a defect rather than a decision.

**The image is written as `Folio.dmg` and published as
`Folio-<version>-macos-arm64.dmg`.** The script writes the short name because a
script that built the long one would be a second reader of the version line;
the rename happens on the way to the release page, in the lane below or by the
person making the page. Both halves of the published name earn their place —
the version, because the tag and the asset are one claim, and the architecture,
because this preview is arm64 and an Intel Mac must be able to tell from the
name that this is not for it.

**A copy of it is published as `Folio-macos-arm64.dmg`, and `dmg.sh` makes that
one.** It is written after the ticket is stapled, out of the stapled image, and
hashed against it: a copy taken any earlier would be the file most people click
and the one Gatekeeper turns away offline. The rename moves it across under the
name it already has, because that name is the whole point —
`/releases/latest/download/` resolves an asset by name, and a name with a
version in it cannot be linked to from outside this repository without going
stale. `checksums.sh` hashes the directory, so both names land in
`SHA256SUMS-macos.txt` under one hash.

**Never sign anything again after it has been stapled.** The ticket lives inside
the signed artifact and a second `codesign` throws it away, so
`stapler validate` fails on a build that was notarized minutes earlier. That is
also why the image is built from the stapled application rather than the other
way round.

### What must be seen before the tag is published

In this order, on the Mac, against the artifacts that are actually going to be
uploaded:

```sh
codesign --verify --deep --strict --verbose=2 target/macos/Folio.app
spctl -a -vvv target/macos/Folio.app
dmg=target/macos-package/Folio-<version>-macos-arm64.dmg
spctl -a -vvv -t open --context context:primary-signature "$dmg"
xcrun stapler validate target/macos/Folio.app
xcrun stapler validate "$dmg"
```

The second must say `accepted` **and** `source=Notarized Developer ID` — an
`accepted` with any other source is a different claim. `stapler validate` passes
on **both** the application and the image, and not on one of the two: a stapled
image holding an unstapled application is a download that works until the reader
is offline.

`notarize.sh` reads that same `source=` line itself, on each of the two
artifacts, and stops the release on anything else. The list is asked again here
because here it is asked of the files that are actually about to be uploaded —
after the renaming and the moving, of the bytes that go up.

Then the clean-user pass: from a second macOS account, fetch the image over the
network rather than copying it, open it, drag Folio to Applications and launch
it. The ordinary identified-developer confirmation is expected. An
**unidentified developer** panel, a notarization failure, or **damaged and
can't be opened** is a release that does not go out. Disconnect the network and
launch once more, to see the stapled ticket used rather than a check that
happened to reach Apple.

### What is kept beside the artifact

Two files, archived with the tag and **not** published:

- **The notarization log**, `<artifact>.notarylog.json`, one per submission —
  the application's and the image's. It is the only record of what the notary
  service looked at, and Apple keeps it for a limited time.
- **`Folio.app.dSYM`**, from `bundle.sh`. The release profile carries
  line-tables-only debug information, which stays in the object files on Apple
  targets, so this is the only thing that turns a crash report from a shipped
  build back into file names and line numbers. It pairs with the binary by UUID
  and exists only on the machine that linked it — lose it and every crash report
  from that release is names without lines for ever.

They are kept beside the artifact rather than inside it: the `.dSYM` is several
times the download and would be signed for no reason, and a notarization log is
nobody's business but the project's.

### The Homebrew tap

`brew install --cask lulu-loopp/folio/folio` reads one file —
`Casks/folio.rb` in the **`lulu-loopp/homebrew-folio`** repository — and two
fields in it change at every release: `version`, which the download URL is built
out of, and `sha256`, which Homebrew checks the image against before it unpacks
anything. Until they are changed, `brew` installs the previous release.

**This is the last step, and it happens after the release page is published**,
because the URL the cask names has to resolve and the hash has to be the hash of
the file that was uploaded.

```sh
gh api repos/lulu-loopp/homebrew-folio/contents/Casks/folio.rb --jq .content \
    | base64 -d > /tmp/folio.rb
scripts/release/macos/cask.sh <version> <the dmg's sha256> --file /tmp/folio.rb --in-place
```

Then commit `/tmp/folio.rb` to the tap as `Casks/folio.rb`, and check it with
`brew fetch --cask lulu-loopp/folio/folio` — which downloads the image and
compares the hash, so it either agrees with the release page or says which of
the two is wrong.

**The hash is copied out of `SHA256SUMS-macos.txt`, never recomputed from a
second download** — the same rule winget's `InstallerSha256` follows, for the
same reason: a hash taken from a second download is a hash of that download.
`cask.sh` refuses anything that is not a version with no `v` and no `-preview`
on it, and anything that is not 64 lower-case hexadecimal digits.

With `--file` only those two lines are replaced and everything else in the cask
is left exactly as the tap has it. Run without `--file`, it prints the whole
cask from the shape this repository knows about, which is the answer to "there
is no tap yet" and not the way to update one. The `-preview` in the URL is part
of the **tag**, not the version; a release tagged any other way needs that line
changed once, by hand, in the tap.

### The lane, and its four secrets

`.github/workflows/release.yml`'s macOS lane builds, bundles, signs, notarizes
and packs the image on a macOS runner, and — like the Windows job — **publishes
nothing**. It reads four repository secrets:

| secret | what it holds |
| --- | --- |
| `MACOS_CERTIFICATE_P12` | The Developer ID Application identity exported as a `.p12`, base64-encoded. |
| `MACOS_CERTIFICATE_PASSWORD` | The password that `.p12` was exported with. |
| `MACOS_KEYCHAIN_PASSWORD` | The password the lane creates its throwaway keychain with, so that the identity is imported into a keychain that exists for the length of the job and is deleted at the end of it. |
| `MACOS_NOTARY_KEY_P8` | The App Store Connect key file, base64-encoded. |

**These four names are fixed here, and the lane reads exactly them.** A secret
is the one kind of configuration nobody can read back to check, so a lane that
spells one differently fails with an empty value and a message about a
certificate rather than about a name — and the place that names are agreed on
has to be the place a person looks, which is this document.

The key id and the issuer id are **repository variables and not secrets**,
`MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER_ID`: they identify a key rather
than open one, and a value that is not secret should not be stored as though it
were, where nobody can read it back to check it.

**Exporting the `.p12`.** On the machine that holds the identity:

```sh
security find-identity -v -p codesigning        # confirm there is exactly one
# Keychain Access ▸ My Certificates ▸ the Developer ID Application row ▸
# right-click ▸ Export… ▸ Personal Information Exchange (.p12), with a password
base64 -i Folio-DeveloperID.p12 | pbcopy        # paste into the secret
rm Folio-DeveloperID.p12                        # the keychain still has it
```

The export must be taken from **My Certificates** and not from Certificates: the
second exports the certificate without the private key, and a `.p12` with no key
imports without complaint and then fails at `codesign` with no certificate
found. Export the `.p8` the same way — `base64 -i AuthKey_<KEYID>.p8` — and keep
neither copy on disk afterwards.

A lane cannot do the clean-user pass, and the signature it makes is the same
signature the owner's machine makes. What it is for is the same thing the
Windows job is for: proving that this commit builds and packages on a machine
nobody has been working on.

### The version in the bundle

`packaging/macos/Info.plist.in` is a **template**, not a plist. Its
`CFBundleShortVersionString` and `CFBundleVersion` are both the literal
`@VERSION@`, and both are filled at bundle time from `[workspace.package]
version` in the workspace `Cargo.toml` — the same line
`bt_app::version::tests::the_version_is_the_manifests_and_nothing_elses` already
holds `folio --version`, the PE `VERSIONINFO` block and every diagnostic header
to. That is why the template is not on that test's list of places carrying a
version: it carries none, so there is nothing for the list to disagree with, and
the count stays at one. Writing a real number into either field by hand is
exactly the drift the gate exists to catch, and it would not be caught there
until the next release moved the other four. M5-5 closes that by extending the
gate over the *generated* plist's two fields, which is where a literal finally
appears.

One consequence for the tag: `CFBundleVersion` accepts only dotted integers, so
a `-preview` suffix could not go in it. It never has to — the suffix is a
release channel that lives on the tag and never reaches the manifest, as the
first section of this document says. The renderer below does not take one on
trust: a version that is not one to three dotted integers is refused rather than
truncated, so the day somebody puts a channel suffix in the manifest, the bundle
step stops instead of shipping a build the system cannot order against the one
before it.

### Rendering the plist

M5-5 closed the gate with a renderer, and the bundle step calls it:

```sh
cargo run -q -p bt-winres --bin render-info-plist > "$app/Contents/Info.plist"
```

Stdout is the finished plist and nothing else; a version the bundle cannot carry
and a placeholder nothing fills both go to stderr with a non-zero exit, which
under `set -e` stops the script before `codesign` sees the file. The version is
`bt-winres`'s `CARGO_PKG_VERSION` — the workspace line — so the release script
never reads `Cargo.toml` itself.

The renderer is `crates/bt-winres/src/plist.rs`, in the crate that already turns
that same line into the Windows `VERSIONINFO` numbers, and it has no
dependencies, so the bundle machine compiles one small crate to get the file.
The gate over it is
`bt_app::version::tests::the_plist_carries_the_workspace_version_twice_and_no_literal`,
beside the one named above: it renders `packaging/macos/Info.plist.in`, checks
both version fields against the manifest's line, and checks that the template
itself still carries no version at all.

### Assembling, signing, notarizing

`scripts/release/macos/` holds four POSIX `sh` scripts, and the order they run in
is the order of this list:

- **`bundle.sh --out <dir>`** assembles `Folio.app` — the rendered plist, the
  release executable as `Contents/MacOS/folio`, `Contents/Resources/Folio.icns`
  built from `assets/app-icon/folio.ico` with `sips` and `iconutil`, and
  `PkgInfo`. It then runs `dsymutil` into `Folio.app.dSYM` **beside** the bundle
  and prints the tree with sizes. The `.dSYM` is archived with the tag and never
  published: it is what turns a crash report from that build back into file names
  and line numbers, it pairs with the image by UUID, and it exists only on the
  machine that linked the binary.
- **`sign.sh --app <bundle> --identity <id>`** signs with `--options runtime`,
  `--timestamp` and `packaging/macos/entitlements.plist`, in nested-code order,
  then reads the signature back: `codesign --verify --deep --strict --verbose=2`,
  `codesign -dv --verbose=4`, the entitlements as signed, and `spctl -a -vvv`.
  `--identity` defaults to `-`, which is ad-hoc and which Gatekeeper correctly
  rejects; that is how the script is exercised without the owner's certificate,
  and on a real identity a Gatekeeper refusal is a non-zero exit.
- **`notarize.sh --path <bundle or image>`** submits, waits, keeps
  `notarytool log` beside the artifact as `<artifact>.notarylog.json`, and
  staples. A `.app` goes up inside a `ditto` zip because `notarytool` takes one
  file; the ticket comes back stapled to the bundle and the zip is thrown away.
- **`dmg.sh --app <bundle> --out <dir> --identity <id>`** stages the application
  beside a link to `/Applications`, writes a compressed read-only image with
  `hdiutil create`, signs it, notarizes it, staples it, and ends with
  `spctl -a -vvv -t open --context context:primary-signature`, which is the
  assessment a downloaded image actually gets. It leaves two files: `Folio.dmg`,
  which the rename gives the published versioned name to, and
  `Folio-macos-arm64.dmg`, the same bytes under the name that never changes.

Both of the last two take `--dry-run`, which prints every command with the real
paths filled in and touches nothing.

The exact sequence, with the flags and what each step's output should say, is in
`packaging/macos/README.md`. Two rules from it are worth repeating here because
they are the two ways a release goes wrong quietly:

- **The signing session must have the login keychain open.** Over ssh,
  `codesign` answers `errSecInternalComponent` and no amount of retrying changes
  it. Notarization is not affected — it authenticates with an App Store Connect
  API key file rather than a keychain.
- **Never re-sign after stapling.** The ticket lives inside the signed artifact,
  and a second `codesign` throws it away; `stapler validate` then fails on a
  build that was notarized minutes earlier.

The credentials are not in this repository and `packaging/macos/.gitignore`
refuses them at the place they would land: the `.p8` App Store Connect key and
any exported `.p12` identity. The key id and issuer id are identifiers rather
than secrets and live in `~/.appstoreconnect/folio-notary.env`, which is the one
home-directory path any of these scripts reads.

<!--
  M6-2. Written against this section as `main` has it. When `feature/macos-docs`
  (M5-6) lands, this subsection belongs between *What must be seen before the tag
  is published* — which ends on "the clean-user pass" — and *What is kept beside
  the artifact*, unchanged except that its first paragraph can then say "the pass
  above" instead of naming it.
-->

### Clean-machine coverage

The clean-user pass — a second macOS account, the image fetched over the network
— is what the release actually gets, and `docs/plans/port/m6-1-checklist.md` is
the walk. It is **not** a clean machine, and the difference is not a formality:
an account is the unit of the data directory, the Downloads folder and the
quarantine attribute, of the privacy grants, of the Services registration and of
the LaunchServices database, but it is **not** the unit of Gatekeeper. One
directory, `/var/db/SystemPolicyConfiguration`, belongs to the whole machine, and
two of the things in it are exactly what this pass is about: `ExecPolicy`, what
this machine has already assessed and approved, and `Tickets`, the notarization
tickets it has already fetched. So a second account can be shown the
identified-developer panel only for a build the machine as a whole has never
opened, and an offline launch tells a stapled ticket from a cached one only if
nothing on the machine went online with that build first. Both are orderings
rather than obstacles, and the checklist is written in that order — first open on
the clean account, first open offline — with the weaker claim spelled out for the
case where the second is not possible.

What no account on one Mac can cover: that Folio needs nothing the development
machine happens to have. Folio installs no kernel extension and no system
service, so that class is nearly empty, but "nearly" is not "empty", and the
release lane covers the rest of it on a machine that is genuinely clean and
genuinely not ours. `.github/workflows/release.yml`'s `macos-artifact` job
downloads the artifact the build job uploaded, on a second runner that did not
build it, checks it against the `SHA256SUMS.txt` the release page publishes, and
then asks the downloaded bytes the questions a download is asked: `spctl` on the
image with `-t open --context context:primary-signature`, the application inside
the mounted image through `codesign --verify --deep --strict`, `spctl` again on
that, and `xcrun stapler validate` on both. A runner has no screen, and
`download-artifact` restores bytes rather than extended attributes, so what it
cannot do is the quarantine attribute and every panel — which is the half the
account covers. Between the two, everything in § M6's acceptance is exercised
except one thing, named below.

**A virtual machine is not taken for 0.4, and these are the numbers.** Measured
on the release Mac, 2026-09-13, without installing anything:

| | |
| --- | --- |
| `Virtualization.framework` | present; the machine is Apple silicon on macOS 26.6.2, and `swiftc` (Swift 6.3.3) and Xcode are installed, so a guest could be written and run without a package manager |
| The restore image | 19,772,231,540 bytes — 18.4 GiB — for the matching build; the link measured at about 21 MB/s, so roughly a quarter of an hour to fetch |
| Free space | 47 GiB on the internal volume, against that image plus a guest disk which Apple's own sample sizes at 64 GB and a macOS install fills about half of. It does not fit with room to work; the external volume has the space and is the owner's media, out of bounds |
| Memory | 16 GiB in total, shared with a production service that runs on this machine full time, at half free with nothing else running. A guest sized for a graphical macOS takes half the machine |
| Tooling | no Tart, no UTM, no package manager. The plan's 2026-09-12 note assumed one of the first two; both are installations this machine does not take, so the route that installs nothing is a Swift program against the framework, ad-hoc signed with the virtualization entitlement — a day of work before the first guest boots |
| **The display** | **the point of order.** A guest's screen exists only inside an AppKit view in a windowed application in a logged-in graphical session. There is no remote console in the framework, and every single thing § M6 asks to *see* — the identified-developer panel, the drag to `/Applications`, the notification and Accessibility prompts, the Services menu — is drawn by the window server. A guest therefore still has to be driven by a person sitting at a Mac, which is precisely what the second account already costs, and it does not buy an unattended acceptance |

What that leaves uncovered, written down rather than closed: **a machine that has
never had this build, a Rust toolchain, or Xcode on it, driven by a person.** The
runner gives the first half without a person; the second account gives the person
without the machine. The one acceptance line that falls in the gap is the offline
launch, and only in the case where the network could not be pulled before the
first open. Nothing else in § M6 depends on it.

Revisit when the machine changes — more memory, or a second Mac — or when a
defect arrives that a development install would have masked. Until then the gap
is the accepted one, and it is this paragraph.
