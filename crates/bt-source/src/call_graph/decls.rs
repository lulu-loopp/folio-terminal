//! **The program's declarations, by identity**: every type with its fields, variants and derives,
//! every alias with its target, every callable with its signature, every `impl` block with the
//! type it is for, the traits it implements and the methods written in it — and the rules a call
//! is answered by, standing on them ([`Tables::method`], [`Tables::associated`],
//! [`Tables::dispatch`], [`Tables::field`]).
//!
//! A type is placed by [`Names`] (the module tree), never by its bare name: two types called
//! `State` in two modules are two keys. The typing steps — a type from a declaration carried
//! through a field, an index, a deref, a method whose declaration is found, a pattern — are the
//! census's ([`crate::census`]'s `types`), and the standard library is its fixed lists.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::names::{Named, Names, canonical_std};
use super::{ArmQuery, Arms, ItemId, Target, Unresolved, Via};
use crate::census::types::{
    Method, Named as TyNamed, Receiver, Std, Ty, Written, closures_by_position, entry_method,
    external_method, impl_header, impl_of, iterator_method, prelude_method, std_kind, std_method,
};
use crate::index::{ConditionalVariant, DeclarationPath, Index, ItemRecord};
use crate::traits::{ResolveCutoff, TraitId, TraitResolver};

/// A first-party type: package, module, name.
pub(crate) type TypeKey = (usize, String, String);

/// A type's trait impls declaring one name, by trait; and the first-party traits it implements,
/// each with its impl.
type TraitMembers = (BTreeMap<TraitRef, Vec<ItemId>>, Vec<(TraitId, usize)>);

/// How many deref steps (and alias expansions) a receiver is followed through — the census's.
pub(crate) const DEREF_STEPS: usize = 16;

/// The traits a derive generates, with the methods each one's expansion declares.
const DERIVED: [(&str, &[&str]); 9] = [
    ("Clone", &["clone", "clone_from"]),
    ("Debug", &["fmt"]),
    ("Default", &["default"]),
    ("PartialEq", &["eq", "ne"]),
    ("PartialOrd", &["partial_cmp", "lt", "le", "gt", "ge"]),
    ("Ord", &["cmp", "max", "min", "clamp"]),
    ("Hash", &["hash"]),
    ("Eq", &[]),
    ("Copy", &[]),
];

/// What a written type path is, placed.
pub(crate) enum Placed {
    /// One type, or one type on each of several arms (`#[cfg] pub use a::T; #[cfg] pub use b::T;`):
    /// a question asked of it is asked of each, and the answers are united.
    Types(Vec<TypeKey>),
    Alias(TypeKey),
    Trait,
    Foreign(Vec<String>),
    /// It names nothing, or several things: why.
    Unplaced(String),
    /// The hop limit was reached: what was being read.
    Cut(String),
}

/// One `struct`, `enum` or `union`.
#[derive(Default)]
pub(crate) struct TypeInfo {
    pub(crate) generics: Vec<String>,
    pub(crate) fields: Vec<(String, Ty)>,
    pub(crate) variants: BTreeMap<String, Vec<(String, Ty)>>,
    pub(crate) derives: BTreeSet<String>,
    /// A field carries `#[from]` (`thiserror`): a `From::from` no byte holds.
    pub(crate) from_field: bool,
    /// The files its declarations are written in.
    pub(crate) files: BTreeSet<usize>,
}

/// One type alias.
pub(crate) struct AliasInfo {
    pub(crate) generics: Vec<String>,
    pub(crate) target: Ty,
    /// Whether the arms admit this declaration of the alias.
    pub(crate) admitted: bool,
}

/// One callable's signature.
#[derive(Clone)]
pub(crate) struct Sig {
    pub(crate) receiver: Receiver,
    pub(crate) ret: Ty,
    pub(crate) params: Vec<Ty>,
    pub(crate) impl_params: Vec<String>,
    pub(crate) self_args: Vec<Ty>,
    /// The function's own type parameters, unknown at a call the rules do not unify.
    pub(crate) generics: Vec<String>,
}

/// What an `impl` block is for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SelfKey {
    Type(TypeKey),
    Foreign(Vec<String>),
    /// `impl<T: …> Tr for T`.
    Blanket(String),
    Other(String),
}

/// A trait an `impl` block implements.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TraitRef {
    First(TraitId),
    Foreign(Vec<String>),
}

/// One `impl` block.
pub(crate) struct ImplInfo {
    pub(crate) self_key: SelfKey,
    pub(crate) traits: Vec<TraitRef>,
    pub(crate) methods: Vec<ItemId>,
    pub(crate) admitted: bool,
    pub(crate) impl_params: Vec<String>,
    pub(crate) self_args: Vec<Ty>,
    /// `type Target = …` of a `Deref` impl.
    pub(crate) deref_target: Option<Ty>,
}

/// A call's answer: where it goes, what it gives back, what its closures receive.
#[derive(Default)]
pub(crate) struct Answer {
    pub(crate) targets: Vec<(Target, Via)>,
    pub(crate) ret: Option<Ty>,
    /// What a closure handed in each argument position receives (a declaration's `impl Fn(..)`).
    pub(crate) by_position: Vec<Option<Vec<Ty>>>,
    /// What any closure argument receives (the standard lists).
    pub(crate) closure: Vec<Ty>,
}

impl Answer {
    fn one(target: Target, via: Via) -> Self {
        Self {
            targets: vec![(target, via)],
            ..Self::default()
        }
    }

    fn unresolved(kind: Unresolved) -> Self {
        Self::one(Target::Unresolved(kind), Via::Direct)
    }

    pub(crate) fn ret(&self) -> Ty {
        self.ret
            .clone()
            .unwrap_or_else(|| Ty::unknown("what the call returns"))
    }

    pub(crate) fn closure_at(&self, at: usize) -> Vec<Ty> {
        match self.by_position.get(at) {
            Some(Some(params)) => params.clone(),
            _ => self.closure.clone(),
        }
    }

    fn with_std(mut self, method: Option<Method>) -> Self {
        if let Some(method) = method {
            self.ret = Some(method.ret);
            self.closure = method.closure;
            self.by_position = method.by_position;
        }
        self
    }
}

/// Everything the rules stand on.
pub(crate) struct Tables<'i> {
    pub(crate) arms: Arms<'i>,
    pub(crate) names: Names<'i>,
    pub(crate) traits: TraitResolver<'i>,
    pub(crate) crates: BTreeSet<String>,
    pub(crate) admitted: Vec<Vec<bool>>,
    pub(crate) types: HashMap<TypeKey, TypeInfo>,
    /// Each alias, once per declaration: one alias written on several `cfg` arms is several.
    pub(crate) aliases: HashMap<TypeKey, Vec<AliasInfo>>,
    pub(crate) sigs: HashMap<ItemId, Sig>,
    pub(crate) impls: Vec<ImplInfo>,
    pub(crate) inherent: HashMap<(TypeKey, String), Vec<ItemId>>,
    pub(crate) type_impls: HashMap<SelfKey, Vec<usize>>,
    pub(crate) trait_impls: HashMap<TraitId, Vec<usize>>,
    pub(crate) foreign_trait_impls: BTreeMap<Vec<String>, Vec<usize>>,
    pub(crate) trait_declared: HashMap<TraitId, Vec<ItemId>>,
    pub(crate) trait_of_item: HashMap<ItemId, TraitId>,
    pub(crate) method_names: HashMap<String, Vec<ItemId>>,
    /// Every `const` and `static` of a module, with its written type.
    pub(crate) values: HashMap<(usize, String, String), Ty>,
    /// The associated `const`s of each `impl` block's self type, by name.
    pub(crate) associated_consts: BTreeSet<(SelfKey, String)>,
    /// The `const`s each first-party trait declares.
    pub(crate) trait_consts: BTreeSet<(TraitId, String)>,
    /// The `impl` blocks whose self type is not placed ([`UnplacedImpl`]).
    pub(crate) unplaced_impls: Vec<UnplacedImpl>,
    /// The free functions of each module, by name (one per `cfg` variant).
    pub(crate) functions: HashMap<(usize, String, String), Vec<ItemId>>,
    pub(crate) unparsed: Vec<String>,
    supertraits: RefCell<HashMap<TraitId, Result<BTreeSet<TraitId>, String>>>,
}

/// Why an `impl` block's self type is not placed.
pub(crate) enum SelfTypeFailure {
    /// It names nothing the module tree holds (a macro-generated type), or several kinds of thing.
    Unplaced(String),
    /// It chains past the hop limit.
    Cut(String),
}

/// **An `impl` block whose self type the module tree does not hold** — a type a macro generates
/// (`bitflags!`, windows-rs's `#[implement]`, objc2's `define_class!`): why, and the methods
/// written in it, which no typed call reaches and an untyped one lists as candidates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnplacedImpl {
    pub what: String,
    pub methods: Vec<ItemId>,
}

