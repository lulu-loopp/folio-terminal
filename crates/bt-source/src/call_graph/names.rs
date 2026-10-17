//! **What a path names**, in the type and the value namespaces, across the packages of one
//! program — [`crate::TraitResolver`]'s rules widened from traits to every name a call can go
//! through: modules, traits, `struct`/`enum`/`union` types, type aliases ([`crate::AliasRecord`]),
//! free functions and enum variants.
//!
//! A path is read the way the compiler reads one, as far as the index can say: `crate`, `self` and
//! `super`; a name the module declares; a name a `use` item of the module brings in, followed to
//! what it names (renames included, `pub use` re-exports across packages included); a name a glob
//! of the module opens, taken only after the module's own names; another first-party package's
//! crate name as a first segment. A path that leaves the packages handed in is **foreign**, and it
//! is answered with its full path after the first-party `use` items on the way are followed
//! (`use std::sync::Mutex` makes `Mutex` the foreign `std::sync::Mutex`): a first segment of
//! `std`, `core` or `alloc`, a first segment the module tree does not hold (the extern prelude: a
//! dependency's crate name), a name of the standard prelude, a primitive type.
//!
//! **Glob refusals.** A name no first-party declaration, `use` or glob supplies, in a module that
//! opens foreign globs: with one foreign glob it is that glob's (`use rayon::prelude::*` makes
//! `par_iter` `rayon::prelude::par_iter`); with two or more, or with one when the standard prelude
//! has the name too, the module tree cannot say which, and the answer is a refusal with its
//! reason — never a pick.
//!
//! Visibility is not read (a private `use` is read as though it were visible, which can only find
//! more), nor are items a macro generates. Like [`crate::TraitResolver`] it follows at most
//! [`HOPS`] `use` and glob steps, and a resolution that reaches the limit says so.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use crate::index::{Index, ItemKind};

/// How many `use` and glob hops a resolution follows — [`crate::TraitResolver`]'s limit.
pub(crate) const HOPS: usize = 16;

/// What one name, or one path, names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Named {
    Module(usize, String),
    Trait(usize, String, String),
    Type(usize, String, String),
    Alias(usize, String, String),
    Function(usize, String, String),
    /// A variant of a first-party enum: package, module, enum, variant.
    Variant(usize, String, String, String),
    /// A path that leaves the packages handed in, in full.
    Foreign(Vec<String>),
}

/// A resolution's answer, with what stopped it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Resolution {
    pub(crate) named: Vec<Named>,
    /// The hop limit was reached somewhere on the way.
    pub(crate) cut: bool,
    /// The module tree cannot say which of several foreign sources supplies a name.
    pub(crate) refused: Option<String>,
}

/// What a module declares under one name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Declared {
    Trait,
    Type,
    Alias,
    Function,
}

#[derive(Default)]
struct Imports {
    names: Vec<(String, Vec<String>)>,
    globs: Vec<Vec<String>>,
}

/// The standard prelude's names (the 2021 edition's), each with the path it is.
const PRELUDE: [(&str, &[&str]); 42] = [
    ("Box", &["std", "boxed", "Box"]),
    ("String", &["std", "string", "String"]),
    ("ToString", &["std", "string", "ToString"]),
    ("Vec", &["std", "vec", "Vec"]),
    ("Option", &["std", "option", "Option"]),
    ("Some", &["std", "option", "Option", "Some"]),
    ("None", &["std", "option", "Option", "None"]),
    ("Result", &["std", "result", "Result"]),
    ("Ok", &["std", "result", "Result", "Ok"]),
    ("Err", &["std", "result", "Result", "Err"]),
    ("drop", &["std", "mem", "drop"]),
    ("Clone", &["std", "clone", "Clone"]),
    ("Copy", &["std", "marker", "Copy"]),
    ("Send", &["std", "marker", "Send"]),
    ("Sync", &["std", "marker", "Sync"]),
    ("Sized", &["std", "marker", "Sized"]),
    ("Unpin", &["std", "marker", "Unpin"]),
    ("Drop", &["std", "ops", "Drop"]),
    ("Fn", &["std", "ops", "Fn"]),
    ("FnMut", &["std", "ops", "FnMut"]),
    ("FnOnce", &["std", "ops", "FnOnce"]),
    ("Iterator", &["std", "iter", "Iterator"]),
    ("IntoIterator", &["std", "iter", "IntoIterator"]),
    (
        "DoubleEndedIterator",
        &["std", "iter", "DoubleEndedIterator"],
    ),
    ("ExactSizeIterator", &["std", "iter", "ExactSizeIterator"]),
    ("Extend", &["std", "iter", "Extend"]),
    ("FromIterator", &["std", "iter", "FromIterator"]),
    ("Default", &["std", "default", "Default"]),
    ("ToOwned", &["std", "borrow", "ToOwned"]),
    ("From", &["std", "convert", "From"]),
    ("Into", &["std", "convert", "Into"]),
    ("TryFrom", &["std", "convert", "TryFrom"]),
    ("TryInto", &["std", "convert", "TryInto"]),
    ("AsRef", &["std", "convert", "AsRef"]),
    ("AsMut", &["std", "convert", "AsMut"]),
    ("PartialEq", &["std", "cmp", "PartialEq"]),
    ("Eq", &["std", "cmp", "Eq"]),
    ("PartialOrd", &["std", "cmp", "PartialOrd"]),
    ("Ord", &["std", "cmp", "Ord"]),
    ("std", &["std"]),
    ("core", &["core"]),
    ("alloc", &["alloc"]),
];

