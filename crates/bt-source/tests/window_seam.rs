//! **The window seam is the only road from the window's own code to the window**
//! (T-HEADLESS-RUNTIME-SEAM; `docs/ARCHITECTURE-GUARDS.md`, "Window seam").
//!
//! `Runtime`, `WindowRuntime`, `App`, `FolioApp` and the rest hold their window through a backend
//! parameter bounded by `crate::window_seam::Backend`: code written for that parameter sees the
//! window, the renderer, the compositor and the event loop only as the seam's door traits, so the
//! compiler is the reader of every call it makes. What the compiler cannot say is the other road —
//! code written for the **native** backend, where the concrete `winit` window, `WindowRenderer`
//! and `Compositor` are in reach and no test can follow — and a door of the seam implemented
//! somewhere the seam does not list. This guard holds both, by item identity:
//!
//! * **the backend's traits** are the seam's `Backend` and every first-party trait that has it
//!   among its supertraits, however deep; a bound is resolved through `use` items, globs and
//!   renames by [`TraitResolver`], so a trait of the same name elsewhere is not one;
//! * **the holders** are the product structs, enums and unions with a type parameter bounded by
//!   one of those traits, and where that parameter stands among their type parameters;
//! * **a holder named for the native backend** — its backend argument left to its default, or a
//!   type that is not a backend parameter in scope — is refused wherever it is written, unless the
//!   native list (`docs/plans/window-seam-native.tsv`) names the place with its reason: in an
//!   `impl` of a holder (an inherent block's methods by name, a trait block by its trait), in any
//!   other function's signature or body, and in a `const` or `static`;
//! * **a shape the guard does not read is refused, by name**: a type alias that names a holder (an
//!   `impl` through it would be written for a name the guard cannot see), and an `impl` whose self
//!   type names a holder for the native backend without being that holder's path;
//! * every product `impl` of a door trait declared in `crate::window_seam` is written there;
//! * every row of the native list is still used (and `scripts/ci/check-window-seam-native.ps1`
//!   holds the list to the merge base's: it only shrinks).
//!
//! The fixture under `tests/fixtures/window_seam/` plants one of each refusal; the last test holds
//! `bt-app` itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bt_source::{
    Certainty, DiskScope, ImplRecord, Index, ItemKind, Occurrence, Span, TargetId, TargetKind,
    TargetRoot, TraitId, TraitResolver, Universe, Vendor, report,
};
use syn::visit::Visit;

/// The module the seam is declared in.
const SEAM: &str = "crate::window_seam";

/// **The places a package may name a holder for the native backend**, each `(kind, owner, name)`:
/// `method` — an inherent block's method (`owner` the holder); `trait` — a trait block (`owner`
/// the holder, `name` the trait's last path segment); `function` — any other function (`owner`
/// the type of its `impl`, or its module); `const` — a `const` or `static` (`owner` its file).
type Allowed = BTreeSet<(String, String, String)>;

/// The native list of `bt-app`, read from its committed file.
fn bt_app_native_list() -> Allowed {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("plans")
        .join("window-seam-native.tsv");
    let text = std::fs::read_to_string(&path).expect("the native list is committed");
    let mut rows = Allowed::new();
    let mut header = false;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            header = true;
            continue;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        let [kind, owner, name, reason] = columns[..] else {
            panic!("a native-list row is four columns: {line}");
        };
        assert!(
            ["method", "trait", "function", "const"].contains(&kind),
            "a native-list row's kind is method, trait, function or const: {line}"
        );
        assert!(
            !reason.trim().is_empty(),
            "a native-list row says why: {line}"
        );
        assert!(
            rows.insert((kind.to_owned(), owner.to_owned(), name.to_owned())),
            "a native-list row is written once: {line}"
        );
    }
    rows
}

/// Whether a build of the shipped program contains `span`.
fn in_the_product(index: &Index, span: Span) -> bool {
    Occurrence {
        span,
        certainty: Certainty::Resolved,
    }
    .in_the_product(index)
}

/// The path segments of a bound, without arguments.
fn segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

