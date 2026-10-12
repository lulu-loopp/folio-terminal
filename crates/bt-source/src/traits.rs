//! **Which trait a path names**, across the packages of one program.
//!
//! A trait is named by its identity ([`TraitId`]): the package, the module path
//! it is declared in, and its name — so the macOS and the Windows `PreparePoint`
//! are two traits, and `impl Run for Mac` written under `use crate::job::Driver
//! as Run` implements `crate::job::Driver`. A path is resolved the way the
//! compiler resolves one in the type namespace, as far as the index can say:
//! `crate`, `self` and `super`; a name the module declares (a trait, or a child
//! module); a name a `use` item of the module brings in, followed to what it
//! names (a `pub use` re-export is one, in another package as well); a name a
//! glob of the module opens; and, as a path's first segment, another package's
//! crate name. A name declared or imported by the module itself is taken before
//! a glob's, as the compiler takes it.
//!
//! **What is not read**: visibility (a private `use` is read as though it were
//! visible, which can only find more), a `use` item inside a function body,
//! macro-generated items, and anything outside the packages handed in — a
//! standard or third-party trait resolves to nothing, which is how a caller
//! tells it is not first-party.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::index::{ImplRecord, Index, TraitRecord};

/// How many `use` and glob hops a resolution follows before it gives up — a
/// cycle of globs (`a` opens `b`, `b` opens `a`) is legal Rust and ends here.
const HOPS: usize = 16;

/// **A trait's identity**: the package it is declared in, the module path of
/// the declaration (inline ancestry included) and its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraitId {
    pub package: String,
    pub module_path: String,
    pub name: String,
}

impl fmt::Display for TraitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}::{}",
            self.package, self.module_path, self.name
        )
    }
}

/// What a name resolves to on the way along a path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Named {
    Module(usize, String),
    Trait(usize, String, String),
}

/// The `use` items of one module, flattened.
#[derive(Default)]
struct Imports<'i> {
    names: Vec<(&'i str, &'i [String])>,
    globs: Vec<&'i [String]>,
}

