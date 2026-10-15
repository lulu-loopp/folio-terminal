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
    Certainty, DiskScope, ImplRecord, Index, ItemKind, MacroKind, MacroRecord, Occurrence, Span,
    TargetId, TargetKind, TargetRoot, TraitId, TraitResolver, Universe, Vendor, report,
};
use proc_macro2::TokenTree;
use syn::visit::Visit;

/// The module the seam is declared in.
const SEAM: &str = "crate::window_seam";

/// **The places a package may name a holder for the native backend**, each `(kind, owner, name)`:
/// `method` — an inherent block's method (`owner` the holder); `trait` — a trait block (`owner`
/// the holder, `name` the trait's last path segment); `function` — any other function (`owner`
/// the type of its `impl`, or its module); `type` — a struct, enum or union whose fields hold one
/// (`owner` its module); `const` — a `const` or `static` (`owner` its file).
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
            ["method", "trait", "function", "type", "const"].contains(&kind),
            "a native-list row's kind is method, trait, function, type or const: {line}"
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

/// The seam module's own name, as a path segment writes it.
fn seam_name() -> &'static str {
    SEAM.rsplit("::").next().unwrap_or(SEAM)
}

/// **What the guard reads code against**: the holders (by name, with where the backend stands
/// among their type parameters), the native backend's own names, the methods a holder has only in
/// a block written for the native backend, and how to find a scope's backend parameters.
struct Reading<'a> {
    holders: BTreeMap<String, usize>,
    natives: BTreeSet<String>,
    seam_traits: BTreeSet<String>,
    native_only: BTreeMap<String, BTreeSet<String>>,
    resolver: &'a TraitResolver<'a>,
    package: &'a str,
    backends: &'a BTreeSet<TraitId>,
}

impl Reading<'_> {
    /// The backend parameters `generics` declares, its bounds resolved in one of `modules`.
    fn parameters(&self, generics: &syn::Generics, modules: &[&str]) -> BTreeSet<String> {
        modules
            .iter()
            .flat_map(|module| {
                backend_parameters(generics, self.resolver, self.package, module, self.backends)
            })
            .collect()
    }

    /// Whether `name`, the segment after `owner` (or the first, with none), is the native
    /// backend: one of its names, standing first or as a member of a module (`crate`, `self`,
    /// `super`, the seam, or any module that brought it in, such as `crate::runtime::windows`).
    /// A member of a type spelled the same (`ShellProgram::Native`, an enum's variant) is not it:
    /// a module's name is snake_case and a type's is CamelCase, which the workspace's
    /// `-D warnings` holds.
    fn is_native(&self, owner: Option<&str>, name: &str) -> bool {
        self.natives.contains(name)
            && owner.is_none_or(|owner| owner.starts_with(|first: char| first.is_ascii_lowercase()))
    }
}

/// **Every holder named for the native backend in a stretch of code**, and every mention of the
/// native backend itself.
///
/// * In **type position** a holder is native when its backend argument is missing (the default is
///   the native backend) or is not one of `generic`.
/// * In **expression and pattern position** — a path such as `FolioApp::new` or `Runtime::create`,
///   a struct literal, a struct or tuple-struct pattern — a backend argument written on the path
///   is judged the same way; an unwritten one is chosen by inference. Inference is read as native
///   when no backend parameter is in scope, and whatever the scope when the path names a method
///   the holder has only in a block written for the native backend (`Runtime::create`: inference
///   can pick nothing else for it). Otherwise it is read as the parameter's.
/// * **The native backend's own name** (each type `crate::window_seam` implements `Backend` for)
///   anywhere outside the seam is a native mention, in any position, standing first or as a
///   member of a module ([`Reading::is_native`]).
/// * **A nested item** (a function, an impl, a type written inside a body) sees none of the scope
///   around it — Rust gives it none — so its backend parameters are its own.
/// * **A `use` in a body that renames** the native backend or the seam names it.
///
/// A macro's tokens are not visited here (`syn` does not descend into them); the macro pass of
/// [`judge`] reads them.
struct NativeMentions<'a> {
    reading: &'a Reading<'a>,
    modules: &'a [&'a str],
    generic: BTreeSet<String>,
    found: BTreeSet<String>,
}

impl NativeMentions<'_> {
    /// The native backend's own names in `path`.
    fn native_names(&mut self, path: &syn::Path) {
        let mut before: Option<String> = None;
        for segment in &path.segments {
            let name = segment.ident.to_string();
            if self.reading.is_native(before.as_deref(), &name) {
                self.found.insert(name.clone());
            }
            before = Some(name);
        }
    }

    /// The holders and native names a path in expression or pattern position names.
    fn judge_value_path(&mut self, path: &syn::Path) {
        self.native_names(path);
        let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
        for (at, segment) in segments.iter().enumerate() {
            let name = segment.ident.to_string();
            let Some(&position) = self.reading.holders.get(&name) else {
                continue;
            };
            let native = if matches!(segment.arguments, syn::PathArguments::AngleBracketed(_)) {
                !written_for_a_parameter(segment, position, &self.generic)
            } else {
                let native_only = segments.get(at + 1).is_some_and(|next| {
                    self.reading
                        .native_only
                        .get(&name)
                        .is_some_and(|methods| methods.contains(&next.ident.to_string()))
                });
                self.generic.is_empty() || native_only
            };
            if native {
                self.found.insert(name);
            }
        }
    }
}

