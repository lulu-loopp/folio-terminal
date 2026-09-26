//! The census's types: what a field, a binding or an expression is, as far as
//! the stated rules of the census note's revision (b)2 §3 can say.
//!
//! **This is not type inference.** `syn` gives syntax, and [`crate::Index`]
//! gives item identity; neither knows what `x` is in `let x = f(y);`. What
//! this module does is smaller and says so: a type is taken from a
//! *declaration* — a struct's field, an enum variant's field, a parameter, a
//! `let` with a written type, a type alias, a function's or a method's written
//! return type — and carried through a bounded set of steps (a field, an
//! index, a deref, a method whose declaration is found, a pattern). Anything
//! the steps do not reach is [`Ty::Unknown`], with the reason, and a site whose
//! receiver is unknown is listed as unknown by the walk rather than guessed.
//!
//! Three sources of declarations, and nothing else:
//!
//! * **The workspace** — every `struct`, `enum`, `union` and type alias, every
//!   method with its receiver and return type, every free function, every
//!   `Deref` impl, read from the [`crate::Index`]es the caller hands in.
//! * **A fixed list of standard types**, keyed by type ([`Std`]): which of
//!   their methods write, which grant mutable access, which read. A method of a
//!   standard type that is not on the list is unresolved.
//! * **The signatures of the standard prelude traits** (`Clone::clone`,
//!   `PartialEq::eq`, `Extend::extend`, …), which hold for every type — used
//!   only when the type itself declares no method of that name and no
//!   workspace trait declares one either.
//!
//! **Which declaration a name means.** A name written in module `m` means the
//! type of that name declared in `m`, when there is one — the language allows
//! no other, since a `use` of a second type of the same name there would not
//! compile. A module-qualified path (`seats::Seats`) means the declaration in a
//! module whose path ends that way. Otherwise the package the name is written
//! in is preferred, then the whole workspace, and a question asked of several
//! declarations is answered only where they agree.

use std::collections::{BTreeMap, BTreeSet};

use crate::index::{ImplRecord, Index, ItemKind, ItemRecord, TokenKind};

/// A type, as far as a declaration says.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Ty {
    Named(Named),
    Ref {
        mutable: bool,
        inner: Box<Ty>,
    },
    Tuple(Vec<Ty>),
    /// `[T]` and `[T; N]`.
    Slice(Box<Ty>),
    /// An iterator, by the type of its item: produced by a standard method on
    /// the fixed list, or written `impl Iterator<Item = T>`.
    Iter(Box<Ty>),
    /// A standard map's `entry(..)`, by the map's value type.
    Entry(Box<Ty>),
    /// Something callable, by its parameters: `impl FnMut(&mut X)`, or a
    /// generic parameter bounded that way — what a closure handed to it gets.
    Fn(Vec<Ty>),
    /// A generic parameter no substitution reached.
    Param(String),
    Unknown(String),
}

/// A path type: its last segment, its generic type arguments, and what is
/// needed to say which declaration it means.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Named {
    pub(crate) name: String,
    /// The first segment when it names a crate (`bt_render`, `std`, `crate`).
    pub(crate) krate: Option<String>,
    /// The module segments between the crate (if any) and the name.
    pub(crate) prefix: Vec<String>,
    pub(crate) args: Vec<Ty>,
    /// The package and module the path is written in.
    pub(crate) home: String,
    pub(crate) module: String,
}

impl Ty {
    pub(crate) fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown(reason.into())
    }

    pub(crate) fn reference(mutable: bool, inner: Self) -> Self {
        Self::Ref {
            mutable,
            inner: Box::new(inner),
        }
    }

    /// Whether a value of this type can carry a reference into the place it
    /// came from — the condition for a binding to keep its origin.
    pub(crate) fn carries_reference(&self) -> bool {
        match self {
            Self::Ref { .. } | Self::Entry(_) => true,
            Self::Iter(item) => item.carries_reference(),
            Self::Tuple(items) => items.iter().any(Self::carries_reference),
            Self::Named(named) => named.args.iter().any(Self::carries_reference),
            Self::Slice(_) | Self::Fn(_) | Self::Param(_) | Self::Unknown(_) => false,
        }
    }

    /// Whether the type spells one of `names` anywhere in it — how a hub is
    /// told: a field whose type holds one of the census's own structs.
    pub(crate) fn mentions(&self, names: &BTreeSet<String>) -> bool {
        match self {
            Self::Named(named) => {
                names.contains(&named.name) || named.args.iter().any(|arg| arg.mentions(names))
            }
            Self::Ref { inner, .. }
            | Self::Slice(inner)
            | Self::Iter(inner)
            | Self::Entry(inner) => inner.mentions(names),
            Self::Tuple(items) | Self::Fn(items) => items.iter().any(|item| item.mentions(names)),
            Self::Param(_) | Self::Unknown(_) => false,
        }
    }

    /// Replace generic parameters by the arguments a use site gave them.
    pub(crate) fn substitute(&self, map: &BTreeMap<String, Self>) -> Self {
        match self {
            Self::Param(name) => map.get(name).cloned().unwrap_or_else(|| self.clone()),
            Self::Named(named) => Self::Named(Named {
                args: named.args.iter().map(|arg| arg.substitute(map)).collect(),
                ..named.clone()
            }),
            Self::Ref { mutable, inner } => Self::reference(*mutable, inner.substitute(map)),
            Self::Tuple(items) => Self::Tuple(items.iter().map(|it| it.substitute(map)).collect()),
            Self::Fn(items) => Self::Fn(items.iter().map(|it| it.substitute(map)).collect()),
            Self::Slice(inner) => Self::Slice(Box::new(inner.substitute(map))),
            Self::Iter(inner) => Self::Iter(Box::new(inner.substitute(map))),
            Self::Entry(inner) => Self::Entry(Box::new(inner.substitute(map))),
            Self::Unknown(_) => self.clone(),
        }
    }

    /// Whether any generic parameter is left in it.
    pub(crate) fn is_open(&self) -> bool {
        match self {
            Self::Param(_) => true,
            Self::Named(named) => named.args.iter().any(Self::is_open),
            Self::Ref { inner, .. }
            | Self::Slice(inner)
            | Self::Iter(inner)
            | Self::Entry(inner) => inner.is_open(),
            Self::Tuple(items) | Self::Fn(items) => items.iter().any(Self::is_open),
            Self::Unknown(_) => false,
        }
    }

    /// The written form, for the inventory and for a reason in an unknown row.
    pub(crate) fn spelling(&self) -> String {
        let list = |items: &[Self]| -> String {
            items
                .iter()
                .map(Self::spelling)
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            Self::Named(named) => {
                let mut out = named.name.clone();
                if !named.args.is_empty() {
                    out.push('<');
                    out.push_str(&list(&named.args));
                    out.push('>');
                }
                out
            }
            Self::Ref { mutable, inner } => {
                format!(
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    inner.spelling()
                )
            }
            Self::Tuple(items) => format!("({})", list(items)),
            Self::Fn(items) => format!("impl Fn({})", list(items)),
            Self::Slice(inner) => format!("[{}]", inner.spelling()),
            Self::Iter(inner) => format!("impl Iterator<Item = {}>", inner.spelling()),
            Self::Entry(inner) => format!("Entry<{}>", inner.spelling()),
            Self::Param(name) => name.clone(),
            Self::Unknown(_) => "_".to_owned(),
        }
    }
}

