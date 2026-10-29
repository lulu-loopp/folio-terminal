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

What every source guard calls product is `bt_source::Occurrence::in_the_product`. Since
2026-10-29 (T-CFG-TEST-USE-IS-NOT-PRODUCT, plan row 77) it reads, beside the file, the item and
the inline module, the `cfg` written on any node (`Index::test_gates`: a `use`, a `const`, an
`impl` header, a macro, a `let`, an expression, a match arm, a parameter), and the index refuses
a `cfg` whose predicate is not a name, a name-value or `all`/`any`/`not` of them, a `cfg_attr`
that writes a `cfg`, and a `cfg` where it reads no gate (`tests/index.rs`
`a_cfg_is_read_wherever_it_stands`, `a_cfg_the_reading_cannot_read_is_refused_by_its_line`).

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
the same site, module, function and reason) is invisible to it, by design. A
key moved to another module is not added (T-MOVED-NOT-ADDED-RULE, the rule of
`scripts/ci/moved-rows.ps1`): a new key whose site, function and reason are a
gone base key's, with only its `module` changed, is that key moved when it is
the one key of that identity on each side, the old module no longer lists the
function and the new one did not at the base, and its `sites` is held to the
base key's (the check reads the list, not the tree, so a same-named function
elsewhere taking over an unknown the old one dropped passes as moved, held to the
base count, as check-dead-code's U-44 rule does); anything else is added,
as before. When red,
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
Since 2026-10-29 (T-GENERATED-STAMP-REFUSES-DIRTY-TREES, plan row 99)
`scripts/ci/check-generated-stamps.ps1` (CI `logic`; its cases in
`check-generated-stamps-tests.ps1`) reads the `# measured at <sha>` line the
test skips and refuses a file without exactly one, a line off the grammar, the
` with uncommitted changes` suffix, a sha that is no commit or not an ancestor
of HEAD, and four files naming different commits; a shallow clone exits 2.
When red: commit everything, run the generator, commit its output.

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

A `const`'s or `static`'s initializer is a body of the graph (`Node::Value`): a use of the value
reaches it (`Via::Deref` for a `LazyLock`, whose initializer runs on the first deref), and its
calls and the functions it names as values are edges. An `impl` block whose self type the module
tree does not hold — a macro-generated type — is listed with its methods
(`Resolver::unplaced_impls`); `the_unplaced_impls_are_the_named_list` holds the list (24 blocks),
and their methods are reachable only as `Ambiguous` candidates.

**The unfollowed edges, by site.** `docs/plans/UNFOLLOWED-EDGES.tsv` lists every `Dynamic` edge
and every `Ambiguous` edge with other than one candidate met walking from every product callable
of bt-app — the calls the door reach does not follow — keyed by file, item, kind and the call's
name, with `sites` (the calls behind the key) and a `reason`. Since 2026-10-28 (plan row 84) its
header also states how many `Ambiguous` calls have exactly one candidate
(`Unresolved::sole_candidate`, the rule the census test's callers count by), held equal to the tree
by the same test, so a rename that takes an edge off the list moves that count.
`the_unfollowed_edges_are_the_committed_list` holds the list equal to the tree;
`scripts/ci/check-unfollowed-edges.ps1` holds it to the merge base's, refusing a key added or
grown unless its reason is `YYYY-MM-DD: <why>` — for a grown key, a reason other than the one the
base's row had (`check-unfollowed-edges-tests.ps1` plants each verdict). When red, type the
receiver or resolve the call; when the door reach must go blind there, regenerate with
`scripts/generate-unfollowed-edges.ps1` and date the reason. A function the walk reaches for the
first time brings its sites in as added keys — expected, and each takes a dated reason like any
other. A row whose name is a file-system question (`is_file`, `exists`, `metadata`,
`file_type`, …) always carries a dated reason — the request-door guard cannot classify it —, held
by `every_file_system_shaped_unfollowed_edge_carries_a_dated_reason`
(T-UNTYPED-RECEIVER-FS-CALLS-TRIAGE): type the receiver, and the guard sees the question as the
effect it is. The per-door counts are printed by
`window_waits_tests::the_unfollowed_edges_of_each_completions_door_are_printed`, a diagnostic.

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

The role and phase writers (`enter_window_thread`, `loop_running`, `exiting`,
`quit_abandoned`, `enter_standalone_main`) are called only where `PINS` places
them, each after the landmark of its owner's body that its pin names. A landmark
is placed by the call it names (T-LANDMARK-GUARD-BY-CALL, 2026-10-24): the call
of a path or a name that resolves to the landmark's item, however it is spelled
(`cli::parse(…)`, `launch_wire::hand_over(…)`, an alias such as `wire::hand_over(…)`
after `use crate::launch_wire as wire;` or a glob that brings `wire` in, a
re-export, a glob re-export), followed through the crate's `use` declarations,
explicit and glob; a method call on the receiver written in `LANDMARKS`
(`builder.build()`, `event_loop.run_app(…)`, `self.session_store.close()`); or a
variant's path (`StartCause::Init`, each declared variant `QuitStep::V`). Never a
bare identifier: a field, a statement `let` or a parameter of the owner and the
local it binds, a path that resolves to another item (one of the crate's, or one
led by a first-party crate, a dependency, `std`/`core`/`alloc` or a primitive
type), another enum's variant
and an enum's associated item are not landmarks. Any other position of a
landmark's name is refused with the name, the shape and the line: a path that
resolves to nothing (no item of the crate, a first segment that is none of those
crates) or to the landmark and another item at once
(globs), a pattern, a parameter (a closure's too), an import, a value, a method
call on another receiver, a path naming a method, a landmark's path not called
directly, a path with generic arguments or a leading `::`, `Enum::*` /
`Enum::{…}`, a rename, a macro or a nested `fn` of the name, and the landmark
itself written inside a nested `fn`'s body or the arguments of a macro other than
the standard ones that run them in place (`IN_PLACE_MACROS`: `matches!`,
`assert!`, `format!`, …) (`window_waits_tests::landmarks_in`; the planted cases are the `a_*landmark*`
tests, and `a_refused_landmark_fails_the_pinned_writers_guard` runs the whole
guard). Not followed: a landmark called inside a helper the owner calls, and a
name brought in by what the guard's `use` reader does not record (a `use` a macro
writes, an `extern crate … as`); either, for a landmark no pin names
(`hand_over`), can leave a moved writer reading the earlier landmark its pin
names. When refused, rename what is not a landmark, or
widen the grammar in its own change with a planted case.

`main`'s settles are pinned the same way (T-LAZY-INITIALIZERS-BEFORE-THE-LOOP,
2026-10-24): `profiles::settle_registry` as `main`'s first statement, before
`parse`; `persist::settle_storage_dir` in the `--remove-shell-integration` door,
before `parse`, and in the claim after the update pass, after `parse`; and
`explorer_menu::settle_package` (T-EXPLORER-MENU-PROBE-OFF-THE-WINDOW-THREAD,
2026-10-25) in the loop's process once the running image is resolved, after
`hand_over`. Every
pinned writer is spelled only as the call through its pin's module path,
`module::writer(…)` (`admission::` for the role and phase writers): an import, a
value or a rename is refused by name and path
(`a_pinned_settle_is_read_through_its_own_module_path`). Only the one segment
before `::` is read, so `x::persist::settle_storage_dir(…)` for another module
named `persist` would pass: a stated limit, the one the `admission::` rule always
had. A pin is one call: the calls are counted per (writer, owner, landmark), and
a second call matching a pin already met is reported as a surplus
(`a_second_call_matching_a_pin_is_a_surplus`).

