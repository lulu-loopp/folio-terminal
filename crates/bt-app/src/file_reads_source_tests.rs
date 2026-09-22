//! AST source pin: test items, comments and strings cannot hide product reads.
//! The small manifest below inventories generic Read/OpenOptions doors, whose
//! receiver types Rust syntax alone cannot establish. Their exact counts and
//! the counting adapter in the same owner are pinned together.
//!
//! # Who wrote a door, and how this module says so
//!
//! A row of `file_reads_doors.txt` is `<count> <key>`, and the key names the
//! owner. Today every key spells a **file** — `main.rs: turn: .open` — and a
//! key that spells a file goes quiet the day its owner moves to another file:
//! the walk finds the door under a different name, and the row it used to match
//! is simply absent from a map nobody compares by owner. That is the defect
//! `docs/plans/bt-app-split-prep.md` exists to remove, and P6 removes it for the
//! twenty-one rows whose owner is a method of one of the two `impl Runtime<'_>`
//! blocks Step 2a moves.
//!
//! The replacement key is the **item**: the identity of plan §2.4 without its
//! module path — the type the `impl` is for, the trait if there is one, and the
//! name. `Runtime::turn`, `<FileAnimationSource as Read>::read`, `read_up_to`.
//!
//! **The module path is deliberately not in it.** Step 2a moves those methods
//! into a newly declared `src/runtime/`, where the same method answers to
//! `crate::runtime` instead of `crate`; a key carrying the module path would
//! change on the one move it exists to survive. `bt-source` drops the self
//! type's qualification for exactly that reason ([`bt_source::ItemQuery`]), and
//! what keeps the shorter key honest is that it is not merely asserted here:
//! [`file_reads_every_item_keyed_door_answers_to_one_item`] resolves every one
//! of them against the crate, and a key naming no item, or two, is a loud
//! refusal rather than a quiet merge.
//!
//! This is the **equivalence commit** of §6.0 rule 3: the item reading is added
//! beside the file reading and the two are asserted to agree row by row. The
//! manifest is untouched here; the deletion commit re-keys it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::visit::{self, Visit};

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || matches!(&attr.meta, syn::Meta::List(list)
            if list.path.is_ident("cfg") && list.parse_args::<syn::Meta>().ok()
                .and_then(|meta| product_cfg(&meta)) == Some(false))
    })
}

// Evaluate only the test bit; all platform/feature choices remain possible.
// In particular cfg(not(test)) must be scanned, and any(test, windows) is not
// a test-only item. A textual search for "test" would silently exempt both.
fn product_cfg(meta: &syn::Meta) -> Option<bool> {
    if meta.path().is_ident("test") {
        return Some(false);
    }
    let syn::Meta::List(list) = meta else {
        return None;
    };
    let children = list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    let values: Vec<_> = children.iter().map(product_cfg).collect();
    if list.path.is_ident("not") && values.len() == 1 {
        return values[0].map(|value| !value);
    }
    if list.path.is_ident("all") {
        if values.contains(&Some(false)) {
            Some(false)
        } else if values.iter().all(|value| *value == Some(true)) {
            Some(true)
        } else {
            None
        }
    } else if list.path.is_ident("any") {
        if values.contains(&Some(true)) {
            Some(true)
        } else if values.iter().all(|value| *value == Some(false)) {
            Some(false)
        } else {
            None
        }
    } else {
        None
    }
}

/// **Who wrote a door** — the §2.4 identity without its module path, which is
/// exactly what a [`bt_source::ItemQuery`] is built from.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Owner {
    /// The `impl` block's self type, last segment, lifetimes and generic
    /// arguments dropped — `Runtime` for `impl Runtime<'_>` and for
    /// `impl crate::Runtime<'_>` alike. `None` at module level.
    type_owner: Option<String>,
    /// The trait, written as its path without generic arguments. `None` for an
    /// inherent `impl`.
    trait_name: Option<String>,
    /// The name of the function itself.
    name: String,
}

impl Owner {
    /// The key an item-keyed row spells, and the way
    /// [`bt_source::ItemIdentity`] prints the same tuple.
    fn key(&self) -> String {
        match (&self.type_owner, &self.trait_name) {
            (Some(owner), Some(trait_name)) => format!("<{owner} as {trait_name}>::{}", self.name),
            (Some(owner), None) => format!("{owner}::{}", self.name),
            (None, Some(trait_name)) => format!("{trait_name}::{}", self.name),
            (None, None) => self.name.clone(),
        }
    }

    /// The same owner, as a question for the crate.
    fn query(&self) -> bt_source::ItemQuery {
        let query = match &self.type_owner {
            Some(owner) => bt_source::ItemQuery::method(owner, &self.name),
            None => bt_source::ItemQuery::function(&self.name),
        };
        match &self.trait_name {
            Some(trait_name) => query.of_trait(trait_name),
            None => query,
        }
    }
}

