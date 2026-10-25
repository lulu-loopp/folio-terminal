//! **A test's verdict does not hang on a file another build step made, on the process's
//! directory or environment, or on a fixture folder another test or another run shares** —
//! T-TEST-ORDER-STRAYS, the guard for shapes (a), (c) and (d) of its census.
//!
//! Guard (source-reading by design): its subject is how the workspace's test code is written,
//! so it reads that code through `bt-source`'s index, the way `temp_paths` does, and binds to no
//! file.
//!
//! # What is read
//!
//! **Test code**: a function or method whose every declaration path is out of the product build
//! (`cfg(test)`, a module under it, an integration-test target) or stands on a `cfg` that names
//! `test` (`any(test, feature = …)`), as `temp_paths` defines it. In **its own body**, closures
//! and nested items included. Every first-party package but `bt-testpath`, which is the owner of
//! test path names and the one place `temp_dir()` becomes a test's path; `bt-source`'s
//! `tests/fixtures/` are source about compilation and are not read.
//!
//! # The grammar: five shapes, each judged on one body or one literal
//!
//! The **owner calls** are `unique_name(`, `temp_path(` and `link_free_temp_dir(` — the
//! `bt_testpath` doors, whose names no two calls of one process share.
//!
//! * **A1 — a bin located by `CARGO_BIN_EXE_`.** A string literal whose value begins
//!   `CARGO_BIN_EXE_` in a file outside its package's `tests/` folder — in a body, in a `const`
//!   or `static` initializer, in product code alike: judged on the literal, keyed by the
//!   innermost item that holds it (`literal` when no item does). Cargo sets that variable, and
//!   builds the bin it names, only for an integration test; read anywhere else it is a file
//!   another step may or may not have made, or an `option_env!` that is `None` and turns the
//!   test into one that tests nothing. Refused.
//! * **A2 — a path beside the test executable.** A body that calls `current_exe(` and `parent(`
//!   is deriving a path from the executable's folder: the profile folder cargo builds into. It
//!   is accepted when the body also makes an owner call (a scratch folder of its own beside the
//!   executable); otherwise it is reading or writing a name there that it did not make unique,
//!   which is a build output (`folio.exe` of the bin, which only some runs build) or a shared
//!   fixture. Refused.
//! * **C1 — the process's directory or environment changed.** A call `set_current_dir(`,
//!   `set_var(` or `remove_var(`, in any spelling of the path to it. Every other test of the
//!   binary reads both, on parallel threads. Refused.
//! * **D1 — a fixture folder by a fixed name.** A body that creates — a call `create_dir_all(`,
//!   `create_dir(`, `write(`, `copy(` or `create(` — and roots a path at the temporary folder (the
//!   path `env::temp_dir`, however long its prefix: a test module's own helper called
//!   `temp_dir(tag)` is not the folder) or at the build directory (a string literal one of whose `/`- or
//!   `\`-separated segments is `target`), and makes no owner call. Its names are fixed: a second
//!   test that passes the same one shares the folder, and what an earlier run left there is this
//!   run's input. Refused.
//!
//! * **D0 — the temporary folder imported.** D1 reads the folder by its path `env::temp_dir`;
//!   an import makes it a root D1 cannot see. Refused:
//!   * a module-level `use` that brings `std::env::temp_dir` in (under any name), globs
//!     `std::env`, or brings `std::env` in under another name (`use std::env as e;`, then
//!     `e::temp_dir()`), in any file, product or test — by the module that writes it
//!     (`<module>::use`);
//!   * a `use` written inside a test-code body (the index records module-level imports only)
//!     whose tree, read up to its `;`, names `env` and `temp_dir`, globs beside `env`, or
//!     renames `env` (`env as e`, `env::{self as e}`) — `use std::env::{temp_dir, var};` inside
//!     a test — by the body's item.
//!
//! **Undecided is refused, never silent**: a body in one of these shapes whose path is in fact
//! made unique some other way, or whose fixed name is the point (a file written for a person to
//! open), is a refusal like any other. Such a body stands only as a row of
//! `docs/plans/TEST-ORDER-EXCEPTIONS.tsv` — file, item, shape, reason — and the rows are an
//! equality with the refusals: an unlisted refusal is red, and a row that no longer refuses is
//! stale and red.
//!
//! # What it does not see
//!
//! * **A path built across two bodies.** A helper that takes the name its caller fixed is judged
//!   on its own body (the `root(name)` shape: no owner call, a `target` root, a create — refused
//!   at the helper), but a root made in one helper and created in another is two bodies with
//!   half the shape each.
//! * **A body that makes one path unique and another not.** One owner call accepts the body.
//! * **A process-global another test mutates** (shape (b) of the census): not this grammar's.
//!   `test_globals.rs` holds the globals tests share or write to `docs/plans/TEST-GLOBALS.tsv`
//!   (T-TEST-PROCESS-GLOBALS), and states its own blind spot: a static reached only through
//!   product calls.
//! * A re-export of `std::env::set_var` under another name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, ItemRecord, LiteralValue, Package, Pattern, Search, Span, TargetId,
    TargetKind, TargetRoot, Universe, Vendor, View, Workspace, is_inside, is_vendored, needle,
    normalized, report, universes,
};

