//! What a type means, what its fields and methods are: the resolver the walk
//! stands on. Every answer comes from a declaration ([`super::types`]) or is
//! an `Err` with the reason, which the walk turns into an unknown row.

use std::collections::{BTreeMap, BTreeSet};

use super::types::{
    Class, Declarations, Method, Named, Place, Receiver, Std, Ty, TypeDecl, builds_itself,
    closures_by_position, entry_method, external_method, iterator_method, prelude_method, std_kind,
    std_method,
};

/// One field of the four: where it is declared and whether it is a hub.
#[derive(Clone, Debug)]
pub(crate) struct Fact {
    pub(crate) owner: String,
    pub(crate) name: String,
    pub(crate) ty: Ty,
    /// Its type holds one of the four structs — `WindowRuntime.tabs`,
    /// `TabState.sessions` — so a change to what it holds is membership.
    pub(crate) hub: bool,
}

impl Fact {
    pub(crate) fn label(&self) -> String {
        format!("{}.{}", self.owner, self.name)
    }
}

/// The census's subject: the package, its four structs with where each is
/// declared, and their fields.
pub(crate) struct Subject {
    pub(crate) package: String,
    pub(crate) structs: BTreeMap<String, Place>,
    pub(crate) facts: Vec<Fact>,
    pub(crate) by_name: BTreeMap<(String, String), usize>,
    /// Every field name of the four: a site spelling one of these through a
    /// receiver that does not resolve is an unknown, never a guess.
    pub(crate) names: BTreeSet<String>,
}

/// What a type resolves to.
pub(crate) enum Kind<'d> {
    Four(String),
    Workspace {
        name: String,
        decls: Vec<&'d TypeDecl>,
        packages: BTreeSet<String>,
    },
    Std(Std),
    External(String),
    Ambiguous(String),
}

pub(crate) enum Owner {
    Fact(usize),
    Other,
}

pub(crate) struct FieldAnswer {
    pub(crate) owner: Owner,
    pub(crate) ty: Ty,
    pub(crate) shared: bool,
}

pub(crate) struct MethodAnswer {
    pub(crate) method: Method,
    /// The method is declared on one of the four: the writes are inside it,
    /// and the place it is called on is only the way there.
    pub(crate) on_four: bool,
    pub(crate) shared: bool,
}

/// A method that did not resolve: why, and whether the place it was asked of
/// is reached through a shared reference — in which case it cannot be a
/// write whatever it is.
pub(crate) struct Unresolved {
    pub(crate) why: String,
    pub(crate) shared: bool,
}

/// How a name was placed: the rule that found its declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum How {
    /// Declared in the module the name is written in, or where its path says.
    Exact,
    /// In the package the name is written in.
    Home,
    /// Somewhere in the workspace.
    Anywhere,
}

/// The resolver: declarations and subject, and the rules that stand on them.
pub(crate) struct Resolver<'d> {
    pub(crate) declarations: &'d Declarations,
    pub(crate) subject: &'d Subject,
}

impl<'d> Resolver<'d> {
    /// The declarations of `places` that `named` can mean, by the rules of
    /// [`super::types`]'s header.
    fn locate(&self, named: &Named, places: &[Place]) -> Option<(Vec<Place>, How)> {
        let mut places: Vec<Place> = places.to_vec();
        if let Some(krate) = &named.krate {
            let package = if krate == "crate" {
                named.home.clone()
            } else {
                self.declarations.crates.get(krate)?.clone()
            };
            places.retain(|place| place.0 == package);
            let module = std::iter::once("crate".to_owned())
                .chain(named.prefix.iter().cloned())
                .collect::<Vec<_>>()
                .join("::");
            let exact: Vec<Place> = places
                .iter()
                .filter(|place| place.1 == module)
                .cloned()
                .collect();
            if !exact.is_empty() {
                return Some((exact, How::Exact));
            }
            return (!places.is_empty()).then_some((places, How::Home));
        }
        let mut module: Vec<&str> = named.module.split("::").collect();
        for segment in &named.prefix {
            match segment.as_str() {
                "self" => {}
                "super" => {
                    module.pop();
                }
                other => module.push(other),
            }
        }
        let module = module.join("::");
        let exact: Vec<Place> = places
            .iter()
            .filter(|place| place.0 == named.home && place.1 == module)
            .cloned()
            .collect();
        if !exact.is_empty() {
            return Some((exact, How::Exact));
        }
        if !named.prefix.is_empty() {
            let suffix = format!(
                "::{}",
                named
                    .prefix
                    .iter()
                    .filter(|segment| !matches!(segment.as_str(), "self" | "super"))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("::")
            );
            let ending: Vec<Place> = places
                .iter()
                .filter(|place| place.1.ends_with(&suffix))
                .cloned()
                .collect();
            if !ending.is_empty() {
                return Some((ending, How::Exact));
            }
        }
        let home: Vec<Place> = places
            .iter()
            .filter(|place| place.0 == named.home)
            .cloned()
            .collect();
        if !home.is_empty() {
            return Some((home, How::Home));
        }
        (!places.is_empty()).then_some((places, How::Anywhere))
    }

