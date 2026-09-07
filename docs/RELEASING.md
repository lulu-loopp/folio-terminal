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

**Bump the versioned download link in both READMEs at release-prep time.**
`README.md` and `README.zh-CN.md` each name `folio-<version>-windows-x64.zip` in
the Download section and link it at
`/releases/download/v<version>-preview/folio-<version>-windows-x64.zip`, which is
the same claim as the tag and the manifest and goes stale the same way.
`/releases/latest` is not a way out of bumping it: it answers 404 on a repository
whose releases are all pre-releases.

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
| `scripts/release/package.ps1` | `folio.msix`, `folio-<version>-windows-x64.zip` with the package and the executable both in it, the MPL-2.0 crate archive, and `SHA256SUMS.txt` over everything beside them |
| `scripts/release/smoke.ps1` | starts the executable that was built and checks the seven things a green build can still be broken about |

## What gets published

**Every asset on a release page comes off the machine that signed it, and none
of it comes out of CI.** `target/release-package` on that machine is the whole of
it: what the three scripts leave there, after `package.ps1 -Sign` has signed the
executable and the package, is exactly what a reader downloads. `gh release
create` is handed that directory and no list is written down anywhere, so there
is no second naming of assets to disagree with it, and nothing is hand-picked out
of `dist/`.

The workflow builds the same directory on a runner and keeps it as a workflow
artifact. That copy is unsigned and its file names are identical, so it is never
uploaded anywhere a stranger can reach. It is there to be compared against — the
same file list, the same notices, the same version, from the same commit — and
then left where it is.

| asset | what it is |
| --- | --- |
| `folio-<version>-windows-x64.zip` | the nine files, in one folder |
| `folio.msix` | **an asset of its own as well as a file in the zip** |
| `option-ext-<version>.crate` | the MPL-2.0 source offer, made good by this release |
| `folio-<version>.cdx.json` | the CycloneDX bill of materials `sbom.ps1` writes |
| `SHA256SUMS.txt` | one line for each of the four above, in the format `sha256sum -c` reads |

`folio.msix` being both is deliberate and is not a duplicate to tidy away. It
has to be **in the zip**, because the package names the folder it was extracted
into and a registration against a folder with no `folio.exe` in it names a path
with nothing at it. It stays **beside the zip** because that copy is the one
`smoke.ps1 -ExpectSigned` opens to read the package identity out of, and because
`SHA256SUMS.txt` is written over the directory: a hash somebody can check
against the file they were handed. Both copies are the same bytes — one file,
packed once, signed once, then copied into the archive.

`scripts/release/smoke-tests.ps1` is `smoke.ps1`'s own self-test, and it is
about the one part of that script a green release does not exercise: the paths
it is handed. It builds nothing, signs nothing and starts nothing — each case
runs `smoke.ps1` in a child shell that was started in the repository and then
walked into a scratch folder, which is the one arrangement under which a
relative path has two answers, and reads the path the refusal names. Run it
after changing how `smoke.ps1` reads its arguments.

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

```powershell
$assets = @(Get-ChildItem target/release-package -File | ForEach-Object { $_.FullName })
$arguments = @('release', 'create', 'v0.2.2-preview') + $assets + @(
    '--draft', '--prerelease',
    '--title', 'Folio 0.2.2',
    '--notes-file', 'docs/plans/release/release-note-v0.2.2-preview.md')
& gh @arguments
```

The list is built and splatted rather than written on one line: `gh` is a native
command, and an array interpolated into one takes its own view of quoting the day
a path has a space in it. `--draft` because a person reads the page and presses
the button; `--prerelease` because every release so far has been one, and because
the update check reads the list endpoint for exactly that reason.

**The assets are whatever is in that directory, and the directory is the signed
build.** Nothing is typed out, so nothing can be left out; nothing is fetched
from a workflow run, so an unsigned file cannot arrive under a signed file's
name. Compare the two if you like — the workflow artifact from the tag's run has
the same file list, the same notices and the same version — but upload the local
one.

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

Four values, and three of them are the version:

| field | file | where it comes from |
| --- | --- | --- |
| `PackageVersion` | all three | the workspace version, without the tag's `-preview` |
| `RelativeFilePath` | installer | `folio-<version>\folio.exe` |
| `InstallerUrl` | installer | the release page's zip asset |
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
also carries `folio.msix`, the crate archive, the bill of materials and
`SHA256SUMS.txt`, and none of those is an installer. The trigger is the release
event and not a tag push, because a tag push here only builds an unsigned
rehearsal and the release page is made by a person from the signed machine.