/// The context a type is written in: the package and module, the generic
/// parameters in scope (with the bounds that type them), and what `Self` is.
#[derive(Clone, Debug, Default)]
pub(crate) struct Written {
    pub(crate) package: String,
    pub(crate) module: String,
    pub(crate) params: BTreeSet<String>,
    /// A generic parameter bounded by `Iterator<Item = T>` or `FnMut(A)`,
    /// read as that bound.
    pub(crate) bounds: BTreeMap<String, Ty>,
    pub(crate) self_ty: Option<Ty>,
    /// Crate names of the workspace, so a path's first segment can be told as
    /// a crate or a module.
    pub(crate) crates: BTreeSet<String>,
}

impl Written {
    /// Bring a `Generics` into scope: its type parameters, and the bounds of
    /// the ones an iterator or a closure bound types.
    pub(crate) fn enter(&mut self, generics: &syn::Generics) {
        for param in &generics.params {
            if let syn::GenericParam::Type(ty) = param {
                self.params.insert(ty.ident.to_string());
            }
        }
        for param in &generics.params {
            if let syn::GenericParam::Type(ty) = param
                && let Some(bound) = self.bound_of(ty.bounds.iter())
            {
                self.bounds.insert(ty.ident.to_string(), bound);
            }
        }
        if let Some(clause) = &generics.where_clause {
            for predicate in &clause.predicates {
                if let syn::WherePredicate::Type(typed) = predicate
                    && let syn::Type::Path(path) = &typed.bounded_ty
                    && let Some(name) = path.path.get_ident()
                    && let Some(bound) = self.bound_of(typed.bounds.iter())
                {
                    self.bounds.insert(name.to_string(), bound);
                }
            }
        }
    }