    /// A type alias, expanded — when the alias is at least as well placed as
    /// a type of the same name.
    pub(crate) fn expand(&self, ty: Ty) -> Ty {
        let mut ty = ty;
        for _ in 0..8 {
            let Ty::Named(named) = &ty else {
                return ty;
            };
            if matches!(named.krate.as_deref(), Some("std" | "core" | "alloc")) {
                return ty;
            }
            let Some(aliases) = self.declarations.aliases.get(&named.name) else {
                return ty;
            };
            let alias_places: Vec<Place> = aliases
                .iter()
                .filter(|alias| alias.public || alias.place.0 == named.home)
                .map(|alias| alias.place.clone())
                .collect();
            let Some((found, how)) = self.locate(named, &alias_places) else {
                return ty;
            };
            let type_places: Vec<Place> = self
                .declarations
                .types
                .get(&named.name)
                .map(|decls| {
                    decls
                        .iter()
                        .filter(|decl| decl.public || decl.place.0 == named.home)
                        .map(|decl| decl.place.clone())
                        .collect()
                })
                .unwrap_or_default();
            if let Some((_, type_how)) = self.locate(named, &type_places)
                && type_how <= how
            {
                return ty;
            }
            let chosen: Vec<&super::types::AliasDecl> = aliases
                .iter()
                .filter(|alias| found.contains(&alias.place))
                .collect();
            let expansions: BTreeSet<Ty> = chosen
                .iter()
                .map(|alias| {
                    let map: BTreeMap<String, Ty> = alias
                        .generics
                        .iter()
                        .cloned()
                        .zip(named.args.iter().cloned())
                        .collect();
                    alias.target.substitute(&map)
                })
                .collect();
            if expansions.len() != 1 {
                return Ty::unknown(format!("aliases of `{}` that disagree", named.name));
            }
            ty = expansions
                .into_iter()
                .next()
                .unwrap_or_else(|| Ty::unknown("no alias"));
        }
        ty
    }

