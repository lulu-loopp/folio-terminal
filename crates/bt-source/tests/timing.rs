//! **The timing-bound tests are exactly the ones on the list** — T-GATES-047,
//! the discovery half of `docs/plans/TIMING-BOUND-TESTS.tsv`, widened by
//! T-TIMING-BOUND-STRAYS (2026-10-24).
//!
//! The list names every test whose verdict can depend on real elapsed time, so
//! that the number only goes down: `scripts/ci/check-timing-bound.ps1` refuses a
//! row added against the merge base, and this test refuses a timing-bound test
//! that is not a row, or a row that names no such test. Between them a new test
//! that waits on the real clock is red either way — unlisted here, or listed
//! there.
//!
//! # What "timing-bound" means here, exactly
//!
//! A **test** is a function carrying `#[test]` (outside comments), in any
//! first-party package's `src/`, `tests/`, `examples/` or `benches/`. It is
//! **timing-bound** when one of three shapes holds.
//!
//! 1. **A real wait or a measurement in its own body** (T-GATES-047): the body —
//!    the braces of that function, closures and nested items included — calls
//!    one of [`REAL_WAITS`] or [`MEASURES`]: a stable standard-library wait whose
//!    length is real time (`thread::sleep`, `Receiver::recv_timeout`,
//!    `Condvar::wait_timeout`, `thread::park_timeout` and their variants) or a
//!    reading of how much real time passed (`Instant::elapsed`).
//! 2. **Clock arithmetic in its own body** (T-TIMING-BOUND-STRAYS): an operand
//!    of `<`, `<=`, `>`, `>=` or binary `-`, or the receiver and the argument of
//!    `duration_since` / `saturating_duration_since` / `checked_duration_since`,
//!    where **both** sides hold a real-clock reading and the readings are not the
//!    same ones. A reading is a call of a path ending `Instant::now` or
//!    `SystemTime::now` (any qualification, `<Instant>::now` too). A side holds the
//!    readings written in it and those of every name bound (`let`, `=`, `+=`)
//!    from an expression holding readings, through the bindings in scope. So
//!    `Instant::now() < give_up`, where `give_up` was bound from an earlier
//!    reading, is two readings; `now + Duration::from_millis(5) > now` is one
//!    reading and its offset, and `Instant::now() - Duration::from_secs(1)` is a
//!    reading and no other. Inside macros the arguments are read when they parse
//!    as comma-separated expressions (or `a; n`).
//! 3. **A budget measured in a test-only helper** (T-TIMING-BOUND-STRAYS): the
//!    test reaches, through edges `bt_source::call_graph` resolves (the root is
//!    the test; only test-only callables of the test's own target are followed,
//!    transitively; a product function ends the walk), a test-only callable that
//!    **measures**: it holds an `elapsed()` call, or a clock-arithmetic `-` /
//!    `duration_since` of shape 2, whose value is **kept** — returned, stored,
//!    pushed, handed to a call, bound to a name that is then kept — rather than
//!    only compared (`<`, `==`, …, or `cmp`/`lt`/… methods) or formatted (the
//!    arguments of `format!`, `print!`, `write!`, `panic!`, `assert*!` and their
//!    kin) inside the helper. A stall timer that only asks "has it been too
//!    long" is a wait, and is not this shape.
//!
//! The scan matches the called name for shape 1, not the callee, so the names
//! are only the standard library's while nothing in the workspace declares a
//! function or method by one of them — and that is asserted, not assumed
//! ([`no_workspace_function_is_named_like_a_real_wait`]). A controlled-clock
//! helper named like a trigger is a finding to rename, never a test silently
//! listed as timing-bound; `CardLoop::sleep_until` in `focus_thumb`'s tests,
//! which advances a model clock, is why `sleep_until` is not a trigger (it is
//! not a stable `std::thread` item either).
//!
//! # What each reading refuses, by file and line
//!
//! * A test body (or a reached helper) whose text does not parse.
//! * Macro arguments that do not parse as expressions and name a clock value
//!   (a reading's path, `elapsed`, a `duration_since`, or a name holding a
//!   reading or a measurement).
//! * An edge the call graph leaves unresolved — `Dynamic`, `Ambiguous`,
//!   `OtherArm` — any of whose candidates is a test-only callable of the test's
//!   target that measures or reaches one that does; an unresolved path whose
//!   last segment names such a callable; an unparsed body or a cutoff on the
//!   walk; a macro whose `macro_rules!` definition in the package names
//!   `elapsed` or `Instant`.
//!
//! # What it does not see, and who does
//!
//! * **A clock reached through a product function the test calls.** A test that
//!   hands `Duration::from_secs(5)` to a product function that polls, or whose
//!   assertion compares its own reading with an instant the product stamped (a
//!   present, a heartbeat's origin), carries a wall-clock bound this scan does
//!   not follow: the walk ends at a product function, because "a test reaches a
//!   product clock reading" holds for nearly every window test and decides
//!   nothing about distance. Review owns discovering those; the product half of
//!   that shape — a product function that is handed `now` and reads the clock
//!   again — is held by [`no_product_function_reads_again_the_clock_it_was_handed`].
//! * **A clock handed through a parameter.** A helper's `started: Instant`
//!   parameter is a value the reading does not trace to a reading; an
//!   `elapsed()` on it is still a measurement.
//! * **`Instant` and `Duration` as values.** Most of the tree's tests build a
//!   controlled clock from one `Instant::now()` plus `Duration::from_*` offsets,
//!   and their verdict does not move with the machine's speed. That is exactly
//!   the shape 2 leaves out.
//!
//! Each run leaves the list it expects in
//! `target/timing-bound-tests/TIMING-BOUND-TESTS.tsv`, with a seam column
//! chosen by what the body calls, for a ticket that converts or removes tests
//! to copy rows from — never to add them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use bt_source::call_graph::{Arms, CallGraph, ItemId, Node, Resolver, Target, Unresolved};
use bt_source::{
    DiskScope, Index, ItemKind, ItemRecord, MacroKind, Package, Pattern, Search, Span, TargetId,
    TargetKind, TargetRoot, Universe, Vendor, View, Workspace, is_vendored, needle, normalized,
    report, universes,
};

/// The list, from the workspace root.
const LIST: &str = "docs/plans/TIMING-BOUND-TESTS.tsv";

/// The list's column header.
const HEADER: &str = "test\tcrate\twall_clock_assumption\tdeterministic_seam";

/// Stable standard-library calls that wait for a length of real time:
/// `thread::sleep`, `thread::sleep_ms`, `Receiver::recv_timeout`,
/// `Condvar::wait_timeout`, `Condvar::wait_timeout_ms`,
/// `Condvar::wait_timeout_while`, `thread::park_timeout`,
/// `thread::park_timeout_ms`.
const REAL_WAITS: [&str; 8] = [
    "sleep",
    "sleep_ms",
    "recv_timeout",
    "wait_timeout",
    "wait_timeout_ms",
    "wait_timeout_while",
    "park_timeout",
    "park_timeout_ms",
];