/// The owner of test path names.
const OWNER: &str = "bt-testpath";

/// The register, from the workspace root.
const LIST: &str = "docs/plans/TEST-ORDER-EXCEPTIONS.tsv";

/// The register's column header.
const HEADER: &str = "file\titem\tshape\treason";

/// The calls that name a path no other call of the process shares.
const OWNER_CALLS: [&str; 3] = ["unique_name", "temp_path", "link_free_temp_dir"];

/// The calls that change the process's directory or environment.
const PROCESS_WRITES: [&str; 3] = ["set_current_dir", "set_var", "remove_var"];

/// The calls that create a file or a folder.
const CREATES: [&str; 5] = ["create_dir_all", "create_dir", "write", "copy", "create"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are declared out.
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

/// Every span of `pattern` in the identifier view of `index`, sorted.
fn spans(index: &Index, pattern: Pattern) -> Vec<Span> {
    let mut found = index
        .search(&Search::new(needle!(pattern), View::Identifiers))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// Every call span of each name, sorted.
fn calls(index: &Index, names: &[&str]) -> Vec<Span> {
    let mut all: Vec<Span> = names
        .iter()
        .flat_map(|name| spans(index, Pattern::call(name)))
        .collect();
    all.sort();
    all
}

/// Whether any span of `sorted` lies inside `outer`.
fn any_inside(sorted: &[Span], outer: Span) -> bool {
    let first = sorted.partition_point(|span| span.start() < outer.start());
    sorted[first..]
        .iter()
        .take_while(|span| span.start() < outer.end())
        .any(|span| span.within(outer))
}

/// Whether a `cfg` predicate, as written, names the `test` configuration — a whole word outside
/// any string, so `feature = "test-shell"` does not.
fn names_test(predicate: &str) -> bool {
    let mut outside = String::with_capacity(predicate.len());
    let mut quoted = false;
    for character in predicate.chars() {
        if character == '"' {
            quoted = !quoted;
            outside.push(' ');
        } else if !quoted {
            outside.push(character);
        }
    }
    outside
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|word| word == "test")
}

/// Whether every declaration of `item` is test code (see the module header).
fn is_test_code(item: &ItemRecord) -> bool {
    let mut identities = item.identities().peekable();
    identities.peek().is_some()
        && identities.all(|identity| {
            !identity.variant.permits_product()
                || identity
                    .variant
                    .predicates()
                    .iter()
                    .any(|predicate| names_test(predicate))
        })
}

/// The string literals of `index` whose value satisfies `keep`, sorted by span.
fn literals(index: &Index, keep: impl Fn(&str) -> bool) -> Vec<Span> {
    let mut found: Vec<Span> = index
        .literals()
        .iter()
        .filter(|literal| matches!(literal.value(), LiteralValue::Str(value) if keep(value)))
        .map(bt_source::LiteralRecord::span)
        .collect();
    found.sort();
    found
}

/// Whether a literal names the build directory: one of its path segments is `target`.
fn names_the_build_directory(value: &str) -> bool {
    value.split(['/', '\\']).any(|segment| segment == "target")
}

/// `path` relative to `root`, with forward slashes.
fn relative(path: &Path, root: &Path) -> String {
    normalized(path)
        .strip_prefix(normalized(root))
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        .to_string_lossy()
        .replace('\\', "/")
}

/// One refusal: where, which item, which shape.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Hit {
    file: String,
    item: String,
    shape: &'static str,
}

