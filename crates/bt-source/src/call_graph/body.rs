//! **One body, walked**: every call site in it, each turned into its edges by the rules of
//! [`super::decls`]. One pass, top-down: an expression is visited once, and the visit answers its
//! type as far as declarations say, so a receiver is typed by what was bound before it and a
//! shadowing binding retypes the name (the scope stack). A closure's body, a nested `fn`'s and the
//! arguments of a macro that parse as expressions are walked as part of the item they are written
//! in, and the edges found there carry the closure or the macro.

use std::collections::BTreeMap;

use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

use super::decls::{Answer, Tables, trait_self};
use super::names::Named;
use super::{CallSite, Edge, ItemId, Node, Target, Unresolved, ValueId, Via};
use crate::census::types::{Receiver, Std, Ty, Written, option_of, std_named};
use crate::declarations::cfg_predicates;
use crate::index::{ConditionalVariant, DeclarationPath, ItemKind, Span};

/// One lexical scope: its bindings, the `fn` items written in it, and its `use` items.
#[derive(Default)]
struct Scope {
    bindings: BTreeMap<String, Ty>,
    /// The `const`s and `static`s the block declares: items, so a nested `fn` sees them too.
    values: BTreeMap<String, Ty>,
    nested: BTreeMap<String, Span>,
    names: Vec<(String, Vec<String>)>,
    globs: Vec<Vec<String>>,
}

/// The default binding mode a pattern is matched in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Move,
    Shared,
    Unique,
}

pub(crate) struct Walker<'t, 'i> {
    tables: &'t Tables<'i>,
    from: Node,
    /// What the body's own declaration stands on: a statement's `#[cfg]` is read on top of it.
    arm_declared: &'i [DeclarationPath],
    arm_variant: &'i ConditionalVariant,
    arm_offset: usize,
    at: usize,
    package: String,
    module: String,
    written: Written,
    scopes: Vec<Scope>,
    closures: Vec<Span>,
    macros: Vec<String>,
    /// Inside a statement or a match arm the arms rule out.
    dead: usize,
    /// The union offset of the parsed text, and the text.
    base: usize,
    text: &'i str,
    pub(crate) edges: Vec<Edge>,
}

