# Windows: CI builds; the signing machine packages

`build-release.yml` owns the shipped Windows compilation. It is separate from
`ci.yml` so ordinary PR validation does not pay for fat LTO, and separate from
the existing `release.yml` rehearsal so a `nextNN` candidate can request just
the build. It runs on `windows-latest` (the requested release build host;
ordinary Windows tests remain on `windows-2025`), on `v*` tags or manual dispatch
with a required `ref`. No workflow here publishes a release.

The checkout is full (`fetch-depth: 0`), without persisted credentials or a
target cache. The shared toolchain action reads the exact Windows MSVC channel
from `rust-toolchain.toml`. The command is `cargo build --release --locked -p
bt-app`; Cargo.toml still owns fat LTO, one codegen unit, unwind and line tables.
`build.rs` asks git for `rev-parse --short=10 HEAD`; it already handles detached
HEAD, worktrees and packed refs. Full history avoids shallow-history abbreviation
differences; the banner is the same as a clean local build of that commit with
the same repository objects. Git can lengthen an abbreviation on a collision.

The 30-day artifact `folio-windows-release-<full SHA>` contains `folio.exe`,
`folio.pdb`, the two build-extracted Microsoft ConPTY sidecars, the existing
`folio-<version>.cdx.json` SBOM, and `BUILDINFO.txt`. BUILDINFO is readable JSON:
schema, full commit, version, `rustc -Vv`, profile, build command, and SHA-256 of
each payload file. Save the PDB and BUILDINFO beyond retention for crash analysis.
These hashes detect corruption/mix-ups; they are not a cryptographic provenance
attestation independent of the GitHub account and workflow being trusted.

## Release steps

1. Coordinator pushes the reviewed commit and dispatches from the branch that
   contains this workflow (or pushes the release tag through the usual process):

   ```powershell
   gh workflow run build-release.yml --ref <workflow-branch> -f ref=<full-commit-or-tag>
   ```

2. After it succeeds, on the owner's signing machine, use a checkout at that
   exact commit, with its release documents and packaging inputs unchanged:

   ```powershell
   ./scripts/release/fetch-ci-build.ps1 -Ref <sha-or-tag> -Out target/ci-unsigned -Account <login>
   ./scripts/release/fetch-ci-build.ps1 -Ref <sha-or-tag> -Out target/ci-unsigned -Account <login> -Apply
   ```

   PowerShell 7 and `gh` are required. The default performs only read-only API
   queries and prints a plan. `-Account`, as in `update-manifests.ps1`, checks the
   active login; it never switches accounts or prints tokens. A branch/tag is
   resolved once per invocation, so use the printed full SHA for an immutable
   second invocation. Missing, expired or unsuccessful artifacts are refused.
   Dispatch `head_sha` may name the workflow branch rather than the input ref:
   selection uses the SHA in the artifact name, the successful producing
   workflow, and BUILDINFO's full commit. Hashes are checked before executing
   only `--version`; a wrong version, unknown hash or wrong commit is refused.
   Output must not exist; a failed download is unverified and must not be signed.

3. Keep the unsigned original and sign a working copy. This performs no Cargo
   command on the workstation:

   ```powershell
   Copy-Item target/ci-unsigned target/ci-signing -Recurse
   ./scripts/release/package.ps1 -Binary target/ci-signing -Sign
   ./scripts/release/smoke.ps1 -Exe target/ci-signing/folio.exe -ExpectSigned
   ```

   Use a new `ci-signing` directory. `-Binary` rechecks the payload against this
   checkout's HEAD and version before any signing. `-Sign` keeps the existing
   Azure Trusted Signing flow: the owner's signed-in session signs the exe and
   newly packed sparse MSIX locally. Signing changes the exe hash; this working
   copy can no longer pass unsigned BUILDINFO verification. For a retry, make
   another copy of the pristine download. PDB and BUILDINFO stay out of the zip.

4. Continue [RELEASING.md](RELEASING.md)'s Mac asset collection, smoke checks,
   draft asset verification, publishing and distribution-manifest update steps.
   The existing SBOM generator runs in the build job, and `-Binary` copies its
   verified output into the release package directory. Packaging still computes
   `SHA256SUMS.txt` over the final signed archives and SBOM. The existing
   `release.yml` unsigned rehearsal and its licensing, archive and smoke checks
   remain intact. Windows signature/timestamp/publisher checks remain local in
   `sign.ps1` and `smoke.ps1 -ExpectSigned`; there is no Windows GitHub attestation
   action to move. Unsigned CI hashes are never compared to signed exe hashes.

An exceptional local build is available with `package.ps1 -BuildLocal -Sign`.
It prints the memory warning and builds with the same locked release command.
Do not run it while the owner is active. Bare `package.ps1` / legacy `-Binaries`
remain packaging-only for existing rehearsal callers; they do not build.

## Candidates (`nextNN`)

Dispatch the candidate commit using the same workflow, fetch its full SHA with
the same script, and run the same `-Binary` packaging path in a checkout at that
commit. `nextNN` is a distribution label, not a new compiled version: retain the
original `Folio <manifest version> (<commit>)` banner. If distributing the
existing loose `folio-nextNN.exe` candidate, copy the verified `folio.exe` under
that name and use `sign.ps1 -Files <candidate> -OutDir <new-signed-directory>` as
before. Keep the verified sidecars beside the candidate. Archive candidates use
the normal package contents and can be copied to a `nextNN` distribution name
after packaging; retain the original checksums and compute a checksum for any
renamed distribution asset. No local compile is part of either candidate path.

## Coordinator proof (not run by the implementer)

Run `pwsh -NoProfile -File scripts/release/ci-build-tests.ps1`. Dispatch this
branch for its full SHA, then fetch in print-only and `-Apply` modes. Confirm the
workflow used the pinned rustc and unchanged release profile, the five file
hashes verify, and `folio.exe --version` equals `Folio <version> (<git rev-parse
--short=10 HEAD>)`. Check SHA, tag and branch lookups resolve to the intended
commit. Change one byte in a disposable downloaded PDB and ensure `-Binary`
refuses before signing; also exercise a different checkout SHA. Then package a
fresh copy with `-Sign`, run signed smoke, inspect the nine-file zip, its SBOM
and final checksums, and retain run URL/BUILDINFO/PDB. No existing `main` CI run
has this artifact contract until this new job has run; do not substitute the
debug or unsigned-rehearsal archive. Release-script tests use offline fixtures
and child-process exit codes, matching the existing script test harness style.

## macOS decision

macOS is unchanged and continues to build on the owner's Mac mini using its own
scripts. The same split could use an Apple silicon macOS runner and download a
bundle plus dSYM (generate dSYM while object files still exist). Signing and
notarisation still need the owner's Developer ID and notary keys. Adopting that
split, or provisioning keys on runners, requires a separate decision; this
Windows job does neither and touches no Mac environment or service.