/// Whether the `use` whose keyword is at `keyword` brings the temporary folder into reach: its
/// tree, up to the `;`, names `env` and `temp_dir`, globs (`*`) beside `env`, or renames `env`
/// (`env as`, or `env::{self as …}`: any `self as` in a tree that names `env`).
fn a_use_brings_the_folder(index: &Index, keyword: Span) -> bool {
    let file = index
        .file_at(keyword.start())
        .expect("a token of the index stands in one of its files");
    let text = &index.text(file.span())[keyword.start() - file.span().start()..];
    let tree = &text[..text.find(';').unwrap_or(text.len())];
    let words: Vec<&str> = tree
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    words.contains(&"env")
        && (words.contains(&"temp_dir")
            || tree.contains('*')
            || words
                .windows(2)
                .any(|pair| pair == ["env", "as"] || pair == ["self", "as"]))
}

/// Every refusal of the grammar in `index`. `root` is what file paths are written relative to,
/// and `tests` is the package's integration-test folder.
fn scan(index: &Index, root: &Path, tests: &Path) -> BTreeSet<Hit> {
    let owner = calls(index, &OWNER_CALLS);
    let process_writes = calls(index, &PROCESS_WRITES);
    let creates = calls(index, &CREATES);
    let current_exe = calls(index, &["current_exe"]);
    let parent = calls(index, &["parent"]);
    let bin_exe = literals(index, |value| value.starts_with("CARGO_BIN_EXE_"));
    // `env::temp_dir` as a path: a test module's own helper named `temp_dir(tag)` (which makes
    // its name with the owner) is not the temporary folder.
    let mut roots = spans(index, Pattern::path("env::temp_dir"));
    roots.extend(literals(index, names_the_build_directory));
    roots.sort();

    let mut found = BTreeSet::new();
    // D0: the spelling of the temporary folder D1 cannot see is a module that imports it, or
    // imports `env` under another name.
    for import in index.uses() {
        let brings_it = import.names().iter().any(|(bound, path)| {
            path.ends_with(&["env".to_owned(), "temp_dir".to_owned()])
                || (path.ends_with(&["env".to_owned()]) && bound != "env")
        }) || import
            .globs()
            .iter()
            .any(|path| path.ends_with(&["env".to_owned()]));
        if !brings_it {
            continue;
        }
        let Some(file) = index.file_at(import.whole().start()) else {
            continue;
        };
        for module in import.module_paths() {
            found.insert(Hit {
                file: relative(file.path(), root),
                item: format!("{module}::use"),
                shape: "D0",
            });
        }
    }
    // A1: on the literal itself, wherever it stands outside `tests/`.
    for literal in &bin_exe {
        let file = index
            .file_at(literal.start())
            .expect("a literal of the index stands in one of its files");
        if is_inside(file.path(), tests) {
            continue;
        }
        // The innermost holder: a function, a type or a `const`/`static`.
        let mut holders: Vec<(Span, Vec<String>)> = index
            .items()
            .iter()
            .filter(|item| literal.within(item.whole()))
            .map(|item| {
                let names = item
                    .identities()
                    .map(|identity| format!("{}::{}", identity.module_path, identity.name))
                    .collect();
                (item.whole(), names)
            })
            .collect();
        holders.extend(
            index
                .values()
                .iter()
                .filter(|value| literal.within(value.whole()))
                .map(|value| {
                    let names = value
                        .module_paths()
                        .into_iter()
                        .map(|module| format!("{module}::{}", value.name()))
                        .collect();
                    (value.whole(), names)
                }),
        );
        let names = holders
            .into_iter()
            .min_by_key(|(whole, _)| whole.len())
            .map_or_else(|| vec!["literal".to_owned()], |(_, names)| names);
        let file = relative(file.path(), root);
        for item in names {
            found.insert(Hit {
                file: file.clone(),
                item,
                shape: "A1",
            });
        }
    }
    let body_use = spans(index, Pattern::identifier("use"));
    for item in index.items() {
        let Some(body) = item.body() else { continue };
        if !item.kind().is_callable() || !is_test_code(item) {
            continue;
        }
        let file_path = index.file_of(item).path();
        let mut shapes = Vec::new();
        let owned = any_inside(&owner, body);
        if any_inside(&current_exe, body) && any_inside(&parent, body) && !owned {
            shapes.push("A2");
        }
        if any_inside(&process_writes, body) {
            shapes.push("C1");
        }
        if any_inside(&creates, body) && any_inside(&roots, body) && !owned {
            shapes.push("D1");
        }
        if body_use
            .iter()
            .filter(|span| span.within(body))
            .any(|span| a_use_brings_the_folder(index, *span))
        {
            shapes.push("D0");
        }
        if shapes.is_empty() {
            continue;
        }
        let file = relative(file_path, root);
        for identity in item.identities() {
            for shape in &shapes {
                found.insert(Hit {
                    file: file.clone(),
                    item: format!("{}::{}", identity.module_path, identity.name),
                    shape,
                });
            }
        }
    }
    found
}

