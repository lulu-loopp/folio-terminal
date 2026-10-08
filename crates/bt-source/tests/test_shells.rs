//! **Every real shell a test starts is started through `bt_pty::test_shell`**
//! — T-TEST-SHELL-HYGIENE (2026-10-04).
//!
//! A test that started the default shell the way a pane starts it ran the
//! user's own `$PROFILE` and appended the line it typed to the user's own
//! PSReadLine history, once per full test run. `bt_pty::test_shell` is where
//! the rule that prevents that is kept — `-NoProfile`, `/D`, `--norc
//! --noprofile`, a temporary `HOME`/`APPDATA`/`XDG_*`, and a PowerShell's
//! history refusal established and read back before anything is typed — and
//! this test is what holds every test in the workspace to that door.
//!
//! # What it reads
//!
//! Every first-party package, `src/`, `tests/`, `examples/` and `benches/`
//! (the universe `timing.rs` reads), and in it only **test code**: bytes no
//! build of the shipped program contains ([`bt_source::Occurrence::in_the_product`]).
//! `bt_pty::test_shell` itself is compiled under `any(test, feature =
//! "test-shell")`, which a product build may turn on, so it is not test code
//! here — it is the door, and the door is allowed to open.
//!
//! Two shapes are refused there:
//!
//! 1. **A pseudoconsole door** ([`PTY_DOORS`]): `PtySession::spawn` and the
//!    resolving doors around it, named as a path — called or handed on as a
//!    value. On a pseudoconsole every program is a terminal's child and every
//!    shell is interactive, so this is refused whatever the program.
//! 2. **A process start off a pseudoconsole** ([`PROCESS_STARTS`]) whose first
//!    argument names a shell: a string literal whose file name is one of
//!    [`SHELLS`], or a name in the argument with one of them as a `_`-separated
//!    word (`git_bash()`, `ComSpec`). Other programs started this way — the
//!    test binary itself, `where.exe`, `node --version` — are not shells and
//!    are not this rule's subject.
//!
//! # What stays review-owned
//!
//! A shell whose program reaches the start through a name that does not say
//! so (`Command::new(program)` with `program` a parameter), including a neutral
//! constant, and direct `portable_pty` backend use inside `bt-pty`'s own tests.
//! Mechanical `use … as …` aliases of a named door are resolved in their
//! lexical module or block scope. A parent-module alias needs no alias inference:
//! the canonical member (`Command::new`, `quiet_command`, `PtySession::spawn`)
//! remains a literal suffix/identifier in the call. Macro-generated imports and
//! type aliases that do not come from a `use` declaration stay review-owned.

use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, LiteralValue, Package, Pattern, Search, Span, TargetId, TargetKind,
    TargetRoot, TokenKind, Universe, Vendor, View, Workspace, is_vendored, needle, report,
    universes,
};
use syn::spanned::Spanned as _;
use syn::visit::Visit as _;

/// The pseudoconsole doors a test may not name: every one of them starts a
/// process on a pseudoconsole.
const PTY_DOORS: [&str; 8] = [
    "PtySession::spawn",
    "PtySession::spawn_default",
    "PtySession::spawn_default_in",
    "PtySession::spawn_default_with",
    "PtySession::spawn_shell_in",
    "PtySession::spawn_shell_in_with",
    "PtySession::spawn_refreshed",
    "PtySession::spawn_interactive",
];

/// The calls that start a process off a pseudoconsole: the standard library's,
/// and `bt_platform`'s three doors in front of it.
const PROCESS_STARTS: [&str; 4] = [
    "Command::new",
    "quiet_command",
    "quiet_command_named",
    "quiet_breakaway_command",
];

/// The shells, by file name without extension — and `comspec`, the variable
/// Windows names its command interpreter by.
const SHELLS: [&str; 12] = [
    "powershell",
    "pwsh",
    "cmd",
    "comspec",
    "sh",
    "bash",
    "dash",
    "ksh",
    "mksh",
    "zsh",
    "fish",
    "wsl",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `timing.rs` does.
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

fn workspace_indices() -> Vec<Index> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            Index::build(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
        })
        .collect()
}

