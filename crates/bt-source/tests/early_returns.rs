//! **A test does not end before it asserts anything where its fixture answers nothing, unless
//! the register says where and why** — 0.4.10 plan row 12 (T-INSTALL-TESTS-SILENT-PASS-OFF-WINDOWS).
//!
//! Guard (source-reading by design): its subject is how the workspace's tests are written, so
//! it reads them through `bt-source`'s index, the way `test_order` and `temp_paths` do, and
//! binds to no file.
//!
//! # The class
//!
//! A test whose fixture answers `None` off one host — `let Some(install) = Install::new(tag)
//! else { return; };` over a test root only Windows signs with — returns before its first
//! assertion there, and libtest counts it passed. On the Mac, 84 of the Windows applier's tests
//! did that: green, asserting nothing. What a host leaves out is said by `#[cfg]` (CONVENTIONS
//! H1 rule 2), so such a road is either gated or a fixture that answers on every host this
//! suite runs on. The road that remains is a fact of a host the suite does not run on, or of
//! the machine's environment, and stands as a row of `docs/plans/EARLY-RETURN-FIXTURES.tsv`.
//!
//! # What is read
//!
//! **A test**: a function carrying `#[test]`, in every first-party package (`bt-source`'s
//! `tests/fixtures/` are source about compilation and are not read). In **its own body** only:
//! a `return` inside a closure, an `async` block or a nested item ends that, not the test, and
//! is not read. A helper the test calls is judged on nothing here (below).
//!
//! **A road out** is a `return` that ends the test as passed — `return;`, `return ();` or
//! `return Ok(…);` — written before any assertion of the test's own body: a macro or a call
//! named `assert…` (`assert!`, `assert_eq!`, a helper `assert_window_spent_by`), `panic!`,
//! `unreachable!`, `expect_err` or `unwrap_err`. A `return` after one of those ends a test that
//! has checked something on the way (`if host != Windows { assert!(files.is_empty()); return; }`
//! asserts the other host's half). Any other `return` hands back a value whose own verdict the
//! test is — `return Err(…)` ends red — and is not a road out.
//!
//! # The grammar: every road out stands in one of four shapes, or it is refused
//!
//! * **G1 — the else of a `let`.** `let PAT = EXPR else { …; return; };`, the else block
//!   ending in the road out. Judged by `EXPR`'s callee.
//! * **G2 — an `if` with no `else`.** `if COND { …; return; }`, the block ending in the road
//!   out; `if let PAT = EXPR` is judged by `EXPR`. Judged by `COND`'s callee.
//! * **G3 — a `match` arm.** `match EXPR { … ARM => return, … }`, the arm a road out or a
//!   block ending in one. Judged by `EXPR`'s callee.
//! * **G4 — the child half of a test.** A G1 or G2 whose `EXPR` or `COND` calls `env::var` or
//!   `env::var_os`: the test is a copy of its own binary, started by its parent half with that
//!   variable, and the parent asserts. Accepted, with no row.
//!
//! **The callee** of an expression is the path of the call it is, by its last two segments
//! (`Install::new`, `launch_pipe::endpoint_for`), read through `&`, `!`, `*`, parentheses, `?`,
//! `.await`, a field and a method's receiver (`TestCa::new().ok()` is `TestCa::new`); a method
//! called on no call is its name (`install.home.journal().exists()` is `journal`). An
//! expression that is no call in that reading — a comparison, a `cfg!`, a `&&` chain — is
//! judged by its whole text with the whitespace out
//! (`crate::host_platform()==crate::HostPlatform::OtherUnix`, `cfg!(not(any(windows,unix)))`).
//!
//! **Every other road out is refused, never silent**: a `return;` at the end of a loop's body,
//! after a statement in the test's own block, in an `if` that has an `else`. A macro's
//! arguments are the test's own code when they parse as comma-separated expressions
//! (`assert!`, `format!`); a `return` among the tokens of any other macro is refused, since what
//! it ends is the macro's to say. A refusal is named with its place, and rewritten into one of
//! the shapes or gated.
//!
//! # The register
//!
//! A G1–G3 site stands only as a row of `docs/plans/EARLY-RETURN-FIXTURES.tsv` — file, callee,
//! hosts, reason — keyed by (file, callee), so a fixture called by many tests of one file is one
//! row. The rows are an equality with the sites: a site no row names is red, and a row that names
//! no site is stale and red. The hosts column says where the road is taken, in exactly one of
//! five spellings:
//!
//! * `other-unix`: a Unix that is not macOS;
//! * `other-platform`: a host that is neither Windows nor a Unix;
//! * `environment: <the fact>`: a machine of any platform that lacks something (a software
//!   adapter, a logon session) — a fact of the environment, not of a platform;
//! * `parent-half`: every host, in the suite's own run, where the road is the test's parent
//!   half: the call it hangs on started the test again alone in a child process and asserted
//!   that child's verdict, and the child takes the other branch;
//! * `child-half`: every host, in the suite's own run, where the test is the entry of a child
//!   another test starts (its mode read from the environment, the arguments or standard input
//!   before the road, which G4 does not see), and the starting test asserts what the child did.
//!
//! None names Windows or macOS, the hosts every suite of the workspace runs on: a road taken
//! there is this guard's defect, never a row.
//!
//! # What it does not see
//!
//! * **A road out inside a helper.** `fn armed(tag) -> Option<Install>` returns `None`, and its
//!   caller's G1 is judged by `armed`; but a helper that itself returns early and silently from
//!   a test's work (`fn cell(…) { let Some(x) = … else { return; }; … }`, the test only calling
//!   it) is no test's own body.
//! * **What an assertion before the road checks.** Any one written before it accepts the road,
//!   even one in a branch the host where the road is taken never runs, and whether what it
//!   checks is enough is review's. An assertion is known by its name only: a helper that
//!   asserts under another name (`nothing_moved(&install)`) is no assertion here.
//! * **A road's polarity.** A row names the fixture a road hangs on, not which answer takes it:
//!   `if !ready() { return; }` and `if ready() { return; }` in one file are one row, judged by
//!   `ready`, whose hosts column says where the one the file has is taken.
//! * **A test that asserts nothing at all**, with no road out: not this grammar's.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::{
    DiskScope, Index, ItemRecord, Package, Pattern, Search, Span, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, View, Workspace, is_vendored, needle, normalized, report, universes,
};
use proc_macro2::TokenTree;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Block, Expr, Stmt, Token};

