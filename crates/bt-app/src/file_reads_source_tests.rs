//! AST source pin: test items, comments and strings cannot hide product reads.
//! The small manifest below inventories generic Read/OpenOptions doors, whose
//! receiver types Rust syntax alone cannot establish. Their exact counts and
//! the counting adapter in the same owner are pinned together.
//!
//! # Who wrote a door, and how a row says so
//!
//! A row of `file_reads_doors.txt` is `<count> <key>`, and the key names the
//! owner one of two ways.
//!
//! * **Item-keyed** — `Runtime::turn: .open`. The owner is the identity of the
//!   plan's §2.4 without its module path: the type the `impl` is for, the trait
//!   if there is one, the name. It says nothing about which file the item is
//!   written in, so it does not change when the item moves.
//! * **File-keyed** — `trace.rs: create: .open`. The old spelling: the file,
//!   the bare name of the function the door stands in, the door. A row like
//!   this goes quiet the day its owner moves to another file — the walk finds
//!   the door under a different name and the row it used to match is simply
//!   absent from a map nobody compares by owner. These are the rows still on
//!   `docs/plans/MIGRATION-DEBT.tsv`, and P16 is the ticket that re-keys them.
//!
//! A file-keyed key's first segment ends in `.rs`, which is how the two are
//! told apart here and in the manifest.
//!
//! **The module path is deliberately not in an item key.** Step 2a moves the
//! two `impl Runtime<'_>` blocks into a newly declared `src/runtime/`, where
//! the same method answers to `crate::runtime` instead of `crate`; a key
//! carrying the module path would change on the one move it exists to survive.
//! `bt-source` drops the self type's qualification for exactly that reason
//! ([`bt_source::ItemQuery`]). What keeps the shorter key honest is that it is
//! not merely asserted here:
//! [`file_reads_every_item_keyed_door_answers_to_one_item`] puts every one of
//! them to the crate, and a key naming no declaration, or two, is a loud
//! refusal rather than a quiet merge.

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

    /// The owner a key spells, read back. `Owner::parse(key).key() == key` for
    /// every key this module writes, which is asserted rather than assumed.
    fn parse(key: &str) -> Self {
        if let Some(rest) = key.strip_prefix('<') {
            let (type_owner, rest) = rest.split_once(" as ").expect("<Type as Trait>::name");
            let (trait_name, name) = rest.split_once(">::").expect("<Type as Trait>::name");
            return Self {
                type_owner: Some(type_owner.to_owned()),
                trait_name: Some(trait_name.to_owned()),
                name: name.to_owned(),
            };
        }
        match key.rsplit_once("::") {
            Some((type_owner, name)) => Self {
                type_owner: Some(type_owner.to_owned()),
                trait_name: None,
                name: name.to_owned(),
            },
            None => Self {
                type_owner: None,
                trait_name: None,
                name: key.to_owned(),
            },
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

fn scan(path: &Path, found: &mut BTreeMap<Door, usize>, counted: &mut BTreeMap<Owner, usize>) {
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
        *counted.entry(owner).or_default() += count;
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
fn walk() -> (BTreeMap<Door, usize>, BTreeMap<Owner, usize>) {
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

/// **The owners the manifest keys by item** — every row whose first segment is
/// not a file name. The manifest is the list: a row is migrated by being
/// written that way, and there is no second place saying which.
fn item_keyed_owners(manifest: &BTreeMap<String, usize>) -> BTreeSet<String> {
    manifest
        .keys()
        .filter_map(|key| key.split_once(": "))
        .filter(|(head, _)| !head.ends_with(".rs"))
        .map(|(head, _)| head.to_owned())
        .collect()
}

/// The key one walked door is compared under: its item where the manifest has
/// been re-keyed, the file it happens to be written in where it has not.
fn key_of(door: &Door, item_keyed: &BTreeSet<String>) -> String {
    let item = door.owner.key();
    if item_keyed.contains(&item) {
        format!("{item}: {}", door.door)
    } else {
        format!("{}: {}: {}", door.file, door.owner.name, door.door)
    }
}

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
    let expected = manifest();
    let item_keyed = item_keyed_owners(&expected);
    let mut actual: BTreeMap<String, usize> = BTreeMap::new();
    for (door, count) in &walked {
        *actual.entry(key_of(door, &item_keyed)).or_default() += count;
    }
    assert_eq!(
        actual, expected,
        "a content or generic read/open door changed; classify it and pin its counting owner"
    );
    // The counting adapters are named by item too, and each is put to the crate
    // before it is looked for: `contains` over a merged key would be answered
    // by any other item of the same name, which is the silent merge an item key
    // exists to make impossible.
    for key in [
        "read_up_to",
        "read_capped",
        "page_count",
        "lede_in_tail",
        "payload_on_stdin",
        "drain",
        "read_profile_for_edit",
        "<FileAnimationSource as Read>::read",
    ] {
        let owner = Owner::parse(key);
        assert_eq!(owner.key(), key, "{key} does not read back as itself");
        source_index()
            .one(&owner.query())
            .unwrap_or_else(|failure| panic!("{failure}"));
        assert!(
            counted.contains_key(&owner),
            "{key} lost its counting adapter"
        );
    }
    let animation = include_str!("animation.rs");
    assert!(animation.contains("file_reads::LEDGER.add("));
    assert!(animation.contains("Lane::Animation"));
    assert!(animation.contains("Lane::Peek"));
}

/// **Every item-keyed row is a fact about an item, asked of the crate.**
///
/// The walk derives the key from the syntax it is standing in. That alone would
/// make the key a string this module invented, so each one is put to
/// `bt-source`: it has to read back as itself, to resolve to exactly one
/// declaration — a key naming two is a refusal naming both, never a merged
/// count — and the doors the walk recorded under it have to be the calls the
/// crate finds inside that declaration's bytes, wherever they are written.
///
/// Nothing here names a file, which is the point: the same assertions hold
/// after the two `impl Runtime<'_>` blocks move.
#[test]
fn file_reads_every_item_keyed_door_answers_to_one_item() {
    let (walked, _) = walk();
    let index = source_index();
    let item_keyed = item_keyed_owners(&manifest());
    assert!(
        !item_keyed.is_empty(),
        "no row is keyed to an item any more; P6 re-keyed twenty-one of them"
    );
    let mut owners: BTreeMap<Owner, BTreeMap<String, usize>> = BTreeMap::new();
    for (door, count) in &walked {
        if item_keyed.contains(&door.owner.key()) {
            *owners
                .entry(door.owner.clone())
                .or_default()
                .entry(door.door.clone())
                .or_default() += count;
        }
    }
    let walked_owners: BTreeSet<String> = owners.keys().map(Owner::key).collect();
    assert_eq!(
        walked_owners, item_keyed,
        "an item-keyed row names an owner this walk found no door in"
    );

    for (owner, doors) in &owners {
        let key = owner.key();
        assert_eq!(
            Owner::parse(&key).key(),
            key,
            "{key} does not read back as itself"
        );
        // The item exists, once. This is what refuses a key that names two
        // declarations instead of merging them.
        index
            .one(&owner.query())
            .unwrap_or_else(|failure| panic!("{failure}"));
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
