# Architecture guards

This is the operator's map of Folio's architectural tripwires. A red guard is
evidence that a ruled boundary moved; do not bless the new output until the
owner's rule has been re-read and the change is intentional.

## File-read doors

The rule: every product file/content read in `bt-app` has a named lane and an
owner. `file_reads_source_tests` walks the product files declared by
`bt_source::Index`, compares the doors with
`crates/bt-app/src/file_reads_doors.txt`, and requires every item-keyed row to
resolve exactly once (uniqueness is the sibling test `file_reads_every_item_keyed_door_answers_to_one_item`). This descends from the 2026-09-20 file-read ruling.

Where and what to do: the guard is
`file_reads_source_tests::file_reads_every_product_content_door_has_a_lane`.
When it is red, classify the read, put it through the correct lane, and re-key a
moved owner by item identity; do not add a file key unless the collision is an
explicit row of `docs/plans/MIGRATION-DEBT.tsv`; that coupling is prose, kept equal by review, not by a test.

## Platform cfg boundaries

The rule: only the named platform modules and crate-root platform exports decide
which implementation compiles. Product modules do not grow opportunistic
`cfg(windows)` or `cfg(target_os = "macos")` forks. This is the platform split
ruled on 2026-09-12 and extended by the macOS backend tickets.

Where and what to do: the principal gate is
`platform_gate_tests::only_the_named_files_decide_what_platform_this_is`, with
the per-door arm tests in `bt-platform`. When it is red, move the platform fact
behind the platform crate/module boundary or update the declared arm and its
cross-platform contract together; never scatter a new local cfg to silence it.

## Ownership census

The rule: the asking/telling fields of `Runtime`, `App`, `WindowState`, and
`TabState` have a committed inventory, every access site is accounted for, and
proved multi-writer facts are explicit annotations. Unknowns may shrink but not
grow. The owner's rulings are dated 2026-09-25. Since 2026-10-11 (O-7) the
census also holds the document and its view, `PreviewBuffer` and
`PreviewPane`, and since O-8 the two contents that hold the view's state,
`DocumentView` and `PictureView`; their multi-writer facts are findings to fix
by routing the write through the type's own door, not annotations (the
ownership contract's §2.2 invariant 1).

Where and what to do: `crates/bt-source/tests/census.rs` compares against
`docs/plans/design/ownership-census-{annotations,unknowns}.tsv` (the dated `ownership-census-2026-09-25.tsv` is a record, not an enforcement input), and
`scripts/ci/check-census-unknowns.ps1` enforces the shrink direction against the
merge base: the unknown list is keyed by identity (`site`, `module`,
`function`, `reason`) and measurements may only shrink — a key the merge base
does not carry, or a key whose `sites` count is larger, is refused by key; a key
that disappears or whose `sites` count drops is the list shrinking. Keys compare
case-sensitively, as `FieldCensus` keys them. It is a shrink-only tripwire over
aggregated counts; a same-key swap (one site resolved and another added under
the same site, module, function and reason) is invisible to it, by design. When red,
fix the routing or regenerate with `scripts/generate-ownership-census.ps1` and
justify each real multi-writer or unknown row; do not edit generated census rows
as a substitute for changing the code.

## Runtime dependency map

The rule: the measured dependency graph inside `bt-app` that the field moves
and the topic moves are ordered by (the ownership contract's §3.1, J3 on
2026-10-11) is a generated artefact of the census family, never hand-kept.
Four files under `docs/plans/design/runtime-dep-map/` — `fields.tsv` (reads and
writes by item, per field of the census's structs), `topics.tsv` (file-to-file
call edges), `doors.tsv` (an outside writer of at most two) and
`unclassified.tsv` (the free functions and `Runtime` methods still in
`main.rs`) — are `bt_source::DependencyMap`'s rendering of the census the
ownership census gate takes; the `README.md` beside them is the route map read
off them. No script reads Rust for them: `bt-source` is the reader.

Where and what to do: `crates/bt-source/tests/census.rs`
`the_committed_runtime_dep_map_is_the_code` compares each file with the
rendering at the tree, line for line but for its `# measured at` line, and
names a differing row on both sides; `crates/bt-source/tests/dep_map.rs` holds
the rules on a fixture. When red, regenerate with
`scripts/generate-runtime-dep-map.ps1` (it runs that test and copies
`target/depmap/`) and commit the files with the change that moved them; never
edit a row. A header count that grows (reads not attributed, calls not
followed) means the map sees less; the change that regenerates it says why.

## Window-wait registry and effects

The rule: work reachable from the window thread does not block, and every
approved bounded wait is registered with its execution effect and owner. The
registry and the generated architecture table descend from the thread-door and
window-wait rulings of 2026-09-26.

Where and what to do: `window_waits_tests::every_door_is_where_the_registry_says`,
`crates/bt-app/src/window_waits.tsv`, `scripts/ci/check-window-waits.ps1`, and the
generated window-waits section of `ARCHITECTURE.md` agree. When red, remove the
wait from the window path or route it through an admitted worker; update the
registry through `scripts/dev/generate-window-waits-table.ps1`, never by
hand-editing the generated architecture region.

The registry's `# completions` section (S-1, 2026-10-11) is the side-effects
contract's ledger, one row per request protocol, held by the
`every_*completions*` tests (among them `every_worker_door_body_is_reached_from_a_completions_door`) and `every_conforming_request_door_holds_no_wait_of_its_own`
in `window_waits_tests` and by the fifth and sixth checks of `scripts/ci/check-window-waits.ps1`. A door that
reaches a lane started elsewhere is listed, with its reason, in
`docs/plans/completions-door-starts.tsv`, which only shrinks; reachability is a call path to
the function that starts the thread, never a shared type or module.
When red, give the new answer, lane, thread or registry row its ledger row (or
correct the row the code moved past); a status names the D-row or registry row
that repairs it and only ever becomes `conforms` — never mark a row conforming
to make a check pass.

