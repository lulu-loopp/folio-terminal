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
//! **A road out** is a `return` that ends the test as passed: `return;`, `return ();` or
//! `return Ok(…);`. Any other `return` hands back a value whose own verdict the test is —
//! `return Err(…)` ends red — and is not a road out.
//!
//! **An assertion** is a macro or a call named `assert…` (`assert!`, `assert_eq!`, a helper
//! `assert_window_spent_by`), `panic!`, `unreachable!`, or a method `expect_err` or
//! `unwrap_err`, written anywhere in the test's own body.
//!
//! # The grammar: every road out is dropped (R), stands in one of four shapes, or is refused
//!
//! * **R — a road after an assertion is not this class, and is dropped.** A road out written
//!   after the test's first assertion (by its place in the text) ends a test that has checked
//!   something on the way: a case table's later row after the first held, the child half after
//!   the child's own assertions, the other host's half that asserts its refusal
//!   (`if host != Windows { assert!(files.is_empty()); return; }`). The assertion counts
//!   wherever it stands before the road, inside the road's own `else` block too, and what it
//!   checks is not judged here (below). The dropped roads are measured, not registered: the
//!   row's report names each one with the assertion before it, and review reads them; a test
//!   whose only assertion before its road is the host itself is this guard's class and is gated
//!   with `#[cfg]` (0.4.10 plan row 12 gated three).
//! * **G1 — the else of a `let`.** `let PAT = EXPR else { …; return; };`, the else block
//!   ending in the road out. Judged by `EXPR`'s callee.
//! * **G2 — an `if` with no `else`.** `if COND { …; return; }`, the block ending in the road
//!   out; `if let PAT = EXPR` is judged by `EXPR`. Judged by `COND`'s callee. The last arm of an
//!   `else if` chain is such an `if` (the inner `if` has no `else`), so a road there is held as
//!   G2; a road in a branch that has an `else` after it is refused.
//! * **G3 — a `match` arm.** `match EXPR { … ARM => return, … }`, the arm a road out or a
//!   block ending in one. Judged by `EXPR`'s callee.
//! * **G4 — the child half of a test.** A G1, G2 or G3 whose `EXPR` or `COND` reads one or more
//!   variables with `env::var` or `env::var_os`, **each of them a variable the same file sets on
//!   a started command** — `.env(NAME, …)`, its first argument spelled as the read's (whitespace
//!   out) — is the child half: the test is a copy of its own binary, started by its parent half
//!   with that variable, and the parent asserts. Accepted, with no row. A read of a variable
//!   the file never sets on a command (an opt-in, a variable the person sets) is no child half:
//!   the road is a G1–G3 site judged by its callee and keyed on its test (`env::var_os` names
//!   the variable, not a fixture), and needs a row (`child-half` when another file's command
//!   starts the child, otherwise where it is taken).
//!
//! **The callee** of an expression is the path of the call it is, by its last two segments
//! (`Install::new`, `launch_pipe::endpoint_for`), read through `&`, `!`, `*`, parentheses, `?`,
//! `.await`, a field and a method's receiver (`TestCa::new().ok()` is `TestCa::new`); a method
//! called on no call is its name (`install.home.journal().exists()` is `journal`). An
//! expression that is no call in that reading — a variable, a comparison, a `cfg!`, a `&&`
//! chain — is judged by its whole text with the whitespace out
//! (`crate::host_platform()==crate::HostPlatform::OtherUnix`, `cfg!(not(any(windows,unix)))`).
//!
//! **Every other road out is refused, never silent**: a `return;` at the end of a loop's body,
//! after a statement in the test's own block, in a branch with an `else` after it. A macro's
//! arguments are the test's own code when they parse as comma-separated expressions
//! (`assert!`, `format!`); a `return` among the tokens of any other macro is refused, since what
//! it ends is the macro's to say. A refusal is named with its place, and rewritten into
//! one of the shapes or gated.
//!
//! # The register
//!
//! A G1–G3 site stands only as a row of `docs/plans/EARLY-RETURN-FIXTURES.tsv` — file, test,
//! callee, hosts, reason. **A callee that is a function named by its path** (`Install::new`,
//! `on_the_software_adapter`) keys the row on (file, callee), the test column `*`, so a fixture
//! called by many tests of one file is one row. **Any other key** — a method read on no call
//! (`iter`), a variable (`powershell`), a whole condition (`cfg!(not(any(windows,unix)))`), a
//! read of the environment G4 does not hold —
//! keys it on (file, test, callee), the test by its function's name: a name that common would
//! otherwise cover another test's road it has nothing to do with. The rows are an equality with
//! the sites: a site no row names is red, and a row that names no site is stale and red. The
//! hosts column says where the road is taken, in exactly one of five spellings:
//!
//! * `other-unix`: a Unix that is not macOS;
//! * `other-platform`: a host that is neither Windows nor a Unix;
//! * `environment: <the fact>`: a machine of any platform that lacks something (a software
//!   adapter, a logon session) — a fact of the environment, not of a platform;
//! * `parent-half`: every host, in the suite's own run, where the road is the test's parent
//!   half: the call it hangs on started the test again alone in a child process and asserted
//!   that child's verdict, and the child takes the other branch;
//! * `child-half`: every host, in the suite's own run, where the test is the entry of a child
//!   another test starts (its mode read from the arguments or standard input, or from a
//!   variable its own file never sets, before the road: what G4 does not hold), and the
//!   starting test asserts what the child did.
//!
//! None names Windows or macOS, the hosts every suite of the workspace runs on: a road taken
//! there is this guard's defect, never a row. The reason column is free, and says the whole
//! fact where the spelling cannot (a fixture that answers nothing on a whole platform the
//! suite runs on stands in the reason, with the plan row that decides it).
//!
//! # What it does not see
//!
//! * **A road out inside a helper.** `fn armed(tag) -> Option<Install>` returns `None`, and its
//!   caller's G1 is judged by `armed`; but a helper that itself returns early and silently from
//!   a test's work (`fn cell(…) { let Some(x) = … else { return; }; … }`, the test only calling
//!   it) is no test's own body. Four of row 12's 84 were this shape (the
//!   `mark_unwritable_*_has_one_opener` tests, through `mark_unwritable_cell`): this guard could
//!   not have named them. Following a test's helpers one level is 0.4.10 plan row 96
//!   (T-EARLY-RETURN-GUARD-FOLLOWS-HELPERS).
//! * **A `continue` or `break` past every assertion.** `for name in names { let Some(p) =
//!   find(name) else { continue; }; assert!(…); }` asserts nothing when every name answers
//!   `None`, and a labelled block's `break` can do the same; neither ends the test, so neither
//!   is read here (row 96 too).
//! * **What an assertion before the road checks** (R). Any one written before it drops the
//!   road, even one in a branch the host where the road is taken never runs, or one that only
//!   names the host, and whether what it checks is enough is review's. An assertion is known by
//!   its name only: a helper that asserts under another name (`nothing_moved(&install)`) is no
//!   assertion here.
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
const HEADER: &str = "file\ttest\tcallee\thosts\treason";