    fn bound_of<'b>(&self, bounds: impl Iterator<Item = &'b syn::TypeParamBound>) -> Option<Ty> {
        for bound in bounds {
            let syn::TypeParamBound::Trait(bound) = bound else {
                continue;
            };
            let Some(last) = bound.path.segments.last() else {
                continue;
            };
            match (last.ident.to_string().as_str(), &last.arguments) {
                (
                    "Iterator" | "DoubleEndedIterator" | "ExactSizeIterator" | "IntoIterator",
                    syn::PathArguments::AngleBracketed(arguments),
                ) => {
                    for argument in &arguments.args {
                        if let syn::GenericArgument::AssocType(item) = argument
                            && item.ident == "Item"
                        {
                            return Some(Ty::Iter(Box::new(self.ty(&item.ty))));
                        }
                    }
                }
                ("Fn" | "FnMut" | "FnOnce", syn::PathArguments::Parenthesized(arguments)) => {
                    return Some(Ty::Fn(
                        arguments
                            .inputs
                            .iter()
                            .map(|input| self.ty(input))
                            .collect(),
                    ));
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn ty(&self, written: &syn::Type) -> Ty {
        match written {
            syn::Type::Reference(reference) => {
                Ty::reference(reference.mutability.is_some(), self.ty(&reference.elem))
            }
            syn::Type::Paren(inner) => self.ty(&inner.elem),
            syn::Type::Group(inner) => self.ty(&inner.elem),
            syn::Type::Tuple(tuple) => {
                Ty::Tuple(tuple.elems.iter().map(|it| self.ty(it)).collect())
            }
            syn::Type::Slice(slice) => Ty::Slice(Box::new(self.ty(&slice.elem))),
            syn::Type::Array(array) => Ty::Slice(Box::new(self.ty(&array.elem))),
            syn::Type::Path(path) if path.qself.is_none() => self.path(&path.path),
            syn::Type::Path(_) => Ty::unknown("a qualified associated type"),
            syn::Type::ImplTrait(bounds) => self
                .bound_of(bounds.bounds.iter())
                .unwrap_or_else(|| Ty::unknown("an `impl Trait` type")),
            syn::Type::TraitObject(bounds) => self
                .bound_of(bounds.bounds.iter())
                .unwrap_or_else(|| Ty::unknown("a trait object")),
            syn::Type::BareFn(function) => Ty::Fn(
                function
                    .inputs
                    .iter()
                    .map(|input| self.ty(&input.ty))
                    .collect(),
            ),
            syn::Type::Ptr(_) => Ty::unknown("a raw pointer"),
            syn::Type::Never(_) => Ty::unknown("the never type"),
            _ => Ty::unknown("a type this census does not read"),
        }
    }

    pub(crate) fn path(&self, path: &syn::Path) -> Ty {
        let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
        let Some(last) = segments.last() else {
            return Ty::unknown("an empty path");
        };
        let name = last.ident.to_string();
        if segments.len() == 1 {
            if name == "Self" {
                return self
                    .self_ty
                    .clone()
                    .unwrap_or_else(|| Ty::unknown("`Self` outside an impl"));
            }
            if let Some(bound) = self.bounds.get(&name) {
                return bound.clone();
            }
            if self.params.contains(&name) {
                return Ty::Param(name);
            }
        }
        if segments.len() > 1 && segments[segments.len() - 2].ident == "Self" {
            return Ty::unknown("an associated type of `Self`");
        }
        let mut leading: Vec<String> = segments[..segments.len() - 1]
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let krate = match leading.first() {
            Some(first)
                if matches!(first.as_str(), "crate" | "std" | "core" | "alloc")
                    || self.crates.contains(first) =>
            {
                Some(leading.remove(0))
            }
            _ => None,
        };
        let args = match &last.arguments {
            syn::PathArguments::AngleBracketed(arguments) => arguments
                .args
                .iter()
                .filter_map(|argument| match argument {
                    syn::GenericArgument::Type(ty) => Some(self.ty(ty)),
                    _ => None,
                })
                .collect(),
            syn::PathArguments::Parenthesized(_) => {
                return Ty::unknown("a closure trait written as a path");
            }
            syn::PathArguments::None => Vec::new(),
        };
        Ty::Named(Named {
            name,
            krate,
            prefix: leading,
            args,
            home: self.package.clone(),
            module: self.module.clone(),
        })
    }
}

/// A method's receiver, as its declaration writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Receiver {
    Shared,
    Unique,
    Value,
    /// An associated function: no `self`.
    None,
}

impl Receiver {
    pub(crate) fn of(signature: &syn::Signature) -> Self {
        match signature.inputs.first() {
            Some(syn::FnArg::Receiver(receiver)) => {
                if receiver.colon_token.is_some() {
                    // `self: &mut Self`, `self: Box<Self>` — the written type.
                    match &*receiver.ty {
                        syn::Type::Reference(reference) if reference.mutability.is_some() => {
                            Self::Unique
                        }
                        syn::Type::Reference(_) => Self::Shared,
                        _ => Self::Value,
                    }
                } else {
                    match &receiver.reference {
                        Some(_) if receiver.mutability.is_some() => Self::Unique,
                        Some(_) => Self::Shared,
                        None => Self::Value,
                    }
                }
            }
            _ => Self::None,
        }
    }
}

/// Where a declaration is: its package and module.
pub(crate) type Place = (String, String);

/// One `struct` or `union`, or one `enum` with its variants' fields.
#[derive(Clone, Debug)]
pub(crate) struct TypeDecl {
    pub(crate) place: Place,
    pub(crate) generics: Vec<String>,
    /// A struct's fields; empty for an enum.
    pub(crate) fields: Vec<(String, Ty)>,
    /// An enum's variants, each with its fields; empty for a struct.
    pub(crate) variants: BTreeMap<String, Vec<(String, Ty)>>,
    pub(crate) is_enum: bool,
    /// Written `pub`: reachable from another crate.
    pub(crate) public: bool,
}

/// One type alias.
#[derive(Clone, Debug)]
pub(crate) struct AliasDecl {
    pub(crate) place: Place,
    pub(crate) generics: Vec<String>,
    pub(crate) target: Ty,
    pub(crate) public: bool,
}

/// One method of the workspace.
#[derive(Clone, Debug)]
pub(crate) struct MethodDecl {
    pub(crate) package: String,
    pub(crate) receiver: Receiver,
    pub(crate) ret: Ty,
    /// The `impl` block's generic parameters and the arguments its self type
    /// gives them, to substitute a use site's arguments.
    pub(crate) impl_params: Vec<String>,
    pub(crate) self_args: Vec<Ty>,
    pub(crate) params: Vec<Ty>,
}

/// One free function of the workspace.
#[derive(Clone, Debug)]
pub(crate) struct FnDecl {
    pub(crate) package: String,
    pub(crate) generics: Vec<String>,
    pub(crate) params: Vec<Ty>,
    pub(crate) ret: Ty,
}

/// One `impl Deref for X`.
#[derive(Clone, Debug)]
pub(crate) struct DerefDecl {
    pub(crate) package: String,
    pub(crate) impl_params: Vec<String>,
    pub(crate) self_args: Vec<Ty>,
    pub(crate) target: Ty,
}

/// Everything the workspace declares that a census step can stand on.
#[derive(Debug, Default)]
pub(crate) struct Declarations {
    pub(crate) types: BTreeMap<String, Vec<TypeDecl>>,
    pub(crate) aliases: BTreeMap<String, Vec<AliasDecl>>,
    pub(crate) methods: BTreeMap<(String, String), Vec<MethodDecl>>,
    pub(crate) functions: BTreeMap<String, Vec<FnDecl>>,
    pub(crate) derefs: BTreeMap<String, Vec<DerefDecl>>,
    /// Every method name a workspace trait declares — the reason a prelude
    /// trait's signature is not trusted for that name.
    pub(crate) trait_methods: BTreeSet<String>,
    /// Crate (library target) names, mapped to their packages.
    pub(crate) crates: BTreeMap<String, String>,
    /// Declarations that did not parse: an answer this census would otherwise
    /// take without them.
    pub(crate) unparsed: Vec<String>,
}

/// The package a record belongs to: the first owner of its file.
pub(crate) fn package_of(index: &Index, item: &ItemRecord) -> String {
    package_at(index, item.whole().start())
}

fn package_at(index: &Index, offset: usize) -> String {
    index
        .file_at(offset)
        .and_then(|file| file.owners().first())
        .map_or_else(String::new, |owner| owner.target.package.clone())
}

/// The module a byte is written in: the innermost module holding it.
pub(crate) fn module_at(index: &Index, offset: usize) -> String {
    index
        .modules()
        .iter()
        .filter(|module| module.span().holds(offset))
        .min_by_key(|module| module.span().len())
        .and_then(|module| module.module_paths().first().cloned())
        .unwrap_or_else(|| "crate".to_owned())
}

fn generic_names(generics: &syn::Generics) -> Vec<String> {
    generics
        .params
        .iter()
        .filter_map(|param| match param {
            syn::GenericParam::Type(ty) => Some(ty.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// The written type arguments of an `impl`'s self type.
fn self_arguments(written: &Written, self_ty: &syn::Type) -> Vec<Ty> {
    match written.ty(self_ty) {
        Ty::Named(named) => named.args,
        _ => Vec::new(),
    }
}

/// An `impl` block's header, parsed: the block with its body emptied.
pub(crate) fn impl_header(index: &Index, block: &ImplRecord) -> Option<syn::ItemImpl> {
    let head = &index.union()[block.whole().start()..block.body().start()];
    syn::parse_str::<syn::ItemImpl>(&format!("{head}{{}}")).ok()
}

/// The innermost `impl` block holding an item, by span.
pub(crate) fn impl_of<'i>(index: &'i Index, item: &ItemRecord) -> Option<&'i ImplRecord> {
    let start = item.whole().start();
    let after = index
        .impls()
        .partition_point(|block| block.body().start() <= start);
    index.impls()[..after]
        .iter()
        .rev()
        .find(|block| block.body().start() <= start && item.whole().end() <= block.body().end())
}

impl Declarations {
    /// Read every declaration of every index handed in.
    pub(crate) fn read(indexes: &[&Index]) -> Self {
        let mut declarations = Self::default();
        for index in indexes {
            for root in index.universe().roots() {
                if root.id.kind == crate::TargetKind::Library {
                    declarations
                        .crates
                        .insert(root.id.name.replace('-', "_"), root.id.package.clone());
                }
            }
        }
        let crates: BTreeSet<String> = declarations.crates.keys().cloned().collect();
        for index in indexes {
            declarations.read_one(index, &crates);
        }
        declarations
    }

    fn written(&self, package: String, module: String, crates: &BTreeSet<String>) -> Written {
        Written {
            package,
            module,
            crates: crates.clone(),
            ..Written::default()
        }
    }

    fn read_one(&mut self, index: &Index, crates: &BTreeSet<String>) {
        self.read_derefs(index, crates);
        self.read_aliases(index, crates);
        for item in index.items() {
            let package = package_of(index, item);
            let module = item
                .module_paths()
                .first()
                .map_or_else(|| "crate".to_owned(), |path| (*path).to_owned());
            match item.kind() {
                ItemKind::Struct | ItemKind::Enum | ItemKind::Union => {
                    self.read_type(index, item, (package, module), crates);
                }
                ItemKind::AssociatedFunction => {
                    self.read_method(index, item, package, module, crates)
                }
                ItemKind::Function => self.read_function(index, item, package, module, crates),
                ItemKind::Field | ItemKind::Variant => {}
            }
        }
    }

    fn read_derefs(&mut self, index: &Index, crates: &BTreeSet<String>) {
        for block in index.impls() {
            let is_deref = block.trait_name().is_some_and(|name| {
                name.rsplit("::")
                    .next()
                    .is_some_and(|last| last.trim() == "Deref")
            });
            if !is_deref {
                continue;
            }
            let package = package_at(index, block.whole().start());
            let module = module_at(index, block.whole().start());
            let Ok(parsed) = syn::parse_str::<syn::ItemImpl>(index.text(block.whole())) else {
                self.unparsed
                    .push(format!("impl Deref for {}", block.type_owner()));
                continue;
            };
            let mut written = self.written(package.clone(), module, crates);
            written.enter(&parsed.generics);
            let target = parsed.items.iter().find_map(|member| match member {
                syn::ImplItem::Type(target) if target.ident == "Target" => {
                    Some(written.ty(&target.ty))
                }
                _ => None,
            });
            if let Some(target) = target {
                self.derefs
                    .entry(block.type_owner().to_owned())
                    .or_default()
                    .push(DerefDecl {
                        package,
                        impl_params: generic_names(&parsed.generics),
                        self_args: self_arguments(&written, &parsed.self_ty),
                        target,
                    });
            }
        }
    }

    /// Type aliases are not items of the index, so they are found by their
    /// keyword: every `type` token outside an `impl` block and outside a
    /// function body, parsed up to its `;`. One that does not parse as an
    /// alias (a trait's associated type, a token inside a macro's rules) is
    /// not one.
    fn read_aliases(&mut self, index: &Index, crates: &BTreeSet<String>) {
        let bodies: Vec<(usize, usize)> = index
            .items()
            .iter()
            .filter(|item| item.kind().is_callable())
            .filter_map(|item| item.body().map(|body| (body.start(), body.end())))
            .collect();
        let union = index.union();
        let tokens = index.tokens();
        for (at, token) in tokens.iter().enumerate() {
            if token.kind() != TokenKind::Identifier || index.text(token.span()) != "type" {
                continue;
            }
            // `pub type X = ..;`: the token before is `pub` with nothing but
            // spacing between. `pub(crate)` puts `crate` there.
            let public = at
                .checked_sub(1)
                .and_then(|before| tokens.get(before))
                .is_some_and(|before| {
                    index.text(before.span()) == "pub"
                        && union[before.span().end()..token.span().start()]
                            .trim()
                            .is_empty()
                });
            let start = token.span().start();
            let inside_impl = index.impls().iter().any(|block| block.body().holds(start));
            let inside_body = bodies
                .iter()
                .any(|(from, to)| (*from..*to).contains(&start));
            if inside_impl || inside_body {
                continue;
            }
            let Some(end) = statement_end(union, start) else {
                continue;
            };
            let Ok(parsed) = syn::parse_str::<syn::ItemType>(&union[start..=end]) else {
                continue;
            };
            let package = package_at(index, start);
            let module = module_at(index, start);
            let mut written = self.written(package.clone(), module.clone(), crates);
            written.enter(&parsed.generics);
            self.aliases
                .entry(parsed.ident.to_string())
                .or_default()
                .push(AliasDecl {
                    place: (package, module),
                    generics: generic_names(&parsed.generics),
                    target: written.ty(&parsed.ty),
                    public,
                });
        }
    }

    fn read_type(
        &mut self,
        index: &Index,
        item: &ItemRecord,
        place: Place,
        crates: &BTreeSet<String>,
    ) {
        let text = index.text(item.whole());
        let parsed: syn::Result<syn::Item> = syn::parse_str(text);
        let Ok(parsed) = parsed else {
            self.unparsed.push(format!("type {}", item.name()));
            return;
        };
        let (public, generics, fields, variants, is_enum) = match parsed {
            syn::Item::Struct(parsed) => (
                matches!(parsed.vis, syn::Visibility::Public(_)),
                parsed.generics,
                parsed.fields.into_iter().collect(),
                Vec::new(),
                false,
            ),
            syn::Item::Union(parsed) => (
                matches!(parsed.vis, syn::Visibility::Public(_)),
                parsed.generics,
                parsed.fields.named.into_iter().collect(),
                Vec::new(),
                false,
            ),
            syn::Item::Enum(parsed) => (
                matches!(parsed.vis, syn::Visibility::Public(_)),
                parsed.generics,
                Vec::new(),
                parsed.variants.into_iter().collect(),
                true,
            ),
            _ => {
                self.unparsed.push(format!("type {}", item.name()));
                return;
            }
        };
        let mut written = self.written(place.0.clone(), place.1.clone(), crates);
        written.enter(&generics);
        let read_fields = |fields: &[syn::Field]| -> Vec<(String, Ty)> {
            fields
                .iter()
                .enumerate()
                .map(|(at, field)| {
                    let name = field
                        .ident
                        .as_ref()
                        .map_or_else(|| at.to_string(), ToString::to_string);
                    (name, written.ty(&field.ty))
                })
                .collect()
        };
        let fields = read_fields(&fields);
        let variants = variants
            .iter()
            .map(|variant| {
                let fields: Vec<syn::Field> = variant.fields.iter().cloned().collect();
                (variant.ident.to_string(), read_fields(&fields))
            })
            .collect();
        self.types
            .entry(item.name().to_owned())
            .or_default()
            .push(TypeDecl {
                place,
                generics: generic_names(&generics),
                fields,
                variants,
                is_enum,
                public,
            });
    }

    fn read_method(
        &mut self,
        index: &Index,
        item: &ItemRecord,
        package: String,
        module: String,
        crates: &BTreeSet<String>,
    ) {
        let Some(signature) = signature_of(index, item) else {
            self.unparsed.push(describe(item));
            return;
        };
        let Some(owner) = item.type_owner() else {
            // A trait's own declaration: its name is what the prelude fallback
            // must not answer for.
            self.trait_methods.insert(item.name().to_owned());
            return;
        };
        let mut written = self.written(package.clone(), module, crates);
        let (impl_params, self_args) =
            match impl_of(index, item).and_then(|block| impl_header(index, block)) {
                Some(header) => {
                    written.enter(&header.generics);
                    let self_ty = written.ty(&header.self_ty);
                    let self_args = self_arguments(&written, &header.self_ty);
                    written.self_ty = Some(self_ty);
                    (generic_names(&header.generics), self_args)
                }
                None => (Vec::new(), Vec::new()),
            };
        written.enter(&signature.generics);
        let ret = match &signature.output {
            syn::ReturnType::Default => Ty::Tuple(Vec::new()),
            syn::ReturnType::Type(_, ty) => written.ty(ty),
        };
        let params = typed_inputs(&written, &signature);
        self.methods
            .entry((owner.to_owned(), item.name().to_owned()))
            .or_default()
            .push(MethodDecl {
                package,
                receiver: Receiver::of(&signature),
                ret,
                impl_params,
                self_args,
                params,
            });
    }

    fn read_function(
        &mut self,
        index: &Index,
        item: &ItemRecord,
        package: String,
        module: String,
        crates: &BTreeSet<String>,
    ) {
        let Some(signature) = signature_of(index, item) else {
            self.unparsed.push(describe(item));
            return;
        };
        let mut written = self.written(package.clone(), module, crates);
        written.enter(&signature.generics);
        let ret = match &signature.output {
            syn::ReturnType::Default => Ty::Tuple(Vec::new()),
            syn::ReturnType::Type(_, ty) => written.ty(ty),
        };
        let params = typed_inputs(&written, &signature);
        self.functions
            .entry(item.name().to_owned())
            .or_default()
            .push(FnDecl {
                package,
                generics: generic_names(&signature.generics),
                params,
                ret,
            });
    }
}

fn typed_inputs(written: &Written, signature: &syn::Signature) -> Vec<Ty> {
    signature
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Typed(typed) => Some(written.ty(&typed.ty)),
            syn::FnArg::Receiver(_) => None,
        })
        .collect()
}

/// The `;` that ends the statement beginning at `start`, at bracket depth
/// zero — a `[u8; 4]` inside it is not the end.
fn statement_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (at, byte) in text.as_bytes()[start..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            b';' if depth == 0 => return Some(start + at),
            _ => {}
        }
        if at > 4096 {
            return None;
        }
    }
    None
}

