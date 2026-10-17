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
//! * **the window's platform handle stays behind the doors** (RULE C, [`HandleWalk`]): code
//!   written for the window names neither `bt_platform::NativeWindow` nor
//!   `bt_platform::Compositor`, and no door of the seam does;
//! * every row of the native list is still used (and `scripts/ci/check-window-seam-native.ps1`
//!   holds the list to the merge base's: it only shrinks).
//!
//! The fixture under `tests/fixtures/window_seam/` plants one of each refusal; the last test holds
//! `bt-app` itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bt_source::{
    Certainty, DiskScope, ImplRecord, Index, ItemKind, MacroKind, MacroRecord, Occurrence,
    ResolveCutoff, Span, TargetId, TargetKind, TargetRoot, TraitId, TraitResolver, Universe,
    Vendor, report,
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

/// **The dependencies `package`'s manifest names**, as crate names (`-` read as `_`), from every
/// `[dependencies]`, `[target.….dependencies]` and `[dependencies.<name>]` table — the crates a
/// product path may be rooted at. Dev- and build-dependencies are not the product's.
fn dependencies_of(package: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(package)
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("the package's manifest is there");
    let mut found = BTreeSet::new();
    let mut in_dependencies = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end_matches(']');
            let last = header.rsplit('.').next().unwrap_or(header);
            if header.ends_with("dependencies")
                && !header.ends_with("dev-dependencies")
                && !header.ends_with("build-dependencies")
            {
                in_dependencies = true;
            } else if header.contains("dependencies.")
                && !header.contains("dev-dependencies.")
                && !header.contains("build-dependencies.")
            {
                found.insert(last.replace('-', "_"));
                in_dependencies = false;
            } else {
                in_dependencies = false;
            }
            continue;
        }
        if in_dependencies && let Some((key, _)) = line.split_once('=') {
            let key = key.trim();
            let name = key.split('.').next().unwrap_or(key).trim_matches('"');
            if !name.is_empty() && !name.starts_with('#') {
                found.insert(name.replace('-', "_"));
            }
        }
    }
    found
}

/// The traits Rust's prelude brings into every module by name: a nested impl of one, written as a
/// single segment the resolver cannot follow, is the standard library's.
const PRELUDE_TRAITS: &[&str] = &[
    "AsMut",
    "AsRef",
    "Clone",
    "Copy",
    "Default",
    "DoubleEndedIterator",
    "Drop",
    "Eq",
    "ExactSizeIterator",
    "Extend",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "FromIterator",
    "Into",
    "IntoIterator",
    "Iterator",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "Send",
    "Sized",
    "Sync",
    "ToOwned",
    "ToString",
    "TryFrom",
    "TryInto",
    "Unpin",
];

/// Every identifier in a stretch of code that is a holder's name, in any position.
struct HolderIdents<'a>(&'a BTreeMap<String, usize>, BTreeSet<String>);

impl<'ast> Visit<'ast> for HolderIdents<'_> {
    fn visit_ident(&mut self, node: &'ast proc_macro2::Ident) {
        let name = node.to_string();
        if self.0.contains_key(&name) {
            self.1.insert(name);
        }
    }
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

thread_local! {
    /// The cutoffs the resolver reported while one judgment decided what a bound, a supertrait or
    /// an implemented trait names — drained into that judgment's refusals (each test's judgment
    /// runs on its own thread).
    static CUTOFFS: std::cell::RefCell<BTreeSet<String>> = std::cell::RefCell::default();
}

/// **What `path`, written in `module`, names — and the refusal when the resolver gave up on it**:
/// every call that *decides* something (which type parameters are backend parameters, which traits
/// are backend traits, which types are the native backend) goes through here, so a chain past the
/// resolver's hop limit is refused by name instead of silently deciding "no".
fn checked(
    resolver: &TraitResolver<'_>,
    package: &str,
    module: &str,
    path: &[String],
    what: &str,
) -> BTreeSet<TraitId> {
    match resolver.resolve_checked(package, module, path) {
        Ok(found) => found,
        Err(ResolveCutoff(found)) => {
            CUTOFFS.with(|cutoffs| {
                cutoffs.borrow_mut().insert(format!(
                    "{what} `{}` in {module} chains past the resolver's hop limit — refused, \
                     never passed",
                    path.join("::")
                ))
            });
            found
        }
    }
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
                let supertraits: BTreeSet<TraitId> = resolver
                    .declarations(&id)
                    .iter()
                    .flat_map(|declared| declared.supertraits())
                    .flat_map(|path| {
                        checked(resolver, package, &id.module_path, path, "the supertrait")
                    })
                    .collect();
                if !found.contains(&id) && supertraits.iter().any(|parent| found.contains(parent)) {
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
            syn::TypeParamBound::Trait(bound) => checked(
                resolver,
                package,
                module,
                &segments(&bound.path),
                "the bound",
            )
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
    /// The seam's traits by every name a first-party `use` gives them: their own, and each local
    /// name a module-level `use` resolves to one (`pub(crate) use crate::window_seam::WindowHost as
    /// Host;` adds `Host`).
    seam_traits: BTreeSet<String>,
    /// Each module's own `use` leaves: a local name with every path it was written for.
    module_uses: BTreeMap<String, BTreeMap<String, Vec<Vec<String>>>>,
    /// Each module's own `use` globs, each as the path it opens.
    module_globs: BTreeMap<String, Vec<Vec<String>>>,
    /// Every first-party module path.
    module_paths: BTreeSet<String>,
    /// The package's dependencies, as crate names: a path rooted at one is another crate's.
    dependencies: BTreeSet<String>,
    native_only: BTreeMap<String, BTreeSet<String>>,
    resolver: &'a TraitResolver<'a>,
    package: &'a str,
    backends: &'a BTreeSet<TraitId>,
}

impl Reading<'_> {
    /// The first-party module a glob's path opens, written in `module`: `crate`, `self`, `super`
    /// and child modules followed; `None` for anything else (another crate's module, a name the
    /// guard does not follow).
    fn module_of(&self, module: &str, path: &[String]) -> Option<String> {
        let mut current = module.to_owned();
        for (at, segment) in path.iter().enumerate() {
            current = match segment.as_str() {
                "crate" if at == 0 => String::from("crate"),
                "self" => current,
                "super" => current
                    .rsplit_once("::")
                    .map(|(parent, _)| parent.to_owned())?,
                name => {
                    let child = format!("{current}::{name}");
                    self.module_paths.contains(&child).then_some(child)?
                }
            };
        }
        Some(current)
    }

    /// **Whether a trait path the resolver did not follow is provably another crate's**, written
    /// in one of `modules`: followed through each module's own `use` leaves and its first-party
    /// globs to a fixed point, it is rooted at `std`, `core`, `alloc` or a dependency of the
    /// package (not also a first-party module's name — that is ambiguous, and refused), or is a
    /// single segment naming one of the prelude's traits. A path rooted at `crate`, `self` or
    /// `super` is first-party and is not; nor is one that does not settle within [`HOPS`].
    fn foreign(&self, path: &[String], modules: &[&str]) -> bool {
        let mut frontier: Vec<(Vec<String>, String)> = modules
            .iter()
            .map(|module| (path.to_vec(), (*module).to_owned()))
            .collect();
        for _ in 0..HOPS {
            let mut next = Vec::new();
            for (path, module) in &frontier {
                let Some(root) = path.first() else {
                    return false;
                };
                if ["crate", "self", "super"].contains(&root.as_str()) {
                    return false;
                }
                let own: Vec<&Vec<String>> = self
                    .module_uses
                    .get(module)
                    .and_then(|names| names.get(root))
                    .map(|leaves| {
                        leaves
                            .iter()
                            .filter(|leaf| leaf.as_slice() != [root.clone()])
                            .collect()
                    })
                    .unwrap_or_default();
                if !own.is_empty() {
                    for leaf in own {
                        next.push((
                            leaf.iter().chain(path.iter().skip(1)).cloned().collect(),
                            module.clone(),
                        ));
                    }
                    continue;
                }
                let globbed: Vec<String> = self
                    .module_globs
                    .get(module)
                    .map(|globs| {
                        globs
                            .iter()
                            .filter_map(|glob| self.module_of(module, glob))
                            .filter(|opened| {
                                self.module_uses
                                    .get(opened)
                                    .is_some_and(|names| names.contains_key(root))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !globbed.is_empty() {
                    for opened in globbed {
                        next.push((path.clone(), opened));
                    }
                    continue;
                }
                let settled = match path.as_slice() {
                    [single] => PRELUDE_TRAITS.contains(&single.as_str()),
                    [root, ..] => {
                        // A dependency's name that is also a first-party module's — beside this
                        // module, at the crate root, or in a module one of its globs opens — is
                        // ambiguous, and not shown to be another crate's.
                        let shadowed = self.module_paths.contains(&format!("{module}::{root}"))
                            || self.module_paths.contains(&format!("crate::{root}"))
                            || self.module_globs.get(module).is_some_and(|globs| {
                                globs.iter().any(|glob| {
                                    self.module_of(module, glob).is_some_and(|opened| {
                                        self.module_paths.contains(&format!("{opened}::{root}"))
                                    })
                                })
                            });
                        ["std", "core", "alloc"].contains(&root.as_str())
                            || (self.dependencies.contains(root) && !shadowed)
                    }
                    [] => false,
                };
                if !settled {
                    return false;
                }
            }
            if next.is_empty() {
                return true;
            }
            frontier = next;
        }
        false
    }

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

/// How many times a body's `use` leaves may be followed into one another before the guard gives up
/// on a path — and refuses it, never passes it.
const HOPS: usize = 8;

/// **The `use` leaves of one block**, each local name with every path it was written for (two
/// leaves of one name in one block are both kept), and the globs written there.
#[derive(Default)]
struct UseScope {
    names: BTreeMap<String, Vec<Vec<String>>>,
    globs: Vec<Vec<String>>,
    /// The block's type aliases, each name with the type it stands for.
    aliases: BTreeMap<String, Vec<syn::Type>>,
}

impl UseScope {
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
            syn::UseTree::Name(name) if name.ident == "self" => {
                // `use a::b::{self}` brings `b`, the module, under its own name.
                if let Some(module) = path.last() {
                    self.names
                        .entry(module.clone())
                        .or_default()
                        .push(path.clone());
                }
            }
            syn::UseTree::Name(name) => {
                let mut full = path.clone();
                full.push(name.ident.to_string());
                self.names
                    .entry(name.ident.to_string())
                    .or_default()
                    .push(full);
            }
            syn::UseTree::Rename(rename) => {
                let full = if rename.ident == "self" {
                    path.clone()
                } else {
                    let mut full = path.clone();
                    full.push(rename.ident.to_string());
                    full
                };
                self.names
                    .entry(rename.rename.to_string())
                    .or_default()
                    .push(full);
            }
            syn::UseTree::Glob(_) => self.globs.push(path.clone()),
        }
    }
}

/// Whether `attrs` keep an item out of a product build: `#[cfg(test)]` or `#[test]`.
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        attribute.path().is_ident("test")
            || (attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<syn::Meta>()
                    .is_ok_and(|meta| meta.path().is_ident("test")))
    })
}