/// One door the walk found: who wrote it, which file that owner is written in
/// today, and the verb.
///
/// The file is here because a not-yet-migrated row's key still spells one. An
/// item-keyed row never reads it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Door {
    file: String,
    owner: Owner,
    door: String,
}

/// One owner that calls the counting adapter, the same way.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Counted {
    file: String,
    owner: Owner,
}

#[derive(Default)]
struct Doors {
    name: String,
    type_owner: Option<String>,
    trait_name: Option<String>,
    found: BTreeMap<(Owner, String), usize>,
    counted: BTreeMap<Owner, usize>,
    modules: Vec<(String, Option<String>)>,
}

impl Doors {
    fn owner(&self) -> Owner {
        Owner {
            type_owner: self.type_owner.clone(),
            trait_name: self.trait_name.clone(),
            name: self.name.clone(),
        }
    }

    fn record(&mut self, door: &str) {
        let owner = self.owner();
        *self.found.entry((owner, door.to_owned())).or_default() += 1;
    }

    fn count_one(&mut self) {
        let owner = self.owner();
        *self.counted.entry(owner).or_default() += 1;
    }
}

/// The self type of an `impl`, as the last segment of its path — the spelling
/// `bt-source` reduces `Runtime<'_>`, `Runtime<'a>` and `crate::Runtime<'_>` to.
fn self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

/// A trait path written out without its generic arguments, which is what
/// `bt_source::ItemQuery::of_trait` matches a recorded spelling by.
fn trait_path(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

impl<'ast> Visit<'ast> for Doors {
    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        if matches!(
            rename.ident.to_string().as_str(),
            "read" | "read_to_end" | "read_to_string" | "read_exact"
        ) {
            self.record("renamed read import");
        }
        visit::visit_use_rename(self, rename);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if test_only(&item.attrs) {
            return;
        }
        if item.content.is_none() {
            let explicit = item.attrs.iter().find_map(|attr| {
                if attr.path().is_ident("path")
                    && let syn::Meta::NameValue(value) = &attr.meta
                    && let syn::Expr::Lit(lit) = &value.value
                    && let syn::Lit::Str(path) = &lit.lit
                {
                    return Some(path.value());
                }
                None
            });
            self.modules.push((item.ident.to_string(), explicit));
        }
        visit::visit_item_mod(self, item);
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if test_only(&item.attrs) {
            return;
        }
        let type_owner = std::mem::replace(&mut self.type_owner, self_type(&item.self_ty));
        let trait_name = std::mem::replace(
            &mut self.trait_name,
            item.trait_.as_ref().map(|(_, path, _)| trait_path(path)),
        );
        visit::visit_item_impl(self, item);
        self.type_owner = type_owner;
        self.trait_name = trait_name;
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        // An `ItemFn` belongs to no `impl`, even when it is written inside one's
        // method: the owner it reports is the free function, not the method
        // around it.
        let previous = (
            std::mem::replace(&mut self.name, item.sig.ident.to_string()),
            self.type_owner.take(),
            self.trait_name.take(),
        );
        visit::visit_item_fn(self, item);
        self.name = previous.0;
        self.type_owner = previous.1;
        self.trait_name = previous.2;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        let previous = std::mem::replace(&mut self.name, item.sig.ident.to_string());
        visit::visit_impl_item_fn(self, item);
        self.name = previous;
    }
    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*expr.func {
            let parts: Vec<_> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let name = parts.join("::");
            if parts.iter().any(|part| part == "file_reads") {
                self.count_one();
            } else if parts.last().is_some_and(|part| {
                matches!(
                    part.as_str(),
                    "open"
                        | "read"
                        | "read_to_string"
                        | "read_to_end"
                        | "read_exact"
                        | "read_vectored"
                        | "load_font_file"
                        | "load_system_fonts"
                )
            }) {
                self.record(&name);
            }
        }
        visit::visit_expr_call(self, expr);
    }
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let name = expr.method.to_string();
        if name == "add"
            && let syn::Expr::Path(path) = &*expr.receiver
            && path
                .path
                .segments
                .iter()
                .any(|part| part.ident == "file_reads")
        {
            self.count_one();
        }
        if matches!(
            name.as_str(),
            "open"
                | "read"
                | "read_to_end"
                | "read_to_string"
                | "read_exact"
                | "read_vectored"
                | "load_font_file"
                | "load_system_fonts"
                | "output"
        ) {
            self.record(&format!(".{name}"));
        }
        visit::visit_expr_method_call(self, expr);
    }
}

