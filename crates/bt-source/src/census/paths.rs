//! **A free function's path, followed to the item it names across the workspace's packages** —
//! the census's own path rules ([`super::resolve`]: `crate`, `self`, `super`, a module's `use`
//! items and globs, a child module), with one step more: a first segment that is a workspace
//! crate's name continues in that crate's root, so a `use other_crate::m::f as g;` and a
//! `pub(crate) use other_crate::m::f;` re-export both lead to `other_crate`'s `m::f`.
//!
//! The answer is an item's identity — package, module path, name — never a spelling: two
//! functions called `contain` in two modules called `contained_panic` are two answers. A path the
//! rules do not place (into a crate that is not among the indexes read, a module a macro writes,
//! `super` past the crate root, a chain of `use` items longer than they follow) names nothing and
//! is kept, with why, as a [`Refusal`]. The rules read the `use` items of modules only: a `use` or
//! a `fn` written inside a function body is the census walk's to refuse (`Walk::block_item`).
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
            .map(|reached| FunctionItem {
                package: reached.decl.place.0.clone(),
                module: reached.decl.place.1.clone(),
                name: reached.name.to_owned(),
            })
            .collect()
    }
}

/// Which `use` items a path is followed through, and which declarations answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Uses {
    /// Every `use` item, a test build's too; every declaration answers.
    Every,
    /// Those a product build can compile: a `use` under `cfg(test)` brings nothing in, and a
    /// declaration only a test build has is skipped, the lookup going on to the module's `use`
    /// items and globs.
    Product,
}

/// Why the rules place no item for a path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unplaced {
    /// Into `std`, `core` or `alloc`: a crate the census does not read.
    Standard,
    /// Its first segment (the one named) is no module, type or workspace crate the rules place:
    /// a crate outside the workspace, or a module a macro writes.
    Unknown(String),
    /// More `use` steps than [`PATH_STEPS`]: a chain that long, or a cycle.
    Depth,
    /// Its `super` goes past the crate root.
    PastRoot,
    /// Its first segment is a type the module declares (`use Kind::*;` of a local enum): what it
    /// brings in is the type's, never a free function.
    Type,
}

/// A path the rules did not place, as written, and why.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Refusal {
    pub(crate) path: String,
    pub(crate) why: Unplaced,
}

impl Refusal {
    /// Why a call of `name` that came this way is refused; `None` for a type's path, which
    /// refuses nothing.
    pub(crate) fn reason(&self, name: &str) -> Option<String> {
        let path = &self.path;
        Some(match &self.why {
            Unplaced::Standard => format!(
                "`{name}` comes through `{path}`, a path into a crate the census does not read"
            ),
            Unplaced::Unknown(first) => format!(
                "`{name}` comes through `{path}`, whose first segment `{first}` is no module, type \
                 or workspace crate the rules place: a crate outside the workspace, or a module a \
                 macro writes"
            ),
            Unplaced::Depth => format!(
                "`{name}` comes through a chain of `use` items longer than the rules follow \
                 ({PATH_STEPS} steps; a cycle never ends)"
            ),
            Unplaced::PastRoot => {
                format!("`{name}` comes through `{path}`, whose `super` goes past the crate root")
            }
            Unplaced::Type => return None,
        })
    }
}

/// One declaration a path reaches, with the name it is declared under.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reached<'d> {
    pub(crate) name: &'d str,
    pub(crate) decl: &'d FnDecl,
}

/// What a free function's path reaches across the workspace's crates.
#[derive(Debug, Default)]
pub(crate) struct Found<'d> {
    /// The declarations it names: under [`Uses::Product`] those a product build has, under
    /// [`Uses::Every`] test-only ones too.
    pub(crate) declarations: Vec<Reached<'d>>,
    /// A module on the way declares the name only for a test build.
    pub(crate) test_only: bool,
    /// The paths on the way the rules do not place, as written (`std::mem::take`, a glob
    /// `std::io::*`), and why: what the name may have been brought in by instead.
    pub(crate) refused: BTreeSet<Refusal>,
}

impl Found<'_> {
    /// Why a call of `name` that reached nothing is refused: the refusal of the smallest path in
    /// [`Found::refused`] that refuses (a type's path does not) — one fixed choice, not the order
    /// the paths were met in.
    pub(crate) fn refusal(&self, name: &str) -> Option<String> {
        self.refused.iter().find_map(|refusal| refusal.reason(name))
    }
}

