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
//!
//! The rules are [`Follow`]'s, one reading for two askers: [`FunctionPaths`] over every `use`
//! item, and the census's typing of a one-segment call (plan row 126) over the `use` items a
//! product build compiles, keeping the paths that leave the crates read as the reason a call is
//! refused.

use std::collections::BTreeSet;

use super::types::{Declarations, FnDecl, Imports, module_at, package_at};
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
        let follow = Follow {
            declarations: &self.declarations,
            uses: Uses::Every,
        };
        follow
            .path(&package_at(index, offset), &module_at(index, offset), path)
            .declarations
            .into_iter()
            .map(|decl| FunctionItem {
                package: decl.place.0.clone(),
                module: decl.place.1.clone(),
                name: decl.name.clone(),
            })
            .collect()
    }
}

/// Which `use` items a path is followed through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Uses {
    /// Every `use` item, a test build's too.
    Every,
    /// Those a product build can compile: a `use` under `cfg(test)` brings nothing in.
    Product,
}

/// What a free function's path reaches across the workspace's crates.
#[derive(Debug, Default)]
pub(crate) struct Found<'d> {
    /// The declarations it names, test-only ones included.
    pub(crate) declarations: Vec<&'d FnDecl>,
    /// The paths on the way that lead outside the crates read, as written (`std::mem::take`, a
    /// glob `std::io::*`): what the name may have been brought in by instead.
    pub(crate) unread: BTreeSet<String>,
}

/// The path rules over one set of declarations.
pub(crate) struct Follow<'d> {
    pub(crate) declarations: &'d Declarations,
    pub(crate) uses: Uses,
}

impl<'d> Follow<'d> {
    /// The free functions `path`, written in `module` of `package`, names: one segment is a name
    /// looked up in the module, more are a path placed first.
    pub(crate) fn path(&self, package: &str, module: &str, path: &[String]) -> Found<'d> {
        let mut found = Found::default();
        let Some((name, at)) = path.split_last() else {
            return found;
        };
        if at.is_empty() {
            self.functions_into(package, module, name, 0, &mut found);
        } else if let Some((package, absolute)) = self.absolute(package, module, path, 0)
            && let Some((name, at)) = absolute.split_last()
        {
            self.functions_into(&package, &at.join("::"), name, 0, &mut found);
        } else {
            found.unread.insert(path.join("::"));
        }
        found
    }

    /// The package and absolute module path `path`, written in `module` of `package`, names:
    /// its first segment placed, then each later one that is not a module there but a name a
    /// `use` item of the module so far brings in (`crate::attention::f` where the crate root
    /// writes `use other::attention;`) replaced by what that `use` names.
    fn absolute(
        &self,
        package: &str,
        module: &str,
        path: &[String],
        depth: usize,
    ) -> Option<(String, Vec<String>)> {
        let (package, placed) = self.placed(package, module, path, depth)?;
        self.settled(package, placed, depth)
    }

    /// `path` with each segment after the first that is a `use` name of the module before it
    /// replaced by the path it names, so every segment but the last is a module.
    fn settled(
        &self,
        mut package: String,
        mut path: Vec<String>,
        mut depth: usize,
    ) -> Option<(String, Vec<String>)> {
        let mut at = 1;
        while at < path.len() {
            let module = path[..at].join("::");
            if self.is_module(&package, &format!("{module}::{}", path[at])) {
                at += 1;
                continue;
            }
            let Some(imported) = self
                .imports(&package, &module)
                .and_then(|imports| imports.get(&path[at]))
            else {
                at += 1;
                continue;
            };
            depth += 1;
            let (to, mut expanded) = self.placed(&package, &module, imported, depth)?;
            expanded.extend(path[at + 1..].iter().cloned());
            package = to;
            path = expanded;
            at = 1;
        }
        Some((package, path))
    }

    /// The first segment of `path` placed: `crate`, `self`, `super`, a child module (a module
    /// is looked up before a `use` name, which may be a function of the same name — `pub use
    /// solve::solve;`), a `use` name, a workspace crate, or a module a glob brings in.
    fn placed(
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
                let child = format!("{module}::{first}");
                if self.is_module(package, &child) {
                    here.extend(path.iter().cloned());
                    return Some((package.to_owned(), here));
                }
                let imports = self.imports(package, module);
                if let Some(imported) = imports.and_then(|imports| imports.get(first)) {
                    let (package, mut expanded) =
                        self.placed(package, module, imported, depth + 1)?;
                    expanded.extend(rest.iter().cloned());
                    return Some((package, expanded));
                }
                if matches!(first.as_str(), "std" | "core" | "alloc") {
                    return None;
                }
                if let Some(other) = self.declarations.crates.get(first) {
                    let mut rooted = vec!["crate".to_owned()];
                    rooted.extend(rest.iter().cloned());
                    return Some((other.clone(), rooted));
                }
                for glob in imports.map(Brought::globs).unwrap_or_default() {
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
    /// item or a glob, followed to the declaration, across crates. A `use` path or a glob that
    /// cannot be placed is kept in [`Found::unread`].
    fn functions_into(
        &self,
        package: &str,
        module: &str,
        name: &str,
        depth: usize,
        found: &mut Found<'d>,
    ) {
        if depth > PATH_STEPS {
            return;
        }
        let declared: Vec<&'d FnDecl> = self
            .declarations
            .functions
            .get(name)
            .into_iter()
            .flatten()
            .filter(|decl| decl.place.0 == package && decl.place.1 == module)
            .collect();
        if !declared.is_empty() {
            found.declarations.extend(declared);
            return;
        }
        let Some(imports) = self.imports(package, module) else {
            return;
        };
        if let Some(path) = imports.get(name) {
            match self.absolute(package, module, path, depth + 1) {
                Some((at, absolute)) => {
                    if let Some((last, within)) = absolute.split_last() {
                        self.functions_into(&at, &within.join("::"), last, depth + 1, found);
                    }
                }
                None => {
                    found.unread.insert(path.join("::"));
                }
            }
            return;
        }
        for glob in imports.globs() {
            match self.absolute(package, module, glob, depth + 1) {
                Some((at, absolute)) => {
                    self.functions_into(&at, &absolute.join("::"), name, depth + 1, found);
                }
                None => {
                    found.unread.insert(format!("{}::*", glob.join("::")));
                }
            }
        }
    }

    fn imports(&self, package: &str, module: &str) -> Option<Brought<'d>> {
        self.declarations
            .imports
            .get(&(package.to_owned(), module.to_owned()))
            .map(|imports| Brought {
                imports,
                uses: self.uses,
            })
    }

    fn is_module(&self, package: &str, module: &str) -> bool {
        self.declarations
            .imports
            .contains_key(&(package.to_owned(), module.to_owned()))
    }
}

/// One module's `use` items, as many of them as [`Uses`] says.
#[derive(Clone, Copy)]
struct Brought<'d> {
    imports: &'d Imports,
    uses: Uses,
}

impl<'d> Brought<'d> {
    fn get(self, name: &str) -> Option<&'d Vec<String>> {
        match self.uses {
            Uses::Every => self.imports.names.get(name),
            Uses::Product => self.imports.product_names.get(name),
        }
    }

    fn globs(self) -> &'d [Vec<String>] {
        match self.uses {
            Uses::Every => &self.imports.globs,
            Uses::Product => &self.imports.product_globs,
        }
    }
}
