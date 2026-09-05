# Releasing

The release workflow (`.github/workflows/release.yml`) runs three scripts in this
order, and each of them can be run by hand exactly as it runs there:

| script | what it produces |
| --- | --- |
| `scripts/release/sbom.ps1` | the bill of materials, written into the output directory |
| `scripts/release/package.ps1` | `folio-<version>-windows-x64.zip`, the MPL-2.0 crate archive, and `SHA256SUMS.txt` over everything beside them |
| `scripts/release/smoke.ps1` | starts the executable that was built and checks the six things a green build can still be broken about |

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

`folio.exe` is signed. `conpty.dll` and `OpenConsole.exe` are Microsoft's, and
they arrive from Microsoft's own package already signed by Microsoft; putting our
signature over theirs would replace a statement Windows already trusts with a
newer and weaker one. `package.ps1 -Sign` checks that the signature they came
with is still valid and still time stamped, and signs neither. The four text
files in the archive carry no signature because no text file can.

### One-time preparation

1. **A Windows SDK**, for `signtool.exe`. Any install of the SDK that includes
   the signing tools will do, as long as it is **10.0.22621.755 or newer** —
   `sign.ps1` picks the newest x64 one under `Windows Kits\10\bin` and refuses an
   older one by name. An older `signtool` does not fail loudly: it ignores the
   signing library, looks in the machine's own certificate store instead, and
   reports that it found no certificate there.

2. **The .NET 8 runtime, x64.** The signing library is a .NET 8 assembly hosted
   inside `signtool`'s native process. Missing, it is the failure Microsoft's own
   troubleshooting page describes as "signing fails with no error code";
   `sign.ps1` checks for it first and says so instead.

3. **The Azure CLI**, `winget install -e --id Microsoft.AzureCLI`. It is not the
   only way to be signed in — the library asks `DefaultAzureCredential`, which
   also reads a service principal out of `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and
   `AZURE_CLIENT_SECRET` — but it is the way a person at a laptop does it.

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
./scripts/release/smoke.ps1 -Exe target/release/folio.exe -ExpectSigned
```

`package.ps1 -Sign` signs `folio.exe` where the build left it, *before* the
archive is built and before `SHA256SUMS.txt` is written, so the hash published
beside the archive is the hash of the signed bytes and the executable `smoke.ps1`
starts afterwards is the executable that ships.

`-ExpectSigned` makes `smoke.ps1` refuse an executable that is not signed, is
signed by somebody else, or is signed without a time stamp. Leave it off for an
ordinary build, which is unsigned and is meant to be.

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

`sign.ps1 -DryRun` resolves every tool, writes the metadata, prints the exact
`signtool` command it would run, and stops. Almost everything that can be
misconfigured is visible in that output without asking the service anything.

| what you see | what it is |
| --- | --- |
| `not signed in to Azure` | nothing here has a credential. The script prints the `az login` line to run. |
| HTTP 401 | the sign-in expired. `az login --use-device-code` again. |
| HTTP 403 | the account is signed in but may not use this profile: check the role assignment, the account and profile names, and that the endpoint's region matches the account's. |
| `no certificates were found that met all the given criteria` | `signtool` never loaded the signing library and fell back to the local certificate store — an SDK older than 10.0.22621.755, or the wrong architecture. |
| nothing at all, and a failure | the .NET 8 runtime is missing. `sign.ps1` checks for it, so this only happens if the check is bypassed with `-DlibDir`. |

### Testing the integration without signing anything

`scripts/release/sign-tests.ps1` runs twelve cases against `sign.ps1` — the
metadata it assembles, the flags it passes, that `-OutDir` never writes back over
what it was given, that verification passes a signed file and refuses a tampered
one, and that a run with no credential refuses early and names the command to
run. It reaches no network and needs no sign-in. Run it after changing either
script.