impl<'ast> Visit<'ast> for NativeMentions<'_> {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.native_names(&node.path);
        for segment in &node.path.segments {
            let name = segment.ident.to_string();
            if let Some(&position) = self.reading.holders.get(&name)
                && !written_for_a_parameter(segment, position, &self.generic)
            {
                self.found.insert(name);
            }
        }
        syn::visit::visit_type_path(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if node.qself.is_none() {
            self.judge_value_path(&node.path);
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if node.qself.is_none() {
            self.judge_value_path(&node.path);
        }
        syn::visit::visit_expr_struct(self, node);
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        if node.qself.is_none() {
            self.judge_value_path(&node.path);
        }
        syn::visit::visit_pat_struct(self, node);
    }

    fn visit_pat_tuple_struct(&mut self, node: &'ast syn::PatTupleStruct) {
        if node.qself.is_none() {
            self.judge_value_path(&node.path);
        }
        syn::visit::visit_pat_tuple_struct(self, node);
    }

    fn visit_item(&mut self, node: &'ast syn::Item) {
        let generics = match node {
            syn::Item::Fn(item) => Some(&item.sig.generics),
            syn::Item::Impl(item) => Some(&item.generics),
            syn::Item::Struct(item) => Some(&item.generics),
            syn::Item::Enum(item) => Some(&item.generics),
            syn::Item::Union(item) => Some(&item.generics),
            syn::Item::Trait(item) => Some(&item.generics),
            syn::Item::Type(item) => Some(&item.generics),
            _ => None,
        };
        let own = generics.map_or_else(BTreeSet::new, |generics| {
            self.reading.parameters(generics, self.modules)
        });
        let outer = std::mem::replace(&mut self.generic, own);
        syn::visit::visit_item(self, node);
        self.generic = outer;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let own = self.reading.parameters(&node.sig.generics, self.modules);
        let outer = self.generic.clone();
        self.generic.extend(own);
        syn::visit::visit_impl_item_fn(self, node);
        self.generic = outer;
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.renames(&node.tree, &mut Vec::new());
    }
}

impl NativeMentions<'_> {
    /// **A `use` in a body that renames** a holder, the native backend, the seam module, or
    /// anything brought from the seam: the body names it (`Runtime` for a holder, `Native` for
    /// the native backend, `WindowHost (renamed from the seam)` for one of the seam's items), so
    /// the item holding the body is refused. A module-level `use` is judged by the `use` pass.
    fn renames(&mut self, tree: &syn::UseTree, path: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(step) => {
                path.push(step.ident.to_string());
                self.renames(&step.tree, path);
                path.pop();
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.renames(item, path);
                }
            }
            syn::UseTree::Rename(rename) => {
                let name = rename.ident.to_string();
                if rename.rename == name {
                    return;
                }
                if self.reading.holders.contains_key(&name)
                    || self.reading.natives.contains(&name)
                    || name == seam_name()
                {
                    self.found.insert(name);
                } else if path.iter().any(|step| step == seam_name()) {
                    self.found.insert(format!("{name} (renamed from the seam)"));
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Glob(_) => {}
        }
    }
}

/// **The impl blocks written inside a body** — a function's, a `const`'s or `static`'s
/// initializer, any expression the guard parses — and the `use` leaves written there, each local
/// name with the path it brings in. They are no impl and no `use` of the index (which records
/// module-level ones), so the guard reads them here.
#[derive(Default)]
struct NestedImpls {
    impls: Vec<syn::ItemImpl>,
    uses: BTreeMap<String, Vec<String>>,
}