impl<'t, 'i> Walker<'t, 'i> {
    /// **Every edge of one callable item's body.** A body syn does not parse is one
    /// [`Unresolved::Unparsed`] edge.
    pub(crate) fn item(tables: &'t Tables<'i>, from: ItemId) -> Vec<Edge> {
        let record = tables.record(from);
        let index = tables.index(from.package());
        let package = tables.package(from.package()).to_owned();
        let module = record
            .module_paths()
            .first()
            .map_or_else(|| "crate".to_owned(), |module| (*module).to_owned());
        let text = index.text(record.whole());
        let base = record.whole().start();
        let mut walker = Walker {
            tables,
            from: Node::Item(from),
            arm_declared: record.declaration_paths(),
            arm_variant: record.variant(),
            arm_offset: record.whole().start(),
            at: from.package(),
            package: package.clone(),
            module: module.clone(),
            written: tables.written(&package, &module),
            scopes: vec![Scope::default()],
            closures: Vec::new(),
            macros: Vec::new(),
            dead: 0,
            base,
            text,
            edges: Vec::new(),
        };
        let parsed =
            match record.kind() {
                ItemKind::Function => syn::parse_str::<syn::ItemFn>(text)
                    .map(|parsed| (parsed.sig, Some(*parsed.block))),
                _ if record.type_owner().is_some() => syn::parse_str::<syn::ImplItemFn>(text)
                    .map(|parsed| (parsed.sig, Some(parsed.block))),
                _ => syn::parse_str::<syn::TraitItemFn>(text)
                    .map(|parsed| (parsed.sig, parsed.default)),
            };
        let (signature, block) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                walker.edges.push(Edge {
                    from: Node::Item(from),
                    site: CallSite {
                        span: record.whole(),
                        name: record.name().to_owned(),
                        closure: None,
                        macro_arg: None,
                    },
                    to: Target::Unresolved(Unresolved::Unparsed {
                        why: error.to_string(),
                    }),
                    via: Via::Direct,
                });
                return walker.edges;
            }
        };
        let Some(block) = block else {
            return Vec::new();
        };
        if record.type_owner().is_some() {
            if let Some(header) = crate::census::types::impl_of(index, record)
                .and_then(|block| crate::census::types::impl_header(index, block))
            {
                walker.written.enter(&header.generics);
                walker.written.self_ty = Some(walker.written.ty(&header.self_ty));
            }
        } else if let Some(id) = tables.trait_of_item.get(&from) {
            walker.written.self_ty = Some(trait_self(id));
            if let Some(Ty::Traits(bounds)) = &walker.written.self_ty {
                walker
                    .written
                    .trait_bounds
                    .insert("Self".to_owned(), bounds.clone());
            }
        }
        walker.function(&signature, &block);
        walker.edges
    }

    /// **Every edge of one `const`'s or `static`'s initializer**: the expression walked as a body
    /// in the module, the `impl` block or the trait it is written in. A trait's `const` with no
    /// default has none; an initializer `syn` does not parse is one [`Unresolved::Unparsed`] edge.
    pub(crate) fn value(tables: &'t Tables<'i>, id: ValueId) -> Vec<Edge> {
        let index = tables.index(id.package());
        let record = &index.values()[id.value()];
        let package = tables.package(id.package()).to_owned();
        let start = record.whole().start();
        let module = record
            .module_paths()
            .first()
            .map_or_else(|| "crate".to_owned(), |module| (*module).to_owned());
        let text = index.text(record.whole());
        let mut walker = Walker {
            tables,
            from: Node::Value(id),
            arm_declared: &record.declared,
            arm_variant: record.variant(),
            arm_offset: start,
            at: id.package(),
            package: package.clone(),
            module: module.clone(),
            written: tables.written(&package, &module),
            scopes: vec![Scope::default()],
            closures: Vec::new(),
            macros: Vec::new(),
            dead: 0,
            base: start,
            text,
            edges: Vec::new(),
        };
        let expr = if record.type_owner().is_some() {
            if let Some(block) = index.impls().iter().find(|block| block.body().holds(start))
                && let Some(header) = crate::census::types::impl_header(index, block)
            {
                walker.written.enter(&header.generics);
                walker.written.self_ty = Some(walker.written.ty(&header.self_ty));
            }
            syn::parse_str::<syn::ImplItem>(text).map(|parsed| match parsed {
                syn::ImplItem::Const(constant) => Some(constant.expr),
                _ => None,
            })
        } else if record.trait_name().is_some() {
            if let Some(declaration) = index
                .traits()
                .iter()
                .find(|declaration| declaration.body().holds(start))
                && let Some(trait_id) = tables
                    .traits
                    .identities(&package, declaration)
                    .into_iter()
                    .next()
            {
                walker.written.self_ty = Some(trait_self(&trait_id));
            }
            syn::parse_str::<syn::TraitItem>(text).map(|parsed| match parsed {
                syn::TraitItem::Const(constant) => constant.default.map(|(_, expr)| expr),
                _ => None,
            })
        } else {
            syn::parse_str::<syn::Item>(text).map(|parsed| match parsed {
                syn::Item::Const(constant) => Some(*constant.expr),
                syn::Item::Static(value) => Some(*value.expr),
                _ => None,
            })
        };
        match expr {
            Ok(Some(expr)) => {
                walker.expr(&expr);
            }
            Ok(None) => {}
            Err(error) => walker.edges.push(Edge {
                from: Node::Value(id),
                site: CallSite {
                    span: record.whole(),
                    name: record.name().to_owned(),
                    closure: None,
                    macro_arg: None,
                },
                to: Target::Unresolved(Unresolved::Unparsed {
                    why: error.to_string(),
                }),
                via: Via::Direct,
            }),
        }
        walker.edges
    }

    fn function(&mut self, signature: &syn::Signature, block: &syn::Block) {
        let saved = self.written.clone();
        self.written.enter(&signature.generics);
        self.scopes.push(Scope::default());
        for input in &signature.inputs {
            match input {
                syn::FnArg::Receiver(_) => {
                    let self_ty = self
                        .written
                        .self_ty
                        .clone()
                        .unwrap_or_else(|| Ty::unknown("`self` outside an impl"));
                    let ty = match Receiver::of(signature) {
                        Receiver::Shared => Ty::reference(false, self_ty),
                        Receiver::Unique => Ty::reference(true, self_ty),
                        Receiver::Value | Receiver::None => self_ty,
                    };
                    self.define("self", ty);
                }
                syn::FnArg::Typed(typed) => {
                    let ty = self.written.ty(&typed.ty);
                    self.bind(&typed.pat, ty, Mode::Move);
                }
            }
        }
        self.block(block);
        self.scopes.pop();
        self.written = saved;
    }

    // ── edges ─────────────────────────────────────────────────────────────

    fn span_of(&self, span: proc_macro2::Span) -> Span {
        let range = span.byte_range();
        let at = |offset: usize| u32::try_from(self.base + offset).unwrap_or(u32::MAX);
        Span::new(at(range.start), at(range.end.max(range.start)))
    }

    fn push(&mut self, span: Span, name: &str, target: Target, via: Via) {
        let target = if self.dead > 0 {
            match target {
                Target::Item(id) => Target::Unresolved(Unresolved::OtherArm {
                    candidates: vec![id],
                }),
                Target::Unresolved(Unresolved::OtherArm { candidates }) => {
                    Target::Unresolved(Unresolved::OtherArm { candidates })
                }
                _ => Target::Unresolved(Unresolved::OtherArm {
                    candidates: Vec::new(),
                }),
            }
        } else {
            target
        };
        self.edges.push(Edge {
            from: self.from,
            site: CallSite {
                span,
                name: name.to_owned(),
                closure: self.closures.last().copied(),
                macro_arg: self.macros.last().cloned(),
            },
            to: target,
            via,
        });
    }

    fn answer(&mut self, span: Span, name: &str, answer: &Answer) {
        for (target, via) in &answer.targets {
            self.push(span, name, target.clone(), via.clone());
        }
    }

    // ── scopes ────────────────────────────────────────────────────────────

    fn define(&mut self, name: &str, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name.to_owned(), ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<&Ty> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.bindings.get(name).or_else(|| scope.values.get(name)))
    }

    fn nested(&self, name: &str) -> Option<Span> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.nested.get(name).copied())
    }

    /// The items a block declares are in scope through all of it: its `fn`s and its `use`s.
    fn hoist(&mut self, block: &syn::Block) {
        for statement in &block.stmts {
            let syn::Stmt::Item(item) = statement else {
                continue;
            };
            match item {
                syn::Item::Fn(function) => {
                    let span = self.span_of(function.span());
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.nested.insert(function.sig.ident.to_string(), span);
                    }
                }
                syn::Item::Const(constant) => {
                    let ty = self.written.ty(&constant.ty);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.values.insert(constant.ident.to_string(), ty);
                    }
                }
                syn::Item::Static(value) => {
                    let ty = self.written.ty(&value.ty);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.values.insert(value.ident.to_string(), ty);
                    }
                }
                syn::Item::Use(used) => {
                    let mut names = Vec::new();
                    let mut globs = Vec::new();
                    flatten_use(&used.tree, &mut Vec::new(), &mut names, &mut globs);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.names.extend(names);
                        scope.globs.extend(globs);
                    }
                }
                _ => {}
            }
        }
    }

    /// A path with a body `use` item's name in front rewritten to what it names, and the globs
    /// of the body's `use` items in scope.
    fn body_path(&self, path: &[String]) -> (Vec<String>, Vec<Vec<String>>) {
        let mut globs = Vec::new();
        for scope in self.scopes.iter().rev() {
            if let Some(first) = path.first()
                && let Some((_, named)) = scope.names.iter().find(|(alias, _)| alias == first)
            {
                let mut rewritten = named.clone();
                rewritten.extend(path[1..].iter().cloned());
                return (rewritten, globs);
            }
            globs.extend(scope.globs.iter().cloned());
        }
        (path.to_vec(), globs)
    }

    // ── statements ────────────────────────────────────────────────────────

    /// Whether the arms rule out code under these attributes.
    fn ruled_out(&self, attrs: &[syn::Attribute]) -> bool {
        let (predicates, compilation) = cfg_predicates(attrs, self.text);
        if predicates.is_empty() {
            return false;
        }
        let index = self.tables.index(self.at);
        let Some(file) = index.file_at(self.arm_offset) else {
            return false;
        };
        let admitted = self.arm_declared.iter().any(|path| {
            let all: Vec<String> = path
                .predicates()
                .iter()
                .chain(self.arm_variant.predicates())
                .chain(&predicates)
                .cloned()
                .collect();
            self.tables_arms_admit(
                file,
                &all,
                path.compilation()
                    .and(self.arm_variant.compilation())
                    .and(compilation),
            )
        });
        !admitted
    }

    fn tables_arms_admit(
        &self,
        file: &crate::index::FileRecord,
        predicates: &[String],
        compilation: crate::Compilation,
    ) -> bool {
        self.tables.arms.admits(&super::ArmQuery {
            package: &self.package,
            file,
            predicates,
            compilation,
        })
    }

    fn block(&mut self, block: &syn::Block) -> Ty {
        self.scopes.push(Scope::default());
        self.hoist(block);
        let mut last = Ty::Tuple(Vec::new());
        for (at, statement) in block.stmts.iter().enumerate() {
            let ty = self.statement(statement);
            if at + 1 == block.stmts.len() && matches!(statement, syn::Stmt::Expr(_, None)) {
                last = ty;
            }
        }
        self.scopes.pop();
        last
    }

    fn guarded<T>(&mut self, attrs: &[syn::Attribute], walk: impl FnOnce(&mut Self) -> T) -> T {
        let out = self.ruled_out(attrs);
        if out {
            self.dead += 1;
        }
        let answer = walk(self);
        if out {
            self.dead -= 1;
        }
        answer
    }

    fn statement(&mut self, statement: &syn::Stmt) -> Ty {
        match statement {
            syn::Stmt::Local(local) => {
                self.guarded(&local.attrs, |walker| {
                    let ty = match &local.init {
                        Some(init) => {
                            let ty = walker.expr(&init.expr);
                            if let Some((_, otherwise)) = &init.diverge {
                                walker.expr(otherwise);
                            }
                            ty
                        }
                        None => Ty::unknown("a `let` without a value"),
                    };
                    walker.bind(&local.pat, ty, Mode::Move);
                });
                Ty::Tuple(Vec::new())
            }
            syn::Stmt::Item(item) => {
                self.nested_item(item);
                Ty::Tuple(Vec::new())
            }
            syn::Stmt::Expr(expr, _) => {
                let attrs = expr_attrs(expr).to_vec();
                self.guarded(&attrs, |walker| walker.expr(expr))
            }
            syn::Stmt::Macro(statement) => {
                self.guarded(&statement.attrs, |walker| walker.mac(&statement.mac));
                Ty::unknown("a macro")
            }
        }
    }

    /// An item written inside a body: walked with the items and `use`s of the enclosing blocks in
    /// scope and none of their bindings, counted against the item it is written in.
    fn nested_item(&mut self, item: &syn::Item) {
        // An item sees the items and the `use`s of the blocks around it, and none of their
        // bindings.
        let items_only: Vec<Scope> = self
            .scopes
            .iter()
            .map(|scope| Scope {
                bindings: BTreeMap::new(),
                values: scope.values.clone(),
                nested: scope.nested.clone(),
                names: scope.names.clone(),
                globs: scope.globs.clone(),
            })
            .collect();
        let saved_scopes = std::mem::replace(&mut self.scopes, items_only);
        let saved = self.written.clone();
        match item {
            syn::Item::Fn(function) => {
                self.guarded(&function.attrs, |walker| {
                    walker.written.self_ty = None;
                    walker.function(&function.sig, &function.block);
                });
            }
            syn::Item::Impl(block) => {
                self.guarded(&block.attrs, |walker| {
                    walker.written.enter(&block.generics);
                    walker.written.self_ty = Some(walker.written.ty(&block.self_ty));
                    for member in &block.items {
                        if let syn::ImplItem::Fn(function) = member {
                            walker.function(&function.sig, &function.block);
                        }
                    }
                });
            }
            syn::Item::Const(constant) => {
                self.expr(&constant.expr);
            }
            syn::Item::Static(value) => {
                self.expr(&value.expr);
            }
            syn::Item::Macro(invocation) => {
                if !invocation.mac.path.is_ident("macro_rules") {
                    self.mac(&invocation.mac);
                }
            }
            _ => {}
        }
        self.scopes = saved_scopes;
        self.written = saved;
    }

    // ── patterns ──────────────────────────────────────────────────────────

    fn peel(&self, ty: Ty, mut mode: Mode) -> (Ty, Mode) {
        let mut ty = self.tables.expand(ty).unwrap_or_else(Ty::Unknown);
        while let Ty::Ref { mutable, inner } = ty {
            mode = match (mode, mutable) {
                (Mode::Move | Mode::Unique, true) => Mode::Unique,
                (Mode::Move | Mode::Unique, false) | (Mode::Shared, _) => Mode::Shared,
            };
            ty = self.tables.expand(*inner).unwrap_or_else(Ty::Unknown);
        }
        (ty, mode)
    }

    fn bind(&mut self, pattern: &syn::Pat, ty: Ty, mode: Mode) {
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
                self.define(&ident.ident.to_string(), bound);
                if let Some((_, sub)) = &ident.subpat {
                    self.bind(sub, ty, mode);
                }
            }
            syn::Pat::Reference(reference) => {
                let inner = match self.tables.expand(ty).unwrap_or_else(Ty::Unknown) {
                    Ty::Ref { inner, .. } => *inner,
                    _ => Ty::unknown("a reference pattern over a type that is not a reference"),
                };
                self.bind(&reference.pat, inner, Mode::Move);
            }
            syn::Pat::Type(typed) => {
                let written = self.written.ty(&typed.ty);
                self.bind(&typed.pat, written, mode);
            }
            syn::Pat::Paren(paren) => self.bind(&paren.pat, ty, mode),
            syn::Pat::Or(or) => {
                for case in &or.cases {
                    self.bind(case, ty.clone(), mode);
                }
            }
            syn::Pat::Tuple(tuple) => {
                let (ty, mode) = self.peel(ty, mode);
                let items = match ty {
                    Ty::Tuple(items) if items.len() == tuple.elems.len() => items,
                    _ => Vec::new(),
                };
                for (at, element) in tuple.elems.iter().enumerate() {
                    let item = items
                        .get(at)
                        .cloned()
                        .unwrap_or_else(|| Ty::unknown("a tuple pattern the rules do not type"));
                    self.bind(element, item, mode);
                }
            }
            syn::Pat::TupleStruct(tuple) => {
                let (ty, mode) = self.peel(ty, mode);
                let variant = last_segment(&tuple.path);
                let fields = self.tables.variant_fields(&ty, &variant);
                for (at, element) in tuple.elems.iter().enumerate() {
                    let item = fields
                        .as_ref()
                        .and_then(|fields| fields.get(at))
                        .map_or_else(
                            || Ty::unknown("a variant the rules do not find"),
                            |(_, ty)| ty.clone(),
                        );
                    self.bind(element, item, mode);
                }
            }
            syn::Pat::Struct(structure) => {
                let (ty, mode) = self.peel(ty, mode);
                let variant = last_segment(&structure.path);
                let fields = self.tables.variant_fields(&ty, &variant);
                for field in &structure.fields {
                    let name = member_name(&field.member);
                    let item = fields
                        .as_ref()
                        .and_then(|fields| fields.iter().find(|(declared, _)| *declared == name))
                        .map_or_else(
                            || Ty::unknown("a field the rules do not find"),
                            |(_, ty)| ty.clone(),
                        );
                    self.bind(&field.pat, item, mode);
                }
            }
            syn::Pat::Slice(slice) => {
                let (ty, mode) = self.peel(ty, mode);
                let element = match &ty {
                    Ty::Slice(element) => (**element).clone(),
                    other => match self.tables.std_of(other) {
                        Some((Std::Vec | Std::VecDeque, args)) => args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Ty::unknown("an element")),
                        _ => Ty::unknown("a slice pattern over a type that is not a slice"),
                    },
                };
                for item in &slice.elems {
                    self.bind(item, element.clone(), mode);
                }
            }
            _ => {}
        }
    }

    // ── expressions ───────────────────────────────────────────────────────

    fn condition(&mut self, expr: &syn::Expr) {
        match expr {
            syn::Expr::Let(binding) => {
                let ty = self.expr(&binding.expr);
                self.bind(&binding.pat, ty, Mode::Move);
            }
            syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.condition(&binary.left);
                self.condition(&binary.right);
            }
            other => {
                self.expr(other);
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn expr(&mut self, expr: &syn::Expr) -> Ty {
        match expr {
            syn::Expr::Array(array) => {
                let mut element = None;
                for item in &array.elems {
                    let ty = self.expr(item);
                    element.get_or_insert(ty);
                }
                Ty::Slice(Box::new(
                    element.unwrap_or_else(|| Ty::unknown("an empty array")),
                ))
            }
            syn::Expr::Assign(assign) => {
                self.expr(&assign.left);
                self.expr(&assign.right);
                Ty::Tuple(Vec::new())
            }
            syn::Expr::Async(block) => {
                self.block(&block.block);
                Ty::unknown("a future")
            }
            syn::Expr::Await(wait) => {
                self.expr(&wait.base);
                Ty::unknown("what a future gives")
            }
            syn::Expr::Binary(binary) => {
                let left = self.expr(&binary.left);
                let right = self.expr(&binary.right);
                self.operated(&binary.op, left, right)
            }
            syn::Expr::Block(block) => self.block(&block.block),
            syn::Expr::Break(stop) => {
                if let Some(value) = &stop.expr {
                    self.expr(value);
                }
                Ty::unknown("a break")
            }
            syn::Expr::Call(call) => self.call(call),
            syn::Expr::Cast(cast) => {
                self.expr(&cast.expr);
                self.written.ty(&cast.ty)
            }
            syn::Expr::Closure(closure) => self.closure(closure, &[]),
            syn::Expr::Const(block) => self.block(&block.block),
            syn::Expr::Continue(_) | syn::Expr::Infer(_) => Ty::unknown("no value"),
            syn::Expr::Field(field) => {
                let base = self.expr(&field.base);
                let name = member_name(&field.member);
                let mut steps = Vec::new();
                let ty = self.tables.field(&base, &name, &mut steps);
                let span = self.span_of(field.member.span());
                for (target, via) in steps {
                    self.push(span, &name, target, via);
                }
                ty
            }
            syn::Expr::ForLoop(looping) => {
                let iterated = self.expr(&looping.expr);
                let item = self.iterated(&iterated);
                self.scopes.push(Scope::default());
                self.bind(&looping.pat, item, Mode::Move);
                self.block(&looping.body);
                self.scopes.pop();
                Ty::Tuple(Vec::new())
            }
            syn::Expr::Group(group) => self.expr(&group.expr),
            syn::Expr::If(branch) => {
                self.scopes.push(Scope::default());
                self.condition(&branch.cond);
                let then = self.block(&branch.then_branch);
                self.scopes.pop();
                match &branch.else_branch {
                    Some((_, otherwise)) => {
                        self.expr(otherwise);
                        then
                    }
                    None => Ty::Tuple(Vec::new()),
                }
            }
            syn::Expr::Index(index) => {
                let base = self.expr(&index.expr);
                let ranged = matches!(&*index.index, syn::Expr::Range(_));
                self.expr(&index.index);
                self.element(&base, ranged)
            }
            syn::Expr::Let(binding) => {
                let ty = self.expr(&binding.expr);
                self.bind(&binding.pat, ty, Mode::Move);
                Ty::unknown("a `let` condition")
            }
            syn::Expr::Lit(literal) => match &literal.lit {
                syn::Lit::Str(_) => Ty::reference(false, std_named("str", Vec::new())),
                syn::Lit::Bool(_) => std_named("bool", Vec::new()),
                syn::Lit::Char(_) => std_named("char", Vec::new()),
                syn::Lit::Int(_) => std_named("{integer}", Vec::new()),
                syn::Lit::Float(_) => std_named("{float}", Vec::new()),
                _ => Ty::unknown("a literal"),
            },
            syn::Expr::Loop(looping) => {
                self.block(&looping.body);
                Ty::unknown("a loop's value")
            }
            syn::Expr::Macro(invocation) => {
                let attrs = invocation.attrs.clone();
                self.guarded(&attrs, |walker| walker.mac(&invocation.mac));
                Ty::unknown("a macro")
            }
            syn::Expr::Match(matching) => {
                let scrutinee = self.expr(&matching.expr);
                for arm in &matching.arms {
                    self.guarded(&arm.attrs, |walker| {
                        walker.scopes.push(Scope::default());
                        walker.bind(&arm.pat, scrutinee.clone(), Mode::Move);
                        if let Some((_, guard)) = &arm.guard {
                            walker.condition(guard);
                        }
                        walker.expr(&arm.body);
                        walker.scopes.pop();
                    });
                }
                Ty::unknown("a match's value")
            }
            syn::Expr::MethodCall(call) => self.method_call(call),
            syn::Expr::Paren(paren) => self.expr(&paren.expr),
            syn::Expr::Path(path) => self.path_value(path),
            syn::Expr::Range(range) => {
                if let Some(start) = &range.start {
                    self.expr(start);
                }
                if let Some(end) = &range.end {
                    self.expr(end);
                }
                Ty::unknown("a range")
            }
            syn::Expr::RawAddr(raw) => {
                self.expr(&raw.expr);
                Ty::unknown("a raw pointer")
            }
            syn::Expr::Reference(reference) => {
                let inner = self.expr(&reference.expr);
                Ty::reference(reference.mutability.is_some(), inner)
            }
            syn::Expr::Repeat(repeat) => {
                let element = self.expr(&repeat.expr);
                self.expr(&repeat.len);
                Ty::Slice(Box::new(element))
            }
            syn::Expr::Return(ret) => {
                if let Some(value) = &ret.expr {
                    self.expr(value);
                }
                Ty::unknown("a return")
            }
            syn::Expr::Struct(structure) => {
                for field in &structure.fields {
                    self.expr(&field.expr);
                }
                if let Some(rest) = &structure.rest {
                    self.expr(rest);
                }
                if structure.qself.is_some() {
                    Ty::unknown("a qualified struct path")
                } else {
                    self.written.path(&structure.path)
                }
            }
            syn::Expr::Try(attempt) => {
                let inner = self.expr(&attempt.expr);
                let inner = self.tables.expand(inner).unwrap_or_else(Ty::Unknown);
                match self.tables.std_of(&inner) {
                    Some((Std::Option | Std::Result, args)) => args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Ty::unknown("what `?` gives")),
                    _ => Ty::unknown("what `?` gives"),
                }
            }
            syn::Expr::TryBlock(block) => {
                self.block(&block.block);
                Ty::unknown("a try block")
            }
            syn::Expr::Tuple(tuple) => {
                Ty::Tuple(tuple.elems.iter().map(|item| self.expr(item)).collect())
            }
            syn::Expr::Unary(unary) => {
                let inner = self.expr(&unary.expr);
                match unary.op {
                    syn::UnOp::Deref(star) => self.deref(inner, self.span_of(star.span)),
                    _ => Ty::unknown("an operator's value"),
                }
            }
            syn::Expr::Unsafe(block) => self.block(&block.block),
            syn::Expr::While(looping) => {
                self.scopes.push(Scope::default());
                self.condition(&looping.cond);
                self.block(&looping.body);
                self.scopes.pop();
                Ty::Tuple(Vec::new())
            }
            syn::Expr::Yield(value) => {
                if let Some(value) = &value.expr {
                    self.expr(value);
                }
                Ty::unknown("a yield")
            }
            other => {
                let span = self.span_of(other.span());
                self.push(
                    span,
                    "?",
                    Target::Unresolved(Unresolved::Path {
                        written: "an expression".to_owned(),
                        why: "an expression form the engine does not read (kept as tokens)"
                            .to_owned(),
                    }),
                    Via::Direct,
                );
                Ty::unknown("an expression the engine does not read")
            }
        }
    }

    /// What a binary operator gives: a comparison or a logical operator a `bool`; arithmetic on
    /// a primitive number (or a literal of one) that number's type. Arithmetic on any other type
    /// runs its operator impl, an implicit call ([`super::Reach::implicit`]), and is not typed.
    fn operated(&self, op: &syn::BinOp, left: Ty, right: Ty) -> Ty {
        use syn::BinOp;
        match op {
            BinOp::Eq(_)
            | BinOp::Ne(_)
            | BinOp::Lt(_)
            | BinOp::Le(_)
            | BinOp::Gt(_)
            | BinOp::Ge(_)
            | BinOp::And(_)
            | BinOp::Or(_) => std_named("bool", Vec::new()),
            _ => {
                let primitive = |ty: &Ty| match self.tables.expand(ty.clone()) {
                    Ok(Ty::Named(named)) if super::decls::is_primitive(&named) => {
                        Some(Ty::Named(named))
                    }
                    _ => None,
                };
                primitive(&left)
                    .filter(|ty| !matches!(ty, Ty::Named(named) if named.name.starts_with('{')))
                    .or_else(|| primitive(&right))
                    .or_else(|| primitive(&left))
                    .unwrap_or_else(|| Ty::unknown("an operator's value"))
            }
        }
    }

    /// `*value`: a reference's pointee, a `Box`'s or an `Rc`'s, or a first-party `Deref`'s
    /// target, whose `deref` is an edge.
    fn deref(&mut self, value: Ty, span: Span) -> Ty {
        let value = self.tables.expand(value).unwrap_or_else(Ty::Unknown);
        match &value {
            Ty::Ref { inner, .. } => (**inner).clone(),
            Ty::Named(named) => {
                if let Some((Std::Box | Std::Shared, args)) = self.tables.std_of(&value) {
                    return args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Ty::unknown("a pointee"));
                }
                if let super::decls::Placed::Foreign(path) = self.tables.place(named)
                    && super::decls::is_std_guard(&path)
                {
                    return named
                        .args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Ty::unknown("a pointee"));
                }
                if let super::decls::Placed::Types(keys) = self.tables.place(named)
                    && let [key] = keys.as_slice()
                    && let Some((target, steps)) = self
                        .tables
                        .deref_of(&super::decls::SelfKey::Type(key.clone()), &named.args)
                {
                    for (target, via) in steps {
                        self.push(span, "*", target, via);
                    }
                    return target;
                }
                Ty::unknown(format!("`*` on `{}`", named.name))
            }
            other => Ty::unknown(format!("`*` on {}", other.spelling())),
        }
    }

    fn element(&self, base: &Ty, ranged: bool) -> Ty {
        let mut ty = base.clone();
        for _ in 0..super::decls::DEREF_STEPS {
            ty = self.tables.expand(ty).unwrap_or_else(Ty::Unknown);
            match ty {
                Ty::Ref { inner, .. } => ty = *inner,
                Ty::Slice(element) => {
                    return if ranged { Ty::Slice(element) } else { *element };
                }
                Ty::Named(_) => match self.tables.std_of(&ty) {
                    Some((Std::Vec | Std::VecDeque, args)) => {
                        let element = args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Ty::unknown("an element"));
                        return if ranged {
                            Ty::Slice(Box::new(element))
                        } else {
                            element
                        };
                    }
                    Some((Std::Map, args)) => {
                        return args
                            .get(1)
                            .cloned()
                            .unwrap_or_else(|| Ty::unknown("a map's value"));
                    }
                    Some((Std::Box | Std::Shared, args)) => {
                        ty = args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Ty::unknown("a pointee"));
                    }
                    _ => return Ty::unknown("indexing a type the rules do not read"),
                },
                _ => return Ty::unknown("indexing a type the rules do not read"),
            }
        }
        Ty::unknown("a deref chain longer than the limit")
    }

    fn iterated(&self, ty: &Ty) -> Ty {
        let ty = self.tables.expand(ty.clone()).unwrap_or_else(Ty::Unknown);
        match ty {
            Ty::Iter(item) => *item,
            Ty::Slice(element) => *element,
            Ty::Ref { mutable, inner } => {
                let inner = self.tables.expand(*inner).unwrap_or_else(Ty::Unknown);
                match &inner {
                    Ty::Slice(element) => Ty::reference(mutable, (**element).clone()),
                    _ => match self.tables.std_of(&inner) {
                        Some((Std::Vec | Std::VecDeque | Std::Set | Std::Option, args)) => {
                            Ty::reference(
                                mutable,
                                args.first()
                                    .cloned()
                                    .unwrap_or_else(|| Ty::unknown("an item")),
                            )
                        }
                        Some((Std::Map, args)) => Ty::Tuple(vec![
                            Ty::reference(
                                false,
                                args.first()
                                    .cloned()
                                    .unwrap_or_else(|| Ty::unknown("a key")),
                            ),
                            Ty::reference(
                                mutable,
                                args.get(1)
                                    .cloned()
                                    .unwrap_or_else(|| Ty::unknown("a value")),
                            ),
                        ]),
                        _ => Ty::unknown("iterating a type the rules do not read"),
                    },
                }
            }
            other => match self.tables.std_of(&other) {
                Some((Std::Vec | Std::VecDeque | Std::Set | Std::Option, args)) => args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Ty::unknown("an item")),
                _ => Ty::unknown("iterating a type the rules do not read"),
            },
        }
    }

    fn closure(&mut self, closure: &syn::ExprClosure, params: &[Ty]) -> Ty {
        let span = self.span_of(closure.span());
        self.scopes.push(Scope::default());
        let mut types = Vec::new();
        for (at, input) in closure.inputs.iter().enumerate() {
            let ty = params
                .get(at)
                .cloned()
                .unwrap_or_else(|| Ty::unknown("a closure parameter without a type"));
            types.push(ty.clone());
            self.bind(input, ty, Mode::Move);
        }
        self.closures.push(span);
        self.expr(&closure.body);
        self.closures.pop();
        self.scopes.pop();
        Ty::Fn(types)
    }

    fn arguments<'a>(
        &mut self,
        args: impl Iterator<Item = &'a syn::Expr>,
        answer: &Answer,
    ) -> Vec<Ty> {
        args.enumerate()
            .map(|(at, arg)| match arg {
                syn::Expr::Closure(closure) => {
                    let params = answer.closure_at(at);
                    self.closure(closure, &params)
                }
                other => self.expr(other),
            })
            .collect()
    }

    fn method_call(&mut self, call: &syn::ExprMethodCall) -> Ty {
        let receiver = self.expr(&call.receiver);
        let name = call.method.to_string();
        let span = self.span_of(call.method.span());
        let answer = self.tables.method(&receiver, &name, &self.written);
        self.answer(span, &name, &answer);
        self.arguments(call.args.iter(), &answer);
        answer.ret()
    }

    fn call(&mut self, call: &syn::ExprCall) -> Ty {
        let mut func = &*call.func;
        while let syn::Expr::Paren(syn::ExprParen { expr, .. })
        | syn::Expr::Group(syn::ExprGroup { expr, .. }) = func
        {
            func = expr;
        }
        match func {
            syn::Expr::Path(path) => {
                let (span, name) = path_site(self, path);
                let answer = self.path_call(path);
                self.answer(span, &name, &answer);
                let args = self.arguments(call.args.iter(), &answer);
                if answer
                    .targets
                    .iter()
                    .all(|(target, _)| matches!(target, Target::Constructor))
                    && matches!(name.as_str(), "Some")
                    && let [only] = args.as_slice()
                {
                    return option_of(only.clone());
                }
                answer.ret()
            }
            syn::Expr::Closure(closure) => {
                let span = self.span_of(closure.span());
                let args: Vec<Ty> = call.args.iter().map(|arg| self.expr(arg)).collect();
                self.push(span, "(closure)", Target::Closure(span), Via::Direct);
                self.closure(closure, &args);
                Ty::unknown("what a closure returns")
            }
            other => {
                let span = self.span_of(other.span());
                self.expr(other);
                self.push(
                    span,
                    "(value)",
                    Target::Unresolved(Unresolved::Dynamic {
                        what: "a called value: a stored closure or a function pointer".to_owned(),
                        candidates: Vec::new(),
                    }),
                    Via::Direct,
                );
                for arg in &call.args {
                    self.expr(arg);
                }
                Ty::unknown("what a called value returns")
            }
        }
    }

    /// **A path in call position**: a binding (a stored closure or a function pointer), a nested
    /// `fn`, `Self::f`, `T::f` on a generic, `<X as Tr>::f`, or a path through the module tree.
    fn path_call(&mut self, path: &syn::ExprPath) -> Answer {
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let Some(name) = segments.last().cloned() else {
            return Answer::default();
        };
        if let Some(qself) = &path.qself {
            let x = self.written.ty(&qself.ty);
            let bound = (qself.position > 0).then(|| {
                let mut trait_path = path.path.clone();
                while trait_path.segments.len() > qself.position {
                    trait_path.segments.pop();
                }
                trait_path.segments.pop_punct();
                match self.written.path(&trait_path) {
                    Ty::Named(named) => Some(named),
                    _ => None,
                }
            });
            return match bound {
                Some(None) => unresolved(Unresolved::Path {
                    written: format!("<_ as _>::{name}"),
                    why: "a qualified path whose trait the rules do not read".to_owned(),
                }),
                Some(Some(bound)) => self
                    .tables
                    .qualified(&x, Some(&bound), &name, &self.written),
                None => self.tables.qualified(&x, None, &name, &self.written),
            };
        }
        if segments.len() == 1 {
            if self.lookup(&name).is_some() {
                return unresolved(Unresolved::Dynamic {
                    what: format!(
                        "`{name}`, a binding called: a stored closure or a function pointer"
                    ),
                    candidates: Vec::new(),
                });
            }
            if let Some(span) = self.nested(&name) {
                return Answer {
                    targets: vec![(Target::Nested(span), Via::Direct)],
                    ..Answer::default()
                };
            }
        }
        if segments.len() >= 2 {
            let first = segments[0].as_str();
            let owner = if first == "Self" && segments.len() == 2 {
                Some(
                    self.written
                        .self_ty
                        .clone()
                        .unwrap_or_else(|| Ty::unknown("`Self` outside an impl")),
                )
            } else if segments.len() == 2 && self.written.params.contains(first) {
                Some(Ty::Param(first.to_owned()))
            } else {
                None
            };
            if let Some(owner) = owner {
                return self.tables.associated(&owner, &name, &self.written);
            }
        }
        let (rewritten, globs) = self.body_path(&segments);
        self.module_call(&rewritten, &globs, &path.path)
    }

    /// A call whose path is read through the module tree.
    fn module_call(
        &mut self,
        segments: &[String],
        globs: &[Vec<String>],
        written: &syn::Path,
    ) -> Answer {
        let name = segments.last().cloned().unwrap_or_default();
        // A glob of a `use` written in the body is the innermost scope: what it opens comes before
        // the module's names (a body's own `use` names were rewritten already).
        let mut named = Vec::new();
        let mut cut = false;
        if segments.len() == 1 {
            for glob in globs {
                let mut through = glob.clone();
                through.push(name.clone());
                let opened = self.tables.names.resolve(self.at, &self.module, &through);
                cut |= opened.cut;
                named.extend(opened.named);
            }
        }
        let resolution = self.tables.names.resolve(self.at, &self.module, segments);
        cut |= resolution.cut;
        if named.is_empty() {
            named = resolution.named.clone();
        }
        if cut {
            return unresolved(Unresolved::Cutoff {
                what: format!(
                    "the path `{}` in {} {}",
                    segments.join("::"),
                    self.package,
                    self.module
                ),
            });
        }
        let mut functions = Vec::new();
        let mut constructor = false;
        let mut foreign = Vec::new();
        for found in &named {
            match found {
                Named::Function(at, module, function) => {
                    functions.extend(self.tables_functions(*at, module, function));
                }
                Named::Variant(..) | Named::Type(..) => constructor = true,
                Named::Foreign(path) => foreign.push(path.clone()),
                Named::Value(at, module, value) => {
                    // A `const` or a `static` called: what it holds is a function value. Its
                    // initializer is reached (the functions it names are edges there), and the
                    // call is dynamic with those functions as its candidates.
                    let values = self
                        .tables
                        .module_values
                        .get(&(*at, module.clone(), value.clone()))
                        .cloned()
                        .unwrap_or_default();
                    let mut candidates = Vec::new();
                    for id in &values {
                        for edge in Walker::value(self.tables, *id) {
                            if let (Target::Item(item), Via::Value) = (&edge.to, &edge.via) {
                                candidates.push(*item);
                            }
                        }
                    }
                    candidates.sort();
                    candidates.dedup();
                    let mut targets: Vec<(Target, Via)> = values
                        .into_iter()
                        .map(|id| (Target::Value(id), Via::Value))
                        .collect();
                    targets.push((
                        Target::Unresolved(Unresolved::Dynamic {
                            what: format!(
                                "`{value}`, a `const` or `static` of {} {module} called: a \
                                 function pointer",
                                self.tables.package(*at)
                            ),
                            candidates,
                        }),
                        Via::Direct,
                    ));
                    return Answer {
                        targets,
                        ..Answer::default()
                    };
                }
                Named::Module(..) | Named::Trait(..) | Named::Alias(..) => {}
            }
        }
        if !functions.is_empty() {
            return self.tables_items(functions);
        }
        if constructor {
            return Answer {
                targets: vec![(Target::Constructor, Via::Direct)],
                ..Answer::default()
            };
        }
        if let Some(path) = foreign.first() {
            if foreign.len() == 1 && segments.len() >= 2 && !is_constructor_path(path) {
                // `Vec::with_capacity`, `std::fs::read`: the foreign prefix asked as a type, which
                // answers the same path and, for a type's builder, what it returns.
                let mut prefix = written.clone();
                prefix.segments.pop();
                prefix.segments.pop_punct();
                if prefix.segments.len() + 1 == segments.len() {
                    let owner = self.written.path(&prefix);
                    if let Ty::Named(named) = &owner
                        && matches!(self.tables.place(named), super::decls::Placed::Foreign(_))
                    {
                        return self.tables.associated(&owner, &name, &self.written);
                    }
                }
            }
            if foreign.len() == 1 {
                if is_constructor_path(path) {
                    return Answer {
                        targets: vec![(Target::Constructor, Via::Direct)],
                        ..Answer::default()
                    };
                }
                return Answer {
                    targets: vec![(Target::Foreign(path.clone()), Via::Direct)],
                    ..Answer::default()
                };
            }
        }
        if segments.len() >= 2 {
            // `Type::f`, `Alias::f`, `Trait::f`: the prefix is a type.
            let mut prefix = written.clone();
            prefix.segments.pop();
            prefix.segments.pop_punct();
            let rewritten_prefix = &segments[..segments.len() - 1];
            let owner = if rewritten_prefix.len() == prefix.segments.len() {
                self.written.path(&prefix)
            } else {
                let path: syn::Path =
                    syn::parse_str(&rewritten_prefix.join("::")).unwrap_or_else(|_| prefix.clone());
                self.written.path(&path)
            };
            if let Ty::Named(_) | Ty::Param(_) | Ty::Traits(_) = &owner {
                return self.tables.associated(&owner, &name, &self.written);
            }
        }
        unresolved(Unresolved::Path {
            written: segments.join("::"),
            why: resolution.refused.unwrap_or_else(|| {
                "a path that names no function the module tree holds".to_owned()
            }),
        })
    }

    fn tables_functions(&self, at: usize, module: &str, name: &str) -> Vec<ItemId> {
        self.tables
            .functions
            .get(&(at, module.to_owned(), name.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn tables_items(&self, found: Vec<ItemId>) -> Answer {
        let (inside, outside): (Vec<ItemId>, Vec<ItemId>) = found
            .into_iter()
            .partition(|id| self.tables.is_admitted(*id));
        if inside.is_empty() {
            return unresolved(Unresolved::OtherArm {
                candidates: outside,
            });
        }
        let sig = self.tables.sigs.get(&inside[0]).cloned();
        Answer {
            targets: inside
                .into_iter()
                .map(|id| (Target::Item(id), Via::Direct))
                .collect(),
            ret: sig.as_ref().map(|sig| {
                let map: std::collections::BTreeMap<String, Ty> = sig
                    .generics
                    .iter()
                    .map(|name| (name.clone(), Ty::unknown(format!("the generic `{name}`"))))
                    .collect();
                sig.ret.substitute(&map)
            }),
            by_position: sig
                .map(|sig| crate::census::types::closures_by_position(&sig.params))
                .unwrap_or_default(),
            closure: Vec::new(),
        }
    }

    /// **A path in value position**: a binding's type; data — a `const`, a `static`, an
    /// associated `const`, a unit struct or a variant — with its type and no edge; anything else is
    /// a function named as a value (`spawn(worker)`, `.map(Self::f)`), and **every** edge its path
    /// gives is pushed with [`Via::Value`] — resolved, foreign or unresolved, never dropped.
    fn path_value(&mut self, path: &syn::ExprPath) -> Ty {
        if path.qself.is_none() && path.path.segments.len() == 1 {
            let name = path.path.segments[0].ident.to_string();
            if let Some(ty) = self.lookup(&name) {
                return ty.clone();
            }
            if let Some(span) = self.nested(&name) {
                let site = self.span_of(path.span());
                self.push(site, &name, Target::Nested(span), Via::Value);
                return Ty::unknown("a function item");
            }
        }
        if let Some((data, values)) = self.data(path) {
            // Data is no call; a `const`'s or `static`'s initializer is reached by the use.
            let (span, name) = path_site(self, path);
            for (id, via) in values {
                self.push(span, &name, Target::Value(id), via);
            }
            return data;
        }
        let (span, name) = path_site(self, path);
        let saved = self.edges.len();
        let answer = self.path_call(path);
        self.edges.truncate(saved);
        for (target, _) in &answer.targets {
            self.push(span, &name, target.clone(), Via::Value);
        }
        Ty::unknown("a function value")
    }

    /// **The type a path in value position has when it names data**, with the values whose
    /// initializers the use reaches: a unit struct or an enum's variant (the type, no value), a
    /// `const` or a `static` of a module (its written type; [`Via::Deref`] for a `LazyLock` or a
    /// `LazyCell`, whose initializer runs on the first deref, [`Via::Value`] otherwise), an
    /// associated `const` (`Self::LIMIT`, `Type::MAX`, `T::ID`: untyped, [`Via::Value`]). `None`
    /// for a path that is no data.
    fn data(&self, path: &syn::ExprPath) -> Option<(Ty, Vec<(ValueId, Via)>)> {
        if path.qself.is_some() {
            return None;
        }
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let (segments, _) = self.body_path(&segments);
        let resolution = self.tables.names.resolve(self.at, &self.module, &segments);
        let found = resolution.named.iter().find_map(|named| match named {
            Named::Variant(at, module, owner, _) | Named::Type(at, module, owner) => Some((
                super::decls::named_of(self.tables.package(*at), module, owner),
                Vec::new(),
            )),
            Named::Value(at, module, value) => {
                let key = (*at, module.clone(), value.clone());
                let ty = self
                    .tables
                    .values
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| Ty::unknown("a value whose type is not read"));
                let via = if self.tables.is_lazy(&ty) {
                    Via::Deref
                } else {
                    Via::Value
                };
                let values = self
                    .tables
                    .module_values
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .map(|id| (*id, via.clone()))
                    .collect();
                Some((ty, values))
            }
            _ => None,
        });
        if found.is_some() {
            return found;
        }
        let (name, prefix) = segments.split_last()?;
        if prefix.is_empty() {
            return None;
        }
        let owner = match prefix {
            [only] if only == "Self" => self.written.self_ty.clone()?,
            [only] if self.written.params.contains(only) => Ty::Param(only.clone()),
            _ => {
                let written: syn::Path = syn::parse_str(&prefix.join("::")).ok()?;
                self.written.path(&written)
            }
        };
        let values = self.tables.associated_values(&owner, name, &self.written);
        (!values.is_empty()).then(|| {
            (
                Ty::unknown("an associated `const`"),
                values.into_iter().map(|id| (id, Via::Value)).collect(),
            )
        })
    }

    // ── macros ────────────────────────────────────────────────────────────

    fn mac(&mut self, mac: &syn::Macro) {
        let name = mac
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        let span = self.span_of(mac.path.span());
        self.push(
            span,
            &name,
            Target::Unresolved(Unresolved::Macro { name: name.clone() }),
            Via::Direct,
        );
        let is_matches = mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "matches");
        if mac.path.is_ident("cfg") {
            // Its arguments are `cfg` predicates, not expressions: nothing runs.
            return;
        }
        self.macros.push(name);
        if is_matches && let Ok((value, pattern, guard)) = mac.parse_body_with(matches_body) {
            // The pattern binds over the guard, as a match arm's does.
            let scrutinee = self.expr(&value);
            self.scopes.push(Scope::default());
            self.bind(&pattern, scrutinee, Mode::Move);
            if let Some(guard) = guard {
                self.condition(&guard);
            }
            self.scopes.pop();
        } else if let Ok(arguments) =
            mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        {
            for argument in &arguments {
                match argument {
                    syn::Expr::Assign(named) if matches!(&*named.left, syn::Expr::Path(_)) => {
                        self.expr(&named.right);
                    }
                    other => {
                        self.expr(other);
                    }
                }
            }
        } else if let Ok((value, pattern, guard)) = mac.parse_body_with(matches_body) {
            let scrutinee = self.expr(&value);
            self.scopes.push(Scope::default());
            self.bind(&pattern, scrutinee, Mode::Move);
            if let Some(guard) = guard {
                self.condition(&guard);
            }
            self.scopes.pop();
        } else if let Ok((value, length)) = mac.parse_body_with(repeat_body) {
            self.expr(&value);
            self.expr(&length);
        }
        self.macros.pop();
    }
}