/// A refusal of the reading itself: an `impl` block's trait or self type chained past the hop
/// limit, which no table may file under nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub what: String,
}

impl<'i> Tables<'i> {
    pub(crate) fn read(
        packages: &[(&'i str, &'i Index)],
        arms: Arms<'i>,
    ) -> Result<Self, Vec<Refusal>> {
        let names = Names::new(packages);
        let traits = TraitResolver::new(packages);
        let mut crates = BTreeSet::new();
        for (_, index) in packages {
            for root in index.universe().roots() {
                if root.id.kind == crate::TargetKind::Library {
                    crates.insert(root.id.name.replace('-', "_"));
                }
            }
        }
        let mut tables = Self {
            arms,
            names,
            traits,
            crates,
            admitted: Vec::new(),
            types: HashMap::new(),
            aliases: HashMap::new(),
            sigs: HashMap::new(),
            impls: Vec::new(),
            inherent: HashMap::new(),
            type_impls: HashMap::new(),
            trait_impls: HashMap::new(),
            foreign_trait_impls: BTreeMap::new(),
            trait_declared: HashMap::new(),
            trait_of_item: HashMap::new(),
            method_names: HashMap::new(),
            functions: HashMap::new(),
            values: HashMap::new(),
            unplaced_impls: Vec::new(),
            associated_consts: BTreeSet::new(),
            trait_consts: BTreeSet::new(),
            unparsed: Vec::new(),
            supertraits: RefCell::default(),
        };
        let mut refusals = Vec::new();
        // Every type, alias and trait of every package first: an `impl` block or a signature of one
        // package may name a type or an alias of a later one.
        for (at, (package, index)) in packages.iter().enumerate() {
            let admitted = index
                .items()
                .iter()
                .map(|item| {
                    admits(
                        &tables.arms,
                        package,
                        index,
                        item.whole().start(),
                        &item.declared,
                        &item.variant,
                    )
                })
                .collect();
            tables.admitted.push(admitted);
            tables.read_types(at, package, index);
            tables.read_aliases(at, package, index);
            tables.read_traits(package, index, at);
        }
        for (at, (package, index)) in packages.iter().enumerate() {
            if let Err(mut refused) = tables.read_impls(at, package, index) {
                refusals.append(&mut refused);
            }
        }
        for (at, (package, index)) in packages.iter().enumerate() {
            tables.read_values(at, package, index);
            tables.read_sigs(at, package, index);
        }
        if refusals.is_empty() {
            Ok(tables)
        } else {
            Err(refusals)
        }
    }

    pub(crate) fn written(&self, package: &str, module: &str) -> Written {
        Written {
            package: package.to_owned(),
            module: module.to_owned(),
            crates: self.crates.clone(),
            traits: true,
            ..Written::default()
        }
    }

