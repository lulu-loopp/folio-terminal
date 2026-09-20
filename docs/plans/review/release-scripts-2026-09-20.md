# Release scripts: the defects every release trips over, at their owners

Branch `fix/release-steps-run-once`, from `main` at `0b8eb7c8`. Brief: the
0.4.3 release is cut by running `docs/RELEASING.md`'s steps once each, with no
manual repair in between, and the two distribution manifests are updated by one
command.

## What was found, per defect

| | state | evidence |
| --- | --- | --- |
| D1 `target/release-package` not cleared | **not reproducible as written; a different defect fixed** | `package.ps1` has emptied it since `2a12d1cf` (2026-09-15), three days before 0.4.2. What it did **not** do is decide *what* it may empty: it removed every entry of whatever `-Output` named. A1 now recognises the nine name shapes this lane writes and refuses anything else by name. |
| D2 `smoke.ps1 -ExpectSigned` defaulted `-Msix` beside `-Exe` | **fixed** | The default is now the loose `folio.msix` beside `-Exe`, else the release archive in `-PackageDirectory`; with neither, it refuses at the door naming both. Three new cases in `smoke-tests.ps1`. |
| D3 `diagnostics.log` opened with a stall line and no header | **fixed, structurally** | The header was written from inside `choose_resident_channel`'s `if channel == Channel::Log` arm, while `note()` appends through a handle of its own from the moment `RESIDENT_LOG` is set. A console run and a `Nowhere` run therefore wrote notes into a file they had never headed. It is now written by `open_run_log`, which opens the file, before `RESIDENT_LOG` exists. |
| D4 `awk: syntax error` at the icons step | **not reproducible in this repository** | `bundle.sh:243` is `sips -g pixelWidth "$src" \| awk '/pixelWidth:/ { print $2 }'`. Run here against `  pixelWidth: 1024` it prints `1024` and exits 0. The fault is in `~/folio-port/launchers/owner_bundle.sh` on the Mac, which is not in this tree — **left for the owner**. |
| D5a `sign.sh` exiting 1 before notarization | **already fixed on main** | `sign.sh` runs `spctl` with `set +e`, prints the verdict, and its last statement is `exit 0` (lines 269–287). Its header says why. |
| D5b `SHA256SUMS-macos.txt` carrying a path prefix | **already fixed on main** | `checksums.sh` does `cd "$dir"` (line 88) and then `shasum -a 256 "$file"` on bare names (line 102), and reads the file back with `shasum -c`. |
| D6 cask and scoop bucket hand-edited | **fixed** | New `scripts/release/update-manifests.ps1`. Print-only by default. |

## The obligation D3 belongs to

*The first line of a run's block in `diagnostics.log` is that run's header.* It
failed because the fact had two owners — the log file (`note`/`append_note`) and
the stream (`eprintln!` after the redirect) — and only one of them wrote the
header. The repair gives the file's first line one owner, the step that opens
the file, and it runs before anything can write through `RESIDENT_LOG`.
Rotation moved with it, which closes a second hole in the same place: a run that
kept its console never checked the 4 MiB cap and still appended to the log.

Test: `bt_app::diagnostics::tests::a_run_heads_its_log_before_anything_else_can_write_into_it`.
**Red on main** at the assertion `!chooser.contains("run_header")` — on `main`
`run_header` is called from inside the channel arm. The behavioural half pins
the ordering: header first, watchdog line second, one header per run.

## Run

- `cargo test -p bt-app --bin folio diagnostics -j 4` — 13 passed, 0 failed.
- `scripts/release/smoke-tests.ps1` — 11 cases, all green (8 before this branch).
- `package.ps1` against a copy of 0.4.2's `target/release-package`: with a
  stranger file it named it and deleted nothing; without one it cleared the
  archive, the stable copy, `SHA256SUMS.txt`, a previous release's
  `Folio-0.4.1-macos-arm64.dmg` and `SHA256SUMS-macos.txt`, kept
  `folio-0.4.2.cdx.json`, and then stopped on the missing build.
- `update-manifests.ps1 -Version 0.4.2 -FromRelease` reproduced both published
  0.4.2 manifests byte for byte ("unchanged"), and a rehearsal at 0.4.3 with
  synthetic checksum files changed exactly six values and nothing else.

## Not verified

Nothing was built, signed or published, and `-Apply` was never run — so the
write half of `update-manifests.ps1` (the push-permission check and the two
`gh api PUT`s) is unexercised. The macOS scripts were read, not run: no Mac.
`package.ps1`'s clearing was exercised without a build, so the run stops at the
missing `folio.exe` rather than producing an archive.