fn scan(path: &Path, found: &mut BTreeMap<Door, usize>, counted: &mut BTreeMap<Counted, usize>) {
    let source = std::fs::read_to_string(path).unwrap();
    let syntax = syn::parse_file(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut doors = Doors::default();
    doors.visit_file(&syntax);
    let relative = path
        .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    for ((owner, door), count) in doors.found {
        *found
            .entry(Door {
                file: relative.clone(),
                owner,
                door,
            })
            .or_default() += count;
    }
    for (owner, count) in doors.counted {
        *counted
            .entry(Counted {
                file: relative.clone(),
                owner,
            })
            .or_default() += count;
    }
    let directory = if matches!(
        path.file_name().unwrap().to_str().unwrap(),
        "main.rs" | "mod.rs"
    ) {
        path.parent().unwrap().to_owned()
    } else {
        path.with_extension("")
    };
    for (name, explicit) in doors.modules {
        let child = if let Some(explicit) = explicit {
            path.parent().unwrap().join(explicit)
        } else {
            let sibling = directory.join(format!("{name}.rs"));
            if sibling.exists() {
                sibling
            } else {
                directory.join(name).join("mod.rs")
            }
        };
        scan(&child, found, counted);
    }
}

/// The whole walk, from the crate root through every product `mod`.
fn walk() -> (BTreeMap<Door, usize>, BTreeMap<Counted, usize>) {
    let mut found = BTreeMap::new();
    let mut counted = BTreeMap::new();
    scan(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
        &mut found,
        &mut counted,
    );
    (found, counted)
}

/// The manifest, as the map the walk is compared with.
fn manifest() -> BTreeMap<String, usize> {
    include_str!("file_reads_doors.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (count, door) = line.split_once(' ').expect("count and door");
            (door.to_owned(), count.parse().unwrap())
        })
        .collect()
}

/// The verb a door spelling ends in — `open` for `.open`, for a bare `open` and
/// for `persist::SettingsStore::open`. `None` for the one door that is not a
/// call, the renamed read import.
fn verb_of(door: &str) -> Option<&str> {
    if door.contains(' ') {
        return None;
    }
    Some(
        door.rsplit("::")
            .next()
            .unwrap_or(door)
            .trim_start_matches('.'),
    )
}

/// **The owners Step 2a moves** — the sixteen methods of the two
/// `impl Runtime<'_>` blocks that hold a door, carrying twenty-one rows between
/// them. P6's subject, and nothing else: the rest of the manifest is P16's.
const MOVING_OWNERS: [&str; 16] = [
    "commit_web_page",
    "create",
    "land_page_source_on",
    "land_preview_source_on",
    "open_rename",
    "open_search",
    "open_web_page_on",
    "place_float",
    "play_video_file_on",
    "pop_out_preview",
    "promote_file_peek",
    "raise_dirty_gate",
    "raise_first_run_if_due",
    "raise_psreadline_invite_if_due",
    "settings_layout",
    "turn",
];

/// This crate, indexed once per process — the workspace read, this package's
/// own `src/` declared as the universe and lowered on the first ask.
fn source_index() -> &'static bt_source::Index {
    bt_source::Index::of_package("bt-app")
}