/// What a path names: a value (`f`, `m::f`; its last segment is the name looked up in the
/// module before it) or a module (a glob's path, `m::*`; every segment is a module).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Value,
    Module,
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
            return found;
        }
        match self.absolute(package, module, path, Shape::Value, 0) {
            Ok((package, absolute)) => {
                if let Some((name, at)) = absolute.split_last() {
                    self.functions_into(&package, &at.join("::"), name, 0, &mut found);
                }
            }
            Err(why) => {
                found.refused.insert(Refusal {
                    path: path.join("::"),
                    why,
                });
            }
        }
        found
    }

    /// The package and absolute path `path`, written in `module` of `package`, names: its first
    /// segment placed, then each later one that is not a module there but a name a `use` item of
    /// the module so far brings in (`crate::attention::f` where the crate root writes `use
    /// other::attention;`) replaced by what that `use` names. A value path's last segment is the
    /// value name, which [`Self::functions_into`] looks up, declarations first, so it is left as
    /// written; a module path's last segment is a module like the others (`use crate::hub::wins::*;`
    /// where `hub` writes `pub use crate::windows as wins;`).
    fn absolute(
        &self,
        package: &str,
        module: &str,
        path: &[String],
        shape: Shape,
        depth: usize,
    ) -> Result<(String, Vec<String>), Unplaced> {
        let (package, placed) = self.placed(package, module, path, depth)?;
        self.settled(package, placed, shape, depth)
    }

    /// `path` with each segment after the first that is a `use` name of the module before it
    /// replaced by the path it names — up to the last for a module path, before it for a value
    /// path — so every segment it settles is a module.
    fn settled(
        &self,
        mut package: String,
        mut path: Vec<String>,
        shape: Shape,
        mut depth: usize,
    ) -> Result<(String, Vec<String>), Unplaced> {
        let value = usize::from(shape == Shape::Value);
        let mut at = 1;
        while at + value < path.len() {
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
        Ok((package, path))
    }

    /// The first segment of `path` placed: `crate`, `self`, `super`, a child module (a module
    /// is looked up before a `use` name, which may be a function of the same name — `pub use
    /// solve::solve;`), a type the module declares (placed nowhere: [`Unplaced::Type`]), a `use`
    /// name, a workspace crate, or a module a glob brings in.
    fn placed(
        &self,
        package: &str,
        module: &str,
        path: &[String],
        depth: usize,
    ) -> Result<(String, Vec<String>), Unplaced> {
        if depth > PATH_STEPS {
            return Err(Unplaced::Depth);
        }
        let Some((first, rest)) = path.split_first() else {
            return Err(Unplaced::Unknown(String::new()));
        };
        let mut here: Vec<String> = module.split("::").map(str::to_owned).collect();
        match first.as_str() {
            "crate" => Ok((package.to_owned(), path.to_vec())),
            "self" => {
                here.extend(rest.iter().cloned());
                Ok((package.to_owned(), here))
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
                    return Err(Unplaced::PastRoot);
                }
                here.extend(rest.iter().cloned());
                Ok((package.to_owned(), here))
            }
            _ => {
                let child = format!("{module}::{first}");
                if self.is_module(package, &child) {
                    here.extend(path.iter().cloned());
                    return Ok((package.to_owned(), here));
                }
                if self.declares_type(package, module, first) {
                    return Err(Unplaced::Type);
                }
                let imports = self.imports(package, module);
                if let Some(imported) = imports.and_then(|imports| imports.get(first)) {
                    let (package, mut expanded) =
                        self.placed(package, module, imported, depth + 1)?;
                    expanded.extend(rest.iter().cloned());
                    return Ok((package, expanded));
                }
                if matches!(first.as_str(), "std" | "core" | "alloc") {
                    return Err(Unplaced::Standard);
                }
                if let Some(other) = self.declarations.crates.get(first) {
                    let mut rooted = vec!["crate".to_owned()];
                    rooted.extend(rest.iter().cloned());
                    return Ok((other.clone(), rooted));
                }
                for glob in imports.map(Brought::globs).unwrap_or_default() {
                    let Ok((at, mut opened)) =
                        self.absolute(package, module, glob, Shape::Module, depth + 1)
                    else {
                        continue;
                    };
                    opened.push(first.clone());
                    if self.is_module(&at, &opened.join("::")) {
                        opened.extend(rest.iter().cloned());
                        return Ok((at, opened));
                    }
                }
                Err(Unplaced::Unknown(first.clone()))
            }
        }
    }

    /// The functions `module::name` of `package` names: declared there, or brought in by a `use`
    /// item or a glob, followed to the declaration, across crates. Under [`Uses::Product`] a
    /// declaration only a test build has is skipped ([`Found::test_only`]) and the lookup goes on.
    /// A `use` path or a glob the rules do not place is kept in [`Found::refused`]; a type's path,
    /// which brings in no function, is not.
    fn functions_into(
        &self,
        package: &str,
        module: &str,
        name: &str,
        depth: usize,
        found: &mut Found<'d>,
    ) {
        if depth > PATH_STEPS {
            found.refused.insert(Refusal {
                path: format!("{module}::{name}"),
                why: Unplaced::Depth,
            });
            return;
        }
        if let Some((name, decls)) = self.declarations.functions.get_key_value(name) {
            let declared: Vec<&'d FnDecl> = decls
                .iter()
                .filter(|decl| decl.place.0 == package && decl.place.1 == module)
                .collect();
            let answers: Vec<Reached<'d>> = declared
                .iter()
                .filter(|decl| self.uses == Uses::Every || decl.callable.is_some())
                .map(|decl| Reached { name, decl })
                .collect();
            if !answers.is_empty() {
                found.declarations.extend(answers);
                return;
            }
            found.test_only |= !declared.is_empty();
        }
        let Some(imports) = self.imports(package, module) else {
            return;
        };
        if let Some(path) = imports.get(name) {
            match self.absolute(package, module, path, Shape::Value, depth + 1) {
                Ok((at, absolute)) => {
                    if let Some((last, within)) = absolute.split_last() {
                        self.functions_into(&at, &within.join("::"), last, depth + 1, found);
                    }
                }
                Err(Unplaced::Type) => {}
                Err(why) => {
                    found.refused.insert(Refusal {
                        path: path.join("::"),
                        why,
                    });
                }
            }
            return;
        }
        for glob in imports.globs() {
            match self.absolute(package, module, glob, Shape::Module, depth + 1) {
                Ok((at, absolute)) => {
                    self.functions_into(&at, &absolute.join("::"), name, depth + 1, found);
                }
                Err(Unplaced::Type) => {}
                Err(why) => {
                    found.refused.insert(Refusal {
                        path: format!("{}::*", glob.join("::")),
                        why,
                    });
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

    /// Whether `module` of `package` declares a struct or an enum named `name`.
    fn declares_type(&self, package: &str, module: &str, name: &str) -> bool {
        self.declarations.types.get(name).is_some_and(|decls| {
            decls
                .iter()
                .any(|decl| decl.place.0 == package && decl.place.1 == module)
        })
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