/// **The backend's traits**: the seam's `Backend` and every first-party trait of `package` with
/// one of them among its supertraits, followed to a fixed point.
fn backend_traits(index: &Index, package: &str, resolver: &TraitResolver<'_>) -> BTreeSet<TraitId> {
    let mut found = BTreeSet::from([TraitId {
        package: package.to_owned(),
        module_path: SEAM.to_owned(),
        name: "Backend".to_owned(),
    }]);
    loop {
        let before = found.len();
        for declaration in index.traits() {
            for id in resolver.identities(package, declaration) {
                if !found.contains(&id)
                    && resolver
                        .supertraits(&id)
                        .iter()
                        .any(|parent| found.contains(parent))
                {
                    found.insert(id);
                }
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// The type parameters of `generics` that are bounded by one of `backends`, written in `module`:
/// in the parameter list and in the `where` clause.
fn backend_parameters(
    generics: &syn::Generics,
    resolver: &TraitResolver<'_>,
    package: &str,
    module: &str,
    backends: &BTreeSet<TraitId>,
) -> BTreeSet<String> {
    let names = |bounds: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::Token![+]>| {
        bounds.iter().any(|bound| match bound {
            syn::TypeParamBound::Trait(bound) => resolver
                .resolve(package, module, &segments(&bound.path))
                .iter()
                .any(|id| backends.contains(id)),
            _ => false,
        })
    };
    let mut found = BTreeSet::new();
    for param in generics.type_params() {
        if names(&param.bounds) {
            found.insert(param.ident.to_string());
        }
    }
    if let Some(clause) = &generics.where_clause {
        for predicate in &clause.predicates {
            let syn::WherePredicate::Type(predicate) = predicate else {
                continue;
            };
            let syn::Type::Path(bounded) = &predicate.bounded_ty else {
                continue;
            };
            if let Some(ident) = bounded.path.get_ident()
                && names(&predicate.bounds)
            {
                found.insert(ident.to_string());
            }
        }
    }
    found
}

/// **Every holder named for the native backend in a stretch of code**: each type path whose last
/// segment is a holder and whose backend argument is missing or is not one of `generic`.
struct NativeMentions<'a> {
    holders: &'a BTreeMap<String, usize>,
    generic: &'a BTreeSet<String>,
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for NativeMentions<'_> {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if let Some(last) = node.path.segments.last()
            && let Some(&position) = self.holders.get(&last.ident.to_string())
            && !written_for_a_parameter(last, position, self.generic)
        {
            self.found.insert(last.ident.to_string());
        }
        syn::visit::visit_type_path(self, node);
    }
}

/// Whether a holder's path segment is written for one of `generic` at the backend's `position`.
fn written_for_a_parameter(
    segment: &syn::PathSegment,
    position: usize,
    generic: &BTreeSet<String>,
) -> bool {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    let Some(syn::Type::Path(argument)) = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            syn::GenericArgument::Type(argument) => Some(argument),
            _ => None,
        })
        .nth(position)
    else {
        return false;
    };
    argument
        .path
        .get_ident()
        .is_some_and(|ident| generic.contains(&ident.to_string()))
}