/// The `test_shells` fixture: each shape the guard refuses and each it allows.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("test_shells");
    let universe = Universe::declare(
        "the test_shells fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "test_shells".to_owned(),
                kind: TargetKind::Library,
                name: "test_shells".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

#[derive(Debug)]
struct UseAlias {
    name: String,
    path: Vec<String>,
    scope: std::ops::Range<usize>,
    binding: std::ops::Range<usize>,
}

fn byte_offset(text: &str, at: proc_macro2::LineColumn) -> usize {
    let line_start = text
        .split_inclusive('\n')
        .take(at.line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    line_start + at.column
}

fn source_range(text: &str, base: usize, span: proc_macro2::Span) -> std::ops::Range<usize> {
    base + byte_offset(text, span.start())..base + byte_offset(text, span.end())
}

fn use_bindings(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(String, Vec<String>, proc_macro2::Span)>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_bindings(&path.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            out.push((name.ident.to_string(), path, name.ident.span()));
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            path.push(rename.ident.to_string());
            out.push((rename.rename.to_string(), path, rename.rename.span()));
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                use_bindings(tree, prefix, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

struct AliasCollector<'a> {
    text: &'a str,
    base: usize,
    scopes: Vec<std::ops::Range<usize>>,
    aliases: Vec<UseAlias>,
}

impl AliasCollector<'_> {
    fn record(&mut self, item: &syn::ItemUse) {
        let mut bindings = Vec::new();
        use_bindings(&item.tree, &mut Vec::new(), &mut bindings);
        for (name, path, binding) in bindings {
            if name != *path.last().expect("a use binding has a path") {
                self.aliases.push(UseAlias {
                    name,
                    path,
                    scope: self.scopes.last().expect("a lexical scope").clone(),
                    binding: source_range(self.text, self.base, binding),
                });
            }
        }
    }

    fn record_items(&mut self, items: &[syn::Item]) {
        for item in items {
            if let syn::Item::Use(item) = item {
                self.record(item);
            }
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for AliasCollector<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let Some((brace, items)) = &item.content else {
            return;
        };
        self.scopes.push(
            self.base + byte_offset(self.text, brace.span.open().end())
                ..self.base + byte_offset(self.text, brace.span.close().start()),
        );
        self.record_items(items);
        for item in items {
            self.visit_item(item);
        }
        self.scopes.pop();
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scopes
            .push(source_range(self.text, self.base, block.span()));
        for statement in &block.stmts {
            if let syn::Stmt::Item(syn::Item::Use(item)) = statement {
                self.record(item);
            }
        }
        syn::visit::visit_block(self, block);
        self.scopes.pop();
    }
}

fn use_aliases(index: &Index) -> Vec<UseAlias> {
    let mut aliases = Vec::new();
    for file in index.files() {
        let text = index.text(file.span());
        let syntax = syn::parse_file(text).unwrap_or_else(|error| {
            panic!(
                "{} is Rust the source index already accepted: {error}",
                file.path().display()
            )
        });
        let scopes = std::iter::once(file.span().start()..file.span().end()).collect();
        let mut collector = AliasCollector {
            text,
            base: file.span().start(),
            scopes,
            aliases: Vec::new(),
        };
        collector.record_items(&syntax.items);
        for item in &syntax.items {
            collector.visit_item(item);
        }
        aliases.extend(collector.aliases);
    }
    aliases
}

fn path_ends_with(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn active_alias_is(aliases: &[UseAlias], name: &str, at: Span, suffix: &[&str]) -> bool {
    aliases
        .iter()
        .filter(|alias| {
            alias.name == name && at.start() >= alias.scope.start && at.end() <= alias.scope.end
        })
        .min_by_key(|alias| alias.scope.end - alias.scope.start)
        .is_some_and(|alias| path_ends_with(&alias.path, suffix))
}

/// Uses of `target` written through a mechanical `use … as …` binding. The
/// narrowest lexical binding wins, as Rust name resolution does for shadowing.
fn aliased_uses(index: &Index, aliases: &[UseAlias], target: &str) -> Vec<Span> {
    let segments: Vec<&str> = target.split("::").collect();
    let mut found = Vec::new();
    for alias in aliases {
        let (written, suffix) =
            if segments.len() > 1 && path_ends_with(&alias.path, &segments[..segments.len() - 1]) {
                (
                    format!("{}::{}", alias.name, segments[segments.len() - 1]),
                    &segments[..segments.len() - 1],
                )
            } else if path_ends_with(&alias.path, &segments) {
                (alias.name.clone(), segments.as_slice())
            } else {
                continue;
            };
        let pattern = if written.contains("::") {
            Pattern::path(&written)
        } else {
            Pattern::identifier(&written)
        };
        for span in in_test_code(index, pattern) {
            if !(span.start() >= alias.binding.start && span.end() <= alias.binding.end)
                && active_alias_is(aliases, &alias.name, span, suffix)
            {
                found.push(span);
            }
        }
    }
    found.sort_by_key(|span| span.start());
    found.dedup();
    found
}

/// Where a span is, for a reader.
fn place(index: &Index, span: Span) -> String {
    index
        .locate(span.start())
        .map_or_else(|| format!("{span:?}"), |location| location.to_string())
}

/// Every match of `pattern` in the test code of `index`.
fn in_test_code(index: &Index, pattern: Pattern) -> Vec<Span> {
    let found = index
        .search(&Search::new(needle!(pattern), View::Identifiers))
        .unwrap_or_else(|failure| panic!("{failure}"));
    found
        .occurrences()
        .iter()
        .filter(|occurrence| !occurrence.in_the_product(index))
        .map(|occurrence| occurrence.span)
        .collect()
}

/// The span of the innermost item body that holds `at`.
fn enclosing_body(index: &Index, at: Span) -> Option<Span> {
    index
        .items()
        .iter()
        .filter_map(|item| item.body())
        .filter(|body| at.within(*body))
        .min_by_key(|body| body.len())
}

/// The byte range of the first argument of the call whose callee ends at
/// `callee.end()`: from just after its `(` to the `,` or `)` that ends it at
/// depth zero. String literals and comments are stepped over whole, so a
/// parenthesis or comma inside one is not read as the call's.
fn first_argument(index: &Index, callee: Span) -> Option<(usize, usize)> {
    let body = enclosing_body(index, callee)?;
    let text = index.text(body);
    let skipped: Vec<Span> = index
        .literals()
        .iter()
        .map(|literal| literal.span())
        .chain(index.comments().iter().map(|comment| comment.span()))
        .filter(|span| span.within(body))
        .collect();
    let mut at = callee.end();
    // The callee's own spelling may end before its parenthesis (`Command::new (`).
    while text[at - body.start()..].starts_with(char::is_whitespace) {
        at += text[at - body.start()..].chars().next()?.len_utf8();
    }
    if !text[at - body.start()..].starts_with('(') {
        return None;
    }
    let start = at + 1;
    let mut depth = 0usize;
    let mut offset = start;
    while offset < body.end() {
        if let Some(span) = skipped.iter().find(|span| span.holds(offset)) {
            offset = span.end();
            continue;
        }
        let character = text[offset - body.start()..].chars().next()?;
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ')' | ',' if depth == 0 => return Some((start, offset)),
            _ => {}
        }
        offset += character.len_utf8();
    }
    None
}

/// The shell a program spelling names, if it names one: its file name, without
/// the extension, whichever separator the path was written with.
fn shell_named_by(value: &str) -> Option<&'static str> {
    let name = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    SHELLS.iter().copied().find(|shell| *shell == stem)
}

/// The shell the first argument in `range` names: a string literal that is
/// one, or a name with one as a `_`-separated word.
fn shell_in_argument(index: &Index, (start, end): (usize, usize)) -> Option<String> {
    let inside = |span: Span| span.start() >= start && span.end() <= end;
    let literal = index
        .literals()
        .iter()
        .filter(|literal| inside(literal.span()))
        .find_map(|literal| match literal.value() {
            LiteralValue::Str(value) => {
                shell_named_by(value).map(|shell| format!("the literal {value:?} ({shell})"))
            }
            _ => None,
        });
    literal.or_else(|| {
        index
            .tokens()
            .iter()
            .filter(|token| token.kind() != TokenKind::Lifetime && inside(token.span()))
            .find_map(|token| {
                let name = index.text(token.name_span());
                name.split('_')
                    .find_map(|word| {
                        let word = word.to_ascii_lowercase();
                        SHELLS.iter().copied().find(|shell| *shell == word)
                    })
                    .map(|shell| format!("the name `{name}` ({shell})"))
            })
    })
}

/// Every refused shape in the test code of `index`, one line each.
fn violations(index: &Index) -> Vec<String> {
    let mut found = Vec::new();
    let aliases = use_aliases(index);
    for door in PTY_DOORS {
        let spans = in_test_code(index, Pattern::path(door))
            .into_iter()
            .chain(aliased_uses(index, &aliases, door));
        for span in spans {
            found.push(format!(
                "{}: {door} — a test's pseudoconsole child is started through \
                 bt_pty::test_shell::TestShell",
                place(index, span)
            ));
        }
    }
    for start in PROCESS_STARTS {
        let pattern = if start.contains("::") {
            Pattern::path(start)
        } else {
            Pattern::identifier(start)
        };
        let spans = in_test_code(index, pattern)
            .into_iter()
            .chain(aliased_uses(index, &aliases, start));
        for span in spans {
            let Some(argument) = first_argument(index, span) else {
                continue;
            };
            if let Some(shell) = shell_in_argument(index, argument) {
                found.push(format!(
                    "{}: {start} starts {shell} — a test's shell is started through \
                     bt_pty::test_shell::Hygiene::command",
                    place(index, span)
                ));
            }
        }
    }
    found.sort();
    found
}

/// RED (T-TEST-SHELL-HYGIENE) — **no test in the workspace starts a real shell
/// except through `bt_pty::test_shell`.**
///
/// MUTATION: put back `PtySession::spawn_default(PtySize::cells(columns, rows),
/// Arc::new(|| {}))` in `bt-app`'s `real_powershell_input_reaches_a_viewport_owned_frame`,
/// or `Command::new("powershell.exe")` in `bt-pty`'s
/// `tests/shell_integration_script.rs`, and this names the line. Rename either start
/// through `use … as …` and the same call remains a finding.
#[test]
fn every_real_shell_a_test_starts_goes_through_the_test_shell_door() {
    let found: Vec<String> = workspace_indices().iter().flat_map(violations).collect();
    assert!(
        found.is_empty(),
        "a test starts a real shell without bt_pty::test_shell, which is how the user's own \
         $PROFILE ran and their own history file was written to by a test run — start it \
         through TestShell (on a pseudoconsole) or Hygiene::command (off one):\n  {}",
        found.join("\n  ")
    );
}

/// RED (T-TEST-SHELL-HYGIENE) — **the guard sees each shape it refuses, and
/// only those.**
///
/// The fixture holds a raw pseudoconsole spawn in a test and the same door
/// handed on as a value, a PowerShell, a `ComSpec` and a `git_bash()` started
/// off a pseudoconsole in tests; and, allowed, the door called from product
/// code, a non-shell program started from a test, a shell started through the
/// helper's own door, and a shell's name in a string that starts nothing.
///
/// MUTATION: drop the `in_the_product` filter and the product call is named;
/// drop the identifier half of `shell_in_argument` and `git_bash()` and
/// `ComSpec` are not. Replace an aliased door's canonical suffix in
/// `active_alias_is` with an unrelated one and the three renamed-door rows disappear. Remove the
/// canonical suffix/identifier search and the three parent-module rows disappear.
#[test]
fn the_guard_names_each_raw_shell_start_and_nothing_else() {
    let index = fixture();
    let found = violations(&index);
    let lines: Vec<String> = found
        .iter()
        .map(|line| {
            let (place, rest) = line.split_once(": ").expect("a place, then the finding");
            let line_number = place.rsplit(':').nth(1).expect("file:line:column");
            format!("{line_number}: {rest}")
        })
        .collect();
    let mut expected = vec![
        "17: PtySession::spawn — a test's pseudoconsole child is started through \
         bt_pty::test_shell::TestShell",
        "22: PtySession::spawn_default — a test's pseudoconsole child is started through \
         bt_pty::test_shell::TestShell",
        "27: Command::new starts the literal \"powershell.exe\" (powershell) — a test's shell \
         is started through bt_pty::test_shell::Hygiene::command",
        "32: Command::new starts the literal \"ComSpec\" (comspec) — a test's shell is started \
         through bt_pty::test_shell::Hygiene::command",
        "37: quiet_command starts the name `git_bash` (bash) — a test's shell is started \
         through bt_pty::test_shell::Hygiene::command",
        "45: Command::new starts the literal \"pwsh\" (pwsh) — a test's shell is started through \
         bt_pty::test_shell::Hygiene::command",
        "46: quiet_command starts the name `git_bash` (bash) — a test's shell is started through \
         bt_pty::test_shell::Hygiene::command",
        "47: PtySession::spawn — a test's pseudoconsole child is started through \
         bt_pty::test_shell::TestShell",
        "48: Command::new starts the literal \"zsh\" (zsh) — a test's shell is started through \
         bt_pty::test_shell::Hygiene::command",
        "49: quiet_command starts the name `git_bash` (bash) — a test's shell is started through \
         bt_pty::test_shell::Hygiene::command",
        "50: PtySession::spawn — a test's pseudoconsole child is started through \
         bt_pty::test_shell::TestShell",
    ];
    expected.sort_unstable();
    let mut lines = lines;
    lines.sort();
    assert_eq!(lines, expected, "{found:#?}");
}