fn describe(item: &ItemRecord) -> String {
    match item.type_owner() {
        Some(owner) => format!("{owner}::{}", item.name()),
        None => item.name().to_owned(),
    }
}

/// A callable's signature, parsed from its declaration alone — the body is
/// not needed to know what a function takes and gives back.
pub(crate) fn signature_of(index: &Index, item: &ItemRecord) -> Option<syn::Signature> {
    let declaration = index.text(item.declaration());
    if item.body().is_some() {
        syn::parse_str::<syn::ImplItemFn>(&format!("{declaration}{{}}"))
            .map(|parsed| parsed.sig)
            .ok()
    } else {
        syn::parse_str::<syn::TraitItemFn>(index.text(item.whole()))
            .map(|parsed| parsed.sig)
            .ok()
    }
}

// ── the standard types, on a fixed list ──────────────────────────────────

/// A standard type the census knows, keyed by what its methods do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Std {
    Vec,
    VecDeque,
    Map,
    Set,
    Option,
    Result,
    String,
    Str,
    PathBuf,
    Path,
    /// `Box`: its own few methods, then the pointee's.
    Box,
    /// `Rc`, `Arc`: shared ownership; the pointee is reached shared.
    Shared,
    Cell,
    RefCell,
    Lock,
    Atomic,
    /// `std::sync::mpsc`'s ends: every method takes `&self`.
    Channel,
    /// A `Copy` value type whose every method reads (`Instant`, `Duration`,
    /// the `NonZero` integers,
    /// the primitives) — a rule keyed by type, with its named exceptions.
    Value,
}