/// The register, from the workspace root.
const LIST: &str = "docs/plans/EARLY-RETURN-FIXTURES.tsv";

/// The register's column header.
const HEADER: &str = "file\tcallee\thosts\treason";

/// The hosts every suite of the workspace runs on: a register row may not name them.
const SUITE_HOSTS: [&str; 2] = ["windows", "macos"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are declared out.
pub(crate) fn universe_for(package: &Package) -> Universe {
    universes::first_party_whole(package, Vendor::Excluded).expect("a package of this workspace")
}

/// Every first-party package's index: the one this process holds for each package's
/// [`universes::first_party_whole`], shared with every other guard that asks for it.
pub(crate) fn workspace_indices() -> Vec<Arc<Index>> {
    let workspace = Workspace::read(&normalized(&workspace_root())).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            Index::shared(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
        })
        .collect()
}

/// One road out in a shape the grammar reads and the register must name: the file it stands in,
/// what it is judged by, and where.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    file: String,
    callee: String,
    line: usize,
    test: String,
    shape: &'static str,
}

/// What a scan found: the sites the register must name, the accepted child halves (G4), and
/// the roads out in no shape, each as `file:line test`.
#[derive(Debug, Default)]
struct Found {
    sites: BTreeSet<Site>,
    children: BTreeSet<String>,
    refused: BTreeSet<String>,
    /// How many tests were read.
    tests: usize,
}

/// Whether `ret` ends the test as passed: no value, `()`, or `Ok(…)`.
fn is_road_out(ret: &syn::ExprReturn) -> bool {
    match ret.expr.as_deref() {
        None => true,
        Some(Expr::Tuple(unit)) => unit.elems.is_empty(),
        Some(Expr::Call(call)) => {
            matches!(&*call.func, Expr::Path(path) if path.path.is_ident("Ok"))
        }
        Some(_) => false,
    }
}

/// The road out `expr` is, or the one the block it is ends in.
fn road_out_of(expr: &Expr) -> Option<&syn::ExprReturn> {
    match expr {
        Expr::Return(ret) if is_road_out(ret) => Some(ret),
        Expr::Block(block) => road_out_ending(&block.block),
        _ => None,
    }
}

/// The road out `block`'s last statement is.
fn road_out_ending(block: &Block) -> Option<&syn::ExprReturn> {
    match block.stmts.last()? {
        Stmt::Expr(expr, _) => road_out_of(expr),
        _ => None,
    }
}