/// Every refusal in every first-party package but the owner's, and how many test-code
/// functions were read.
fn workspace_hits() -> (BTreeSet<Hit>, usize) {
    let root = normalized(&workspace_root());
    let workspace = Workspace::read(&root).expect("this workspace");
    let packages: Vec<&Package> = workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .collect();
    assert!(
        packages.iter().any(|package| package.name() == OWNER),
        "{OWNER} is a member of this workspace"
    );
    let mut hits = BTreeSet::new();
    let mut read = 0;
    for package in packages
        .into_iter()
        .filter(|package| package.name() != OWNER)
    {
        let index = Index::build(&universe_for(package))
            .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
        hits.extend(scan(&index, &root, &package.directory().join("tests")));
        read += index
            .items()
            .iter()
            .filter(|item| item.body().is_some() && item.kind().is_callable() && is_test_code(item))
            .count();
    }
    (hits, read)
}

/// The register's rows as hits.
fn register() -> BTreeSet<Hit> {
    rows_of(&std::fs::read_to_string(workspace_root().join(LIST)).expect("the register"))
}

/// The rows of a register's text, refusing a malformed line, and a row listed twice, by number.
fn rows_of(text: &str) -> BTreeSet<Hit> {
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
        let [file, item, shape, reason] = fields[..] else {
            panic!("{LIST}:{}: four tab-separated fields: {line}", number + 1);
        };
        let shape = match shape {
            "A1" => "A1",
            "A2" => "A2",
            "C1" => "C1",
            "D0" => "D0",
            "D1" => "D1",
            other => panic!("{LIST}:{}: no shape {other:?}", number + 1),
        };
        assert!(
            !reason.trim().is_empty(),
            "{LIST}:{}: a row says why",
            number + 1
        );
        let fresh = rows.insert(Hit {
            file: file.to_owned(),
            item: item.to_owned(),
            shape,
        });
        assert!(fresh, "{LIST}:{}: listed twice: {line}", number + 1);
    }
    assert!(header, "{LIST} has its header");
    rows
}

/// RED (T-TEST-ORDER-STRAYS review N3) — **a row listed twice is refused by its line**, even
/// with another reason: the rows are a set, and a second one would vanish into the first.
///
/// MUTATION: drop the `fresh` assertion in `rows_of` and this does not panic.
#[test]
#[should_panic(expected = "4: listed twice")]
fn a_row_listed_twice_is_refused() {
    let _ = rows_of(&format!(
        "# a register\n{HEADER}\na.rs\tcrate::f\tD1\tone reason\na.rs\tcrate::f\tD1\tanother\n"
    ));
}