impl NestedImpls {
    fn leaves(&mut self, tree: &syn::UseTree, path: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(step) => {
                path.push(step.ident.to_string());
                self.leaves(&step.tree, path);
                path.pop();
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.leaves(item, path);
                }
            }
            syn::UseTree::Name(name) => {
                let mut full = path.clone();
                full.push(name.ident.to_string());
                self.uses.insert(name.ident.to_string(), full);
            }
            syn::UseTree::Rename(rename) => {
                let mut full = path.clone();
                full.push(rename.ident.to_string());
                self.uses.insert(rename.rename.to_string(), full);
            }
            syn::UseTree::Glob(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for NestedImpls {
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.impls.push(node.clone());
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.leaves(&node.tree, &mut Vec::new());
    }
}

/// **The refusals of the impl blocks written inside a body**, `place` naming the item that holds
/// it and `modules` where it is written. Refused by name, with no list row, whatever the item:
///
/// * an impl whose trait is a door of the seam — resolved through the body's own `use` leaves and
///   then the module's (`use crate::m::Host;` reaching a renamed re-export of `WindowHost`), or
///   spelled as one of the seam's traits;
/// * an impl whose self type names a holder for the native backend: a native block is written at
///   module level, where the native list names its methods, and one inside a body is not.
fn nested_refusals(
    reading: &Reading<'_>,
    modules: &[&str],
    place: &str,
    nested: &NestedImpls,
) -> Vec<String> {
    let mut refusals = Vec::new();
    for block in &nested.impls {
        let self_type = match &*block.self_ty {
            syn::Type::Path(path) => segments(&path.path).join("::"),
            _ => String::from("a type that is not a path"),
        };
        if let Some((_, path, _)) = &block.trait_ {
            let written = segments(path);
            let expanded = written
                .first()
                .and_then(|first| nested.uses.get(first))
                .map(|full| {
                    full.iter()
                        .chain(written.iter().skip(1))
                        .cloned()
                        .collect::<Vec<_>>()
                });
            let doors: Vec<String> = modules
                .iter()
                .flat_map(|module| {
                    let mut found = reading.resolver.resolve(reading.package, module, &written);
                    if let Some(expanded) = &expanded {
                        found.extend(reading.resolver.resolve(reading.package, module, expanded));
                    }
                    found
                })
                .filter(|id| id.package == reading.package && id.module_path == SEAM)
                .map(|id| id.to_string())
                .collect();
            let named_like_a_door = written
                .last()
                .is_some_and(|last| reading.seam_traits.contains(last));
            if !doors.is_empty() || named_like_a_door {
                let door = doors.first().cloned().unwrap_or_else(|| written.join("::"));
                refusals.push(format!(
                    "{place} implements {door} for `{self_type}` inside its body — a door of the \
                     seam is implemented in the seam"
                ));
            }
        }
        let generic = reading.parameters(&block.generics, modules);
        let named = native_mentions(reading, modules, &generic, |visit| {
            visit.visit_type(&block.self_ty);
        });
        let holder = match &*block.self_ty {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map(|last| last.ident.to_string())
                .filter(|last| named.contains(last)),
            _ => None,
        };
        if let Some(holder) = holder {
            refusals.push(format!(
                "{place} writes a block of `{holder}` for the native backend inside its body — a \
                 native block is written at module level, where the native list names its methods"
            ));
        }
    }
    refusals
}

/// The impl blocks and `use` leaves written inside whatever `visit` walks.
fn nested_in(visit: impl FnOnce(&mut NestedImpls)) -> NestedImpls {
    let mut nested = NestedImpls::default();
    visit(&mut nested);
    nested
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

/// The holders and native names a stretch of code names for the native backend, written in one of
/// `modules`, with `generic` the backend parameters in scope.
fn native_mentions(
    reading: &Reading<'_>,
    modules: &[&str],
    generic: &BTreeSet<String>,
    visit: impl FnOnce(&mut NativeMentions<'_>),
) -> BTreeSet<String> {
    let mut mentions = NativeMentions {
        reading,
        modules,
        generic: generic.clone(),
        found: BTreeSet::new(),
    };
    visit(&mut mentions);
    mentions.found
}

/// The holder whose own block `parsed` is, by the last segment of its self type's path.
fn holder_path<'p>(
    parsed: &'p syn::ItemImpl,
    holders: &BTreeMap<String, usize>,
) -> Option<&'p syn::PathSegment> {
    match &*parsed.self_ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .filter(|last| holders.contains_key(&last.ident.to_string())),
        _ => None,
    }
}

