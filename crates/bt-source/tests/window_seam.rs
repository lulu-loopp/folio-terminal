//! **The window seam is the only road from the window's own code to the window**
//! (T-HEADLESS-RUNTIME-SEAM; `docs/ARCHITECTURE-GUARDS.md`, "Window seam").
//!
//! `Runtime`, `WindowRuntime`, `App`, `FolioApp` and the rest hold their window through a backend
//! parameter bounded by `crate::window_seam::Backend`: a block written for that parameter sees the
//! window, the renderer, the compositor and the event loop only as the seam's door traits, so the
//! compiler is the reader of every call they make. What the compiler cannot say is the other road
//! — a block written for the **native** backend, where the concrete `winit` window,
//! `WindowRenderer` and `Compositor` are in reach and no test can follow — and a door of the seam
//! implemented somewhere the seam does not list. This guard holds both, by item identity:
//!
//! * the structs that hold a backend are the product structs with a type parameter bounded by the
//!   seam's `Backend` — the bound resolved through `use` items, globs and renames by
//!   [`TraitResolver`], so a trait of the same name elsewhere is not it;
//! * every product `impl` of such a struct is written for a backend parameter (a parameter of the
//!   block bounded by that same trait), or it is a native-only block on [`Allowed`] — each of its
//!   methods by name for an inherent block, the trait for a trait block — and every entry of the
//!   list is still used;
//! * every product `impl` of a door trait declared in `crate::window_seam` is written in
//!   `crate::window_seam`.
//!
//! The fixture under `tests/fixtures/window_seam/` plants one of each refusal; the last test holds
//! `bt-app` itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bt_source::{
    Certainty, DiskScope, Index, ItemKind, Occurrence, Span, TargetId, TargetKind, TargetRoot,
    TraitId, TraitResolver, Universe, Vendor, report,
};

/// The module the seam is declared in.
const SEAM: &str = "crate::window_seam";

/// **The native-only blocks a package may write**: inherent methods by `(type, method)`, trait
/// blocks by `(type, trait)` — the trait's last path segment.
struct Allowed<'a> {
    inherent: &'a [(&'a str, &'a str)],
    traits: &'a [(&'a str, &'a str)],
}

/// The native-only blocks `bt-app` writes, and why each cannot be generic.
const BT_APP: Allowed<'static> = Allowed {
    inherent: &[
        // Builds the GPU device, the `winit` window, the renderer and the compositor that the
        // backend parameter is then instantiated with.
        ("Runtime", "create"),
        // Asks `winit` for a second window and builds its renderer and compositor.
        ("Runtime", "open_window"),
    ],
    traits: &[
        // The `winit` adapter: hands each event to `FolioApp::on_window_event` and the other
        // generic doors, with the `ActiveEventLoop` as the loop door.
        ("FolioApp", "ApplicationHandler"),
    ],
};

/// Whether a build of the shipped program contains `span`.
fn in_the_product(index: &Index, span: Span) -> bool {
    Occurrence {
        span,
        certainty: Certainty::Resolved,
    }
    .in_the_product(index)
}

/// The seam's `Backend`, as an identity of `package`.
fn backend(package: &str) -> TraitId {
    TraitId {
        package: package.to_owned(),
        module_path: SEAM.to_owned(),
        name: "Backend".to_owned(),
    }
}

/// The path segments of a bound, without arguments.
fn segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