/// The test column of a row keyed on its file alone: a function named by its path.
const FILE_WIDE: &str = "*";

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
/// the test column of its row (`*`, or the test function's name), what it is judged by, and
/// where.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    file: String,
    key_test: String,
    callee: String,
    line: usize,
    test: String,
    shape: &'static str,
}

impl Site {
    /// The register row's key this site needs: (file, test column, callee).
    fn key(&self) -> (String, String, String) {
        (
            self.file.clone(),
            self.key_test.clone(),
            self.callee.clone(),
        )
    }
}

/// What a scan found: the sites the register must name, the accepted child halves (G4), the
/// roads out in no shape, each as `file:line test`, and the roads R dropped, each with the
/// assertion written before it.
#[derive(Debug, Default)]
struct Found {
    sites: BTreeSet<Site>,
    children: BTreeSet<String>,
    refused: BTreeSet<String>,
    dropped: BTreeSet<String>,
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

/// The callee of `expr`, as the module header defines it, and whether it is a function named
/// by its path (`true`, a row keyed file-wide) or a method read on no call (`false`, a row
/// keyed on its test); `None` where it makes no call.
fn callee(expr: &Expr) -> Option<(String, bool)> {
    match expr {
        Expr::Call(call) => match &*call.func {
            Expr::Path(path) => Some((last_two(&path.path), true)),
            other => callee(other),
        },
        Expr::MethodCall(call) => {
            callee(&call.receiver).or_else(|| Some((call.method.to_string(), false)))
        }
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

/// `text` with its whitespace out.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect()
}

/// The variables `expr` reads with `env::var` or `env::var_os`, each as its first argument's
/// text in `text` with the whitespace out; empty where it reads none.
fn environment_reads(expr: &Expr, text: &str) -> Vec<String> {
    struct Reads<'t> {
        text: &'t str,
        names: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Reads<'_> {
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
                    let name = call.args.first().map_or_else(String::new, |name| {
                        squeezed(&self.text[name.span().byte_range()])
                    });
                    self.names.push(name);
                }
            }
            visit::visit_expr_call(self, call);
        }
    }
    let mut reads = Reads {
        text,
        names: Vec::new(),
    };
    reads.visit_expr(expr);
    reads.names
}