/// A path by its last two segments.
fn last_two(path: &syn::Path) -> String {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments[segments.len().saturating_sub(2)..].join("::")
}

/// The callee of `expr`, as the module header defines it; `None` where it makes no call.
fn callee(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Call(call) => match &*call.func {
            Expr::Path(path) => Some(last_two(&path.path)),
            other => callee(other),
        },
        Expr::MethodCall(call) => callee(&call.receiver).or_else(|| Some(call.method.to_string())),
        Expr::Reference(inner) => callee(&inner.expr),
        Expr::Unary(inner) => callee(&inner.expr),
        Expr::Paren(inner) => callee(&inner.expr),
        Expr::Try(inner) => callee(&inner.expr),
        Expr::Await(inner) => callee(&inner.base),
        Expr::Field(inner) => callee(&inner.base),
        Expr::Let(inner) => callee(&inner.expr),
        _ => None,
    }
}

/// Whether `expr` calls `env::var` or `env::var_os` anywhere in it.
fn reads_the_environment(expr: &Expr) -> bool {
    struct Reads(bool);
    impl<'ast> Visit<'ast> for Reads {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let Expr::Path(path) = &*call.func {
                let segments: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                if segments.ends_with(&["env".to_owned(), "var".to_owned()])
                    || segments.ends_with(&["env".to_owned(), "var_os".to_owned()])
                {
                    self.0 = true;
                }
            }
            visit::visit_expr_call(self, call);
        }
    }
    let mut reads = Reads(false);
    reads.visit_expr(expr);
    reads.0
}

/// What a road a shape holds is judged by: the child half's dispatch (G4), or a callee.
#[derive(Debug)]
enum Judged {
    Child,
    By(String),
}

/// Where a road out stands in one test's own body: every road out by its offset in the test's
/// text, and those a shape holds, each with its shape and what it is judged by.
struct Roads<'t> {
    /// The test's whole text, which every offset is into.
    text: &'t str,
    out: Vec<usize>,
    held: Vec<(usize, &'static str, Judged)>,
    /// Where an assertion is written in the body, by its offset.
    asserts: Vec<usize>,
}

/// Whether a macro or a call by this name is an assertion: `assert…` (`assert!`,
/// `assert_eq!`, a helper `assert_window_spent_by`), `panic!`, `unreachable!`, or a method that
/// fails the test on the other answer (`expect_err`, `unwrap_err`).
fn asserts(name: &str) -> bool {
    name.starts_with("assert")
        || matches!(name, "panic" | "unreachable" | "expect_err" | "unwrap_err")
}

fn offset(spanned: &impl Spanned) -> usize {
    spanned.span().byte_range().start
}

impl Roads<'_> {
    /// The road at `ret`, held by `shape` and judged by `expr`.
    fn hold(&mut self, ret: &syn::ExprReturn, shape: &'static str, expr: &Expr) {
        let judged = if reads_the_environment(expr) {
            Judged::Child
        } else {
            Judged::By(callee(expr).unwrap_or_else(|| {
                self.text[expr.span().byte_range()]
                    .split_whitespace()
                    .collect()
            }))
        };
        self.held.push((offset(ret), shape, judged));
    }
}

