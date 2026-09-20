//! AST source pin: test items, comments and strings cannot hide product reads.
//! The small manifest below inventories generic Read/OpenOptions doors, whose
//! receiver types Rust syntax alone cannot establish. Their exact counts and
//! the counting adapter in the same owner are pinned together.

use std::collections::BTreeMap;
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

#[derive(Default)]
struct Doors {
    owner: String,
    found: BTreeMap<String, usize>,
    counted: BTreeMap<String, usize>,
    modules: Vec<(String, Option<String>)>,
}

impl Doors {
    fn record(&mut self, door: &str) {
        *self
            .found
            .entry(format!("{}: {door}", self.owner))
            .or_default() += 1;
    }
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
        if !test_only(&item.attrs) {
            visit::visit_item_impl(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        let previous = std::mem::replace(&mut self.owner, item.sig.ident.to_string());
        visit::visit_item_fn(self, item);
        self.owner = previous;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_only(&item.attrs) {
            return;
        }
        let previous = std::mem::replace(&mut self.owner, item.sig.ident.to_string());
        visit::visit_impl_item_fn(self, item);
        self.owner = previous;
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
                *self.counted.entry(self.owner.clone()).or_default() += 1;
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
            *self.counted.entry(self.owner.clone()).or_default() += 1;
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

fn scan(path: &Path, found: &mut BTreeMap<String, usize>, counted: &mut BTreeMap<String, usize>) {
    let source = std::fs::read_to_string(path).unwrap();
    let syntax = syn::parse_file(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut doors = Doors::default();
    doors.visit_file(&syntax);
    let relative = path
        .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    for (door, count) in doors.found {
        found.insert(format!("{relative}: {door}"), count);
    }
    for (owner, count) in doors.counted {
        counted.insert(format!("{relative}: {owner}"), count);
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

#[test]
fn file_reads_every_product_content_door_has_a_lane() {
    let mut actual = BTreeMap::new();
    let mut counted = BTreeMap::new();
    scan(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
        &mut actual,
        &mut counted,
    );
    let expected: BTreeMap<String, usize> = include_str!("file_reads_doors.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (count, door) = line.split_once(' ').expect("count and door");
            (door.to_owned(), count.parse().unwrap())
        })
        .collect();
    assert_eq!(
        actual, expected,
        "a content or generic read/open door changed; classify it and pin its counting owner"
    );
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
            counted.contains_key(owner),
            "{owner} lost its counting adapter"
        );
    }
    let animation = include_str!("animation.rs");
    assert!(animation.contains("file_reads::LEDGER.add("));
    assert!(animation.contains("Lane::Animation"));
    assert!(animation.contains("Lane::Peek"));
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
    assert_eq!(doors.found, expected);
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