/// The type parameters of `generics` that are bounded by `wanted`, written in `module`: in the
/// parameter list and in the `where` clause.
fn backend_parameters(
    generics: &syn::Generics,
    resolver: &TraitResolver<'_>,
    package: &str,
    module: &str,
    wanted: &TraitId,
) -> BTreeSet<String> {
    let names = |bounds: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::Token![+]>| {
        bounds.iter().any(|bound| match bound {
            syn::TypeParamBound::Trait(bound) => resolver
                .resolve(package, module, &segments(&bound.path))
                .contains(wanted),
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

/// **Every refusal of the rule in `package`**, one sentence each, and the entries of `allowed`
/// that nothing used.
fn judge(index: &Index, package: &str, allowed: &Allowed<'_>) -> (Vec<String>, Vec<String>) {
    let resolver = TraitResolver::new(&[(package, index)]);
    let wanted = backend(package);
    let mut refusals = Vec::new();

    // The structs that hold a backend, and where among their type parameters it stands.
    let mut holders: BTreeMap<String, usize> = BTreeMap::new();
    for item in index.items() {
        if item.kind() != ItemKind::Struct || !in_the_product(index, item.whole()) {
            continue;
        }
        let parsed: syn::ItemStruct = syn::parse_str(index.text(item.whole()))
            .unwrap_or_else(|error| panic!("the struct `{}` parses: {error}", item.name()));
        for module in item.module_paths() {
            let held = backend_parameters(&parsed.generics, &resolver, package, module, &wanted);
            if let Some(position) = parsed
                .generics
                .type_params()
                .position(|param| held.contains(&param.ident.to_string()))
            {
                holders.insert(item.name().to_owned(), position);
            }
        }
    }
    assert!(
        !holders.is_empty(),
        "no struct of `{package}` holds a backend bounded by {wanted}: the guard would hold nothing"
    );

    let mut used = BTreeSet::new();
    for block in index.impls() {
        if !in_the_product(index, block.whole()) {
            continue;
        }
        let modules = block.module_paths();
        let whole = index.text(block.whole());
        let header = &whole[..block.body().start() - block.whole().start()];
        let parsed: syn::ItemImpl = syn::parse_str(&format!("{header} {{}}"))
            .unwrap_or_else(|error| panic!("an impl header parses: {error}\n{header}"));

        // A door of the seam, implemented somewhere else.
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

        // A block of a backend holder.
        let Some(&position) = holders.get(block.type_owner()) else {
            continue;
        };
        let syn::Type::Path(self_type) = &*parsed.self_ty else {
            continue;
        };
        let written_for = self_type.path.segments.last().and_then(|last| {
            let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
                return None;
            };
            arguments
                .args
                .iter()
                .filter_map(|argument| match argument {
                    syn::GenericArgument::Type(argument) => Some(argument),
                    _ => None,
                })
                .nth(position)
        });
        let generic = written_for.is_some_and(|argument| {
            let syn::Type::Path(argument) = argument else {
                return false;
            };
            argument.path.get_ident().is_some_and(|ident| {
                modules.iter().any(|module| {
                    backend_parameters(&parsed.generics, &resolver, package, module, &wanted)
                        .contains(&ident.to_string())
                })
            })
        });
        if generic {
            continue;
        }
        let owner = block.type_owner();
        if let Some(path) = block.trait_path() {
            let name = path.last().map_or("", String::as_str);
            if allowed.traits.contains(&(owner, name)) {
                used.insert(format!("{owner} {name}"));
            } else {
                refusals.push(format!(
                    "`impl {name} for {owner}` is written for the native backend in {} — the \
                     window is reached through the seam",
                    modules.join(", ")
                ));
            }
            continue;
        }
        let whole: syn::ItemImpl = syn::parse_str(whole)
            .unwrap_or_else(|error| panic!("a native `{owner}` block parses: {error}"));
        for member in &whole.items {
            let syn::ImplItem::Fn(method) = member else {
                continue;
            };
            let name = method.sig.ident.to_string();
            if allowed.inherent.contains(&(owner, name.as_str())) {
                used.insert(format!("{owner} {name}"));
            } else {
                refusals.push(format!(
                    "`{owner}::{name}` is written for the native backend in {} — the window is \
                     reached through the seam",
                    modules.join(", ")
                ));
            }
        }
    }
    let unused = allowed
        .inherent
        .iter()
        .chain(allowed.traits)
        .map(|(owner, name)| format!("{owner} {name}"))
        .filter(|entry| !used.contains(entry))
        .collect();
    refusals.sort();
    (refusals, unused)
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

/// RED (planted) — **each road around the seam is named, and nothing else is**: the native block
/// holding a method off the list, the trait written for the native backend's holder alone, and the
/// door implemented outside the seam; while the generic blocks (the bound written directly, through
/// the prelude's glob, through a rename and in a `where` clause), the listed native method, a
/// struct with no backend and a test module's native block pass.
#[test]
fn every_road_around_the_window_seam_is_named() {
    let index = fixture();
    let allowed = Allowed {
        inherent: &[("Runtime", "create"), ("Runtime", "retired")],
        traits: &[],
    };
    let (refusals, unused) = judge(&index, "window-seam", &allowed);
    assert_eq!(
        refusals,
        [
            "`Runtime::sneaks_past_the_seam` is written for the native backend in crate::runtime \
             — the window is reached through the seam",
            "`impl Default for Shell` is written for the native backend in crate::runtime — the \
             window is reached through the seam",
            "window-seam crate::window_seam::WindowHost is implemented for `Stray` in \
             crate::elsewhere — a door of the seam is implemented in the seam",
        ],
        "the three plants, and only them"
    );
    assert_eq!(
        unused,
        ["Runtime retired"],
        "a list entry nothing uses is named, so the list only shrinks"
    );
}

/// GUARD — **`bt-app` reaches its window only through the seam**: no block of a backend holder is
/// written for the native backend but the ones [`BT_APP`] lists, and every door of the seam is
/// implemented in the seam.
#[test]
fn bt_app_reaches_its_window_only_through_the_seam() {
    let (refusals, unused) = judge(Index::of_package("bt-app"), "bt-app", &BT_APP);
    assert!(
        refusals.is_empty(),
        "the window seam is the only road to the window:\n{}",
        refusals.join("\n")
    );
    assert!(
        unused.is_empty(),
        "native-only entries nothing writes any more — take them off the list: {unused:?}"
    );
}