/// Calls that read how much real time passed.
const MEASURES: [&str; 1] = ["elapsed"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `real_workspace` does.
fn universe_for(package: &Package) -> Universe {
    if package.name() == "bt-source" {
        let scopes = vec![
            DiskScope::under(package.directory().join("src")),
            DiskScope::under(package.directory().join("tests")).excluding(&["fixtures"]),
        ];
        return Universe::declare(
            "the whole of bt-source, fixtures aside",
            package.targets().to_vec(),
            scopes,
            Vendor::Excluded,
        )
        .expect("this crate is where it says it is");
    }
    universes::whole_package(package, Vendor::Excluded).expect("a package of this workspace")
}

/// Every span of `pattern` in `view`, sorted.
fn spans(index: &Index, search: &Search) -> Vec<Span> {
    let mut found = index
        .search(search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// The spans of `sorted` that lie inside `outer`.
fn inside(sorted: &[Span], outer: Span) -> impl Iterator<Item = &Span> {
    let first = sorted.partition_point(|span| span.start() < outer.start());
    sorted[first..]
        .iter()
        .take_while(move |span| span.start() < outer.end())
        .filter(move |span| span.within(outer))
}

/// The names a test answers to, one per target that compiles it:
/// `<package>:<kind>:<target> <module path>::<name>`, with the crate root's
/// `crate` left off — the target and the filter `cargo test` takes.
fn identities(index: &Index, item: &ItemRecord) -> BTreeSet<String> {
    let owners = index.file_of(item).owners();
    let mut found = BTreeSet::new();
    for declared in item.module_paths() {
        let owner = owners
            .iter()
            .filter(|owner| {
                declared == owner.module_path
                    || declared.starts_with(&format!("{}::", owner.module_path))
            })
            .max_by_key(|owner| owner.module_path.len())
            .expect("an item's module path extends its file's");
        let path = declared
            .strip_prefix("crate")
            .map_or(declared, |rest| rest.trim_start_matches("::"));
        let name = if path.is_empty() {
            item.name().to_owned()
        } else {
            format!("{path}::{}", item.name())
        };
        found.insert(format!("{} {name}", owner.target));
    }
    found
}

/// `file:line` of a union offset, the file relative to the workspace root.
fn place(index: &Index, offset: usize) -> String {
    let root = normalized(&workspace_root());
    index.locate(offset).map_or_else(
        || format!("offset {offset}"),
        |location| {
            let file = normalized(location.file);
            let shown = file.strip_prefix(&root).unwrap_or(&file);
            format!(
                "{}:{}",
                shown.display().to_string().replace('\\', "/"),
                location.line
            )
        },
    )
}

/// Whether no product build compiles `record`: its file, or an arm it stands on,
/// is test-gated (an inline `#[cfg(test)] mod` included).
fn test_only(index: &Index, record: &ItemRecord) -> bool {
    !index.file_of(record).permits_product()
        || !record
            .identities()
            .any(|identity| identity.variant.permits_product())
}

/// The targets that compile `record`'s file.
fn targets(index: &Index, record: &ItemRecord) -> BTreeSet<String> {
    index
        .file_of(record)
        .owners()
        .iter()
        .map(|owner| owner.target.to_string())
        .collect()
}

/// Every `#[test]` attribute of `index`, sorted.
fn test_attributes(index: &Index) -> Vec<Span> {
    spans(
        index,
        &Search::new(needle!(Pattern::text("#[test]")), View::CodeKeepingLiterals),
    )
}

/// Whether `item` is a test: a callable carrying `#[test]`.
fn is_test(attributes: &[Span], item: &ItemRecord) -> bool {
    item.body().is_some()
        && item.kind().is_callable()
        && inside(attributes, item.declaration()).next().is_some()
}

/// One callable's text, read for its clock (see [`reading`]).
fn read_callable(index: &Index, record: &ItemRecord) -> Result<reading::Reading, String> {
    let text = index.text(record.whole());
    let (_, block) = reading::callable(
        text,
        record.kind() == ItemKind::Function,
        record.type_owner().is_some(),
    )
    .map_err(|error| {
        format!(
            "{}: the text of `{}` does not parse ({error}), so its clock cannot be read",
            place(index, record.whole().start()),
            record.name()
        )
    })?;
    Ok(block.map(|block| reading::read(&block)).unwrap_or_default())
}

/// One timing-bound test the scan found.
#[derive(Default)]
struct Found {
    package: String,
    waits: BTreeSet<&'static str>,
    measures: bool,
    /// Shape 2: clock arithmetic in its own body.
    arithmetic: bool,
    /// Shape 3: the test-only helpers that measure, which it reaches.
    helpers: BTreeSet<String>,
}

impl Found {
    /// The row this test would have if it were listed today.
    fn row(&self, identity: &str) -> String {
        if !self.waits.is_empty() || self.measures {
            let calls: Vec<&str> = self
                .waits
                .iter()
                .copied()
                .chain(self.measures.then_some("elapsed"))
                .collect();
            let (assumption, seam) = match (self.waits.is_empty(), self.measures) {
                (false, false) => (
                    "the awaited event happens within the numeric wait",
                    "completion signal, controlled receiver or controlled sleeper",
                ),
                (true, true) => (
                    "the operation finishes within the measured real time",
                    "operation counter or controlled clock",
                ),
                _ => (
                    "the awaited event happens within the numeric wait, and the measured real time stays within its bound",
                    "completion signal plus operation counter or controlled clock",
                ),
            };
            return format!(
                "{identity}\t{}\t{assumption} (calls {} in its own body)\t{seam}",
                self.package,
                calls.join(", ")
            );
        }
        let mut assumptions = Vec::new();
        let mut seams = Vec::new();
        if self.arithmetic {
            assumptions.push(
                "two real-clock readings stand within the distance its comparison allows (clock arithmetic in its own body)"
                    .to_owned(),
            );
            seams.push("one reading handed down, or a completion signal");
        }
        if !self.helpers.is_empty() {
            assumptions.push(format!(
                "the operation finishes within the real time its helper measures (measures in its helper {})",
                self.helpers.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            seams.push("operation counter or controlled clock");
        }
        format!(
            "{identity}\t{}\t{}\t{}",
            self.package,
            assumptions.join("; "),
            seams.join(", plus ")
        )
    }
}

/// Every test of one index whose own body holds shape 1 or shape 2, added to
/// `found` by identity; what shape 2's reading cannot decide goes to `refusals`.
fn scan_index(
    index: &Index,
    package: &str,
    found: &mut BTreeMap<String, Found>,
    refusals: &mut Vec<String>,
) {
    let attributes = test_attributes(index);
    let hits: Vec<(&'static str, bool, Vec<Span>)> = REAL_WAITS
        .iter()
        .map(|name| (*name, false))
        .chain(MEASURES.iter().map(|name| (*name, true)))
        .map(|(name, measures)| {
            let search = Search::new(needle!(Pattern::call(name)), View::Identifiers);
            (name, measures, spans(index, &search))
        })
        .collect();
    for item in index.items() {
        let Some(body) = item.body() else { continue };
        if !is_test(&attributes, item) {
            continue;
        }
        let mut waits = BTreeSet::new();
        let mut measured = false;
        for (name, measures, sites) in &hits {
            if inside(sites, body).next().is_some() {
                if *measures {
                    measured = true;
                } else {
                    waits.insert(*name);
                }
            }
        }
        let arithmetic = match read_callable(index, item) {
            Ok(read) => {
                let base = item.whole().start();
                refusals.extend(
                    read.refusals
                        .iter()
                        .map(|(at, why)| format!("{}: {why}", place(index, base + at))),
                );
                !read.arithmetic.is_empty()
            }
            Err(refusal) => {
                refusals.push(refusal);
                false
            }
        };
        if waits.is_empty() && !measured && !arithmetic {
            continue;
        }
        for identity in identities(index, item) {
            let entry = found.entry(identity).or_default();
            entry.package = package.to_owned();
            entry.waits.extend(waits.iter().copied());
            entry.measures |= measured;
            entry.arithmetic |= arithmetic;
        }
    }
}

/// **Shape 3**: the test-only helpers that measure, and the walk to them from a
/// test through the call graph's resolved edges.
struct Helpers<'r, 'i> {
    resolver: &'r Resolver<'i>,
    graph: CallGraph<'r, 'i>,
    readings: RefCell<HashMap<ItemId, Result<bool, String>>>,
    reaches: RefCell<HashMap<ItemId, bool>>,
    /// Per package, its test-only callables that measure.
    measuring: RefCell<HashMap<String, Vec<ItemId>>>,
}

impl<'r, 'i> Helpers<'r, 'i> {
    fn new(resolver: &'r Resolver<'i>) -> Self {
        Self {
            resolver,
            graph: CallGraph::new(resolver),
            readings: RefCell::default(),
            reaches: RefCell::default(),
            measuring: RefCell::default(),
        }
    }

    /// Whether the language lets `from`'s body call `id`: a function or an inherent
    /// method written without `pub` is private to the module that declares it and
    /// its descendants; a trait's method, or anything `pub`, is taken as visible.
    fn visible_from(&self, id: ItemId, from: ItemId) -> bool {
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        if record.trait_name().is_some()
            || !reading::private(
                index.text(record.whole()),
                record.kind() == ItemKind::Function,
                record.type_owner().is_some(),
            )
        {
            return true;
        }
        let caller = self.resolver.record(from);
        record.module_paths().iter().any(|declared| {
            caller
                .module_paths()
                .iter()
                .any(|calling| calling == declared || calling.starts_with(&format!("{declared}::")))
        })
    }

    fn test_only(&self, id: ItemId) -> bool {
        test_only(self.resolver.index_of(id), self.resolver.record(id))
    }

    fn of_target(&self, id: ItemId, within: &BTreeSet<String>) -> bool {
        !targets(self.resolver.index_of(id), self.resolver.record(id)).is_disjoint(within)
    }

    /// Whether test-only `id` measures (its reading's kept measurements), or why
    /// it cannot be told.
    fn measures(&self, id: ItemId) -> Result<bool, String> {
        if let Some(known) = self.readings.borrow().get(&id) {
            return known.clone();
        }
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        let answer = read_callable(index, record).and_then(|read| {
            if let Some((at, why)) = read.refusals.first() {
                return Err(format!(
                    "{}: {why}",
                    place(index, record.whole().start() + at)
                ));
            }
            Ok(!read.kept.is_empty())
        });
        self.readings.borrow_mut().insert(id, answer.clone());
        answer
    }

    fn name(&self, id: ItemId) -> String {
        let record = self.resolver.record(id);
        record.type_owner().map_or_else(
            || record.name().to_owned(),
            |owner| format!("{owner}::{}", record.name()),
        )
    }

    /// `package`'s test-only callables that measure.
    fn measuring(&self, package: &str) -> Vec<ItemId> {
        if let Some(known) = self.measuring.borrow().get(package) {
            return known.clone();
        }
        let found: Vec<ItemId> = self
            .resolver
            .callables(package)
            .into_iter()
            .filter(|id| self.test_only(*id) && self.measures(*id) == Ok(true))
            .collect();
        self.measuring
            .borrow_mut()
            .insert(package.to_owned(), found.clone());
        found
    }

    /// Whether test-only `id` measures or reaches, through test-only callables
    /// of `within`, one that does.
    fn reaches_a_measurement(&self, id: ItemId, within: &BTreeSet<String>) -> bool {
        if let Some(known) = self.reaches.borrow().get(&id) {
            return *known;
        }
        self.reaches.borrow_mut().insert(id, false);
        let (hits, _) = self.walk(id, within);
        let answer = self.measures(id) == Ok(true) || !hits.is_empty();
        self.reaches.borrow_mut().insert(id, answer);
        answer
    }

    /// From `root`, every test-only callable of `within` that measures and is
    /// reached through resolved edges, and what the walk cannot decide.
    fn walk(&self, root: ItemId, within: &BTreeSet<String>) -> (BTreeSet<ItemId>, Vec<String>) {
        let mut stack = vec![root];
        let mut seen = BTreeSet::from([root]);
        let mut hits = BTreeSet::new();
        let mut refusals = Vec::new();
        while let Some(at) = stack.pop() {
            let index = self.resolver.index_of(at);
            let package = self.resolver.package(at);
            for edge in self.graph.edges(Node::Item(at)).iter() {
                let site = || place(index, edge.site.span.start());
                match &edge.to {
                    Target::Item(to) => {
                        if !self.test_only(*to) || !self.of_target(*to, within) {
                            continue;
                        }
                        match self.measures(*to) {
                            Ok(true) => {
                                hits.insert(*to);
                            }
                            Ok(false) => {}
                            Err(refusal) => refusals.push(refusal),
                        }
                        if seen.insert(*to) {
                            stack.push(*to);
                        }
                    }
                    Target::Unresolved(
                        Unresolved::Dynamic { candidates, .. }
                        | Unresolved::Ambiguous { candidates, .. }
                        | Unresolved::OtherArm { candidates },
                    ) => {
                        let measuring: Vec<String> = candidates
                            .iter()
                            .filter(|candidate| {
                                self.test_only(**candidate)
                                    && self.of_target(**candidate, within)
                                    && self.visible_from(**candidate, at)
                                    && self.reaches_a_measurement(**candidate, within)
                            })
                            .map(|candidate| self.name(*candidate))
                            .collect();
                        if !measuring.is_empty() {
                            refusals.push(format!(
                                "{}: a call to `{}` the call graph does not resolve, and a candidate measures real time or reaches a helper that does: {}",
                                site(),
                                edge.site.name,
                                measuring.join(", ")
                            ));
                        }
                    }
                    Target::Unresolved(Unresolved::Path { written, .. }) => {
                        let last = written.rsplit("::").next().unwrap_or(written);
                        let named = self.measuring(package).into_iter().any(|id| {
                            self.resolver.record(id).name() == last
                                && self.of_target(id, within)
                                && self.visible_from(id, at)
                        });
                        if named {
                            refusals.push(format!(
                                "{}: the path `{written}` names nothing the call graph places, and a test-only helper of that name measures real time",
                                site()
                            ));
                        }
                    }
                    Target::Unresolved(Unresolved::Unparsed { why }) => refusals.push(format!(
                        "{}: a body on the walk does not parse ({why})",
                        site()
                    )),
                    Target::Unresolved(Unresolved::Cutoff { what }) => {
                        refusals.push(format!("{}: the walk was cut off at {what}", site()))
                    }
                    Target::Unresolved(Unresolved::Macro { name }) => {
                        let defined = index.macros().iter().any(|record| {
                            record.kind() == MacroKind::Definition
                                && defined_macro(index, record) == name.rsplit("::").next()
                                && {
                                    let tokens = index.text(record.tokens());
                                    tokens.contains("elapsed") || tokens.contains("Instant")
                                }
                        });
                        if defined {
                            refusals.push(format!(
                                "{}: the macro `{name}!`'s expansion is not followed, and its definition here names a clock",
                                site()
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        (hits, refusals)
    }
}

/// The name a `macro_rules!` definition defines.
fn defined_macro<'i>(index: &'i Index, record: &bt_source::MacroRecord) -> Option<&'i str> {
    let written = index.text(record.span());
    let rest = written.split_once('!')?.1.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// Every test of every package that reaches a test-only helper that measures
/// (shape 3), added to `found`; what the walk cannot decide goes to `refusals`.
fn scan_helpers(
    indices: &[(String, Index)],
    found: &mut BTreeMap<String, Found>,
    refusals: &mut Vec<String>,
) {
    let packages: Vec<(&str, &Index)> = indices
        .iter()
        .map(|(name, index)| (name.as_str(), index))
        .collect();
    let resolver = Resolver::new(&packages, Arms::filtered(|_| true))
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let helpers = Helpers::new(&resolver);
    for (package, index) in indices {
        let attributes = test_attributes(index);
        for record in index.items() {
            if !is_test(&attributes, record) {
                continue;
            }
            let Some(id) = resolver.id_of(package, record) else {
                continue;
            };
            let within = targets(index, record);
            let (hits, refused) = helpers.walk(id, &within);
            refusals.extend(refused);
            if hits.is_empty() {
                continue;
            }
            for identity in identities(index, record) {
                let entry = found.entry(identity).or_default();
                entry.package = package.clone();
                entry
                    .helpers
                    .extend(hits.iter().map(|hit| helpers.name(*hit)));
            }
        }
    }
}

/// Every function or method an index declares under a trigger's name.
fn declared_triggers(index: &Index) -> Vec<String> {
    index
        .items()
        .iter()
        .filter(|item| item.kind().is_callable())
        .filter(|item| REAL_WAITS.contains(&item.name()) || MEASURES.contains(&item.name()))
        .flat_map(|item| item.identities().map(|identity| identity.to_string()))
        .collect()
}

/// Every first-party package's index.
fn workspace_indices() -> Vec<(String, Index)> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            let index = Index::build(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect()
}

/// A fixture directory under `tests/fixtures/`, as the library `package`.
fn fixture_named(directory: &str, package: &str) -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(directory);
    let universe = Universe::declare(
        format!("the {package} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: package.to_owned(),
                kind: TargetKind::Library,
                name: package.to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// The `timing` fixture: a controlled clock whose method is named like a wait,
/// and a real sleep.
fn fixture() -> Index {
    fixture_named("timing", "timing")
}

/// The committed rows, by test identity, with their crate column.
fn committed() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(workspace_root().join(LIST)).expect("the list is committed");
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    assert_eq!(lines.next(), Some(HEADER), "{LIST}'s column header");
    let mut rows = BTreeMap::new();
    for line in lines {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "{LIST}: a row is four columns: {line}");
        assert!(
            rows.insert(columns[0].to_owned(), columns[1].to_owned())
                .is_none(),
            "{LIST} names {} twice",
            columns[0]
        );
    }
    rows
}

/// Leave the list this scan expects where a ticket can copy rows from it.
fn leave_rendering(found: &BTreeMap<String, Found>) {
    let out = workspace_root().join("target").join("timing-bound-tests");
    std::fs::create_dir_all(&out).expect("target/ is writable");
    let mut text = String::from(HEADER);
    text.push('\n');
    for (identity, test) in found {
        text.push_str(&test.row(identity));
        text.push('\n');
    }
    std::fs::write(out.join("TIMING-BOUND-TESTS.tsv"), text).expect("written");
}

/// RED (T-GATES-047; widened by T-TIMING-BOUND-STRAYS) — **every test whose
/// verdict the scan's three shapes tie to the real clock is a row of the
/// timing-bound list, and every row is such a test; nothing the readings cannot
/// decide is passed over.**
///
/// The list was first written by attributing clock calls to the nearest test
/// above them, which listed source-reading tests for a product function's
/// `Instant` and could not see a new test at all. This is the list's other
/// half: the list only shrinks against the merge base
/// (`scripts/ci/check-timing-bound.ps1`), and here the tree is held to the
/// list, so a new timing-bound test cannot land by leaving the list alone.
///
/// MUTATION: add `#[test] fn planted() { std::thread::sleep(std::time::Duration::from_millis(1)); }`
/// to any crate and this names it as unlisted; so does
/// `#[test] fn planted() { let a = std::time::Instant::now(); assert!(std::time::Instant::now() > a); }`;
/// delete one row and this names that test.
#[test]
fn the_timing_bound_tests_are_exactly_the_listed_ones() {
    let indices = workspace_indices();
    let mut found = BTreeMap::new();
    let mut misnamed = Vec::new();
    let mut refusals = Vec::new();
    for (package, index) in &indices {
        scan_index(index, package, &mut found, &mut refusals);
        misnamed.extend(declared_triggers(index));
    }
    scan_helpers(&indices, &mut found, &mut refusals);
    assert!(
        misnamed.is_empty(),
        "the scan reads these names as the standard library's real waits and measurements, and \
         the workspace declares functions by them — rename each (a controlled clock is not a \
         real wait), so no test calling it is listed as timing-bound:\n  {}",
        misnamed.join("\n  ")
    );
    assert!(
        refusals.is_empty(),
        "the timing scan cannot decide these, and does not pass over them ({}):\n  {}",
        refusals.len(),
        refusals.join("\n  ")
    );
    leave_rendering(&found);
    let listed = committed();
    println!(
        "{} timing-bound tests found, {} listed",
        found.len(),
        listed.len()
    );

    let unlisted: Vec<String> = found
        .iter()
        .filter(|(identity, _)| !listed.contains_key(*identity))
        .map(|(identity, test)| format!("  {}", test.row(identity)))
        .collect();
    let stale: Vec<String> = listed
        .keys()
        .filter(|identity| !found.contains_key(*identity))
        .map(|identity| format!("  {identity}"))
        .collect();
    let wrong_crate: Vec<String> = found
        .iter()
        .filter_map(|(identity, test)| {
            let written = listed.get(identity)?;
            (written != &test.package)
                .then(|| format!("  {identity}: {written} is not {}", test.package))
        })
        .collect();
    assert!(
        unlisted.is_empty() && stale.is_empty() && wrong_crate.is_empty(),
        "{LIST} and the tree disagree.\n\n\
         Timing-bound tests that are not listed ({}) — give each a seam instead of the real \
         clock (a completion signal, a controlled receiver, sleeper or clock, one reading handed \
         down); a row is never added to let one past, and scripts/ci/check-timing-bound.ps1 \
         refuses one:\n{}\n\n\
         Rows that name no timing-bound test ({}) — the test was converted, renamed or moved; \
         remove the row (a moved test that still waits is a new row, and refused):\n{}\n\n\
         Rows with the wrong crate ({}):\n{}\n\n\
         The list this tree implies is in target/timing-bound-tests/TIMING-BOUND-TESTS.tsv.",
        unlisted.len(),
        unlisted.join("\n"),
        stale.len(),
        stale.join("\n"),
        wrong_crate.len(),
        wrong_crate.join("\n"),
    );
}

/// RED (T-GATES-047 round 4) — **a controlled clock's method named like a wait
/// is not a real wait, and a real `std::thread::sleep` still is.**
///
/// The fixture declares `CardLoop::sleep_until`, which advances a model clock
/// the way `focus_thumb`'s card loop does, and two tests: one that calls it and
/// one that sleeps. The scan must find only the second, and the fixture's own
/// `sleep_until` must not be a declared trigger.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and the controlled
/// test is found.
#[test]
fn a_controlled_sleep_until_is_not_a_real_wait_and_a_real_sleep_is() {
    let index = fixture();
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_index(&index, "timing", &mut found, &mut refusals);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["timing:lib:timing a_test_that_really_sleeps"],
        "only the real sleep is timing-bound"
    );
    assert_eq!(refusals, Vec::<String>::new());
    assert_eq!(declared_triggers(&index), Vec::<String>::new());
}

/// RED (T-TIMING-BOUND-STRAYS) — **clock arithmetic between two readings is
/// timing-bound, one reading and its offsets is not, and a clock inside macro
/// arguments this reading cannot parse is refused by file and line.**
///
/// MUTATIONS: in `reading::Walk::two_readings`, answer `true` when either side
/// holds a reading — `one_origin_and_its_offsets` is found; drop the
/// `duration_since` family from `reading::DISTANCES` — `a_reading_inside_an_assertion`
/// is not; parse no macro arguments — it is not either, and the refusal is gone.
#[test]
fn clock_arithmetic_is_two_readings_and_a_clock_in_an_unparsed_macro_is_refused() {
    let index = fixture_named("timing_strays", "strays");
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_index(&index, "strays", &mut found, &mut refusals);
    let arithmetic: Vec<&str> = found
        .iter()
        .filter(|(_, test)| test.arithmetic)
        .map(|(identity, _)| identity.as_str())
        .collect();
    assert_eq!(
        arithmetic,
        [
            "strays:lib:strays a_measurement_by_subtraction",
            "strays:lib:strays a_poll_on_a_second_reading",
            "strays:lib:strays a_reading_inside_an_assertion",
        ],
        "only two readings compared or subtracted are clock arithmetic"
    );
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert!(
        refusals[0].starts_with("crates/bt-source/tests/fixtures/timing_strays/lib.rs:49: ")
            && refusals[0].contains("`late_by!`"),
        "the refusal names the macro by file and line: {refusals:#?}"
    );
}

/// RED (T-TIMING-BOUND-STRAYS) — **a test that reaches a test-only helper that
/// measures and hands the measurement back is timing-bound, one helper down or
/// two; a helper whose readings only answer "too long?" is a wait and is not;
/// an unresolved call whose candidates measure is refused by file and line.**
///
/// MUTATIONS: in `reading::Walk::spend`, count a consumed measurement as kept —
/// `a_stall_timer_in_a_helper` is found; in `Helpers::walk`, stop at the first
/// test-only callable instead of following it — `a_budget_measured_two_helpers_down`
/// is not found; skip the `Ambiguous` arm — the refusal is gone.
#[test]
fn a_budget_measured_in_a_helper_is_found_through_the_call_graph() {
    let index = fixture_named("timing_strays", "strays");
    let held = vec![("strays".to_owned(), index)];
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    scan_helpers(&held, &mut found, &mut refusals);
    let reached: Vec<(String, Vec<String>)> = found
        .iter()
        .map(|(identity, test)| (identity.clone(), test.helpers.iter().cloned().collect()))
        .collect();
    assert_eq!(
        reached,
        [
            (
                "strays:lib:strays tests::a_budget_measured_in_a_helper".to_owned(),
                vec!["costs".to_owned()]
            ),
            (
                "strays:lib:strays tests::a_budget_measured_two_helpers_down".to_owned(),
                vec!["costs".to_owned()]
            ),
        ],
        "the two budgets are found, through one helper and through two"
    );
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert!(
        refusals[0].starts_with("crates/bt-source/tests/fixtures/timing_strays/lib.rs:")
            && refusals[0].contains("Measured::cost"),
        "the refusal names the call by file and line and the candidate that measures: {refusals:#?}"
    );
}

/// RED (T-GATES-047 round 4) — **nothing in the workspace declares a function
/// or method by a trigger's name**, so every call the scan matches is the
/// standard library's.
///
/// The list test asserts the same thing over the same indices before it
/// compares; this one says it on its own, fixture-free.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and this names
/// `focus_thumb`'s `CardLoop::sleep_until`.
#[test]
fn no_workspace_function_is_named_like_a_real_wait() {
    let misnamed: Vec<String> = workspace_indices()
        .iter()
        .flat_map(|(_, index)| declared_triggers(index))
        .collect();
    assert!(misnamed.is_empty(), "{misnamed:#?}");
}

// ── a product function handed `now` that reads the clock again ─────────────

/// **The product functions handed `now` that read the clock again on purpose**,
/// each with how many such readings it holds and why (T-TIMING-BOUND-STRAYS).
const READ_AGAIN: [(&str, usize, &str); 1] = [(
    "bt-app crate::FolioApp::settle_quit",
    1,
    "the Retire arm reads the clock after `pty_door::wait_for_retirements`, a real wait in the \
     same pass of up to `pane_retirement_deadline`, and the page teardown's bound is counted \
     from when that wait ended",
)];

/// What the handed-`now` reading found in one set of indices: per function, the
/// places of its readings again; and what it refuses.
fn handed_now(indices: &[(&str, &Index)]) -> (BTreeMap<String, Vec<String>>, Vec<String>) {
    let mut found = BTreeMap::new();
    let mut refusals = Vec::new();
    for (package, index) in indices {
        for record in index.items() {
            if test_only(index, record) {
                continue;
            }
            if record.kind() == ItemKind::Field && record.name() == "now" {
                if reading::instant_field(index.text(record.whole())) {
                    refusals.push(format!(
                        "{}: a product type carries `now: Instant` ({}); a function handed it \
                         could read the clock again without a parameter named `now`",
                        place(index, record.whole().start()),
                        record.type_owner().unwrap_or("?")
                    ));
                }
                continue;
            }
            if !record.kind().is_callable() || record.body().is_none() {
                continue;
            }
            let text = index.text(record.whole());
            let parsed = reading::callable(
                text,
                record.kind() == ItemKind::Function,
                record.type_owner().is_some(),
            );
            let (signature, Some(block)) = (match parsed {
                Ok(parsed) => parsed,
                Err(error) => {
                    refusals.push(format!(
                        "{}: the text of `{}` does not parse ({error})",
                        place(index, record.whole().start()),
                        record.name()
                    ));
                    continue;
                }
            }) else {
                continue;
            };
            let read = reading::handed_now(&signature, &block);
            let base = record.whole().start();
            refusals.extend(
                read.refusals
                    .iter()
                    .map(|(at, why)| format!("{}: {why}", place(index, base + at))),
            );
            if read.again.is_empty() {
                continue;
            }
            let identity = record
                .identities()
                .next()
                .map(|identity| identity.to_string())
                .unwrap_or_else(|| record.name().to_owned());
            found.insert(
                format!("{package} {identity}"),
                read.again
                    .iter()
                    .map(|at| place(index, base + at))
                    .collect(),
            );
        }
    }
    (found, refusals)
}

/// RED (T-TIMING-BOUND-STRAYS) — **no product function handed the turn's `now`
/// reads the clock again, but for the named list; a reading that is only the
/// origin of a measurement is not one.**
///
/// A function handed `now` is answering for that instant; a second reading in
/// its body makes the answer depend on how long the turn took before it got
/// there, and a test handing it `now` cannot hold that still — the class behind
/// `the_final_write_is_judged_and_the_verdict_is_what_decides`, whose quit
/// judged its write on a reading of its own three seconds of a loaded fixture
/// later.
///
/// # The grammar
///
/// * **Handed**: a parameter named `now` whose type is a path ending `Instant`
///   (or a reference to one), on a product function or method, or on a closure
///   inside one; a closure parameter `now` with no written type whose body
///   reads the clock is refused, and so is a product type with a field `now`
///   of `Instant` (a function handed it would be this shape unseen).
/// * **Reading again**: a call of a path ending `Instant::now` in that body,
///   closures included, nested `fn` items read on their own signature.
/// * **Admitted**: a reading bound by `let x = Instant::now();` where every use
///   of `x` in the function is the receiver of `.elapsed()` — the origin of a
///   measurement, not a second answer for the turn.
/// * **Refused**: a product body that does not parse, and macro arguments that
///   do not parse and name `Instant` and `now`.
///
/// MUTATION: put `let now = Instant::now();` back in `settle_quit`'s `Write`
/// arm — this names it, by count; drop the `elapsed` admission — this names
/// `warm_web_engine`.
#[test]
fn no_product_function_reads_again_the_clock_it_was_handed() {
    let held = workspace_indices();
    let indices: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), index))
        .collect();
    let (found, refusals) = handed_now(&indices);
    assert!(
        refusals.is_empty(),
        "the handed-now reading cannot decide these:\n  {}",
        refusals.join("\n  ")
    );
    let listed: BTreeMap<&str, usize> = READ_AGAIN
        .iter()
        .map(|(key, count, _)| (*key, *count))
        .collect();
    let mut wrong = Vec::new();
    for (key, places) in &found {
        if listed.get(key.as_str()) != Some(&places.len()) {
            wrong.push(format!(
                "  {key} reads the clock again {} time(s) ({}) — hand it the turn's `now`, or, \
                 when a real wait stands between, list it in READ_AGAIN with why",
                places.len(),
                places.join(", ")
            ));
        }
    }
    for key in listed.keys() {
        if !found.contains_key(*key) {
            wrong.push(format!(
                "  {key} is listed in READ_AGAIN and reads nothing again"
            ));
        }
    }
    println!(
        "{} product functions handed `now` read it again",
        found.len()
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// RED (T-TIMING-BOUND-STRAYS) — **the handed-now reading over its fixture**:
/// the function handed `now` that reads the clock again is found, the one whose
/// reading is a measurement's origin is not, and an untyped closure `now` and a
/// type carrying `now` are refused by file and line.
///
/// MUTATIONS: drop the measurement admission in `reading::handed_now` —
/// `handed_and_measured` is found; read closures as handed nothing — the
/// closure's refusal is gone.
#[test]
fn the_handed_now_reading_finds_refuses_and_admits_what_it_says() {
    let index = fixture_named("timing_strays", "strays");
    let (found, refusals) = handed_now(&[("strays", &index)]);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(names, ["strays crate::handed_and_read_again"]);
    assert_eq!(refusals.len(), 2, "{refusals:#?}");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("lib.rs:74: ") && refusal.contains("closure")),
        "{refusals:#?}"
    );
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("lib.rs:82: ") && refusal.contains("Carries")),
        "{refusals:#?}"
    );
}

// ── reading a body's clock ──────────────────────────────────────────────────

/// **What a body says about the real clock, read from its parse**
/// (T-TIMING-BOUND-STRAYS): shape 2's clock arithmetic, shape 3's kept
/// measurements, and the handed-`now` readings; every offset is a byte offset
/// into the text handed in.
mod reading {
    use std::collections::{BTreeMap, BTreeSet};

    use proc_macro2::{TokenStream, TokenTree};
    use syn::parse::Parser;
    use syn::punctuated::Punctuated;
    use syn::spanned::Spanned;
    use syn::visit::{self, Visit};
    use syn::{BinOp, Expr, FnArg, Pat, Stmt, Token, Type};

    /// A byte offset in the text handed to [`callable`].
    pub type Offset = usize;

    /// The methods whose receiver and argument are two instants whose distance
    /// is asked.
    const DISTANCES: [&str; 3] = [
        "duration_since",
        "saturating_duration_since",
        "checked_duration_since",
    ];

    /// The methods that compare their receiver with their argument.
    const COMPARING: [&str; 8] = ["cmp", "partial_cmp", "eq", "ne", "lt", "le", "gt", "ge"];

    /// `Duration`'s and `Instant`'s methods whose answer carries their argument's
    /// value as much as their receiver's: a measurement handed to one goes where
    /// the answer goes.
    const CARRYING: [&str; 14] = [
        "saturating_sub",
        "checked_sub",
        "saturating_add",
        "checked_add",
        "saturating_mul",
        "checked_mul",
        "checked_div",
        "abs_diff",
        "min",
        "max",
        "mul_f32",
        "mul_f64",
        "div_f32",
        "div_f64",
    ];

    /// Macros whose arguments are formatted or asserted on: a value handed to
    /// one is consumed, not kept.
    const CONSUMING: [&str; 24] = [
        "format",
        "format_args",
        "print",
        "println",
        "eprint",
        "eprintln",
        "write",
        "writeln",
        "panic",
        "unreachable",
        "todo",
        "unimplemented",
        "assert",
        "assert_eq",
        "assert_ne",
        "debug_assert",
        "debug_assert_eq",
        "debug_assert_ne",
        "dbg",
        "trace",
        "debug",
        "matches",
        "assert_matches",
        "debug_assert_matches",
    ];

    /// What one body says.
    #[derive(Debug, Default)]
    pub struct Reading {
        /// Clock arithmetic between two different readings (shape 2).
        pub arithmetic: Vec<Offset>,
        /// Measurements whose value is kept (shape 3).
        pub kept: Vec<Offset>,
        /// What this reading cannot decide, and why.
        pub refusals: Vec<(Offset, String)>,
    }

    /// The signature and body of a callable's whole text, parsed as the item it
    /// is.
    pub fn callable(
        text: &str,
        function: bool,
        owned: bool,
    ) -> syn::Result<(syn::Signature, Option<syn::Block>)> {
        if function {
            syn::parse_str::<syn::ItemFn>(text).map(|parsed| (parsed.sig, Some(*parsed.block)))
        } else if owned {
            syn::parse_str::<syn::ImplItemFn>(text).map(|parsed| (parsed.sig, Some(parsed.block)))
        } else {
            syn::parse_str::<syn::TraitItemFn>(text).map(|parsed| (parsed.sig, parsed.default))
        }
    }

    fn offset(spanned: &impl Spanned) -> Offset {
        spanned.span().byte_range().start
    }

    /// Which clock a call reads, if it is a reading.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Clock {
        Instant,
        SystemTime,
    }

    /// `Instant::now()` / `SystemTime::now()` in any qualification,
    /// `<Instant>::now()` too.
    pub fn read_of(expr: &Expr) -> Option<Clock> {
        let Expr::Call(call) = expr else { return None };
        if !call.args.is_empty() {
            return None;
        }
        let Expr::Path(path) = &*call.func else {
            return None;
        };
        let mut segments = path.path.segments.iter().rev();
        let last = segments.next()?;
        if last.ident != "now" || !last.arguments.is_none() {
            return None;
        }
        let owner = match (segments.next(), &path.qself) {
            (Some(owner), None) => owner.ident.to_string(),
            (None, Some(qself)) => {
                let Type::Path(ty) = &*qself.ty else {
                    return None;
                };
                ty.path.segments.last()?.ident.to_string()
            }
            _ => return None,
        };
        match owner.as_str() {
            "Instant" => Some(Clock::Instant),
            "SystemTime" => Some(Clock::SystemTime),
            _ => None,
        }
    }

    /// The name an expression is, when it is a lone identifier.
    fn lone_name(expr: &Expr) -> Option<String> {
        let Expr::Path(path) = expr else { return None };
        if path.qself.is_some() || path.path.segments.len() != 1 {
            return None;
        }
        Some(path.path.segments[0].ident.to_string())
    }

    /// A macro's arguments, when they parse as comma-separated expressions or as
    /// `a; n`.
    fn arguments(mac: &syn::Macro) -> Option<Vec<Expr>> {
        if let Ok(list) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            return Some(list.into_iter().collect());
        }
        let repeat = |input: syn::parse::ParseStream<'_>| -> syn::Result<Vec<Expr>> {
            let value: Expr = input.parse()?;
            input.parse::<Token![;]>()?;
            let count: Expr = input.parse()?;
            Ok(vec![value, count])
        };
        if let Ok(read) = repeat.parse2(mac.tokens.clone()) {
            return Some(read);
        }
        let matching = |input: syn::parse::ParseStream<'_>| -> syn::Result<Vec<Expr>> {
            let value: Expr = input.parse()?;
            input.parse::<Token![,]>()?;
            Pat::parse_multi_with_leading_vert(input)?;
            let mut read = vec![value];
            if input.peek(Token![if]) {
                input.parse::<Token![if]>()?;
                read.push(input.parse()?);
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
                if !input.is_empty() {
                    let rest = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
                    read.extend(rest);
                }
            }
            Ok(read)
        };
        matching.parse2(mac.tokens.clone()).ok()
    }

    /// Whether a callable's whole text declares it without `pub`.
    pub fn private(text: &str, function: bool, owned: bool) -> bool {
        let inherited = |vis: &syn::Visibility| matches!(vis, syn::Visibility::Inherited);
        if function {
            syn::parse_str::<syn::ItemFn>(text).is_ok_and(|parsed| inherited(&parsed.vis))
        } else if owned {
            syn::parse_str::<syn::ImplItemFn>(text).is_ok_and(|parsed| inherited(&parsed.vis))
        } else {
            false
        }
    }

    /// Whether a field's whole text declares a field of type `Instant` (or a
    /// reference to one).
    pub fn instant_field(text: &str) -> bool {
        syn::Field::parse_named
            .parse_str(text.trim_end_matches(','))
            .is_ok_and(|field| is_instant(&field.ty))
    }

    fn names_in(tokens: TokenStream, out: &mut Vec<String>) {
        for tree in tokens {
            match tree {
                TokenTree::Ident(ident) => out.push(ident.to_string()),
                TokenTree::Group(group) => names_in(group.stream(), out),
                _ => {}
            }
        }
    }

    /// Every name a pattern binds.
    fn pattern_names(pattern: &Pat, out: &mut Vec<String>) {
        match pattern {
            Pat::Ident(ident) => {
                out.push(ident.ident.to_string());
                if let Some((_, inner)) = &ident.subpat {
                    pattern_names(inner, out);
                }
            }
            Pat::Tuple(tuple) => tuple.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::TupleStruct(tuple) => tuple.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::Struct(fields) => fields
                .fields
                .iter()
                .for_each(|field| pattern_names(&field.pat, out)),
            Pat::Slice(slice) => slice.elems.iter().for_each(|p| pattern_names(p, out)),
            Pat::Or(or) => or.cases.iter().for_each(|p| pattern_names(p, out)),
            Pat::Reference(reference) => pattern_names(&reference.pat, out),
            Pat::Type(typed) => pattern_names(&typed.pat, out),
            Pat::Paren(paren) => pattern_names(&paren.pat, out),
            _ => {}
        }
    }

    // ── shapes 2 and 3 ──

    /// What a name holds: the readings it was bound from, and the measurements
    /// whose value it carries.
    #[derive(Clone, Debug, Default)]
    struct Binding {
        readings: BTreeSet<Offset>,
        measured: Vec<Offset>,
    }

    /// What becomes of an expression's value.
    #[derive(Clone, Debug)]
    enum Use {
        /// Returned, stored, handed on: a measurement here is kept.
        Kept,
        /// Compared, formatted, discarded: a measurement here is not.
        Consumed,
        /// Bound to these names, whose own uses decide.
        Bind(Vec<String>),
    }

    #[derive(Default)]
    struct Walk {
        scopes: Vec<BTreeMap<String, Binding>>,
        /// The names a `let` is binding while its initialiser is read.
        pending: BTreeMap<String, Vec<Offset>>,
        out: Reading,
    }

    /// Every reading an expression holds, through the bindings in scope.
    struct Readings<'w> {
        walk: &'w Walk,
        found: BTreeSet<Offset>,
    }

    impl<'ast> Visit<'ast> for Readings<'_> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if read_of(expr).is_some() {
                self.found.insert(offset(expr));
                return;
            }
            if let Some(name) = lone_name(expr) {
                if let Some(binding) = self.walk.lookup(&name) {
                    self.found.extend(binding.readings.iter().copied());
                }
                return;
            }
            visit::visit_expr(self, expr);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(args) = arguments(mac) {
                for arg in &args {
                    self.visit_expr(arg);
                }
            }
        }
    }

    impl Walk {
        fn lookup(&self, name: &str) -> Option<&Binding> {
            self.scopes.iter().rev().find_map(|scope| scope.get(name))
        }

        fn lookup_mut(&mut self, name: &str) -> Option<&mut Binding> {
            self.scopes
                .iter_mut()
                .rev()
                .find_map(|scope| scope.get_mut(name))
        }

        fn bind(&mut self, name: String, binding: Binding) {
            if self.scopes.is_empty() {
                self.scopes.push(BTreeMap::new());
            }
            self.scopes
                .last_mut()
                .expect("a scope")
                .insert(name, binding);
        }

        fn scoped(&mut self, inner: impl FnOnce(&mut Self)) {
            self.scopes.push(BTreeMap::new());
            inner(self);
            self.scopes.pop();
        }

        fn readings(&self, expr: &Expr) -> BTreeSet<Offset> {
            let mut readings = Readings {
                walk: self,
                found: BTreeSet::new(),
            };
            readings.visit_expr(expr);
            readings.found
        }

        /// Shape 2's question: do both sides hold readings, and not the same ones?
        fn two_readings(&self, left: &Expr, right: &Expr) -> bool {
            let (left, right) = (self.readings(left), self.readings(right));
            !left.is_empty() && !right.is_empty() && left != right
        }

        /// A measurement (or what a name carries) meets its use.
        fn spend(&mut self, measured: &[Offset], how: &Use) {
            if measured.is_empty() {
                return;
            }
            match how {
                Use::Kept => self.out.kept.extend_from_slice(measured),
                Use::Consumed => {}
                Use::Bind(names) => {
                    for name in names {
                        if let Some(pending) = self.pending.get_mut(name) {
                            pending.extend_from_slice(measured);
                        } else if let Some(binding) = self.lookup_mut(name) {
                            binding.measured.extend_from_slice(measured);
                        } else {
                            self.out.kept.extend_from_slice(measured);
                        }
                    }
                }
            }
        }

        /// Whether macro tokens name a clock value this walk would have to follow.
        fn names_a_clock(&self, tokens: &TokenStream) -> bool {
            let mut names = Vec::new();
            names_in(tokens.clone(), &mut names);
            let reads = names.iter().any(|name| name == "now")
                && names
                    .iter()
                    .any(|name| name == "Instant" || name == "SystemTime");
            reads
                || names
                    .iter()
                    .any(|name| name == "elapsed" || DISTANCES.contains(&name.as_str()))
                || names.iter().any(|name| {
                    self.lookup(name).is_some_and(|binding| {
                        !binding.readings.is_empty() || !binding.measured.is_empty()
                    })
                })
        }

        fn mac(&mut self, mac: &syn::Macro, how: &Use) {
            let name = mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            match arguments(mac) {
                Some(args) => {
                    let each = if CONSUMING.contains(&name.as_str()) {
                        Use::Consumed
                    } else {
                        Use::Kept
                    };
                    let _ = how;
                    for arg in &args {
                        self.expr(arg, &each);
                    }
                }
                None => {
                    if self.names_a_clock(&mac.tokens) {
                        self.out.refusals.push((
                            offset(mac),
                            format!(
                                "the arguments of `{name}!` do not parse as expressions, and they \
                                 name a clock value this reading would have to follow"
                            ),
                        ));
                    }
                }
            }
        }

        fn block(&mut self, block: &syn::Block, how: &Use) {
            self.scopes.push(BTreeMap::new());
            let last = block.stmts.len().checked_sub(1);
            for (at, stmt) in block.stmts.iter().enumerate() {
                let tail = Some(at) == last;
                match stmt {
                    Stmt::Local(local) => self.local(local),
                    Stmt::Expr(expr, semi) => {
                        let how = if tail && semi.is_none() {
                            how.clone()
                        } else {
                            Use::Consumed
                        };
                        self.expr(expr, &how);
                    }
                    Stmt::Macro(mac) => {
                        let how = if tail && mac.semi_token.is_none() {
                            how.clone()
                        } else {
                            Use::Consumed
                        };
                        self.mac(&mac.mac, &how);
                    }
                    Stmt::Item(item) => self.visit_item(item),
                }
            }
            self.scopes.pop();
        }

        fn local(&mut self, local: &syn::Local) {
            let mut names = Vec::new();
            pattern_names(&local.pat, &mut names);
            let mut readings = BTreeSet::new();
            if let Some(init) = &local.init {
                readings = self.readings(&init.expr);
                for name in &names {
                    self.pending.insert(name.clone(), Vec::new());
                }
                self.expr(&init.expr, &Use::Bind(names.clone()));
                if let Some((_, diverge)) = &init.diverge {
                    self.expr(diverge, &Use::Consumed);
                }
            }
            for name in names {
                let measured = self.pending.remove(&name).unwrap_or_default();
                self.bind(
                    name,
                    Binding {
                        readings: readings.clone(),
                        measured,
                    },
                );
            }
        }

        fn bind_pattern(&mut self, pattern: &Pat) {
            let mut names = Vec::new();
            pattern_names(pattern, &mut names);
            for name in names {
                self.bind(name, Binding::default());
            }
        }

        #[allow(clippy::too_many_lines)]
        fn expr(&mut self, expr: &Expr, how: &Use) {
            if read_of(expr).is_some() {
                return;
            }
            let at = offset(expr);
            match expr {
                Expr::Path(_) => {
                    if let Some(name) = lone_name(expr) {
                        let measured = self
                            .lookup(&name)
                            .map(|binding| binding.measured.clone())
                            .unwrap_or_default();
                        self.spend(&measured, how);
                    }
                }
                Expr::MethodCall(call) => {
                    let method = call.method.to_string();
                    if method == "elapsed" && call.args.is_empty() {
                        self.spend(&[at], how);
                        self.expr(&call.receiver, &Use::Kept);
                        return;
                    }
                    if DISTANCES.contains(&method.as_str()) && call.args.len() == 1 {
                        if self.two_readings(&call.receiver, &call.args[0]) {
                            self.out.arithmetic.push(at);
                            self.spend(&[at], how);
                        }
                        self.expr(&call.receiver, &Use::Kept);
                        self.expr(&call.args[0], &Use::Kept);
                        return;
                    }
                    if COMPARING.contains(&method.as_str()) {
                        self.expr(&call.receiver, &Use::Consumed);
                        for arg in &call.args {
                            self.expr(arg, &Use::Consumed);
                        }
                        return;
                    }
                    let each = if super::REAL_WAITS.contains(&method.as_str()) {
                        Use::Consumed
                    } else if CARRYING.contains(&method.as_str()) {
                        how.clone()
                    } else {
                        Use::Kept
                    };
                    self.expr(&call.receiver, how);
                    for arg in &call.args {
                        self.expr(arg, &each);
                    }
                }
                Expr::Call(call) => {
                    let waits = match &*call.func {
                        Expr::Path(path) => path.path.segments.last().is_some_and(|segment| {
                            super::REAL_WAITS.contains(&segment.ident.to_string().as_str())
                        }),
                        _ => false,
                    };
                    self.expr(&call.func, &Use::Consumed);
                    let each = if waits { Use::Consumed } else { Use::Kept };
                    for arg in &call.args {
                        self.expr(arg, &each);
                    }
                }
                Expr::Binary(binary) => match binary.op {
                    BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_) => {
                        if self.two_readings(&binary.left, &binary.right) {
                            self.out.arithmetic.push(at);
                        }
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                    BinOp::Eq(_) | BinOp::Ne(_) => {
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                    BinOp::Sub(_) => {
                        if self.two_readings(&binary.left, &binary.right) {
                            self.out.arithmetic.push(at);
                            self.spend(&[at], how);
                        }
                        self.expr(&binary.left, how);
                        self.expr(&binary.right, how);
                    }
                    BinOp::Add(_) | BinOp::Mul(_) | BinOp::Div(_) | BinOp::Rem(_) => {
                        self.expr(&binary.left, how);
                        self.expr(&binary.right, how);
                    }
                    BinOp::AddAssign(_)
                    | BinOp::SubAssign(_)
                    | BinOp::MulAssign(_)
                    | BinOp::DivAssign(_)
                    | BinOp::RemAssign(_) => {
                        if let Some(name) = lone_name(&binary.left) {
                            let readings = self.readings(&binary.right);
                            self.expr(&binary.right, &Use::Bind(vec![name.clone()]));
                            if let Some(binding) = self.lookup_mut(&name) {
                                binding.readings.extend(readings);
                            }
                        } else {
                            self.expr(&binary.left, &Use::Kept);
                            self.expr(&binary.right, &Use::Kept);
                        }
                    }
                    _ => {
                        self.expr(&binary.left, &Use::Consumed);
                        self.expr(&binary.right, &Use::Consumed);
                    }
                },
                Expr::Assign(assign) => {
                    if let Some(name) = lone_name(&assign.left) {
                        let readings = self.readings(&assign.right);
                        self.expr(&assign.right, &Use::Bind(vec![name.clone()]));
                        if let Some(binding) = self.lookup_mut(&name) {
                            binding.readings = readings;
                        }
                    } else {
                        self.expr(&assign.left, &Use::Kept);
                        self.expr(&assign.right, &Use::Kept);
                    }
                }
                Expr::Cast(cast) => self.expr(&cast.expr, how),
                Expr::Paren(paren) => self.expr(&paren.expr, how),
                Expr::Group(group) => self.expr(&group.expr, how),
                Expr::Reference(reference) => self.expr(&reference.expr, how),
                Expr::Unary(unary) => self.expr(&unary.expr, how),
                Expr::Try(attempt) => self.expr(&attempt.expr, how),
                Expr::Block(block) => self.block(&block.block, how),
                Expr::Unsafe(block) => self.block(&block.block, how),
                Expr::If(branch) => {
                    self.expr(&branch.cond, &Use::Consumed);
                    self.block(&branch.then_branch, how);
                    if let Some((_, otherwise)) = &branch.else_branch {
                        self.expr(otherwise, how);
                    }
                }
                Expr::Match(choice) => {
                    self.expr(&choice.expr, &Use::Kept);
                    for arm in &choice.arms {
                        self.scoped(|walk| {
                            walk.bind_pattern(&arm.pat);
                            if let Some((_, guard)) = &arm.guard {
                                walk.expr(guard, &Use::Consumed);
                            }
                            walk.expr(&arm.body, how);
                        });
                    }
                }
                Expr::Closure(closure) => self.scoped(|walk| {
                    for input in &closure.inputs {
                        walk.bind_pattern(input);
                    }
                    walk.expr(&closure.body, &Use::Kept);
                }),
                Expr::ForLoop(each) => {
                    self.expr(&each.expr, &Use::Kept);
                    self.scoped(|walk| {
                        walk.bind_pattern(&each.pat);
                        walk.block(&each.body, &Use::Consumed);
                    });
                }
                Expr::While(each) => {
                    self.expr(&each.cond, &Use::Consumed);
                    self.block(&each.body, &Use::Consumed);
                }
                Expr::Loop(each) => self.block(&each.body, &Use::Consumed),
                Expr::Let(binding) => {
                    let mut names = Vec::new();
                    pattern_names(&binding.pat, &mut names);
                    let readings = self.readings(&binding.expr);
                    for name in &names {
                        self.pending.insert(name.clone(), Vec::new());
                    }
                    self.expr(&binding.expr, &Use::Bind(names.clone()));
                    for name in names {
                        let measured = self.pending.remove(&name).unwrap_or_default();
                        self.bind(
                            name,
                            Binding {
                                readings: readings.clone(),
                                measured,
                            },
                        );
                    }
                }
                Expr::Macro(mac) => self.mac(&mac.mac, how),
                _ => visit::visit_expr(self, expr),
            }
        }
    }

    impl<'ast> Visit<'ast> for Walk {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            self.expr(expr, &Use::Kept);
        }

        fn visit_block(&mut self, block: &'ast syn::Block) {
            self.block(block, &Use::Kept);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.mac(mac, &Use::Kept);
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let outer = std::mem::take(&mut self.scopes);
            self.block(&item.block, &Use::Kept);
            self.scopes = outer;
        }
    }

    /// Shapes 2 and 3 of one body: its tail is the function's answer, so a
    /// measurement there is kept.
    pub fn read(block: &syn::Block) -> Reading {
        let mut walk = Walk::default();
        walk.block(block, &Use::Kept);
        walk.out
    }

    // ── the handed-now reading ──

    /// What the handed-now reading says of one function.
    #[derive(Debug, Default)]
    pub struct Handed {
        /// Readings of `Instant::now()` in a body handed `now`, measurement
        /// origins aside.
        pub again: Vec<Offset>,
        /// What it cannot decide, and why.
        pub refusals: Vec<(Offset, String)>,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Handedness {
        No,
        Yes,
        /// A closure parameter `now` with no written type.
        Unknown,
    }

    fn is_instant(ty: &Type) -> bool {
        match ty {
            Type::Reference(reference) => is_instant(&reference.elem),
            Type::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Instant"),
            _ => false,
        }
    }

    /// Whether a signature hands its body `now: Instant`.
    fn signature_hands_now(signature: &syn::Signature) -> bool {
        signature.inputs.iter().any(|input| {
            let FnArg::Typed(typed) = input else {
                return false;
            };
            matches!(&*typed.pat, Pat::Ident(ident) if ident.ident == "now")
                && is_instant(&typed.ty)
        })
    }

    #[derive(Default)]
    struct Uses {
        /// Uses of a name other than as the receiver of `.elapsed()`.
        other: usize,
    }

    struct HandedWalk {
        handed: Vec<Handedness>,
        /// Readings found, with whether the body they stand in is handed `now`.
        readings: Vec<(Offset, Handedness)>,
        /// `let x = Instant::now();` origins, by name.
        origins: BTreeMap<String, Vec<Offset>>,
        uses: BTreeMap<String, Uses>,
        refusals: Vec<(Offset, String)>,
    }

    impl HandedWalk {
        fn current(&self) -> Handedness {
            *self.handed.last().unwrap_or(&Handedness::No)
        }
    }

    impl<'ast> Visit<'ast> for HandedWalk {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if read_of(expr) == Some(Clock::Instant) {
                self.readings.push((offset(expr), self.current()));
                return;
            }
            match expr {
                Expr::MethodCall(call) if call.method == "elapsed" && call.args.is_empty() => {
                    if lone_name(&call.receiver).is_none() {
                        self.visit_expr(&call.receiver);
                    }
                }
                Expr::Path(_) => {
                    if let Some(name) = lone_name(expr) {
                        self.uses.entry(name).or_default().other += 1;
                    }
                }
                Expr::Closure(closure) => {
                    let mut handed = self.current();
                    for input in &closure.inputs {
                        match input {
                            Pat::Type(typed) => {
                                if matches!(&*typed.pat, Pat::Ident(ident) if ident.ident == "now")
                                {
                                    handed = if is_instant(&typed.ty) {
                                        Handedness::Yes
                                    } else {
                                        Handedness::No
                                    };
                                }
                            }
                            Pat::Ident(ident) if ident.ident == "now" => {
                                handed = Handedness::Unknown;
                            }
                            _ => {}
                        }
                    }
                    self.handed.push(handed);
                    self.visit_expr(&closure.body);
                    self.handed.pop();
                }
                _ => visit::visit_expr(self, expr),
            }
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let (Pat::Ident(ident), Some(init)) = (&local.pat, &local.init)
                && read_of(&init.expr) == Some(Clock::Instant)
                && init.diverge.is_none()
            {
                self.origins
                    .entry(ident.ident.to_string())
                    .or_default()
                    .push(offset(&*init.expr));
            }
            visit::visit_local(self, local);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            match arguments(mac) {
                Some(args) => {
                    for arg in &args {
                        self.visit_expr(arg);
                    }
                }
                None => {
                    let mut names = Vec::new();
                    names_in(mac.tokens.clone(), &mut names);
                    if self.current() != Handedness::No
                        && names.iter().any(|name| name == "Instant")
                        && names.iter().any(|name| name == "now")
                    {
                        self.refusals.push((
                            offset(mac),
                            "macro arguments that do not parse name `Instant` and `now` in a body \
                             handed `now`"
                                .to_owned(),
                        ));
                    }
                }
            }
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let handed = if signature_hands_now(&item.sig) {
                Handedness::Yes
            } else {
                Handedness::No
            };
            self.handed.push(handed);
            self.visit_block(&item.block);
            self.handed.pop();
        }
    }

    /// The handed-now reading of one function (see the guard's grammar).
    pub fn handed_now(signature: &syn::Signature, block: &syn::Block) -> Handed {
        let handed = if signature_hands_now(signature) {
            Handedness::Yes
        } else {
            Handedness::No
        };
        let mut walk = HandedWalk {
            handed: vec![handed],
            readings: Vec::new(),
            origins: BTreeMap::new(),
            uses: BTreeMap::new(),
            refusals: Vec::new(),
        };
        walk.visit_block(block);
        let admitted: BTreeSet<Offset> = walk
            .origins
            .iter()
            .filter(|(name, _)| walk.uses.get(*name).is_none_or(|uses| uses.other == 0))
            .flat_map(|(_, at)| at.iter().copied())
            .collect();
        let mut out = Handed {
            again: Vec::new(),
            refusals: walk.refusals,
        };
        for (at, handed) in walk.readings {
            if admitted.contains(&at) {
                continue;
            }
            match handed {
                Handedness::Yes => out.again.push(at),
                Handedness::Unknown => out.refusals.push((
                    at,
                    "a closure handed a `now` whose type is not written reads the clock".to_owned(),
                )),
                Handedness::No => {}
            }
        }
        out
    }
}