## Lane contract and the completion seam

The rule: every lane is held to the lane contract's claims, and a lane that
fails one says so as a dated row of `lane::EXPECTED_FAILURES` naming the
structural-debt row that repairs it — never by being declared conformant.
Every request a lane admits ends exactly once (the side-effects contract's
C2–C4 and C6, `docs/plans/design/side-effects-contract-2026-10-11.md`
revision (b)7; ruled 2026-10-11).

What a lane does to adopt the seam (S-3, in `crates/bt-app/src/lane.rs`): it
takes its worker from a `lane::Executor` (`start_worker`, never the thread
door itself); it admits each request through a `lane::Ledger` (`admit`, or
`admit_joined` for the same operation asked again), which mints the identity
or refuses at the door, and records the sink the identity is owed when its
requester goes; it runs each request's work inside `Worker::task`, so a panic
there is that request's `Failed` and the worker goes on; it ends each identity
through the hook `Ledger::publish` — `Done`, `Failed`, `Cancelled`, or
`Superseded { by }` for one a newer, different request displaced — while
`Joined` and `WorkerGone` come from the ledger itself; it says no failure line
of its own (the ledger says each once, or once per streak); and its
`LaneUnderTest` adapter hands the real lane `Executor::scripted` and reports
`sunk` and `said`. What the proof covers:
`lane_contract_tests::the_ledger_ends_every_identity_exactly_once_in_every_interleaving`
plays every order of three identities' outcomes (the six terminals, a racing
answer after a cancel), their requesters' drops and the worker's end —
3,302,423 interleavings — on the real `Ledger`, and requires one delivery per
identity, at its requester or its sink, with the terminal the contract names,
each failure said once and the fault exactly when the worker ended. It does not
cover a lane's own admission policy or its queue; the lane's claims do, through
its adapter.

Where and what to do:
`lane_contract_tests::every_lane_runs_every_claim_and_fails_only_where_the_ledger_says`
holds each lane to `EXPECTED_FAILURES` both ways (an undeclared failure and an
unexpected pass are red), and `scripts/ci/check-expected-failures.ps1` holds the
list — kept as data in `crates/bt-app/src/lane_expected_failures.tsv`, equal to
the code by `lane_contract_tests::the_expected_failures_are_this_list` — to the
merge base: no lane gains a row for a claim the merge base already
had; a claim new to the suite brings the rows of the lanes that do not keep it
yet. When red, repair the lane (a repair removes its row in the same change),
or restore the behaviour the claim caught; do not add a row to quiet it.

## Timing-bound tests

The rule: a test that depends on wall-clock progress is exceptional, named, and
bounded; ordinary tests synchronize on state. The exact inventory was ruled by
T-GATES on 2026-09-30.

Where and what to do: `docs/plans/TIMING-BOUND-TESTS.tsv`,
`crates/bt-source/tests/timing.rs`, `scripts/ci/check-timing-bound.ps1`, and
`scripts/ci/check-timing-bound-tests.ps1` are the gate. When red, replace the
sleep/deadline with a deterministic signal; if time itself is the subject, add
the narrow test and its reason to the inventory in the same change.

## Raw-effect and thread-door exceptions

The rule: `bt-app` and `bt-platform` create workers through the admitted thread
door; bare spawn/wait effects are a small reviewed exception set. The ruling is
the 2026-09-26 thread-door design, and the live exception inventory is
`docs/plans/window-thread-bare-sites.tsv`.

Where and what to do: the raw-effect half of
`window_waits_tests::every_door_is_where_the_registry_says` and
`scripts/ci/check-window-waits.ps1` enforce it. When red, use
`spawn_at_priority` with a named worker/effect; retain a bare site only when the
door cannot own it and record that reason in the exception ledger.

## Machine-independent paths

The rule: committed code, tests, and documentation do not carry one developer's
absolute path or account name. Portable examples use neutral sample roots and
test scratch comes from the repository's test-path owner. The repository rule
dates to 2026-08-27.

Where and what to do: `scripts/check-machine-paths.ps1` is the gate. When red,
replace the machine path with a workspace-relative path, a declared fixture, or
`bt_testpath`; use neutral names such as `alice` for an account example rather
than adding an exception for the current workstation.

## Third-party notices

The rule: the dependency graph and the shipped `THIRD-PARTY-NOTICES.md` agree,
and notice generation is deterministic. Licensing inputs are product inputs,
not prose to patch around a diff.

Where and what to do: `scripts/check-notices.ps1` runs
`scripts/generate-notices.ps1` and compares the result. When red, correct the
dependency/license metadata or regenerate the notice through the script; do not
hand-edit generated notice entries.

## Documentation vocabulary

The rule: reader-facing names and prohibited stale terms are consistent across
the documentation set. The guard implements the copy rulings recorded in
`docs/RULES.md`, including their dated replacements.

Where and what to do: `scripts/check-doc-words.ps1` is the gate. When red, use
the ruled current term everywhere the script names; if the vocabulary itself is
changing, update the ruling and the checker in the same commit rather than
adding a one-off spelling exemption.

## Vendored notices

The rule: every vendored component retains the upstream copyright and license
material required for redistribution, including the exact upstream version the
workspace vendors. Vendored code is not made ours by being copied into the
tree.

Where and what to do: `scripts/check-vendor-notices.ps1` is the gate. When red,
restore the upstream notice/license from the vendored version or update the
versioned evidence together with the vendor update; do not rewrite upstream
attribution to fit Folio's generated notice format.
