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
readers (four lexical shapes, by file).
A script is a reader when it holds a read verb of its language, from a
closed list matched as written, an alias or a short verb only as a whole word
(only an ASCII letter, a digit or `_` beside it blocks a match, so `$type`,
`[type]`, `-type`, `$cat` and the word in a comment count — noise, never
silence):
PowerShell (`.ps1`, `.psm1`) `Get-Content`, `Select-String`,
`ReadAll{Text,Lines,Bytes}`, `ReadLines`, `OpenText`, `[IO.File]::Open`,
`OpenRead`, `Import-Csv`, `Get-FileHash` and the aliases `gc`, `cat`, `type`,
`sls`; Python (`.py`) the built-in `open(` (no `.` before it), `.read_text(`,
`.read_bytes(`, `.open(`,
`fileinput` and a `glob(`/`rglob(`/`iglob(` whose pattern ends in `.rs`; the
shell (`.sh`, `.bash`, `.zsh`) `cat`, `grep`, `rg`, `sed`, `awk`, `head`, `tail`, `wc`, `cut`,
`sort`, `diff`, `cmp`, `md5sum`, `shasum`, a `<` input redirection and a
`find … -name '*.rs'`. In a reader a `.rs` path literal is a hit in exactly
two spellings, `"…lib.rs"` and `'…lib.rs'`, each path-shaped, its opener the
nearest quote of its kind on the same line; a Python glob or a shell `find`
over `*.rs` is a hit with the tree it walks as its subject (the scripts'
`read_dir` + `"rs"`). Three other spellings are diagnostics that fail naming
`file:line`, the shape and what to do: a `.rs` extension inside a PowerShell
here-string (`@"…"@`, `@'…'@`) or a shell heredoc (`<<EOF` … `EOF`) that is
not one of the two spellings; an interpolated value — PowerShell's
double-quoted `"$dir/lib.rs"`, `"${x}.rs"`, `"$(…)/a.rs"` (a `$` after an even
run of backticks and before a name or an expression), the shell's
double-quoted `"$root/x.rs"`, `"${dir}/x.rs"`, `"$(pwd)/a.rs"` (a `$` after an
even run of backslashes), Python's `f"{root}/x.rs"`, `"{}.rs".format(…)` and
`"%s.rs" % …`; and any other quoted `.rs` value with its opener on its line
that is not path-shaped (`'crates/a b/lib.rs'`, an escaped `` `$ `` or `\$`,
a sub-expression holding a quote of its own, `'.rs'` alone). The author
rewrites the literal into one of the two spellings or lists the file. Out of
scope, as limits: a `.rs` with no quote on its line outside a here-string or
heredoc (a comment, `-Filter *.rs`, a bare `cat crates/x/src/lib.rs`), a `.rs`
that ends no quoted value, a closer with no opener on its line (a string
spanning lines, a Python triple-quoted body), a path handed to a command run
by a verb not on the list (`subprocess.run(['cat', …])`), and `.RS` or a verb
in another case. The walks are as narrow as written: a glob pattern on a
continuation line, `find -iname`, `find -path '*.rs'`, `-name \*.rs`, an
unquoted pattern and a `-name` on a continuation line are silent in a file with
no other verb (unreadable values in a reader), and a `.sh` whose only source
read is inside an embedded `python3 - <<'PY'` heredoc is judged by the shell's
verbs alone. A GitHub workflow (`.yml`/`.yaml` under `.github/workflows/`
only) and a composite action (`action.yml`/`action.yaml` under
`.github/actions/`) are containers, not scripts: each `run:` value of a step
(an item of `jobs.<job>.steps` or `runs.steps`) is a unit judged by exactly
the grammar above in its step's language, a reader on its own text, its
findings on the YAML file's own lines (the block scalar's offset added). The
value is a block scalar (`|`/`>` with any indicators, read line by line as
written, `>` not folded) or a one-line plain value, its YAML comment aside;
a quoted or flow value, an alias or anchor, a tag, a value continued on
further lines or keys nested under `run:` is the diagnostic
`UnreadableWorkflowRun` (rewrite it as a block or a one-line plain value).
The shape above the values is read as well, and what it does not read is the
diagnostic `UnreadableWorkflowShape` on its line, so no job or step is skipped
in silence: on the mappings the steps hang from (top level, `jobs`, a job,
the workflow's and a job's `defaults`/`defaults.run`, `runs`) a value that is
no block mapping (an anchored or tagged job body `build: &b`, an alias, a
flow value), a `steps:` that is no block sequence (`[ … ]`, `*list`), a merge
key (`<<:`); a line in one of those mappings or in a step where a node
starts that is no key (`- *step`, `- { run: … }`, `- &a run: …` even with
the step's other keys after it, `&b run: …`, `!!str run: …`, `? run`,
`&k build:` under `jobs:` — never skipped with its body unrecorded — and a
line at the steps' own column that is no `- `); a step whose item holds no
mapping (a bare `-`, `- - …`, `- # comment`); and a `run:` that is no step's
(a column-0 `---` or `...` is a document marker and skipped) — only `defaults.run`, a job's
`defaults.run` and anything under a step's `with:` stay silent, so a misread
path (a byte-order mark before `jobs:`) is a diagnostic too. An anchored or
aliased job, step or `run:` value is never read through.
The language is the step's `shell:` (before or after its `run:`), else the
job's `defaults.run.shell`, else the workflow's (`pwsh`/`powershell`
PowerShell, `bash`/`sh` the shell, `python` Python), else GitHub's default on
the job's `runs-on` (pwsh on `windows-`, bash on `ubuntu-`/`macos-`; a
`${{ matrix.<name> }}` runner is judged under both, a finding under either
counting once); any other shell or runner, a job with neither, and a
composite action's step without its own `shell:` leave it undecided, which is
the diagnostic `UndecidedWorkflowShell` on the `run:` line when the value
names `.rs` (reader or not; give the step a `shell:`) and nothing when it
names none — it has nothing the tripwire judges, as a `.rs` with no quote is
out of scope. A `${{ … }}` expression in a quoted `.rs` value of a reader
unit is interpolation in either quote (GitHub substitutes it before the shell
runs); elsewhere it is ignored, and an unquoted `.rs` path in a `run:`
(`ci.yml`'s `file=crates/bt-app/src/explorer_menu.rs`) is out of scope as in
a script. Nothing outside a step's `run:` value is read: names, `env:` and
`with:` values, `uses:` (a step that runs a script by path has that script
scanned as a file). The three workflow diagnostics are about what the scan
can read, not about a source being read: no list clears them, and a listed
workflow that has one is refused all the same; a workflow's reader hits and
the path shapes of its reader units are cleared by file, as a script's are.
The split is deliberate: a listed file is a declared reader of sources, so an
interpolated or unreadable `.rs` path in it (`InterpolatedShellPath`,
`UnreadableShellPath`, `ShellPathInAHereString`) is part of what its list vouches for, while an
undecided shell, an unreadable `run:` value or an unread workflow shape hides
what the file reads at all, which no list can vouch for.
A line that only looks like a here-string or heredoc opener
(`$m = 'a@'`, `$(( 1 << 3 ))`) opens a phantom one whose `.rs` names are
reported — noise, never a silent pass. A script reader is cleared like a Rust
one, by file: a gate or tool whose subject really is source files is a
`FileScoped` variant (the vendor-notice, dead-code, lint-probe and
embedded-spaces gates, the split plan's module-graph generator,
reader-shapes measurement and topic mover in `scripts/dev/`, and the gate
canaries of `.github/workflows/ci.yml`, which plant a defect into a committed
source and read it back); the split
freshness generator is a MIGRATION-DEBT `exception`. A suite that writes a
scratch `.rs` file into a temporary tree is no reader of a committed source and gets no
row: it spells the scratch name so that the script holds no `.rs` literal (as
`[IO.Path]::ChangeExtension('crates/x/src/lib', 'rs')` does, or by giving the
scratch file another extension), and a single-quoted scratch path the scan
reports is a false positive the author rewrites, as is a `.rs` literal in a
script that reads only something else (`bt-app-split-table.py` reads a JSON and
takes a node's name as `Path(…).stem`);
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

A limit of the reading, since the window seam (T-HEADLESS-RUNTIME-SEAM): a call
on a backend's associated type (`B::Glass`) is resolved through the bounds its
declaring trait gives that name, and those bounds are keyed by the bare names of
the associated type and of the trait — not by trait identity, as the
completions guard's `TraitResolver` keys them. Two traits declaring an
associated type of one name would be read as one; `bt-app` has one today.

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

## Call graph

The rule: a guard that asks what a function can run asks `bt_source::call_graph`, never a
spelling. `Resolver::new(&[(package, &Index)], Arms)` reads a program's packages; its
`resolve_callees(item)` turns every call site of a body into edges, and
`CallGraph::walk(roots, boundary)` follows them from a set of roots, each item once, stopping at
the boundary items (recorded in `stopped_at`, not expanded), and returns `Reach { reached,
stopped_at, foreign, unresolved, implicit }`, every foreign and unresolved edge with the path
(root … caller) that first reached it (T-BT-SOURCE-CALL-GRAPH, S-2a of
`docs/plans/design/side-effects-contract-2026-10-11.md` revision (c)2).

What it resolves: path calls through the module tree by identity (`crate`/`self`/`super`, the
module's items, `use` renames, `pub use` re-exports across packages, globs after the module's own
names, function-body `use`s); constructors (resolved, not walked); `Type::f` and `Self::f`;
methods on a receiver whose type the census's steps state (parameter, `self`, a written or a
carried `let`, a field, a pattern), through `&`, `Box`, `Rc`/`Arc` and first-party `Deref`
targets up to 16 levels, each `Deref` step an edge; a call on a stated first-party trait (`dyn`,
`impl`, a bound generic, a default's `self`, `Tr::m`) into every admitted impl and the default
where it is not overridden; `<X as Tr>::m`; a type through `type` aliases (`AliasRecord`, an index
record); an inline closure; a function named as a value; a nested `fn`. A callee outside the
packages is `Target::Foreign` with its full path after first-party `use`s (`std::sync::Mutex::lock`)
— a boundary, not a failure.

What it marks, never drops: `Dynamic` (a stored closure, a function pointer, a called value, a
first-party trait no impl answers, a foreign trait object with its first-party impls as
candidates), `Ambiguous` (an untyped receiver, every first-party method of the name as candidates;
several trait impls of one name), `Macro` (every invocation; arguments that parse as expressions
are walked, their edges carrying the macro), `OtherArm` (only another arm's items answer, or a
statement's or a match arm's `#[cfg]` the arms rule out), `Generated` (a derive's method,
`#[from]`), `Path` (a refusal: two foreign globs or one and the prelude, a type the module tree
does not hold — a macro-generated one —, a method a stated type does not declare), `Cutoff` (a
chain past the hop limit) and `Unparsed`. A first-party impl of a foreign trait runs only through
an implicit call (a `Drop` at a scope's end, an operator, `?`, `for`) or a foreign callback, so
`Reach::implicit` lists every admitted one by trait — `impl Drop for ComputationLane` among
them — for a guard to review. The module doc carries the grammar table and the kinds.

How a guard uses it: build the resolver once over the packages it judges, with `Arms` saying which
`cfg` arms count (the default is the product as `Compilation` reads it; bt-app passes its feature
evaluation); turn its ledger's rows into roots and boundary items; walk; judge `reached`,
`foreign` (by path) and `unresolved` (by kind), failing closed on what it has not reviewed. A
guard that follows an unresolved edge on its own stated rule passes it to `walk_following`, and
the edge stays in `unresolved` all the same. Where and what to do:
`crates/bt-source/tests/call_graph.rs` holds a fixture per grammar row and per kind
(`tests/fixtures/call_graph/`), each red under a named mutation, and prints the walk's counts over
the real tree (`CALL_GRAPH_REAL_TREE`; not a timing — the timing-bound list only shrinks). When a
guard needs a shape the grammar does not have, the grammar is widened in a ticket of its own, with
its fixture; a shape is never resolved by a guess.

## Cross-crate chains

The rule: every backticked span in a chain table of `docs/ARCHITECTURE.md` §7,
in every cell but `crate`, is a code name that resolves to a product item of the
row's crate, or a thread name a string literal in first-party product code spells
(T-CHAINS-72). §7 is hand-written and the ticket that moves a hop edits its row;
this guard is what makes a row that was not edited go red instead of going stale.
The grammar is listed in §7's own header and in the guard's doc: `item` at the
crate's root, `module::item` in the module it is declared in, `Type::member`
(a method, a field or a variant, its impl anywhere in the crate),
`module::Type::member`, any of them after a package segment, and
`a-thread-name`. Refused, by row or by table, and never skipped: every other
shape between backticks, a name that resolves nowhere, an item that stands only
in a test build, a package named outside the crate cell, a row whose cells do not
match the header, a `crate` cell that is not one package, an `entry` cell with no
span, an unpaired backtick, a table without the separator as its second line, a
table with no rows, a table without the two columns, and a document without §7 or
a §7 without a table.

Where and what to do: `crates/bt-source/tests/chains.rs`
`every_name_in_the_chain_tables_is_an_item_of_its_crate` (about 11 s, most of it
lowering `bt-app`), with every refusal planted in
`a_chain_row_names_an_item_or_is_refused_by_name` and
`a_section_the_guard_cannot_read_is_refused`. When red, read where the hop went
and edit the row to the item it is now; what is not a code name or a thread is
written outside the backticks, or the grammar is widened in its own change with a
planted case.

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
the function that starts the thread, never a shared type or module. Since T-BT-SOURCE-CALL-GRAPH
the path is bt-source's call graph (above, `window_waits_tests::door_reach`), walked under the
product's arms, following its resolved edges and two of the token resolver's reaches kept by name
in `followed`: an untyped call whose method name one first-party method carries, and `.into()` to
every product `From::from`; every other unresolved edge is not followed (S-2b fails closed on all
of them). The path follows a call on
a receiver stated to be a first-party trait (`dyn Trait` behind `&`, `Box`, `Arc` or `Rc`,
`impl Trait`, a generic parameter bound by the trait, `self` in the trait's default method, or
a path call `Trait::m` / `D::m` with `D: Trait`) into that trait's implementations, so a body
reached only through a trait object (the update job's `driver.prepare`) needs no list line. A
trait is its identity — package, module path and name, resolved through `use` items, globs and
re-exports by `bt_source::TraitResolver` — so two traits of one name (the macOS and the
Windows `PreparePoint`) are never followed together. Every trait the guard decides with — the traits a
product `impl` block implements, a trait's supertraits, the traits a receiver's stated type or
a bound names — comes from the resolver's checked API (`resolve_checked`,
`supertraits_checked`, `implemented_checked`): a path chained past the resolver's hop limit
fails the guard by name ("… chains past the resolver's hop limit — refused, never passed"),
never read as "no trait" (a cycle among `use`s and globs names nothing more, and is no cutoff). What is followed: the method of every
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
fails the test by name; a binding in a closed block or after the call does not shadow. The call
graph also resolves `<X as Trait>::m` (to `X`'s impl, or the trait's default) and a type written
through an alias (followed to what it names on every admitted arm). What is not followed: a stored
closure, a function pointer, a standard or third-party trait object (and a foreign supertrait's
methods, even on a first-party implementing type), and an untyped receiver whose method name is
shared; the call graph marks each of them as an unresolved edge, and the reach leaves them out.
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
worker ends owing any request, after every owed identity has its terminal (a worker that ends owing nothing wakes nobody); it admits each request
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
the wake and its terminal — which carries no answer — taken past the drain mark; and the git
mutation lane (T-GIT-MUTATIONS-ON-THE-STORAGE-WORKER, `git_mutation_lane`) on `lane::Queue` too,
one share per repository, made by its first request and let go once nothing of it is held or owed, its
identities the share's generation beside the minted number so a later share never repeats one; and the
storage worker (T-STORAGE-WORKER-ON-THE-SEAM, `persist::StorageLane`) on `Ledger` and `Executor` with a
line of its own, as the hand-off lane is — one FIFO, the trial's receipt served ahead of it, and each job
kind's own answer road, read only once the request's terminal has been taken (`persist::OnTheWorker`), or
left to the lane's sink at admission by a reader that reads its road alone. A lane of
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

## Window seam

The rule: a window's own code reaches the window through `crate::window_seam`
(T-HEADLESS-RUNTIME-SEAM, 2026-10-13; `ARCHITECTURE.md` §4.5). The types that
hold a backend — a type parameter bounded by the seam's `Backend` or by a trait
that has it among its supertraits — are written for that parameter, so the
compiler sees every call that code makes to the window as a call on a seam
trait. A holder named for the **native** backend — its backend argument left to
its default or a concrete type — is refused wherever it is written (an `impl` of
it, any other function's signature or body — the methods of a generic holder
block and of a generic trait impl included, read with the block's parameter in
scope — a `const` or `static`, a struct, enum or union's whole declaration —
fields, generics, discriminants — a backend parameter's own default aside)
unless `docs/plans/window-seam-native.tsv` names the place with its reason. It
is read in type position and in expression and pattern position
(`FolioApp::new(..)`, a struct literal, a struct or tuple-struct pattern): there
an unwritten backend is chosen by inference, read as native where no backend
parameter is in scope, and in any scope when the path names a method the holder
has only in a native block (`Runtime::create` inside a function generic over the
backend still builds the native one; a path finds an inherent method before a
trait's, so a native inherent or trait-impl method stays native-only unless a
generic inherent block has it). A nested item sees none of the scope around it;
a trait's own backend parameters, and `Self` for a backend trait, are in scope
in its methods' default bodies. The native backend's own name (`Native`,
standing first or after a module's name) is a native mention anywhere outside
the seam. An impl's generics, trait arguments, associated types and consts, a
trait's declaration (its generics, supertraits and associated items), and a
native block's associated items have no row and are refused by name; so are a
`use` that renames a holder, the native backend or the seam (at module level by
name; in a body as a mention of the item holding it, as is a rename of anything
brought from the seam), a module-level type alias of a holder or of the native
backend (its type or its generics), and an `impl` whose self type names a holder
natively without being its path.

Four general rules close what an enumeration of positions would leave open.
**Every impl below module level is read whole, in its own scope, and no listing
covers it:** one `syn` walk over each product file finds every `ItemImpl` that
is not at module level — in a signature, a body, a type position, an
initializer, a default body, wherever the syntax tree holds one — and refuses it
by name when its trait is a door of the seam, when its self type reaches a
holder (spelled, or through the body's `use` leaves and type aliases composed
with each other to a fixed point, refused when they do not settle), or when
anything in it names a holder or the native backend for the native backend (a
method body reaching a native-only method included). **A body the guard does not
read for mentions introduces no names:** in a native block's method, a listed
function, method or trait default (any self-type shape: the row's owner and the
self type name the same set of types, each path's last segment, order, prefixes,
generics and lifetimes aside) or a listed `const`, every `use` and `type`, and
any other item spelling a holder, is refused by name; a `mod` below module level
is refused anywhere, and an `extern crate` anywhere in a product file. **A body
does not alias the seam's traits or module:** a body `use` whose leaves,
expanded through the body's own to a fixed point, pass through the seam module
or resolve to a seam trait is refused by name. A body `use` of a re-exported
non-trait seam item (`LoopProxy as Win`) or a body glob of a re-exporting module
is not refused by this rule — the resolver follows traits only — and leaves no
hole: holders and the native backend are read by name wherever they are written,
the seam's traits under every name a module-level `use` gives them, and a body
glob in scope refuses any nested trait impl. **Every trait impl's trait is
resolved, or shown to be another crate's, or refused — at module level and below
it alike:** its path is expanded through the body's `use` leaves (each block's
own, every path per name, a `{self}` leaf under the module's name) and resolved
with the module's resolver, whose hop limit refuses — for every decision the
guard takes from it: a trait impl's trait, a bound that makes a type parameter a
backend parameter, a supertrait that makes a trait a backend trait, the trait
that makes a type the native backend (a cycle among `use`s and globs names
nothing more, and is not a cutoff); one the resolver does not follow passes only
when, after the module's own `use` leaves and first-party globs, it is rooted at
`std`, `core`, `alloc` or a dependency in the package's manifest that is not
also a first-party module's name in scope (beside the module, at the crate root,
or in a module the module's own globs open, one level), or is a single segment
naming a prelude trait; one spelled as a seam trait is a door — only when the
resolver followed nothing, at module level and below it alike (one it followed
to a first-party trait of the same name is that trait); a path that does not
settle, any other unresolved path, and a body `use` glob in scope are refused by
name. The seam's traits go by every name a module-level first-party `use` gives
them, and a holder, a native name or a seam trait under any of those names in a
macro's tokens is refused by name — the guard does not read macros; `paste!`,
`concat_idents!` and any `use` of the `paste` crate are refused, since they
build names from pieces. With all of these, every door of the seam the product
implements is implemented in the seam (the headless backend is test-only and is
not read).

**The platform handle stays behind the doors** (T-NATIVE-HANDLE-BEHIND-THE-SEAM,
RULE C): code written for the window — an item with a backend parameter in
scope, or whose generics, `where` clause or `impl Trait` (or `dyn`) arguments
are bounded by a seam trait: a function over `&impl WindowHost`, a generic
holder block's method, a trait's default body, a holder's declaration — names
neither `bt_platform::NativeWindow` nor `bt_platform::Compositor`, in its
signature, its body or a macro's tokens in it, and no door trait of the seam
names either in its declaration. What is followed: a name, to what it stands for,
through the body's `use` leaves, the module's own (renames included), child
modules, `crate`/`self`/`super` paths and the module's globs (a glob of
`bt_platform` opens both names), to a fixed point within the walk's hop limit; a
cycle names nothing more. What is refused, by name, wherever it is written: a
name that does not settle within the hop limit; a type alias or an associated
type naming either handle (the walk does not follow aliases, as Rules A and B do
not); and a module-level macro — an invocation or a `macro_rules!` body — whose
tokens spell either (a module-level macro is no item's, so it is read as code
written for the window). Code written for no window — the web engine's
host, a lane's executor, the platform's own glue — may name them: no door hands
it a handle. The seam's own two sealed values, `webhost::SeatAddress` (the
window's handle, read only by the engine) and `window_seam::PageTree` (the tree,
walked only by the seam), are how a page reaches its window. When red, add the
door the code needs to `WindowHost` (or `CompositorDoor`), with the
`bt-platform` call in the native impl and a recorded `WindowEffect` (or a
documented stand-in answer in `headless::PlatformAnswers`) in the headless one.

**The startup order is a shared function, not native code** (T-STARTUP-ORDER-SHARED):
`crate::startup::begin` (the removal verbs, the command line, the hand-over) and
`crate::startup::resume` (the startup migration, the script's
preparation, the window, the PowerShell questions, the first tab) are generic
over the steps they are handed, and name no holder. `main` and `Runtime::create`
hand them their steps as closures written in their own bodies, which is why the
order needed no native-list row; the headless start hands them recording
stand-ins, and the order is a behavioural test, not a reading of either body. A
new startup step that must run in that order is a field of `startup::FrontDoor`
or `startup::FirstWindow` and a `headless::StartupStep`, not a line placed by
hand between two others.

Where and what to do: `crates/bt-source/tests/window_seam.rs`
(`bt_app_reaches_its_window_only_through_the_seam`) reads `bt-app` by item
identity — every bound resolved through `use` items, globs and renames by
`TraitResolver`, the backend's position read from the holder's own parameters —
and its fixture (`tests/fixtures/window_seam/`) plants each refusal
(`every_road_around_the_window_seam_is_named`). The native list only shrinks:
`scripts/ci/check-window-seam-native.ps1` refuses a row the merge base does not
have (`gates-can-fail` plants one). When red, write the new code for the backend
parameter and add the door it needs to the seam's trait (with its `Native` impl
beside the others and its headless impl in `headless.rs`); never add a native
row for code that only reads or tells the window. A row nothing writes any more
is named too, and is taken off.

## Runtime voice

The rule: a `Runtime` method says its `diagnostics.log` line through the window it
has in hand, `WindowHost::note` (T-DIAGNOSTICS-TEST-SINK, 2026-10-17). The door's
native arm is `diagnostics::note` — the resident log and a console run's trace
queue, both process-wide and unreadable by a test — and the headless window keeps
the exact text on its ledger (`WindowEffect::Noted`, at most `headless::NOTES_KEPT`
lines between two takes — a test's bound mirroring `ime_report::PAIRING_LINES_MAX`,
not derived from it — the rest counted by `Ledger::notes_beyond_the_cap`, and a
plain `Ledger::take` after one refuses with the count rather than hand back a
shorter record), so a headless test asserts the line the road wrote, not the line a builder would have
written. A road a headless test drives says its line through the door; the roads
no headless test drives yet keep their direct lines, each a row with its reason,
and the list only shrinks.

Where and what to do: `crates/bt-source/tests/runtime_voice.rs`
(`bt_app_runtime_roads_say_their_lines_through_the_window`) reads every product
`impl` block of `Runtime` through `bt-source` and counts each mention of
`diagnostics::note` by the item holding it — a call, or the voice handed on as a
value (a helper's `say`, a body's `use`) — against
`docs/plans/runtime-voice-direct-lines.tsv` (road, shape, count, reason), both ways (an unlisted line, a count above its row, and a row whose
road says fewer or none are refused). What it cannot read is refused by its line,
never passed: a bare `note` in a runtime road that is neither that path's tail nor
a method after `.` (a name a `use`, a renamed module or a local gives it), and a
`use` anywhere in the package that names the voice or opens its module by a glob.
Not read, as limits: a free function a runtime method calls that says a line of
its own, a stored closure or function pointer holding the voice, and the other
backend holders (`FolioApp`, `App`), which have no window in hand. Its fixture
(`tests/fixtures/runtime_voice/`) plants each refusal
(`every_direct_line_in_a_runtime_road_is_named`). When red, say the line through
`self.window.window.note(..)` and read it back in the road's headless test; a row
whose road moved to the door is taken off in the same change. Across commits
`scripts/ci/check-runtime-voice.ps1` holds the list to the merge base's (or its
seed): a row the base does not carry, or a count that grew, is refused by identity
(`gates-can-fail` plants a row). A row is never added for a new line.

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
