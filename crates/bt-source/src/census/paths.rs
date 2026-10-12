//! **A free function's path, followed to the item it names across the workspace's packages** —
//! the census's own path rules ([`super::resolve`]: `crate`, `self`, `super`, a module's `use`
//! items and globs, a child module), with one step more: a first segment that is a workspace
//! crate's name continues in that crate's root, so a `use other_crate::m::f as g;` and a
//! `pub(crate) use other_crate::m::f;` re-export both lead to `other_crate`'s `m::f`.
//!
//! The answer is an item's identity — package, module path, name — never a spelling: two
//! functions called `contain` in two modules called `contained_panic` are two answers. A path the
//! rules do not follow (a `use` written inside a function body, a macro, a path into a crate that
//! is not among the indexes read) names nothing.

use std::collections::BTreeSet;

use super::types::{Declarations, module_at, package_at};
use crate::index::Index;

/// How many `use` steps a path is followed through; a cycle of globs is not followed.
const PATH_STEPS: usize = 8;

/// A free function of the workspace, by its identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionItem {
    /// Its package (`bt-platform`).
    pub package: String,
    /// The module it is declared in (`crate::contained_panic`).
    pub module: String,
    /// Its name.
    pub name: String,
}

/// The free functions and `use` items of a set of indexes, read once, to follow paths over.
#[derive(Debug)]
pub struct FunctionPaths {
    declarations: Declarations,
}

impl FunctionPaths {
    /// Read the declarations of every index handed in; a path is followed into any of their
    /// library crates.
    #[must_use]
    pub fn read(indexes: &[&Index]) -> Self {
        Self {
            declarations: Declarations::read(indexes),
        }
    }

    /// **The free functions `path`, written at byte `offset` of `index`, names** — the
    /// declarations it reaches through the module's `use` items and the crates' re-exports. Empty
    /// for a path the rules do not follow; more than one answer only for one name declared more
    /// than once in one module (its `cfg` variants).
    #[must_use]
    pub fn functions_named(
        &self,
        index: &Index,
        offset: usize,
        path: &[String],
    ) -> BTreeSet<FunctionItem> {
        let package = package_at(index, offset);
        let module = module_at(index, offset);
        let mut found = BTreeSet::new();
        let Some((name, at)) = path.split_last() else {
            return found;
        };
        if at.is_empty() {
            self.functions_into(&package, &module, name, 0, &mut found);
        } else if let Some((package, absolute)) = self.absolute(&package, &module, path, 0)
            && let Some((name, at)) = absolute.split_last()
        {
            self.functions_into(&package, &at.join("::"), name, 0, &mut found);
        }
        found
    }

    /// The package and absolute module path `path`, written in `module` of `package`, names.
    fn absolute(
        &self,
        package: &str,
        module: &str,
        path: &[String],
        depth: usize,
    ) -> Option<(String, Vec<String>)> {
        if depth > PATH_STEPS {
            return None;
        }
        let (first, rest) = path.split_first()?;
        let mut here: Vec<String> = module.split("::").map(str::to_owned).collect();
        match first.as_str() {
            "crate" => Some((package.to_owned(), path.to_vec())),
            "self" => {
                here.extend(rest.iter().cloned());
                Some((package.to_owned(), here))
            }
            "super" => {
                let mut rest = path;
                while let Some((first, after)) = rest.split_first()
                    && first == "super"
                {
                    here.pop();
                    rest = after;
                }
                if here.is_empty() {
                    return None;
                }
                here.extend(rest.iter().cloned());
                Some((package.to_owned(), here))
            }
            _ => {
                let imports = self.imports(package, module);
                if let Some(imported) = imports.and_then(|imports| imports.names.get(first)) {
                    let (package, mut expanded) =
                        self.absolute(package, module, imported, depth + 1)?;
                    expanded.extend(rest.iter().cloned());
                    return Some((package, expanded));
                }
                if matches!(first.as_str(), "std" | "core" | "alloc") {
                    return None;
                }
                let child = format!("{module}::{first}");
                if self.is_module(package, &child) {
                    here.extend(path.iter().cloned());
                    return Some((package.to_owned(), here));
                }
                if let Some(other) = self.declarations.crates.get(first) {
                    let mut rooted = vec!["crate".to_owned()];
                    rooted.extend(rest.iter().cloned());
                    return Some((other.clone(), rooted));
                }
                for glob in imports
                    .map(|imports| imports.globs.as_slice())
                    .unwrap_or_default()
                {
                    let Some((at, mut opened)) = self.absolute(package, module, glob, depth + 1)
                    else {
                        continue;
                    };
                    opened.push(first.clone());
                    if self.is_module(&at, &opened.join("::")) {
                        opened.extend(rest.iter().cloned());
                        return Some((at, opened));
                    }
                }
                None
            }
        }
    }

    /// The functions `module::name` of `package` names: declared there, or brought in by a `use`
    /// item or a glob, followed to the declaration, across crates.
    fn functions_into(
        &self,
        package: &str,
        module: &str,
        name: &str,
        depth: usize,
        found: &mut BTreeSet<FunctionItem>,
    ) {
        if depth > PATH_STEPS {
            return;
        }
        let declared: Vec<FunctionItem> = self
            .declarations
            .functions
            .get(name)
            .into_iter()
            .flatten()
            .filter(|decl| decl.place.0 == package && decl.place.1 == module)
            .map(|decl| FunctionItem {
                package: decl.place.0.clone(),
                module: decl.place.1.clone(),
                name: name.to_owned(),
            })
            .collect();
        if !declared.is_empty() {
            found.extend(declared);
            return;
        }
        let Some(imports) = self.imports(package, module) else {
            return;
        };
        if let Some(path) = imports.names.get(name) {
            if let Some((at, absolute)) = self.absolute(package, module, path, depth + 1)
                && let Some((last, within)) = absolute.split_last()
            {
                self.functions_into(&at, &within.join("::"), last, depth + 1, found);
            }
            return;
        }
        for glob in &imports.globs {
            if let Some((at, absolute)) = self.absolute(package, module, glob, depth + 1) {
                self.functions_into(&at, &absolute.join("::"), name, depth + 1, found);
            }
        }
    }

    fn imports(&self, package: &str, module: &str) -> Option<&super::types::Imports> {
        self.declarations
            .imports
            .get(&(package.to_owned(), module.to_owned()))
    }

    fn is_module(&self, package: &str, module: &str) -> bool {
        self.declarations
            .imports
            .contains_key(&(package.to_owned(), module.to_owned()))
    }
}