    pub(crate) fn index(&self, at: usize) -> &'i Index {
        self.names.packages[at].2
    }

    pub(crate) fn package(&self, at: usize) -> &'i str {
        self.names.packages[at].0
    }

    pub(crate) fn record(&self, id: ItemId) -> &'i ItemRecord {
        &self.index(id.package()).items()[id.item()]
    }

    pub(crate) fn is_admitted(&self, id: ItemId) -> bool {
        self.admitted[id.package()][id.item()]
    }

    fn read_types(&mut self, at: usize, package: &str, index: &Index) {
        for item in index.items() {
            if !item.kind().is_type() {
                continue;
            }
            let Ok(parsed) = syn::parse_str::<syn::Item>(index.text(item.whole())) else {
                self.unparsed.push(format!("type {}", item.name()));
                continue;
            };
            for module in item.module_paths() {
                let mut written = self.written(package, module);
                let (attrs, generics, fields, variants): (
                    _,
                    _,
                    Vec<syn::Field>,
                    Vec<syn::Variant>,
                ) = match &parsed {
                    syn::Item::Struct(it) => (
                        &it.attrs,
                        &it.generics,
                        it.fields.iter().cloned().collect(),
                        Vec::new(),
                    ),
                    syn::Item::Union(it) => (
                        &it.attrs,
                        &it.generics,
                        it.fields.named.iter().cloned().collect(),
                        Vec::new(),
                    ),
                    syn::Item::Enum(it) => (
                        &it.attrs,
                        &it.generics,
                        Vec::new(),
                        it.variants.iter().cloned().collect(),
                    ),
                    _ => continue,
                };
                written.enter(generics);
                let read = |fields: &[syn::Field]| -> Vec<(String, Ty)> {
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
                let mut from_field = fields
                    .iter()
                    .any(|field| field.attrs.iter().any(|a| a.path().is_ident("from")));
                let mut variant_fields = BTreeMap::new();
                for variant in &variants {
                    let fields: Vec<syn::Field> = variant.fields.iter().cloned().collect();
                    from_field |= fields
                        .iter()
                        .any(|field| field.attrs.iter().any(|a| a.path().is_ident("from")));
                    variant_fields.insert(variant.ident.to_string(), read(&fields));
                }
                let info = TypeInfo {
                    files: BTreeSet::from([item.file]),
                    generics: generic_names(generics),
                    fields: read(&fields),
                    variants: variant_fields,
                    derives: derives(attrs),
                    from_field,
                };
                let key = (at, module.to_owned(), item.name().to_owned());
                match self.types.get_mut(&key) {
                    // One type on two arms: its members are the union of what each declares.
                    Some(known) => {
                        for field in info.fields {
                            if !known.fields.iter().any(|(name, _)| *name == field.0) {
                                known.fields.push(field);
                            }
                        }
                        known.variants.extend(info.variants);
                        known.derives.extend(info.derives);
                        known.files.extend(info.files);
                        known.from_field |= info.from_field;
                    }
                    None => {
                        self.types.insert(key, info);
                    }
                }
            }
        }
    }

    fn read_aliases(&mut self, at: usize, package: &str, index: &Index) {
        for alias in index.aliases() {
            let Ok(parsed) = syn::parse_str::<syn::ItemType>(index.text(alias.whole())) else {
                self.unparsed.push(format!("type alias {}", alias.name()));
                continue;
            };
            let admitted = admits(
                &self.arms,
                package,
                index,
                alias.whole().start(),
                &alias.declared,
                &alias.variant,
            );
            for module in alias.module_paths() {
                let mut written = self.written(package, module);
                written.enter(&parsed.generics);
                self.aliases
                    .entry((at, module.to_owned(), alias.name().to_owned()))
                    .or_default()
                    .push(AliasInfo {
                        generics: generic_names(&parsed.generics),
                        target: written.ty(&parsed.ty),
                        admitted,
                    });
            }
        }
    }

    fn read_traits(&mut self, package: &str, index: &Index, at: usize) {
        for declaration in index.traits() {
            let ids = self.traits.identities(package, declaration);
            let members: Vec<ItemId> = index
                .items()
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.kind().is_callable()
                        && item.type_owner().is_none()
                        && declaration.body().holds(item.whole().start())
                })
                .map(|(item, _)| ItemId::new(at, item))
                .collect();
            for id in ids {
                for member in &members {
                    self.trait_of_item.insert(*member, id.clone());
                }
                self.trait_declared
                    .entry(id)
                    .or_default()
                    .extend(members.iter().copied());
            }
        }
    }

    fn read_impls(&mut self, at: usize, package: &str, index: &Index) -> Result<(), Vec<Refusal>> {
        let mut refusals = Vec::new();
        // The methods of each block, by the block's position.
        let mut members: BTreeMap<usize, Vec<ItemId>> = BTreeMap::new();
        let blocks = index.impls();
        for (item, record) in index.items().iter().enumerate() {
            if !record.kind().is_callable() || record.type_owner().is_none() {
                continue;
            }
            if let Some(block) = impl_of(index, record)
                && let Some(position) = blocks.iter().position(|it| std::ptr::eq(it, block))
            {
                members
                    .entry(position)
                    .or_default()
                    .push(ItemId::new(at, item));
            }
        }
        for (position, block) in blocks.iter().enumerate() {
            let Some(header) = impl_header(index, block) else {
                self.unparsed
                    .push(format!("impl block for {}", block.type_owner()));
                continue;
            };
            let module = block
                .module_paths()
                .first()
                .map_or_else(|| "crate".to_owned(), |module| (*module).to_owned());
            let mut written = self.written(package, &module);
            written.enter(&header.generics);
            let self_ty = written.ty(&header.self_ty);
            let self_args = match &self_ty {
                Ty::Named(named) => named.args.clone(),
                _ => Vec::new(),
            };
            let file = index
                .file_at(block.whole().start())
                .and_then(|file| index.files().iter().position(|it| std::ptr::eq(it, file)));
            let self_keys = match self.self_keys(&self_ty, file) {
                Ok(keys) => keys,
                Err(SelfTypeFailure::Cut(what)) => {
                    refusals.push(Refusal {
                        what: format!(
                            "the self type `{}` of {package}'s `impl` block in {module} ({what})",
                            block.type_owner()
                        ),
                    });
                    continue;
                }
                Err(SelfTypeFailure::Unplaced(why)) => {
                    // A type a macro generates (`bitflags!`, `#[implement]`, a generated enum):
                    // its methods are listed, never filed under nothing.
                    self.unplaced_impls.push(UnplacedImpl {
                        what: format!(
                            "the self type `{}` of {package}'s `impl` block in {module}: {why}",
                            block.type_owner()
                        ),
                        methods: members.remove(&position).unwrap_or_default(),
                    });
                    continue;
                }
            };
            let self_key = self_keys
                .first()
                .cloned()
                .unwrap_or_else(|| SelfKey::Other(self_ty.spelling()));
            let mut traits = Vec::new();
            if let Some(path) = block.trait_path() {
                match self.traits.implemented_checked(package, block) {
                    Ok(found) if !found.is_empty() => {
                        traits.extend(found.into_iter().map(TraitRef::First));
                    }
                    Ok(_) => traits.push(TraitRef::Foreign(self.foreign_trait(at, &module, path))),
                    Err(ResolveCutoff(_)) => refusals.push(Refusal {
                        what: format!(
                            "the trait `{}` of {package}'s `impl` block for `{}`",
                            path.join("::"),
                            block.type_owner()
                        ),
                    }),
                }
            }
            let deref_target = if traits.iter().any(|it| matches!(it, TraitRef::Foreign(path) if path.last().is_some_and(|last| last == "Deref")))
            {
                syn::parse_str::<syn::ItemImpl>(index.text(block.whole()))
                    .ok()
                    .and_then(|parsed| {
                        parsed.items.iter().find_map(|member| match member {
                            syn::ImplItem::Type(target) if target.ident == "Target" => {
                                Some(written.ty(&target.ty))
                            }
                            _ => None,
                        })
                    })
            } else {
                None
            };
            let methods = members.remove(&position).unwrap_or_default();
            let admitted = admits(
                &self.arms,
                package,
                index,
                block.whole().start(),
                &block.declared,
                &block.variant,
            );
            let info = ImplInfo {
                self_key: self_key.clone(),
                traits: traits.clone(),
                methods: methods.clone(),
                admitted,
                impl_params: generic_names(&header.generics),
                self_args,
                deref_target,
            };
            let at_impl = self.impls.len();
            self.impls.push(info);
            for key in &self_keys {
                if traits.is_empty() {
                    if let SelfKey::Type(key) = key {
                        for method in &methods {
                            self.inherent
                                .entry((key.clone(), self.record(*method).name().to_owned()))
                                .or_default()
                                .push(*method);
                        }
                    }
                } else {
                    self.type_impls
                        .entry(key.clone())
                        .or_default()
                        .push(at_impl);
                }
            }
            for trait_ref in traits {
                match trait_ref {
                    TraitRef::First(id) => self.trait_impls.entry(id).or_default().push(at_impl),
                    TraitRef::Foreign(path) => self
                        .foreign_trait_impls
                        .entry(path)
                        .or_default()
                        .push(at_impl),
                }
            }
        }
        if refusals.is_empty() {
            Ok(())
        } else {
            Err(refusals)
        }
    }

    /// Every `const` and `static`: a module's with its type, an `impl` block's by its self type, a
    /// trait's by the trait.
    fn read_values(&mut self, at: usize, package: &str, index: &Index) {
        for value in index.values() {
            let text = index.text(value.whole());
            if let Some(owner) = value.type_owner() {
                let start = value.whole().start();
                let Some(block) = index.impls().iter().find(|block| block.body().holds(start))
                else {
                    self.unparsed
                        .push(format!("the impl block of const {}", value.name()));
                    continue;
                };
                let Some(header) = impl_header(index, block) else {
                    continue;
                };
                let module = block
                    .module_paths()
                    .first()
                    .map_or_else(|| "crate".to_owned(), |module| (*module).to_owned());
                let mut written = self.written(package, &module);
                written.enter(&header.generics);
                let self_ty = written.ty(&header.self_ty);
                let file = index
                    .file_at(start)
                    .and_then(|file| index.files().iter().position(|it| std::ptr::eq(it, file)));
                let _ = owner;
                for key in self.self_keys(&self_ty, file).unwrap_or_default() {
                    self.associated_consts
                        .insert((key, value.name().to_owned()));
                }
                continue;
            }
            if value.trait_name().is_some() {
                let start = value.whole().start();
                if let Some(declaration) = index
                    .traits()
                    .iter()
                    .find(|declaration| declaration.body().holds(start))
                {
                    for id in self.traits.identities(package, declaration) {
                        self.trait_consts.insert((id, value.name().to_owned()));
                    }
                }
                continue;
            }
            let ty = match syn::parse_str::<syn::Item>(text) {
                Ok(syn::Item::Const(constant)) => Some(constant.ty),
                Ok(syn::Item::Static(value)) => Some(value.ty),
                _ => None,
            };
            let Some(ty) = ty else {
                self.unparsed
                    .push(format!("const or static {}", value.name()));
                continue;
            };
            for module in value.module_paths() {
                let written = self.written(package, module);
                self.values.insert(
                    (at, module.to_owned(), value.name().to_owned()),
                    written.ty(&ty),
                );
            }
        }
    }

    /// **Whether `owner::name` is an associated `const`** — of the type's own impls, of a
    /// first-party trait it implements, or of a trait a generic or a trait object is known by:
    /// data, no call.
    pub(crate) fn is_associated_const(&self, owner: &Ty, name: &str, written: &Written) -> bool {
        let Ok(owner) = self.expand(owner.clone()) else {
            return false;
        };
        let declared_by = |traits: &BTreeSet<TraitId>| {
            traits.iter().any(|id| {
                let mut closure = BTreeSet::from([id.clone()]);
                if let Ok(supertraits) = self.supertraits_of(id) {
                    closure.extend(supertraits);
                }
                closure
                    .iter()
                    .any(|id| self.trait_consts.contains(&(id.clone(), name.to_owned())))
            })
        };
        match &owner {
            Ty::Named(named) => match self.place(named) {
                Placed::Types(keys) => keys.into_iter().any(|key| {
                    let key = SelfKey::Type(key);
                    if self
                        .associated_consts
                        .contains(&(key.clone(), name.to_owned()))
                    {
                        return true;
                    }
                    let (_, implemented) = self.trait_members(&key, name);
                    let traits: BTreeSet<TraitId> =
                        implemented.into_iter().map(|(id, _)| id).collect();
                    declared_by(&traits)
                }),
                Placed::Foreign(path) => self
                    .associated_consts
                    .contains(&(SelfKey::Foreign(path), name.to_owned())),
                _ => false,
            },
            Ty::Traits(bounds) => self
                .bound_traits(bounds)
                .is_ok_and(|(first, _)| declared_by(&first)),
            Ty::Param(param) => written.trait_bounds.get(param).is_some_and(|bounds| {
                self.bound_traits(bounds)
                    .is_ok_and(|(first, _)| declared_by(&first))
            }),
            _ => false,
        }
    }

    fn read_sigs(&mut self, at: usize, package: &str, index: &Index) {
        for (item, record) in index.items().iter().enumerate() {
            if !record.kind().is_callable() {
                continue;
            }
            let id = ItemId::new(at, item);
            if record.kind() == crate::index::ItemKind::Function {
                for module in record.module_paths() {
                    self.functions
                        .entry((at, module.to_owned(), record.name().to_owned()))
                        .or_default()
                        .push(id);
                }
            }
            if record.type_owner().is_some() || record.trait_name().is_some() {
                self.method_names
                    .entry(record.name().to_owned())
                    .or_default()
                    .push(id);
            }
            let Some(signature) = crate::census::types::signature_of(index, record) else {
                self.unparsed
                    .push(format!("signature of {}", record.name()));
                continue;
            };
            let module = record
                .module_paths()
                .first()
                .map_or_else(|| "crate".to_owned(), |module| (*module).to_owned());
            let mut written = self.written(package, &module);
            let mut impl_params = Vec::new();
            let mut self_args = Vec::new();
            if record.type_owner().is_some() {
                if let Some(header) =
                    impl_of(index, record).and_then(|block| impl_header(index, block))
                {
                    written.enter(&header.generics);
                    let self_ty = written.ty(&header.self_ty);
                    if let Ty::Named(named) = &self_ty {
                        self_args.clone_from(&named.args);
                    }
                    written.self_ty = Some(self_ty);
                    impl_params = generic_names(&header.generics);
                }
            } else if let Some(id) = self.trait_of_item.get(&id) {
                written.self_ty = Some(trait_self(id));
            }
            written.enter(&signature.generics);
            let ret = match &signature.output {
                syn::ReturnType::Default => Ty::Tuple(Vec::new()),
                syn::ReturnType::Type(_, ty) => written.ty(ty),
            };
            let params = signature
                .inputs
                .iter()
                .filter_map(|input| match input {
                    syn::FnArg::Typed(typed) => Some(written.ty(&typed.ty)),
                    syn::FnArg::Receiver(_) => None,
                })
                .collect();
            self.sigs.insert(
                id,
                Sig {
                    receiver: Receiver::of(&signature),
                    ret,
                    params,
                    impl_params,
                    self_args,
                    generics: generic_names(&signature.generics),
                },
            );
        }
    }

    /// The trait path an `impl` writes, when it is no first-party trait: its full foreign path.
    fn foreign_trait(&self, at: usize, module: &str, path: &[String]) -> Vec<String> {
        let resolution = self.names.resolve(at, module, path);
        resolution
            .named
            .iter()
            .find_map(|named| match named {
                Named::Foreign(path) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| path.to_vec())
    }

    /// **What an `impl` block is for.** A self type placed as several types (one name declared on
    /// several arms of one module path, `cfg`-alternative files) is the one declared in the
    /// block's own file when there is one — a file is compiled whole, on one arm — and otherwise
    /// each of them, so that no method is filed under nothing.
    ///
    /// `Err` is a self type the module tree cannot place (it names nothing, or several kinds of
    /// thing) or that chains past the hop limit: filed under nothing, its methods would be missing
    /// from every call on the type, so the reading refuses it.
    fn self_keys(&self, ty: &Ty, file: Option<usize>) -> Result<Vec<SelfKey>, SelfTypeFailure> {
        match ty {
            Ty::Ref { inner, .. } => self.self_keys(inner, file),
            Ty::Param(name) => Ok(vec![SelfKey::Blanket(name.clone())]),
            Ty::Named(named) => match self.place(named) {
                Placed::Types(keys) => {
                    let here: Vec<TypeKey> = keys
                        .iter()
                        .filter(|key| {
                            self.types.get(*key).is_some_and(|info| {
                                file.is_some_and(|file| info.files.contains(&file))
                            })
                        })
                        .cloned()
                        .collect();
                    let chosen = if here.len() == 1 { here } else { keys };
                    Ok(chosen.into_iter().map(SelfKey::Type).collect())
                }
                Placed::Foreign(path) => Ok(vec![SelfKey::Foreign(path)]),
                Placed::Alias(key) => {
                    let mut keys = Vec::new();
                    for target in self.alias_targets(&key, &named.args) {
                        keys.extend(self.self_keys(&target, file)?);
                    }
                    if keys.is_empty() {
                        return Err(SelfTypeFailure::Unplaced(format!(
                            "the alias `{}` names no type on any arm",
                            named.name
                        )));
                    }
                    Ok(keys)
                }
                Placed::Trait => Ok(vec![SelfKey::Other(format!("dyn {}", named.name))]),
                Placed::Unplaced(why) => Err(SelfTypeFailure::Unplaced(why)),
                Placed::Cut(what) => Err(SelfTypeFailure::Cut(format!(
                    "{what} chains past the hop limit"
                ))),
            },
            other => Ok(vec![SelfKey::Other(other.spelling())]),
        }
    }

    // ── placing a written type ────────────────────────────────────────────

    pub(crate) fn place(&self, named: &TyNamed) -> Placed {
        if named.home.is_empty() {
            // A standard type a fixed list names (`std_named`).
            return Placed::Foreign(canonical_std(&named.name));
        }
        let Some(at) = self.names.package_at(&named.home) else {
            return Placed::Unplaced(format!(
                "`{}` is written outside these packages",
                named.name
            ));
        };
        let path: Vec<String> = named
            .krate
            .iter()
            .cloned()
            .chain(named.prefix.iter().cloned())
            .chain(std::iter::once(named.name.clone()))
            .collect();
        let resolution = self.names.resolve(at, &named.module, &path);
        if resolution.cut {
            return Placed::Cut(format!(
                "the type path `{}` in {} {}",
                path.join("::"),
                named.home,
                named.module
            ));
        }
        let mut types = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut foreign = BTreeSet::new();
        let mut trait_found = false;
        for found in &resolution.named {
            match found {
                Named::Type(at, module, name) => {
                    types.insert((*at, module.clone(), name.clone()));
                }
                Named::Alias(at, module, name) => {
                    aliases.insert((*at, module.clone(), name.clone()));
                }
                Named::Foreign(path) => {
                    foreign.insert(path.clone());
                }
                Named::Trait(..) => trait_found = true,
                Named::Module(..) | Named::Function(..) | Named::Value(..) | Named::Variant(..) => {
                }
            }
        }
        match (types.len(), aliases.len(), foreign.len()) {
            (1.., 0, 0) => Placed::Types(types.into_iter().collect()),
            (0, 1, 0) => Placed::Alias(aliases.into_iter().next().unwrap_or_default()),
            (_, 1.., 0) => {
                // A name a type declares on one arm and an alias on another: the types of every
                // arm, when each alias names a type.
                let mut keys = types;
                for alias in &aliases {
                    for target in self.alias_targets(alias, &named.args) {
                        let Ty::Named(target) = &target else {
                            return Placed::Unplaced(format!(
                                "the type path `{}` names an alias of no named type",
                                path.join("::")
                            ));
                        };
                        let Placed::Types(found) = self.place(target) else {
                            return Placed::Unplaced(format!(
                                "the type path `{}` names a type on one arm and an alias of no \
                                 type on another",
                                path.join("::")
                            ));
                        };
                        keys.extend(found);
                    }
                }
                Placed::Types(keys.into_iter().collect())
            }
            (0, 0, 1) => Placed::Foreign(foreign.into_iter().next().unwrap_or_default()),
            (0, 0, 0) if trait_found => Placed::Trait,
            (0, 0, 0) => Placed::Unplaced(resolution.refused.unwrap_or_else(|| {
                format!(
                    "the type path `{}` names nothing the module tree holds",
                    path.join("::")
                )
            })),
            _ => Placed::Unplaced(format!(
                "the type path `{}` names types, aliases or foreign paths at once",
                path.join("::")
            )),
        }
    }

    /// `ty` with every alias followed to what it names, its arguments substituted. `Err` is a
    /// chain past the limit.
    pub(crate) fn expand(&self, ty: Ty) -> Result<Ty, String> {
        let mut ty = ty;
        for _ in 0..DEREF_STEPS {
            let Ty::Named(named) = &ty else {
                return Ok(ty);
            };
            let Placed::Alias(key) = self.place(named) else {
                return Ok(ty);
            };
            let targets = self.alias_targets(&key, &named.args);
            let [target] = targets.as_slice() else {
                return Ok(Ty::unknown(format!(
                    "the arms of the alias `{}` name {} types",
                    named.name,
                    if targets.is_empty() {
                        "no"
                    } else {
                        "different"
                    }
                )));
            };
            ty = target.clone();
        }
        Err("an alias chain longer than the hop limit".to_owned())
    }

    /// What an alias names with `args` substituted, once per distinct target its admitted arms
    /// write: one alias on two arms naming one type is that type, and two types are two (never
    /// the last one read).
    fn alias_targets(&self, key: &TypeKey, args: &[Ty]) -> Vec<Ty> {
        let mut targets: Vec<Ty> = Vec::new();
        for alias in self.aliases.get(key).into_iter().flatten() {
            if !alias.admitted {
                continue;
            }
            let target = alias.target.substitute(&generic_map(&alias.generics, args));
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        targets
    }

    /// The standard type `ty` is, when it is one of the fixed list's.
    pub(crate) fn std_of(&self, ty: &Ty) -> Option<(Std, Vec<Ty>)> {
        let Ty::Named(named) = ty else {
            return None;
        };
        match self.place(named) {
            Placed::Foreign(path)
                if path.first().is_some_and(|first| {
                    matches!(first.as_str(), "std" | "core" | "alloc" | "str")
                }) || std_kind(&named.name).is_some_and(|_| path.len() == 1) =>
            {
                std_kind(path.last().map_or("", String::as_str))
                    .map(|kind| (kind, named.args.clone()))
            }
            _ => None,
        }
    }

    // ── members of a type ─────────────────────────────────────────────────

    fn sig_answer(&self, items: &[ItemId], args: &[Ty]) -> (Option<Ty>, Vec<Option<Vec<Ty>>>) {
        let mut ret = None;
        let mut by_position = Vec::new();
        for item in items {
            let Some(sig) = self.sigs.get(item) else {
                continue;
            };
            let mut map = substitution(&sig.impl_params, &sig.self_args, args);
            for name in &sig.generics {
                map.insert(name.clone(), Ty::unknown(format!("the generic `{name}`")));
            }
            let found = sig.ret.substitute(&map);
            let found = if found.is_a_parameter() {
                Ty::unknown("a generic return type")
            } else {
                found
            };
            let params: Vec<Ty> = sig
                .params
                .iter()
                .map(|param| param.substitute(&map))
                .collect();
            match &ret {
                Some(earlier) if *earlier != found => {
                    ret = Some(Ty::unknown(
                        "declarations that disagree about what they return",
                    ));
                }
                Some(_) => {}
                None => {
                    ret = Some(found);
                    by_position = closures_by_position(&params);
                }
            }
        }
        (ret, by_position)
    }

    /// Items answering a call, with the ones outside the arms told apart: none in the arms is
    /// [`Unresolved::OtherArm`].
    fn items(&self, found: Vec<ItemId>, via: Via, args: &[Ty]) -> Answer {
        let (inside, outside): (Vec<ItemId>, Vec<ItemId>) =
            found.into_iter().partition(|id| self.is_admitted(*id));
        if inside.is_empty() {
            return Answer::unresolved(Unresolved::OtherArm {
                candidates: outside,
            });
        }
        let (ret, by_position) = self.sig_answer(&inside, args);
        Answer {
            targets: inside
                .into_iter()
                .map(|id| (Target::Item(id), via.clone()))
                .collect(),
            ret,
            by_position,
            closure: Vec::new(),
        }
    }

    /// The trait impls of `key` declaring `name`, by trait; and the first-party traits it
    /// implements, with each impl.
    fn trait_members(&self, key: &SelfKey, name: &str) -> TraitMembers {
        let mut by_trait: BTreeMap<TraitRef, Vec<ItemId>> = BTreeMap::new();
        let mut implemented = Vec::new();
        for &at_impl in self.type_impls.get(key).into_iter().flatten() {
            let block = &self.impls[at_impl];
            let declared: Vec<ItemId> = block
                .methods
                .iter()
                .copied()
                .filter(|id| self.record(*id).name() == name)
                .collect();
            for trait_ref in &block.traits {
                if let TraitRef::First(id) = trait_ref {
                    implemented.push((id.clone(), at_impl));
                }
                if !declared.is_empty() {
                    by_trait
                        .entry(trait_ref.clone())
                        .or_default()
                        .extend(declared.iter().copied());
                }
            }
        }
        (by_trait, implemented)
    }

    /// **`name` on a first-party type or a foreign type**, its own members only: an inherent
    /// method, else the one trait impl declaring it (several traits: ambiguous), else a trait
    /// default the type's impl does not override; `None` when the type has none of these.
    fn members(&self, key: &SelfKey, name: &str, args: &[Ty]) -> Option<Answer> {
        if let SelfKey::Type(type_key) = key
            && let Some(found) = self.inherent.get(&(type_key.clone(), name.to_owned()))
        {
            return Some(self.items(found.clone(), Via::Direct, args));
        }
        let (by_trait, implemented) = self.trait_members(key, name);
        match by_trait.len() {
            0 => {}
            1 => {
                let found = by_trait.into_values().next().unwrap_or_default();
                return Some(self.items(found, Via::Direct, args));
            }
            _ => {
                return Some(Answer::unresolved(Unresolved::Ambiguous {
                    method: name.to_owned(),
                    candidates: by_trait.into_values().flatten().collect(),
                }));
            }
        }
        let defaults: Vec<ItemId> = implemented
            .iter()
            .flat_map(|(id, _)| self.trait_declared.get(id).into_iter().flatten())
            .copied()
            .filter(|id| {
                let record = self.record(*id);
                record.name() == name && record.body().is_some()
            })
            .collect();
        if !defaults.is_empty() {
            return Some(self.items(defaults, Via::Direct, args));
        }
        None
    }

    /// What a derive or `#[from]` generates for `key` under `name`.
    fn generated(&self, key: &TypeKey, name: &str) -> Option<Answer> {
        let info = self.types.get(key)?;
        for (derive, methods) in DERIVED {
            if info.derives.contains(derive) && methods.contains(&name) {
                return Some(Answer::unresolved(Unresolved::Generated {
                    by: format!("#[derive({derive})]"),
                }));
            }
        }
        if name == "from" && info.from_field {
            return Some(Answer::unresolved(Unresolved::Generated {
                by: "#[from]".to_owned(),
            }));
        }
        None
    }

    /// **`Type::name(…)`**, the type written as `owner`.
    pub(crate) fn associated(&self, owner: &Ty, name: &str, written: &Written) -> Answer {
        let owner = match self.expand(owner.clone()) {
            Ok(owner) => owner,
            Err(why) => return Answer::unresolved(Unresolved::Cutoff { what: why }),
        };
        match &owner {
            Ty::Traits(bounds) => self.dispatch_bounds(bounds, name),
            Ty::Param(param) => self.dispatch_param(param, name, written),
            Ty::Named(named) => match self.place(named) {
                Placed::Types(keys) => {
                    let mut answer = merge(
                        keys.iter()
                            .map(|key| self.associated_on(key, named, name))
                            .collect(),
                    );
                    if answer.ret.is_none() && builds_itself(name) {
                        answer.ret = Some(owner.clone());
                    }
                    // `Type::method(x, …)`: the receiver is the first argument.
                    if answer.targets.iter().any(|(target, _)| {
                        matches!(target, Target::Item(id)
                            if self.sigs.get(id).is_some_and(|sig| sig.receiver != Receiver::None))
                    }) {
                        answer.by_position.insert(0, None);
                    }
                    answer
                }
                Placed::Foreign(path) => {
                    if let Some(answer) =
                        self.members(&SelfKey::Foreign(path.clone()), name, &named.args)
                    {
                        return answer;
                    }
                    let mut full = path.clone();
                    full.push(name.to_owned());
                    let mut answer = Answer::one(Target::Foreign(full), Via::Direct);
                    if builds_itself(name) {
                        answer.ret = Some(owner.clone());
                    }
                    answer
                }
                Placed::Alias(_) => Answer::unresolved(Unresolved::Path {
                    written: format!("{}::{name}", named.name),
                    why: "an alias that names no type".to_owned(),
                }),
                Placed::Trait => {
                    let bounds = vec![named.clone()];
                    self.dispatch_bounds(&bounds, name)
                }
                Placed::Unplaced(why) => Answer::unresolved(Unresolved::Path {
                    written: format!("{}::{name}", named.name),
                    why,
                }),
                Placed::Cut(what) => Answer::unresolved(Unresolved::Cutoff { what }),
            },
            other => Answer::unresolved(Unresolved::Path {
                written: format!("{}::{name}", other.spelling()),
                why: "a call on a type that is not a path".to_owned(),
            }),
        }
    }

    /// `Type::name` on one first-party type: a variant's constructor, its members, what a derive
    /// generates.
    fn associated_on(&self, key: &TypeKey, named: &TyNamed, name: &str) -> Answer {
        if self
            .types
            .get(key)
            .is_some_and(|info| info.variants.contains_key(name))
        {
            return Answer::one(Target::Constructor, Via::Direct);
        }
        self.members(&SelfKey::Type(key.clone()), name, &named.args)
            .or_else(|| self.generated(key, name))
            .unwrap_or_else(|| {
                Answer::unresolved(Unresolved::Path {
                    written: format!("{}::{name}", named.name),
                    why: format!("`{}` declares no associated function `{name}`", named.name),
                })
            })
    }

    /// **`receiver.name(…)`**, with the auto-deref the language applies: `&`, `Box`, `Rc`/`Arc`
    /// and first-party `Deref` targets, N levels up to [`DEREF_STEPS`], each first-party `Deref`
    /// step an edge of its own.
    pub(crate) fn method(&self, receiver: &Ty, name: &str, written: &Written) -> Answer {
        self.method_at(receiver, name, written, 0)
    }

    fn method_at(&self, ty: &Ty, name: &str, written: &Written, depth: usize) -> Answer {
        if depth > DEREF_STEPS {
            return Answer::unresolved(Unresolved::Cutoff {
                what: format!("a deref chain longer than {DEREF_STEPS} steps before `{name}`"),
            });
        }
        let ty = match self.expand(ty.clone()) {
            Ok(ty) => ty,
            Err(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
        };
        match ty {
            Ty::Ref { inner, .. } => self.method_at(&inner, name, written, depth + 1),
            Ty::Traits(bounds) => self.dispatch_bounds(&bounds, name),
            Ty::Param(param) => self.dispatch_param(&param, name, written),
            Ty::Assoc {
                param,
                name: associated,
            } => self.dispatch_associated(&param, &associated, name, written),
            Ty::Iter(item) => Answer::one(
                Target::Foreign(foreign(&["core", "iter", "Iterator"], name)),
                Via::Direct,
            )
            .with_std(iterator_method(&item, name)),
            Ty::Entry(value) => Answer::one(
                Target::Foreign(foreign(&["std", "collections", "Entry"], name)),
                Via::Direct,
            )
            .with_std(entry_method(&value, name)),
            Ty::Slice(element) => Answer::one(
                Target::Foreign(foreign(&["core", "slice"], name)),
                Via::Direct,
            )
            .with_std(
                std_method(Std::Vec, &[*element], name)
                    .or_else(|| prelude_method(name, &Ty::unknown("a slice"))),
            ),
            Ty::Tuple(_) => Answer::one(Target::Foreign(foreign(&["()"], name)), Via::Direct)
                .with_std(prelude_method(name, &Ty::unknown("a tuple"))),
            Ty::Fn(_) => Answer::unresolved(Unresolved::Dynamic {
                what: format!("`{name}` on a closure or a function value"),
                candidates: Vec::new(),
            }),
            Ty::Unknown(_) => self.untyped(name),
            Ty::Named(named) => match self.place(&named) {
                Placed::Types(keys) => merge(
                    keys.iter()
                        .map(|key| self.method_on(key, &named, name, written, depth))
                        .collect(),
                ),
                Placed::Foreign(path) => {
                    if let Some(answer) =
                        self.members(&SelfKey::Foreign(path.clone()), name, &named.args)
                    {
                        return answer;
                    }
                    let kind = self.std_of(&Ty::Named(named.clone())).map(|(kind, _)| kind);
                    let listed = kind.and_then(|kind| std_method(kind, &named.args, name));
                    if (matches!(kind, Some(Std::Box | Std::Shared)) || is_std_guard(&path))
                        && listed.is_none()
                    {
                        let pointee = named
                            .args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Ty::unknown("a pointer without its argument"));
                        return self.method_at(&pointee, name, written, depth + 1);
                    }
                    let found = listed
                        .or_else(|| external_method(&named.name, name))
                        .or_else(|| prelude_method(name, &Ty::Named(named.clone())));
                    let mut full = path;
                    full.push(name.to_owned());
                    let mut answer =
                        Answer::one(Target::Foreign(full), Via::Direct).with_std(found);
                    if is_primitive(&named) && returns_self(name) {
                        answer.ret = Some(Ty::Named(named.clone()));
                    }
                    answer
                }
                Placed::Alias(_) | Placed::Trait => self.untyped(name),
                Placed::Unplaced(why) => Answer::unresolved(Unresolved::Path {
                    written: format!("{}.{name}", named.name),
                    why,
                }),
                Placed::Cut(what) => Answer::unresolved(Unresolved::Cutoff { what }),
            },
        }
    }

    /// `name` on one first-party type: its members, else its `Deref` target's (the steps' edges
    /// first), else what a derive generates, else a standard prelude trait's.
    fn method_on(
        &self,
        key: &TypeKey,
        named: &TyNamed,
        name: &str,
        written: &Written,
        depth: usize,
    ) -> Answer {
        let self_key = SelfKey::Type(key.clone());
        if let Some(answer) = self.members(&self_key, name, &named.args) {
            return answer;
        }
        if let Some((target, mut steps)) = self.deref_of(&self_key, &named.args) {
            let mut answer = self.method_at(&target, name, written, depth + 1);
            steps.append(&mut answer.targets);
            answer.targets = steps;
            return answer;
        }
        if let Some(answer) = self.generated(key, name) {
            return answer;
        }
        if let Some(found) = prelude_method(name, &Ty::Named(named.clone())) {
            return Answer::one(
                Target::Foreign(foreign(&["std", "{prelude trait}"], name)),
                Via::Direct,
            )
            .with_std(Some(found));
        }
        Answer::unresolved(Unresolved::Path {
            written: format!("{}.{name}", named.name),
            why: format!("`{}` declares no method `{name}`", named.name),
        })
    }

    /// A method on a receiver the rules do not type: every first-party method of the name, never
    /// a pick.
    pub(crate) fn untyped(&self, name: &str) -> Answer {
        let candidates: Vec<ItemId> = self
            .method_names
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| self.is_admitted(*id) && self.record(*id).body().is_some())
            .collect();
        Answer::unresolved(Unresolved::Ambiguous {
            method: name.to_owned(),
            candidates,
        })
    }

    /// The `Deref` target of a first-party type, with the deref methods that run on the way.
    pub(crate) fn deref_of(&self, key: &SelfKey, args: &[Ty]) -> Option<(Ty, Vec<(Target, Via)>)> {
        let mut target = None;
        let mut steps = Vec::new();
        for &at_impl in self.type_impls.get(key).into_iter().flatten() {
            let block = &self.impls[at_impl];
            if !block.admitted {
                continue;
            }
            let is_deref = block.traits.iter().any(|it| {
                matches!(it, TraitRef::Foreign(path) if path.last().is_some_and(|last| last == "Deref" || last == "DerefMut"))
            });
            if !is_deref {
                continue;
            }
            if let Some(found) = &block.deref_target {
                let map = substitution(&block.impl_params, &block.self_args, args);
                target.get_or_insert_with(|| found.substitute(&map));
            }
            steps.extend(
                block
                    .methods
                    .iter()
                    .filter(|id| self.is_admitted(**id))
                    .map(|id| (Target::Item(*id), Via::Deref)),
            );
        }
        target.map(|target| (target, steps))
    }

    // ── trait dispatch ────────────────────────────────────────────────────

    fn supertraits_of(&self, id: &TraitId) -> Result<BTreeSet<TraitId>, String> {
        if let Some(known) = self.supertraits.borrow().get(id) {
            return known.clone();
        }
        let found = self
            .traits
            .supertraits_checked(id)
            .map_err(|_| format!("a supertrait of `{id}`"));
        self.supertraits
            .borrow_mut()
            .insert(id.clone(), found.clone());
        found
    }

    /// The traits a written bound list names: first-party identities, and the foreign paths.
    fn bound_traits(
        &self,
        bounds: &[TyNamed],
    ) -> Result<(BTreeSet<TraitId>, Vec<Vec<String>>), String> {
        let mut first = BTreeSet::new();
        let mut others = Vec::new();
        for bound in bounds {
            let path: Vec<String> = bound
                .krate
                .iter()
                .cloned()
                .chain(bound.prefix.iter().cloned())
                .chain(std::iter::once(bound.name.clone()))
                .collect();
            match self
                .traits
                .resolve_checked(&bound.home, &bound.module, &path)
            {
                Ok(found) if !found.is_empty() => first.extend(found),
                Ok(_) => {
                    let foreign_path = match self.names.package_at(&bound.home) {
                        Some(at) => self.foreign_trait(at, &bound.module, &path),
                        None => path,
                    };
                    others.push(foreign_path);
                }
                Err(_) => {
                    return Err(format!(
                        "the trait path `{}` in {} {}",
                        path.join("::"),
                        bound.home,
                        bound.module
                    ));
                }
            }
        }
        Ok((first, others))
    }

    pub(crate) fn dispatch_bounds(&self, bounds: &[TyNamed], name: &str) -> Answer {
        match self.bound_traits(bounds) {
            Ok((first, others)) => self.dispatch(&first, &others, name),
            Err(what) => Answer::unresolved(Unresolved::Cutoff { what }),
        }
    }

    fn dispatch_param(&self, param: &str, name: &str, written: &Written) -> Answer {
        match written.trait_bounds.get(param) {
            Some(bounds) if !bounds.is_empty() => self.dispatch_bounds(bounds, name),
            _ => Answer::unresolved(Unresolved::Path {
                written: format!("{param}::{name}"),
                why: format!("the generic `{param}` is bounded by no trait"),
            }),
        }
    }

    /// `B::Glass`'s method: the traits the associated type's declarations bound it by.
    fn dispatch_associated(
        &self,
        param: &str,
        associated: &str,
        name: &str,
        written: &Written,
    ) -> Answer {
        let Some(bounds) = written.trait_bounds.get(param) else {
            return self.untyped(name);
        };
        let (first, _) = match self.bound_traits(bounds) {
            Ok(found) => found,
            Err(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
        };
        let mut found = Vec::new();
        for id in &first {
            for declaration in self.traits.declarations(id) {
                let Some(at) = self.names.package_at(&id.package) else {
                    continue;
                };
                let index = self.index(at);
                let Ok(parsed) = syn::parse_str::<syn::ItemTrait>(index.text(declaration.whole()))
                else {
                    continue;
                };
                let written = self.written(&id.package, &id.module_path);
                for member in &parsed.items {
                    if let syn::TraitItem::Type(item) = member
                        && item.ident == associated
                    {
                        found.extend(written.trait_paths(item.bounds.iter()));
                    }
                }
            }
        }
        if found.is_empty() {
            return self.untyped(name);
        }
        self.dispatch_bounds(&found, name)
    }

    /// **A call of `name` on a value known only by the traits `stated`** (first-party) and
    /// `foreign`: the method of every admitted impl of each declaring trait (a supertrait's only
    /// for a type implementing a stated trait), the default only where an impl does not override
    /// it or there is none; a first-party trait with neither, and a foreign trait object, are
    /// [`Unresolved::Dynamic`].
    pub(crate) fn dispatch(
        &self,
        stated: &BTreeSet<TraitId>,
        foreign_bounds: &[Vec<String>],
        name: &str,
    ) -> Answer {
        let mut closure = stated.clone();
        let mut work: Vec<TraitId> = stated.iter().cloned().collect();
        while let Some(id) = work.pop() {
            match self.supertraits_of(&id) {
                Ok(supertraits) => {
                    for supertrait in supertraits {
                        if closure.insert(supertrait.clone()) {
                            work.push(supertrait);
                        }
                    }
                }
                Err(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
            }
        }
        let declaring: Vec<&TraitId> = closure
            .iter()
            .filter(|id| {
                self.trait_declared
                    .get(*id)
                    .is_some_and(|methods| methods.iter().any(|m| self.record(*m).name() == name))
            })
            .collect();
        if !declaring.is_empty() {
            let implementors: BTreeSet<&SelfKey> = stated
                .iter()
                .flat_map(|id| self.trait_impls.get(id).into_iter().flatten())
                .map(|at| &self.impls[*at])
                .filter(|block| block.admitted)
                .map(|block| &block.self_key)
                .collect();
            let mut targets: Vec<(Target, Via)> = Vec::new();
            let mut ret = None;
            for id in declaring {
                let blocks: Vec<&ImplInfo> = self
                    .trait_impls
                    .get(id)
                    .into_iter()
                    .flatten()
                    .map(|at| &self.impls[*at])
                    .filter(|block| block.admitted)
                    .filter(|block| stated.contains(id) || implementors.contains(&block.self_key))
                    .collect();
                let mut default_runs = blocks.is_empty();
                for block in blocks {
                    let overrides: Vec<ItemId> = block
                        .methods
                        .iter()
                        .copied()
                        .filter(|m| self.record(*m).name() == name && self.is_admitted(*m))
                        .collect();
                    default_runs |= overrides.is_empty();
                    targets.extend(
                        overrides
                            .into_iter()
                            .map(|m| (Target::Item(m), Via::Dispatch(id.clone()))),
                    );
                }
                let declared: Vec<ItemId> = self.trait_declared[id]
                    .iter()
                    .copied()
                    .filter(|m| self.record(*m).name() == name)
                    .collect();
                if ret.is_none() {
                    ret = declared
                        .first()
                        .and_then(|m| self.sigs.get(m))
                        .map(|sig| sig.ret.clone());
                }
                if default_runs {
                    targets.extend(
                        declared
                            .into_iter()
                            .filter(|m| self.record(*m).body().is_some() && self.is_admitted(*m))
                            .map(|m| (Target::Item(m), Via::Dispatch(id.clone()))),
                    );
                }
            }
            let mut seen = BTreeSet::new();
            targets.retain(|(target, _)| seen.insert(target.clone()));
            if targets.is_empty() {
                return Answer::unresolved(Unresolved::Dynamic {
                    what: format!(
                        "`{name}` of {}, which no first-party implementation answers",
                        closure
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    candidates: Vec::new(),
                });
            }
            return Answer {
                targets,
                ret: ret.filter(|ret| !ret.is_a_parameter()),
                ..Answer::default()
            };
        }
        if !foreign_bounds.is_empty() {
            let candidates: Vec<ItemId> = foreign_bounds
                .iter()
                .flat_map(|path| self.foreign_trait_impls.get(path).into_iter().flatten())
                .map(|at| &self.impls[*at])
                .filter(|block| block.admitted)
                .flat_map(|block| block.methods.iter().copied())
                .filter(|m| self.record(*m).name() == name && self.is_admitted(*m))
                .collect();
            return Answer::unresolved(Unresolved::Dynamic {
                what: format!(
                    "`{name}` on a value known by {}",
                    foreign_bounds
                        .iter()
                        .map(|path| format!("`{}`", path.join("::")))
                        .collect::<Vec<_>>()
                        .join(" + ")
                ),
                candidates,
            });
        }
        Answer::unresolved(Unresolved::Path {
            written: name.to_owned(),
            why: format!("no trait the receiver is known by declares `{name}`"),
        })
    }

    /// **`<X as Tr>::name`** (`trait_path` `None`: `<X>::name`).
    pub(crate) fn qualified(
        &self,
        x: &Ty,
        trait_bound: Option<&TyNamed>,
        name: &str,
        written: &Written,
    ) -> Answer {
        let Some(bound) = trait_bound else {
            return self.associated(x, name, written);
        };
        let (first, foreign_bounds) = match self.bound_traits(std::slice::from_ref(bound)) {
            Ok(found) => found,
            Err(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
        };
        let x = match self.expand(x.clone()) {
            Ok(x) => x,
            Err(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
        };
        let mut x = x;
        while let Ty::Ref { inner, .. } = x {
            x = *inner;
        }
        let key = match &x {
            Ty::Param(_) | Ty::Traits(_) => return self.dispatch(&first, &foreign_bounds, name),
            Ty::Named(named) => match self.place(named) {
                Placed::Types(keys) => {
                    let keys: Vec<SelfKey> = keys.into_iter().map(SelfKey::Type).collect();
                    let args = named.args.clone();
                    return merge(
                        keys.iter()
                            .map(|key| {
                                self.qualified_on(key, &first, &foreign_bounds, bound, name, &args)
                            })
                            .collect(),
                    );
                }
                Placed::Foreign(path) => SelfKey::Foreign(path),
                Placed::Cut(what) => return Answer::unresolved(Unresolved::Cutoff { what }),
                Placed::Alias(_) | Placed::Trait | Placed::Unplaced(_) => {
                    return self.dispatch(&first, &foreign_bounds, name);
                }
            },
            _ => return self.dispatch(&first, &foreign_bounds, name),
        };
        let args = match &x {
            Ty::Named(named) => named.args.clone(),
            _ => Vec::new(),
        };
        self.qualified_on(&key, &first, &foreign_bounds, bound, name, &args)
    }

    /// `<X as Tr>::name` for one `X`: its impl of the trait, else the trait's default, else (a
    /// foreign trait) the foreign path.
    fn qualified_on(
        &self,
        key: &SelfKey,
        first: &BTreeSet<TraitId>,
        foreign_bounds: &[Vec<String>],
        bound: &TyNamed,
        name: &str,
        args: &[Ty],
    ) -> Answer {
        let wanted: Vec<TraitRef> = first
            .iter()
            .cloned()
            .map(TraitRef::First)
            .chain(foreign_bounds.iter().cloned().map(TraitRef::Foreign))
            .collect();
        let mut found = Vec::new();
        for &at_impl in self.type_impls.get(key).into_iter().flatten() {
            let block = &self.impls[at_impl];
            if block.traits.iter().any(|it| wanted.contains(it)) {
                found.extend(
                    block
                        .methods
                        .iter()
                        .copied()
                        .filter(|m| self.record(*m).name() == name),
                );
            }
        }
        if !found.is_empty() {
            return self.items(found, Via::Direct, args);
        }
        let defaults: Vec<ItemId> = first
            .iter()
            .flat_map(|id| self.trait_declared.get(id).into_iter().flatten())
            .copied()
            .filter(|m| self.record(*m).name() == name && self.record(*m).body().is_some())
            .collect();
        if !defaults.is_empty() {
            return self.items(defaults, Via::Direct, args);
        }
        if first.is_empty()
            && let Some(path) = foreign_bounds.first()
        {
            let mut full = path.clone();
            full.push(name.to_owned());
            return Answer::one(Target::Foreign(full), Via::Direct);
        }
        Answer::unresolved(Unresolved::Path {
            written: format!("<_ as {}>::{name}", bound.name),
            why: "neither the implementation nor the trait declares it".to_owned(),
        })
    }

    // ── fields ────────────────────────────────────────────────────────────

    /// `base.field`, with the auto-deref the language applies; each first-party `Deref` step is
    /// pushed onto `steps`.
    pub(crate) fn field(&self, base: &Ty, field: &str, steps: &mut Vec<(Target, Via)>) -> Ty {
        let mut ty = base.clone();
        for _ in 0..DEREF_STEPS {
            ty = match self.expand(ty) {
                Ok(ty) => ty,
                Err(why) => return Ty::unknown(why),
            };
            match ty {
                Ty::Ref { inner, .. } => ty = *inner,
                Ty::Tuple(items) => {
                    return field
                        .parse::<usize>()
                        .ok()
                        .and_then(|at| items.get(at).cloned())
                        .unwrap_or_else(|| Ty::unknown("a tuple field that is not there"));
                }
                Ty::Named(named) => match self.place(&named) {
                    Placed::Types(keys) => {
                        let declared: BTreeSet<Ty> = keys
                            .iter()
                            .filter_map(|key| {
                                let info = self.types.get(key)?;
                                let (_, found) =
                                    info.fields.iter().find(|(name, _)| name == field)?;
                                let map = generic_map(&info.generics, &named.args);
                                Some(found.substitute(&map))
                            })
                            .collect();
                        if declared.len() == 1 {
                            return declared
                                .into_iter()
                                .next()
                                .unwrap_or_else(|| Ty::unknown("a field"));
                        }
                        if declared.len() > 1 {
                            return Ty::unknown(format!(
                                "the arms of `{}` disagree about `{field}`",
                                named.name
                            ));
                        }
                        let [key] = keys.as_slice() else {
                            return Ty::unknown(format!(
                                "`{}` on several arms declares no field `{field}`",
                                named.name
                            ));
                        };
                        match self.deref_of(&SelfKey::Type(key.clone()), &named.args) {
                            Some((target, deref_steps)) => {
                                steps.extend(deref_steps);
                                ty = target;
                            }
                            None => {
                                return Ty::unknown(format!(
                                    "`{}` declares no field `{field}`",
                                    named.name
                                ));
                            }
                        }
                    }
                    Placed::Foreign(path) => match self.std_of(&Ty::Named(named.clone())) {
                        Some((Std::Box | Std::Shared, args)) => {
                            ty = args
                                .first()
                                .cloned()
                                .unwrap_or_else(|| Ty::unknown("a pointer"));
                        }
                        _ if is_std_guard(&path) => {
                            ty = named
                                .args
                                .first()
                                .cloned()
                                .unwrap_or_else(|| Ty::unknown("a guard"));
                        }
                        _ => {
                            return Ty::unknown(format!("a field of the foreign `{}`", named.name));
                        }
                    },
                    _ => return Ty::unknown(format!("a field of `{}`", named.name)),
                },
                _ => return Ty::unknown("a field of a type the rules do not read"),
            }
        }
        Ty::unknown("a deref chain longer than the limit")
    }

    /// The variant fields a pattern binds over `ty`: `Some`/`Ok`/`Err`, or a first-party enum's.
    pub(crate) fn variant_fields(&self, ty: &Ty, variant: &str) -> Option<Vec<(String, Ty)>> {
        let Ty::Named(named) = ty else {
            return None;
        };
        if let Some((kind, args)) = self.std_of(ty) {
            let inner = match (kind, variant) {
                (Std::Option, "Some") | (Std::Result, "Ok") => args.first().cloned(),
                (Std::Result, "Err") => args.get(1).cloned(),
                _ => None,
            };
            return inner.map(|inner| vec![("0".to_owned(), inner)]);
        }
        let Placed::Types(keys) = self.place(named) else {
            return None;
        };
        let info = keys.iter().find_map(|key| self.types.get(key))?;
        let map = generic_map(&info.generics, &named.args);
        if let Some(fields) = info.variants.get(variant) {
            return Some(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.substitute(&map)))
                    .collect(),
            );
        }
        if info.variants.is_empty() {
            return Some(
                info.fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.substitute(&map)))
                    .collect(),
            );
        }
        None
    }
}

/// The `Self` of a trait's own method: a value known by that trait.
pub(crate) fn trait_self(id: &TraitId) -> Ty {
    match named_of(&id.package, &id.module_path, &id.name) {
        Ty::Named(named) => Ty::Traits(vec![named]),
        other => other,
    }
}

/// A first-party declaration's path as a written type: `crate::…::name`, read in its own module.
pub(crate) fn named_of(package: &str, module: &str, name: &str) -> Ty {
    let mut prefix: Vec<String> = module.split("::").map(str::to_owned).collect();
    let krate = if prefix.first().is_some_and(|first| first == "crate") {
        prefix.remove(0);
        Some("crate".to_owned())
    } else {
        None
    };
    Ty::Named(TyNamed {
        name: name.to_owned(),
        krate,
        prefix,
        args: Vec::new(),
        home: package.to_owned(),
        module: module.to_owned(),
    })
}

fn foreign(path: &[&str], name: &str) -> Vec<String> {
    path.iter()
        .map(|segment| (*segment).to_owned())
        .chain(std::iter::once(name.to_owned()))
        .collect()
}

pub(crate) fn generic_names(generics: &syn::Generics) -> Vec<String> {
    generics
        .params
        .iter()
        .filter_map(|param| match param {
            syn::GenericParam::Type(ty) => Some(ty.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn derives(attrs: &[syn::Attribute]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for attribute in attrs {
        if !attribute.path().is_ident("derive") {
            continue;
        }
        let _ = attribute.parse_nested_meta(|meta| {
            if let Some(last) = meta.path.segments.last() {
                found.insert(last.ident.to_string());
            }
            Ok(())
        });
    }
    found
}

pub(crate) fn substitution(
    params: &[String],
    self_args: &[Ty],
    args: &[Ty],
) -> BTreeMap<String, Ty> {
    let mut map: BTreeMap<String, Ty> = params
        .iter()
        .map(|name| {
            (
                name.clone(),
                Ty::unknown(format!("the unbound parameter `{name}`")),
            )
        })
        .collect();
    for (written, given) in self_args.iter().zip(args) {
        if let Ty::Param(name) = written
            && params.contains(name)
        {
            map.insert(name.clone(), given.clone());
        }
    }
    map
}

/// Whether the arms admit something declared by `declared` with `variant`, written at byte
/// `offset` of `index`: any one of its declaration paths, with the predicates written on it and
/// inside the file.
pub(crate) fn admits(
    arms: &Arms<'_>,
    package: &str,
    index: &Index,
    offset: usize,
    declared: &[DeclarationPath],
    variant: &ConditionalVariant,
) -> bool {
    let Some(file) = index.file_at(offset) else {
        return false;
    };
    declared.iter().any(|path| {
        let predicates: Vec<String> = path
            .predicates
            .iter()
            .chain(&variant.predicates)
            .cloned()
            .collect();
        arms.admits(&ArmQuery {
            package,
            file,
            predicates: &predicates,
            compilation: path.compilation.and(variant.compilation),
        })
    })
}

/// Answers for the arms of one type, united: every target once, and the return type where they
/// agree.
fn merge(answers: Vec<Answer>) -> Answer {
    let mut out = Answer::default();
    let mut returns: Vec<Option<Ty>> = Vec::new();
    for answer in answers {
        for target in answer.targets {
            if !out.targets.contains(&target) {
                out.targets.push(target);
            }
        }
        returns.push(answer.ret);
        if out.by_position.is_empty() {
            out.by_position = answer.by_position;
        }
        if out.closure.is_empty() {
            out.closure = answer.closure;
        }
    }
    out.ret = match returns.split_first() {
        Some((first, rest)) if rest.iter().all(|other| other == first) => first.clone(),
        Some(_) => Some(Ty::unknown("arms that disagree about what they return")),
        None => None,
    };
    out
}

/// The associated functions that build their own type (`Vec::new()`), as the census reads them.
fn builds_itself(name: &str) -> bool {
    crate::census::types::builds_itself(name) || name == "with_capacity"
}

/// A primitive number, or a literal of one whose type is not written.
pub(crate) fn is_primitive(named: &TyNamed) -> bool {
    matches!(
        named.name.as_str(),
        "u8" | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "{integer}"
            | "{float}"
    )
}

/// The methods of the primitive numbers that give back a number of the receiver's own type.
fn returns_self(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "clamp"
            | "max"
            | "min"
            | "round"
            | "floor"
            | "ceil"
            | "trunc"
            | "sqrt"
            | "powi"
            | "powf"
            | "signum"
            | "saturating_add"
            | "saturating_sub"
            | "saturating_mul"
            | "wrapping_add"
            | "wrapping_sub"
            | "wrapping_mul"
            | "rem_euclid"
            | "div_euclid"
            | "mul_add"
            | "recip"
            | "fract"
            | "exp"
            | "ln"
            | "sin"
            | "cos"
            | "hypot"
            | "pow"
            | "abs_diff"
            | "next_power_of_two"
    )
}

/// The standard library's guards and wrappers that `Deref` to their first argument, as `Box` and
/// `Rc`/`Arc` do (a `static LANE: LazyLock<Lane>` among them): a method or a field they do not have
/// is their pointee's.
pub(crate) fn is_std_guard(path: &[String]) -> bool {
    path.first()
        .is_some_and(|first| matches!(first.as_str(), "std" | "core" | "alloc"))
        && path.last().is_some_and(|last| {
            matches!(
                last.as_str(),
                "MutexGuard"
                    | "RwLockReadGuard"
                    | "RwLockWriteGuard"
                    | "Ref"
                    | "RefMut"
                    | "ManuallyDrop"
                    | "AssertUnwindSafe"
                    | "Pin"
                    | "LazyLock"
                    | "LazyCell"
            )
        })
}

/// A declaration's generic parameters, each the argument a use site gives it — and a parameter
/// the use site leaves unwritten unknown, never a parameter of the reading's own scope.
fn generic_map(generics: &[String], args: &[Ty]) -> BTreeMap<String, Ty> {
    generics
        .iter()
        .enumerate()
        .map(|(at, name)| {
            (
                name.clone(),
                args.get(at)
                    .cloned()
                    .unwrap_or_else(|| Ty::unknown(format!("the unwritten argument `{name}`"))),
            )
        })
        .collect()
}