A statement's `cfg` is refused, not read (T-WINDOW-WAITS-STATEMENT-CFG,
2026-10-26). The index reads a file's, an item's and a module's `cfg`, never a
statement's, so a gated call would count on a build it is not in:
`#[cfg(test)] admission::exiting();` after `run_app` would meet the tail's pin.
One reader (`window_waits_tests::under_statement_cfg`) serves every pin table:
a call or a landmark that stands under a `#[cfg…]` or `#[cfg_attr(…)]` — the
statement, block, match arm or list element an outer one governs, or the block an
inner `#![…]` one opens — is refused by what is called, line and owner. That
covers a pinned writer's call, an `enter_callback` entry, a remover helper's call
(`deferred_removal::copy_new`, `failure_without_plan`), a pinned door's call
(below), and a landmark of `LANDMARKS`. A gated landmark is refused through the
one placement engine, so each table reports it once. A refused call's pin, entry
or helper is not also called missing or miscounted. The predicate is not
evaluated: a pin has no platform or feature column, so a call that is in one
shipped build and not another can be neither counted nor dropped. A pinned call
or a landmark that must be platform-gated moves into a gated item (a `fn`, a
module), whose `cfg` the index reads
(`a_pinned_writer_call_under_a_statement_cfg_is_refused`,
`a_callback_entry_under_a_statement_cfg_is_refused`,
`a_landmark_under_a_statement_cfg_is_refused`,
`a_remover_helper_call_under_a_statement_cfg_is_refused`).

A wait reached through the generic admission is pinned by its door
(T-DOOR-PINS-BY-DOOR, 2026-10-25): `DOOR_PINS` places each call of a pinned door
after its owner's landmark, through the same placement and count as `PINS`
(`window_waits_tests::placed_and_counted`, one engine for both tables). Row 30's
settle is admitted twice in `fn main`, after `build` (the build-error return) and
after `run_app` (the tail); row 17's flush is admitted once, inside its one road
`trace_sink::Shutdown::flush_through_its_door`, and `main` takes that road as
`trace_shutdown.flush_through_its_door()` after `build` and after `run_app`. A
pinned call with none found is red, a second matching one a surplus. "After
`run_app`" is all a tail pin says: it does not pin the settle before the footer,
or before the flush, which is the order row 30 exists for — a fixed landmark set
cannot.

