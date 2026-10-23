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

   A tag push builds with `FOLIO_UPDATER=on`; a dispatch does only with
   `-f updater=true`, which a release dispatched this way must pass
   (RELEASING.md, "The workflow"). `smoke.ps1 -ExpectSigned` in step 3 refuses
   a release build that says `updater off`.

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

   **The CI-built `folio.exe` also decides what the other eight archive
   members must be** (0.4.6 ticket U-9). Its `build.rs` embedded the release
   manifest `FOLIO_RELEASE_MANIFEST` on the runner: the name, SHA-256 and size
   of the two ConPTY sidecars it extracted and of the six text files from the
   commit it built (`scripts/release/archive-members.txt` is the list). Before
   packing, `package.ps1` reads that resource out of the executable — never by
   running it — and refuses, naming each member, when a file in this checkout or
   in `target/ci-signing` is missing, unlisted, or differs. That is the
   "release documents and packaging inputs unchanged" of step 2, now checked
   rather than asked; `.gitattributes` holds those six files to LF so the runner
   and this machine hash the same bytes. `smoke.ps1` reads the manifest again
   from the `folio.exe` inside the packed archive and checks every entry against
   it; signing changes only `folio.exe` and `folio.msix`, which the manifest does
   not list.

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
after packaging (a candidate is dispatched without `updater`, so a signed one is
smoked with `-ExpectSigned -Updater off`); retain the original checksums and compute a checksum for any
renamed distribution asset. No local compile is part of either candidate path.

## Coordinator proof (not run by the implementer)

Run `pwsh -NoProfile -File scripts/release/ci-build-tests.ps1`. Dispatch this
branch for its full SHA, then fetch in print-only and `-Apply` modes. Confirm the
workflow used the pinned rustc and unchanged release profile, the five file
hashes verify, and `folio.exe --version` equals `Folio <version> (<git rev-parse
--short=10 HEAD>)`. Check SHA, tag and branch lookups resolve to the intended
commit. Change one byte in a disposable downloaded PDB and ensure `-Binary`
refuses before signing; also exercise a different checkout SHA. Then package a
fresh copy with `-Sign`, run signed smoke, inspect the ten-file zip, its SBOM
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

## What CI runs on macOS

CI's `core-macos` job (`.github/workflows/ci.yml`) is a check and lint lane, not
a release build. Since ticket 72 (2026-09-26) it:

- checks every product crate with `--all-targets`, `bt-app` included;
- runs the same clippy line as the Windows `logic` job, over the whole
  workspace with `-D warnings`, and plants a macOS-only dead item to show that
  line refuses it;