/// The primitive types, which a path may begin with (`u8::from`, `str::from_utf8`).
const PRIMITIVES: [&str; 18] = [
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64", "Self",
];

/// The path the standard prelude gives `name`, if it gives one (`std`, `core` and `alloc`
/// themselves among them, as a path's first segment).
pub(crate) fn prelude(name: &str) -> Option<Vec<String>> {
    PRELUDE
        .iter()
        .find(|(prelude, _)| *prelude == name)
        .map(|(_, path)| path.iter().map(|segment| (*segment).to_owned()).collect())
}

/// The path of a standard type a fixed list names without one (`std_named("Option")`).
pub(crate) fn canonical_std(name: &str) -> Vec<String> {
    let path: &[&str] = match name {
        "Vec" => &["std", "vec", "Vec"],
        "VecDeque" => &["std", "collections", "VecDeque"],
        "HashMap" => &["std", "collections", "HashMap"],
        "BTreeMap" => &["std", "collections", "BTreeMap"],
        "HashSet" => &["std", "collections", "HashSet"],
        "BTreeSet" => &["std", "collections", "BTreeSet"],
        "Option" => &["std", "option", "Option"],
        "Result" => &["std", "result", "Result"],
        "String" => &["std", "string", "String"],
        "PathBuf" => &["std", "path", "PathBuf"],
        "Path" => &["std", "path", "Path"],
        "Box" => &["std", "boxed", "Box"],
        "Rc" => &["std", "rc", "Rc"],
        "Arc" => &["std", "sync", "Arc"],
        "Cell" => &["std", "cell", "Cell"],
        "RefCell" => &["std", "cell", "RefCell"],
        "Mutex" => &["std", "sync", "Mutex"],
        "RwLock" => &["std", "sync", "RwLock"],
        _ => return vec![name.to_owned()],
    };
    path.iter().map(|segment| (*segment).to_owned()).collect()
}

/// A resolution asked: package, module, path.
type ResolutionKey = (usize, String, Vec<String>);

/// **Resolves paths over the indexes of one program's packages.**
pub(crate) struct Names<'i> {
    pub(crate) packages: Vec<(&'i str, String, &'i Index)>,
    modules: BTreeSet<(usize, String)>,
    declared: BTreeMap<(usize, String, String), BTreeSet<Declared>>,
    variants: BTreeMap<(usize, String, String), BTreeSet<String>>,
    imports: BTreeMap<(usize, String), Imports>,
    cache: RefCell<BTreeMap<ResolutionKey, Resolution>>,
}

/// One resolution's state.
#[derive(Default)]
struct Walk {
    cut: Cell<bool>,
    refused: RefCell<Option<String>>,
    open: RefCell<BTreeSet<(usize, String, String, bool)>>,
}