What is read (`pinned_door_failures`'s grammar): every product token `admitted`,
among the tokens of the smallest function body the index records around it, or —
outside every recorded body (a signature, a module-level macro such as
`define_class!`) — among its file's. Not an admission: a declaration, a method, a
field, a `let`, a macro of the name, a name in a `use` that is not renamed, a
path that resolves only to items the crate itself declares
(`OnTheWorker::admitted`), and a bare name, neither called nor turbofished, that
resolves to nothing (a local, as the `define_class!` methods' in
`macos_http.rs`). Read first, before any resolution: an `admitted::<…>` inside a
macro's arguments (other than `IN_PLACE_MACROS`) or inside a nested `fn`'s body —
which the index does not record, so its calls would be the owner's, placed by
where they are written — is refused. A path resolving to `admitted` in
`bt_platform::admission` or `bt_effects::admission` must be the call
`admitted::<doors::Door, …>(…)`, its first generic argument written as a path
ending `doors::Door` that resolves to that admission's `doors`. The pair is
stated because the `use` reader does not follow a `use` across crates; the
consequence is that a path into any other crate is refused, not read — a third
crate's re-export of the admission (`bt_term::admission::admitted::<…>`) would
otherwise pass unseen. Refused by owner, line and what is written: a path that
resolves to nothing (a name a macro-written `use` or an `extern crate … as`
brings in), a path into another crate, a path resolving to the admission and
another item at once, no turbofish (a door inferred from the argument), an
imported or aliased door, another module's `doors`, the admission as a value
(so a local named `admitted` where the name is the admission's is refused too),
inside a macro's arguments or a nested `fn`, and a rename `admitted as …` in a
`use`. Every token of a pinned road's name but its declaration must be the call
on the pin's receiver, matched by spelling — a same-name method of another type
called through a rebound `trace_shutdown` would count as the road — outside any
macro's arguments and nested `fn`. A pinned door's call that stands under a
statement's `#[cfg…]` or `#[cfg_attr(…)]` (the statement or block an outer one
governs, or the block an inner `#![…]` one opens) is refused: the index reads a
file's, an item's and a module's `cfg` but never a statement's, so a gated call
would meet its pin on a build it is not in — the same reader, and the same
refusal, as the writer table's and the landmarks' (above). Not read: closures
and run-time-dead branches — a call in a closure's body, or in a branch that
never runs (`if cfg!(…)`, `if false`), is placed where it is written, as a
landmark is, and is still in the build, so a closure written after `run_app`
that is never called, or a settle under `if false`, would meet the tail's pin. An
unreadable admission is refused wherever it is (it may be a pinned door's), and
its owner's pins are not also called "not there" (`deleting_the_tail_settle_is_red`,
`a_second_settle_in_another_branch_is_a_surplus`,
`a_door_through_an_alias_or_another_module_is_refused`,
`a_path_to_admitted_that_names_nothing_or_another_crate_is_refused`,
`a_door_call_inside_a_nested_fn_is_refused`, `a_use_bound_is_no_use_declaration`,
`a_pinned_call_under_a_statement_cfg_is_refused`,
`a_module_level_macro_is_read_among_its_file`,
`a_road_call_is_pinned_by_its_receiver`, `a_name_that_is_not_the_admission_is_not_read`).

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

## Request doors return at once (C1)

The rule: a request door asked on the window thread performs no synchronous external effect and
waits on no lock, thread, process, file or owner door before its request leaves the thread
(C1 of `docs/plans/design/side-effects-contract-2026-10-11.md`, revision (b)2;
T-REQUEST-DOOR-GUARD, S-2b). Each door is a row of `crates/bt-app/src/window_waits.tsv`'s
`# request-doors` (`door`, `asked on`, `boundary`, `handed`, `mailbox`), one per `# completions`
door whose thread is not the window's or `main`'s (`window_waits_tests::every_request_door_has_its_line`
holds the equality both ways). A `window` row is walked over `bt_source::call_graph` from the door to
its boundary: the calls its request leaves the thread by, each of which the walk must meet. A
call that takes the worker's body names that argument, `key@N` (`lane::Executor::start_worker@2`,
counted as `Handed::argument` counts: a method's receiver not counted, a UFCS path call's every
written argument counted); a closure literal written there is the worker's and is not walked
(`CallSite::closures`, `Handed`; an `@N` on a foreign `std::`/`core::` callee is refused, its
signature unread), while every other closure written at the call (one the boundary
item runs on the caller's thread: `admit_between`'s `between`, a refusal's voice) and the call's own
body are walked as the door's. A first-party boundary item is walked, not a stop: a stop would hide
the enqueue's own locks. `—` is a door that hands nothing over (it arms a wait); its walk is judged
all the same, with no boundary to meet. A row asked `main (before the loop)` or in a `door
process` is not walked, and those rows are a closed list (`window_waits_tests::NOT_WALKED`): a
walked door that reaches one is a `NotWalked` violation, since the reach contradicts the row, and
moving a row out of the walk is a diff that list names. The walk fails on an effect (a
`# vocabulary` path; under `std::fs`, `std::net`, `std::process`, `std::os::{windows, unix}::fs` or
the `std::io` read and write handles, but not a method of a `Metadata`, `FileType` or
`Permissions` value or of their `*Ext` traits, which reads the value (ruling D2, 2026-10-26); a
`Path`'s or `PathBuf`'s question to the disk (`is_file`,
`exists`, `metadata`, `read_dir`, `canonicalize`, …); `std::env::set_current_dir`; a function of the `windows`
crate's `Storage::FileSystem`, `System::Registry` and `System::IO` modules, or the shell's
`ShellExecute(Ex)W` and `SHCreateStreamOnFileEx`), an `# entrances` item, an owner door's or a
worker's body, a blocking primitive outside the row's `mailbox` items (the `std::sync` and
`std::thread` waits, joins, parks, scopes and `Once`, and the Win32 `WaitFor*Object*`,
`SignalObjectAndWait` and `SleepEx`), another window door that does not conform (`Inherits`, by its
status), a boundary never met, and every unresolved edge but an allowed macro (never the print,
write or `dbg!` family) and a derive's or `#[from]`'s callee; `Cutoff` and `Unparsed` are never
listable. Every `mailbox` item is a dated line of a closed list (`window_waits_tests::MAILBOX`),
which says what the item holds its lock across: naming a new item in a cell needs its line, and a
line no cell names is refused. Four hold their lock across a thread start (`lane::Queue::ask`,
`lane::Coalescing::ask`, `GitMutationLane::ask_through`, `persist::SessionWriter::start`); the last
two hold a lock no worker takes, which is what makes them safe. A `mailbox` item that takes the lock
as an argument (`lane::lock(&Mutex<T>)`, read from its signature, as written or through a type
alias, which the resolver follows: `Resolver::argument_type_paths`) locks whatever its caller hands
it: it is exempt only where another `mailbox` item of the row calls it, so the cell names the methods
that lock through it. Every first-party impl of a
foreign trait is walked once as a root of its own, one `Implicit` row per impl *method*, its
verdict held by what stands behind it (an effect, a blocking primitive or a door-level finding
behind it takes only `violation`). A `const`'s and a plain `static`'s initializer are not walked
(the compiler evaluates them; a function one names runs where it is called, a `Dynamic` edge); a
`LazyLock`'s or `OnceLock`'s is. A key in any cell names one item per `cfg` arm (one per
platform's module); two items on one arm (a bare name two modules declare) are refused, and so is
a first-party `# vocabulary` or `# entrances` path naming nothing.

**The sites, by site.** `docs/plans/REQUEST-DOOR-SITES.tsv` lists every failure keyed by file,
item, kind and name (no line), with `sites`, a verdict and a reason: `std` or `follows <key>; …`
(read: nothing behind it, or the named items, which the walk then follows), `unreviewed` (not read
yet, counted), or `violation` (a C1 breach; an effect, a blocking primitive and every door-level
kind take no other verdict, the reason naming its finding; an unread row's reason is
`2026-10-24: unreviewed since 2026-10-24 (ruling Q8 (B))`). After the review's fold: 7 991 keys,
28 589 sites; 53 `std`, 7 546 `unreviewed`, 392 `violation`; 29 of the 57 walked doors breach C1.
The violations are six findings, each a ticket: (a) the window thread's `diagnostics::note` (row
20) inherited by 16 request doors and the gesture doors; (b) the trace sink and stderr written on
the calling thread; (c) the seven gesture doors that answer and draw inline (`Runtime::hand_off`,
the clipboard doors, pane reap and close), measured over each gesture's whole road; (d) two lazy
initializers that touch the file system on first use (`profiles::registry`, `persist::storage_dir`);
(e) the implicit impls behind which is an effect, a blocking primitive or a door, unproven to stay
off a request door's road; (f) `explorer_menu`'s request and probe asking the disk for the package
file on the window thread.

Where and what to do: `crates/bt-source/tests/request_doors.rs` —
`every_request_door_returns_at_once` (one index of the tree, about 20 s; prints `REQUEST_DOOR` per
door) holds the list equal to the tree, naming the doors that meet a key it does not list; the
fixture tests (`tests/fixtures/call_graph/request_doors/`) hold each class, each red under a named
mutation. `scripts/ci/check-request-door-sites.ps1` (its planted cases in
`check-request-door-sites-tests.ps1`) holds the list to the merge base's: a key added or grown,
a verdict moved to `unreviewed` or `violation` or away from `violation`, or a `follows` row
re-pointed, takes a new reason `YYYY-MM-DD: <why>`; it prints both debts' counts. `gates-can-fail` plants a file write and a stored `Box<dyn Fn()>` call
in `HandoffLane::submit` and requires each refused by file, site and door. When red: keep the
door from reaching it, or correct the row's cells (a hand-over call the boundary misses walks a
worker's closure as the door's), and regenerate with `scripts/generate-request-door-sites.ps1`,
reading each new site and dating its reason.
Since 2026-10-29 (T-WINDOW-WAITS-STALE-KEY-REFUSED, plan row 76) the walk's registry reader
refuses, by `window_waits.tsv:<line>`, an `# effects` row whose `crate`, `arm` and `item` name no
admitted item of the product or two, and a `kind` off the registry's closed list, before any walk
(`an_effects_row_names_one_item_of_its_arm_or_is_refused_by_its_line`); a moved or renamed door
body is re-keyed in the move commit, or the reader stops the list.

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
left to the lane's sink at admission by a reader that reads its road alone; and the clipboard lane
(T-CLIPBOARD-LANE, `clipboard_lane`) on `lane::Queue`, its asker a window that replaces a copy it
owes by a newer one (`Superseded`; a paste is never replaced, it queues) and withdraws a paste it no
longer wants (`Cancelled`, which the claim `AWithdrawnRequestEndsCancelledOnce` holds every lane to:
once, in the sink, never run when its work had not begun — and never said by a lane that declares
no withdrawal; and `AWithdrawnRequestLeavesTheLine`: a withdrawn request leaves the worker's line,
so the bound holds of the line as of the ledger). A lane of
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