/// **Resolves trait paths over the indexes of one program's packages.**
pub struct TraitResolver<'i> {
    packages: Vec<(&'i str, String, &'i Index)>,
    modules: BTreeSet<(usize, &'i str)>,
    declared: BTreeMap<(usize, &'i str, &'i str), Vec<&'i TraitRecord>>,
    imports: BTreeMap<(usize, &'i str), Imports<'i>>,
}

impl<'i> TraitResolver<'i> {
    /// The resolver over `packages`, each `(package name, its index)`. A
    /// package's crate name is its name with `-` written `_`.
    #[must_use]
    pub fn new(packages: &[(&'i str, &'i Index)]) -> Self {
        let mut modules = BTreeSet::new();
        let mut declared: BTreeMap<(usize, &str, &str), Vec<&TraitRecord>> = BTreeMap::new();
        let mut imports: BTreeMap<(usize, &str), Imports> = BTreeMap::new();
        for (at, (_, index)) in packages.iter().enumerate() {
            for module in index.modules() {
                for path in module.module_paths() {
                    modules.insert((at, path.as_str()));
                }
            }
            for declaration in index.traits() {
                for path in &declaration.declared {
                    declared
                        .entry((at, path.module_path.as_str(), declaration.name.as_str()))
                        .or_default()
                        .push(declaration);
                }
            }
            for used in index.uses() {
                for path in &used.declared {
                    let entry = imports.entry((at, path.module_path.as_str())).or_default();
                    entry.names.extend(
                        used.names
                            .iter()
                            .map(|(name, path)| (name.as_str(), path.as_slice())),
                    );
                    entry.globs.extend(used.globs.iter().map(Vec::as_slice));
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
            imports,
        }
    }

    /// **The traits `path`, written in `module` of `package`, names** — empty
    /// for a path that names no trait of these packages (a type, a standard or
    /// third-party trait, a name the index cannot follow).
    #[must_use]
    pub fn resolve(&self, package: &str, module: &str, path: &[String]) -> BTreeSet<TraitId> {
        let Some(at) = self.package_at(package) else {
            return BTreeSet::new();
        };
        self.path(at, module, path, 0)
            .into_iter()
            .filter_map(|named| match named {
                Named::Trait(at, module_path, name) => Some(TraitId {
                    package: self.packages[at].0.to_owned(),
                    module_path,
                    name,
                }),
                Named::Module(..) => None,
            })
            .collect()
    }

    /// The identities a trait declaration answers to, one per module path that
    /// reaches it.
    #[must_use]
    pub fn identities(&self, package: &str, declaration: &TraitRecord) -> BTreeSet<TraitId> {
        declaration
            .declared
            .iter()
            .map(|path| TraitId {
                package: package.to_owned(),
                module_path: path.module_path.clone(),
                name: declaration.name.clone(),
            })
            .collect()
    }

    /// The declarations of `id`: one, or one per arm it is written on.
    #[must_use]
    pub fn declarations(&self, id: &TraitId) -> Vec<&'i TraitRecord> {
        self.package_at(&id.package)
            .and_then(|at| {
                self.declared
                    .get(&(at, id.module_path.as_str(), id.name.as_str()))
                    .cloned()
            })
            .unwrap_or_default()
    }

    /// The first-party traits `id`'s declarations name as supertraits, each
    /// resolved in the declaration's own module.
    #[must_use]
    pub fn supertraits(&self, id: &TraitId) -> BTreeSet<TraitId> {
        let mut found = BTreeSet::new();
        for declaration in self.declarations(id) {
            for path in &declaration.supertraits {
                found.extend(self.resolve(&id.package, &id.module_path, path));
            }
        }
        found
    }

    /// The traits an `impl` block of `package` implements: its trait path
    /// resolved in each module the block is written in. Empty for an inherent
    /// block and for a trait that is not first-party.
    #[must_use]
    pub fn implemented(&self, package: &str, block: &ImplRecord) -> BTreeSet<TraitId> {
        let Some(path) = block.trait_path() else {
            return BTreeSet::new();
        };
        block
            .module_paths()
            .into_iter()
            .flat_map(|module| self.resolve(package, module, path))
            .collect()
    }

    fn package_at(&self, package: &str) -> Option<usize> {
        self.packages.iter().position(|(name, ..)| *name == package)
    }

    /// What `path`, written in `module` of package `at`, names.
    fn path(&self, at: usize, module: &str, path: &[String], hops: usize) -> Vec<Named> {
        let Some((first, rest)) = path.split_first() else {
            return Vec::new();
        };
        if hops > HOPS {
            return Vec::new();
        }
        let starts = match first.as_str() {
            "crate" => vec![Named::Module(at, "crate".to_owned())],
            "self" => vec![Named::Module(at, module.to_owned())],
            "super" => parent(module)
                .map(|parent| vec![Named::Module(at, parent.to_owned())])
                .unwrap_or_default(),
            name => self.in_scope(at, module, name, hops, true),
        };
        let mut current = starts;
        for segment in rest {
            current = current
                .into_iter()
                .flat_map(|named| match named {
                    Named::Module(at, module) => match segment.as_str() {
                        "self" => vec![Named::Module(at, module)],
                        "super" => parent(&module)
                            .map(|parent| vec![Named::Module(at, parent.to_owned())])
                            .unwrap_or_default(),
                        name => self.in_scope(at, &module, name, hops, false),
                    },
                    Named::Trait(..) => Vec::new(),
                })
                .collect();
        }
        current.sort();
        current.dedup();
        current
    }

    /// What `name` is in `module` of package `at`: a trait the module
    /// declares, a child module, or what a `use` item of the module names —
    /// before any of those, a glob's; and, as a path's first segment
    /// (`first`), another package's crate name.
    fn in_scope(
        &self,
        at: usize,
        module: &str,
        name: &str,
        hops: usize,
        first: bool,
    ) -> Vec<Named> {
        let mut found = Vec::new();
        if self.declared.contains_key(&(at, module, name)) {
            found.push(Named::Trait(at, module.to_owned(), name.to_owned()));
        }
        let child = format!("{module}::{name}");
        if self.modules.contains(&(at, child.as_str())) {
            found.push(Named::Module(at, child));
        }
        let imports = self.imports.get(&(at, module));
        for (alias, path) in imports
            .map(|imports| imports.names.as_slice())
            .unwrap_or_default()
        {
            if *alias == name {
                found.extend(self.path(at, module, path, hops + 1));
            }
        }
        if !found.is_empty() {
            return found;
        }
        for glob in imports
            .map(|imports| imports.globs.as_slice())
            .unwrap_or_default()
        {
            for opened in self.path(at, module, glob, hops + 1) {
                if let Named::Module(at, module) = opened {
                    found.extend(self.in_scope(at, &module, name, hops + 1, false));
                }
            }
        }
        if !found.is_empty() || !first {
            return found;
        }
        self.packages
            .iter()
            .position(|(_, crate_name, _)| crate_name == name)
            .map(|package| vec![Named::Module(package, "crate".to_owned())])
            .unwrap_or_default()
    }
}

/// The module `module` is declared in; `None` for a crate root.
fn parent(module: &str) -> Option<&str> {
    module.rsplit_once("::").map(|(parent, _)| parent)
}