- tests the portable crates plus `bt-render` (on the runner's Metal device)
  and `bt-corpus`.

`bt-term` and `bt-app` are checked but not tested there. Their suites contain
tests that assert Windows facts on every host, and those tests are red on a
Mac by design: 43 in `bt-term` and 282 in `bt-app` on the owner's Mac mini
(structural-debt D-83). A macOS run of either suite should be read against
those counts, not against zero.

## The window thread's checks (A2a, 2026-09-26)

`scripts/ci/check-window-waits.ps1` runs in `logic` and in `core-macos`. Until
ticket A2e switches the lint on, `docs/plans/window-thread-bare-sites.tsv` lists
every vocabulary site the product has outside a registered door, and the script
holds it to shrinking: each row's count is no larger than at the merge base with
`origin/main`, or than in the pinned seed commit `2cc59a83` when the merge base
predates the file. A missing merge base is exit 2, not a pass. `core-macos`
therefore checks out with full history (`fetch-depth: 0`), as `logic` and
`gates-can-fail` already did; `gates-can-fail` plants a row and requires red.

The same script is the configuration fence: it pins the five `clippy.toml`
files (root, `vendor/`, `crates/bt-corpus/`, `crates/bt-source/`, and the lint
probe's generated one), refuses any other `clippy.toml` or `.clippy.toml`, any
`.cargo/config`, `CLIPPY_CONF_DIR` anywhere a build could read it or set in the
job, and any `-A`/`-W`/`--cap-lints` touching the lint in a workflow, a
`.cargo/config.toml` or a script's `RUSTFLAGS`. A second step on both jobs,
**the lint probe's positive control** (`scripts/ci/check-lint-probe.ps1`),
regenerates `crates/bt-lint-probe` from the registry, requires it unchanged and
tracked, then runs `cargo clippy -p bt-lint-probe --all-targets --
--force-warn clippy::disallowed_methods` and requires one report per vocabulary
entry assigned to the runner's platform. `bt-lint-probe` is on `logic`'s fmt
line.

## Job budgets (T-CI-LOGIC-JOB-BUDGET, T-CI-JOB-BUDGETS-REDERIVE, 2026-10-17)

A job's `timeout-minutes` is derived, and the derivation sits in a comment
above the line with the runs it came from: the largest total seen with a cold
cache (nothing restored, everything compiled: the toolchain step's log says
`No cache found.`), times 1.36, rounded up. A job that restores no cache
(`release-script-tests`, `cask-audit`) takes the largest total of every run in
the window instead. Network-bound jobs get a 10-minute floor: a stall, not a
hang, is what trips them, and a budget a stall trips first is a false cancel.
`cask-audit` is one (the online strict audit downloads the DMG);
`release-script-tests` runs offline steps (no network, a `gh` stand-in) but is
short enough that a stall in its checkout or the runner's start would trip a
derived 4 first, so it takes the same floor. When a cold total nears the
budget divided by 1.36, the number is derived again from new runs, not raised
by hand.

Measured on 2026-10-17 over every completed run of 2026-10-15 to 10-17
(38658095635 to 39032766393, 58 runs, and the first attempt of each of the
three reruns): a
job's cold runs are all of its green runs in that window whose cache did not
restore; "restored" is the ten green runs on `main` (39032766393 back to
38691455267) where the job's cache did restore, in full or by its prefix.
Minutes.

| job | runner | restored p50 / max | cold p50 / max (run) | budget | was |
|---|---|---|---|---|---|
| `logic` | Windows | 43.9 / 47.45 | 61.9 / 77.17 (39014647034) | 105 | 104 |
| `logic`'s `test` step | Windows | - | 47.8 (38842728496) | 65 | 65 |
| `core-macos` | macOS | 37.1 / 41.27 | 41.1 / 50.63 (39020858358) | 69 | 60 |
| `gpu` | Windows | 5.0 / 5.57 | 19.4 / 20.95 (38708586878) | 29 | 40 |
| `conpty` | Windows | 6.4 / 7.08 | 10.7 / 11.80 (38714746975) | 17 | 30 |
| `smoke` | Windows | 4.2 / 5.47 | 10.1 / 11.05 (39014776542) | 16 | 60 |
| `core-linux` | Linux | 7.8 / 8.53 | 10.4 / 10.82 (39010930188) | 15 | 45 |
| `gates-can-fail` | Windows | 6.4 / 7.4 | 8.1 / 10.15 (38693069271) | 14 | 20 |
| `licensing` | Windows | 2.9 / 3.35 | 6.3 / 7.92 (39014776542) | 11 | 16 |
| `wasm-lib-check` | Windows | 1.3 / 2.5 | 3.8 / 4.77 (38720519586) | 7 | 30 |
| `release-script-tests` | Windows | no cache: 1.8 / 2.53 (38687157155) | - | 10 (floor; the rule gives 4) | 10 |
| `cask-audit` | macOS | no cache: 1.1 / 1.80 (38665182697) | - | 10 (floor; the rule gives 3) | 10 |

The 1.36 was, when `logic`'s budget was first derived, the slowest cold `test`
step over the fastest (47.8 / 35.1 min; cause unknown: the fast ones were
mostly branch dispatches, the slow ones main pushes), and every job uses that
one factor. Over this window the same ratio, each job's longest step, cold, is
larger on every runner: `logic`'s `test` 47.8 / 29.8 = 1.60; `core-macos`'s
`test the application` 27.4 / 8.5 = 3.22 (two speeds of runner on the same
image: run 38842728496 ran the application's tests in 128 s, run 38830428679
the same tests in 250 s); `gpu` 1.62, `conpty` 1.71, `smoke` 1.73,
`core-linux` 1.68, `gates-can-fail` 1.99, `licensing` 1.68, `wasm-lib-check`
1.39. Those ratios measure the distance from the fastest runner to the slowest,
which the largest total already sits at; what the factor is for is how much
slower than the slowest seen the next runner may be, and no run here measures
that. The factor stays 1.36 until a run says otherwise.

The `test` step carries a budget of its own below the job's, so a run past it
fails on that step with a line naming the budget instead of being cancelled
without a reason wherever the job's clock stopped. The licensing gates
(`check-notices.ps1`, `check-machine-paths.ps1`, `check-doc-words.ps1`,
`check-vendor-notices.ps1`, with `cargo fetch` and the pinned `cargo-about`
before them) are the `licensing` job rather than the tail of `logic`: none of
them reads a build, and as the tail they were what a slow suite cancelled.

The cold regime is the one that sets the budget because a cache-key change
puts every run in it: rust-cache hashes every installed toolchain into the
key's prefix, and as the runner image's own `stable` 1.99.0 (in place of
1.98.1) rolled out across runners from 2026-10-15 (38671015245 already on it at
02:26; 38691455267 and 38693259445 still warm on 1.98.1 at 06:16 and 06:34), no
key matched on the new runners, the cold `logic` ran past its old 75 minutes,
and a cancelled job saves no cache, so `main`'s runs there stayed cold. The toolchain action
now removes every toolchain but the pinned one before the cache step, so the key
follows `rust-toolchain.toml` and not the image.

An evicted cache puts runs in it too: the store was full, and branch runs
pushed `main`'s entries out (`core-macos` in 39017771237 restored `main`'s cache
at 08:57, and in 39019651600, seven minutes later, found none; the sizes, the
rule that stops it and what it still leaves are in "Cache store" below). A run
that starts before the previous push's save has landed is cold as well
(`logic` in 39017771237 looked at 08:35; 39014647034's `logic` saved at 09:00).

## Cache store (T-CI-CACHE-STORE-BUDGET, 2026-10-17)

GitHub keeps 10 GB of caches per repository; past that it evicts the entries
read least recently, and an entry nobody reads for seven days goes anyway. A
run restores what its own ref saved or what the default branch saved (a pull
request's run also what its base branch saved), never a sibling branch's: an entry saved by a branch dispatch, a pull request or a tag
is readable by that ref alone. rust-cache keys an entry
`v0-rust-<job>-<os>-<arch>-<environment hash>-<dependency hash>` (the
dependency hash covers the manifests' dependency sections, `Cargo.lock`,
`rust-toolchain(.toml)` and `.cargo/config.toml`) and restores
by the key without the dependency hash when the full key is missing, taking the
newest entry with that prefix; before saving it removes the workspace's own
crates from `target/`, so an entry is the compiled dependencies and the
registry files they came from (`licensing`, which compiles nothing into
`target/`, saves 0.14 GB, the most the registry part of any entry can be).

The store on 2026-10-17 (`gh cache list`), 9.97 GB in 11 entries:

| job | ref | size (GB) | state |
|---|---|---|---|
| `logic` | `main` | 1.82 | current |
| `core-macos` | `main` | 1.02 | current |
| `smoke` | `main` | 0.92 | current |
| `gpu` | `main` | 0.76 | current |
| `core-linux` | `main` | 0.50 | current |
| `gates-can-fail` | `main` | 0.41 | current |
| `conpty` | `main` | 0.38 | current |
| `wasm-lib-check` | `main` | 0.38 | current |
| `licensing` | `main` | 0.14 | current |
| `logic` | `main` | 1.82 | superseded (the previous `Cargo.lock`) |
| `logic` | a branch | 1.82 | readable by that branch only |

Over the 58 `ci.yml` runs of 2026-10-15..17 branch runs saved 157 entries
(113.7 GB) and `main` 37 (25.0 GB); the branch entries were read back by their
own branch 18 times. After the toolchain pin, 12 of `main`'s 90 job runs found
no entry although `main` had saved that job's entry under the same prefix
minutes to an hour earlier: branch bursts had evicted it.

The rule: **only `main` saves** (`save-if` in `.github/actions/toolchain`);
every other run restores `main`'s entry and saves nothing. The store then
holds `main`'s set, S = 6.34 GB, and superseded entries, which nothing reads
and which are therefore evicted first. The bound that keeps `main` warm is
S + the largest entry <= 10 GB (today 6.34 + 1.82 = 8.16): one job whose key
moved re-saves without evicting a sibling. A dependency change moves every key
at once and needs 2 S = 12.67 GB for a moment; up to 2 S - 10 = 2.67 GB of the
outgoing set can be evicted before the incoming set is saved, so a second
`main` run started before that landing's last job has saved (`logic`, up to
about 77 minutes in) can find some jobs cold. When S + the largest entry passes 10 GB, the job sizes are measured
again and the set is made smaller before anything else is cached. A branch
whose dependency hash matches `main`'s restores the full key; one that changed it
restores `main`'s entry by prefix and compiles its difference on every
dispatch (before this rule only its first dispatch did).

Not taken: `cache-targets: false` (an entry would be the registry alone, about
0.14 GB, and every job would compile its dependencies cold: `logic` 61.9 min
p50 against 43.9 restored); one `shared-key` for several jobs (rust-cache
saves only when the full key was missing, so the first job to finish would
write the shared entry with its own artefacts and the others would never add
theirs); a workflow deleting branch entries (with branches no longer saving
there is nothing to delete after the last such entry ages out).