/// A module-level item, as a refusal names the place a nested impl is written in.
fn describe(item: &syn::Item) -> String {
    match item {
        syn::Item::Fn(item) => format!("fn {}", item.sig.ident),
        syn::Item::Impl(item) => match &*item.self_ty {
            syn::Type::Path(path) => format!("impl {}", segments(&path.path).join("::")),
            _ => String::from("impl"),
        },
        syn::Item::Const(item) => format!("const {}", item.ident),
        syn::Item::Static(item) => format!("static {}", item.ident),
        syn::Item::Struct(item) => format!("struct {}", item.ident),
        syn::Item::Enum(item) => format!("enum {}", item.ident),
        syn::Item::Union(item) => format!("union {}", item.ident),
        syn::Item::Trait(item) => format!("trait {}", item.ident),
        syn::Item::Type(item) => format!("type {}", item.ident),
        _ => String::from("an item"),
    }
}

/// **RULE A: every impl that is not at module level, found by one walk over the whole file.**
///
/// The walk enters every module-level item of a product file (a `#[cfg(test)]` item or module and
/// the seam's own module left out) and records each `ItemImpl` it meets below module level —
/// in a function's signature or body, a type position (an array length, a const-generic argument
/// or default, a field's type, an enum's discriminant), a `const`'s or `static`'s type or
/// initializer, a trait's default body: wherever syn can reach one. Each is read **whole**, in its
/// own scope (Rust gives a nested item none of the scope around it), and refused by name — no
/// listing of the place around it covers it — when its trait is a door of the seam, when its self
/// type reaches a holder (spelled, or through the body's `use` leaves and type aliases followed to
/// a fixed point: its methods would be no method the guard's native-only reading knows), or when
/// anything in it names a holder or the native backend for the native backend, a method body
/// reaching a native-only method included. In a body the guard does not read for mentions (a
/// native block's method, a listed function or `const`) no name is introduced: every `use`,
/// `type` and `extern crate`, and any item spelling a holder, is refused; a `mod` below module
/// level is refused anywhere.
///
/// **RULE B: a nested impl's trait is resolved, or shown to be another crate's, or refused.** Each
/// block keeps its own `use` leaves — every path per local name, a `{self}` leaf under the
/// module's own name — and a nested trait impl's trait path is expanded through the blocks around
/// it, innermost first, until its first segment is no body's name; every expansion is resolved
/// with the module's [`TraitResolver`]. One the resolver does not follow passes only when it is
/// provably another crate's ([`FileWalk::foreign`]). A path that does not settle within [`HOPS`],
/// any other unresolved path, and a body `use` glob in scope of a nested trait impl are refused by
/// name: "unsupported" is a diagnostic, never silence.
struct FileWalk<'r, 'a> {
    reading: &'r Reading<'a>,
    file: String,
    /// The module paths of the module being walked (a file may be reached by several).
    modules: Vec<String>,
    outer: String,
    depth: usize,
    /// Items entered below module level, around the walk's position.
    nesting: usize,
    /// The native list, for knowing which bodies the guard does not read for mentions.
    allowed: &'r Allowed,
    /// Whether the walk is in a body whose mentions the guard does not read: a native block's
    /// method, a listed function, a listed `const`.
    unread: bool,
    /// The module-level impl being walked: whether it is written for the native backend, and the
    /// names its self type is spelled with ([`type_names`]: every path's last segment, generics
    /// and lifetimes aside).
    block: Option<(bool, BTreeSet<String>)>,
    scopes: Vec<UseScope>,
    refusals: Vec<String>,
}