/// **The holders and native names a macro's tokens name**, read lexically: every identifier that
/// is a holder's name, and every native name standing first or as a member of a module
/// ([`Reading::is_native`]).
fn macro_mentions(tokens: proc_macro2::TokenStream, reading: &Reading<'_>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => found.extend(macro_mentions(group.stream(), reading)),
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                if reading.holders.contains_key(&name) {
                    found.insert(name.clone());
                }
                // `owner::name`: two `:` puncts after an identifier.
                let owner = match at.checked_sub(3).map(|start| &tokens[start..at]) {
                    Some(
                        [
                            TokenTree::Ident(owner),
                            TokenTree::Punct(a),
                            TokenTree::Punct(b),
                        ],
                    ) if a.as_char() == ':' && b.as_char() == ':' => Some(owner.to_string()),
                    _ => None,
                };
                if reading.is_native(owner.as_deref(), &name) {
                    found.insert(name);
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
    found
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
    let backend = TraitId {
        package: package.to_owned(),
        module_path: SEAM.to_owned(),
        name: "Backend".to_owned(),
    };
    // The native backend: every product type the seam implements `Backend` for.
    let natives: BTreeSet<String> = index
        .impls()
        .iter()
        .filter(|block| {
            in_the_product(index, block.whole())
                && block.module_paths().contains(&SEAM)
                && resolver.implemented(package, block).contains(&backend)
        })
        .map(|block| block.type_owner().to_owned())
        .collect();
    assert!(
        !natives.is_empty(),
        "the seam of `{package}` implements `Backend` for no product type: the native backend \
         has no name to read"
    );
    // The methods a holder has only in a block written for the native backend: a path call to
    // one of them is native in any scope, because inference can pick nothing else for it. A path
    // finds an inherent method before a trait's, so a native method — inherent or a trait
    // impl's — is native-only unless a generic *inherent* block has it too. (A generic trait
    // impl's method of the same name is no answer: the trait may not be in scope, or be another
    // trait; the guard errs to refusing.)
    let mut native_methods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut generic_methods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for block in index.impls() {
        if !in_the_product(index, block.whole()) {
            continue;
        }
        let parsed = impl_header(index, block);
        let Some(last) = holder_path(&parsed, &holders) else {
            continue;
        };
        let holder = last.ident.to_string();
        let generic: BTreeSet<String> = block
            .module_paths()
            .iter()
            .flat_map(|module| {
                backend_parameters(&parsed.generics, &resolver, package, module, &backends)
            })
            .collect();
        let whole: syn::ItemImpl = syn::parse_str(index.text(block.whole()))
            .unwrap_or_else(|error| panic!("a `{holder}` block parses: {error}"));
        let methods = whole.items.iter().filter_map(|member| match member {
            syn::ImplItem::Fn(method) => Some(method.sig.ident.to_string()),
            _ => None,
        });
        if !written_for_a_parameter(last, holders[&holder], &generic) {
            native_methods.entry(holder).or_default().extend(methods);
        } else if whole.trait_.is_none() {
            generic_methods.entry(holder).or_default().extend(methods);
        }
    }
    let mut native_only: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (holder, methods) in &native_methods {
        let generic = generic_methods.get(holder);
        native_only.entry(holder.clone()).or_default().extend(
            methods
                .iter()
                .filter(|method| generic.is_none_or(|generic| !generic.contains(*method)))
                .cloned(),
        );
    }
    let reading = Reading {
        holders: holders.clone(),
        natives: natives.clone(),
        seam_traits: index
            .traits()
            .iter()
            .filter(|declared| declared.module_paths().contains(&SEAM))
            .map(|declared| declared.name().to_owned())
            .collect(),
        native_only,
        resolver: &resolver,
        package,
        backends: &backends,
    };

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

    // The impl blocks: the doors of the seam, and the blocks of the holders. A holder's block
    // written for the native backend is judged as a block (its methods by name, its trait by
    // name) and its items are not read again; every other block's functions are read by the
    // function pass below, with the block's backend parameters in scope, and its associated types
    // and consts here.
    let mut native_blocks: Vec<Span> = Vec::new();
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
        let seam_block = modules.contains(&SEAM);

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
        let Some(last) = holder_path(&parsed, &holders) else {
            // Not a holder's own block: whatever it names for the native backend in its self type
            // is a shape this guard does not read.
            let named = native_mentions(&reading, &modules, &generic, |visit| {
                visit.visit_type(&parsed.self_ty)
            });
            if !named.is_empty() && !seam_block {
                refusals.push(format!(
                    "an impl in {} is written for a type that names {} for the native backend \
                     without being its path — a shape the window-seam guard does not read",
                    modules.join(", "),
                    named.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
            if !seam_block {
                associated_items(index, block, &reading, &generic, &mut refusals);
            }
            continue;
        };
        if written_for_a_parameter(last, holders[&last.ident.to_string()], &generic) {
            if !seam_block {
                associated_items(index, block, &reading, &generic, &mut refusals);
            }
            continue;
        }
        native_blocks.push(block.body());
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
                // A native block's associated type or const has no list row: the block is native
                // throughout, and only its methods are named by the list.
                let name = match member {
                    syn::ImplItem::Const(declared) => declared.ident.to_string(),
                    syn::ImplItem::Type(declared) => declared.ident.to_string(),
                    _ => String::from("an item the guard does not read"),
                };
                refusals.push(format!(
                    "`{holder}::{name}` is an associated item of a native block in {} — only a \
                     listed method is written for the native backend",
                    modules.join(", ")
                ));
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

    // A trait's backend parameters are in scope in its methods' declarations and default bodies:
    // its own type parameters bounded by the backend, and `Self` when the trait is itself a
    // backend trait (the backend among its supertraits) or its `where` clause bounds `Self`.
    let mut trait_generics: Vec<(Span, BTreeSet<String>)> = Vec::new();
    for declared in index.traits() {
        if !in_the_product(index, declared.whole()) {
            continue;
        }
        let parsed: syn::ItemTrait = syn::parse_str(index.text(declared.whole()))
            .unwrap_or_else(|error| panic!("the trait `{}` parses: {error}", declared.name()));
        let mut generic = reading.parameters(&parsed.generics, &declared.module_paths());
        if resolver
            .identities(package, declared)
            .iter()
            .any(|id| backends.contains(id))
        {
            generic.insert(String::from("Self"));
        }
        trait_generics.push((declared.body(), generic));
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
        if item.module_paths().contains(&SEAM) {
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
        for (body, block) in block_generics.iter().chain(&trait_generics) {
            if body.holds(at) {
                generic.extend(block.iter().cloned());
            }
        }
        let owner = item
            .type_owner()
            .map_or_else(|| item.module_paths().join(", "), str::to_owned);
        // The impl blocks written inside the body — every body, a native block's methods and the
        // listed functions included ([`nested_refusals`]).
        let nested = nested_in(|visit| visit.visit_block(&parsed.block));
        refusals.extend(nested_refusals(
            &reading,
            &item.module_paths(),
            &format!("`{owner}::{}`", item.name()),
            &nested,
        ));
        // A native block's method is judged with its block, by the list.
        if native_blocks.iter().any(|body| body.holds(at)) {
            continue;
        }
        let named = native_mentions(&reading, &item.module_paths(), &generic, |visit| {
            visit.visit_item_fn(&parsed)
        });
        if named.is_empty() {
            continue;
        }
        if !allow("function", &owner, item.name()) {
            refusals.push(format!(
                "`{owner}::{}` names {} for the native backend — the window is reached through \
                 the seam",
                item.name(),
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    // Every type's fields — a holder's and any other type's — with the type's own backend
    // parameters in scope.
    for item in index.items() {
        if !matches!(
            item.kind(),
            ItemKind::Struct | ItemKind::Enum | ItemKind::Union
        ) || !in_the_product(index, item.whole())
            || item.module_paths().contains(&SEAM)
        {
            continue;
        }
        let parsed: syn::Item = syn::parse_str(index.text(item.whole()))
            .unwrap_or_else(|error| panic!("the type `{}` parses: {error}", item.name()));
        // The fields and the generics: a holder's own declaration defaults its backend parameter
        // to the native backend, which is the seam's design, not a field holding it.
        let (generics, fields): (&syn::Generics, Vec<&syn::Field>) = match &parsed {
            syn::Item::Struct(declared) => (&declared.generics, declared.fields.iter().collect()),
            syn::Item::Enum(declared) => (
                &declared.generics,
                declared
                    .variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .collect(),
            ),
            syn::Item::Union(declared) => {
                (&declared.generics, declared.fields.named.iter().collect())
            }
            _ => continue,
        };
        let modules = item.module_paths();
        let generic = reading.parameters(generics, &modules);
        // Its generics too — bounds, `where` clause and defaults — but for a backend parameter's
        // default, which is the seam's design.
        let mut read_generics = generics.clone();
        for param in read_generics.type_params_mut() {
            if generic.contains(&param.ident.to_string()) {
                param.eq_token = None;
                param.default = None;
            }
        }
        let named = native_mentions(&reading, &modules, &generic, |visit| {
            visit.visit_generics(&read_generics);
            for field in fields {
                visit.visit_field(field);
            }
        });
        if !named.is_empty() {
            let owner = item.module_paths().join(", ");
            if !allow("type", &owner, item.name()) {
                refusals.push(format!(
                    "the type `{owner}::{}` holds {} for the native backend — the window is \
                     reached through the seam",
                    item.name(),
                    named.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
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
        // The modules this file is: the whole-file module records over its bytes.
        let modules: Vec<String> = index
            .modules()
            .iter()
            .filter(|module| module.span() == file.span())
            .flat_map(|module| module.module_paths().iter().cloned())
            .collect();
        if modules.iter().any(|module| module == SEAM) {
            continue;
        }
        file_items(
            &parsed.items,
            &reading,
            &modules,
            &mut |kind, item_name, what| match kind {
                "nested" => refusals.push(what.replace(
                    "{place}",
                    &format!("the `const` or `static` `{item_name}` in {name}"),
                )),
                "alias" => refusals.push(format!(
                    "the type alias `{item_name}` in {name} names {what} — the window-seam guard \
                     reads holders by their own name; write the holder"
                )),
                "alias-native" => refusals.push(format!(
                    "the type alias `{item_name}` in {name} names {what} for the native backend — \
                     the window is reached through the seam"
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

    // `use` items renaming the native backend or the seam module: the guard reads both by their
    // own names. (One in a body is read by the function pass, as a mention.)
    for declared in index.uses() {
        if !in_the_product(index, declared.whole()) || declared.module_paths().contains(&SEAM) {
            continue;
        }
        for (name, path) in declared.names() {
            let Some(last) = path.last() else {
                continue;
            };
            if holders.contains_key(last) && name != last {
                refusals.push(format!(
                    "`use {} as {name}` in {} renames the holder {last} — the guard reads holders \
                     by their own name; write the holder",
                    path.join("::"),
                    declared.module_paths().join(", ")
                ));
            }
            if (natives.contains(last) || last == seam_name()) && name != last {
                refusals.push(format!(
                    "`use {} as {name}` in {} renames {last} — the guard reads the native backend \
                     and the seam by their own names",
                    path.join("::"),
                    declared.module_paths().join(", ")
                ));
            }
        }
    }

    // Every trait's declaration but its methods (the function pass reads those): its generics,
    // supertraits, associated consts and associated types.
    for declared in index.traits() {
        if !in_the_product(index, declared.whole()) || declared.module_paths().contains(&SEAM) {
            continue;
        }
        let mut parsed: syn::ItemTrait = syn::parse_str(index.text(declared.whole()))
            .unwrap_or_else(|error| panic!("the trait `{}` parses: {error}", declared.name()));
        parsed
            .items
            .retain(|member| !matches!(member, syn::TraitItem::Fn(_)));
        let modules = declared.module_paths();
        // The scope its methods have: its own backend parameters, and `Self` for a backend trait.
        let generic = trait_generics
            .iter()
            .find(|(body, _)| *body == declared.body())
            .map(|(_, generic)| generic.clone())
            .unwrap_or_default();
        let nested = nested_in(|visit| {
            for member in &parsed.items {
                visit.visit_trait_item(member);
            }
        });
        refusals.extend(nested_refusals(
            &reading,
            &modules,
            &format!("the trait `{}::{}`", modules.join(", "), declared.name()),
            &nested,
        ));
        let named = native_mentions(&reading, &modules, &generic, |visit| {
            visit.visit_item_trait(&parsed);
        });
        if !named.is_empty() {
            refusals.push(format!(
                "the trait `{}::{}` names {} for the native backend in its declaration — the \
                 window is reached through the seam",
                modules.join(", "),
                declared.name(),
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    // Macros: `syn` does not read their tokens as code, so a holder or a native name in a macro's
    // tokens — an invocation's or a `macro_rules!` body's — is refused by name. Read lexically,
    // outermost macro only (an inner one's tokens are the outer one's).
    let seam_spans: Vec<Span> = index
        .modules()
        .iter()
        .filter(|module| module.module_paths().iter().any(|path| path == SEAM))
        .map(|module| module.span())
        .collect();
    let product_macros: Vec<&MacroRecord> = index
        .macros()
        .iter()
        .filter(|found| {
            in_the_product(index, found.span())
                && !seam_spans.iter().any(|seam| found.span().within(*seam))
        })
        .collect();
    for found in &product_macros {
        if product_macros
            .iter()
            .any(|outer| outer.span() != found.span() && found.span().within(outer.tokens()))
        {
            continue;
        }
        let tokens: proc_macro2::TokenStream = index
            .text(found.tokens())
            .parse()
            .unwrap_or_else(|error| panic!("a macro's tokens lex: {error}"));
        let named = macro_mentions(tokens, &reading);
        if named.is_empty() {
            continue;
        }
        let at = index
            .locate(found.span().start())
            .expect("a macro is in a file of the index");
        let file = at
            .file
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let written = match found.kind() {
            MacroKind::Definition => format!("macro_rules! {}", found.path()),
            MacroKind::Invocation => format!("{}!", found.path()),
        };
        refusals.push(format!(
            "`{written}` at {file}:{}: a macro's tokens name {} — the guard does not read macros",
            at.line,
            named.into_iter().collect::<Vec<_>>().join(", ")
        ));
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

/// **An impl block's generics, trait arguments, associated types and consts**, read with the
/// block's backend parameters (`generic`) in scope: one naming a holder or the native backend
/// natively is refused, by name — there is no list row for any of them. (Its functions are read by
/// the function pass.)
fn associated_items(
    index: &Index,
    block: &ImplRecord,
    reading: &Reading<'_>,
    generic: &BTreeSet<String>,
    refusals: &mut Vec<String>,
) {
    let whole: syn::ItemImpl = syn::parse_str(index.text(block.whole()))
        .unwrap_or_else(|error| panic!("an impl block parses: {error}"));
    let modules = block.module_paths();
    let named = native_mentions(reading, &modules, generic, |visit| {
        visit.visit_generics(&whole.generics);
    });
    if !named.is_empty() {
        refusals.push(format!(
            "an impl for `{}` in {} names {} for the native backend in its generics — the window \
             is reached through the seam",
            block.type_owner(),
            modules.join(", "),
            named.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some((_, path, _)) = &whole.trait_ {
        let named = native_mentions(reading, &modules, generic, |visit| visit.visit_path(path));
        if !named.is_empty() {
            refusals.push(format!(
                "the impl of `{}` for `{}` in {} names {} for the native backend in its trait — \
                 the window is reached through the seam",
                path.segments
                    .last()
                    .map_or_else(String::new, |last| last.ident.to_string()),
                block.type_owner(),
                block.module_paths().join(", "),
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    for member in &whole.items {
        if let syn::ImplItem::Const(declared) = member {
            let nested = nested_in(|visit| visit.visit_expr(&declared.expr));
            refusals.extend(nested_refusals(
                reading,
                &modules,
                &format!("`{}::{}`", block.type_owner(), declared.ident),
                &nested,
            ));
        }
        let (kind, name, named) = match member {
            syn::ImplItem::Type(declared) => (
                "type",
                declared.ident.to_string(),
                native_mentions(reading, &modules, generic, |visit| {
                    visit.visit_impl_item_type(declared);
                }),
            ),
            syn::ImplItem::Const(declared) => (
                "const",
                declared.ident.to_string(),
                native_mentions(reading, &modules, generic, |visit| {
                    visit.visit_impl_item_const(declared);
                }),
            ),
            _ => continue,
        };
        if !named.is_empty() {
            refusals.push(format!(
                "the associated {kind} `{}::{name}` in {} names {} for the native backend — the \
                 window is reached through the seam",
                block.type_owner(),
                block.module_paths().join(", "),
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }
}

/// The type aliases naming a holder at all, and the `const` and `static` items naming one for the
/// native backend, among `items` and their inline modules — a `#[cfg(test)]` module left out.
fn file_items(
    items: &[syn::Item],
    reading: &Reading<'_>,
    modules: &[String],
    found: &mut impl FnMut(&str, &str, &str),
) {
    let written: Vec<&str> = modules.iter().map(String::as_str).collect();
    // A `const`'s or `static`'s initializer is a body: the impls written there are read as a
    // function's are; each refusal is handed back with `{place}` for the item to fill in.
    let nested = |item_name: &str, nested: NestedImpls, found: &mut dyn FnMut(&str, &str, &str)| {
        for refusal in nested_refusals(reading, &written, "{place}", &nested) {
            found("nested", item_name, &refusal);
        }
    };
    let none = BTreeSet::new();
    for item in items {
        match item {
            syn::Item::Type(alias) => {
                // Any holder at all: an alias written for a parameter is still a name the guard
                // does not read.
                let mut any = Vec::new();
                AnyHolder(&reading.holders, &mut any).visit_type(&alias.ty);
                if !any.is_empty() {
                    found("alias", &alias.ident.to_string(), &any.join(", "));
                }
                // And the native backend, in its type or its generics (a default, a bound): an
                // alias of it reaches the native window through `<N as Backend>::…` with no
                // mention left to read.
                let native: BTreeSet<String> = native_mentions(reading, &[], &none, |visit| {
                    visit.visit_generics(&alias.generics);
                    visit.visit_type(&alias.ty);
                })
                .into_iter()
                .filter(|name| !reading.holders.contains_key(name))
                .collect();
                if !native.is_empty() {
                    let names = native.into_iter().collect::<Vec<_>>().join(", ");
                    found("alias-native", &alias.ident.to_string(), &names);
                }
            }
            syn::Item::Const(constant) => {
                nested(
                    &constant.ident.to_string(),
                    nested_in(|visit| visit.visit_expr(&constant.expr)),
                    found,
                );
                let named = native_mentions(reading, &[], &none, |visit| {
                    visit.visit_item_const(constant)
                });
                if !named.is_empty() {
                    let names = named.into_iter().collect::<Vec<_>>().join(", ");
                    found("const", &constant.ident.to_string(), &names);
                }
            }
            syn::Item::Static(fixed) => {
                nested(
                    &fixed.ident.to_string(),
                    nested_in(|visit| visit.visit_expr(&fixed.expr)),
                    found,
                );
                let named =
                    native_mentions(reading, &[], &none, |visit| visit.visit_item_static(fixed));
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
                    let inner_modules: Vec<String> = modules
                        .iter()
                        .map(|path| format!("{path}::{}", module.ident))
                        .collect();
                    file_items(inner, reading, &inner_modules, found);
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
/// native holders; a `const` naming one; a method of a generic holder block taking a holder whose
/// backend defaults to the native one; a generic trait impl of a holder naming the native backend
/// in a method's body, and another in its trait's arguments; a native holder built in expression
/// position by inference, by a struct literal, and taken apart in a struct pattern and a
/// tuple-struct pattern; a native-only method called by inference inside a function generic over
/// the backend; an inner function of a generic function, which sees none of its scope; a type
/// whose field holds a native holder, and one whose generic parameter defaults to one; an
/// associated const and an associated type naming one; a trait whose associated items, and one
/// whose supertrait and bound, name them; a holder block's own `where` clause naming one; the
/// native backend named by its path and through a module that brought it in; `use` items
/// renaming the native backend and the seam; a holder named in a macro invocation's tokens and in
/// a `macro_rules!` body; a `use` renaming a holder, at module level and in a body; module-level
/// type aliases of the native backend, by its type and by a generic default; a native-only method
/// beside a generic trait impl's method of the same name (`generic_builds` stays refused); a door
/// of the seam implemented inside a body, directly, through a renaming `use` and through a body's
/// `use` of a module's renamed re-export, inside a listed native method and inside a `const`'s
/// initializer; a native block of a holder written inside a body, and inside a listed function; a
/// trait method's default body naming a native holder (the trait's scope is no exemption); and a
/// list row nothing uses. The generic blocks (the bound written
/// directly, through the prelude's glob, through a rename, in a `where` clause and through a
/// supertrait), a holder built by inference inside a generic function, a trait method's default
/// body written for the trait's own backend parameter and for a backend trait's `Self`, an enum's
/// variant spelled as the backend, the listed native method, function and `const`, a type with no backend and a
/// test module's native block pass.
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
            "`Runtime::create` implements window-seam crate::window_seam::WindowHost for `Made` \
             inside its body — a door of the seam is implemented in the seam",
            "`Runtime::sneaks_past_the_seam` is written for the native backend in crate::runtime \
             — the window is reached through the seam",
            "`Runtime::takes_the_native` names Runtime for the native backend — the window is \
             reached through the seam",
            "`Shell::fmt` names Native, Shell for the native backend — the window is reached \
             through the seam",
            "`crate::listed_rebuild` writes a block of `Runtime` for the native backend inside \
             its body — a native block is written at module level, where the native list names \
             its methods",
            "`crate::runtime::builds_by_a_literal` names Shell for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::builds_by_inference` names Runtime for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::generic_builds` names Runtime for the native backend — the window \
             is reached through the seam",
            "`crate::runtime::hides_a_door` implements window-seam \
             crate::window_seam::WindowHost for `Hidden` inside its body — a door of the seam is \
             implemented in the seam",
            "`crate::runtime::hides_a_native_block` names Native, Runtime for the native backend \
             — the window is reached through the seam",
            "`crate::runtime::hides_a_native_block` writes a block of `Runtime` for the native \
             backend inside its body — a native block is written at module level, where the \
             native list names its methods",
            "`crate::runtime::hides_a_reexported_door` implements window-seam \
             crate::window_seam::WindowHost for `Hidden` inside its body — a door of the seam is \
             implemented in the seam",
            "`crate::runtime::hides_a_renamed_door` implements window-seam \
             crate::window_seam::WindowHost for `Hidden` inside its body — a door of the seam is \
             implemented in the seam",
            "`crate::runtime::hides_a_renamed_door` names WindowHost (renamed from the seam) for \
             the native backend — the window is reached through the seam",
            "`crate::runtime::holds_an_inner_function` names Shell for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::names_it_by_its_path` names Native for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::native_default` names Runtime for the native backend — the window \
             is reached through the seam",
            "`crate::runtime::rebuild_every_window` names Runtime for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::renames_a_holder_inside` names Shell for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::takes_a_pair_apart` names Pair for the native backend — the window \
             is reached through the seam",
            "`crate::runtime::takes_apart_by_pattern` names Shell for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::through_a_module` names Native for the native backend — the window \
             is reached through the seam",
            "`impl Default for Shell` is written for the native backend in crate::runtime — the \
             window is reached through the seam",
            "`macro_rules! builds_one` at runtime.rs:142: a macro's tokens name Runtime — the \
             guard does not read macros",
            "`use crate::Runtime as Rt` in crate::runtime renames the holder Runtime — the guard \
             reads holders by their own name; write the holder",
            "`use crate::window_seam as ws` in crate::runtime renames window_seam — the guard \
             reads the native backend and the seam by their own names",
            "`use crate::window_seam::Native as N` in crate::runtime renames Native — the guard \
             reads the native backend and the seam by their own names",
            "`vec!` at runtime.rs:138: a macro's tokens name Runtime — the guard does not read \
             macros",
            "an impl for `Runtime` in crate::runtime names Native, Shell for the native backend \
             in its generics — the window is reached through the seam",
            "an impl in crate::runtime is written for a type that names Native, Runtime for the \
             native backend without being its path — a shape the window-seam guard does not read",
            "the `const` or `static` `PLANTED` in runtime.rs names Runtime for the native \
             backend — the window is reached through the seam",
            "the `const` or `static` `_` in runtime.rs implements window-seam \
             crate::window_seam::WindowHost for `Made` inside its body — a door of the seam is \
             implemented in the seam",
            "the associated const `Plain::NATIVE` in crate::runtime names Runtime for the native \
             backend — the window is reached through the seam",
            "the associated type `Shell::Item` in crate::runtime names Native, Shell for the \
             native backend — the window is reached through the seam",
            "the impl of `Twin` for `Shell` in crate::runtime names Native, Shell for the native \
             backend in its trait — the window is reached through the seam",
            "the trait `crate::runtime::Bounded` names Native, Runtime, Shell for the native \
             backend in its declaration — the window is reached through the seam",
            "the trait `crate::runtime::Keeps` names Native, Runtime, Shell for the native \
             backend in its declaration — the window is reached through the seam",
            "the type `crate::runtime::Defaulted` holds Runtime for the native backend — the \
             window is reached through the seam",
            "the type `crate::runtime::Keeper` holds Runtime for the native backend — the window \
             is reached through the seam",
            "the type alias `NativeBackend` in runtime.rs names Native for the native backend — \
             the window is reached through the seam",
            "the type alias `NativeRuntime` in runtime.rs names Native for the native backend — \
             the window is reached through the seam",
            "the type alias `NativeRuntime` in runtime.rs names Runtime — the window-seam guard \
             reads holders by their own name; write the holder",
            "the type alias `Windowed` in runtime.rs names Native for the native backend — the \
             window is reached through the seam",
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