impl<'i> Names<'i> {
    pub(crate) fn new(packages: &[(&'i str, &'i Index)]) -> Self {
        let mut modules = BTreeSet::new();
        let mut declared: BTreeMap<(usize, String, String), BTreeSet<Declared>> = BTreeMap::new();
        let mut variants: BTreeMap<(usize, String, String), BTreeSet<String>> = BTreeMap::new();
        let mut imports: BTreeMap<(usize, String), Imports> = BTreeMap::new();
        for (at, (_, index)) in packages.iter().enumerate() {
            for module in index.modules() {
                for path in module.module_paths() {
                    modules.insert((at, path.clone()));
                }
            }
            let mut declare = |module: &str, name: &str, what: Declared| {
                declared
                    .entry((at, module.to_owned(), name.to_owned()))
                    .or_default()
                    .insert(what);
            };
            for declaration in index.traits() {
                for module in declaration.module_paths() {
                    declare(module, declaration.name(), Declared::Trait);
                }
            }
            for alias in index.aliases() {
                for module in alias.module_paths() {
                    declare(module, alias.name(), Declared::Alias);
                }
            }
            for item in index.items() {
                let what = match item.kind() {
                    ItemKind::Struct | ItemKind::Enum | ItemKind::Union => Declared::Type,
                    ItemKind::Function => Declared::Function,
                    ItemKind::Variant => {
                        if let Some(owner) = item.type_owner() {
                            for module in item.module_paths() {
                                variants
                                    .entry((at, module.to_owned(), owner.to_owned()))
                                    .or_default()
                                    .insert(item.name().to_owned());
                            }
                        }
                        continue;
                    }
                    _ => continue,
                };
                for module in item.module_paths() {
                    declare(module, item.name(), what);
                }
            }
            for used in index.uses() {
                for module in used.module_paths() {
                    let entry = imports.entry((at, module.to_owned())).or_default();
                    entry.names.extend(used.names().iter().cloned());
                    entry.globs.extend(used.globs().iter().cloned());
                }
            }
        }
        Self {
            packages: packages
                .iter()
                .map(|(name, index)| (*name, name.replace('-', "_"), *index))
                .collect(),
            modules,
            declared,
            variants,
            imports,
            cache: RefCell::default(),
        }
    }

    /// The package `name`'s position, if it is one of these.
    pub(crate) fn package_at(&self, name: &str) -> Option<usize> {
        self.packages
            .iter()
            .position(|(package, ..)| *package == name)
    }

    /// The variants a first-party enum declares.
    pub(crate) fn variants_of(
        &self,
        at: usize,
        module: &str,
        name: &str,
    ) -> Option<&BTreeSet<String>> {
        self.variants.get(&(at, module.to_owned(), name.to_owned()))
    }

    /// **What `path`, written in `module` of package `at`, names.**
    pub(crate) fn resolve(&self, at: usize, module: &str, path: &[String]) -> Resolution {
        let key = (at, module.to_owned(), path.to_vec());
        if let Some(found) = self.cache.borrow().get(&key) {
            return found.clone();
        }
        let walk = Walk::default();
        let mut named = self.path(at, module, path, 0, false, &walk);
        named.sort();
        named.dedup();
        let resolution = Resolution {
            named,
            cut: walk.cut.get(),
            refused: walk.refused.borrow().clone(),
        };
        self.cache.borrow_mut().insert(key, resolution.clone());
        resolution
    }

    /// `imported`: the path a `use` item writes, whose first segment — when nothing in the tree
    /// holds it — is a dependency's crate name even with nothing after it (`use serde_json;`,
    /// `use alpha::*`).
    fn path(
        &self,
        at: usize,
        module: &str,
        path: &[String],
        hops: usize,
        imported: bool,
        walk: &Walk,
    ) -> Vec<Named> {
        let Some((first, rest)) = path.split_first() else {
            return Vec::new();
        };
        if hops > HOPS {
            walk.cut.set(true);
            return Vec::new();
        }
        let starts = match first.as_str() {
            "crate" => vec![Named::Module(at, "crate".to_owned())],
            "self" => vec![Named::Module(at, module.to_owned())],
            "super" => parent(module)
                .map(|parent| vec![Named::Module(at, parent.to_owned())])
                .unwrap_or_default(),
            name => self.in_scope(
                at,
                module,
                name,
                hops,
                Some(imported || !rest.is_empty()),
                walk,
            ),
        };
        let mut current = starts;
        for segment in rest {
            current = current
                .into_iter()
                .flat_map(|named| self.step(named, segment, hops, walk))
                .collect();
        }
        current
    }

    /// One segment further along a path.
    fn step(&self, named: Named, segment: &str, hops: usize, walk: &Walk) -> Vec<Named> {
        match named {
            Named::Module(at, module) => match segment {
                "self" => vec![Named::Module(at, module)],
                "super" => parent(&module)
                    .map(|parent| vec![Named::Module(at, parent.to_owned())])
                    .unwrap_or_default(),
                name => self.in_scope(at, &module, name, hops, None, walk),
            },
            Named::Foreign(mut path) => {
                path.push(segment.to_owned());
                vec![Named::Foreign(path)]
            }
            Named::Type(at, module, name) => {
                if self
                    .variants_of(at, &module, &name)
                    .is_some_and(|variants| variants.contains(segment))
                {
                    vec![Named::Variant(at, module, name, segment.to_owned())]
                } else {
                    Vec::new()
                }
            }
            Named::Trait(..) | Named::Alias(..) | Named::Function(..) | Named::Variant(..) => {
                Vec::new()
            }
        }
    }