/// What a road a shape holds is judged by: the child half's dispatch (G4), or a callee or a
/// condition with whether its row is keyed file-wide.
#[derive(Debug)]
enum Judged {
    Child,
    By { key: String, file_wide: bool },
}

/// Where a road out stands in one test's own body: every road out by its offset in the test's
/// text, and those a shape holds, each with its shape and what it is judged by.
struct Roads<'t> {
    /// The test's whole text, which every offset is into.
    text: &'t str,
    /// The test's file with its whitespace out, where G4 looks for `.env(NAME,`.
    file: &'t str,
    out: Vec<usize>,
    held: Vec<(usize, &'static str, Judged)>,
    /// Where an assertion is written in the body, by its offset, with its name.
    asserts: Vec<(usize, String)>,
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
    /// The road at `ret`, held by `shape` and judged by `expr`: the child half (G4) when every
    /// variable it reads is one this file sets on a started command, else its callee, else its
    /// whole text; keyed on its test where it reads the environment, since `env::var_os` names
    /// the variable, not a fixture.
    fn hold(&mut self, ret: &syn::ExprReturn, shape: &'static str, expr: &Expr) {
        let reads = environment_reads(expr, self.text);
        let judged = if !reads.is_empty()
            && reads
                .iter()
                .all(|name| self.file.contains(&format!(".env({name},")))
        {
            Judged::Child
        } else if let Some((key, file_wide)) = callee(expr) {
            Judged::By {
                key,
                file_wide: file_wide && reads.is_empty(),
            }
        } else {
            Judged::By {
                key: squeezed(&self.text[expr.span().byte_range()]),
                file_wide: false,
            }
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
            && let Some(segment) = path.path.segments.last()
            && asserts(&segment.ident.to_string())
        {
            self.asserts.push((offset(call), segment.ident.to_string()));
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if asserts(&call.method.to_string()) {
            self.asserts
                .push((offset(&call.method), call.method.to_string()));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(segment) = mac.path.segments.last()
            && asserts(&segment.ident.to_string())
        {
            self.asserts
                .push((offset(&mac.path), format!("{}!", segment.ident)));
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
    let mut files: std::collections::BTreeMap<PathBuf, String> = std::collections::BTreeMap::new();
    for item in index.items() {
        if !is_test(&attributes, item) {
            continue;
        }
        found.tests += 1;
        let record = index.file_of(item);
        let file: &str = files
            .entry(record.path().to_path_buf())
            .or_insert_with(|| squeezed(index.text(record.span())));
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
        let parsed = syn::parse_str::<syn::ItemFn>(text)
            .expect("the index parsed this function's file, so its own text parses");
        let mut roads = Roads {
            text,
            file,
            out: Vec::new(),
            held: Vec::new(),
            asserts: Vec::new(),
        };
        roads.visit_block(&parsed.block);
        // R: a road with an assertion written before it is no road of this class (the test has
        // checked something on the way); it is dropped, and named with that assertion.
        if let Some((first_assert, name)) = roads.asserts.iter().min().cloned() {
            let (_, assert_line) = place(first_assert);
            for at in roads.out.iter().filter(|at| **at > first_assert) {
                let (file, line) = place(*at);
                found.dropped.insert(format!(
                    "{file}:{line} {test} (after `{name}` at line {assert_line})"
                ));
            }
            roads.out.retain(|at| *at < first_assert);
            roads.held.retain(|(at, _, _)| *at < first_assert);
        }
        let held: Vec<usize> = roads.held.iter().map(|(at, _, _)| *at).collect();
        for (at, shape, judged) in roads.held.drain(..) {
            let (file, line) = place(at);
            match judged {
                Judged::Child => {
                    found.children.insert(format!("{file}:{line} {test}"));
                }
                Judged::By { key, file_wide } => {
                    found.sites.insert(Site {
                        file,
                        key_test: if file_wide {
                            FILE_WIDE.to_owned()
                        } else {
                            parsed.sig.ident.to_string()
                        },
                        callee: key,
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

/// The register's rows, as (file, test column, callee).
fn register() -> BTreeSet<(String, String, String)> {
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
fn rows_of(text: &str) -> BTreeSet<(String, String, String)> {
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
        let [file, test, callee, hosts, reason] = fields[..] else {
            panic!("{LIST}:{}: five tab-separated fields: {line}", number + 1);
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
        let fresh = rows.insert((file.to_owned(), test.to_owned(), callee.to_owned()));
        assert!(fresh, "{LIST}:{}: listed twice: {line}", number + 1);
    }
    assert!(header, "{LIST} has its header");
    rows
}

/// The sites no row names, each with the key its row needs, and the rows that name no site.
fn judged(
    sites: &BTreeSet<Site>,
    rows: &BTreeSet<(String, String, String)>,
) -> (Vec<String>, Vec<String>) {
    let keys: BTreeSet<(String, String, String)> = sites.iter().map(Site::key).collect();
    let unlisted = sites
        .iter()
        .filter(|site| !rows.contains(&site.key()))
        .map(|site| {
            format!(
                "{}:{} {} ({}, `{}`; row key `{}\t{}\t{}`)",
                site.file,
                site.line,
                site.test,
                site.shape,
                site.callee,
                site.file,
                site.key_test,
                site.callee
            )
        })
        .collect();
    let stale = rows
        .difference(&keys)
        .map(|(file, test, callee)| format!("{file}\t{test}\t{callee}"))
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
/// is accepted only over a variable its file sets on a command, a road out in no shape is
/// refused, a road after an assertion is dropped and named, and what is not the test's own road
/// is not read.**
///
/// The fixture holds, for each line of the module doc: G1 over a fixture answering `Option`
/// (with `return;`, `return ();` and, in a test returning `Result`, `return Ok(())`), G2 over a
/// `cfg!`, an `if let` and the last arm of an `else if` chain, G3 over a call; the callee read
/// through `!`, `&`, `?` and a field, a method on no call and a variable (both keyed on their
/// test), the child half by an `if`, a `let` and a `match` (G4) and an opt-in read no command of
/// the file sets (a site); a road out at the end of a loop, after a statement, in a branch with
/// an `else` and among a macro's tokens (refused); a road after `assert!`, `panic!`,
/// `expect_err` and an `assert…` helper (dropped); and a `return` in a closure (in a body and in
/// an `assert!`'s argument), in an `async` block, in a nested function, a `return Err(…)` and a
/// helper's own `return` (none read).
///
/// MUTATION: descend into closures in `Roads` and `a_return_in_a_closure_is_not_the_tests` is
/// refused; keep the roads after the first assertion and
/// `a_road_after_an_assertion_has_checked_something` is a site; accept any read of the
/// environment in `hold` and `an_opt_in_is_no_child_half` is a child half; drop the
/// `else_branch` check from G2 and `an_if_with_an_else` is a site instead of refused; key every
/// callee file-wide and the two bare-name sites name `*`.
#[test]
fn the_grammar_reads_each_shape_and_refuses_a_road_in_none() {
    let (index, directory) = fixture();
    let mut found = Found::default();
    scan(&index, &directory, &mut found);
    let sites: Vec<String> = found
        .sites
        .iter()
        .map(|site| {
            format!(
                "{} {} {} [{}]",
                site.test, site.shape, site.callee, site.key_test
            )
        })
        .collect();
    assert_eq!(
        sites,
        [
            "crate::tests::a_callee_through_a_field G1 Settings::load [*]",
            "crate::tests::an_if_let_is_judged_by_its_expression G2 TestRoot::new [*]",
            "crate::tests::a_callee_through_a_question_mark G1 TestRoot::new [*]",
            "crate::tests::a_fixture_answering_none G1 TestRoot::new [*]",
            "crate::tests::a_fixture_answering_none_in_a_result_test G1 TestRoot::new [*]",
            "crate::tests::a_unit_return_is_a_road G1 endpoint [*]",
            "crate::tests::a_callee_through_a_reference G1 endpoint [*]",
            "crate::tests::an_endpoint_by_a_match G3 endpoint [*]",
            "crate::tests::a_callee_through_a_not G2 ready [*]",
            "crate::tests::an_else_if_last_arm_is_held_as_g2 G2 ready [*]",
            "crate::tests::a_bare_name_in_another_test G1 iter [a_bare_name_in_another_test]",
            "crate::tests::a_bare_name_in_one_test G1 iter [a_bare_name_in_one_test]",
            "crate::tests::a_host_by_its_cfg G2 cfg!(not(windows)) [a_host_by_its_cfg]",
            "crate::tests::a_variable_is_judged_by_its_name G1 found [a_variable_is_judged_by_its_name]",
            "crate::tests::an_opt_in_is_no_child_half G2 env::var_os [an_opt_in_is_no_child_half]",
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
        [
            "crate::tests::a_child_half_by_a_let",
            "crate::tests::a_child_half_by_a_match",
            "crate::tests::a_child_half_reads_its_part",
        ],
        "{found:#?}"
    );
    assert_eq!(
        strip(&found.refused),
        [
            "crate::tests::a_road_after_a_statement",
            "crate::tests::a_road_among_a_macros_tokens",
            "crate::tests::a_road_at_the_end_of_a_loop",
            "crate::tests::an_if_with_an_else",
        ],
        "{found:#?}"
    );
    assert_eq!(
        strip(&found.dropped),
        [
            "crate::tests::a_panic_first_drops_the_road (after `panic!` at line 160)",
            "crate::tests::a_road_after_an_assertion_has_checked_something (after `assert!` at line 310)",
            "crate::tests::an_assert_helper_first_drops_the_road (after `assert_nothing_moved` at line 177)",
            "crate::tests::an_expect_err_first_drops_the_road (after `expect_err` at line 169)",
        ],
        "{found:#?}"
    );
    assert_eq!(found.tests, 30, "{found:#?}");
}

/// RED (0.4.10 plan row 12) — **the register is an equality with the sites**: a site no row
/// names is unlisted, and a row naming no site is stale; **a row keyed on one test covers that
/// test's road only** (review N5): a bare name's row for one test leaves the same name in
/// another test unlisted, and a bare name cannot be registered file-wide.
///
/// MUTATION: answer no stale rows from `judged` and the stale rows are not named; key a bare
/// name file-wide in `scan` and `a_bare_name_in_another_test` is covered by the other test's row.
#[test]
fn a_row_naming_no_road_is_stale_and_an_unnamed_road_is_unlisted() {
    let (index, directory) = fixture();
    let mut found = Found::default();
    scan(&index, &directory, &mut found);
    let rows = rows_of(&format!(
        "# a register\n{HEADER}\n\
         lib.rs\t*\tTestRoot::new\tenvironment: no test root\tthe fixture's root\n\
         lib.rs\ta_host_by_its_cfg\tcfg!(not(windows))\tother-platform\tthe host check\n\
         lib.rs\ta_bare_name_in_one_test\titer\tenvironment: no names\tone test's names\n\
         lib.rs\t*\titer\tenvironment: no names\tevery test's names, which no row may say\n\
         lib.rs\t*\tgone::away\tother-unix\ta fixture no test calls any more\n"
    ));
    let (unlisted, stale) = judged(&found.sites, &rows);
    let unlisted_tests: Vec<&str> = unlisted
        .iter()
        .filter_map(|entry| entry.split(' ').nth(1))
        .collect();
    assert!(
        unlisted_tests.contains(&"crate::tests::a_bare_name_in_another_test")
            && !unlisted_tests.contains(&"crate::tests::a_bare_name_in_one_test")
            && !unlisted_tests.contains(&"crate::tests::a_fixture_answering_none")
            && !unlisted_tests.contains(&"crate::tests::a_host_by_its_cfg")
            && unlisted_tests.contains(&"crate::tests::an_endpoint_by_a_match"),
        "{unlisted:#?}"
    );
    assert_eq!(stale, ["lib.rs\t*\tgone::away", "lib.rs\t*\titer"]);
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
         a.rs\t*\tf\tother-unix\tone reason\n\
         a.rs\t*\tg\tenvironment: macos without a root\tanother\n"
    ));
}