    pub(crate) fn kind(&self, named: &Named) -> Kind<'d> {
        let name = &named.name;
        if let Some(krate) = &named.krate
            && matches!(krate.as_str(), "std" | "core" | "alloc")
        {
            return std_kind(name)
                .map_or_else(|| Kind::External(format!("{krate}::{name}")), Kind::Std);
        }
        let decls: &[TypeDecl] = self.declarations.types.get(name).map_or(&[], Vec::as_slice);
        let places: Vec<Place> = decls
            .iter()
            .filter(|decl| decl.public || decl.place.0 == named.home)
            .map(|decl| decl.place.clone())
            .collect();
        let Some((found, how)) = self.locate(named, &places) else {
            if named.krate.is_some() && named.krate.as_deref() != Some("crate") {
                return Kind::External(format!(
                    "{}::{name}",
                    named.krate.as_deref().unwrap_or_default()
                ));
            }
            return std_kind(name).map_or_else(|| Kind::External(name.clone()), Kind::Std);
        };
        if how == How::Anywhere && std_kind(name).is_some() {
            return Kind::Ambiguous(format!(
                "`{name}` is both a standard type and a workspace type"
            ));
        }
        if let Some(four) = self.subject.structs.get(name)
            && found.iter().all(|place| place == four)
        {
            return Kind::Four(name.clone());
        }
        let decls: Vec<&TypeDecl> = decls
            .iter()
            .filter(|decl| found.contains(&decl.place))
            .collect();
        let packages = found.iter().map(|place| place.0.clone()).collect();
        Kind::Workspace {
            name: name.clone(),
            decls,
            packages,
        }
    }

    fn fact_of(&self, owner: &str, field: &str) -> Option<usize> {
        self.subject
            .by_name
            .get(&(owner.to_owned(), field.to_owned()))
            .copied()
    }

    /// A struct's field, by name, with its declared type substituted.
    fn declared_field(decls: &[&TypeDecl], args: &[Ty], field: &str) -> Result<Option<Ty>, String> {
        let mut found: Option<Ty> = None;
        let mut missing = false;
        for decl in decls.iter().filter(|decl| !decl.is_enum) {
            let Some((_, ty)) = decl.fields.iter().find(|(name, _)| name == field) else {
                missing = true;
                continue;
            };
            let map: BTreeMap<String, Ty> = decl
                .generics
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect();
            let ty = ty.substitute(&map);
            match &found {
                Some(earlier) if *earlier != ty => {
                    return Err(format!(
                        "two declarations disagree about the type of `{field}`"
                    ));
                }
                _ => found = Some(ty),
            }
        }
        if found.is_some() && missing {
            return Err(format!(
                "`{field}` is declared by some of the types of that name and not by others"
            ));
        }
        Ok(found)
    }

    /// An enum variant's fields, when the scrutinee's type is a workspace enum
    /// that declares the variant.
    pub(crate) fn variant_fields(&self, ty: &Ty, variant: &str) -> Option<Vec<(String, Ty)>> {
        let Ty::Named(named) = self.expand(ty.clone()) else {
            return None;
        };
        let Kind::Workspace { decls, .. } = self.kind(&named) else {
            return None;
        };
        let mut found: Option<Vec<(String, Ty)>> = None;
        for decl in decls.iter().filter(|decl| decl.is_enum) {
            let fields = decl.variants.get(variant)?;
            let map: BTreeMap<String, Ty> = decl
                .generics
                .iter()
                .cloned()
                .zip(named.args.iter().cloned())
                .collect();
            let fields: Vec<(String, Ty)> = fields
                .iter()
                .map(|(name, ty)| (name.clone(), ty.substitute(&map)))
                .collect();
            match &found {
                Some(earlier) if *earlier != fields => return None,
                _ => found = Some(fields),
            }
        }
        found
    }

    pub(crate) fn deref_target(
        &self,
        name: &str,
        packages: &BTreeSet<String>,
        args: &[Ty],
    ) -> Result<Option<Ty>, String> {
        let Some(all) = self.declarations.derefs.get(name) else {
            return Ok(None);
        };
        let mut found: Option<Ty> = None;
        for decl in all.iter().filter(|decl| packages.contains(&decl.package)) {
            let map = substitution(&decl.impl_params, &decl.self_args, args);
            let target = decl.target.substitute(&map);
            match &found {
                Some(earlier) if *earlier != target => {
                    return Err(format!("two `Deref` impls of `{name}` disagree"));
                }
                _ => found = Some(target),
            }
        }
        Ok(found)
    }

    /// `base.field`, with the auto-deref the language applies, and the
    /// census's rule for `Deref`: a type's own fields first, and the target's
    /// only for a name the type does not declare.
    pub(crate) fn field(&self, ty: &Ty, shared: bool, field: &str) -> Result<FieldAnswer, String> {
        let mut ty = ty.clone();
        let mut shared = shared;
        for _ in 0..16 {
            ty = self.expand(ty);
            match ty {
                Ty::Ref { mutable, inner } => {
                    shared = shared || !mutable;
                    ty = *inner;
                }
                Ty::Tuple(items) => {
                    let item = field
                        .parse::<usize>()
                        .ok()
                        .and_then(|at| items.get(at).cloned())
                        .ok_or_else(|| format!("a tuple has no field `{field}`"))?;
                    return Ok(FieldAnswer {
                        owner: Owner::Other,
                        ty: item,
                        shared,
                    });
                }
                Ty::Named(named) => match self.kind(&named) {
                    Kind::Four(owner) => {
                        let decls: Vec<&TypeDecl> = self
                            .declarations
                            .types
                            .get(&owner)
                            .map(|all| {
                                all.iter()
                                    .filter(|decl| {
                                        Some(&decl.place) == self.subject.structs.get(&owner)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if let Some(field_ty) = Self::declared_field(&decls, &named.args, field)? {
                            let fact = self.fact_of(&owner, field).ok_or_else(|| {
                                format!("`{owner}.{field}` is not in the inventory")
                            })?;
                            return Ok(FieldAnswer {
                                owner: Owner::Fact(fact),
                                ty: field_ty,
                                shared,
                            });
                        }
                        let packages = BTreeSet::from([self.subject.package.clone()]);
                        match self.deref_target(&owner, &packages, &named.args)? {
                            Some(target) => ty = target,
                            None => return Err(format!("`{owner}` declares no field `{field}`")),
                        }
                    }
                    Kind::Workspace {
                        name,
                        decls,
                        packages,
                    } => {
                        if let Some(field_ty) = Self::declared_field(&decls, &named.args, field)? {
                            return Ok(FieldAnswer {
                                owner: Owner::Other,
                                ty: field_ty,
                                shared,
                            });
                        }
                        match self.deref_target(&name, &packages, &named.args)? {
                            Some(target) => ty = target,
                            None if decls.iter().all(|decl| decl.is_enum) => {
                                return Err(format!("`{name}` is an enum and has no fields"));
                            }
                            None => return Err(format!("`{name}` declares no field `{field}`")),
                        }
                    }
                    Kind::Std(Std::Box) => ty = first_arg(&named),
                    Kind::Std(Std::Shared) => {
                        shared = true;
                        ty = first_arg(&named);
                    }
                    Kind::Std(_) => {
                        return Err(format!("`{}` has no public field `{field}`", named.name));
                    }
                    Kind::External(name) => {
                        return Err(format!("a field of `{name}`, a type outside the workspace"));
                    }
                    Kind::Ambiguous(why) => return Err(why),
                },
                Ty::Param(name) => return Err(format!("a field of the generic `{name}`")),
                Ty::Unknown(why) => return Err(why),
                Ty::Slice(_) | Ty::Iter(_) | Ty::Entry(_) | Ty::Fn(_) => {
                    return Err(format!("`{field}` on a type with no fields"));
                }
            }
        }
        Err("a deref chain longer than this census follows".to_owned())
    }

    /// Every declaration of `type_name::method`, turned into one answer where
    /// they agree.
    pub(crate) fn workspace_methods(
        &self,
        type_name: &str,
        method: &str,
        packages: Option<&BTreeSet<String>>,
        args: &[Ty],
    ) -> Result<Option<Method>, String> {
        let Some(all) = self
            .declarations
            .methods
            .get(&(type_name.to_owned(), method.to_owned()))
        else {
            return Ok(None);
        };
        let answers: Vec<Method> = all
            .iter()
            .map(|decl| {
                let map = substitution(&decl.impl_params, &decl.self_args, args);
                let ret = decl.ret.substitute(&map);
                let ret = if ret.is_open() {
                    Ty::unknown(format!("`{type_name}::{method}` returns a generic type"))
                } else {
                    ret
                };
                let class = match decl.receiver {
                    Receiver::Unique if holds_unique_reference(&ret) => Class::Access,
                    Receiver::Unique => Class::Write,
                    Receiver::Shared | Receiver::Value | Receiver::None => Class::Read,
                };
                // By the declaration's own parameters, receiver excluded:
                // called as `x.f(a)` that is the arguments' order, and called
                // as `Type::f(x, a)` [`Resolver::associated`] shifts it.
                let params: Vec<Ty> = decl
                    .params
                    .iter()
                    .map(|param| param.substitute(&map))
                    .collect();
                let by_position = closures_by_position(&params);
                Method {
                    class,
                    ret,
                    closure: Vec::new(),
                    by_position,
                }
            })
            .collect();
        if let Some(agreed) = agreement(&answers) {
            return Ok(Some(agreed));
        }
        // They disagree: the ones declared where the type is.
        if let Some(packages) = packages {
            let narrowed: Vec<Method> = all
                .iter()
                .zip(answers)
                .filter(|(decl, _)| packages.contains(&decl.package))
                .map(|(_, answer)| answer)
                .collect();
            if let Some(agreed) = agreement(&narrowed) {
                return Ok(Some(agreed));
            }
        }
        Err(format!(
            "the declarations of `{type_name}::{method}` disagree"
        ))
    }

    /// Whether `Type::method` is declared with a receiver — called with the
    /// path syntax, its first argument is then the receiver.
    pub(crate) fn has_receiver(&self, type_name: &str, method: &str) -> bool {
        self.declarations
            .methods
            .get(&(type_name.to_owned(), method.to_owned()))
            .is_some_and(|all| all.iter().any(|decl| decl.receiver != Receiver::None))
    }

    fn prelude(&self, method: &str, receiver: &Ty) -> Option<Method> {
        if self.declarations.trait_methods.contains(method) {
            return None;
        }
        prelude_method(method, receiver)
    }

    /// `receiver.method(..)`, with the auto-deref the language applies.
    pub(crate) fn method(
        &self,
        ty: &Ty,
        shared: bool,
        method: &str,
    ) -> Result<MethodAnswer, Unresolved> {
        let mut ty = ty.clone();
        let mut shared = shared;
        let fail = |why: String, shared: bool| Unresolved { why, shared };
        for _ in 0..16 {
            ty = self.expand(ty);
            let current = ty.clone();
            match ty {
                Ty::Ref { mutable, inner } => {
                    shared = shared || !mutable;
                    ty = *inner;
                }
                Ty::Iter(item) => {
                    return iterator_method(&item, method)
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(
                                format!("`{method}` is not on the census's iterator list"),
                                shared,
                            )
                        });
                }
                Ty::Entry(value) => {
                    return entry_method(&value, method)
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(
                                format!("`{method}` is not on the census's entry list"),
                                shared,
                            )
                        });
                }
                Ty::Slice(element) => {
                    return std_method(Std::Vec, &[*element], method)
                        .or_else(|| self.prelude(method, &current))
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(
                                format!("`[T]::{method}` is not on the census's list"),
                                shared,
                            )
                        });
                }
                Ty::Tuple(_) | Ty::Fn(_) => {
                    return self
                        .prelude(method, &current)
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(format!("`{method}` on {}", current.spelling()), shared)
                        });
                }
                Ty::Param(name) => {
                    return self
                        .prelude(method, &current)
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(format!("`{method}` on the generic `{name}`"), shared)
                        });
                }
                Ty::Unknown(why) => return Err(fail(why, shared)),
                Ty::Named(named) => match self.kind(&named) {
                    Kind::Four(owner) => {
                        let packages = BTreeSet::from([self.subject.package.clone()]);
                        let found = self
                            .workspace_methods(&owner, method, Some(&packages), &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            return Ok(answer(found, true, shared));
                        }
                        match self
                            .deref_target(&owner, &packages, &named.args)
                            .map_err(|why| fail(why, shared))?
                        {
                            Some(target) => ty = target,
                            None => {
                                return self
                                    .prelude(method, &current)
                                    .map(|found| answer(found, true, shared))
                                    .ok_or_else(|| {
                                        fail(
                                            format!("`{owner}` declares no method `{method}`"),
                                            shared,
                                        )
                                    });
                            }
                        }
                    }
                    Kind::Workspace { name, packages, .. } => {
                        let found = self
                            .workspace_methods(&name, method, Some(&packages), &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            return Ok(answer(found, false, shared));
                        }
                        match self
                            .deref_target(&name, &packages, &named.args)
                            .map_err(|why| fail(why, shared))?
                        {
                            Some(target) => ty = target,
                            None => {
                                return self
                                    .prelude(method, &current)
                                    .map(|found| answer(found, false, shared))
                                    .ok_or_else(|| {
                                        fail(
                                            format!("`{name}` declares no method `{method}`"),
                                            shared,
                                        )
                                    });
                            }
                        }
                    }
                    Kind::Std(kind) => {
                        if let Some(found) = std_method(kind, &named.args, method) {
                            return Ok(answer(found, false, shared));
                        }
                        // A workspace extension trait implemented for the
                        // standard type.
                        let found = self
                            .workspace_methods(&named.name, method, None, &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            return Ok(answer(found, false, shared));
                        }
                        match kind {
                            Std::Box => ty = first_arg(&named),
                            Std::Shared => {
                                shared = true;
                                ty = first_arg(&named);
                            }
                            _ => {
                                return self
                                    .prelude(method, &current)
                                    .map(|found| answer(found, false, shared))
                                    .ok_or_else(|| {
                                        fail(
                                            format!(
                                                "`{}::{method}` is not on the census's list",
                                                named.name
                                            ),
                                            shared,
                                        )
                                    });
                            }
                        }
                    }
                    Kind::External(name) => {
                        let found = self
                            .workspace_methods(&named.name, method, None, &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            return Ok(answer(found, false, shared));
                        }
                        return external_method(&named.name, method)
                            .or_else(|| self.prelude(method, &current))
                            .map(|found| answer(found, false, shared))
                            .ok_or_else(|| {
                                fail(
                                    format!("`{name}::{method}`, a type outside the workspace"),
                                    shared,
                                )
                            });
                    }
                    Kind::Ambiguous(why) => return Err(fail(why, shared)),
                },
            }
        }
        Err(fail(
            "a deref chain longer than this census follows".to_owned(),
            shared,
        ))
    }

    /// Whether `ty` (after references and aliases) is one of the four.
    pub(crate) fn is_four(&self, ty: &Ty) -> bool {
        match self.expand(ty.clone()) {
            Ty::Ref { inner, .. } => self.is_four(&inner),
            Ty::Named(named) => matches!(self.kind(&named), Kind::Four(_)),
            _ => false,
        }
    }

    /// `Type::function(..)`: what it returns and what closures in each
    /// argument position receive. `owner` is the type the path names.
    pub(crate) fn associated(&self, owner: &Ty, function: &str) -> (Ty, Vec<Option<Vec<Ty>>>) {
        let Ty::Named(named) = self.expand(owner.clone()) else {
            return (
                Ty::unknown(format!("`{function}` on a type that is not a path")),
                Vec::new(),
            );
        };
        let packages = match self.kind(&named) {
            Kind::Four(_) => Some(BTreeSet::from([self.subject.package.clone()])),
            Kind::Workspace { packages, .. } => Some(packages),
            Kind::Std(_) => None,
            Kind::External(name) | Kind::Ambiguous(name) => {
                if builds_itself(function) {
                    return (Ty::Named(named), Vec::new());
                }
                return (Ty::unknown(format!("`{name}::{function}`")), Vec::new());
            }
        };
        let found = match packages {
            Some(packages) => {
                self.workspace_methods(&named.name, function, Some(&packages), &named.args)
            }
            None => Ok(None),
        };
        match found {
            Ok(Some(found)) => {
                let mut by_position = found.by_position;
                if self.has_receiver(&named.name, function) {
                    by_position.insert(0, None);
                }
                (found.ret, by_position)
            }
            Ok(None) if builds_itself(function) => (Ty::Named(named), Vec::new()),
            Ok(None) => (
                Ty::unknown(format!("`{}::{function}` is not declared", named.name)),
                Vec::new(),
            ),
            Err(why) => (Ty::unknown(why), Vec::new()),
        }
    }
}