/// The holders `node` names for the native backend, with `generic` the backend parameters in scope.
fn native_mentions(
    holders: &BTreeMap<String, usize>,
    generic: &BTreeSet<String>,
    visit: impl FnOnce(&mut NativeMentions<'_>),
) -> BTreeSet<String> {
    let mut mentions = NativeMentions {
        holders,
        generic,
        found: BTreeSet::new(),
    };
    visit(&mut mentions);
    mentions.found
}

/// The header of an `impl` block, parsed: its generics, self type and trait.
fn impl_header(index: &Index, block: &ImplRecord) -> syn::ItemImpl {
    let whole = index.text(block.whole());
    let header = &whole[..block.body().start() - block.whole().start()];
    syn::parse_str(&format!("{header} {{}}"))
        .unwrap_or_else(|error| panic!("an impl header parses: {error}\n{header}"))
}

/// The holders of `package`, by name, with where the backend stands among their type parameters.
fn holders_of(
    index: &Index,
    package: &str,
    resolver: &TraitResolver<'_>,
    backends: &BTreeSet<TraitId>,
) -> BTreeMap<String, usize> {
    let mut holders = BTreeMap::new();
    for item in index.items() {
        if !matches!(
            item.kind(),
            ItemKind::Struct | ItemKind::Enum | ItemKind::Union
        ) || !in_the_product(index, item.whole())
        {
            continue;
        }
        let parsed: syn::Item = syn::parse_str(index.text(item.whole()))
            .unwrap_or_else(|error| panic!("the type `{}` parses: {error}", item.name()));
        let generics = match &parsed {
            syn::Item::Struct(item) => &item.generics,
            syn::Item::Enum(item) => &item.generics,
            syn::Item::Union(item) => &item.generics,
            _ => continue,
        };
        for module in item.module_paths() {
            let held = backend_parameters(generics, resolver, package, module, backends);
            if let Some(position) = generics
                .type_params()
                .position(|param| held.contains(&param.ident.to_string()))
            {
                holders.insert(item.name().to_owned(), position);
            }
        }
    }
    holders
}

/// **Every refusal of the rule in `package`**, one sentence each, and the rows of `allowed` that
/// nothing used.
fn judge(index: &Index, package: &str, allowed: &Allowed) -> (Vec<String>, Vec<String>) {
    let resolver = TraitResolver::new(&[(package, index)]);
    let backends = backend_traits(index, package, &resolver);
    let holders = holders_of(index, package, &resolver, &backends);
    assert!(
        !holders.is_empty(),
        "no type of `{package}` holds a backend: the guard would hold nothing"
    );
    let mut refusals = Vec::new();
    let mut used: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut allow = |kind: &str, owner: &str, name: &str| {
        let row = (kind.to_owned(), owner.to_owned(), name.to_owned());
        let listed = allowed.contains(&row);
        if listed {
            used.insert(row);
        }
        listed
    };

    // The impl blocks: the doors of the seam, and the blocks of the holders.
    let mut holder_blocks: Vec<Span> = Vec::new();
    let mut block_generics: Vec<(Span, BTreeSet<String>)> = Vec::new();
    for block in index.impls() {
        if !in_the_product(index, block.whole()) {
            continue;
        }
        let modules = block.module_paths();
        let parsed = impl_header(index, block);
        let generic: BTreeSet<String> = modules
            .iter()
            .flat_map(|module| {
                backend_parameters(&parsed.generics, &resolver, package, module, &backends)
            })
            .collect();
        block_generics.push((block.body(), generic.clone()));

        for door in resolver.implemented(package, block) {
            if door.package == package && door.module_path == SEAM && !modules.contains(&SEAM) {
                refusals.push(format!(
                    "{door} is implemented for `{}` in {} — a door of the seam is implemented in \
                     the seam",
                    block.type_owner(),
                    modules.join(", ")
                ));
            }
        }

        let owner = block.type_owner();
        let own_path = match &*parsed.self_ty {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .filter(|last| holders.contains_key(&last.ident.to_string())),
            _ => None,
        };
        let Some(last) = own_path else {
            // Not a holder's own block: whatever it names for the native backend in its self type
            // is a shape this guard does not read.
            let named = native_mentions(&holders, &generic, |visit| {
                visit.visit_type(&parsed.self_ty)
            });
            if !named.is_empty() {
                refusals.push(format!(
                    "an impl in {} is written for a type that names {} for the native backend \
                     without being its path — a shape the window-seam guard does not read",
                    modules.join(", "),
                    named.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
            continue;
        };
        holder_blocks.push(block.body());
        if written_for_a_parameter(last, holders[&last.ident.to_string()], &generic) {
            continue;
        }
        let holder = last.ident.to_string();
        if let Some(path) = block.trait_path() {
            let name = path.last().map_or("", String::as_str);
            if !allow("trait", &holder, name) {
                refusals.push(format!(
                    "`impl {name} for {holder}` is written for the native backend in {} — the \
                     window is reached through the seam",
                    modules.join(", ")
                ));
            }
            continue;
        }
        let whole: syn::ItemImpl = syn::parse_str(index.text(block.whole()))
            .unwrap_or_else(|error| panic!("a native `{owner}` block parses: {error}"));
        for member in &whole.items {
            let syn::ImplItem::Fn(method) = member else {
                continue;
            };
            let name = method.sig.ident.to_string();
            if !allow("method", &holder, &name) {
                refusals.push(format!(
                    "`{holder}::{name}` is written for the native backend in {} — the window is \
                     reached through the seam",
                    modules.join(", ")
                ));
            }
        }
    }

    // Every other function: its signature and its body.
    for item in index.items() {
        if !matches!(
            item.kind(),
            ItemKind::Function | ItemKind::AssociatedFunction
        ) || !in_the_product(index, item.whole())
        {
            continue;
        }
        let at = item.whole().start();
        // The seam itself is where the native backend meets the parameter: its `Backend` trait
        // names the holders for `Self`, and `Native`'s impl of it is the native side by definition.
        if holder_blocks.iter().any(|body| body.holds(at)) || item.module_paths().contains(&SEAM) {
            continue;
        }
        let text = index.text(item.whole());
        let parsed: syn::ItemFn = match syn::parse_str(text) {
            Ok(parsed) => parsed,
            Err(_) => match syn::parse_str::<syn::TraitItemFn>(text) {
                Ok(declared) => syn::ItemFn {
                    attrs: declared.attrs,
                    vis: syn::Visibility::Inherited,
                    sig: declared.sig,
                    block: Box::new(declared.default.unwrap_or_else(|| syn::parse_quote!({}))),
                },
                Err(error) => panic!("the function `{}` parses: {error}", item.name()),
            },
        };
        let mut generic: BTreeSet<String> = item
            .module_paths()
            .into_iter()
            .flat_map(|module| {
                backend_parameters(&parsed.sig.generics, &resolver, package, module, &backends)
            })
            .collect();
        for (body, block) in &block_generics {
            if body.holds(at) {
                generic.extend(block.iter().cloned());
            }
        }
        let named = native_mentions(&holders, &generic, |visit| visit.visit_item_fn(&parsed));
        if named.is_empty() {
            continue;
        }
        let owner = item
            .type_owner()
            .map_or_else(|| item.module_paths().join(", "), str::to_owned);
        if !allow("function", &owner, item.name()) {
            refusals.push(format!(
                "`{owner}::{}` names {} for the native backend — the window is reached through \
                 the seam",
                item.name(),
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    // What only the files say: type aliases, and `const` and `static` items.
    for file in index.files() {
        if !file.permits_product() {
            continue;
        }
        let Ok(parsed) = syn::parse_file(index.text(file.span())) else {
            continue; // bt-source's own gates refuse a product file that does not parse.
        };
        let name = file
            .path()
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        file_items(
            &parsed.items,
            &holders,
            &mut |kind, item_name, what| match kind {
                "alias" => refusals.push(format!(
                    "the type alias `{item_name}` in {name} names {what} — the window-seam guard \
                     reads holders by their own name; write the holder"
                )),
                _ => {
                    if !allow("const", &name, item_name) {
                        refusals.push(format!(
                            "the `const` or `static` `{item_name}` in {name} names {what} for the \
                             native backend — the window is reached through the seam"
                        ));
                    }
                }
            },
        );
    }

    let unused = allowed
        .iter()
        .filter(|row| !used.contains(*row))
        .map(|(kind, owner, name)| format!("{kind} {owner} {name}"))
        .collect();
    refusals.sort();
    refusals.dedup();
    (refusals, unused)
}

/// The type aliases naming a holder at all, and the `const` and `static` items naming one for the
/// native backend, among `items` and their inline modules — a `#[cfg(test)]` module left out.
fn file_items(
    items: &[syn::Item],
    holders: &BTreeMap<String, usize>,
    found: &mut impl FnMut(&str, &str, &str),
) {
    let none = BTreeSet::new();
    for item in items {
        match item {
            syn::Item::Type(alias) => {
                // Any holder at all: an alias written for a parameter is still a name the guard
                // does not read.
                let mut any = Vec::new();
                AnyHolder(holders, &mut any).visit_type(&alias.ty);
                if !any.is_empty() {
                    found("alias", &alias.ident.to_string(), &any.join(", "));
                }
            }
            syn::Item::Const(constant) => {
                let named =
                    native_mentions(holders, &none, |visit| visit.visit_item_const(constant));
                if !named.is_empty() {
                    let names = named.into_iter().collect::<Vec<_>>().join(", ");
                    found("const", &constant.ident.to_string(), &names);
                }
            }
            syn::Item::Static(fixed) => {
                let named = native_mentions(holders, &none, |visit| visit.visit_item_static(fixed));
                if !named.is_empty() {
                    let names = named.into_iter().collect::<Vec<_>>().join(", ");
                    found("const", &fixed.ident.to_string(), &names);
                }
            }
            syn::Item::Mod(module) => {
                let test_only = module.attrs.iter().any(|attribute| {
                    attribute.path().is_ident("cfg")
                        && attribute
                            .parse_args::<syn::Meta>()
                            .is_ok_and(|meta| meta.path().is_ident("test"))
                });
                if let Some((_, inner)) = &module.content
                    && !test_only
                {
                    file_items(inner, holders, found);
                }
            }
            _ => {}
        }
    }
}

/// Every holder a type names, by name.
struct AnyHolder<'a>(&'a BTreeMap<String, usize>, &'a mut Vec<String>);

impl<'ast> Visit<'ast> for AnyHolder<'_> {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if let Some(last) = node.path.segments.last()
            && self.0.contains_key(&last.ident.to_string())
        {
            self.1.push(last.ident.to_string());
        }
        syn::visit::visit_type_path(self, node);
    }
}

/// The fixture crate.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("window_seam");
    let universe = Universe::declare(
        "the window-seam fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "window-seam".to_owned(),
                kind: TargetKind::Library,
                name: "window_seam".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

fn row(kind: &str, owner: &str, name: &str) -> (String, String, String) {
    (kind.to_owned(), owner.to_owned(), name.to_owned())
}

/// RED (planted) — **each road around the window seam is named, and nothing else is.** The
/// fixture plants: a native block holding a method off the list; a trait written for the native
/// holder alone; a door implemented outside the seam; a holder whose bound is a supertrait of the
/// backend, with a native block; a type alias of a holder, with a block written through it; an
/// impl whose self type names a holder natively without being its path; a free function over
/// native holders; a `const` naming one; and a list row nothing uses. The generic blocks (the
/// bound written directly, through the prelude's glob, through a rename, in a `where` clause and
/// through a supertrait), the listed native method, function and `const`, a type with no backend
/// and a test module's native block pass.
#[test]
fn every_road_around_the_window_seam_is_named() {
    let index = fixture();
    let allowed = Allowed::from([
        row("method", "Runtime", "create"),
        row("method", "Runtime", "retired"),
        row("function", "crate", "listed_rebuild"),
        row("const", "lib.rs", "_"),
    ]);
    let (refusals, unused) = judge(&index, "window-seam", &allowed);
    assert_eq!(
        refusals,
        [
            "`Indirect::sneaks_through_a_supertrait` is written for the native backend in \
             crate::runtime — the window is reached through the seam",
            "`Runtime::sneaks_past_the_seam` is written for the native backend in crate::runtime \
             — the window is reached through the seam",
            "`crate::runtime::rebuild_every_window` names Runtime for the native backend — the \
             window is reached through the seam",
            "`impl Default for Shell` is written for the native backend in crate::runtime — the \
             window is reached through the seam",
            "an impl in crate::runtime is written for a type that names Runtime for the native \
             backend without being its path — a shape the window-seam guard does not read",
            "the `const` or `static` `PLANTED` in runtime.rs names Runtime for the native backend \
             — the window is reached through the seam",
            "the type alias `NativeRuntime` in runtime.rs names Runtime — the window-seam guard \
             reads holders by their own name; write the holder",
            "window-seam crate::window_seam::WindowHost is implemented for `Stray` in \
             crate::elsewhere — a door of the seam is implemented in the seam",
        ],
        "the plants, and only them"
    );
    assert_eq!(
        unused,
        ["method Runtime retired"],
        "a list row nothing uses is named, so the list only shrinks"
    );
}

/// GUARD — **`bt-app` reaches its window only through the seam**: nothing names a holder for the
/// native backend but the places `docs/plans/window-seam-native.tsv` lists, no shape the guard
/// does not read is written, every door of the seam is implemented in the seam, and every row of
/// the list is used.
#[test]
fn bt_app_reaches_its_window_only_through_the_seam() {
    let (refusals, unused) = judge(Index::of_package("bt-app"), "bt-app", &bt_app_native_list());
    assert!(
        refusals.is_empty(),
        "the window seam is the only road to the window:\n{}",
        refusals.join("\n")
    );
    assert!(
        unused.is_empty(),
        "native-list rows nothing writes any more — take them off the list: {unused:?}"
    );
}
