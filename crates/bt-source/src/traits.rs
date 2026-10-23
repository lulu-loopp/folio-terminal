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
//!
//! **What a guard decides with**: [`TraitResolver::resolve_checked`],
//! [`TraitResolver::supertraits_checked`] and [`TraitResolver::implemented_checked`], which
//! answer [`ResolveCutoff`] when a path chains past the resolver's hop limit, so a guard refuses
//! what it could not read instead of reading it as "no trait". [`TraitResolver::resolve`],
//! [`TraitResolver::supertraits`] and [`TraitResolver::implemented`] swallow the cutoff and are
//! for callers that decide nothing from the answer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::index::{ImplRecord, Index, TraitRecord};

/// How many `use` and glob hops a resolution follows before it gives up. A cycle (`a` opens `b`,
/// `b` opens `a`; `use anyhow::anyhow;`) does not end here: a name already being looked up in the
/// same module along one walk names nothing more there. A chain longer than this is a cutoff,
/// which [`TraitResolver::resolve_checked`] reports and [`TraitResolver::resolve`] does not.
pub(crate) const HOPS: usize = 16;

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
    /// third-party trait, a name the index cannot follow). **It swallows the
    /// cutoff**: a path chained past [`HOPS`] answers what was found before the
    /// limit, often nothing, with no word of it — a caller that must not pass
    /// what it could not read asks [`Self::resolve_checked`].
    #[must_use]
    pub fn resolve(&self, package: &str, module: &str, path: &[String]) -> BTreeSet<TraitId> {
        self.resolve_checked(package, module, path)
            .unwrap_or_else(|ResolveCutoff(found)| found)
    }

    /// [`Self::resolve`], **saying when it gave up**: a path whose `use` items, renames and globs
    /// chain further than the resolver follows (its hop limit) is `Err`, carrying what it found
    /// before it stopped — a caller that must not pass what it could not read refuses it instead
    /// of reading the shortfall as "names no trait".
    ///
    /// # Errors
    ///
    /// [`ResolveCutoff`] when the hop limit was reached anywhere on the way.
    pub fn resolve_checked(
        &self,
        package: &str,
        module: &str,
        path: &[String],
    ) -> Result<BTreeSet<TraitId>, ResolveCutoff> {
        let Some(at) = self.package_at(package) else {
            return Ok(BTreeSet::new());
        };
        let walk = Walk::default();
        let found: BTreeSet<TraitId> = self
            .path(at, module, path, 0, &walk)
            .into_iter()
            .filter_map(|named| match named {
                Named::Trait(at, module_path, name) => Some(TraitId {
                    package: self.packages[at].0.to_owned(),
                    module_path,
                    name,
                }),
                Named::Module(..) => None,
            })
            .collect();
        if walk.cut.get() {
            Err(ResolveCutoff(found))
        } else {
            Ok(found)
        }
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
    /// resolved in the declaration's own module. **It swallows the cutoff**, as
    /// [`Self::resolve`] does; a caller that decides from the answer asks
    /// [`Self::supertraits_checked`].
    #[must_use]
    pub fn supertraits(&self, id: &TraitId) -> BTreeSet<TraitId> {
        self.supertraits_checked(id)
            .unwrap_or_else(|ResolveCutoff(found)| found)
    }

    /// [`Self::supertraits`], **saying when it gave up** on any of them.
    ///
    /// # Errors
    ///
    /// [`ResolveCutoff`] when a supertrait path chains past the hop limit, carrying every
    /// supertrait found.
    pub fn supertraits_checked(&self, id: &TraitId) -> Result<BTreeSet<TraitId>, ResolveCutoff> {
        let paths = self
            .declarations(id)
            .into_iter()
            .flat_map(|declaration| declaration.supertraits.iter());
        self.resolve_all(paths.map(|path| {
            (
                id.package.as_str(),
                id.module_path.as_str(),
                path.as_slice(),
            )
        }))
    }

    /// The traits an `impl` block of `package` implements: its trait path
    /// resolved in each module the block is written in. Empty for an inherent
    /// block and for a trait that is not first-party. **It swallows the
    /// cutoff**, as [`Self::resolve`] does; a caller that decides from the
    /// answer asks [`Self::implemented_checked`].
    #[must_use]
    pub fn implemented(&self, package: &str, block: &ImplRecord) -> BTreeSet<TraitId> {
        self.implemented_checked(package, block)
            .unwrap_or_else(|ResolveCutoff(found)| found)
    }

    /// [`Self::implemented`], **saying when it gave up** in any module the block is written in.
    ///
    /// # Errors
    ///
    /// [`ResolveCutoff`] when the block's trait path chains past the hop limit in one of its
    /// modules, carrying every trait found.
    pub fn implemented_checked(
        &self,
        package: &str,
        block: &ImplRecord,
    ) -> Result<BTreeSet<TraitId>, ResolveCutoff> {
        let Some(path) = block.trait_path() else {
            return Ok(BTreeSet::new());
        };
        self.resolve_all(
            block
                .module_paths()
                .into_iter()
                .map(|module| (package, module, path)),
        )
    }

    /// Every `(package, module, path)` resolved and united — `Err` with the union when any of
    /// them was cut off.
    fn resolve_all<'p>(
        &self,
        paths: impl Iterator<Item = (&'p str, &'p str, &'p [String])>,
    ) -> Result<BTreeSet<TraitId>, ResolveCutoff> {
        let mut found = BTreeSet::new();
        let mut cut = false;
        for (package, module, path) in paths {
            match self.resolve_checked(package, module, path) {
                Ok(named) => found.extend(named),
                Err(ResolveCutoff(named)) => {
                    cut = true;
                    found.extend(named);
                }
            }
        }
        if cut {
            Err(ResolveCutoff(found))
        } else {
            Ok(found)
        }
    }

    fn package_at(&self, package: &str) -> Option<usize> {
        self.packages.iter().position(|(name, ..)| *name == package)
    }

    /// What `path`, written in `module` of package `at`, names.
    fn path(
        &self,
        at: usize,
        module: &str,
        path: &[String],
        hops: usize,
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
            name => self.in_scope(at, module, name, hops, true, walk),
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
                        name => self.in_scope(at, &module, name, hops, false, walk),
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
        walk: &Walk,
    ) -> Vec<Named> {
        // A name already being looked up in this module, further up this walk, is a cycle
        // (`use anyhow::anyhow;`, two modules' globs of each other): it names nothing more there.
        let key = (at, module.to_owned(), name.to_owned(), first);
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
        first: bool,
        walk: &Walk,
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
                found.extend(self.path(at, module, path, hops + 1, walk));
            }
        }
        if !found.is_empty() {
            return found;
        }
        for glob in imports
            .map(|imports| imports.globs.as_slice())
            .unwrap_or_default()
        {
            for opened in self.path(at, module, glob, hops + 1, walk) {
                if let Named::Module(at, module) = opened {
                    found.extend(self.in_scope(at, &module, name, hops + 1, false, walk));
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

/// One resolution's state: whether the hop limit was reached, and the names being looked up
/// along the way (a cycle among them names nothing more).
#[derive(Default)]
struct Walk {
    cut: std::cell::Cell<bool>,
    open: std::cell::RefCell<BTreeSet<(usize, String, String, bool)>>,
}

/// **The resolver gave up**: a path chained past its hop limit. Carries what was found before it
/// stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveCutoff(pub BTreeSet<TraitId>);

/// The module `module` is declared in; `None` for a crate root.
fn parent(module: &str) -> Option<&str> {
    module.rsplit_once("::").map(|(parent, _)| parent)
}