/// How many times a pattern stands inside one item, asked of the crate.
///
/// The needle records no site on purpose: the scope is an item of the product,
/// so nothing this module writes can be inside it and there is nothing to
/// exclude.
fn occurrences_inside(owner: &Owner, pattern: bt_source::Pattern) -> usize {
    let search = bt_source::Search::new(
        bt_source::Needle::new(pattern),
        bt_source::View::Identifiers,
    )
    .in_scope(bt_source::Scope::Item(owner.query()));
    source_index()
        .search(&search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .len()
}

#[test]
fn file_reads_every_product_content_door_has_a_lane() {
    let (walked, counted) = walk();
    let mut actual: BTreeMap<String, usize> = BTreeMap::new();
    for (door, count) in &walked {
        *actual
            .entry(format!("{}: {}: {}", door.file, door.owner.name, door.door))
            .or_default() += count;
    }
    assert_eq!(
        actual,
        manifest(),
        "a content or generic read/open door changed; classify it and pin its counting owner"
    );
    let adapters: BTreeSet<String> = counted
        .keys()
        .map(|entry| format!("{}: {}", entry.file, entry.owner.name))
        .collect();
    for owner in [
        "preview.rs: read_up_to",
        "pdf.rs: read_capped",
        "pdf.rs: page_count",
        "attention_words.rs: lede_in_tail",
        "attention_wire.rs: payload_on_stdin",
        "git.rs: drain",
        "shell_integration.rs: read_profile_for_edit",
        "animation.rs: read",
    ] {
        assert!(
            adapters.contains(owner),
            "{owner} lost its counting adapter"
        );
    }
    let animation = include_str!("animation.rs");
    assert!(animation.contains("file_reads::LEDGER.add("));
    assert!(animation.contains("Lane::Animation"));
    assert!(animation.contains("Lane::Peek"));
}

/// **The equivalence of §6.0 rule 3**, for the twenty-one rows P6 re-keys.
///
/// The walk says a door was written in `main.rs`, inside a function of some
/// name. The crate is asked the same question as an item: which declaration
/// carries that name, what type owns it, which file it is written in, and how
/// many times the verb is called inside its bytes. The two readings have to
/// agree on every row, and a disagreement is a finding and not a number to
/// adjust.
#[test]
fn file_reads_every_item_keyed_door_answers_to_one_item() {
    let (walked, _) = walk();
    let index = source_index();
    let moving: BTreeSet<&str> = MOVING_OWNERS.into_iter().collect();
    let mut rows = 0_usize;
    let mut owners: BTreeMap<Owner, BTreeMap<String, usize>> = BTreeMap::new();
    for (door, count) in &walked {
        if door.file != "main.rs" || !moving.contains(door.owner.name.as_str()) {
            continue;
        }
        rows += 1;
        *owners
            .entry(door.owner.clone())
            .or_default()
            .entry(door.door.clone())
            .or_default() += count;
    }
    assert_eq!(rows, 21, "P6's rows");
    assert_eq!(owners.len(), 16, "P6's owners");

    for (owner, doors) in &owners {
        // The item exists, once. This is what refuses a key that names two
        // declarations instead of merging them.
        let record = index
            .one(&owner.query())
            .unwrap_or_else(|failure| panic!("{failure}"));
        // The old reading and the new one name the same declaration.
        assert_eq!(
            record.name(),
            owner.name,
            "{} answers to another name",
            owner.key()
        );
        assert_eq!(
            record.type_owner(),
            Some("Runtime"),
            "{} is not a method of `Runtime`",
            owner.key()
        );
        assert_eq!(
            index
                .file_of(record)
                .path()
                .file_name()
                .and_then(std::ffi::OsStr::to_str),
            Some("main.rs"),
            "{} is not written where its row says",
            owner.key()
        );
        // Every call of the verb inside the item's bytes, whichever way it is
        // written: the completeness half, which goes red on a door the walk
        // never saw as well as on one it saw twice.
        let mut by_verb: BTreeMap<&str, usize> = BTreeMap::new();
        for (door, count) in doors {
            let Some(verb) = verb_of(door) else {
                panic!("{door} is not a call");
            };
            *by_verb.entry(verb).or_default() += count;
        }
        for (verb, total) in by_verb {
            assert_eq!(
                occurrences_inside(owner, bt_source::Pattern::call(verb)),
                total,
                "`{verb}(` inside {}",
                owner.key()
            );
        }
        // And the row's own spelling, where the door is a path: `create`'s six
        // opens are six different stores, and a verb total cannot say which.
        for (door, count) in doors {
            if door.contains("::") {
                assert_eq!(
                    occurrences_inside(owner, bt_source::Pattern::path(door)),
                    *count,
                    "`{door}` inside {}",
                    owner.key()
                );
            }
        }
    }
}

#[test]
fn file_reads_source_guard_sees_reads_after_tests_and_ignores_comment_decoys() {
    let syntax = syn::parse_file(
        r#"
        // fs::read("decoy");
        #[cfg(test)] mod tests { fn fixture() { fs::read("fixture"); } }
        #[cfg(not(test))] fn shipping() { std::fs::read("also unlabelled"); }
        fn later() { let _ = "File::open"; std::fs::read("unlabelled"); }
    "#,
    )
    .unwrap();
    let mut doors = Doors::default();
    doors.visit_file(&syntax);
    let expected: BTreeMap<String, usize> = [
        ("later: std::fs::read".to_owned(), 1),
        ("shipping: std::fs::read".to_owned(), 1),
    ]
    .into();
    let actual: BTreeMap<String, usize> = doors
        .found
        .into_iter()
        .map(|((owner, door), count)| (format!("{}: {door}", owner.key()), count))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn file_reads_reporting_borrows_the_watchdog_and_installs_no_wake() {
    let clock = include_str!("file_reads.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    for forbidden in [
        "thread::spawn",
        "spawn_at_priority",
        "Instant::now",
        "ControlFlow::",
        "sleep(",
        "WaitUntil",
    ] {
        assert!(
            !clock.contains(forbidden),
            "the reporting clock acquired its own wake: {forbidden}"
        );
    }
    let watchdog = include_str!("hang_watch.rs");
    let start = watchdog.find("fn watch_forever(").unwrap();
    let body = &watchdog[start..watchdog[start..].find("\n}\n").unwrap() + start];
    assert_eq!(body.matches("reads.tick(").count(), 1);
    assert!(body.contains("match watch.poll(now_ms,"));
    assert!(body.contains("reads.tick(\n            now_ms,"));
    assert!(clock.contains("!ledger.changed() && !trace && !self.reporter.active()"));
}
