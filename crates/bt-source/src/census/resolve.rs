//! What a type means, what its fields and methods are: the resolver the walk
//! stands on. Every answer comes from a declaration ([`super::types`]) or is
//! an `Err` with the reason, which the walk turns into an unknown row.

use std::collections::{BTreeMap, BTreeSet};

use super::Callable;
use super::types::{
    Binding, Callees, Class, Declarations, FnDecl, Method, MethodDecl, Named, Place, Receiver, Std,
    Ty, TypeDecl, builds_itself, closures_by_position, entry_method, external_method,
    iterator_method, prelude_method, std_kind, std_method,
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
    /// The names of the package's own product free functions and methods: a
    /// call spelling one that does not resolve is listed as not followed.
    pub(crate) functions: BTreeSet<String>,
    pub(crate) methods: BTreeSet<String>,
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

/// **Where a type is declared**: the packages and the declarations a name was placed at. A
/// method declared in one of those packages is the type's, by the name it is filed under; one
/// declared elsewhere is the type's only when it is a trait's, implemented on a self type placed
/// at one of these declarations ([`Resolver::workspace_methods`]).
pub(crate) struct Home {
    pub(crate) packages: BTreeSet<String>,
    pub(crate) places: BTreeSet<Place>,
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
                Ty::Assoc { param, name } => {
                    return Err(format!("a field of the associated type `{param}::{name}`"));
                }
                Ty::Traits(_) => return Err("a field of a trait object".to_owned()),
                Ty::Unknown(why) => return Err(why),
                Ty::Slice(_) | Ty::Iter(_) | Ty::Entry(_) | Ty::Fn(_) => {
                    return Err(format!("`{field}` on a type with no fields"));
                }
            }
        }
        Err("a deref chain longer than this census follows".to_owned())
    }

    /// Every declaration of `type_name::method`, turned into one answer where
    /// they agree. `home` is where the receiver's type is declared; `None` for
    /// a standard or an outside type, whose methods the workspace declares only
    /// by its traits.
    pub(crate) fn workspace_methods(
        &self,
        type_name: &str,
        method: &str,
        home: Option<&Home>,
        args: &[Ty],
    ) -> Result<Option<Method>, String> {
        let packages = home.map(|home| &home.packages);
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
                let ret = if ret.is_a_parameter() {
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
                    callees: Callees::default(),
                }
            })
            .collect();
        let callees = self.reached(all, home);
        let reaching = |method: Method| Method {
            callees: callees.clone(),
            ..method
        };
        if let Some(agreed) = agreement(&answers) {
            return Ok(Some(reaching(agreed)));
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
                return Ok(Some(reaching(agreed)));
            }
        }
        Err(format!(
            "the declarations of `{type_name}::{method}` disagree"
        ))
    }

    /// **What a call of one of `all` reaches** on a receiver declared at `home`: the product
    /// declarations that are the receiver type's — and, as the language picks, an inherent method
    /// before a trait's of the same name.
    ///
    /// Methods are filed under their type's bare name, so `all` holds the methods of every type of
    /// that name. One declared in the type's own packages is the type's (an inherent method can be
    /// declared nowhere else). One declared in another package is the type's only when it is a
    /// trait's, implemented on a self type placed at one of the type's own declarations: the
    /// orphan rule puts a package's trait on another package's type in the trait's package, and
    /// the call graph matches an impl to a type the same way (`trait_members`, by the resolved
    /// self type). A trait method whose self type the rules cannot place is not guessed either
    /// way: with no inherent method to answer first, the call is refused with the reason — even
    /// over a trait method of the type's own packages, since the unplaced one may be the type's
    /// too. And a call whose only product declarations of that name are other types' is refused
    /// as well: the answer then stands on a method this type does not declare.
    fn reached(&self, all: &[MethodDecl], home: Option<&Home>) -> Callees {
        let mut found: BTreeSet<Callable> = BTreeSet::new();
        let mut unplaced: Option<String> = None;
        let mut others = false;
        for decl in all {
            let Some(callable) = &decl.callable else {
                continue;
            };
            let Some(home) = home else {
                found.insert(callable.clone());
                continue;
            };
            if home.packages.contains(&decl.package) {
                found.insert(callable.clone());
                continue;
            }
            if !callable.function.starts_with('<') {
                others = true;
                continue;
            }
            let placed = match &decl.self_ty {
                Some(ty) => self.self_type_places(ty),
                None => Err("an `impl` block whose header does not parse".to_owned()),
            };
            match placed {
                Ok(places) if !places.is_disjoint(&home.places) => {
                    found.insert(callable.clone());
                }
                Ok(_) => others = true,
                Err(why) => {
                    unplaced.get_or_insert(why);
                }
            }
        }
        let inherent: BTreeSet<Callable> = found
            .iter()
            .filter(|callable| !callable.function.starts_with('<'))
            .cloned()
            .collect();
        if !inherent.is_empty() {
            return Callees::items(inherent);
        }
        if let Some(why) = unplaced {
            return Callees::refused(format!(
                "a method a trait implements outside its type's package, on a self type the rules \
                 do not place: {why}"
            ));
        }
        if found.is_empty() && others {
            return Callees::others_only(
                "a method only other types of the receiver's name declare".to_owned(),
            );
        }
        Callees::items(found)
    }

    /// The declarations an `impl` block's self type is placed at: none for a standard or an
    /// outside type, which no workspace type's method call reaches.
    fn self_type_places(&self, ty: &Ty) -> Result<BTreeSet<Place>, String> {
        match self.expand(ty.clone()) {
            Ty::Ref { inner, .. } => self.self_type_places(&inner),
            Ty::Named(named) => self.placed_for_sure(&named),
            Ty::Unknown(why) => Err(why),
            Ty::Tuple(_) | Ty::Slice(_) | Ty::Fn(_) | Ty::Iter(_) | Ty::Entry(_) => {
                Ok(BTreeSet::new())
            }
            Ty::Param(name) => Err(format!("an `impl` on the generic `{name}`")),
            Ty::Assoc { param, name } => Err(format!(
                "an `impl` on the associated type `{param}::{name}`"
            )),
            Ty::Traits(_) => Err("an `impl` on a trait object".to_owned()),
        }
    }

    /// **An `impl` header's self type, placed only where the rules are sure.** The rules place a
    /// name without reading `use` items, so a bare name not declared in the impl's own module goes
    /// to a type of that name in the impl's package, then anywhere — and `use other::X; impl T for
    /// X` beside a type `X` of the impl's own package would be placed at the wrong one. Sure is:
    /// the name declared in the impl's own module or where its path says, a path rooted at a named
    /// workspace crate that reaches one declaration of that crate's package, or a bare name the
    /// workspace declares once and no standard type has. Anything else is an `Err`: a path through
    /// a module name that matches no module (`use other as alias; impl T for alias::X`) is not
    /// placed by package (row 84, N2).
    fn placed_for_sure(&self, named: &Named) -> Result<BTreeSet<Place>, String> {
        let name = &named.name;
        if matches!(named.krate.as_deref(), Some("std" | "core" | "alloc")) {
            return Ok(BTreeSet::new());
        }
        let decls: &[TypeDecl] = self.declarations.types.get(name).map_or(&[], Vec::as_slice);
        let visible: Vec<Place> = decls
            .iter()
            .filter(|decl| decl.public || decl.place.0 == named.home)
            .map(|decl| decl.place.clone())
            .collect();
        let Some((found, how)) = self.locate(named, &visible) else {
            // No workspace type of that name: a standard or an outside one.
            return Ok(BTreeSet::new());
        };
        // A path rooted at a named workspace crate is in that crate's package for sure; a path
        // through `crate::` or a module name may go through a `use … as` the rules do not read.
        let crate_rooted = named
            .krate
            .as_deref()
            .is_some_and(|krate| krate != "crate" && self.declarations.crates.contains_key(krate));
        let sure = how == How::Exact
            || (crate_rooted && found.len() == 1)
            || (decls.len() == 1 && std_kind(name).is_none());
        if !sure {
            return Err(if std_kind(name).is_some() {
                format!("`{name}` is both a standard type and a workspace type")
            } else {
                format!(
                    "`{name}` is not declared in the impl's own module, and the workspace \
                     declares {} types of that name",
                    decls.len()
                )
            });
        }
        if found.len() > 1 {
            return Err(format!("`{name}` names {} declarations", found.len()));
        }
        Ok(found.into_iter().collect())
    }

    /// Where one of the four is declared: the subject package, at its one declaration.
    fn four_home(&self, owner: &str) -> Home {
        Home {
            packages: BTreeSet::from([self.subject.package.clone()]),
            places: self
                .subject
                .structs
                .get(owner)
                .cloned()
                .into_iter()
                .collect(),
        }
    }

    /// **A method of the traits an associated type is bounded by** — the trait's own declaration
    /// of it, read for its receiver and its return as an implementation's would be — and what the
    /// call reaches: [`Resolver::through_binding`].
    fn bound_method(
        &self,
        param: &str,
        associated: &str,
        method: &str,
    ) -> Result<Option<Method>, String> {
        let Some(traits) = self.declarations.associated_bounds.get(associated) else {
            return Ok(None);
        };
        let callees = self.through_binding(param, associated, method);
        let answers: Vec<Method> = traits
            .iter()
            .filter_map(|bound| {
                self.declarations
                    .trait_declarations
                    .get(&(bound.clone(), method.to_owned()))
            })
            .flatten()
            .map(|decl| {
                let ret = if decl.ret.is_a_parameter() {
                    Ty::unknown(format!("`{associated}::{method}` returns a generic type"))
                } else {
                    decl.ret.clone()
                };
                let class = match decl.receiver {
                    Receiver::Unique if holds_unique_reference(&ret) => Class::Access,
                    Receiver::Unique => Class::Write,
                    Receiver::Shared | Receiver::Value | Receiver::None => Class::Read,
                };
                Method {
                    class,
                    ret,
                    closure: Vec::new(),
                    by_position: closures_by_position(&decl.params),
                    callees: callees.clone(),
                }
            })
            .collect();
        if answers.is_empty() {
            return Ok(None);
        }
        agreement(&answers).map(Some).ok_or_else(|| {
            format!("the bounds of `{associated}` declare `{method}` in ways that disagree")
        })
    }

    /// **What a call of `method` through `param::associated` reaches** (T-CENSUS-FOLLOWS-
    /// ASSOCIATED-TYPE-CALLS): the bound trait's implementation of it on the type the product
    /// impls bind the associated type to.
    ///
    /// - **The binding.** Every product `impl` of a trait that declares an associated type of that
    ///   name binds it; the call is followed only when they all bind one type
    ///   ([`Resolver::same_type`]) — so `B::Device`, bound by `Backend`'s one product impl and by
    ///   `Glass`'s, both to one `GpuContext`, is followed. With no product binding, a binding to
    ///   the impl's own generic, bindings that differ, or bindings the rules cannot compare, the
    ///   call is refused with that reason.
    /// - **The method.** On a value of a generic's associated type the language offers only the
    ///   bound traits' methods, so the callee is the bound trait's `impl` method on that type —
    ///   never the type's own inherent method of the same name, which the window seam's forwards
    ///   call in turn — or, where that impl does not write it, the trait's provided method.
    ///
    /// Associated types and traits are keyed by bare name, as
    /// [`Declarations::associated_bounds`] says.
    fn through_binding(&self, param: &str, associated: &str, method: &str) -> Callees {
        let through = format!("`{param}::{associated}`");
        let declarers = self.declarations.associated_declarers.get(associated);
        let bindings: Vec<&Binding> = self
            .declarations
            .associated_bindings
            .get(associated)
            .into_iter()
            .flatten()
            .filter(|binding| {
                binding.product
                    && declarers.is_some_and(|traits| traits.contains(&binding.trait_name))
            })
            .collect();
        let Some((first, rest)) = bindings.split_first() else {
            return Callees::refused(format!(
                "an associated type no product impl binds, which the census follows to no \
                 implementation: {through}"
            ));
        };
        let generic = bindings
            .iter()
            .find_map(|binding| match self.expand(binding.ty.clone()) {
                Ty::Param(generic) => Some(generic),
                _ => None,
            });
        if let Some(generic) = generic {
            return Callees::refused(format!(
                "an associated type an impl binds to its own generic, which the census follows to \
                 no implementation: {through} (`{generic}`)"
            ));
        }
        for binding in rest {
            match self.same_type(&first.ty, &binding.ty) {
                Ok(true) => {}
                Ok(false) => {
                    return Callees::refused(format!(
                        "an associated type the product impls bind to types that differ: \
                         {through} (`{}`, `{}`)",
                        first.ty.spelling(),
                        binding.ty.spelling()
                    ));
                }
                Err(why) => {
                    return Callees::refused(format!(
                        "an associated type whose bindings the rules do not compare: {through}, \
                         {why}"
                    ));
                }
            }
        }
        let bound = &first.ty;
        let owner = match self.expand(bound.clone()) {
            Ty::Named(named) => named.name,
            other => {
                return Callees::refused(format!(
                    "an associated type bound to a type that is not a path: {through} (`{}`)",
                    other.spelling()
                ));
            }
        };
        let traits = self.declarations.associated_bounds.get(associated);
        let mut found: BTreeSet<Callable> = BTreeSet::new();
        for decl in self
            .declarations
            .methods
            .get(&(owner, method.to_owned()))
            .into_iter()
            .flatten()
        {
            let (Some(callable), Some(trait_name), Some(self_ty)) =
                (&decl.callable, &decl.trait_name, &decl.self_ty)
            else {
                continue;
            };
            if !traits.is_some_and(|traits| traits.contains(trait_name)) {
                continue;
            }
            match self.same_type(self_ty, bound) {
                Ok(true) => {
                    found.insert(callable.clone());
                }
                Ok(false) => {}
                Err(why) => {
                    return Callees::refused(format!(
                        "an associated type whose bound type's impls the rules do not compare: \
                         {through}, {why}"
                    ));
                }
            }
        }
        if found.is_empty() {
            // The impl does not write it: the bound trait's provided method answers.
            found = traits
                .into_iter()
                .flatten()
                .filter_map(|bound| {
                    self.declarations
                        .trait_declarations
                        .get(&(bound.clone(), method.to_owned()))
                })
                .flatten()
                .filter_map(|decl| decl.callable.clone())
                .collect();
        }
        if found.is_empty() {
            return Callees::refused(format!(
                "a method of an associated type's bound trait that neither the bound type's impl \
                 nor the trait writes: {through} (`{method}`)"
            ));
        }
        Callees::items(found)
    }

    /// **Whether two types, each as written where it is, are one type.** The same spelling in the
    /// same module is one type, whatever it names: the items of one module see one scope.
    /// Otherwise a workspace type is its declaration, placed only where the rules are sure
    /// ([`Resolver::placed_for_sure`]), with its arguments compared the same way; a standard type
    /// is its kind; two outside types of one name written differently cannot be told apart and
    /// are an `Err`, as is a type the rules do not read.
    fn same_type(&self, a: &Ty, b: &Ty) -> Result<bool, String> {
        let (a, b) = (self.expand(a.clone()), self.expand(b.clone()));
        if a == b {
            return Ok(true);
        }
        let all = |xs: &[Ty], ys: &[Ty]| -> Result<bool, String> {
            if xs.len() != ys.len() {
                return Ok(false);
            }
            for (x, y) in xs.iter().zip(ys) {
                if !self.same_type(x, y)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        match (&a, &b) {
            (
                Ty::Ref {
                    mutable: first,
                    inner: x,
                },
                Ty::Ref {
                    mutable: second,
                    inner: y,
                },
            ) => Ok(first == second && self.same_type(x, y)?),
            (Ty::Tuple(xs), Ty::Tuple(ys)) => all(xs, ys),
            (Ty::Slice(x), Ty::Slice(y)) => self.same_type(x, y),
            // A place is a module: a type is its name there.
            (Ty::Named(x), Ty::Named(y)) if x.name != y.name => Ok(false),
            (Ty::Named(x), Ty::Named(y)) => {
                let x_places = self.placed_for_sure(x)?;
                let y_places = self.placed_for_sure(y)?;
                match (x_places.is_empty(), y_places.is_empty()) {
                    (false, false) => Ok(x_places == y_places && all(&x.args, &y.args)?),
                    (true, true) if std_kind(&x.name).is_some() => all(&x.args, &y.args),
                    (true, true) => Err(format!(
                        "`{}`, a type outside the workspace, written two ways",
                        x.name
                    )),
                    _ => Ok(false),
                }
            }
            (Ty::Param(_) | Ty::Assoc { .. } | Ty::Traits(_) | Ty::Unknown(_), _)
            | (_, Ty::Param(_) | Ty::Assoc { .. } | Ty::Traits(_) | Ty::Unknown(_)) => {
                Err(format!(
                    "`{}` and `{}`, which the rules do not compare",
                    a.spelling(),
                    b.spelling()
                ))
            }
            _ => Ok(false),
        }
    }

    /// **Where a call goes on when only other types of the receiver's name declare the method**
    /// (T-CENSUS-FOLLOWS-ASSOCIATED-TYPE-CALLS, N3): the receiver's type declares no such method,
    /// so the language derefs it — to its `Deref` target, when the type has one. `None` keeps the
    /// answer and its refusal: any other answer, and a type with no target (or targets that
    /// disagree, which the refusal already lists).
    fn past_other_types(&self, found: &Method, name: &str, home: &Home, args: &[Ty]) -> Option<Ty> {
        if !found.callees.others_only {
            return None;
        }
        self.deref_target(name, &home.packages, args).ok().flatten()
    }

    /// Whether a workspace trait's product impl on a type outside the workspace declares
    /// `function` (row 84, N1): an `impl` filed under the type's name whose self type is no
    /// workspace declaration.
    fn outside_type_implements(&self, type_name: &str, function: &str) -> bool {
        self.declarations
            .methods
            .get(&(type_name.to_owned(), function.to_owned()))
            .into_iter()
            .flatten()
            .any(|decl| {
                decl.callable
                    .as_ref()
                    .is_some_and(|callable| callable.function.starts_with('<'))
                    && decl.self_ty.as_ref().is_some_and(|ty| {
                        self.self_type_places(ty)
                            .is_ok_and(|places| places.is_empty())
                    })
            })
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
                Ty::Assoc { param, name } => {
                    return self
                        .bound_method(&param, &name, method)
                        .map_err(|why| fail(why, shared))?
                        .map(|found| answer(found, false, shared))
                        .ok_or_else(|| {
                            fail(
                                format!(
                                    "`{method}` on `{param}::{name}`, whose bounds declare no such \
                                     method"
                                ),
                                shared,
                            )
                        });
                }
                Ty::Traits(_) => return Err(fail("a trait object".to_owned(), shared)),
                Ty::Unknown(why) => return Err(fail(why, shared)),
                Ty::Named(named) => match self.kind(&named) {
                    Kind::Four(owner) => {
                        let home = self.four_home(&owner);
                        let found = self
                            .workspace_methods(&owner, method, Some(&home), &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            match self.past_other_types(&found, &owner, &home, &named.args) {
                                Some(target) => {
                                    ty = target;
                                    continue;
                                }
                                None => return Ok(answer(found, true, shared)),
                            }
                        }
                        match self
                            .deref_target(&owner, &home.packages, &named.args)
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
                    Kind::Workspace {
                        name,
                        decls,
                        packages,
                    } => {
                        let home = Home {
                            packages,
                            places: decls.iter().map(|decl| decl.place.clone()).collect(),
                        };
                        let found = self
                            .workspace_methods(&name, method, Some(&home), &named.args)
                            .map_err(|why| fail(why, shared))?;
                        if let Some(found) = found {
                            match self.past_other_types(&found, &name, &home, &named.args) {
                                Some(target) => {
                                    ty = target;
                                    continue;
                                }
                                None => return Ok(answer(found, false, shared)),
                            }
                        }
                        match self
                            .deref_target(&name, &home.packages, &named.args)
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

    /// `Type::function(..)`: what it returns, what closures in each
    /// argument position receive, and the product items the call reaches.
    /// `owner` is the type the path names.
    pub(crate) fn associated(&self, owner: &Ty, function: &str) -> Associated {
        let named = match self.expand(owner.clone()) {
            Ty::Named(named) => named,
            other => {
                let unresolved = match &other {
                    Ty::Param(param) => format!(
                        "a call through a generic's trait, which the census follows to no \
                         implementation: `{param}::{function}`"
                    ),
                    Ty::Assoc { param, name } => match self.bound_method(param, name, function) {
                        Ok(Some(found)) => match found.callees.refused {
                            None => {
                                return (
                                    Ty::unknown(format!(
                                        "`{function}` on a type that is not a path"
                                    )),
                                    Vec::new(),
                                    found.callees,
                                );
                            }
                            Some(why) => why,
                        },
                        Ok(None) => format!(
                            "a call through an associated type, whose bounds declare no such \
                             function: `{param}::{name}::{function}`"
                        ),
                        Err(why) => {
                            format!(
                                "a call through an associated type, whose bounds disagree: {why}"
                            )
                        }
                    },
                    Ty::Unknown(why) => format!(
                        "a call by a type's path, whose function the rules do not resolve: {why}"
                    ),
                    _ => format!(
                        "a call by a type's path, whose function the rules do not resolve: `{function}` on {}",
                        other.spelling()
                    ),
                };
                return (
                    Ty::unknown(format!("`{function}` on a type that is not a path")),
                    Vec::new(),
                    Callees::unresolved(unresolved),
                );
            }
        };
        let home = match self.kind(&named) {
            Kind::Four(owner) => Some(self.four_home(&owner)),
            Kind::Workspace {
                decls, packages, ..
            } => Some(Home {
                packages,
                places: decls.iter().map(|decl| decl.place.clone()).collect(),
            }),
            Kind::Std(_) => None,
            Kind::External(name) => {
                if builds_itself(function) {
                    return (Ty::Named(named), Vec::new(), Callees::default());
                }
                if self.outside_type_implements(&named.name, function) {
                    // The language picks the outside type's own `f` before a trait's, and the
                    // census does not read the outside type: it cannot tell which this is.
                    return (
                        Ty::unknown(format!("`{name}::{function}`")),
                        Vec::new(),
                        Callees::unresolved(format!(
                            "a type's path on a type outside the workspace, whose own functions \
                             the census does not read: `{name}::{function}`, which a workspace \
                             trait implements on it"
                        )),
                    );
                }
                // A module's path is spelled like a type's: what it names is the free
                // functions' rules to say.
                return (
                    Ty::unknown(format!("`{name}::{function}`")),
                    Vec::new(),
                    Callees::default(),
                );
            }
            Kind::Ambiguous(why) => {
                if builds_itself(function) {
                    return (Ty::Named(named), Vec::new(), Callees::default());
                }
                return (
                    Ty::unknown(format!("`{}::{function}`", named.name)),
                    Vec::new(),
                    Callees::unresolved(format!(
                        "a call by a type's path, whose function the rules do not resolve: {why}"
                    )),
                );
            }
        };
        // A standard type's own functions are the lists' (none reaches a product item); a
        // workspace extension trait implemented for it is the workspace's, as for a method call.
        let workspace = home.is_some();
        let found = match home {
            Some(home) => self.workspace_methods(&named.name, function, Some(&home), &named.args),
            None => self.workspace_methods(&named.name, function, None, &named.args),
        };
        match found {
            Ok(Some(found)) => {
                let mut by_position = found.by_position;
                if self.has_receiver(&named.name, function) {
                    by_position.insert(0, None);
                }
                (found.ret, by_position, found.callees)
            }
            Ok(None) if builds_itself(function) => {
                (Ty::Named(named), Vec::new(), Callees::default())
            }
            Ok(None) if !workspace => (
                Ty::unknown(format!("`{}::{function}` is not declared", named.name)),
                Vec::new(),
                Callees::default(),
            ),
            Ok(None) => (
                Ty::unknown(format!("`{}::{function}` is not declared", named.name)),
                Vec::new(),
                Callees::unresolved(format!(
                    "a call by a type's path, whose function the rules do not resolve: `{}::{function}` \
                     is not declared",
                    named.name
                )),
            ),
            Err(why) => (
                Ty::unknown(why.clone()),
                Vec::new(),
                Callees::unresolved(format!(
                    "a call by a type's path, whose function the rules do not resolve: {why}"
                )),
            ),
        }
    }

    // ── a free function's path, to the items it names ─────────────────────

    /// Whether `package` has a module at the absolute path `module`.
    fn is_module(&self, package: &str, module: &str) -> bool {
        self.declarations
            .imports
            .contains_key(&(package.to_owned(), module.to_owned()))
    }

    /// A path written in `module` of `package` (`seats::f`, `crate::a::f`,
    /// `super::f`, `self::f`) as the absolute path it names inside that
    /// package — `crate`, the modules, the last segment. The first segment is
    /// `crate`, `self` or `super`; or a name a `use` item of the module brings
    /// in; or a child module; or a module a glob of the module brings in.
    /// `None` for a path that leaves the package: another crate, the
    /// standard library, or a first segment that is no module of the package
    /// (a type the census did not place, an external crate).
    pub(crate) fn absolute(
        &self,
        package: &str,
        module: &str,
        path: &[String],
    ) -> Option<Vec<String>> {
        self.absolute_at(package, module, path, 0)
    }

    fn absolute_at(
        &self,
        package: &str,
        module: &str,
        path: &[String],
        depth: usize,
    ) -> Option<Vec<String>> {
        if depth > PATH_STEPS {
            return None;
        }
        let (first, rest) = path.split_first()?;
        let mut here: Vec<String> = module.split("::").map(str::to_owned).collect();
        match first.as_str() {
            "crate" => Some(path.to_vec()),
            "self" => {
                here.extend(rest.iter().cloned());
                Some(here)
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
                Some(here)
            }
            _ => {
                let imports = self
                    .declarations
                    .imports
                    .get(&(package.to_owned(), module.to_owned()));
                if let Some(imported) = imports.and_then(|imports| imports.names.get(first)) {
                    let mut expanded = self.absolute_at(package, module, imported, depth + 1)?;
                    expanded.extend(rest.iter().cloned());
                    return Some(expanded);
                }
                if self.declarations.crates.contains_key(first)
                    || matches!(first.as_str(), "std" | "core" | "alloc")
                {
                    return None;
                }
                let child = format!("{module}::{first}");
                if self.is_module(package, &child) {
                    here.extend(path.iter().cloned());
                    return Some(here);
                }
                for glob in imports
                    .map(|imports| imports.globs.as_slice())
                    .unwrap_or_default()
                {
                    let Some(mut opened) = self.absolute_at(package, module, glob, depth + 1)
                    else {
                        continue;
                    };
                    opened.push(first.clone());
                    if self.is_module(package, &opened.join("::")) {
                        opened.extend(rest.iter().cloned());
                        return Some(opened);
                    }
                }
                None
            }
        }
    }

    /// What the absolute path `module::name` names among `package`'s free
    /// functions: the product items declared in that module, or brought into
    /// it by a `use` item or a glob, followed to the declaration.
    pub(crate) fn functions_at(&self, package: &str, module: &str, name: &str) -> Reach {
        let mut reach = Reach::default();
        self.functions_into(package, module, name, 0, &mut reach);
        reach
    }

    fn functions_into(
        &self,
        package: &str,
        module: &str,
        name: &str,
        depth: usize,
        reach: &mut Reach,
    ) {
        if depth > PATH_STEPS {
            return;
        }
        let declared: Vec<&FnDecl> = self
            .declarations
            .functions
            .get(name)
            .into_iter()
            .flatten()
            .filter(|decl| decl.place.0 == package && decl.place.1 == module)
            .collect();
        if !declared.is_empty() {
            let items: Vec<&Callable> = declared
                .iter()
                .filter_map(|decl| decl.callable.as_ref())
                .collect();
            if items.is_empty() {
                reach.test_only = true;
            }
            reach.items.extend(items.into_iter().cloned());
            return;
        }
        let Some(imports) = self
            .declarations
            .imports
            .get(&(package.to_owned(), module.to_owned()))
        else {
            return;
        };
        if let Some(path) = imports.names.get(name) {
            match self.absolute_at(package, module, path, depth + 1) {
                Some(absolute) => {
                    if let Some((last, at)) = absolute.split_last() {
                        self.functions_into(package, &at.join("::"), last, depth + 1, reach);
                    }
                }
                None => reach.outside = true,
            }
            return;
        }
        for glob in &imports.globs {
            if let Some(absolute) = self.absolute_at(package, module, glob, depth + 1) {
                self.functions_into(package, &absolute.join("::"), name, depth + 1, reach);
            }
        }
    }
}

/// What a free function's path reaches.
#[derive(Debug, Default)]
pub(crate) struct Reach {
    /// The product items it names.
    pub(crate) items: BTreeSet<Callable>,
    /// A `use` item takes the name out of the package.
    pub(crate) outside: bool,
    /// It names a declaration only a test build has.
    pub(crate) test_only: bool,
}

/// How many `use` steps a path is followed through before the rules stop:
/// a re-export of a re-export is real, a cycle of globs is not followed.
const PATH_STEPS: usize = 8;

/// What `Type::function(..)` returns, what closures in each argument
/// position receive, and the product items a call of it reaches.
pub(crate) type Associated = (Ty, Vec<Option<Vec<Ty>>>, Callees);

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
        Ty::Fn(_) | Ty::Param(_) | Ty::Assoc { .. } | Ty::Traits(_) | Ty::Unknown(_) => false,
    }
}