pub(crate) fn first_arg(named: &Named) -> Ty {
    named
        .args
        .first()
        .cloned()
        .unwrap_or_else(|| Ty::unknown(format!("`{}` without its argument", named.name)))
}

fn answer(method: Method, on_four: bool, shared: bool) -> MethodAnswer {
    MethodAnswer {
        method,
        on_four,
        shared,
    }
}

pub(crate) fn substitution(
    params: &[String],
    self_args: &[Ty],
    args: &[Ty],
) -> BTreeMap<String, Ty> {
    let mut map = BTreeMap::new();
    for (written, given) in self_args.iter().zip(args) {
        if let Ty::Param(name) = written
            && params.contains(name)
        {
            map.insert(name.clone(), given.clone());
        }
    }
    map
}

fn agreement(answers: &[Method]) -> Option<Method> {
    let first = answers.first()?;
    if answers.iter().any(|other| other.class != first.class) {
        return None;
    }
    let mut agreed = first.clone();
    if answers.iter().any(|other| other.ret != first.ret) {
        agreed.ret = Ty::unknown("declarations that disagree about what they return");
    }
    if answers
        .iter()
        .any(|other| other.by_position != first.by_position)
    {
        agreed.by_position = Vec::new();
    }
    Some(agreed)
}

fn holds_unique_reference(ty: &Ty) -> bool {
    match ty {
        Ty::Ref { mutable, inner } => *mutable || holds_unique_reference(inner),
        Ty::Named(named) => named.args.iter().any(holds_unique_reference),
        Ty::Tuple(items) => items.iter().any(holds_unique_reference),
        Ty::Iter(inner) | Ty::Slice(inner) => holds_unique_reference(inner),
        Ty::Entry(_) => true,
        Ty::Fn(_) | Ty::Param(_) | Ty::Unknown(_) => false,
    }
}