/// `Some`, `Ok`, `Err` — the prelude's constructors.
fn is_constructor_path(path: &[String]) -> bool {
    matches!(path, [std, module, ty, variant]
        if std == "std"
            && matches!((module.as_str(), ty.as_str()), ("option", "Option") | ("result", "Result"))
            && matches!(variant.as_str(), "Some" | "Ok" | "Err"))
}

fn unresolved(kind: Unresolved) -> Answer {
    Answer {
        targets: vec![(Target::Unresolved(kind), Via::Direct)],
        ..Answer::default()
    }
}

/// The site a path call is named at: its last segment.
fn path_site(walker: &Walker<'_, '_>, path: &syn::ExprPath) -> (Span, String) {
    path.path.segments.last().map_or_else(
        || (walker.span_of(path.span()), String::new()),
        |segment| {
            (
                walker.span_of(segment.ident.span()),
                segment.ident.to_string(),
            )
        },
    )
}

/// `matches!(value, pattern if guard)`.
fn matches_body(input: syn::parse::ParseStream<'_>) -> syn::Result<MatchesBody> {
    let value: syn::Expr = input.parse()?;
    input.parse::<syn::Token![,]>()?;
    let pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.peek(syn::Token![if]) {
        input.parse::<syn::Token![if]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    if input.peek(syn::Token![,]) {
        input.parse::<syn::Token![,]>()?;
    }
    Ok((value, pattern, guard))
}

/// `matches!`'s value, pattern and guard.
type MatchesBody = (syn::Expr, syn::Pat, Option<syn::Expr>);

/// `vec![value; length]`.
fn repeat_body(input: syn::parse::ParseStream<'_>) -> syn::Result<(syn::Expr, syn::Expr)> {
    let value: syn::Expr = input.parse()?;
    input.parse::<syn::Token![;]>()?;
    let length: syn::Expr = input.parse()?;
    Ok((value, length))
}

fn member_name(member: &syn::Member) -> String {
    match member {
        syn::Member::Named(name) => name.to_string(),
        syn::Member::Unnamed(index) => index.index.to_string(),
    }
}

fn last_segment(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

/// A body `use` tree, flattened into the names it brings in and the globs it opens.
fn flatten_use(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    names: &mut Vec<(String, Vec<String>)>,
    globs: &mut Vec<Vec<String>>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten_use(&path.tree, prefix, names, globs);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let name = name.ident.to_string();
            if name == "self" {
                if let Some(last) = prefix.last() {
                    names.push((last.clone(), prefix.clone()));
                }
            } else {
                let mut path = prefix.clone();
                path.push(name.clone());
                names.push((name, path));
            }
        }
        syn::UseTree::Rename(rename) => {
            let original = rename.ident.to_string();
            let mut path = prefix.clone();
            if original != "self" {
                path.push(original);
            }
            names.push((rename.rename.to_string(), path));
        }
        syn::UseTree::Glob(_) => globs.push(prefix.clone()),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                flatten_use(tree, prefix, names, globs);
            }
        }
    }
}