2026-10-24 (T-TIMING-BOUND-STRAYS): the scan reads three shapes, not one. Besides
a std real wait or `elapsed()` in the test's own body, it lists **clock
arithmetic in the test's own body** (a comparison, a subtraction or a
`duration_since` whose two sides hold different `Instant::now()` /
`SystemTime::now()` readings, followed through the test's bindings; one reading
and its offsets is not one) and **a budget measured in a test-only helper** (a
test-only callable of the test's own target, reached through resolved call-graph
edges, that keeps an `elapsed()` or a two-reading difference rather than only
comparing or formatting it). Unresolved edges whose candidates measure, macro
arguments that do not parse and name a clock, and bodies that do not parse are
refused by file and line. The list's `# widened <date> by <ticket> (+<n> rows)`
line admits exactly the rows a widening finds; `check-timing-bound.ps1` refuses
any other growth. Its product half is
`no_product_function_reads_again_the_clock_it_was_handed`: a product function
handed `now: Instant` judges on that instant, and a second reading needs a
listed reason (a real wait in between). Not read: a reading handed back by a
call (`while Instant::now() < deadline_in(..)`), and an instant handed under any
name but `now` (`deadline: Instant` rightly meets a fresh reading); a widening
line counts only in a change that also changes the scan.

2026-10-27 (T-TRIAL-DEATH-WAIT-CONTROLLED-CLOCK): the trial's deadline on an update lock
holder's road is read on the road's own clock, `update_apply::TrialClock` (`Road::trial_clock`
on both platforms) — every `began_ms` the road records (a launch's, a retrial's, an adopted
trial's in `adopting`, a reserved last trial's `TrialPlanned`), each turn of `watch_trial`,
`decide`'s `now_ms` — and the watch pauses through it; the product's is `WallClock` (the wall clock and the
worker's wait door). It is a per-instance seam like the save budget above: a test that
proves the watch ended at the trial's end, not at its deadline, hands its road a
`CountedClock`, which moves only by the pauses it is asked for, ends the trial inside the
pause the test chooses, and is asked how many pauses the watch took. Elapsed real time over
the whole road is never that verdict.

2026-10-27 (T-ELAPSED-UPPER-BOUNDS-ON-OTHER-WAITS): O's wait for the applier it started is
read and paused on `update_apply::WaitClock` (`Leaving::leave_on`; `leave` hands it `WallClock`),
so a test counts its pauses with `CountedClock`; a road with no clock (a lock only tried, a mark
only read) is proven by a signal awaited within the suite's patience, or by the state it leaves.

## Test order and shared fixtures

The rule: a test's verdict does not hang on a file another build step made, on the
process's directory or environment, or on a fixture folder another test or an
earlier run shares (T-TEST-ORDER-STRAYS, 2026-10-24).

Where and what to do: `crates/bt-source/tests/test_order.rs` is the gate and
`docs/plans/TEST-ORDER-EXCEPTIONS.tsv` its register. It refuses, in test code, a
`CARGO_BIN_EXE_` literal outside an integration test's folder, in a body, a `const` initializer or product code alike (A1), a path derived from the
test executable's folder with no `bt_testpath` name (A2), a
`set_current_dir`/`set_var`/`remove_var` call (C1), and a create under the
temporary folder or the build directory with no `bt_testpath` name (D1), and a
module-level `use` that brings `env::temp_dir` in, globs `env` or renames `env`, or a
`use` inside a test body that does one of those (D0: a bare `temp_dir()` call could then
not be told from a test's own helper). A row listed twice is refused. When red,
name the path with `bt_testpath::unique_name`/`temp_path`, locate a bin through
`CARGO_BIN_EXE_<name>` in an integration test (cargo builds it for that target),
or, when the fixed name is the subject, add a row saying why. A process-global that
two tests touch is not in its grammar; the next paragraph's guard holds it. An `#[ignore]`d test that
must run alone or serially says so on its line of `scripts/ci/ignored-tests.txt`
(`run=alone`, `run=serial`).

**Process-globals tests share** (T-TEST-PROCESS-GLOBALS, 2026-10-24).
`crates/bt-source/tests/test_globals.rs` is the gate and `docs/plans/TEST-GLOBALS.tsv`
its register. A process-global is a `static` of an `Atomic*`, a non-unit `Mutex`/`RwLock`,
a `OnceLock`/`OnceCell`, a `LazyLock`, or a `static mut`. It may be written in a module or
in a body. Not one: a `thread_local!` (each test has its own thread), a unit lock (the
remedy), a static inside a `#[test]` body. A test's reach is its body and the test-only
callables of its target that it reaches through resolved call-graph edges. It also takes
in, to a fixed point, what runs without an edge: a test-only impl of a foreign trait (a
`Drop`, winit's `resumed`) whose self type a body of the reach names, a test-only
`GlobalAlloc` (in every test of its target), and a function written in an item-position
macro's tokens (`define_class!`) whose type the reach names. A global that two tests of one
binary reach (G1), or that one test's reach writes (G2), is a row. An array of cells is
the cell, and a field or index of a `static mut` under `=`, `op=` or `&mut` writes it.
`store`, `fetch_*`, `lock`, `get_or_init` and the like are writes, and any use the table
in the module doc does not read is undecided and taken as a write. G3 is the reach itself.
Refused by file and line: an unresolved edge with a test-only candidate that reaches a
global, an unresolved path whose last segment is a global's name, an unparsed body on the
walk, a global named where the call graph places no value (a glob `use` makes a name
nameable too), and an implicitly run impl that reaches a global and whose type no test's
reach names. A row is file, global, binary, class and a dated reason, and the rows equal the
hits both ways. The guard checks `lock:<NAME>` (every test that reaches the global, each
`cfg` arm on its own, reaches the one unit lock of the binary that NAME names; two locks of
one name are spelled by their module path's tail), `single writer`, `one main` (a
`harness = false` target, from its manifest) and `run=alone`/`run=serial` (each test
carries `#[ignore]` and that condition on the ignored list). It also checks that a `write-once` row's writes
are once-writes. `per-test key`, `monotone`, `write-once`, `not on a test thread` and
`serial helpers` are by hand. `scripts/ci/check-test-globals.ps1` admits an added row of
a checked class with its dated reason. It refuses an added by-hand row unless a new
`# widened <date> by <ticket> (+n rows)` line admits exactly those rows in a change to the
scan. `check-test-globals-tests.ps1` plants each verdict. When red, make the state the
test's own (a value handed in, a key per test), or take one unit lock in every test that
reaches it, or add the row. **Not seen:** a global a test reaches only through product
functions; the walk ends there, as `timing.rs`'s does. The CJK probe's `i18n` switch is
one. `the_product_reach_of_the_tests_is_measured` prints `TEST_GLOBALS_PRODUCT_REACH` as
a measure, not a gate.

## Raw-effect and thread-door exceptions

The rule: `bt-app` and `bt-platform` create workers through the admitted thread
door; bare spawn/wait effects are a small reviewed exception set. The ruling is
the 2026-09-26 thread-door design, and the live exception inventory is
`docs/plans/window-thread-bare-sites.tsv`.

Where and what to do: the raw-effect half of
`window_waits_tests::every_door_is_where_the_registry_says` and
`scripts/ci/check-window-waits.ps1` enforce it. When red, use
`spawn_at_priority` with a named worker/effect; retain a bare site only when the
door cannot own it and record that reason in the exception ledger. A worker's
site goes through its own function, which takes the worker's `&WorkerCtx` and is
a `worker-door-body` row (a family door only where one exists:
`wait::sleep_within`). A body both the window thread and a worker run will take
`&Leg<'_, doors::D>`, but a leg carries no authority yet: no function is a door
by the leg it takes, so its sites stay bare rows, and `shared-body` and
`# graphs` admit nothing until T-LEG-AUTHORITY (0.4.9). A
merge keeps both sides' removals: the script refuses a merge that brings back a
row either side removed. The registry's two closed exception lists, `# counted-only`
and `# emergency`, grow only by a ruling: a row the merge base lacks needs its
`ruling` cell — the ticket and the day of the ruling that allowed it
(`T-A2-BARE-SITE-LINT 2026-10-17`) — or `check-window-waits.ps1` refuses it; a
removal, and an existing row re-ruled (its `ruling` cell changed), are always
allowed. The ruling's day is a shape check (`YYYY-MM-DD`), read against no
clock. The script compares keys and rulings byte for byte, case included.

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
have (`gates-can-fail` plants one), except a row moved (T-MOVED-NOT-ADDED-RULE):
a `function` or `type` row whose owner is a module, or a `const` row whose owner
is a file, that left one module or file for one other with its kind and name.
A `method` or `trait` row's owner is its type, which a move between files does
not change. A row renamed in place - its kind and owner exactly one gone base
row's, and no other row of that kind and owner left or arrived - is accepted
only with a reason `YYYY-MM-DD: <TICKET-ID> <why>`, new against the base row's:
a move needs no reason, a rename does, because the item changed. When red, write the new code for the backend
parameter and add the door it needs to the seam's trait (with its `Native` impl
beside the others and its headless impl in `headless.rs`); never add a native
row for code that only reads or tells the window. A row nothing writes any more
is named too, and is taken off.

## Environment doors

The rule: a `BT_` variable is the person's shell speaking to one run of the
program, so it is read once, at the program's entry, and handed to the code
that acts on it; a library never reads one at the moment of use
(T-DIAGNOSTIC-DOORS-AT-THE-ENTRY, 2026-10-17). Read in a library, a door is
honoured by every binary that reaches the library — a test binary as readily as
`folio.exe` — and a shell left with `BT_PTY_DUMP` and `BT_CARD_TRACE` set made
the test suite write gigabytes of recordings into the folder they named.
`bt_app::entry_doors` is the product's one reader of the recording and trace
doors; `bt-pty` takes a `PtyRecording` on the command it spawns, `bt_app::trace`
and the platform's IME and summon traces open what the entry installed, and the
three crates under `bt-app` ask `bt_viewport::trace::perf`.

Where and what to do: `crates/bt-source/tests/env_doors.rs` reads every product
item (no `cfg(test)`, no integration test or build script, no `cfg` naming
`test`) of every first-party package and judges every token `var`, `var_os`,
`vars` and `vars_os` by a closed grammar: `std::env::var_os("…")` and
`std::env::var("…")` (`::std` too), `env::var_os(…)` in a file with a
module-level `use std::env;`, the bare call in a file whose `use std::env::…`
brings the name in under its own name, and in each of them an argument that is
one string literal or a `const NAME: &str = "…";` with one value in the same
file. A method call, a declaration and a bare name the file does not import
from `std::env` are no read. Every other shape that names one of the four is a
refusal by file and line, never a silent skip: a computed argument (a
parameter, `format!`, another module's constant, a macro's `$name`), an
enumeration (`vars()`, `vars_os()`), the reader handed on as a value, a path
through another module (any other bytes beside a separator, `<T as X>::var_os`
among them), a comment between the tokens (`std::env::/*…*/var_os`), a bare
reader under a glob of `std::env`, and a `use` that renames a reader. Extending the grammar is another ticket. Its fixture
`tests/fixtures/env_doors` plants one function per shape, and the test holds
the scan to a table. The list `docs/plans/ENV-DOORS.tsv` is an equality keyed
by file and item: every item that reads a `BT_` name or holds a refusal has
exactly its row, the row's `names` and `refused` are exactly what the item
reads, and a row whose item reads nothing is stale. Its categories are `entry`
(the product's entry reader and the tools' own entries), `door` (a behaviour or
test door still read where it is used; the ticket column names the follow-up
that moves it) and `unjudged` (a refused shape that reads no `BT_` door; the
reason says what it reads, and T-ENV-UNJUDGED-ROWS rewrites each). A name is a
`BT_` name in any case, as Windows reads it. **The list only shrinks**:
`scripts/ci/check-env-doors.ps1` refuses, against the merge base, a row added
and a row that reads a name or holds a refusal its base row did not. The list's
last column, `module`, is the module the item is written in, so a row moved to
another file and module in the same package, with its path inside the module
unchanged, is that row moved and held to its base row, not added
(T-MOVED-NOT-ADDED-RULE). A row renamed in place - its file, module and names
exactly one gone base row's, and no other row of those left or arrived - is
accepted, held to its base row the same way, only with a reason
`YYYY-MM-DD: <TICKET-ID> <why>`, new against the base row's: a move needs no
reason, a rename does, because the item changed. The census unknowns and the
pointer debt have no rename arm (their rows carry no reason of their own), nor
does the embedded-spaces list (its rows name no item). When red,
move the read to the entry and hand the answer down as a parameter or an
installed answer; the grammar is not widened to quiet a refusal.

**The pointer debt's import rows** (`scripts/ci/check-pointer-debt.ps1`): its rows
carry a last column, `needle`, the needle each occurrence matched, written by the
query (`pointer_app_tests::the_six_doors`); a reader moved into a module brings its imports,
and an `(outside any item)` row the base does not have is
accepted for a module and door only for a needle that an owner row moved into
that module with that door reads, once per needle. A second import row of a
needle, an import of a needle no moved owner there reads (named), and an import
row with no needle column are refused. The needles come from the moved owner
rows, never from the import's text, and rows still compare on owner and door,
so a base written before the column matches row for row (2026-10-27, ruled in
K2 batch 23, replacing the count of moved owners: two moved owners that read
three button and wheel types bring three imports and no new reader). Every import
row of one module and door shares that key, so an import row with a needle matches
only a base import row of the same needle (or one written before the column), never
one of another needle: the imports judged are the ones added, in any order of the
list (2026-10-27, batch 23's review, finding 1).

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

## Runtime topics from outside

The rule: the crate root and every module outside `runtime/` reach `runtime`'s items
only through what `runtime/mod.rs` re-exports (`runtime::AfterTheWait`,
`use runtime::platform_pointer_of;`), never by a path into a topic module
(T-ROOT-NEVER-IMPORTS-A-RUNTIME-TOPIC, 2026-10-27, plan row 80). Plan row 59 made the
topic modules `pub(crate) mod` so that rustdoc resolves the doc links that name a moved
item by its path; from then on `runtime::<topic>::item` compiles anywhere in the crate,
and this guard is what keeps K2's one road. `runtime/pointer` is a topic like the rest.

Where and what to do: `crates/bt-source/tests/root_topic_paths.rs`
(`bt_app_reaches_runtime_topics_only_through_the_re_exports`) reads, through
`bt-source`, every product path outside `runtime/` whose head is `runtime` (spelled
`runtime`, `crate::runtime`, `self::runtime` or `super::…::runtime`) followed by the
name of a child module of `crate::runtime` — private ones included — and asserts there
are none. The shapes read: a path in an expression, a type, a pattern, an `impl` header
or a generic argument (G1); a module-level `use` in any tree shape, by its
`bt_source::UseRecord` names and globs (G2); a `use` inside a body (G3); a macro's
arguments, counted with their candidate certainty (G4). Doc comments and literals are
not code and not read (G5). Product is `Occurrence::in_the_product`, whose gate grain
reads a module-level `use`'s own `cfg`, so a `#[cfg(test)] use` at the root is out of the
product by its own gate (the guard read `UseRecord::variant` itself until plan row 77, 2026-10-29). Refused by their line, never passed: a `use` that names
`runtime` under another name or opens it by a glob, a group, glob or rename after
`runtime::` inside a body, a path into a topic whose head is not `crate`, `self` or
`super` (in a path, a module-level `use` and a body group alike), a comment inside a
path into a topic (the search compares the path's bytes), and a path under a `#[cfg]` on
a statement or a match arm inside an item that a product build may compile (a test-only
one is out of the product by the query's gate grain since plan row 77, 2026-10-29; a
field's gate is the field item's own and is read). The road itself is read too: a `use` in
`runtime/mod.rs` that re-exports a module (`pub(crate) use pointer::touch;`), or any glob
there, would open a module under a new name and is refused by its line; an item
re-export is the road (ruled 2026-10-27, the row's review, F4). There is no allow list. Its fixture (`tests/fixtures/root_topic_paths/`) plants
each shape (`every_path_into_a_runtime_topic_from_outside_is_named`,
`a_test_gated_use_is_out_of_the_product_by_its_own_gate`). When red, add the name to a
re-export in `runtime/mod.rs` and spell the path `runtime::name`; gate a test-only
statement by its item, not the statement.

## Native labels

The rule: every word a person reads on a native menu or dialog that `bt-platform`
builds is passed in by the caller, from `bt-app`'s `i18n` table, with the call that
shows it (T-NATIVE-LABELS-FROM-THE-CALLER, 2026-10-21). `bt-platform` keeps no words
of its own: the menu bar and Dock rows (`menu::MenuPlan`), the formula's right-click
row (`MathContextMenu::request(copy_latex)`), the Windows image filter's name
(`ImagePicker::request(kind, start, image_types)`), an alert's title and text, the
Explorer verb and the toast sender's name (`Notifier::new(display_name, wake)`). The
system's own words — a dialog's title and buttons, AppKit's rows — stay the system's.

Where and what to do: `crates/bt-source/tests/native_labels.rs`
(`bt_platform_native_labels_come_from_the_caller`) reads the product bytes of
`menu.rs`, `macos_menu.rs`, `macos_dialogs.rs` and `explorer_command.rs` whole, of
the `impl` blocks of `MathContextMenu`, `FolderPicker`, `ImagePicker` and
`SaveFilePicker` on every arm, and of the dialog functions it names, **and of every
item and `const`/`static` initializer the call graph reaches from them** (a helper on
none of the lists, a label `const` written elsewhere, a method behind a first-party
trait object; T-NATIVE-LABELS-GUARD-REACH, 2026-10-25). What runs with no call edge
is taken in too, to a fixed point: every first-party impl of a foreign trait (`Drop`,
`Display`, an iterator's `next`) whose self type a body of the reach names, and the
functions, `static`s and `const`s an item-position macro writes (`define_class!`,
`thread_local!`) — in the scope, by a `sel!` selector, by a type a reached body names,
or by a path — read by syntax: a free call takes in every package function of its
name, `Owner::f` the functions of that type or module (refused when the package names
`Owner` and none is `f`), a method call every package method of its name, and a call
of a binding is refused, and so is a free call no package function answers whose name
a package `use` brings in or the package gives to something that is not a type; a
`const` named there whose initializer calls, names a path or names a package value is
refused; an item-position macro in the scope whose tokens do not parse
as items is refused by its line. An ambiguous, other-arm or dynamic call with
candidates is followed into every candidate but the ones a `BOUNDARY` row drops
(caller, name called, candidate, reason: a `.push` on a `Vec` never runs a
first-party `push`); an edge the guard cannot read (a dynamic call with none, an
unplaced path, a generated item, macro arguments) is refused by its line unless an
`UNREAD` row reviews it (caller, name called, kind, reason); a cut-off chain and an
unparsed body are refused with no row. Macro arguments are read where the call graph
parses them; `msg_send!`'s `part: argument` list is read as a body; any other
arguments that parse as no expression list are refused by their line when they hold a
call, a path, a macro or a package name. A call of any `macro_rules!` of `bt-platform`
is refused unless the macro is a row of `MACROS` (name, reason; empty), and a literal
inside a `macro_rules!` definition in the scope is refused by its line, because it
lands in an expansion the reading cannot see. **A string literal that holds a letter
is a label** — plain, raw, byte or C, each piece of `concat!`, read with its `&`
mnemonics taken out and its ends trimmed, so `"&Copy"`, `" copy"` and `"Save &"` are
labels too — and is refused, except by its place, a closed list: inside an attribute
(a doc comment, a class or link name), right after `extern` (an ABI), inside a call of
the guard's `ERROR_CALLS` (`Err(…)`, `.map_err(…)`, `.ok_or_else(…)`: an error text),
and inside a call of its `PROGRAMMER_CALLS` (`.expect(…)`) or a macro of its
`PROGRAMMER_MACROS` (`panic!`, `unreachable!`, the `assert` macros: a programmer's
text). Any other diagnostic-looking text is a row. A literal
that must stay is a row of the guard's `ALLOWED` (file, text as written, reason), one
row per literal. Every row allows one use and a row nothing uses is refused. The lists
are held to the merge base's by `scripts/ci/check-native-labels.ps1` (its plants in
`check-native-labels-tests.ps1`): the four row lists only shrink, the `FILES`,
`TYPES` and `FUNCTIONS` scope only grows and the exempt lists `ERROR_CALLS`,
`PROGRAMMER_CALLS` and `PROGRAMMER_MACROS` only shrink; an
honest move of an `ALLOWED` row to another file passes (`moved-rows.ps1`), and a row
added, a scope entry taken off or an exempt macro added needs a new dated
`// widened <date> by <ticket> (+<n> rows): <what>` line, counting them, in a change
to the guard's grammar. Its fixture (`tests/fixtures/native_labels/`) plants each case
(`a_literal_with_a_word_in_a_guarded_item_is_named`,
`what_a_guarded_item_reaches_is_read`, `what_runs_with_no_call_edge_is_read`,
`an_allowed_row_allows_one_literal`,
`an_ambiguous_candidate_is_followed_unless_a_row_stops_it`). When red, add the words to
`i18n.rs` (English in both columns, Chinese pending with its `CHINESE_PENDING` rows),
pass them in at the call, and take the literal out; a text no person reads gets a row
with its reason; a new guarded surface adds its item to the guard's lists. An entry of
the guard's `FILES`, `TYPES` or `FUNCTIONS` that names nothing in the index is refused
by name, so a renamed item cannot drop out silently, and a literal the index holds no
record of is refused by its line. Not seen, as limits: a literal `bt-app` passes in
(the caller's table is `bt-app`'s runtime tests' subject), a character literal, the
expansion of a macro on `MACROS`, an item a derive or an attribute macro generates, a
blanket impl (`impl<T: Bound> Trait for T`, whose self type no body names, so an
implicit call running one is never taken in), and
what a foreign function or a foreign macro's expansion does with the words.

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
rewrapped, keeps its row with the line corrected; a literal moved to another
file with its item keeps its row with the path and line corrected, and is that
row moved, not added, when its text and reason left one file for one other and
no more rows arrived than left (T-MOVED-NOT-ADDED-RULE); a row pointed at another
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
