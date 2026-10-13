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
explicit row of `docs/plans/MIGRATION-DEBT.tsv`. One direction of that coupling is a test: a debt row keyed `ledger key naming a file` must be a line of the ledger (`every_row_is_a_reader_or_a_categorised_exception`, below); the other — every file key has its debt row — is prose, kept by review.

## Source readers

The rule: a test that reads source text or a committed document is on a list
that says why. A reader bound to a Rust file is on `bt_source::FileScoped` (the
permanent allowlist), on `docs/plans/MIGRATION-DEBT.tsv` (readers waiting on a
seam; it only shrinks, and `scripts/ci/check-migration-debt.ps1` refuses an
added row against the merge base) or on `docs/plans/SOURCE-READERS.tsv`
(readers whose subject really is the bytes). Since
T-SOURCE-READERS-TSV-TRIPWIRE (2026-10-12) every item of test or build-support
code that reads a **committed non-Rust file** has a row on
`SOURCE-READERS.tsv` or `MIGRATION-DEBT.tsv`, keyed by its file and its item
(the `owner` column), and never on both: an `include_str!`/`include_bytes!` of
a committed path, or a **call** of `read_to_string`, `fs::read` or `File::open`
(through bt-source's call pattern; a bare mention is no call) whose own
argument reduces to a committed document, or **any other call or method handed
such a path** — a wrapper of the same crate (`read_session(&fixture_path(…))`),
a function of another crate, a method called on the path — which is a reader
named by its callee in the `mechanism` column. Each call is judged by its own
argument: a literal, a `Path`/`.join` chain, `env!`, `concat!` and `format!`
spliced piece by piece (placeholders take their arguments as `std` does; a
string that spells a path into other text names that path), the innermost
binding in scope at the call (a `let` from the end of its statement, so `let
path = read(path)` reads the earlier one; an `if let`, `match` arm or `for`
variable over its body; a loop over a literal list is each element in turn),
a constant through bt-source's module trees across every first-party package
(bare, `self::`, `super::`, `crate::`, a crate's name, `use` and `use …::*`,
every file a module is written in, `impl` blocks' constants), and a same-file
helper with its parameters bound. The tree compiles, so every path the
reading code names is one Rust let it name (a `pub use` of a `pub(crate)` item
is E0364; a glob re-exports only what it may; a type or trait the code names is
visible to it): visibility only tells same-named candidates apart — where two
globs bring in two constants of one name, the one visible to the module the
path is written in (carried through every re-export; visible when every `use`
on its way admits that module and so does the constant) is the constant, several
visible or none is the diagnostic, and a lone candidate is the constant even
when invisible to the reader, as the code could not have compiled otherwise.
An associated constant written `<Type as Trait>::NAME` is the impl of the trait
that path names, or that trait's default where the impl does not declare it;
`Type::NAME` is the inherent impl's, or the one trait impl's, and several trait
impls make it the diagnostic. A trait path is followed through the module
trees, `use`s (an alias to what it names) and globs to a first-party trait;
another crate's trait is its full written path after first-party `use`s are
followed (`use dep::Trait as A` is `dep::Trait`, `a::Same` is not `b::Same`),
and one that cannot be followed is the diagnostic. A constant name is SCREAMING_SNAKE_CASE:
CI's workspace-wide `clippy -D warnings` makes `non_upper_case_globals` (not in
the workspace's `Cargo.toml`: rustc's default warn) refuse any other, and a
snake_case constant that is there is found by its name all the same. A macro is
not a call (`assert_eq!(path, …)` compares), nor are the calls that only build
or describe a path (`Path::new`, `PathBuf::from`, `.join`, `.display()` and the
rest of the test's `PATH_BUILDING`). A path under `target/` or a temporary
directory, or an absolute path, is not a document, and does not make another
call in the same function a reader.
The subject set is what `git ls-files` names minus `.rs` files; generated files
under `target/` and bytes the shipped program embeds are not subjects.

Where and what to do: `crates/bt-source/tests/tripwire.rs`.
`no_source_reader_names_a_file_outside_the_two_lists` holds the Rust-source
readers (four lexical shapes, by file). In a PowerShell reader (a `.ps1`
holding `Get-Content`, `Select-String` or `ReadAll{Text,Lines,Bytes}`) a `.rs`
path literal is a hit in exactly two spellings, `"…lib.rs"` and `'…lib.rs'`,
each path-shaped, its opener the nearest quote of its kind on the same line.
Three other spellings are diagnostics that fail naming `file:line`, the shape
and what to do: a `.rs` extension inside a here-string (`@"…"@`, `@'…'@`) that
is not one of the two spellings; an interpolated double-quoted value
(`"$dir/lib.rs"`, `"${x}.rs"`, `"$(…)/a.rs"`, a `$` after an even run of
backticks and before a name or an expression); and any other quoted `.rs` value with its opener on its line that
is not path-shaped (`'crates/a b/lib.rs'`, an escaped `` `$ ``, a
sub-expression holding a quote of its own). The author rewrites the literal
into one of the two spellings or lists the file. Out of scope, as limits: a
`.rs` with no quote on its line outside a here-string (a comment,
`-Filter *.rs`), a closer with no opener on its line (a string spanning lines),
and `.RS` in upper case; a line that only looks like a here-string opener
(`$m = 'a@'`) opens a phantom one whose `.rs` names are reported — noise, never
a silent pass. A shell reader is cleared like a Rust one, by file: a gate whose
subject really is source files is a `FileScoped` variant (the vendor-notice,
dead-code, lint-probe and embedded-spaces gates). A suite that writes a scratch
`.rs` file into a temporary tree is no reader of a committed source and gets no
row: it spells the scratch name so that the script holds no `.rs` literal (as
`[IO.Path]::ChangeExtension('crates/x/src/lib', 'rs')` does, or by giving the
scratch file another extension), and a single-quoted scratch path the scan
reports is a false positive the author rewrites;
`every_reader_of_a_committed_document_is_on_a_list` holds the document readers
(by item, through `bt-source`'s index — no script reads Rust for it), and its
message names the item, the file and both lists;
`every_source_reader_names_a_reason_and_is_not_also_debt` refuses a reader on
both. When red, a reader whose subject really is the document gets a
`SOURCE-READERS.tsv` row (file, owner as the message spells it, mechanism,
subject, ticket, `no`, reason); a reader that pins a fact about the code by
reading text is migrated to a `bt-source` query instead, and a debt row is never
added. The scan never drops a path it gives up on in silence: a rooted path
handed to any call that it cannot reduce to a literal (a parameter, a constant
it cannot read), a `concat!`/`format!` holding a rooted piece and an
unreducible piece anywhere (both outcomes: each fully reduced rooted piece that
names a committed document is still a reader), a constant that reaches itself again (cycles are
detected; acyclic chains are followed to their end), and a constant-spelled
name the module trees hold no constant, `use`, glob or `thread_local!` of are
counted and listed by the test as a diagnostic — grouped by cause with the
exact total and a few examples each — not refused. Every place the evaluator answers "nothing" says beside it
why that is a resolution, a diagnostic or no path into the checkout. The method
exclusion is a list of names, so a reading method that shares an excluded name
is a blind spot, and a new
path-describing method is a false positive until it is added to
`PATH_BUILDING`. Not covered, and said in the test's header: a path handed in
as a parameter (the function that hands it on is not the reader; its caller
that names the file is), a root reached through a method, a closure or another
file's function, and a read call renamed by `use … as …` (bt-source does not
resolve aliases).

Since T-SOURCE-READERS-STALE (2026-10-12) the two lists are a **two-way
equality**: every reader is a row, and every row is a reader or a categorised
exception. `every_row_is_a_reader_or_a_categorised_exception` resolves each
row's `(file, owner)` to an item bt-source finds in that file, spelled as the
reader refusal spells it (an owner that is gone, or is an item of another file,
is refused; a MIGRATION-DEBT `ledger key naming a file` resolves to a line of
its ledger; a non-Rust file, such as a Python script, only as an `exception`),
and holds it by its `category` column, which every row of both lists carries:
`read` (the documents the scan finds the item reading are exactly those the
subject names — one it reads and the row does not name is refused too — and a Rust-source shape on a line of the item names every `.rs` name in
it), `unreduced` (the diagnostic names the item: the read exists and the
evaluator cannot reduce its path, so the subject is not checked; the row stays
while the diagnostic entry does, and when the entry goes it is `read` or
stale), `rust-source` (every name in the subject is a `.rs` name a Rust-source
shape on a line of the item reports), or `exception` (a reader no scan sees;
its `reason`, or `needed_seam`, names the blind spot). No other value, none
missing. The subject is spelled as the mechanism resolves it: an include's
literal from the row's file's directory, any other path from the workspace
root. When red, a row whose item is gone or reads nothing is removed, a row
whose item reads something else is corrected to what it reads, and a row whose
reader the evaluator cannot see becomes an `exception` naming the blind spot —
the evaluator is not widened for it. A `.rs` row whose file bt-source does not
index (`bt-source`'s `tests/fixtures/`, `vendor/`) is refused as an item that is
gone, whatever its category: such a file is no reader's, and that is intended.
A test that reads only a file it writes
itself is no reader of a committed document and has no row.
`a_row_no_reader_accounts_for_is_refused` plants each refusal.
`scripts/ci/check-migration-debt.ps1` finds its six identity columns by name, so
the `category` column is not a new debt row
(`scripts/ci/check-migration-debt-tests.ps1`).

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
the function that starts the thread, never a shared type or module. The path follows a call on
a receiver stated to be a first-party trait (`dyn Trait` behind `&`, `Box`, `Arc` or `Rc`,
`impl Trait`, a generic parameter bound by the trait, `self` in the trait's default method, or
a path call `Trait::m` / `D::m` with `D: Trait`) into that trait's implementations, so a body
reached only through a trait object (the update job's `driver.prepare`) needs no list line. A
trait is its identity — package, module path and name, resolved through `use` items, globs and
re-exports by `bt_source::TraitResolver` — so two traits of one name (the macOS and the
Windows `PreparePoint`) are never followed together. What is followed: the method of every
product `impl` of the trait, on every platform arm; the trait's default body, only for an
impl that does not override it (or when there is no impl); and the trait's first-party
supertraits, whose impls count only for types implementing the stated trait. The answer is the
trait's implementations alone — never united with the by-name answer — and a call on a
parameter is not typed by the parameter's declaration when another binding of its name is in
scope at the call: one begun before the call whose scope holds it (a `let` to the end of its
block; an `if let`/`while let` binding, each `let` of a chain included, from the end of that
`let` through the later operands and the body; a match-arm pattern, and each `let` of its guard
from the end of that `let`, through the arm; a `for` or closure binding over its body), the
scopes read by parsing the function body with `syn`, and a product body that does not parse
fails the test by name; a binding in a closed block or after the call does not shadow. What is
not followed: a
stored closure, a function pointer, `<X as Trait>::m`, a type written through an alias, a
standard or third-party trait object (and a foreign supertrait's methods, even on a
first-party implementing type), and an untyped receiver whose method name is shared; those are
S-2a's call graph.
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
takes its worker from a `lane::Executor` (`start_worker("<thread name>", …)`,
never the thread door itself — the completions reachability guard's
`worker_roots` reads that call as the thread's start, so the lane's thread name
and the worker-door bodies its body reaches are seen as a thread door's are);
it builds its `lane::Ledger` with its own wake, which the ledger calls when the
worker ends, after every owed identity has its terminal; it admits each request
through the ledger (`admit`, or `admit_joined` for the same operation asked
again), which mints the identity or refuses at the door, and records the sink
the identity is owed when its requester goes; it runs each request's work
inside `lane::task`, so a panic there is that request's `Failed` and the
worker goes on (the task and the worker's body run inside
`contained_panic::contain`, so the resident panic hook lets the panic reach the
seam; S-5 round 3); it ends each request it runs through the hook
`Ledger::publish` — `Done`, `Failed`, `Cancelled`, or `Superseded { by }` for one
a newer, different request displaced — and wakes its consumer after it
publishes, while a joined request ends only with the one it joined (`Joined`,
carrying that one's terminal, the worker's end included: a terminal published
for a joined identity is refused) and `WorkerGone` comes from the ledger; it
says no failure line of its own (the ledger says each failed request once, a
joined one carrying a failure included, or once per streak); and its
`LaneUnderTest` adapter hands the real lane `Executor::scripted` and reports
`sunk` and `said`. The OS hand-off lane (`handoff_lane`, S-5) is the first
lane built this way; the font, taskbar and program-walk lanes (S-4) followed,
each on `lane::Coalescing` — the latest-value shape: one run out and at most one
waiting, a request asked while one waits joined to it when it is the same
operation and superseding it when it is not, the worker started by the first
request, every terminal held for the application and taken in mint order
below the earliest request still owed; and the computation lane (S-8,
`computation_lane`) on `lane::Queue` — the first-in-first-out shape: every request run in
the order minted unless a newer one for the same target displaces it while it waits
(`Superseded { by }`) or is the same operation and joins it, the worker started by the
first request, a refused request handed back with its job, a supersession announced by
the wake and its terminal — which carries no answer — taken past the drain mark. A lane of
several threads keeps one ledger per thread, so one thread's end ends only its own requests,
and keeps what each request asked, so a `Failed` one is settled — as its surface's own error,
or held by its pane — and asked again only by new input to that surface, rather than left in
flight or retried by a redraw. What the proof covers:
`lane_contract_tests::the_ledger_ends_every_identity_exactly_once_in_every_interleaving`
plays every order of three identities' outcomes (the six terminals, a racing
answer after a cancel, a terminal published for a joined identity itself),
their requesters' drops and the worker's end — 4,103,588 interleavings — on the
real `Ledger`, and requires one delivery per identity, at its requester or its
sink, with the terminal the contract names, each failed request said once, one
wake and the fault exactly when the worker ended. It does not cover a lane's
own admission policy or its queue; the lane's claims do, through its adapter.

Where and what to do:
`lane_contract_tests::every_lane_runs_every_claim_and_fails_only_where_the_ledger_says`
holds each lane to `EXPECTED_FAILURES` both ways (an undeclared failure and an
unexpected pass are red), and `scripts/ci/check-expected-failures.ps1` holds the
list — kept as data in `crates/bt-app/src/lane_expected_failures.tsv`, equal to
the code by `lane_contract_tests::the_expected_failures_are_this_list` — to the
merge base: no lane gains a row for a claim the merge base already
had; a claim new to the suite brings the rows of the lanes that do not keep it
yet. Since S-8 every lane keeps every claim and the list's `# rows` holds only
its header — the shrink's end, which the script accepts. When red, repair the
lane (a repair removes its row in the same change), or restore the behaviour the
claim caught; do not add a row to quiet it.

## Contained panics

The rule: under the resident application's panic hook a panic ends the process
before any unwinding unless the hook's one decision,
`the_hook_lets_this_panic_unwind`, says it is contained, so the only catch of a
panic in the product is the boundary that decision honours,
`bt_platform::contained_panic::contain` (re-exported as `bt-app`'s
`contained_panic`), or a catch inside a closure handed to it. A worker whose
panic is meant to be a value runs its work inside `contain`; one whose panic is
meant to end the process catches nothing, and its `# completions` row says so
(`docs/ARCHITECTURE.md` §5.5; T-PANIC-ROWS-HONEST, 2026-10-12).

Where and what to do: `crates/bt-source/tests/contained_panic.rs` reads every
product occurrence of `catch_unwind` in `bt-app`, `bt-platform` and `bt-pty`
through `bt_source` items — the innermost item must be the boundary itself, by
its identity (`bt-platform`, `crate::contained_panic`, `contain`), or the
innermost closure around the occurrence must be an argument of a call whose
path `bt_source::FunctionPaths` follows, through `use` items and re-exports
across the workspace's crates, to exactly that item: a call edge, never a
spelling. Its fixture, read beside `bt-platform`, allows a catch handed to the
real item by an alias and by its path, and refuses a lookalike
`contained_panic::contain` (its body and its caller), a closure handed to a
helper that calls the boundary, a bare catch and a renamed import, by site. There is no allowlist. When red, run the work inside `contain` and give
the panic its row's failure value, or take the catch out and say in the row
that a panic ends the process; never wrap a catch to quiet the guard.

The join half (T-JOINED-PANIC-VALUES, 2026-10-12): a `JoinHandle::join` answers
`Err` under the resident hook only for a thread whose body is contained, so a
join whose `Err` is made into a value — a variant, an error, a default — must
join such a thread. The same file reads every product `join` of `bt-app`,
`bt-platform`, `bt-pty`, `bt-term`, `bt-render`, `bt-corpus` and `bt-persist`:
one that throws its result away or carries the panic on (`unwrap`, `expect`) is
not judged; any other is refused unless its handle follows — through `let`s,
values given to a `self` field in its package, free functions' tails (by
`FunctionPaths`) and method receivers — to a thread start (`std`'s
`thread::spawn`, a `spawn` method, or the thread door `spawn_at_priority`
by its identity) whose closure is a call edge to the boundary. Every occurrence
of `join` is a supported shape or refused with its site, never passed: a handle
handed in as a parameter, bound by a pattern that is not the whole value,
returned by an associated function or another method, started through a renamed
`use` of `spawn` or taken out of a collection is refused; so are `join()` in a
macro's tokens, a path-called `join` in a macro's tokens whatever its arguments
(`JoinHandle::join(h)` inside `assert!`), and `JoinHandle::join` handed on as a
value. A `T::join(x)` through a type other than `JoinHandle` passes only when it
is one of a closed list — the four `windows_future` async types
(`IAsyncAction`, `IAsyncOperation`, `IAsyncActionWithProgress`,
`IAsyncOperationWithProgress`) written in full, `windows_future::…::join`, the
WinRT deployment waits — and the first segment is not shadowed there (no
workspace crate, no `use` or glob in scope, no item or module of the package by
that name); any other path-called `join` is refused, a generic parameter's
associated type (`T::Handle::join(h)`) and a dependency's own thread handle
written from its crate root (`ScopedJoinHandle::join(h)`) among them. A method
`join` that takes arguments, a `fn join` declaration and a name that is not
called (a binding, a field, a member: `let (join, run)`, `layout.join`) are no
join. Its fixture `tests/fixtures/joined_panic` allows a door thread contained
through an alias and a contained handle kept in a field, and refuses a bare
thread's `Err → Panicked` arm, the lookalike, a free function's bare handle made a
default, a parameter, a helper-mediated body, an associated function's handle, a
renamed spawn, a collection, a macro's `join()`, a join handed on as a value, a
foreign type's join through an imported name, a path-called join in a macro, a
generic parameter's handle, a dependency's thread handle and a listed type's
join whose crate name a local module shadows. When red,
contain the thread's body and give the panic its failure value, or carry the
panic on (`expect`) and say the thread's panic ends the process.

## Timing-bound tests

The rule: a test that depends on wall-clock progress is exceptional, named, and
bounded; ordinary tests synchronize on state. The exact inventory was ruled by
T-GATES on 2026-09-30.

Where and what to do: `docs/plans/TIMING-BOUND-TESTS.tsv`,
`crates/bt-source/tests/timing.rs`, `scripts/ci/check-timing-bound.ps1`, and
`scripts/ci/check-timing-bound-tests.ps1` are the gate. When red, replace the
sleep/deadline with a deterministic signal; if time itself is the subject, add
the narrow test and its reason to the inventory in the same change.

A product budget that a test reaches but does not test is a per-instance seam,
never a global or an environment variable. The session writer's save budget
(`persist::SESSION_SAVE_BUDGET`, three seconds) is each store's own value from
its opening; `SessionStore::set_save_budget` (`#[cfg(test)]`) gives one store
the suite's patience (`lane::PATIENCE`). A test that writes real files and asks
whether they landed sets it, so a loaded machine answers later rather than with
a timeout. A test whose subject is the budget keeps the product's:
`persist::tests::a_quit_leaves_a_writer_that_never_answers_behind` and the
stalled writer of
`persist::tests::a_write_that_misses_the_budget_still_answers_true_and_keeps_the_last_layout`.
Every product store keeps the product's budget.

A wait for an engine's terminal answer about a load is not a number of its
own: it is `LOAD_ANSWER_PATIENCE` (`bt-platform`'s `video_load`), derived from
the product's `LOAD_BUDGET` and `IDLE_POLL_INTERVAL` so that it cannot end
before the engine's own refusal lands, and handed to the engine's wait door —
so it is not a row (T-VIDEO-ENGINE-NO-DURATION).

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

## Embedded spaces in string literals

The rule: a message literal is one sentence; a literal that wraps continues
with `\` at the end of the line, so the break and the next line's indentation
are not in the string (`docs/CONVENTIONS.md` §五, T-EMBEDDED-SPACES-LITERALS,
2026-10-13). A lost `\` leaves a run of spaces inside the message that the
compiler and rustfmt both accept.

Where and what to do: `scripts/ci/check-embedded-spaces.ps1` lexes every
tracked `.rs` under `crates/` (comments, character literals and lifetimes
skipped; every string form, raw ones included) and refuses, by file:line, a
string literal with a run of three or more spaces: inside a source line, with
a character other than a space before it and a non-blank after it; or, in an
escaping (non-raw) literal, at the start of a source line that follows a real
line break (no `\` continuation before it) when a non-blank follows — the
shape a `\` lost at the end of a wrapped line leaves. Blanks after the opening
quote, after a continuation, at the end of a source line, and a raw literal's
indentation are not runs; a fixture that keeps its own line breaks is a raw
string. A shape the lexer cannot finish (a string, raw string or block comment
that does not close, a character literal whose escape or quote is missing) is
refused by its file:line, never passed. A literal with a run passes only on
its row of `docs/plans/embedded-spaces-allowed.txt` (`path:line reason`, the
line the literal opens on; two literals on one line take two rows). The list
is exact both ways (a row beyond the literals its line opens is stale) and only
shrinks against the merge base. A row's identity is its path, its literal's
own text on the source line of its first run (the characters inside the
literal on that line, trimmed — not the code around it; read for the base from
the base's own copy of the file, through the same lexer, where a file that does
not lex fails the check as `base <sha>:<path>:<line>`) and its reason, with
multiplicity. A literal that moves, or whose surrounding code is renamed or
rewrapped, keeps its row with the line corrected; a row pointed at another
literal is an added row and refused; and an edit to a listed literal's pinned
text is refused too — there is no override and no replacement path. Columns
that must evolve do not belong in a string literal: take them out (cells in a
slice joined at run time, or a fixture file read through a listed door).
`scripts/ci/check-embedded-spaces-tests.ps1` holds each rule on scratch
repositories, and `gates-can-fail` plants a literal in the real tree. When red,
repair the literal as a continuation; a fixture's columns (screen captures,
tables, code samples after an escaped line break) are the only rows, and they
were the baseline — a new fixture builds its columns another way (`format!`
padding, cells joined at run time, or a `concat!` whose spaces stand in a
literal of their own, where they start the literal and are no run).

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