/// The attributes written on an expression statement.
fn expr_attrs(expr: &syn::Expr) -> &[syn::Attribute] {
    match expr {
        syn::Expr::Array(it) => &it.attrs,
        syn::Expr::Assign(it) => &it.attrs,
        syn::Expr::Async(it) => &it.attrs,
        syn::Expr::Await(it) => &it.attrs,
        syn::Expr::Binary(it) => &it.attrs,
        syn::Expr::Block(it) => &it.attrs,
        syn::Expr::Break(it) => &it.attrs,
        syn::Expr::Call(it) => &it.attrs,
        syn::Expr::Cast(it) => &it.attrs,
        syn::Expr::Closure(it) => &it.attrs,
        syn::Expr::Const(it) => &it.attrs,
        syn::Expr::Continue(it) => &it.attrs,
        syn::Expr::Field(it) => &it.attrs,
        syn::Expr::ForLoop(it) => &it.attrs,
        syn::Expr::Group(it) => &it.attrs,
        syn::Expr::If(it) => &it.attrs,
        syn::Expr::Index(it) => &it.attrs,
        syn::Expr::Infer(it) => &it.attrs,
        syn::Expr::Let(it) => &it.attrs,
        syn::Expr::Lit(it) => &it.attrs,
        syn::Expr::Loop(it) => &it.attrs,
        syn::Expr::Macro(it) => &it.attrs,
        syn::Expr::Match(it) => &it.attrs,
        syn::Expr::MethodCall(it) => &it.attrs,
        syn::Expr::Paren(it) => &it.attrs,
        syn::Expr::Path(it) => &it.attrs,
        syn::Expr::Range(it) => &it.attrs,
        syn::Expr::RawAddr(it) => &it.attrs,
        syn::Expr::Reference(it) => &it.attrs,
        syn::Expr::Repeat(it) => &it.attrs,
        syn::Expr::Return(it) => &it.attrs,
        syn::Expr::Struct(it) => &it.attrs,
        syn::Expr::Try(it) => &it.attrs,
        syn::Expr::TryBlock(it) => &it.attrs,
        syn::Expr::Tuple(it) => &it.attrs,
        syn::Expr::Unary(it) => &it.attrs,
        syn::Expr::Unsafe(it) => &it.attrs,
        syn::Expr::While(it) => &it.attrs,
        syn::Expr::Yield(it) => &it.attrs,
        _ => &[],
    }
}