impl FileWalk<'_, '_> {
    /// A path's expansions through the body's `use` leaves, innermost block first, to a fixed
    /// point; `Err` when it does not settle within [`HOPS`].
    fn expand(&self, path: Vec<String>) -> Result<Vec<Vec<String>>, ()> {
        let mut frontier = vec![path];
        let mut settled = Vec::new();
        for _ in 0..HOPS {
            let mut next = Vec::new();
            for path in frontier {
                let Some(first) = path.first() else {
                    continue;
                };
                let found = self
                    .scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.names.get(first));
                match found {
                    None => settled.push(path),
                    Some(leaves) => {
                        for leaf in leaves {
                            next.push(leaf.iter().chain(path.iter().skip(1)).cloned().collect());
                        }
                    }
                }
            }
            if next.is_empty() {
                return Ok(settled);
            }
            frontier = next;
        }
        Err(())
    }

    /// **The holders a self type reaches** through the body's `use` leaves and type aliases,
    /// composed to a fixed point: a path is expanded through the `use` leaves; an expansion — or the
    /// written path — that is one segment naming a body alias is followed into the alias's type,
    /// whose own paths are read the same way. `Err` when that does not settle within [`HOPS`].
    fn holders_behind(&self, written: &syn::Type) -> Result<Vec<String>, ()> {
        let mut found = Vec::new();
        let mut types = vec![written.clone()];
        for _ in 0..HOPS {
            let mut next = Vec::new();
            for found_type in &types {
                let mut inner = Vec::new();
                AnyHolder(&self.reading.holders, &mut inner).visit_type(found_type);
                found.extend(inner);
                let mut paths = Vec::new();
                AllTypePaths(&mut paths).visit_type(found_type);
                for written in paths {
                    let mut names = vec![written.clone()];
                    if let Ok(expansions) = self.expand(written.clone()) {
                        names.extend(expansions);
                    } else {
                        return Err(());
                    }
                    for name in names {
                        if name
                            .last()
                            .is_some_and(|last| self.reading.holders.contains_key(last))
                        {
                            found.push(name.last().cloned().unwrap_or_default());
                        }
                        if let [single] = name.as_slice()
                            && let Some(aliased) = self
                                .scopes
                                .iter()
                                .rev()
                                .find_map(|scope| scope.aliases.get(single))
                        {
                            next.extend(aliased.iter().cloned());
                        }
                    }
                }
            }
            if next.is_empty() {
                return Ok(found);
            }
            types = next;
        }
        Err(())
    }

    /// **An item introducing a name below module level**, judged where it stands: a `mod` is
    /// refused anywhere (its `use`s and names have no scope the guard reads); in a body whose
    /// mentions the guard does not read, every `use`, `type`, `extern crate`, and any other item
    /// spelling a holder's name, is refused — a listed body introduces no names.
    fn introduced(&mut self, node: &syn::Item) {
        let (kind, name, line) = match node {
            syn::Item::Mod(item) => (
                "module",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Use(item) => {
                let mut scope = UseScope::default();
                scope.leaves(&item.tree, &mut Vec::new());
                let names: Vec<String> = scope
                    .names
                    .keys()
                    .cloned()
                    .chain(
                        scope
                            .globs
                            .iter()
                            .map(|glob| format!("{}::*", glob.join("::"))),
                    )
                    .collect();
                ("use", names.join(", "), item.use_token.span.start().line)
            }
            syn::Item::Type(item) => (
                "type",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::ExternCrate(item) => (
                "extern crate",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Fn(item) => (
                "fn",
                item.sig.ident.to_string(),
                item.sig.ident.span().start().line,
            ),
            syn::Item::Struct(item) => (
                "struct",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Enum(item) => (
                "enum",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Union(item) => (
                "union",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Trait(item) => (
                "trait",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Const(item) => (
                "const",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            syn::Item::Static(item) => (
                "static",
                item.ident.to_string(),
                item.ident.span().start().line,
            ),
            _ => return,
        };
        let at = format!(
            "the {kind} `{name}` at {}:{line} inside `{}`",
            self.file, self.outer
        );
        if kind == "module" {
            self.refusals.push(format!(
                "{at} — a module below module level is a shape the guard does not read"
            ));
            return;
        }
        if kind == "extern crate" {
            self.refusals.push(format!(
                "{at} — an `extern crate` is refused in product files: the guard reads `use` \
                 items, not crate aliases"
            ));
            return;
        }
        if !self.unread {
            return;
        }
        let names_a_holder = {
            let mut idents = HolderIdents(&self.reading.holders, BTreeSet::new());
            idents.visit_item(node);
            !idents.1.is_empty()
        };
        if ["use", "type"].contains(&kind) || names_a_holder {
            self.refusals.push(format!(
                "{at} — a body the guard does not read for mentions (a native block's method, a \
                 listed function or `const`) introduces no names"
            ));
        }
    }

    /// **A body's `use` that reaches the seam, or the `paste` crate, is refused by name**: each
    /// leaf (and glob) is expanded through the body's leaves to a fixed point; one passing through
    /// the seam module or resolving to a seam trait aliases the seam in a body, which a body does
    /// not do; one rooted at `paste` brings in name construction the guard cannot read.
    fn body_use(&mut self, used: &syn::ItemUse) {
        let mut scope = UseScope::default();
        scope.leaves(&used.tree, &mut Vec::new());
        let line = used.use_token.span.start().line;
        let modules: Vec<String> = self.modules.clone();
        let modules: Vec<&str> = modules.iter().map(String::as_str).collect();
        let leaves: Vec<(String, Vec<String>)> = scope
            .names
            .iter()
            .flat_map(|(name, paths)| paths.iter().map(|path| (name.clone(), path.clone())))
            .chain(
                scope
                    .globs
                    .iter()
                    .map(|glob| (format!("{}::*", glob.join("::")), glob.clone())),
            )
            .collect();
        for (name, path) in leaves {
            let at = format!(
                "the use `{name}` at {}:{line} inside `{}`",
                self.file, self.outer
            );
            let Ok(expansions) = self.expand(path.clone()) else {
                self.refusals.push(format!(
                    "{at} does not settle within {HOPS} hops — the guard does not follow it"
                ));
                continue;
            };
            for expansion in expansions {
                if expansion.first().is_some_and(|root| root == "paste") {
                    self.refusals.push(format!(
                        "{at} brings in the `paste` crate, which builds names the guard cannot \
                         read — refused wherever it is written"
                    ));
                }
                let mut doors: BTreeSet<String> = BTreeSet::new();
                for module in &modules {
                    let found = match self.reading.resolver.resolve_checked(
                        self.reading.package,
                        module,
                        &expansion,
                    ) {
                        Ok(found) => found,
                        Err(ResolveCutoff(found)) => {
                            self.refusals.push(format!(
                                "{at} reaches `{}` through a chain past the resolver's hop limit \
                                 — refused, never passed",
                                expansion.join("::")
                            ));
                            found
                        }
                    };
                    doors.extend(
                        found
                            .iter()
                            .filter(|id| {
                                id.package == self.reading.package && id.module_path == SEAM
                            })
                            .map(ToString::to_string),
                    );
                }
                if expansion.iter().any(|segment| segment == seam_name()) {
                    doors.insert(expansion.join("::"));
                }
                if let Some(door) = doors.into_iter().next() {
                    self.refusals.push(format!(
                        "{at} reaches the seam ({door}) — a body does not alias the seam"
                    ));
                }
            }
        }
    }

    fn nested(&mut self, block: &syn::ItemImpl) {
        let reading = self.reading;
        let modules: Vec<&str> = self.modules.iter().map(String::as_str).collect();
        let line = block.impl_token.span.start().line;
        let place = format!("the impl at {}:{line} inside `{}`", self.file, self.outer);
        // Its self type, with the body's `use` leaves and type aliases followed: any holder there
        // makes a block of the holder below module level.
        let mut holders = Vec::new();
        AnyHolder(&reading.holders, &mut holders).visit_type(&block.self_ty);
        match self.holders_behind(&block.self_ty) {
            Ok(behind) => holders.extend(behind),
            Err(()) => self.refusals.push(format!(
                "{place} has a self type whose body `use` leaves and aliases do not settle within \
                 {HOPS} hops — the guard does not follow them"
            )),
        }
        holders.sort();
        holders.dedup();
        if !holders.is_empty() {
            self.refusals.push(format!(
                "{place} is a block of `{}` — a holder's block is written at module level",
                holders.join(", ")
            ));
        }
        // Whole, in its own scope.
        let generic = reading.parameters(&block.generics, &modules);
        let named = native_mentions(reading, &modules, &generic, |visit| {
            visit.visit_item_impl(block);
        });
        if !named.is_empty() {
            self.refusals.push(format!(
                "{place} names {} for the native backend — an impl below module level is read \
                 whole, in its own scope, and no listing covers it",
                named.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        // Its trait, through the body's `use` leaves.
        let Some((_, path, _)) = &block.trait_ else {
            return;
        };
        let written = segments(path);
        let shown = written.join("::");
        if self.scopes.iter().any(|scope| !scope.globs.is_empty()) {
            self.refusals.push(format!(
                "{place} implements `{shown}` with a body `use` glob in scope — the guard does not \
                 read globs in bodies"
            ));
        }
        let Ok(expansions) = self.expand(written.clone()) else {
            self.refusals.push(format!(
                "{place} implements `{shown}` through body `use` leaves that do not settle within \
                 {HOPS} hops — the guard does not follow them"
            ));
            return;
        };
        let mut doors = BTreeSet::new();
        // A path spelled as one of the seam's traits that the resolver did not follow is a door
        // all the same — only when it followed nothing, as at module level; one it did follow is
        // named by its identity (a first-party trait of a seam trait's name elsewhere is that
        // trait, not a door).
        let mut spelled = BTreeSet::new();
        for expansion in &expansions {
            let mut resolved: BTreeSet<TraitId> = BTreeSet::new();
            for module in &modules {
                match reading
                    .resolver
                    .resolve_checked(reading.package, module, expansion)
                {
                    Ok(found) => resolved.extend(found),
                    Err(ResolveCutoff(found)) => {
                        resolved.extend(found);
                        self.refusals.push(format!(
                            "{place} implements `{shown}`, which reaches `{}` through a chain past \
                             the resolver's hop limit — refused, never passed",
                            expansion.join("::")
                        ));
                    }
                }
            }
            doors.extend(
                resolved
                    .iter()
                    .filter(|id| id.package == reading.package && id.module_path == SEAM)
                    .map(ToString::to_string),
            );
            if resolved.is_empty()
                && expansion
                    .last()
                    .is_some_and(|last| reading.seam_traits.contains(last))
            {
                spelled.insert(expansion.join("::"));
            }
            if resolved.is_empty() && !reading.foreign(expansion, &modules) {
                self.refusals.push(format!(
                    "{place} implements `{shown}`, which reaches `{}`, a trait the guard cannot \
                     resolve and cannot show to be another crate's — the guard does not follow it",
                    expansion.join("::")
                ));
            }
        }
        if doors.is_empty() {
            doors = spelled;
        }
        for door in doors {
            self.refusals.push(format!(
                "{place} implements {door} — a door of the seam is implemented in the seam"
            ));
        }
    }
}

impl<'ast> Visit<'ast> for FileWalk<'_, '_> {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        if self.depth == 0 {
            if let syn::Item::Mod(module) = node {
                let attrs_test = test_only(&module.attrs);
                let inner: Vec<String> = self
                    .modules
                    .iter()
                    .map(|path| format!("{path}::{}", module.ident))
                    .collect();
                if attrs_test || inner.iter().any(|path| path == SEAM) {
                    return;
                }
                let outer = std::mem::replace(&mut self.modules, inner);
                syn::visit::visit_item_mod(self, module);
                self.modules = outer;
                return;
            }
            let attrs: &[syn::Attribute] = match node {
                syn::Item::Fn(item) => &item.attrs,
                syn::Item::Impl(item) => &item.attrs,
                syn::Item::Const(item) => &item.attrs,
                syn::Item::Static(item) => &item.attrs,
                syn::Item::Struct(item) => &item.attrs,
                syn::Item::Enum(item) => &item.attrs,
                syn::Item::Union(item) => &item.attrs,
                syn::Item::Trait(item) => &item.attrs,
                syn::Item::Type(item) => &item.attrs,
                _ => &[],
            };
            if test_only(attrs) {
                return;
            }
            if let syn::Item::ExternCrate(item) = node {
                self.refusals.push(format!(
                    "the extern crate `{}` at {}:{} — an `extern crate` is refused in product \
                     files: the guard reads `use` items, not crate aliases",
                    item.ident,
                    self.file,
                    item.ident.span().start().line
                ));
                return;
            }
            let owner = self.modules.join(", ");
            let allowed = self.allowed;
            let file = self.file.clone();
            let listed = |kind: &str, owner: &str, name: String| {
                allowed.contains(&(kind.to_owned(), owner.to_owned(), name))
            };
            self.unread = match node {
                syn::Item::Fn(item) => listed("function", &owner, item.sig.ident.to_string()),
                syn::Item::Const(item) => listed("const", &file, item.ident.to_string()),
                syn::Item::Static(item) => listed("const", &file, item.ident.to_string()),
                _ => false,
            };
            self.block = match node {
                syn::Item::Impl(item) => {
                    let native = holder_path(item, &self.reading.holders).is_some_and(|last| {
                        let modules: Vec<&str> = self.modules.iter().map(String::as_str).collect();
                        let generic = self.reading.parameters(&item.generics, &modules);
                        let holder = last.ident.to_string();
                        !written_for_a_parameter(last, self.reading.holders[&holder], &generic)
                    });
                    Some((native, type_names(&item.self_ty)))
                }
                _ => None,
            };
            self.outer = describe(node);
            self.depth += 1;
            syn::visit::visit_item(self, node);
            self.depth -= 1;
            self.unread = false;
            self.block = None;
            return;
        }
        self.introduced(node);
        if let syn::Item::Impl(block) = node {
            self.nested(block);
        }
        self.nesting += 1;
        syn::visit::visit_item(self, node);
        self.nesting -= 1;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if test_only(&node.attrs) {
            return;
        }
        // A module-level impl's own method: unread when its block is native or the list names it.
        if self.nesting == 0
            && let Some((native, owner)) = &self.block
        {
            // Any listed row of this method's name whose owner spells the same type — the same
            // paths' last segments, generics and lifetimes aside, whatever the shape (`&Plain`,
            // `&'a Plain`, a tuple, a slice, `dyn Trait`) — makes the body unread.
            let method = node.sig.ident.to_string();
            let listed = self.allowed.iter().any(|(kind, row_owner, name)| {
                kind == "function" && *name == method && owner_names(row_owner) == *owner
            });
            self.unread = *native || listed;
            syn::visit::visit_impl_item_fn(self, node);
            self.unread = false;
            return;
        }
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if test_only(&node.attrs) {
            return;
        }
        // A module-level trait's default method: unread when the list names it (a trait method's
        // row is owned by its module).
        if self.nesting == 0 && self.outer.starts_with("trait ") {
            self.unread = self.allowed.contains(&(
                "function".to_owned(),
                self.modules.join(", "),
                node.sig.ident.to_string(),
            ));
            syn::visit::visit_trait_item_fn(self, node);
            self.unread = false;
            return;
        }
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        // A block's `use` items are in scope in the whole block, wherever they stand in it.
        let mut scope = UseScope::default();
        for statement in &node.stmts {
            match statement {
                syn::Stmt::Item(syn::Item::Use(used)) => {
                    scope.leaves(&used.tree, &mut Vec::new());
                }
                syn::Stmt::Item(syn::Item::Type(alias)) => {
                    scope
                        .aliases
                        .entry(alias.ident.to_string())
                        .or_default()
                        .push((*alias.ty).clone());
                }
                _ => {}
            }
        }
        self.scopes.push(scope);
        for statement in &node.stmts {
            if let syn::Stmt::Item(syn::Item::Use(used)) = statement {
                self.body_use(used);
            }
        }
        syn::visit::visit_block(self, node);
        self.scopes.pop();
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

/// **The holders, native names and seam traits a macro's tokens name**, read lexically: every
/// identifier that is a holder's name or one of the seam's traits (a macro that writes
/// `impl WindowHost for …` implements a door wherever it is expanded), and every native name
/// standing first or as a member of a module ([`Reading::is_native`]).
fn macro_mentions(tokens: proc_macro2::TokenStream, reading: &Reading<'_>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => found.extend(macro_mentions(group.stream(), reading)),
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                if reading.holders.contains_key(&name) || reading.seam_traits.contains(&name) {
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
fn judge(
    index: &Index,
    package: &str,
    allowed: &Allowed,
    dependencies: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    CUTOFFS.with(|cutoffs| cutoffs.borrow_mut().clear());
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
                && block.trait_path().is_some_and(|path| {
                    block.module_paths().into_iter().any(|module| {
                        checked(
                            &resolver,
                            package,
                            module,
                            path,
                            "the trait of a native candidate",
                        )
                        .contains(&backend)
                    })
                })
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
    let mut seam_traits: BTreeSet<String> = index
        .traits()
        .iter()
        .filter(|declared| declared.module_paths().contains(&SEAM))
        .map(|declared| declared.name().to_owned())
        .collect();
    let mut module_uses: BTreeMap<String, BTreeMap<String, Vec<Vec<String>>>> = BTreeMap::new();
    let mut module_globs: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    let mut early_refusals = Vec::new();
    for declared in index.uses() {
        if !in_the_product(index, declared.whole()) {
            continue;
        }
        for module in declared.module_paths() {
            module_globs
                .entry(module.to_owned())
                .or_default()
                .extend(declared.globs().iter().cloned());
            for (name, path) in declared.names() {
                if path.first().is_some_and(|root| root == "paste") {
                    early_refusals.push(format!(
                        "`use {} as {name}` in {module} brings in the `paste` crate, which builds \
                         names the guard cannot read — refused wherever it is written",
                        path.join("::")
                    ));
                }
            }
            for (name, path) in declared.names() {
                module_uses
                    .entry(module.to_owned())
                    .or_default()
                    .entry(name.clone())
                    .or_default()
                    .push(path.clone());
                let resolved = match resolver.resolve_checked(package, module, path) {
                    Ok(found) => found,
                    Err(ResolveCutoff(found)) => {
                        early_refusals.push(format!(
                            "`use {} as {name}` in {module} chains past the resolver's hop limit — \
                             refused, never passed",
                            path.join("::")
                        ));
                        found
                    }
                };
                if resolved
                    .iter()
                    .any(|id| id.package == package && id.module_path == SEAM)
                {
                    seam_traits.insert(name.clone());
                }
            }
        }
    }
    let reading = Reading {
        holders: holders.clone(),
        natives: natives.clone(),
        seam_traits,
        module_uses,
        module_globs,
        module_paths: index
            .modules()
            .iter()
            .flat_map(|module| module.module_paths().iter().cloned())
            .collect(),
        dependencies: dependencies.clone(),
        native_only,
        resolver: &resolver,
        package,
        backends: &backends,
    };

    let mut refusals = early_refusals;
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

        // The doors it implements, through `checked`: a cutoff here is refused by name (outside the
        // seam the module-level trait check below refuses it too; inside it, only this does).
        let implemented: BTreeSet<TraitId> = block
            .trait_path()
            .map(|path| {
                modules
                    .iter()
                    .flat_map(|module| checked(&resolver, package, module, path, "the trait"))
                    .collect()
            })
            .unwrap_or_default();
        for door in implemented {
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
        // Its trait: resolved, or shown to be another crate's, or refused (RULE B at module
        // level); a path spelled as a seam trait the resolver did not follow is a door all the
        // same.
        if let Some(path) = block.trait_path()
            && !seam_block
        {
            let shown = path.join("::");
            let mut resolved = BTreeSet::new();
            let mut cut = false;
            for module in &modules {
                match resolver.resolve_checked(package, module, path) {
                    Ok(found) => resolved.extend(found),
                    Err(ResolveCutoff(found)) => {
                        resolved.extend(found);
                        cut = true;
                    }
                }
            }
            if cut {
                refusals.push(format!(
                    "the impl of `{shown}` for `{owner}` in {}: its path chains past the \
                     resolver's hop limit — refused, never passed",
                    modules.join(", ")
                ));
            } else if resolved.is_empty() {
                if path
                    .last()
                    .is_some_and(|last| reading.seam_traits.contains(last))
                {
                    refusals.push(format!(
                        "the impl of `{shown}` for `{owner}` in {} is spelled as a door of the \
                         seam — a door of the seam is implemented in the seam",
                        modules.join(", ")
                    ));
                } else if !reading.foreign(path, &modules) {
                    refusals.push(format!(
                        "the impl of `{shown}` for `{owner}` in {}: a trait the guard cannot \
                         resolve and cannot show to be another crate's — the guard does not follow \
                         it",
                        modules.join(", ")
                    ));
                }
            }
        }
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
        // The whole item — generics, fields, an enum's discriminants — but for a backend
        // parameter's own default: a holder's declaration defaulting it to the native backend is
        // the seam's design.
        let generics = match &parsed {
            syn::Item::Struct(declared) => &declared.generics,
            syn::Item::Enum(declared) => &declared.generics,
            syn::Item::Union(declared) => &declared.generics,
            _ => continue,
        };
        let modules = item.module_paths();
        let generic = reading.parameters(generics, &modules);
        let mut read = parsed.clone();
        let read_generics = match &mut read {
            syn::Item::Struct(declared) => &mut declared.generics,
            syn::Item::Enum(declared) => &mut declared.generics,
            syn::Item::Union(declared) => &mut declared.generics,
            _ => continue,
        };
        for param in read_generics.type_params_mut() {
            if generic.contains(&param.ident.to_string()) {
                param.eq_token = None;
                param.default = None;
            }
        }
        let named = native_mentions(&reading, &modules, &generic, |visit| match &read {
            syn::Item::Struct(declared) => visit.visit_item_struct(declared),
            syn::Item::Enum(declared) => visit.visit_item_enum(declared),
            syn::Item::Union(declared) => visit.visit_item_union(declared),
            _ => {}
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

    // What only the files say: type aliases, `const` and `static` items, and every impl below
    // module level (RULE A, [`FileWalk`]).
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
        let mut walk = FileWalk {
            reading: &reading,
            file: name.clone(),
            modules: modules.clone(),
            outer: String::new(),
            depth: 0,
            nesting: 0,
            allowed,
            unread: false,
            block: None,
            scopes: Vec::new(),
            refusals: Vec::new(),
        };
        walk.visit_file(&parsed);
        refusals.extend(walk.refusals);
        let mut handles = HandleWalk {
            reading: &reading,
            modules: modules.clone(),
            block: None,
            item: None,
            scopes: Vec::new(),
            found: BTreeMap::new(),
        };
        handles.visit_file(&parsed);
        refusals.extend(handles.refusals());
        file_items(
            &parsed.items,
            &reading,
            &modules,
            &mut |kind, item_name, what| match kind {
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
        if ["paste", "concat_idents"].contains(&found.name()) {
            let at = index
                .locate(found.span().start())
                .expect("a macro is in a file of the index");
            let file = at
                .file
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            refusals.push(format!(
                "`{}!` at {file}:{}: it builds names from pieces, which the guard cannot read — \
                 refused wherever it is written",
                found.name(),
                at.line
            ));
        }
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

    refusals.extend(doors_handing_out_a_handle(index, &reading));

    let unused = allowed
        .iter()
        .filter(|row| !used.contains(*row))
        .map(|(kind, owner, name)| format!("{kind} {owner} {name}"))
        .collect();
    refusals.extend(CUTOFFS.with(|cutoffs| std::mem::take(&mut *cutoffs.borrow_mut())));
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
                let named = native_mentions(reading, &[], &none, |visit| {
                    visit.visit_item_const(constant)
                });
                if !named.is_empty() {
                    let names = named.into_iter().collect::<Vec<_>>().join(", ");
                    found("const", &constant.ident.to_string(), &names);
                }
            }
            syn::Item::Static(fixed) => {
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

/// **The names a type is spelled with**: the last segment of every path in it (a trait object's
/// bounds included), generic arguments and lifetimes aside — `&'a Plain` and `&Plain` are
/// `{Plain}`, `(&Plain, u8)` is `{Plain, u8}`, `Runtime<'_, B>` is `{Runtime}`.
fn type_names(written: &syn::Type) -> BTreeSet<String> {
    struct Names(BTreeSet<String>);
    impl<'ast> Visit<'ast> for Names {
        fn visit_path(&mut self, node: &'ast syn::Path) {
            if let Some(last) = node.segments.last() {
                self.0.insert(last.ident.to_string());
            }
        }
        fn visit_lifetime(&mut self, _: &'ast syn::Lifetime) {}
    }
    let mut names = Names(BTreeSet::new());
    names.visit_type(written);
    names.0
}

/// [`type_names`] for an owner as the index and the native list spell it (`&Plain`, `Runtime`):
/// parsed as a type when it is one, else its identifiers past `::`, generics and lifetimes aside.
fn owner_names(owner: &str) -> BTreeSet<String> {
    if let Ok(parsed) = syn::parse_str::<syn::Type>(owner) {
        return type_names(&parsed);
    }
    let mut depth = 0usize;
    let mut kept = String::new();
    for character in owner.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => kept.push(character),
            _ => {}
        }
    }
    kept.split(|character: char| {
        !(character.is_alphanumeric() || character == '_' || character == ':' || character == '\'')
    })
    .filter(|token| !token.is_empty() && !token.starts_with('\''))
    .filter_map(|token| token.rsplit("::").next().map(str::to_owned))
    .filter(|token| !token.is_empty())
    .collect()
}

/// Every path written in a type position, as its segments.
struct AllTypePaths<'a>(&'a mut Vec<Vec<String>>);

impl<'ast> Visit<'ast> for AllTypePaths<'_> {
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.0.push(segments(&node.path));
        syn::visit::visit_type_path(self, node);
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

/// The crate whose two handles stay behind the seam.
const PLATFORM: &str = "bt_platform";

/// **The platform's handles**: the window's own (`bt_platform::NativeWindow`) and its composition
/// tree's (`bt_platform::Compositor`).
const PLATFORM_HANDLES: &[&str] = &["NativeWindow", "Compositor"];

/// **RULE C: the window's platform handle stays behind the seam's doors**
/// (T-NATIVE-HANDLE-BEHIND-THE-SEAM).
///
/// Code written for the window — an item with a backend parameter in scope, or whose generics,
/// `where` clause or `impl Trait` arguments are bounded by one of the seam's traits (a function
/// over `&impl WindowHost`, a method of a generic holder block, a trait's default body, a holder's
/// declaration) — names neither `bt_platform::NativeWindow` nor `bt_platform::Compositor`, in its
/// signature, its body or a macro's tokens in it. A name is followed to what it stands for through
/// the body's `use` leaves, the module's own, `crate`/`self`/`super` paths and the module's globs
/// (a glob of `bt_platform` itself opens both names), to a fixed point within [`HOPS`]; a name
/// that does not settle is refused. Code written for no window (the platform's own glue: the web
/// engine's host, a lane's executor) may name them: no door hands it a handle to speak with.
struct HandleWalk<'r, 'a> {
    reading: &'r Reading<'a>,
    /// The module paths of the module being walked (a file may be reached by several).
    modules: Vec<String>,
    /// The impl block or trait around the walk: its name, and whether its own generics (or a
    /// trait's supertraits) put the window in scope.
    block: Option<(String, bool)>,
    /// The item being read: its name as a refusal says it, and whether it is written for the
    /// window.
    item: Option<(String, bool)>,
    scopes: Vec<UseScope>,
    found: BTreeMap<String, BTreeSet<String>>,
}

impl HandleWalk<'_, '_> {
    /// Whether `generics`, and `inputs` (a signature's argument and return types), put the window
    /// in scope: a backend parameter, or a bound naming one of the seam's traits.
    fn for_the_window(&self, generics: &syn::Generics, inputs: &[&syn::Type]) -> bool {
        struct Bounds<'s>(&'s BTreeSet<String>, bool);
        impl<'ast> Visit<'ast> for Bounds<'_> {
            fn visit_trait_bound(&mut self, node: &'ast syn::TraitBound) {
                if node
                    .path
                    .segments
                    .last()
                    .is_some_and(|last| self.0.contains(&last.ident.to_string()))
                {
                    self.1 = true;
                }
                syn::visit::visit_trait_bound(self, node);
            }
        }
        let modules: Vec<&str> = self.modules.iter().map(String::as_str).collect();
        if !self.reading.parameters(generics, &modules).is_empty() {
            return true;
        }
        let mut bounds = Bounds(&self.reading.seam_traits, false);
        bounds.visit_generics(generics);
        for input in inputs {
            bounds.visit_type(input);
        }
        bounds.1
    }

    /// **What `path` stands for, if it is one of the platform's handles** — `Err` when it does not
    /// settle within [`HOPS`].
    fn handle_named(&self, path: &[String]) -> Result<Option<String>, ()> {
        // Each candidate with the module it is read in, and whether the body's own `use` leaves
        // are in scope for it: only in the walk's own module, before a path has left it.
        let mut frontier: Vec<(Vec<String>, String, bool)> = self
            .modules
            .iter()
            .map(|module| (path.to_vec(), module.clone(), true))
            .collect();
        // A cycle among `use`s and globs names nothing more, and is not a cutoff.
        let mut seen: BTreeSet<(Vec<String>, String, bool)> = BTreeSet::new();
        for _ in 0..HOPS {
            let mut next = Vec::new();
            for (path, module, in_the_body) in frontier {
                if !seen.insert((path.clone(), module.clone(), in_the_body)) {
                    continue;
                }
                let Some(first) = path.first() else {
                    continue;
                };
                if let [root, name, ..] = path.as_slice()
                    && root == PLATFORM
                    && PLATFORM_HANDLES.contains(&name.as_str())
                {
                    return Ok(Some(format!("{root}::{name}")));
                }
                // Another crate's path names what that crate says, and neither handle is the
                // platform crate's but by that crate's own path.
                if path.len() > 1
                    && (["std", "core", "alloc"].contains(&first.as_str())
                        || self.reading.dependencies.contains(first))
                {
                    continue;
                }
                let rest = || path.iter().skip(1).cloned();
                // A body's own `use` leaves, innermost block first.
                if let Some(leaves) = in_the_body
                    .then(|| {
                        self.scopes
                            .iter()
                            .rev()
                            .find_map(|scope| scope.names.get(first))
                    })
                    .flatten()
                    .filter(|leaves| leaves.iter().any(|leaf| leaf.as_slice() != [first.clone()]))
                {
                    for leaf in leaves {
                        next.push((
                            leaf.iter().cloned().chain(rest()).collect(),
                            module.clone(),
                            in_the_body,
                        ));
                    }
                    continue;
                }
                if ["crate", "self", "super"].contains(&first.as_str()) {
                    // The module the path's leading segments name, and the rest read there.
                    let mut opened = None;
                    for split in (1..path.len()).rev() {
                        if let Some(found) = self.reading.module_of(&module, &path[..split]) {
                            opened = Some((found, path[split..].to_vec()));
                            break;
                        }
                    }
                    if let Some((found, rest)) = opened {
                        next.push((rest, found, false));
                    }
                    continue;
                }
                let own: Vec<&Vec<String>> = self
                    .reading
                    .module_uses
                    .get(&module)
                    .and_then(|names| names.get(first))
                    .map(|leaves| {
                        leaves
                            .iter()
                            .filter(|leaf| leaf.as_slice() != [first.clone()])
                            .collect()
                    })
                    .unwrap_or_default();
                if !own.is_empty() {
                    for leaf in own {
                        next.push((
                            leaf.iter().cloned().chain(rest()).collect(),
                            module.clone(),
                            false,
                        ));
                    }
                    continue;
                }
                // A child module's name: the rest is read there.
                if path.len() > 1
                    && let Some(child) = self.reading.module_of(&module, &path[..1])
                {
                    next.push((path[1..].to_vec(), child, false));
                    continue;
                }
                if path.len() == 1 {
                    let globs = self
                        .scopes
                        .iter()
                        .filter(|_| in_the_body)
                        .flat_map(|scope| scope.globs.iter())
                        .chain(self.reading.module_globs.get(&module).into_iter().flatten());
                    for glob in globs {
                        if glob.as_slice() == [PLATFORM]
                            && PLATFORM_HANDLES.contains(&first.as_str())
                        {
                            return Ok(Some(format!("{PLATFORM}::{first}")));
                        }
                        if let Some(opened) = self.reading.module_of(&module, glob)
                            && self
                                .reading
                                .module_uses
                                .get(&opened)
                                .is_some_and(|names| names.contains_key(first))
                        {
                            next.push((path.clone(), opened, false));
                        }
                    }
                }
            }
            if next.is_empty() {
                return Ok(None);
            }
            frontier = next;
        }
        Err(())
    }

    /// Read `path` for the item being read, when that item is written for the window.
    fn judge_path(&mut self, path: Vec<String>) {
        let Some((item, true)) = &self.item else {
            return;
        };
        let item = item.clone();
        match self.handle_named(&path) {
            Ok(Some(named)) => {
                self.found.entry(item).or_default().insert(named);
            }
            Ok(None) => {}
            Err(()) => {
                self.found
                    .entry(item)
                    .or_default()
                    .insert(format!("`{}`, which does not settle", path.join("::")));
            }
        }
    }

    /// Every path a macro's tokens spell (`a::b::c`), read as [`Self::judge_path`] reads one.
    fn judge_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let mut at = 0;
        while at < tokens.len() {
            match &tokens[at] {
                TokenTree::Group(group) => {
                    self.judge_tokens(group.stream());
                    at += 1;
                }
                TokenTree::Ident(first) => {
                    let mut path = vec![first.to_string()];
                    at += 1;
                    while let [
                        TokenTree::Punct(a),
                        TokenTree::Punct(b),
                        TokenTree::Ident(next),
                        ..,
                    ] = &tokens[at..]
                        && a.as_char() == ':'
                        && b.as_char() == ':'
                    {
                        path.push(next.to_string());
                        at += 3;
                    }
                    self.judge_path(path);
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => at += 1,
            }
        }
    }

    /// Read one item: `name` as a refusal says it, written for the window or not.
    fn read(&mut self, name: String, for_the_window: bool, visit: impl FnOnce(&mut Self)) {
        let outer = self.item.replace((name, for_the_window));
        let block = self.block.take();
        visit(self);
        self.block = block;
        self.item = outer;
    }

    fn module_name(&self) -> String {
        self.modules.first().cloned().unwrap_or_default()
    }

    /// Each item read, with every handle it names.
    fn refusals(self) -> Vec<String> {
        self.found
            .into_iter()
            .map(|(item, named)| {
                format!(
                    "{item} names {} — the window's platform handle and its composition tree are \
                     the native backend's own: code written for the window speaks to the platform \
                     through the seam's doors",
                    named.into_iter().collect::<Vec<_>>().join(", ")
                )
            })
            .collect()
    }
}

impl<'ast> Visit<'ast> for HandleWalk<'_, '_> {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        let attrs = match node {
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Struct(item) => &item.attrs,
            syn::Item::Enum(item) => &item.attrs,
            syn::Item::Union(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            _ => return syn::visit::visit_item(self, node),
        };
        if !test_only(attrs) {
            syn::visit::visit_item(self, node);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let outer = self.modules.clone();
        self.modules = outer
            .iter()
            .map(|module| format!("{module}::{}", node.ident))
            .collect();
        syn::visit::visit_item_mod(self, node);
        self.modules = outer;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let inputs = signature_types(&node.sig);
        let window = self.for_the_window(&node.sig.generics, &inputs);
        let name = format!("`{}::{}`", self.module_name(), node.sig.ident);
        self.read(name, window, |walk| syn::visit::visit_item_fn(walk, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let window = self.for_the_window(&node.generics, &[&*node.self_ty]);
        let name = match &*node.self_ty {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map_or_else(String::new, |last| last.ident.to_string()),
            _ => String::from("impl"),
        };
        let outer = self.block.replace((name.clone(), window));
        let item = self.item.replace((format!("`impl {name}`"), window));
        syn::visit::visit_item_impl(self, node);
        self.item = item;
        self.block = outer;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let (owner, block) = self.block.clone().unwrap_or_default();
        let inputs = signature_types(&node.sig);
        let window = block || self.for_the_window(&node.sig.generics, &inputs);
        let name = format!("`{owner}::{}`", node.sig.ident);
        self.read(name, window, |walk| {
            syn::visit::visit_impl_item_fn(walk, node);
        });
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let supertraits: Vec<syn::Type> = node
            .supertraits
            .iter()
            .map(|bound| {
                syn::Type::TraitObject(syn::TypeTraitObject {
                    dyn_token: None,
                    bounds: std::iter::once(bound.clone()).collect(),
                })
            })
            .collect();
        let inputs: Vec<&syn::Type> = supertraits.iter().collect();
        let window = self.for_the_window(&node.generics, &inputs);
        let name = node.ident.to_string();
        let outer = self.block.replace((name.clone(), window));
        let item = self.item.replace((format!("the trait `{name}`"), window));
        syn::visit::visit_item_trait(self, node);
        self.item = item;
        self.block = outer;
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let (owner, block) = self.block.clone().unwrap_or_default();
        let inputs = signature_types(&node.sig);
        let window = block || self.for_the_window(&node.sig.generics, &inputs);
        let name = format!("`{owner}::{}`", node.sig.ident);
        self.read(name, window, |walk| {
            syn::visit::visit_trait_item_fn(walk, node);
        });
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let window = self.for_the_window(&node.generics, &[]);
        let name = format!("the type `{}::{}`", self.module_name(), node.ident);
        self.read(name, window, |walk| {
            syn::visit::visit_item_struct(walk, node)
        });
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let window = self.for_the_window(&node.generics, &[]);
        let name = format!("the type `{}::{}`", self.module_name(), node.ident);
        self.read(name, window, |walk| syn::visit::visit_item_enum(walk, node));
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        let window = self.for_the_window(&node.generics, &[]);
        let name = format!("the type `{}::{}`", self.module_name(), node.ident);
        self.read(name, window, |walk| {
            syn::visit::visit_item_union(walk, node)
        });
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        let mut scope = UseScope::default();
        for statement in &node.stmts {
            if let syn::Stmt::Item(syn::Item::Use(declared)) = statement {
                scope.leaves(&declared.tree, &mut Vec::new());
            }
        }
        self.scopes.push(scope);
        syn::visit::visit_block(self, node);
        self.scopes.pop();
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        self.judge_path(segments(node));
        syn::visit::visit_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.judge_tokens(node.tokens.clone());
        syn::visit::visit_macro(self, node);
    }

    fn visit_item_use(&mut self, _: &'ast syn::ItemUse) {}
}

/// A signature's argument and return types: where an `impl Trait` puts the window in scope.
fn signature_types(signature: &syn::Signature) -> Vec<&syn::Type> {
    signature
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Typed(typed) => Some(&*typed.ty),
            syn::FnArg::Receiver(_) => None,
        })
        .chain(match &signature.output {
            syn::ReturnType::Type(_, output) => Some(&**output),
            syn::ReturnType::Default => None,
        })
        .collect()
}

/// **No door of the seam hands a platform handle out**: every trait declared in the seam — its
/// methods' signatures, its associated types' bounds and defaults — names neither handle.
fn doors_handing_out_a_handle(index: &Index, reading: &Reading<'_>) -> Vec<String> {
    let mut refusals = Vec::new();
    for declared in index.traits() {
        if !in_the_product(index, declared.whole()) || !declared.module_paths().contains(&SEAM) {
            continue;
        }
        let parsed: syn::ItemTrait = syn::parse_str(index.text(declared.whole()))
            .unwrap_or_else(|error| panic!("the door `{}` parses: {error}", declared.name()));
        let mut walk = HandleWalk {
            reading,
            modules: vec![SEAM.to_owned()],
            block: None,
            item: Some((format!("the door `{SEAM}::{}`", declared.name()), true)),
            scopes: Vec::new(),
            found: BTreeMap::new(),
        };
        for member in &parsed.items {
            match member {
                syn::TraitItem::Fn(method) => walk.visit_signature(&method.sig),
                other => walk.visit_trait_item(other),
            }
        }
        refusals.extend(walk.found.into_iter().map(|(door, named)| {
            format!(
                "{door} names {} — no door of the seam hands the platform handle out",
                named.into_iter().collect::<Vec<_>>().join(", ")
            )
        }));
    }
    refusals
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
/// trait method's default body naming a native holder (the trait's scope is no exemption); impls
/// below module level of every kind (RULE A): a generic holder block reaching a native-only
/// method, a non-holder block doing so inside a listed function, a trait impl for a reference to
/// a native holder, a door inside an array length in a signature; an enum discriminant naming a
/// native holder; body `use`s (RULE B): a glob, a chain, one name for two traits in sibling
/// blocks, a `{self}` leaf; a macro whose tokens implement a door; a holder reached through a body
/// alias and a renaming body `use` inside the listed `create` and `listed_rebuild` (a listed body
/// introduces no names) and through a body alias in a body the guard reads; a module below module
/// level; a nested impl of a trait the guard cannot resolve or show to be another crate's; a macro
/// implementing a door through a renamed re-export; `paste!`; a body `use` aliasing a re-exported
/// door handed to a path macro (`implements!(H, Made)`); an `extern crate self as me` and a door
/// implemented through it at module level; a chain of renamed re-exports past the resolver's hop
/// limit and an impl through its far end; a body `use` of a body alias, composed; a module-level
/// impl rooted at a name that is both a dependency and a first-party module; the `paste` crate
/// under another name; a listed trait default body and a listed method behind a reference self
/// type, each introducing a name; a listed method behind a tuple self type holding a lifetime,
/// introducing a name; a holder bounded, and a backend trait's supertrait, through a chain of
/// re-exports of `Backend` past the resolver's hop limit; a seam type implementing that chain's far
/// end (the native list's decision, and the door check, each refuse it); the platform's handles
/// in code written for the window (RULE C, `handles.rs`): in a signature over a door, through a
/// renamed crate in a generic block's method, through the module's `use` and a body's renaming
/// `use`, in a macro's tokens, in a holder's field and through a glob of the platform crate; a
/// door of the seam handing the handle out; and a list row nothing uses. A
/// module-level impl of a dependency's trait passes, and so do a module-level and a nested impl of
/// a first-party trait spelled as a seam trait, which the resolver follows to that trait. The generic blocks (the bound written
/// directly, through the prelude's glob, through a rename, in a `where` clause and through a
/// supertrait), a holder built by inference inside a generic function, a trait method's default
/// body written for the trait's own backend parameter and for a backend trait's `Self`, an enum's
/// variant spelled as the backend, the listed native method, function and `const`, a type with no backend and a
/// test module's native block pass; so do the platform's own glue naming both handles, and code
/// written for the window naming the backend's associated types and the fixture's own
/// `NativeWindow`.
#[test]
fn every_road_around_the_window_seam_is_named() {
    let index = fixture();
    let allowed = Allowed::from([
        row("method", "Runtime", "create"),
        row("method", "Runtime", "retired"),
        row("function", "crate", "listed_rebuild"),
        row("const", "lib.rs", "_"),
        row("function", "crate", "listed_default"),
        row("function", "&Plain", "listed_on_a_reference"),
        row("function", "(&'a Plain, u8)", "listed_on_a_tuple"),
    ]);
    let (refusals, unused) = judge(
        &index,
        "window-seam",
        &allowed,
        &BTreeSet::from(["elsewhere".to_owned(), "outside".to_owned()]),
    );
    assert_eq!(
        refusals,
        [
            "`Indirect::sneaks_through_a_supertrait` is written for the native backend in \
             crate::runtime — the window is reached through the seam",
            "`Runtime::reaches_the_tree` names bt_platform::Compositor \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`Runtime::sneaks_past_the_seam` is written for the native backend in crate::runtime \
             — the window is reached through the seam",
            "`Runtime::takes_the_native` names Runtime for the native backend — the window is \
             reached through the seam",
            "`Shell::fmt` names Native, Shell for the native backend — the window is reached \
             through the seam",
            "`crate::handles::globbed::through_the_glob` names bt_platform::Compositor \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`crate::handles::hands_out_the_handle` names bt_platform::NativeWindow \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`crate::handles::holds_a_handle` names bt_platform::NativeWindow \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`crate::handles::renames_the_handle` names bt_platform::NativeWindow \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`crate::handles::says_it_in_a_macro` names bt_platform::Compositor \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "`crate::runtime::aliases_a_holder_inside` names Runtime for the native backend — \
             the window is reached through the seam",
            "`crate::runtime::builds_by_a_literal` names Shell for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::builds_by_inference` names Runtime for the native backend — the \
             window is reached through the seam",
            "`crate::runtime::composes_a_use_and_an_alias` names Runtime for the native backend \
             — the window is reached through the seam",
            "`crate::runtime::generic_builds` names Runtime for the native backend — the window \
             is reached through the seam",
            "`crate::runtime::hides_a_native_block` names Native, Runtime for the native backend \
             — the window is reached through the seam",
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
            "`macro_rules! door_through_a_reexport` at runtime.rs:384: a macro's tokens name \
             Host — the guard does not read macros",
            "`macro_rules! makes_a_door` at runtime.rs:350: a macro's tokens name WindowHost — \
             the guard does not read macros",
            "`paste!` at runtime.rs:394: it builds names from pieces, which the guard cannot \
             read — refused wherever it is written",
            "`use Bk16 as Bk17` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Bk17 as Bk18` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Bk18 as Bk19` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Bk19 as Bk20` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Hop16 as Hop17` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Hop17 as Hop18` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Hop18 as Hop19` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use Hop19 as Hop20` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "`use crate::Runtime as Rt` in crate::runtime renames the holder Runtime — the guard \
             reads holders by their own name; write the holder",
            "`use crate::window_seam as ws` in crate::runtime renames window_seam — the guard \
             reads the native backend and the seam by their own names",
            "`use crate::window_seam::Native as N` in crate::runtime renames Native — the guard \
             reads the native backend and the seam by their own names",
            "`use paste::paste as glue` in crate::runtime brings in the `paste` crate, which \
             builds names the guard cannot read — refused wherever it is written",
            "`vec!` at runtime.rs:138: a macro's tokens name Runtime — the guard does not read \
             macros",
            "an impl for `Runtime` in crate::runtime names Native, Shell for the native backend \
             in its generics — the window is reached through the seam",
            "an impl in crate::runtime is written for a type that names Native, Runtime for the \
             native backend without being its path — a shape the window-seam guard does not read",
            "the `const` or `static` `PLANTED` in runtime.rs names Runtime for the native \
             backend — the window is reached through the seam",
            "the associated const `Plain::NATIVE` in crate::runtime names Runtime for the native \
             backend — the window is reached through the seam",
            "the associated type `Shell::Item` in crate::runtime names Native, Shell for the \
             native backend — the window is reached through the seam",
            "the bound `crate::elsewhere::Bk20` in crate chains past the resolver's hop limit — \
             refused, never passed",
            "the door `crate::window_seam::HandleDoor` names bt_platform::NativeWindow — no \
             door of the seam hands the platform handle out",
            "the extern crate `self` at runtime.rs:415 — an `extern crate` is refused in product \
             files: the guard reads `use` items, not crate aliases",
            "the impl at lib.rs:102 inside `fn listed_rebuild` is a block of `Runtime` — a \
             holder's block is written at module level",
            "the impl at lib.rs:106 inside `fn listed_rebuild` is a block of `Runtime` — a \
             holder's block is written at module level",
            "the impl at lib.rs:110 inside `fn listed_rebuild` names Runtime for the native \
             backend — an impl below module level is read whole, in its own scope, and no \
             listing covers it",
            "the impl at lib.rs:61 inside `impl Runtime` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at lib.rs:66 inside `impl Runtime` is a block of `Runtime` — a holder's \
             block is written at module level",
            "the impl at lib.rs:66 inside `impl Runtime` names Runtime for the native backend — \
             an impl below module level is read whole, in its own scope, and no listing covers \
             it",
            "the impl at lib.rs:74 inside `impl Runtime` is a block of `Runtime` — a holder's \
             block is written at module level",
            "the impl at lib.rs:80 inside `impl Runtime` is a block of `Runtime` — a holder's \
             block is written at module level",
            "the impl at lib.rs:84 inside `impl Runtime` is a block of `Runtime` — a holder's \
             block is written at module level",
            "the impl at lib.rs:84 inside `impl Runtime` names Native, Runtime for the native \
             backend — an impl below module level is read whole, in its own scope, and no \
             listing covers it",
            "the impl at lib.rs:97 inside `fn listed_rebuild` is a block of `Runtime` — a \
             holder's block is written at module level",
            "the impl at lib.rs:97 inside `fn listed_rebuild` names Native, Runtime for the \
             native backend — an impl below module level is read whole, in its own scope, and no \
             listing covers it",
            "the impl at runtime.rs:243 inside `fn hides_a_door` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:252 inside `fn hides_a_renamed_door` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:259 inside `fn hides_a_native_block` is a block of `Runtime` \
             — a holder's block is written at module level",
            "the impl at runtime.rs:259 inside `fn hides_a_native_block` names Native, Runtime \
             for the native backend — an impl below module level is read whole, in its own \
             scope, and no listing covers it",
            "the impl at runtime.rs:267 inside `const _` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:276 inside `fn hides_a_reexported_door` implements \
             window-seam crate::window_seam::WindowHost — a door of the seam is implemented in \
             the seam",
            "the impl at runtime.rs:285 inside `fn sized` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:305 inside `fn hides_behind_a_glob` implements Host — a door \
             of the seam is implemented in the seam",
            "the impl at runtime.rs:305 inside `fn hides_behind_a_glob` implements `Host` with a \
             body `use` glob in scope — the guard does not read globs in bodies",
            "the impl at runtime.rs:305 inside `fn hides_behind_a_glob` implements `Host`, which \
             reaches `Host`, a trait the guard cannot resolve and cannot show to be another \
             crate's — the guard does not follow it",
            "the impl at runtime.rs:315 inside `fn hides_behind_a_chain` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:325 inside `fn hides_in_a_sibling_block` implements \
             window-seam crate::window_seam::WindowHost — a door of the seam is implemented in \
             the seam",
            "the impl at runtime.rs:344 inside `fn hides_behind_self` implements window-seam \
             crate::window_seam::WindowHost — a door of the seam is implemented in the seam",
            "the impl at runtime.rs:361 inside `fn aliases_a_holder_inside` is a block of \
             `Runtime` — a holder's block is written at module level",
            "the impl at runtime.rs:371 inside `fn hides_a_module` implements Host — a door of \
             the seam is implemented in the seam",
            "the impl at runtime.rs:371 inside `fn hides_a_module` implements `Host`, which \
             reaches `Host`, a trait the guard cannot resolve and cannot show to be another \
             crate's — the guard does not follow it",
            "the impl at runtime.rs:380 inside `fn implements_a_mystery` implements `Mystery`, \
             which reaches `Mystery`, a trait the guard cannot resolve and cannot show to be \
             another crate's — the guard does not follow it",
            "the impl at runtime.rs:427 inside `fn composes_a_use_and_an_alias` is a block of \
             `Runtime` — a holder's block is written at module level",
            "the impl of `Hop20` for `Far` in crate::elsewhere: its path chains past the \
             resolver's hop limit — refused, never passed",
            "the impl of `Twin` for `Shell` in crate::runtime names Native, Shell for the native \
             backend in its trait — the window is reached through the seam",
            "the impl of `elsewhere::Imported` for `Plain` in crate::runtime: a trait the guard \
             cannot resolve and cannot show to be another crate's — the guard does not follow it",
            "the impl of `me::window_seam::WindowHost` for `Aliased` in crate::runtime is \
             spelled as a door of the seam — a door of the seam is implemented in the seam",
            "the module `hidden` at runtime.rs:368 inside `fn hides_a_module` — a module below \
             module level is a shape the guard does not read",
            "the supertrait `crate::elsewhere::Bk20` in crate chains past the resolver's hop \
             limit — refused, never passed",
            "the trait `Hop20` in crate::elsewhere chains past the resolver's hop limit — \
             refused, never passed",
            "the trait `crate::elsewhere::Bk20` in crate::window_seam chains past the resolver's \
             hop limit — refused, never passed",
            "the trait `crate::runtime::Bounded` names Native, Runtime, Shell for the native \
             backend in its declaration — the window is reached through the seam",
            "the trait `crate::runtime::Keeps` names Native, Runtime, Shell for the native \
             backend in its declaration — the window is reached through the seam",
            "the trait of a native candidate `crate::elsewhere::Bk20` in crate::window_seam \
             chains past the resolver's hop limit — refused, never passed",
            "the type `Aliased` at lib.rs:101 inside `fn listed_rebuild` — a body the guard does \
             not read for mentions (a native block's method, a listed function or `const`) \
             introduces no names",
            "the type `Aliased` at lib.rs:73 inside `impl Runtime` — a body the guard does not \
             read for mentions (a native block's method, a listed function or `const`) \
             introduces no names",
            "the type `Inner` at lib.rs:141 inside `impl` — a body the guard does not read for \
             mentions (a native block's method, a listed function or `const`) introduces no \
             names",
            "the type `Inner` at lib.rs:155 inside `impl` — a body the guard does not read for \
             mentions (a native block's method, a listed function or `const`) introduces no \
             names",
            "the type `crate::handles::Keeps` names bt_platform::NativeWindow \
             — the window's platform handle and its composition tree are the native \
             backend's own: code written for the window speaks to the platform through the \
             seam's doors",
            "the type `crate::runtime::Counted` holds Runtime for the native backend — the \
             window is reached through the seam",
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
            "the use `Chained` at runtime.rs:313 inside `fn hides_behind_a_chain` reaches the \
             seam (window-seam crate::window_seam::WindowHost) — a body does not alias the seam",
            "the use `Door` at runtime.rs:250 inside `fn hides_a_renamed_door` reaches the seam \
             (crate::window_seam::WindowHost) — a body does not alias the seam",
            "the use `Either` at runtime.rs:323 inside `fn hides_in_a_sibling_block` reaches the \
             seam (window-seam crate::window_seam::WindowHost) — a body does not alias the seam",
            "the use `H` at runtime.rs:409 inside `fn hides_a_door_in_a_macro` reaches the seam \
             (window-seam crate::window_seam::WindowHost) — a body does not alias the seam",
            "the use `Host` at runtime.rs:274 inside `fn hides_a_reexported_door` reaches the \
             seam (window-seam crate::window_seam::WindowHost) — a body does not alias the seam",
            "the use `Inside` at lib.rs:131 inside `trait Listed` — a body the guard does not \
             read for mentions (a native block's method, a listed function or `const`) \
             introduces no names",
            "the use `Renamed` at lib.rs:105 inside `fn listed_rebuild` — a body the guard does \
             not read for mentions (a native block's method, a listed function or `const`) \
             introduces no names",
            "the use `Renamed` at lib.rs:79 inside `impl Runtime` — a body the guard does not \
             read for mentions (a native block's method, a listed function or `const`) \
             introduces no names",
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
    let (refusals, unused) = judge(
        Index::of_package("bt-app"),
        "bt-app",
        &bt_app_native_list(),
        &dependencies_of("bt-app"),
    );
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