fn listed(hits: &BTreeSet<Hit>) -> String {
    hits.iter()
        .map(|hit| format!("{}\t{}\t{}", hit.file, hit.item, hit.shape))
        .collect::<Vec<_>>()
        .join("\n  ")
}

/// RED (T-TEST-ORDER-STRAYS) — **no test locates a build output by the executable's folder or
/// by `CARGO_BIN_EXE_` outside an integration test, changes the process's directory or
/// environment, or creates a fixture folder by a fixed name, except as a row of the register;
/// and every row still is one.**
///
/// MUTATION: put back `attention_ownership`'s old `root(name)` (`.join(name)` without
/// `bt_testpath::unique_name`) and this names it under D1; put back the old manifest test's
/// `current_exe().parent()…join("folio.exe")` and this names it under A2.
#[test]
fn every_test_that_depends_on_a_build_output_the_process_or_a_fixed_fixture_is_on_the_register() {
    let (hits, read) = workspace_hits();
    assert!(
        read > 1000,
        "the scan read {read} test functions; the workspace has thousands"
    );
    let rows = register();
    let unlisted: BTreeSet<Hit> = hits.difference(&rows).cloned().collect();
    let stale: BTreeSet<Hit> = rows.difference(&hits).cloned().collect();
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "refused and not on {LIST} (make the path the test's own with `bt_testpath`, locate a \
         bin by `CARGO_BIN_EXE_` in an integration test, or add a row saying why):\n  {}\n\
         on {LIST} and no longer refused (remove the row):\n  {}",
        listed(&unlisted),
        listed(&stale)
    );
}

/// The `test_order` fixture, as an index, with its root and its integration-test folder.
fn fixture() -> (Index, PathBuf) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("test_order");
    let universe = Universe::declare(
        "the test_order fixture",
        vec![
            TargetRoot {
                id: TargetId {
                    package: "test_order".to_owned(),
                    kind: TargetKind::Library,
                    name: "test_order".to_owned(),
                },
                file: directory.join("lib.rs"),
            },
            TargetRoot {
                id: TargetId {
                    package: "test_order".to_owned(),
                    kind: TargetKind::IntegrationTest,
                    name: "beside".to_owned(),
                },
                file: directory.join("tests").join("beside.rs"),
            },
        ],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    (index, directory)
}

/// RED (T-TEST-ORDER-STRAYS) — **each shape is refused in test code, its accepted twin is not,
/// and product code is not read.**
///
/// The fixture holds every shape once in a test, its twin made with an owner call, the same
/// shapes in product code, and `CARGO_BIN_EXE_` in an integration test.
///
/// MUTATION: drop the owner check from D1 and `a_fixture_named_by_the_owner` is refused; read
/// product code and `product_writes_its_own_files` is refused; drop the `tests/` exemption from
/// A1 and `beside::a_bin_located_where_cargo_built_it` is refused; judge A1 on bodies again and
/// `BIN` is not; drop D0's body rule and `a_folder_imported_inside_the_body` is not.
#[test]
fn the_grammar_refuses_each_shape_and_accepts_its_twin() {
    let (index, directory) = fixture();
    let found: Vec<String> = scan(&index, &directory, &directory.join("tests"))
        .into_iter()
        .map(|hit| format!("{} {} {}", hit.file, hit.item, hit.shape))
        .collect();
    assert_eq!(
        found,
        [
            "lib.rs crate::tests::BIN A1",
            "lib.rs crate::tests::a_bin_located_outside_an_integration_test A1",
            "lib.rs crate::tests::a_build_output_beside_the_test_executable A2",
            "lib.rs crate::tests::a_fixed_fixture_in_the_build_directory D1",
            "lib.rs crate::tests::a_fixed_fixture_in_the_temporary_folder D1",
            "lib.rs crate::tests::a_folder_imported_inside_the_body D0",
            "lib.rs crate::tests::a_folder_through_a_renamed_self D0",
            "lib.rs crate::tests::aliased::use D0",
            "lib.rs crate::tests::imported::use D0",
            "lib.rs crate::tests::the_environment_changed C1",
            "lib.rs crate::tests::the_process_directory_changed C1",
        ],
        "{found:#?}"
    );
}
