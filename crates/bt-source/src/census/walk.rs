//! The walk over one function body: every place expression that names a field
//! of the census's structs, classified by the context it is used in.
//!
//! Two passes meet at every node. [`Walk::eval`] is bottom-up and answers
//! *what is this* — a type from a declaration, whether it is reached through a
//! shared reference, and which field of the four it lives inside (its origin).
//! [`Walk::visit`] is top-down and carries *what is being done to it* — read,
//! assigned, called with a method that writes, lent `&mut` — down the place
//! until it meets the innermost field of the four, where it is recorded. The
//! outer fields on the same place, reached through an index or an access
//! method, are recorded as mutable access; nothing is recorded twice as a
//! write.

use std::collections::{BTreeMap, BTreeSet};

use syn::punctuated::Punctuated;

use super::Sink;
use super::resolve::{FieldAnswer, Kind, Owner, Resolver, first_arg};
use super::types::{
    Class, Receiver, Std, Ty, Written, closures_by_position, impl_header, impl_of, option_of,
};
use crate::index::{Index, ItemKind, ItemRecord};

/// Which of the census's columns a site lands in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Column {
    /// A proven write: an assignment, or a call whose method resolves to one
    /// that writes.
    Write,
    /// Mutable access or escape: a `&mut` lend, `get_mut`, `iter_mut`, an
    /// index in a mutable place, a method that hands back `&mut`.
    Access,
    /// A change to what a hub holds: `push`, `insert`, `remove`, an assignment
    /// of the whole.
    Membership,
    /// A change through a shared reference: `Cell`, `RefCell`, atomics, locks.
    Inner,
}

impl Column {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Access => "access",
            Self::Membership => "membership",
            Self::Inner => "inner",
        }
    }
}

#[derive(Clone, Debug)]
enum Effect {
    Write,
    Lend,
    Access,
    Inner,
    /// It does not resolve, and why.
    Unknown(String),
}

#[derive(Clone, Debug)]
struct Use {
    effect: Effect,
    kind: String,
}

#[derive(Clone, Debug)]
enum Pending {
    /// Nothing mutable happens to this value.
    Read,
    /// A nearer field of the four took the mutable use; this one is on the
    /// path to it.
    Attributed,
    /// This use is still looking for the innermost field of the four.
    Use(Use),
}

impl Pending {
    fn using(effect: Effect, kind: impl Into<String>) -> Self {
        Self::Use(Use {
            effect,
            kind: kind.into(),
        })
    }
}

/// Where a value lives: the innermost field of the four it is inside, and
/// whether it is that field exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Origin {
    fact: usize,
    exact: bool,
}

fn inexact(origin: Option<Origin>) -> Option<Origin> {
    origin.map(|origin| Origin {
        exact: false,
        ..origin
    })
}

#[derive(Clone, Debug)]
struct Binding {
    ty: Ty,
    origin: Option<Origin>,
}

#[derive(Clone, Debug)]
struct Val {
    ty: Ty,
    origin: Option<Origin>,
    /// Reached through a shared reference: nothing but inner mutability can
    /// change it.
    shared: bool,
}