impl<'ast> Visit<'ast> for Roads<'_> {
    fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}

    fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}

    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_expr_return(&mut self, ret: &'ast syn::ExprReturn) {
        if is_road_out(ret) {
            self.out.push(offset(ret));
        }
        visit::visit_expr_return(self, ret);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = &local.init
            && let Some((_, diverge)) = &init.diverge
            && let Some(ret) = road_out_of(diverge)
        {
            self.hold(ret, "G1", &init.expr);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_if(&mut self, branch: &'ast syn::ExprIf) {
        if branch.else_branch.is_none()
            && let Some(ret) = road_out_ending(&branch.then_branch)
        {
            self.hold(ret, "G2", &branch.cond);
        }
        visit::visit_expr_if(self, branch);
    }

    fn visit_expr_match(&mut self, choice: &'ast syn::ExprMatch) {
        for arm in &choice.arms {
            if let Some(ret) = road_out_of(&arm.body) {
                self.hold(ret, "G3", &choice.expr);
            }
        }
        visit::visit_expr_match(self, choice);
    }

    /// A macro's arguments are read as the test's own code when they are comma-separated
    /// expressions (`assert!`, `format!`); in any other macro a `return` token is a road out
    /// in no shape, since what it ends is the macro's to say.
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*call.func
            && path
                .path
                .segments
                .last()
                .is_some_and(|segment| asserts(&segment.ident.to_string()))
        {
            self.asserts.push(offset(call));
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if asserts(&call.method.to_string()) {
            self.asserts.push(offset(&call.method));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|segment| asserts(&segment.ident.to_string()))
        {
            self.asserts.push(offset(&mac.path));
        }
        match mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            Ok(arguments) => {
                for argument in &arguments {
                    self.visit_expr(argument);
                }
            }
            Err(_) => {
                let mut tokens: Vec<TokenTree> = mac.tokens.clone().into_iter().collect();
                while let Some(token) = tokens.pop() {
                    match token {
                        TokenTree::Ident(word) if word == "return" => {
                            self.out.push(word.span().byte_range().start);
                        }
                        TokenTree::Group(group) => tokens.extend(group.stream()),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// `path` relative to `root`, with forward slashes.
fn relative(path: &Path, root: &Path) -> String {
    normalized(path)
        .strip_prefix(normalized(root))
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every `#[test]` attribute of `index`, sorted.
fn test_attributes(index: &Index) -> Vec<Span> {
    let mut found = index
        .search(&Search::new(
            needle!(Pattern::text("#[test]")),
            View::CodeKeepingLiterals,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// Whether `item` is a test: a callable carrying `#[test]`.
fn is_test(attributes: &[Span], item: &ItemRecord) -> bool {
    let declaration = item.declaration();
    let first = attributes.partition_point(|span| span.start() < declaration.start());
    item.body().is_some()
        && item.kind().is_callable()
        && attributes[first..]
            .iter()
            .take_while(|span| span.start() < declaration.end())
            .any(|span| span.within(declaration))
}

/// Every road out of every test of `index`, judged; file paths relative to `root`.
fn scan(index: &Index, root: &Path, found: &mut Found) {
    let attributes = test_attributes(index);
    for item in index.items() {
        if !is_test(&attributes, item) {
            continue;
        }
        found.tests += 1;
        let whole = item.whole();
        let text = index.text(whole);
        let names: Vec<String> = item
            .identities()
            .map(|identity| format!("{}::{}", identity.module_path, identity.name))
            .collect();
        let test = names.join(" ");
        let place = |at: usize| {
            let location = index
                .locate(whole.start() + at)
                .expect("an offset of a test stands in one of the index's files");
            (relative(location.file, root), location.line)
        };
        let parsed = match syn::parse_str::<syn::ItemFn>(text) {
            Ok(parsed) => parsed,
            Err(error) => {
                let (file, line) = place(0);
                found.refused.insert(format!(
                    "{file}:{line} {test}: its text does not parse ({error}), so its roads out \
                     cannot be read"
                ));
                continue;
            }
        };
        let mut roads = Roads {
            text,
            out: Vec::new(),
            held: Vec::new(),
            asserts: Vec::new(),
        };
        roads.visit_block(&parsed.block);
        // A road with an assertion written before it is no road of this class: the test has
        // checked something on the way.
        let first_assert = roads.asserts.iter().copied().min().unwrap_or(usize::MAX);
        roads.out.retain(|at| *at < first_assert);
        roads.held.retain(|(at, _, _)| *at < first_assert);
        let held: Vec<usize> = roads.held.iter().map(|(at, _, _)| *at).collect();
        for (at, shape, judged) in roads.held.drain(..) {
            let (file, line) = place(at);
            match judged {
                Judged::Child => {
                    found.children.insert(format!("{file}:{line} {test}"));
                }
                Judged::By(callee) => {
                    found.sites.insert(Site {
                        file,
                        callee,
                        line,
                        test: test.clone(),
                        shape,
                    });
                }
            }
        }
        for at in roads.out.iter().filter(|at| !held.contains(at)) {
            let (file, line) = place(*at);
            found.refused.insert(format!("{file}:{line} {test}"));
        }
    }
}

/// Every test of every first-party package, judged.
fn workspace_found() -> Found {
    let root = normalized(&workspace_root());
    let mut found = Found::default();
    for index in workspace_indices() {
        scan(&index, &root, &mut found);
    }
    found
}

/// The register's rows, as (file, callee).
fn register() -> BTreeSet<(String, String)> {
    rows_of(&std::fs::read_to_string(workspace_root().join(LIST)).expect("the register"))
}

/// Whether a hosts field is one of the three spellings, naming neither suite host.
fn hosts_are_spelled(hosts: &str) -> bool {
    let named = hosts
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| SUITE_HOSTS.contains(&word.to_lowercase().as_str()));
    let spelled = matches!(
        hosts,
        "other-unix" | "other-platform" | "parent-half" | "child-half"
    ) || hosts
        .strip_prefix("environment: ")
        .is_some_and(|fact| !fact.trim().is_empty());
    spelled && !named
}

/// The rows of a register's text, refusing a malformed line, a hosts field in no spelling (or
/// naming a host the suites run on), and a row listed twice, by number.
fn rows_of(text: &str) -> BTreeSet<(String, String)> {
    let mut rows = BTreeSet::new();
    let mut header = false;
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            assert_eq!(line, HEADER, "{LIST}:{}: the header", number + 1);
            header = true;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [file, callee, hosts, reason] = fields[..] else {
            panic!("{LIST}:{}: four tab-separated fields: {line}", number + 1);
        };
        assert!(
            hosts_are_spelled(hosts),
            "{LIST}:{}: hosts {hosts:?} is not `other-unix`, `other-platform`, `parent-half`, \
             `child-half` or `environment: <fact>`, or names a host every suite runs on ({}): \
             a road taken there is gated with `#[cfg]` (CONVENTIONS H1 rule 2), never a row",
            number + 1,
            SUITE_HOSTS.join(", ")
        );
        assert!(
            !reason.trim().is_empty(),
            "{LIST}:{}: a row says why",
            number + 1
        );
        let fresh = rows.insert((file.to_owned(), callee.to_owned()));
        assert!(fresh, "{LIST}:{}: listed twice: {line}", number + 1);
    }
    assert!(header, "{LIST} has its header");
    rows
}

/// The sites no row names, and the rows that name no site.
fn judged(sites: &BTreeSet<Site>, rows: &BTreeSet<(String, String)>) -> (Vec<String>, Vec<String>) {
    let keys: BTreeSet<(String, String)> = sites
        .iter()
        .map(|site| (site.file.clone(), site.callee.clone()))
        .collect();
    let unlisted = sites
        .iter()
        .filter(|site| !rows.contains(&(site.file.clone(), site.callee.clone())))
        .map(|site| {
            format!(
                "{}:{} {} ({}, `{}`)",
                site.file, site.line, site.test, site.shape, site.callee
            )
        })
        .collect();
    let stale = rows
        .difference(&keys)
        .map(|(file, callee)| format!("{file}\t{callee}"))
        .collect();
    (unlisted, stale)
}

/// RED (0.4.10 plan row 12) — **no test of the workspace returns before it asserts anything,
/// except in a shape the grammar reads and at a fixture the register names with its reason;
/// and every row still names one.**
///
/// MUTATION: in `update_apply_windows::tests::a_recovery_leaves_handoff_to_an_election_still_in_flight`,
/// put back `let Some(_root) = TestCa::new().ok() else { return; };` as its first statement,
/// and this names that line under G1, `TestCa::new`.
#[test]
fn no_test_returns_before_it_asserts_but_at_a_registered_fixture() {
    let found = workspace_found();
    assert!(
        found.tests > 1000,
        "the scan read {} tests; the workspace has thousands",
        found.tests
    );
    assert!(
        found.refused.is_empty(),
        "these tests take a road out (a `return` that ends them passed) in no shape this guard \
         reads — at the end of a loop, after a statement, in an `if` with an `else`, among a \
         macro's tokens; write it as the else of a `let`, an `if` with no `else` or a `match` \
         arm, or gate the test with `#[cfg]`:\n  {}",
        found
            .refused
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let (unlisted, stale) = judged(&found.sites, &register());
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "these tests return before they assert anything where what they hang on answers nothing: \
         gate the test with `#[cfg]` on the hosts it needs (CONVENTIONS H1 rule 2), make the \
         fixture answer on every host the suite runs on, or add a row to {LIST} saying where \
         and why:\n  {}\n\
         on {LIST} and naming no road out any more (remove the row):\n  {}",
        unlisted.join("\n  "),
        stale.join("\n  ")
    );
}

/// The `early_returns` fixture, as an index, with its root.
fn fixture() -> (Index, PathBuf) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("early_returns");
    let universe = Universe::declare(
        "the early_returns fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "early_returns".to_owned(),
                kind: TargetKind::Library,
                name: "early_returns".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    (index, directory)
}

/// RED (0.4.10 plan row 12) — **each shape is read and judged by its callee, the child half
/// is accepted, a road out in no shape is refused, and what is not the test's own road is not
/// read.**
///
/// The fixture holds G1 over a fixture answering `Option` (and one in a test returning
/// `Result`), G2 over a `cfg!`, G3 over a call, the child half (G4), a road out at the end of a
/// loop, one in an `if` with an `else` and one among a macro's tokens (refused), and a `return`
/// in a closure (in a body and in an `assert!`'s argument), in a nested function, after an
/// assertion, a `return Err(…)` and a helper's own `return` (none read).
///
/// MUTATION: descend into closures in `Roads` and `a_return_in_a_closure_is_not_the_tests` is
/// refused; keep the roads after the first assertion and
/// `a_road_after_an_assertion_has_checked_something` is a site; drop G4's check and
/// `a_child_half_reads_its_part` is a site; drop the `else_branch` check from G2 and
/// `an_if_with_an_else` is a site instead of refused.
#[test]
fn the_grammar_reads_each_shape_and_refuses_a_road_in_none() {
    let (index, directory) = fixture();
    let mut found = Found::default();
    scan(&index, &directory, &mut found);
    let sites: Vec<String> = found
        .sites
        .iter()
        .map(|site| format!("{} {} {}", site.test, site.shape, site.callee))
        .collect();
    assert_eq!(
        sites,
        [
            "crate::tests::a_fixture_answering_none G1 TestRoot::new",
            "crate::tests::a_fixture_answering_none_in_a_result_test G1 TestRoot::new",
            "crate::tests::a_host_by_its_cfg G2 cfg!(not(windows))",
            "crate::tests::an_endpoint_by_a_match G3 endpoint",
        ],
        "{found:#?}"
    );
    let strip = |entries: &BTreeSet<String>| -> Vec<String> {
        let mut tests: Vec<String> = entries
            .iter()
            .map(|entry| {
                entry
                    .split_once(' ')
                    .map_or(entry.clone(), |(_, test)| test.to_owned())
            })
            .collect();
        tests.sort();
        tests
    };
    assert_eq!(
        strip(&found.children),
        ["crate::tests::a_child_half_reads_its_part"],
        "{found:#?}"
    );
    assert_eq!(
        strip(&found.refused),
        [
            "crate::tests::a_road_among_a_macros_tokens",
            "crate::tests::a_road_at_the_end_of_a_loop",
            "crate::tests::an_if_with_an_else",
        ],
        "{found:#?}"
    );
    assert_eq!(found.tests, 12, "{found:#?}");
}

/// RED (0.4.10 plan row 12) — **the register is an equality with the sites**: a site no row
/// names is unlisted, and a row naming no site is stale.
///
/// MUTATION: answer no stale rows from `judged` and the stale row is not named.
#[test]
fn a_row_naming_no_road_is_stale_and_an_unnamed_road_is_unlisted() {
    let (index, directory) = fixture();
    let mut found = Found::default();
    scan(&index, &directory, &mut found);
    let rows = rows_of(&format!(
        "# a register\n{HEADER}\n\
         lib.rs\tTestRoot::new\tenvironment: no test root\tthe fixture's root\n\
         lib.rs\tcfg!(not(windows))\tother-platform\tthe host check\n\
         lib.rs\tgone::away\tother-unix\ta fixture no test calls any more\n"
    ));
    let (unlisted, stale) = judged(&found.sites, &rows);
    assert_eq!(
        unlisted
            .iter()
            .map(|entry| entry
                .rsplit_once(" (")
                .map_or(entry.as_str(), |(_, shape)| shape))
            .collect::<Vec<_>>(),
        ["G3, `endpoint`)"],
        "{unlisted:#?}"
    );
    assert_eq!(stale, ["lib.rs\tgone::away"]);
}

/// RED (0.4.10 plan row 12) — **a row may not name a host every suite runs on**: a road taken
/// on Windows or macOS is gated, never registered.
///
/// MUTATION: drop the suite-host check from `hosts_are_spelled` and an `environment:` row
/// naming macOS is taken.
#[test]
#[should_panic(expected = "3: hosts \"environment: macos without a root\"")]
fn a_row_naming_a_suite_host_is_refused() {
    let _ = rows_of(&format!(
        "{HEADER}\n\
         a.rs\tf\tother-unix\tone reason\n\
         a.rs\tg\tenvironment: macos without a root\tanother\n"
    ));
}