pub(crate) fn std_kind(name: &str) -> Option<Std> {
    Some(match name {
        "Vec" => Std::Vec,
        "VecDeque" => Std::VecDeque,
        "BTreeMap" | "HashMap" => Std::Map,
        "BTreeSet" | "HashSet" => Std::Set,
        "Option" => Std::Option,
        "Result" => Std::Result,
        "String" => Std::String,
        "str" => Std::Str,
        "PathBuf" => Std::PathBuf,
        "Path" => Std::Path,
        "Box" => Std::Box,
        "Rc" | "Arc" => Std::Shared,
        "Cell" => Std::Cell,
        "RefCell" => Std::RefCell,
        "Mutex" | "RwLock" => Std::Lock,
        "AtomicBool" | "AtomicU8" | "AtomicU16" | "AtomicU32" | "AtomicU64" | "AtomicUsize"
        | "AtomicI8" | "AtomicI16" | "AtomicI32" | "AtomicI64" | "AtomicIsize" => Std::Atomic,
        "Sender" | "SyncSender" | "Receiver" => Std::Channel,
        "Instant" | "Duration" | "SystemTime" | "bool" | "char" | "u8" | "u16" | "u32" | "u64"
        | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "f32" | "f64"
        | "NonZeroU8" | "NonZeroU16" | "NonZeroU32" | "NonZeroU64" | "NonZeroUsize"
        | "NonZeroI32" | "NonZeroI64" => Std::Value,
        _ => return None,
    })
}

/// What a method does to the place it is called on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// It writes the receiver (`push`, `insert`, a workspace `&mut self`
    /// method that hands back no `&mut`).
    Write,
    /// It grants mutable access and proves nothing (`get_mut`, `iter_mut`,
    /// `as_mut`, a workspace `&mut self` method that returns `&mut`).
    Access,
    /// It hands back what the receiver holds (`unwrap`, `expect`) — a write
    /// into the result is a write into the receiver.
    Pass,
    /// It reads.
    Read,
    /// It changes the receiver through a shared reference (`Cell::set`,
    /// `RefCell::borrow_mut`, `AtomicBool::store`).
    Inner,
}

/// A method's answer: what it does, what it returns, and what the closures
/// handed to it receive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Method {
    pub(crate) class: Class,
    pub(crate) ret: Ty,
    /// What any closure argument receives (the standard lists).
    pub(crate) closure: Vec<Ty>,
    /// What a closure in each argument position receives (a workspace
    /// declaration's `impl FnMut(..)` parameters).
    pub(crate) by_position: Vec<Option<Vec<Ty>>>,
}

impl Method {
    /// What a closure handed as argument `at` receives.
    pub(crate) fn closure_at(&self, at: usize) -> Vec<Ty> {
        match self.by_position.get(at) {
            Some(Some(params)) => params.clone(),
            _ => self.closure.clone(),
        }
    }
}