    /// What `name` is in `module` of package `at`. `first` is `Some(more)` when the name is a
    /// path's first segment (`more`: segments follow it), which is where another crate's name,
    /// the standard prelude and the primitive types are in scope.
    fn in_scope(
        &self,
        at: usize,
        module: &str,
        name: &str,
        hops: usize,
        first: Option<bool>,
        walk: &Walk,
    ) -> Vec<Named> {
        let key = (at, module.to_owned(), name.to_owned(), first.is_some());
        if !walk.open.borrow_mut().insert(key.clone()) {
            return Vec::new();
        }
        let found = self.in_scope_once(at, module, name, hops, first, walk);
        walk.open.borrow_mut().remove(&key);
        found
    }

    fn in_scope_once(
        &self,
        at: usize,
        module: &str,
        name: &str,
        hops: usize,
        first: Option<bool>,
        walk: &Walk,
    ) -> Vec<Named> {
        let mut found = Vec::new();
        if let Some(declared) = self.declared.get(&(at, module.to_owned(), name.to_owned())) {
            for what in declared {
                let (module, name) = (module.to_owned(), name.to_owned());
                found.push(match what {
                    Declared::Trait => Named::Trait(at, module, name),
                    Declared::Type => Named::Type(at, module, name),
                    Declared::Alias => Named::Alias(at, module, name),
                    Declared::Function => Named::Function(at, module, name),
                });
            }
        }
        let child = format!("{module}::{name}");
        if self.modules.contains(&(at, child.clone())) {
            found.push(Named::Module(at, child));
        }
        let imports = self.imports.get(&(at, module.to_owned()));
        for (alias, path) in imports
            .map(|imports| imports.names.as_slice())
            .unwrap_or_default()
        {
            if alias == name {
                found.extend(self.path(at, module, path, hops + 1, true, walk));
            }
        }
        if !found.is_empty() {
            return found;
        }
        let mut foreign_globs: Vec<Vec<String>> = Vec::new();
        for glob in imports
            .map(|imports| imports.globs.as_slice())
            .unwrap_or_default()
        {
            for opened in self.path(at, module, glob, hops + 1, true, walk) {
                match opened {
                    Named::Module(at, module) => {
                        found.extend(self.in_scope(at, &module, name, hops + 1, None, walk));
                    }
                    Named::Type(at, module, owner) => {
                        if self
                            .variants_of(at, &module, &owner)
                            .is_some_and(|variants| variants.contains(name))
                        {
                            found.push(Named::Variant(at, module, owner, name.to_owned()));
                        }
                    }
                    Named::Foreign(path) => foreign_globs.push(path),
                    _ => {}
                }
            }
        }
        if !found.is_empty() {
            return found;
        }
        let prelude_name = first.and_then(|_| prelude(name));
        match foreign_globs.as_slice() {
            [] => {}
            [glob] if prelude_name.is_none() => {
                let mut path = glob.clone();
                path.push(name.to_owned());
                return vec![Named::Foreign(path)];
            }
            globs => {
                let spelled: Vec<String> = globs.iter().map(|glob| glob.join("::")).collect();
                *walk.refused.borrow_mut() = Some(format!(
                    "`{name}` may come from {} and the module tree cannot say which",
                    if prelude_name.is_some() {
                        format!(
                            "the glob `{}::*` or the standard prelude",
                            spelled.join("::*`, `")
                        )
                    } else {
                        format!("the globs `{}::*`", spelled.join("::*`, `"))
                    }
                ));
                return Vec::new();
            }
        }
        let Some(more) = first else {
            return Vec::new();
        };
        if let Some(package) = self
            .packages
            .iter()
            .position(|(_, crate_name, _)| crate_name == name)
        {
            return vec![Named::Module(package, "crate".to_owned())];
        }
        if let Some(path) = prelude_name {
            return vec![Named::Foreign(path)];
        }
        if PRIMITIVES.contains(&name) || more {
            // A primitive type, or — as a path's first segment that nothing in the tree holds —
            // a dependency's crate name (the extern prelude).
            return vec![Named::Foreign(vec![name.to_owned()])];
        }
        Vec::new()
    }
}

/// The module `module` is declared in; `None` for a crate root.
pub(crate) fn parent(module: &str) -> Option<&str> {
    module.rsplit_once("::").map(|(parent, _)| parent)
}