impl Val {
    fn unknown(reason: impl Into<String>) -> Self {
        Self {
            ty: Ty::unknown(reason),
            origin: None,
            shared: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Move,
    Shared,
    Unique,
}

// ── the walk ────────────────────────────────────────────────────────────

/// One function body, walked.
pub(crate) struct Walk<'w, 'd> {
    resolver: &'w Resolver<'d>,
    written: Written,
    scopes: Vec<BTreeMap<String, Binding>>,
    sink: &'w mut Sink,
    module: String,
    function: String,
}

impl<'w, 'd> Walk<'w, 'd> {
    /// Walk one callable item of the subject. `Err` is a body that does not
    /// parse.
    pub(crate) fn item(
        resolver: &'w Resolver<'d>,
        index: &Index,
        item: &ItemRecord,
        module: String,
        function: String,
        crates: &BTreeSet<String>,
        sink: &'w mut Sink,
    ) -> Result<(), String> {
        let text = index.text(item.whole());
        let (signature, block) = match item.kind() {
            ItemKind::Function => {
                let parsed =
                    syn::parse_str::<syn::ItemFn>(text).map_err(|error| error.to_string())?;
                (parsed.sig, *parsed.block)
            }
            _ if item.type_owner().is_some() => {
                let parsed =
                    syn::parse_str::<syn::ImplItemFn>(text).map_err(|error| error.to_string())?;
                (parsed.sig, parsed.block)
            }
            _ => {
                let parsed =
                    syn::parse_str::<syn::TraitItemFn>(text).map_err(|error| error.to_string())?;
                let Some(block) = parsed.default else {
                    return Ok(());
                };
                (parsed.sig, block)
            }
        };
        let mut written = Written {
            package: super::types::package_of(index, item),
            module: module.clone(),
            crates: crates.clone(),
            ..Written::default()
        };
        if item.type_owner().is_some()
            && let Some(header) = impl_of(index, item).and_then(|block| impl_header(index, block))
        {
            written.enter(&header.generics);
            written.self_ty = Some(written.ty(&header.self_ty));
        }
        let mut walk = Walk {
            resolver,
            written,
            scopes: vec![BTreeMap::new()],
            sink,
            module,
            function,
        };
        walk.function_body(&signature, &block);
        Ok(())
    }

    fn function_body(&mut self, signature: &syn::Signature, block: &syn::Block) {
        let saved = self.written.clone();
        self.written.enter(&signature.generics);
        self.scopes.push(BTreeMap::new());
        for input in &signature.inputs {
            match input {
                syn::FnArg::Receiver(_) => {
                    let self_ty = self
                        .written
                        .self_ty
                        .clone()
                        .unwrap_or_else(|| Ty::unknown("`self` of a trait's own method"));
                    let ty = match Receiver::of(signature) {
                        Receiver::Shared => Ty::reference(false, self_ty),
                        Receiver::Unique => Ty::reference(true, self_ty),
                        Receiver::Value | Receiver::None => self_ty,
                    };
                    self.define("self", ty, None);
                }
                syn::FnArg::Typed(typed) => {
                    let ty = self.written.ty(&typed.ty);
                    self.bind(&typed.pat, ty, None, Mode::Move);
                }
            }
        }
        self.block(block);
        self.scopes.pop();
        self.written = saved;
    }

    // ── bindings ──────────────────────────────────────────────────────────

    fn define(&mut self, name: &str, ty: Ty, origin: Option<Origin>) {
        let origin = if ty.carries_reference() { origin } else { None };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Binding { ty, origin });
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// Bind a pattern against a type, with the language's default binding
    /// modes. Every name the pattern binds is defined — with an unknown type
    /// when the rules do not reach it — so a shadowing binding is never read
    /// as the one it shadows.
    fn bind(&mut self, pattern: &syn::Pat, ty: Ty, origin: Option<Origin>, mode: Mode) {
        match pattern {
            syn::Pat::Ident(ident) => {
                let bound = if ident.by_ref.is_some() {
                    Ty::reference(ident.mutability.is_some(), ty.clone())
                } else {
                    match mode {
                        Mode::Move => ty.clone(),
                        Mode::Shared => Ty::reference(false, ty.clone()),
                        Mode::Unique => Ty::reference(true, ty.clone()),
                    }
                };
                self.define(&ident.ident.to_string(), bound, origin);
                if let Some((_, sub)) = &ident.subpat {
                    self.bind(sub, ty, origin, mode);
                }
            }
            syn::Pat::Reference(reference) => match self.resolver.expand(ty) {
                Ty::Ref { inner, .. } => self.bind(&reference.pat, *inner, origin, Mode::Move),
                _ => self.bind(
                    &reference.pat,
                    Ty::unknown("a reference pattern over a type that is not a reference"),
                    None,
                    Mode::Move,
                ),
            },
            syn::Pat::Type(typed) => {
                let written = self.written.ty(&typed.ty);
                self.bind(&typed.pat, written, origin, mode);
            }
            syn::Pat::Paren(paren) => self.bind(&paren.pat, ty, origin, mode),
            syn::Pat::Or(or) => {
                for case in &or.cases {
                    self.bind(case, ty.clone(), origin, mode);
                }
            }
            syn::Pat::Tuple(tuple) => {
                let (ty, mode) = self.peel(ty, mode);
                let why = match &ty {
                    Ty::Unknown(why) => why.clone(),
                    other => format!("a tuple pattern over {}", other.spelling()),
                };
                let items = match ty {
                    Ty::Tuple(items) if items.len() == tuple.elems.len() => items,
                    _ => Vec::new(),
                };
                for (at, element) in tuple.elems.iter().enumerate() {
                    let item = items
                        .get(at)
                        .cloned()
                        .unwrap_or_else(|| Ty::unknown(why.clone()));
                    self.bind(element, item, inexact(origin), mode);
                }
            }
            syn::Pat::TupleStruct(tuple) => {
                let (ty, mode) = self.peel(ty, mode);
                let variant = last_segment(&tuple.path);
                let fields = self.variant_fields(&ty, &variant);
                for (at, element) in tuple.elems.iter().enumerate() {
                    let item = fields
                        .as_ref()
                        .and_then(|fields| fields.get(at))
                        .map_or_else(
                            || match &ty {
                                Ty::Unknown(why) => Ty::unknown(why.clone()),
                                other => Ty::unknown(format!(
                                    "a variant `{variant}` of {}, which the rules do not find",
                                    other.spelling()
                                )),
                            },
                            |(_, ty)| ty.clone(),
                        );
                    self.bind(element, item, inexact(origin), mode);
                }
            }
            syn::Pat::Struct(structure) => {
                let (ty, mode) = self.peel(ty, mode);
                let variant = last_segment(&structure.path);
                let variant_fields = self.variant_fields(&ty, &variant);
                for field in &structure.fields {
                    let name = member_name(&field.member);
                    if let Some(fields) = &variant_fields {
                        let item = fields
                            .iter()
                            .find(|(declared, _)| *declared == name)
                            .map_or_else(
                                || {
                                    Ty::unknown(format!(
                                        "a field `{name}` the variant does not declare"
                                    ))
                                },
                                |(_, ty)| ty.clone(),
                            );
                        self.bind(&field.pat, item, inexact(origin), mode);
                        continue;
                    }
                    match self.resolver.field(&ty, false, &name) {
                        Ok(FieldAnswer {
                            owner: Owner::Fact(fact),
                            ty: field_ty,
                            ..
                        }) => {
                            if mode == Mode::Unique || pattern_takes_unique(&field.pat) {
                                self.record(
                                    fact,
                                    &Use {
                                        effect: Effect::Access,
                                        kind: "pattern".to_owned(),
                                    },
                                );
                            }
                            self.bind(
                                &field.pat,
                                field_ty,
                                Some(Origin { fact, exact: true }),
                                mode,
                            );
                        }
                        Ok(FieldAnswer { ty: field_ty, .. }) => {
                            self.bind(&field.pat, field_ty, inexact(origin), mode);
                        }
                        Err(why) => self.bind(&field.pat, Ty::unknown(why), None, mode),
                    }
                }
            }
            syn::Pat::Slice(slice) => {
                let (ty, mode) = self.peel(ty, mode);
                let element = match &ty {
                    Ty::Slice(element) => (**element).clone(),
                    Ty::Named(named) if named.name == "Vec" => first_arg(named),
                    _ => Ty::unknown("a slice pattern over a type that is not a slice"),
                };
                for item in &slice.elems {
                    self.bind(item, element.clone(), inexact(origin), mode);
                }
            }
            _ => {}
        }
    }

    /// The fields a variant pattern binds: `Some`/`Ok`/`Err` of the standard
    /// enums, or a variant of the workspace enum the scrutinee's type is.
    fn variant_fields(&self, ty: &Ty, variant: &str) -> Option<Vec<(String, Ty)>> {
        if let Ty::Named(named) = ty {
            let standard = match (named.name.as_str(), variant) {
                ("Option", "Some") | ("Result", "Ok") => named.args.first().cloned(),
                ("Result", "Err") => named.args.get(1).cloned(),
                _ => None,
            };
            if let Some(inner) = standard {
                return Some(vec![("0".to_owned(), inner)]);
            }
        }
        self.resolver.variant_fields(ty, variant)
    }

    /// The references in front of a type, taken off as the default binding
    /// modes take them; aliases expanded on the way.
    fn peel(&self, ty: Ty, mut mode: Mode) -> (Ty, Mode) {
        let mut ty = self.resolver.expand(ty);
        while let Ty::Ref { mutable, inner } = ty {
            mode = match (mode, mutable) {
                (Mode::Move | Mode::Unique, true) => Mode::Unique,
                (Mode::Move | Mode::Unique, false) | (Mode::Shared, _) => Mode::Shared,
            };
            ty = self.resolver.expand(*inner);
        }
        (ty, mode)
    }

    // ── what a thing is ───────────────────────────────────────────────────

    fn eval(&self, expr: &syn::Expr) -> Val {
        match expr {
            syn::Expr::Path(path) => {
                if path.qself.is_none()
                    && path.path.segments.len() == 1
                    && let Some(binding) = self.lookup(&path.path.segments[0].ident.to_string())
                {
                    return Val {
                        ty: binding.ty.clone(),
                        origin: binding.origin,
                        shared: false,
                    };
                }
                Val::unknown("a path that is not a local binding")
            }
            syn::Expr::Field(field) => {
                let base = self.eval(&field.base);
                let name = member_name(&field.member);
                match self.resolver.field(&base.ty, base.shared, &name) {
                    Ok(FieldAnswer {
                        owner: Owner::Fact(fact),
                        ty,
                        shared,
                    }) => Val {
                        ty,
                        origin: Some(Origin { fact, exact: true }),
                        shared,
                    },
                    Ok(FieldAnswer { ty, shared, .. }) => Val {
                        ty,
                        origin: inexact(base.origin),
                        shared,
                    },
                    Err(why) => Val {
                        ty: Ty::Unknown(why),
                        origin: inexact(base.origin),
                        shared: base.shared,
                    },
                }
            }
            syn::Expr::Index(index) => {
                let base = self.eval(&index.expr);
                let ranged = matches!(&*index.index, syn::Expr::Range(_));
                let (ty, shared) = self.element(&base.ty, base.shared, ranged);
                Val {
                    ty,
                    origin: inexact(base.origin),
                    shared,
                }
            }
            syn::Expr::Unary(unary) => match unary.op {
                syn::UnOp::Deref(_) => self.deref(self.eval(&unary.expr)),
                _ => Val::unknown("an operator's result"),
            },
            syn::Expr::Reference(reference) => {
                let inner = self.eval(&reference.expr);
                Val {
                    ty: Ty::reference(reference.mutability.is_some(), inner.ty),
                    origin: inner.origin,
                    shared: false,
                }
            }
            syn::Expr::Paren(paren) => self.eval(&paren.expr),
            syn::Expr::Group(group) => self.eval(&group.expr),
            syn::Expr::MethodCall(call) => {
                let receiver = self.eval(&call.receiver);
                let name = call.method.to_string();
                match self.resolver.method(&receiver.ty, receiver.shared, &name) {
                    Ok(found) => {
                        let ret = found.method.ret;
                        let origin = if ret.carries_reference() {
                            inexact(receiver.origin)
                        } else {
                            None
                        };
                        Val {
                            ty: ret,
                            origin,
                            shared: false,
                        }
                    }
                    Err(unresolved) => {
                        Val::unknown(format!("what `{name}` returns: {}", unresolved.why))
                    }
                }
            }
            syn::Expr::Call(call) => self.callee(call).0,
            syn::Expr::Try(attempt) => {
                let inner = self.eval(&attempt.expr);
                match self.resolver.expand(inner.ty) {
                    Ty::Named(named) if matches!(named.name.as_str(), "Option" | "Result") => {
                        let ty = first_arg(&named);
                        Val {
                            origin: if ty.carries_reference() {
                                inner.origin
                            } else {
                                None
                            },
                            ty,
                            shared: false,
                        }
                    }
                    other => Val::unknown(format!("`?` on {}", other.spelling())),
                }
            }
            syn::Expr::Tuple(tuple) => {
                let items: Vec<Val> = tuple.elems.iter().map(|item| self.eval(item)).collect();
                let origins: BTreeSet<usize> = items
                    .iter()
                    .filter_map(|item| item.origin.map(|origin| origin.fact))
                    .collect();
                let origin = if origins.len() == 1 {
                    items.iter().find_map(|item| inexact(item.origin))
                } else {
                    None
                };
                Val {
                    ty: Ty::Tuple(items.into_iter().map(|item| item.ty).collect()),
                    origin,
                    shared: false,
                }
            }
            syn::Expr::Struct(structure) if structure.qself.is_none() => Val {
                ty: self.written.path(&structure.path),
                origin: None,
                shared: false,
            },
            syn::Expr::Cast(cast) => Val {
                ty: self.written.ty(&cast.ty),
                origin: None,
                shared: false,
            },
            syn::Expr::Lit(_) => Val::unknown("a literal"),
            syn::Expr::If(_) | syn::Expr::Match(_) | syn::Expr::Block(_) | syn::Expr::Loop(_) => {
                Val::unknown("the value of a block, a branch or a loop")
            }
            syn::Expr::Macro(mac) => Val::unknown(format!(
                "what `{}!` expands to",
                last_segment(&mac.mac.path)
            )),
            syn::Expr::Closure(_) => Val::unknown("a closure"),
            _ => Val::unknown("an operator's result"),
        }
    }

    /// `*value`.
    fn deref(&self, value: Val) -> Val {
        match self.resolver.expand(value.ty) {
            Ty::Ref { mutable, inner } => Val {
                ty: *inner,
                origin: value.origin,
                shared: !mutable || value.shared,
            },
            Ty::Named(named) => match self.resolver.kind(&named) {
                Kind::Std(Std::Box) => Val {
                    ty: first_arg(&named),
                    origin: value.origin,
                    shared: value.shared,
                },
                Kind::Std(Std::Shared) => Val {
                    ty: first_arg(&named),
                    origin: value.origin,
                    shared: true,
                },
                Kind::Workspace { name, packages, .. } => {
                    match self.resolver.deref_target(&name, &packages, &named.args) {
                        Ok(Some(target)) => Val {
                            ty: target,
                            origin: value.origin,
                            shared: value.shared,
                        },
                        Ok(None) => Val::unknown(format!("`*` on `{name}`, which has no `Deref`")),
                        Err(why) => Val::unknown(why),
                    }
                }
                _ => Val::unknown(format!("`*` on `{}`", named.name)),
            },
            other => Val::unknown(format!("`*` on {}", other.spelling())),
        }
    }

    /// What indexing a value of `ty` gives.
    fn element(&self, ty: &Ty, shared: bool, ranged: bool) -> (Ty, bool) {
        let mut ty = ty.clone();
        let mut shared = shared;
        for _ in 0..16 {
            match self.resolver.expand(ty) {
                Ty::Ref { mutable, inner } => {
                    shared = shared || !mutable;
                    ty = *inner;
                }
                Ty::Slice(element) => {
                    return if ranged {
                        (Ty::Slice(element), shared)
                    } else {
                        (*element, shared)
                    };
                }
                Ty::Named(named) => match self.resolver.kind(&named) {
                    Kind::Std(Std::Vec | Std::VecDeque) => {
                        let element = first_arg(&named);
                        return if ranged {
                            (Ty::Slice(Box::new(element)), shared)
                        } else {
                            (element, shared)
                        };
                    }
                    Kind::Std(Std::Map) => {
                        return (
                            named
                                .args
                                .get(1)
                                .cloned()
                                .unwrap_or_else(|| Ty::unknown("a map without its value type")),
                            shared,
                        );
                    }
                    Kind::Std(Std::Box) => ty = first_arg(&named),
                    Kind::Std(Std::Shared) => {
                        shared = true;
                        ty = first_arg(&named);
                    }
                    _ => return (Ty::unknown(format!("indexing `{}`", named.name)), shared),
                },
                other => {
                    return (
                        Ty::unknown(format!("indexing {}", other.spelling())),
                        shared,
                    );
                }
            }
        }
        (
            Ty::unknown("a deref chain longer than this census follows"),
            shared,
        )
    }

    /// A call `f(..)` or `Type::f(..)`: its value, and what closures in each
    /// argument position receive.
    fn callee(&self, call: &syn::ExprCall) -> (Val, Vec<Option<Vec<Ty>>>) {
        let syn::Expr::Path(path) = &*call.func else {
            return (
                Val::unknown("a call of something that is not a path"),
                Vec::new(),
            );
        };
        if path.qself.is_some() {
            return (Val::unknown("a call through a qualified path"), Vec::new());
        }
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let args: Vec<Val> = call.args.iter().map(|arg| self.eval(arg)).collect();
        let last = segments.last().cloned().unwrap_or_default();
        if segments.len() == 1 && last == "Some" && args.len() == 1 {
            let val = Val {
                ty: option_of(args[0].ty.clone()),
                origin: args[0].origin,
                shared: false,
            };
            return (val, Vec::new());
        }
        let (ret, closures) = if segments.len() >= 2 {
            let owner_ty = if segments[segments.len() - 2] == "Self" {
                self.written
                    .self_ty
                    .clone()
                    .unwrap_or_else(|| Ty::unknown("`Self` outside an impl"))
            } else {
                let mut shortened = path.path.clone();
                shortened.segments.pop();
                shortened.segments.pop_punct();
                self.written.path(&shortened)
            };
            self.resolver.associated(&owner_ty, &last)
        } else {
            self.free_function(&last, &args)
        };
        let origin = if ret.carries_reference() {
            let lenders: Vec<Option<Origin>> = args
                .iter()
                .filter(|arg| arg.ty.carries_reference())
                .map(|arg| arg.origin)
                .collect();
            match lenders.as_slice() {
                [only] => inexact(*only),
                _ => None,
            }
        } else {
            None
        };
        (
            Val {
                ty: ret,
                origin,
                shared: false,
            },
            closures,
        )
    }

    /// A free function's return type and closure parameters — with one
    /// bounded unification: a generic parameter the return type names is
    /// bound from the argument whose declared type is that parameter behind
    /// references or a slice.
    fn free_function(&self, name: &str, args: &[Val]) -> (Ty, Vec<Option<Vec<Ty>>>) {
        let Some(all) = self.resolver.declarations.functions.get(name) else {
            return (
                Ty::unknown(format!("`{name}` is not a function the workspace declares")),
                Vec::new(),
            );
        };
        let home: Vec<_> = all
            .iter()
            .filter(|decl| decl.package == self.written.package)
            .collect();
        let candidates = if home.is_empty() {
            all.iter().collect()
        } else {
            home
        };
        let mut found: Option<(Ty, Vec<Option<Vec<Ty>>>)> = None;
        for decl in candidates {
            let mut map = BTreeMap::new();
            for (param, arg) in decl.params.iter().zip(args) {
                unify(param, &arg.ty, &decl.generics, &mut map);
            }
            let ret = decl.ret.substitute(&map);
            let ret = if ret.is_open() {
                Ty::unknown(format!("`{name}` returns a generic type"))
            } else {
                ret
            };
            let params: Vec<Ty> = decl
                .params
                .iter()
                .map(|param| param.substitute(&map))
                .collect();
            let answer = (ret, closures_by_position(&params));
            match &found {
                Some(earlier) if *earlier != answer => {
                    return (
                        Ty::unknown(format!("the declarations of `{name}` disagree")),
                        Vec::new(),
                    );
                }
                _ => found = Some(answer),
            }
        }
        found.unwrap_or_else(|| {
            (
                Ty::unknown(format!("`{name}` has no declaration")),
                Vec::new(),
            )
        })
    }

    // ── what is done to it ────────────────────────────────────────────────

    fn record(&mut self, fact: usize, use_: &Use) {
        let hub = self.resolver.subject.facts[fact].hub;
        let label = self.resolver.subject.facts[fact].label();
        let column = match &use_.effect {
            Effect::Write if hub => Column::Membership,
            Effect::Write => Column::Write,
            Effect::Lend | Effect::Access => Column::Access,
            Effect::Inner => Column::Inner,
            Effect::Unknown(why) => {
                let reason = format!("{}: {why}", use_.kind);
                self.sink
                    .unknown(label, &self.module, &self.function, reason);
                return;
            }
        };
        let kind = match use_.effect {
            Effect::Lend => "lend".to_owned(),
            _ => use_.kind.clone(),
        };
        self.sink
            .row(label, column, &self.module, &self.function, kind);
    }

    /// A write-shaped use of `.name`, a field name of the four, through a
    /// receiver the rules do not type.
    fn unknown_name(&mut self, name: &str, use_: &Use, why: &str) {
        let doing = match &use_.effect {
            Effect::Lend => "a `&mut` lend".to_owned(),
            _ => use_.kind.clone(),
        };
        self.sink.unknown(
            format!("?.{name}"),
            &self.module,
            &self.function,
            format!("{doing} through a receiver that does not resolve: {why}"),
        );
    }

    fn block(&mut self, block: &syn::Block) {
        self.scopes.push(BTreeMap::new());
        for statement in &block.stmts {
            self.statement(statement);
        }
        self.scopes.pop();
    }

    fn statement(&mut self, statement: &syn::Stmt) {
        match statement {
            syn::Stmt::Local(local) => {
                let Some(init) = &local.init else {
                    self.bind(
                        &local.pat,
                        Ty::unknown("a `let` without a value"),
                        None,
                        Mode::Move,
                    );
                    return;
                };
                let value = self.eval(&init.expr);
                let pending = if pattern_takes_unique(&local.pat) {
                    Pending::using(Effect::Access, "ref mut")
                } else {
                    Pending::Read
                };
                self.visit(&init.expr, pending);
                if let Some((_, otherwise)) = &init.diverge {
                    self.visit(otherwise, Pending::Read);
                }
                self.bind(&local.pat, value.ty, value.origin, Mode::Move);
            }
            syn::Stmt::Item(item) => self.nested_item(item),
            syn::Stmt::Expr(expr, _) => self.visit(expr, Pending::Read),
            syn::Stmt::Macro(statement) => self.macro_body(&statement.mac),
        }
    }

    /// A `fn` or an `impl` written inside a body: walked with a fresh scope
    /// and counted against the item it is written in, which is the only
    /// identity it has.
    fn nested_item(&mut self, item: &syn::Item) {
        let saved_scopes = std::mem::replace(&mut self.scopes, vec![BTreeMap::new()]);
        let saved = self.written.clone();
        match item {
            syn::Item::Fn(function) => {
                self.written.self_ty = None;
                self.function_body(&function.sig, &function.block);
            }
            syn::Item::Impl(block) => {
                self.written.enter(&block.generics);
                self.written.self_ty = Some(self.written.ty(&block.self_ty));
                for member in &block.items {
                    if let syn::ImplItem::Fn(function) = member {
                        self.function_body(&function.sig, &function.block);
                    }
                }
            }
            _ => {}
        }
        self.scopes = saved_scopes;
        self.written = saved;
    }

    fn visit(&mut self, expr: &syn::Expr, pending: Pending) {
        match expr {
            syn::Expr::Field(field) => {
                let base = self.eval(&field.base);
                let name = member_name(&field.member);
                match self.resolver.field(&base.ty, base.shared, &name) {
                    Ok(FieldAnswer {
                        owner: Owner::Fact(fact),
                        ..
                    }) => {
                        let next = match pending {
                            Pending::Use(use_) => {
                                self.record(fact, &use_);
                                Pending::Attributed
                            }
                            other => other,
                        };
                        self.visit(&field.base, next);
                    }
                    Ok(_) => self.visit(&field.base, pending),
                    Err(why) => {
                        if let Pending::Use(use_) = &pending
                            && self.resolver.subject.names.contains(&name)
                        {
                            self.unknown_name(&name, use_, &why);
                        }
                        self.visit(&field.base, pending);
                    }
                }
            }
            syn::Expr::Index(index) => {
                self.visit(&index.index, Pending::Read);
                let next = match pending {
                    Pending::Read => Pending::Read,
                    Pending::Attributed => Pending::using(Effect::Access, "index"),
                    use_ @ Pending::Use(_) => use_,
                };
                self.visit(&index.expr, next);
            }
            syn::Expr::Paren(paren) => self.visit(&paren.expr, pending),
            syn::Expr::Group(group) => self.visit(&group.expr, pending),
            syn::Expr::Unary(unary) => match unary.op {
                syn::UnOp::Deref(_) => self.visit(&unary.expr, pending),
                _ => self.visit(&unary.expr, Pending::Read),
            },
            syn::Expr::Reference(reference) => {
                if reference.mutability.is_some() {
                    self.visit(&reference.expr, Pending::using(Effect::Lend, "&mut"));
                } else {
                    self.visit(&reference.expr, Pending::Read);
                }
            }
            syn::Expr::RawAddr(raw) => self.visit(&raw.expr, Pending::using(Effect::Lend, "raw")),
            syn::Expr::Assign(assign) => {
                self.visit(&assign.right, Pending::Read);
                self.visit(&assign.left, Pending::using(Effect::Write, "assign"));
            }
            syn::Expr::Binary(binary) => {
                self.visit(&binary.right, Pending::Read);
                if is_compound_assignment(&binary.op) {
                    self.visit(&binary.left, Pending::using(Effect::Write, "compound"));
                } else {
                    self.visit(&binary.left, Pending::Read);
                }
            }
            syn::Expr::MethodCall(call) => self.method_call(call, pending),
            syn::Expr::Call(call) => self.call(call),
            syn::Expr::Path(path) => {
                if let Pending::Use(use_) = pending
                    && path.qself.is_none()
                    && path.path.segments.len() == 1
                    && let Some(origin) = self
                        .lookup(&path.path.segments[0].ident.to_string())
                        .and_then(|binding| binding.origin)
                {
                    self.record(origin.fact, &use_);
                }
            }
            syn::Expr::Try(attempt) => self.visit(&attempt.expr, pending),
            syn::Expr::If(branch) => {
                self.scopes.push(BTreeMap::new());
                self.condition(&branch.cond);
                self.block(&branch.then_branch);
                self.scopes.pop();
                if let Some((_, otherwise)) = &branch.else_branch {
                    self.visit(otherwise, Pending::Read);
                }
            }
            syn::Expr::While(looping) => {
                self.scopes.push(BTreeMap::new());
                self.condition(&looping.cond);
                self.block(&looping.body);
                self.scopes.pop();
            }
            syn::Expr::Let(binding) => {
                // A `let` outside a condition's `&&` chain: its bindings do
                // not outlive it.
                self.scopes.push(BTreeMap::new());
                self.let_binding(binding);
                self.scopes.pop();
            }
            syn::Expr::Match(matching) => {
                let value = self.eval(&matching.expr);
                let unique = matching
                    .arms
                    .iter()
                    .any(|arm| pattern_takes_unique(&arm.pat));
                let pending = if unique {
                    Pending::using(Effect::Access, "ref mut")
                } else {
                    Pending::Read
                };
                self.visit(&matching.expr, pending);
                for arm in &matching.arms {
                    self.scopes.push(BTreeMap::new());
                    self.bind(&arm.pat, value.ty.clone(), value.origin, Mode::Move);
                    if let Some((_, guard)) = &arm.guard {
                        self.visit(guard, Pending::Read);
                    }
                    self.visit(&arm.body, Pending::Read);
                    self.scopes.pop();
                }
            }
            syn::Expr::ForLoop(looping) => {
                let value = self.eval(&looping.expr);
                self.visit(&looping.expr, Pending::Read);
                let item = self.iterated_item(&value.ty);
                self.scopes.push(BTreeMap::new());
                self.bind(&looping.pat, item, inexact(value.origin), Mode::Move);
                self.block(&looping.body);
                self.scopes.pop();
            }
            syn::Expr::Loop(looping) => self.block(&looping.body),
            syn::Expr::Block(block) => self.block(&block.block),
            syn::Expr::Unsafe(block) => self.block(&block.block),
            syn::Expr::Async(block) => self.block(&block.block),
            syn::Expr::TryBlock(block) => self.block(&block.block),
            syn::Expr::Const(block) => self.block(&block.block),
            syn::Expr::Closure(closure) => self.closure(closure, &[], None),
            syn::Expr::Macro(mac) => self.macro_body(&mac.mac),
            syn::Expr::Return(ret) => {
                if let Some(value) = &ret.expr {
                    self.visit(value, Pending::Read);
                }
            }
            syn::Expr::Break(stop) => {
                if let Some(value) = &stop.expr {
                    self.visit(value, Pending::Read);
                }
            }
            syn::Expr::Yield(value) => {
                if let Some(value) = &value.expr {
                    self.visit(value, Pending::Read);
                }
            }
            syn::Expr::Await(wait) => self.visit(&wait.base, Pending::Read),
            syn::Expr::Cast(cast) => self.visit(&cast.expr, Pending::Read),
            syn::Expr::Array(array) => {
                for item in &array.elems {
                    self.visit(item, Pending::Read);
                }
            }
            syn::Expr::Tuple(tuple) => {
                for item in &tuple.elems {
                    self.visit(item, Pending::Read);
                }
            }
            syn::Expr::Repeat(repeat) => {
                self.visit(&repeat.expr, Pending::Read);
                self.visit(&repeat.len, Pending::Read);
            }
            syn::Expr::Range(range) => {
                if let Some(start) = &range.start {
                    self.visit(start, Pending::Read);
                }
                if let Some(end) = &range.end {
                    self.visit(end, Pending::Read);
                }
            }
            syn::Expr::Struct(structure) => {
                for field in &structure.fields {
                    self.visit(&field.expr, Pending::Read);
                }
                if let Some(rest) = &structure.rest {
                    self.visit(rest, Pending::Read);
                }
            }
            _ => {}
        }
    }

    /// A condition: `let` bindings in a `&&` chain are in scope for the
    /// branch, which the caller has opened a scope for.
    fn condition(&mut self, expr: &syn::Expr) {
        match expr {
            syn::Expr::Let(binding) => self.let_binding(binding),
            syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.condition(&binary.left);
                self.condition(&binary.right);
            }
            syn::Expr::Paren(paren) => self.condition(&paren.expr),
            other => self.visit(other, Pending::Read),
        }
    }

    fn let_binding(&mut self, binding: &syn::ExprLet) {
        let value = self.eval(&binding.expr);
        let pending = if pattern_takes_unique(&binding.pat) {
            Pending::using(Effect::Access, "ref mut")
        } else {
            Pending::Read
        };
        self.visit(&binding.expr, pending);
        self.bind(&binding.pat, value.ty, value.origin, Mode::Move);
    }

    /// What `for x in <this>` binds `x` to.
    fn iterated_item(&self, ty: &Ty) -> Ty {
        match self.resolver.expand(ty.clone()) {
            Ty::Iter(item) => *item,
            Ty::Ref { mutable, inner } => match self.resolver.expand(*inner) {
                Ty::Slice(element) => Ty::reference(mutable, *element),
                Ty::Named(named) => match self.resolver.kind(&named) {
                    Kind::Std(Std::Vec | Std::VecDeque | Std::Set) => {
                        Ty::reference(mutable, first_arg(&named))
                    }
                    Kind::Std(Std::Option) => Ty::reference(mutable, first_arg(&named)),
                    Kind::Std(Std::Map) => Ty::Tuple(vec![
                        Ty::reference(false, first_arg(&named)),
                        Ty::reference(
                            mutable,
                            named
                                .args
                                .get(1)
                                .cloned()
                                .unwrap_or_else(|| Ty::unknown("a map without its value type")),
                        ),
                    ]),
                    _ => Ty::unknown(format!("iterating `&{}`", named.name)),
                },
                other => Ty::unknown(format!("iterating {}", other.spelling())),
            },
            Ty::Named(named) => match self.resolver.kind(&named) {
                Kind::Std(Std::Vec | Std::VecDeque | Std::Set | Std::Option) => first_arg(&named),
                _ => Ty::unknown(format!("iterating `{}`", named.name)),
            },
            other => Ty::unknown(format!("iterating {}", other.spelling())),
        }
    }

    fn closure(&mut self, closure: &syn::ExprClosure, params: &[Ty], origin: Option<Origin>) {
        self.scopes.push(BTreeMap::new());
        for (at, input) in closure.inputs.iter().enumerate() {
            let ty = if let syn::Pat::Type(typed) = input {
                self.written.ty(&typed.ty)
            } else {
                params
                    .get(at)
                    .cloned()
                    .unwrap_or_else(|| Ty::unknown("a closure parameter without a type"))
            };
            self.bind(input, ty, inexact(origin), Mode::Move);
        }
        self.visit(&closure.body, Pending::Read);
        self.scopes.pop();
    }

    fn method_call(&mut self, call: &syn::ExprMethodCall, pending: Pending) {
        let receiver = self.eval(&call.receiver);
        let name = call.method.to_string();
        let found = self.resolver.method(&receiver.ty, receiver.shared, &name);
        for (at, arg) in call.args.iter().enumerate() {
            match arg {
                syn::Expr::Closure(closure) => {
                    let params = found
                        .as_ref()
                        .map(|found| found.method.closure_at(at))
                        .unwrap_or_default();
                    self.closure(closure, &params, receiver.origin);
                }
                other => self.visit(other, Pending::Read),
            }
        }
        let kind = format!("call `{name}`");
        let next = match found {
            Ok(found) => {
                let kind = format!("call:{name}");
                match found.method.class {
                    Class::Write if found.on_four => Pending::Attributed,
                    Class::Write if found.shared => Pending::using(
                        Effect::Unknown(
                            "it writes, and the receiver is reached through a shared reference"
                                .to_owned(),
                        ),
                        format!("call `{name}`"),
                    ),
                    Class::Write => Pending::using(Effect::Write, kind),
                    Class::Access => match pending {
                        use_ @ Pending::Use(_) => use_,
                        _ if found.on_four => Pending::Attributed,
                        _ => Pending::using(Effect::Access, kind),
                    },
                    Class::Pass => pending,
                    Class::Read => Pending::Read,
                    Class::Inner => Pending::using(Effect::Inner, kind),
                }
            }
            // Behind a shared reference only inner mutability could change
            // it, and inner mutability is a method of a type the fixed list
            // knows; on one of the four the call's writes are in its own body.
            Err(unresolved) if unresolved.shared || self.resolver.is_four(&receiver.ty) => {
                Pending::Read
            }
            Err(unresolved) => Pending::using(Effect::Unknown(unresolved.why), kind),
        };
        self.visit(&call.receiver, next);
    }

    fn call(&mut self, call: &syn::ExprCall) {
        self.visit(&call.func, Pending::Read);
        let swaps = mem_function(&call.func);
        let closures = self.callee(call).1;
        for (at, arg) in call.args.iter().enumerate() {
            match (&swaps, arg) {
                (Some(name), syn::Expr::Reference(reference)) if reference.mutability.is_some() => {
                    self.visit(
                        &reference.expr,
                        Pending::using(Effect::Write, format!("call:mem::{name}")),
                    );
                }
                (_, syn::Expr::Closure(closure)) => {
                    let params = closures.get(at).cloned().flatten().unwrap_or_default();
                    self.closure(closure, &params, None);
                }
                _ => self.visit(arg, Pending::Read),
            }
        }
    }

    /// A macro's arguments, read as expressions when they are expressions; the
    /// tokens of one that is not are scanned for a write-shaped field of the
    /// four, which is listed unknown.
    fn macro_body(&mut self, mac: &syn::Macro) {
        if let Ok(arguments) =
            mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        {
            for argument in &arguments {
                match argument {
                    // `format!("{x}", x = value)`: a named argument, not an
                    // assignment.
                    syn::Expr::Assign(named) if matches!(&*named.left, syn::Expr::Path(_)) => {
                        self.visit(&named.right, Pending::Read);
                    }
                    other => self.visit(other, Pending::Read),
                }
            }
            return;
        }
        if let Ok((value, guard)) = mac.parse_body_with(matches_body) {
            self.visit(&value, Pending::Read);
            if let Some(guard) = guard {
                self.visit(&guard, Pending::Read);
            }
            return;
        }
        if let Ok((value, length)) = mac.parse_body_with(repeat_body) {
            self.visit(&value, Pending::Read);
            self.visit(&length, Pending::Read);
            return;
        }
        let mut tokens = Vec::new();
        flatten(mac.tokens.clone(), &mut tokens);
        for at in 0..tokens.len() {
            let proc_macro2::TokenTree::Punct(dot) = &tokens[at] else {
                continue;
            };
            if dot.as_char() != '.' {
                continue;
            }
            let Some(proc_macro2::TokenTree::Ident(name)) = tokens.get(at + 1) else {
                continue;
            };
            let name = name.to_string();
            if !self.resolver.subject.names.contains(&name) {
                continue;
            }
            if assignment_follows(&tokens[at + 2..]) {
                self.sink.unknown(
                    format!("?.{name}"),
                    &self.module,
                    &self.function,
                    "an assignment inside a macro whose arguments are not expressions".to_owned(),
                );
            }
        }
    }
}

fn matches_body(input: syn::parse::ParseStream<'_>) -> syn::Result<(syn::Expr, Option<syn::Expr>)> {
    let value: syn::Expr = input.parse()?;
    input.parse::<syn::Token![,]>()?;
    let _pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.peek(syn::Token![if]) {
        input.parse::<syn::Token![if]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    if input.peek(syn::Token![,]) {
        input.parse::<syn::Token![,]>()?;
    }
    if !input.is_empty() {
        return Err(input.error("more after the pattern"));
    }
    Ok((value, guard))
}

fn repeat_body(input: syn::parse::ParseStream<'_>) -> syn::Result<(syn::Expr, syn::Expr)> {
    let value: syn::Expr = input.parse()?;
    input.parse::<syn::Token![;]>()?;
    let length: syn::Expr = input.parse()?;
    if !input.is_empty() {
        return Err(input.error("more after the length"));
    }
    Ok((value, length))
}

fn flatten(stream: proc_macro2::TokenStream, out: &mut Vec<proc_macro2::TokenTree>) {
    for tree in stream {
        match tree {
            proc_macro2::TokenTree::Group(group) => flatten(group.stream(), out),
            other => out.push(other),
        }
    }
}

/// `=` alone, or an operator glued to `=` (`+=`, `<<=`), and not `==` or `=>`.
fn assignment_follows(rest: &[proc_macro2::TokenTree]) -> bool {
    let mut at = 0;
    while let Some(proc_macro2::TokenTree::Punct(punct)) = rest.get(at) {
        let joint = punct.spacing() == proc_macro2::Spacing::Joint;
        if punct.as_char() == '=' {
            if joint {
                return false;
            }
            return at > 0 || !joint;
        }
        if !joint || !"+-*/%^&|<>".contains(punct.as_char()) {
            return false;
        }
        at += 1;
    }
    false
}

fn mem_function(func: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let [.., module, name] = segments.as_slice() else {
        return None;
    };
    let from_std = segments.len() == 2 || matches!(segments[0].as_str(), "std" | "core");
    (module == "mem" && from_std && matches!(name.as_str(), "take" | "replace" | "swap"))
        .then(|| name.clone())
}

fn member_name(member: &syn::Member) -> String {
    match member {
        syn::Member::Named(ident) => ident.to_string(),
        syn::Member::Unnamed(at) => at.index.to_string(),
    }
}

fn is_compound_assignment(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

/// Whether a pattern binds anything `ref mut`.
fn pattern_takes_unique(pattern: &syn::Pat) -> bool {
    match pattern {
        syn::Pat::Ident(ident) => {
            (ident.by_ref.is_some() && ident.mutability.is_some())
                || ident
                    .subpat
                    .as_ref()
                    .is_some_and(|(_, sub)| pattern_takes_unique(sub))
        }
        syn::Pat::Tuple(tuple) => tuple.elems.iter().any(pattern_takes_unique),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().any(pattern_takes_unique),
        syn::Pat::Struct(structure) => structure
            .fields
            .iter()
            .any(|field| pattern_takes_unique(&field.pat)),
        syn::Pat::Reference(reference) => pattern_takes_unique(&reference.pat),
        syn::Pat::Type(typed) => pattern_takes_unique(&typed.pat),
        syn::Pat::Paren(paren) => pattern_takes_unique(&paren.pat),
        syn::Pat::Or(or) => or.cases.iter().any(pattern_takes_unique),
        syn::Pat::Slice(slice) => slice.elems.iter().any(pattern_takes_unique),
        _ => false,
    }
}

/// Bind generic parameters from an argument's type: the parameter itself, or
/// behind references, or the element of a slice or a `Vec`.
fn unify(param: &Ty, arg: &Ty, generics: &[String], map: &mut BTreeMap<String, Ty>) {
    match (param, arg) {
        (Ty::Param(name), _) if generics.contains(name) => {
            if !matches!(arg, Ty::Unknown(_)) {
                map.entry(name.clone()).or_insert_with(|| arg.clone());
            }
        }
        (Ty::Ref { inner: param, .. }, Ty::Ref { inner: arg, .. }) => {
            unify(param, arg, generics, map)
        }
        (Ty::Slice(param), Ty::Slice(arg)) => unify(param, arg, generics, map),
        (Ty::Slice(param), Ty::Named(named)) if named.name == "Vec" => {
            if let Some(element) = named.args.first() {
                unify(param, element, generics, map);
            }
        }
        (Ty::Named(param), Ty::Named(arg)) if param.name == arg.name => {
            for (param, arg) in param.args.iter().zip(&arg.args) {
                unify(param, arg, generics, map);
            }
        }
        _ => {}
    }
}

fn last_segment(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}