fn method(class: Class, ret: Ty) -> Method {
    with_closure(class, ret, Vec::new())
}

fn with_closure(class: Class, ret: Ty, closure: Vec<Ty>) -> Method {
    Method {
        class,
        ret,
        closure,
        by_position: Vec::new(),
    }
}

/// The closure parameters a declaration's parameters give, by position.
pub(crate) fn closures_by_position(params: &[Ty]) -> Vec<Option<Vec<Ty>>> {
    params
        .iter()
        .map(|param| match param {
            Ty::Fn(inputs) => Some(inputs.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn std_named(name: &str, args: Vec<Ty>) -> Ty {
    Ty::Named(Named {
        name: name.to_owned(),
        krate: Some("std".to_owned()),
        prefix: Vec::new(),
        args,
        home: String::new(),
        module: String::new(),
    })
}

pub(crate) fn option_of(inner: Ty) -> Ty {
    std_named("Option", vec![inner])
}

fn unknown_ret(name: &str) -> Ty {
    Ty::unknown(format!("what `{name}` returns"))
}

fn arg(args: &[Ty], at: usize) -> Ty {
    args.get(at)
        .cloned()
        .unwrap_or_else(|| Ty::unknown("a type argument that is not written"))
}

/// A standard type's associated functions that build one (`Vec::new()`), and
/// `Default::default` / `From::from` on any type: they return the type.
pub(crate) fn builds_itself(name: &str) -> bool {
    matches!(name, "new" | "with_capacity" | "default" | "from")
}

/// The fixed list. `None` is "not on the list": the method is unresolved.
pub(crate) fn std_method(kind: Std, args: &[Ty], name: &str) -> Option<Method> {
    let shared = |inner: Ty| Ty::reference(false, inner);
    let unique = |inner: Ty| Ty::reference(true, inner);
    let t = arg(args, 0);
    Some(match kind {
        Std::Vec | Std::VecDeque => match name {
            "push"
            | "push_back"
            | "push_front"
            | "insert"
            | "clear"
            | "truncate"
            | "dedup"
            | "dedup_by"
            | "dedup_by_key"
            | "extend"
            | "extend_from_slice"
            | "append"
            | "resize"
            | "resize_with"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "sort_unstable"
            | "sort_unstable_by"
            | "sort_unstable_by_key"
            | "reverse"
            | "rotate_left"
            | "rotate_right"
            | "swap"
            | "fill"
            | "reserve"
            | "shrink_to_fit"
            | "make_contiguous"
            | "copy_from_slice"
            | "clone_from_slice"
            | "shrink_to" => {
                let closure = match name {
                    "sort_by_key" | "sort_unstable_by_key" | "dedup_by_key" => {
                        vec![unique(t.clone())]
                    }
                    "sort_by" | "sort_unstable_by" => vec![shared(t.clone()), shared(t.clone())],
                    _ => Vec::new(),
                };
                with_closure(Class::Write, unknown_ret(name), closure)
            }
            "retain" => with_closure(Class::Write, Ty::Tuple(Vec::new()), vec![shared(t)]),
            "retain_mut" => with_closure(Class::Write, Ty::Tuple(Vec::new()), vec![unique(t)]),
            "pop" | "pop_back" | "pop_front" => method(Class::Write, option_of(t)),
            "remove" if kind == Std::VecDeque => method(Class::Write, option_of(t)),
            "remove" | "swap_remove" => method(Class::Write, t),
            "drain" => method(Class::Write, Ty::Iter(Box::new(t))),
            "split_off" => method(Class::Write, std_named("Vec", vec![t])),
            "get_mut" | "first_mut" | "last_mut" | "front_mut" | "back_mut" => {
                method(Class::Access, option_of(unique(t)))
            }
            "iter_mut" => method(Class::Access, Ty::Iter(Box::new(unique(t)))),
            "as_mut_slice" | "as_mut" => method(Class::Access, unique(Ty::Slice(Box::new(t)))),
            "get" | "first" | "last" | "front" | "back" => {
                method(Class::Read, option_of(shared(t)))
            }
            "iter" => method(Class::Read, Ty::Iter(Box::new(shared(t)))),
            "len"
            | "is_empty"
            | "capacity"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "binary_search"
            | "binary_search_by"
            | "binary_search_by_key"
            | "to_vec"
            | "as_slice"
            | "windows"
            | "chunks"
            | "concat"
            | "join"
            | "clone"
            | "to_owned"
            | "split_first"
            | "split_last"
            | "range"
            | "eq"
            | "ne"
            | "as_ref"
            | "is_sorted"
            | "partition_point"
            | "hash"
            | "into_iter"
            | "split_at" => {
                let closure = match name {
                    "partition_point" | "binary_search_by" | "binary_search_by_key" => {
                        vec![shared(t.clone())]
                    }
                    _ => Vec::new(),
                };
                with_closure(Class::Read, unknown_ret(name), closure)
            }
            _ => return None,
        },
        Std::Map => {
            let k = t;
            let v = arg(args, 1);
            match name {
                "insert" | "remove" => method(Class::Write, option_of(v)),
                "clear" | "append" | "extend" | "remove_entry" | "pop_first" | "pop_last"
                | "split_off" | "drain" | "extract_if" | "reserve" | "shrink_to_fit" => {
                    method(Class::Write, unknown_ret(name))
                }
                "retain" => with_closure(
                    Class::Write,
                    Ty::Tuple(Vec::new()),
                    vec![shared(k), unique(v)],
                ),
                "entry" => method(Class::Write, Ty::Entry(Box::new(v))),
                "get_mut" => method(Class::Access, option_of(unique(v))),
                "values_mut" => method(Class::Access, Ty::Iter(Box::new(unique(v)))),
                "iter_mut" | "range_mut" => method(
                    Class::Access,
                    Ty::Iter(Box::new(Ty::Tuple(vec![shared(k), unique(v)]))),
                ),
                "first_entry" | "last_entry" => method(Class::Access, unknown_ret(name)),
                "get" => method(Class::Read, option_of(shared(v))),
                "values" => method(Class::Read, Ty::Iter(Box::new(shared(v)))),
                "keys" => method(Class::Read, Ty::Iter(Box::new(shared(k)))),
                "iter" | "range" => method(
                    Class::Read,
                    Ty::Iter(Box::new(Ty::Tuple(vec![shared(k), shared(v)]))),
                ),
                "first_key_value" | "last_key_value" | "get_key_value" => method(
                    Class::Read,
                    option_of(Ty::Tuple(vec![shared(k), shared(v)])),
                ),
                "contains_key" | "len" | "is_empty" | "clone" | "eq" | "ne" | "capacity" => {
                    method(Class::Read, unknown_ret(name))
                }
                _ => return None,
            }
        }
        Std::Set => match name {
            "insert" | "remove" | "clear" | "take" | "append" | "extend" | "pop_first"
            | "pop_last" | "replace" | "drain" | "extract_if" | "reserve" | "split_off" => {
                method(Class::Write, unknown_ret(name))
            }
            "retain" => with_closure(Class::Write, Ty::Tuple(Vec::new()), vec![shared(t)]),
            "iter" => method(Class::Read, Ty::Iter(Box::new(shared(t)))),
            "first" | "last" | "get" => method(Class::Read, option_of(shared(t))),
            "contains"
            | "len"
            | "is_empty"
            | "is_subset"
            | "is_superset"
            | "is_disjoint"
            | "difference"
            | "union"
            | "intersection"
            | "symmetric_difference"
            | "range"
            | "clone"
            | "eq"
            | "ne"
            | "capacity" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::Option => match name {
            "take" | "replace" => method(Class::Write, option_of(t)),
            "insert" | "get_or_insert" | "get_or_insert_default" | "get_or_insert_with" => {
                method(Class::Write, unique(t))
            }
            "take_if" => with_closure(Class::Write, option_of(t.clone()), vec![unique(t)]),
            "as_mut" => method(Class::Access, option_of(unique(t))),
            "as_deref_mut" => method(
                Class::Access,
                option_of(Ty::unknown("what `as_deref_mut` derefs to")),
            ),
            "iter_mut" => method(Class::Access, Ty::Iter(Box::new(unique(t)))),
            "unwrap" | "expect" | "unwrap_unchecked" => method(Class::Pass, t),
            "as_ref" => method(Class::Read, option_of(shared(t))),
            "iter" => method(Class::Read, Ty::Iter(Box::new(shared(t)))),
            "is_some" | "is_none" | "is_some_and" | "is_none_or" | "map" | "map_or"
            | "map_or_else" | "and_then" | "filter" | "inspect" | "ok_or" | "ok_or_else" | "or"
            | "or_else" | "xor" | "and" | "zip" | "flatten" | "as_deref" | "unwrap_or_default"
            | "unwrap_or" | "unwrap_or_else" | "cloned" | "copied" | "clone" | "eq" | "ne"
            | "cmp" | "partial_cmp" | "hash" | "into_iter" | "is_some_then" => {
                let closure = match name {
                    "is_some_and" | "is_none_or" | "map" | "map_or" | "map_or_else"
                    | "and_then" => {
                        vec![t.clone()]
                    }
                    "filter" | "inspect" => vec![shared(t.clone())],
                    _ => Vec::new(),
                };
                let ret = match name {
                    "unwrap_or" | "unwrap_or_default" | "unwrap_or_else" => t,
                    "cloned" | "copied" => match t {
                        Ty::Ref { inner, .. } => option_of(*inner),
                        other => option_of(other),
                    },
                    "filter" | "inspect" | "or" | "or_else" | "xor" | "clone" => option_of(t),
                    _ => unknown_ret(name),
                };
                with_closure(Class::Read, ret, closure)
            }
            _ => return None,
        },
        Std::Result => match name {
            "unwrap" | "expect" => method(Class::Pass, t),
            "as_mut" => method(Class::Access, unknown_ret(name)),
            "ok" => method(Class::Read, option_of(t)),
            "err" => method(Class::Read, option_of(arg(args, 1))),
            "is_ok" | "is_err" | "is_ok_and" | "is_err_and" | "as_ref" | "map" | "map_err"
            | "and_then" | "or_else" | "unwrap_or" | "unwrap_or_default" | "unwrap_or_else"
            | "unwrap_err" | "expect_err" | "clone" | "inspect" | "inspect_err" | "map_or"
            | "map_or_else" | "into_iter" | "iter" | "eq" | "ne" => {
                let ret = match name {
                    "unwrap_or" | "unwrap_or_default" | "unwrap_or_else" => t,
                    _ => unknown_ret(name),
                };
                method(Class::Read, ret)
            }
            _ => return None,
        },
        Std::String | Std::Str => match name {
            "push"
            | "push_str"
            | "clear"
            | "truncate"
            | "insert"
            | "insert_str"
            | "pop"
            | "remove"
            | "retain"
            | "drain"
            | "extend"
            | "replace_range"
            | "reserve"
            | "shrink_to_fit"
            | "make_ascii_lowercase"
            | "make_ascii_uppercase"
            | "split_off" => method(Class::Write, unknown_ret(name)),
            "as_mut_str" => method(Class::Access, unknown_ret(name)),
            "clone" | "to_string" | "to_owned" | "replace" | "replacen" | "repeat"
            | "to_lowercase" | "to_uppercase" | "to_ascii_lowercase" | "to_ascii_uppercase" => {
                method(Class::Read, std_named("String", Vec::new()))
            }
            "len"
            | "is_empty"
            | "as_str"
            | "chars"
            | "bytes"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "trim_matches"
            | "trim_start_matches"
            | "trim_end_matches"
            | "split"
            | "split_whitespace"
            | "splitn"
            | "rsplit"
            | "rsplitn"
            | "split_once"
            | "rsplit_once"
            | "lines"
            | "find"
            | "rfind"
            | "eq_ignore_ascii_case"
            | "char_indices"
            | "parse"
            | "get"
            | "as_bytes"
            | "is_char_boundary"
            | "capacity"
            | "strip_prefix"
            | "strip_suffix"
            | "eq"
            | "ne"
            | "cmp"
            | "partial_cmp"
            | "hash"
            | "matches"
            | "is_ascii"
            | "encode_utf16"
            | "as_ref"
            | "into_bytes"
            | "into_boxed_str"
            | "escape_debug"
            | "escape_default"
            | "fmt"
            | "into" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::PathBuf | Std::Path => {
            match name {
                "push" | "pop" | "set_extension" | "set_file_name" | "clear" | "reserve" => {
                    method(Class::Write, unknown_ret(name))
                }
                "as_mut_os_string" => method(Class::Access, unknown_ret(name)),
                "clone" | "to_path_buf" | "join" | "with_extension" | "with_file_name"
                | "to_owned" => method(Class::Read, std_named("PathBuf", Vec::new())),
                "as_path" | "display" | "exists" | "is_dir" | "is_file" | "parent"
                | "file_name" | "extension" | "file_stem" | "to_str" | "to_string_lossy"
                | "components" | "starts_with" | "ends_with" | "canonicalize" | "metadata"
                | "is_absolute" | "is_relative" | "as_os_str" | "iter" | "has_root"
                | "strip_prefix" | "eq" | "ne" | "cmp" | "partial_cmp" | "hash" | "as_ref"
                | "try_exists" | "read_dir" | "symlink_metadata" | "ancestors"
                | "into_os_string" => method(Class::Read, unknown_ret(name)),
                _ => return None,
            }
        }
        Std::Box => match name {
            "as_mut" => method(Class::Access, unique(t)),
            "as_ref" => method(Class::Read, shared(t)),
            _ => return None,
        },
        Std::Shared => match name {
            "clone" => method(Class::Read, std_named("Arc", vec![t])),
            "as_ref" | "as_ptr" | "downgrade" | "strong_count" | "ptr_eq" => {
                method(Class::Read, unknown_ret(name))
            }
            _ => return None,
        },
        Std::Cell => match name {
            "set" | "replace" | "take" | "swap" | "update" => {
                method(Class::Inner, unknown_ret(name))
            }
            "get_mut" => method(Class::Access, unique(t)),
            "get" | "clone" => method(Class::Read, t),
            _ => return None,
        },
        Std::RefCell => match name {
            "borrow_mut" | "try_borrow_mut" | "replace" | "replace_with" | "swap" | "take" => {
                method(Class::Inner, unknown_ret(name))
            }
            "get_mut" => method(Class::Access, unique(t)),
            "borrow" | "try_borrow" | "clone" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::Lock => match name {
            "lock" | "try_lock" | "write" | "try_write" => method(Class::Inner, unknown_ret(name)),
            "get_mut" => method(Class::Access, unknown_ret(name)),
            "read" | "try_read" | "is_poisoned" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::Atomic => match name {
            "store"
            | "swap"
            | "fetch_add"
            | "fetch_sub"
            | "fetch_or"
            | "fetch_and"
            | "fetch_xor"
            | "fetch_nand"
            | "fetch_max"
            | "fetch_min"
            | "fetch_update"
            | "compare_exchange"
            | "compare_exchange_weak" => method(Class::Inner, unknown_ret(name)),
            "get_mut" => method(Class::Access, unknown_ret(name)),
            "load" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::Channel => match name {
            "iter" | "try_iter" => method(Class::Read, Ty::Iter(Box::new(t))),
            "send" | "try_send" | "recv" | "try_recv" | "recv_timeout" | "clone"
            | "send_timeout" => method(Class::Read, unknown_ret(name)),
            _ => return None,
        },
        Std::Value => match name {
            "make_ascii_uppercase" | "make_ascii_lowercase" => {
                method(Class::Write, unknown_ret(name))
            }
            // Every other method of these types takes `self` by value or
            // `&self`: a value type has nothing to hand out mutably.
            _ => method(Class::Read, unknown_ret(name)),
        },
    })
}

/// An iterator's methods: what each returns, and what its closures receive.
/// An iterator is a temporary, so none of these touches a place; the item's
/// reference is what carries the origin.
pub(crate) fn iterator_method(item: &Ty, name: &str) -> Option<Method> {
    let same = || Ty::Iter(Box::new(item.clone()));
    let shared_item = || Ty::reference(false, item.clone());
    Some(match name {
        "next" | "last" | "nth" | "next_back" | "min" | "max" => {
            method(Class::Read, option_of(item.clone()))
        }
        "find" => with_closure(Class::Read, option_of(item.clone()), vec![shared_item()]),
        "max_by_key" | "min_by_key" => {
            with_closure(Class::Read, option_of(item.clone()), vec![shared_item()])
        }
        "max_by" | "min_by" => with_closure(
            Class::Read,
            option_of(item.clone()),
            vec![shared_item(), shared_item()],
        ),
        "rev" | "skip" | "take" | "step_by" | "peekable" | "fuse" | "chain" | "cycle" => {
            method(Class::Read, same())
        }
        "filter" | "skip_while" | "take_while" | "inspect" => {
            with_closure(Class::Read, same(), vec![shared_item()])
        }
        "enumerate" => method(
            Class::Read,
            Ty::Iter(Box::new(Ty::Tuple(vec![
                std_named("usize", Vec::new()),
                item.clone(),
            ]))),
        ),
        "cloned" | "copied" => method(
            Class::Read,
            Ty::Iter(Box::new(match item {
                Ty::Ref { inner, .. } => (**inner).clone(),
                other => other.clone(),
            })),
        ),
        "map" | "filter_map" | "flat_map" | "map_while" | "scan" => with_closure(
            Class::Read,
            Ty::Iter(Box::new(unknown_ret(name))),
            vec![item.clone()],
        ),
        "for_each" | "any" | "all" | "position" | "rposition" | "find_map" | "partition"
        | "try_for_each" => with_closure(Class::Read, unknown_ret(name), vec![item.clone()]),
        "fold" => with_closure(
            Class::Read,
            unknown_ret(name),
            vec![Ty::unknown("a fold's accumulator"), item.clone()],
        ),
        "collect" | "count" | "sum" | "product" | "unzip" | "is_empty" | "len" | "zip"
        | "flatten" | "into_iter" | "eq" | "ne" | "cmp" | "partial_cmp" | "is_sorted"
        | "by_ref" => method(Class::Read, unknown_ret(name)),
        _ => return None,
    })
}

/// A map entry's methods.
pub(crate) fn entry_method(value: &Ty, name: &str) -> Option<Method> {
    Some(match name {
        "or_insert" | "or_insert_with" | "or_default" | "or_insert_with_key" => {
            method(Class::Pass, Ty::reference(true, value.clone()))
        }
        "and_modify" => with_closure(
            Class::Pass,
            Ty::Entry(Box::new(value.clone())),
            vec![Ty::reference(true, value.clone())],
        ),
        "key" => method(Class::Read, unknown_ret(name)),
        _ => return None,
    })
}

/// The standard prelude traits' methods, which hold for every type: the
/// fallback when a type declares no method of that name. `receiver` is the
/// type the method is found on, which `clone` returns.
pub(crate) fn prelude_method(name: &str, receiver: &Ty) -> Option<Method> {
    Some(match name {
        "clone" | "to_owned" => method(Class::Read, receiver.clone()),
        "to_string" => method(Class::Read, std_named("String", Vec::new())),
        "eq" | "ne" | "cmp" | "partial_cmp" | "lt" | "le" | "gt" | "ge" | "hash" | "fmt"
        | "into" | "try_into" | "into_iter" | "max" | "min" | "clamp" | "borrow" | "type_id" => {
            method(Class::Read, unknown_ret(name))
        }
        "clone_from" | "extend" | "write_str" | "write_fmt" | "write_char" => {
            method(Class::Write, unknown_ret(name))
        }
        _ => return None,
    })
}
