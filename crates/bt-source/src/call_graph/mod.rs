//! **A resolved, execution-aware call graph over a program's packages** (T-BT-SOURCE-CALL-GRAPH,
//! S-2a of `docs/plans/design/side-effects-contract-2026-10-11.md` revision (c)2): every call
//! site of a body turned into its edges — to the items it can run, to a foreign callee as a
//! boundary, or to an **unresolved** edge with its kind and site, never to silence — and a walk
//! from a set of roots that stops at declared boundary items and carries every unresolved edge it
//! met with the path that reached it.
//!
//! Names are placed through the module tree by identity (`crate`/`self`/`super`, the module's own
//! items, `use` items and renames, `pub use` re-exports across packages, globs after the module's
//! own names, a crate name as a first segment) — never by a bare name. Receivers are typed by the
//! census's steps, from declarations: a type from a parameter, `self`, a `let` (written or
//! carried from its initialiser), a field, an index, a deref, a method or function whose
//! declaration is found, a pattern over a typed scrutinee — and a shadowing binding retypes the
//! name. Anything the steps do not type is untyped, and an untyped receiver's call is an
//! [`Unresolved::Ambiguous`] edge listing every first-party method of the name.
//!
//! **`?` values and variant patterns** (plan row 101):
//!
//! | # | Form | Rule |
//! |---|---|---|
//! | Q1 | `e?` | `e`'s type, aliases followed: a standard `Option<T>` or `Result<T, E>` gives `T` |
//! | Q2 | a foreign `Result` alias | Only the rows of the fixed table (`std::io::Result`, `std::fmt::Result`, `std::thread::Result`, `anyhow::Result`, `windows::core::Result`), followed to the standard `Result` each names, its error type included — so `?`, `Ok`/`Err` and the standard methods read it as one `Result`. Any other foreign type is not a `Result` to the rules, whatever its name |
//! | Q3 | variant patterns | `Some`/`Ok`/`Err` over Q1/Q2 types, a first-party enum's variant (tuple or struct form) or tuple struct, nested to any depth (`Ok(E::V(Some(x)))`), in `let`, `let … else`, `if let`, `while let` and `match` arms |
//! | Q4 | `..` | In a tuple or tuple-variant pattern the elements after `..` are the last fields, counted from the end; in a slice pattern `name @ ..` is a slice of the element. An element count the fields cannot hold types nothing |
//! | Q5 | the adapters that keep the value | `Option::ok_or`/`ok_or_else` and `Result::map_err`/`or_else`/`inspect`/`inspect_err` answer a `Result` of the same value; anyhow's `Context::context`/`with_context` on a `Result` or an `Option` an `anyhow::Result` of it |
//!
//! Refused, by kind (the receiver stays untyped, its call [`Unresolved::Ambiguous`]): `?` on a
//! type that is neither Q1 nor Q2 or on an untyped operand; a variant the scrutinee's type does
//! not declare, or a scrutinee that is untyped, a generic or `impl Trait`; an element count the
//! fields cannot hold; and a pattern a macro writes, or one `syn` keeps as tokens
//! (`Pat::Macro`, `Pat::Verbatim`): every name in it is bound untyped, so an outer binding of
//! that name does not show through. Held limit: anyhow's `context`/`with_context` are read by
//! method name on every standard `Option`/`Result`, whether or not the package depends on anyhow
//! (true while anyhow is the one dependency whose trait gives them these names); a first-party
//! trait's method of the name is asked first and wins.
//!
//! # The grammar
//!
//! | # | Shape | Resolution rule | Target |
//! |---|---|---|---|
//! | R1 | Path call `f()`, `a::b::f()`, `self::`/`super::`/`crate::`, `other_crate::m::f()` | The module tree, `use` names and renames, globs (first-party first), `pub use` re-exports across packages, function-body `use` items in their block. A name a single foreign glob supplies is that glob's; a prelude name is the prelude's. | [`Target::Item`] (each `cfg` variant) or [`Target::Foreign`] |
//! | R2 | Tuple-struct or tuple-variant constructor `S(x)`, `E::V(x)`, `Some(x)` | Placed as R1 | [`Target::Constructor`] (no body; not walked) |
//! | R3 | Associated function `Type::f()`, `Self::f()`, `Type::<A>::f()` | The type placed through the module tree: inherent impl first, then the one trait impl declaring `f` (several traits: ambiguous), then a trait default the type's impl does not override | [`Target::Item`] |
//! | R4 | Method `x.m()` on a stated receiver | Auto-deref through `&`, `Box`, `Rc`/`Arc` and first-party `Deref` targets, N levels up to 16; at each level the type's inherent method, then its one trait impl's, then a default. Each first-party `Deref` step is an edge to its `deref`/`deref_mut` ([`Via::Deref`]) | [`Target::Item`], or [`Target::Foreign`] on a foreign type |
//! | R5 | Method on a stated first-party trait: `dyn Tr` (bare, `&`, `Box`, `Arc`, `Rc`), `impl Tr`, `T: Tr` (inline or `where`), `self` in a default method, `Tr::m` / `T::m` | Every admitted impl's method; the default only for an impl that does not override it, or when there is none; supertraits, whose impls count only for types implementing the stated trait | [`Target::Item`] set, [`Via::Dispatch`]; no impl and no default: [`Unresolved::Dynamic`] |
//! | R6 | UFCS `<X as Tr>::m()` | `X` stated: that impl's `m`, else `Tr`'s default; `X` a bound generic: R5; a foreign trait on a first-party `X`: its impl, else foreign. `<X>::m` is R3 | [`Target::Item`] |
//! | R7 | Type alias `type A = B;` | Followed to `B` through [`crate::AliasRecord`]s, arguments substituted, up to the hop limit | as `B` |
//! | R8 | Inline closure call `(\|x\| …)(y)`; any closure literal | The closure's body is walked as part of the item it is written in; each edge found inside carries [`CallSite::closure`], and [`CallSite::closures`] names every closure open around it with the call it is an argument of ([`Handed`]: the call's site span and the argument's position), so a closure handed to a call is told from one bound, stored or called in place | [`Target::Closure`] |
//! | R9 | A path in value position: `spawn(worker)`, `.map(Self::f)` | Data — a `const` or `static` ([`crate::ValueRecord`]), an associated `const`, a unit struct or variant, a binding — is no edge (a `static` is typed by its declaration); anything else is placed as R1/R3 and **every** edge it gives is pushed | any [`Target`], [`Via::Value`] |
//! | R10 | A `fn` or `impl` written inside a body | Walked as part of the enclosing item | [`Target::Nested`] |
//! | R11 | A callee outside the packages handed in | Its full path after the first-party `use` items are followed (`std::sync::Mutex::lock`, `std::fs::write`) | [`Target::Foreign`]: a boundary, not a failure |
//!
//! # The unresolved kinds
//!
//! | Kind | When |
//! |---|---|
//! | [`Unresolved::Dynamic`] | A stored closure or a function pointer called; a call on an `Fn` parameter; a called value; a first-party trait object no implementation answers; a foreign trait object (`dyn std::io::Write`), carrying the first-party impls of that trait as candidates |
//! | [`Unresolved::Ambiguous`] | An untyped receiver (every first-party method of the name as candidates, possibly one, possibly none); several trait impls of one name on a stated type |
//! | [`Unresolved::Macro`] | Every macro invocation: its expansion is not seen. Arguments that parse as expressions are walked, their edges carrying [`CallSite::macro_arg`] |
//! | [`Unresolved::OtherArm`] | A callee only items outside the [`Arms`] answer; a site under a statement's or a match arm's `#[cfg]` the arms rule out |
//! | [`Unresolved::Generated`] | A callee a derive (`#[derive(Clone)]`'s `clone`) or `#[from]` generates |
//! | [`Unresolved::Path`] | A path the rules refuse or that names nothing: two foreign globs (or one and the prelude) that may supply a name, a type the module tree does not hold, a method a stated type does not declare, a qualified path whose trait is not read |
//! | [`Unresolved::Cutoff`] | A `use`/glob/alias chain past the hop limit, or a deref chain past 16 steps. No list may excuse it |
//! | [`Unresolved::Unparsed`] | A body `syn` does not parse. No list may excuse it |
//!
//! **Implicit calls are not edges**: a `Drop` at the end of a scope, an operator, `?`'s
//! `From::from`, `for`'s `IntoIterator`/`next`, and a foreign callee calling back into a
//! first-party impl (`sort` → `Ord::cmp`) all run a first-party impl of a *foreign* trait, so
//! [`Reach::implicit`] lists every admitted first-party impl of a foreign trait, by trait, for a
//! guard to review — `impl Drop for ComputationLane` among them.
//!
//! **A type on several arms of one module path** (one name declared in `cfg`-alternative files) is
//! asked on each arm and the answers are united; an arm whose type lacks the member adds a
//! [`Unresolved::Path`] edge beside the others' items. An `impl` block's self type is the one
//! declared in the block's own file when there is one; an `impl` block whose self type the module
//! tree does not hold (a macro-generated type) is listed with its methods
//! ([`Resolver::unplaced_impls`]), and its methods are reachable only as [`Unresolved::Ambiguous`]
//! candidates, never through a typed call; one chained past the hop limit refuses the reading.
//! The standard library's guards and wrappers (`MutexGuard`, `Ref`, `RefMut`, `Pin`, `LazyLock`,
//! …) deref to their pointee as `Box` and `Rc`/`Arc` do.
//!
//! **A `const`'s or `static`'s initializer is a body of the graph** ([`Node::Value`]): a use of
//! the value is a [`Target::Value`] edge to it — [`Via::Deref`] for a `LazyLock` or `LazyCell`,
//! whose initializer runs on its first deref, [`Via::Value`] otherwise — and the walk follows its
//! calls and the functions it names as values, a closure's included. A `const` or `static`
//! holding a function pointer, called (`F()`, `Self::F()`, `Type::F()`), is that edge and a
//! [`Unresolved::Dynamic`] whose candidates are the functions the initializer names; an
//! initializer that names its own value again is read once, its inner call's candidates the outer
//! reading's.
//!
//! **What is not read**, as limits: visibility; items a macro generates (a `bitflags!` type, a
//! generated enum, a `thread_local!` static: a path through one is [`Unresolved::Path`]); the
//! type of an associated `const`, and its initializer when reached through a generic (`T::ID`),
//! which is the trait's own declaration only; a foreign function's return type beyond the
//! census's fixed lists; `cfg!`'s arguments, which are predicates; a `#[cfg]` written on an
//! expression that is no statement or match arm (a field initialiser's, an element's) — its code
//! is walked as though the arms admitted it. One alias on several arms naming different types is
//! untyped, never one arm's.

mod body;
mod decls;
mod names;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::rc::Rc;

pub use decls::{Refusal, UnplacedImpl};

use crate::declarations::Compilation;
use crate::index::{FileRecord, Index, ItemRecord, Span};
use crate::traits::TraitId;

/// **An item of the packages handed in**: its package's position among them and its position in
/// that package's [`Index::items`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    package: u32,
    item: u32,
}

impl ItemId {
    pub(crate) fn new(package: usize, item: usize) -> Self {
        Self {
            package: u32::try_from(package).unwrap_or(u32::MAX),
            item: u32::try_from(item).unwrap_or(u32::MAX),
        }
    }

    /// The package's position among those handed to [`Resolver::new`].
    #[must_use]
    pub const fn package(self) -> usize {
        self.package as usize
    }

    /// The item's position in its package's [`Index::items`].
    #[must_use]
    pub const fn item(self) -> usize {
        self.item as usize
    }
}

/// **A `const` or a `static` of the packages handed in** ([`crate::ValueRecord`]): its package's
/// position and its position in that package's [`Index::values`]. Its initializer is a body of the
/// graph, walked when a use of the value reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId {
    package: u32,
    value: u32,
}

impl ValueId {
    pub(crate) fn new(package: usize, value: usize) -> Self {
        Self {
            package: u32::try_from(package).unwrap_or(u32::MAX),
            value: u32::try_from(value).unwrap_or(u32::MAX),
        }
    }

    #[must_use]
    pub const fn package(self) -> usize {
        self.package as usize
    }

    /// The value's position in its package's [`Index::values`].
    #[must_use]
    pub const fn value(self) -> usize {
        self.value as usize
    }
}

/// **A body of the graph**: a callable item's, or a `const`'s or `static`'s initializer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Node {
    Item(ItemId),
    Value(ValueId),
}

/// What an arm filter is asked about one declaration path of one thing: its package, its file, the
/// `cfg` predicates standing on it (on the declarations reaching the file, then inside it), and
/// what they say about a product build.
pub struct ArmQuery<'a> {
    pub package: &'a str,
    pub file: &'a FileRecord,
    pub predicates: &'a [String],
    pub compilation: Compilation,
}

/// **Which `cfg` arms count.** The default ([`Arms::product`]) is every arm a product build can
/// contain, as [`Compilation`] reads it (`test` out, every platform and feature in); a caller
/// narrows it ([`Arms::filtered`]) to a platform or a feature set.
pub struct Arms<'a> {
    admits: Box<dyn Fn(&ArmQuery<'_>) -> bool + 'a>,
}

impl<'a> Arms<'a> {
    #[must_use]
    pub fn product() -> Self {
        Self {
            admits: Box::new(|query| query.compilation.permits_product()),
        }
    }

    #[must_use]
    pub fn filtered(admits: impl Fn(&ArmQuery<'_>) -> bool + 'a) -> Self {
        Self {
            admits: Box::new(admits),
        }
    }

    pub(crate) fn admits(&self, query: &ArmQuery<'_>) -> bool {
        (self.admits)(query)
    }
}

/// Where one call is written.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CallSite {
    /// The name called, in the union of its package's index: a path's last segment, a method's
    /// name, a macro's path.
    pub span: Span,
    pub name: String,
    /// The innermost closure literal the site is written in, if any: code defined here that runs
    /// where the closure is handed.
    pub closure: Option<Span>,
    /// Every closure literal the site is written in, outermost first (the last is
    /// [`Self::closure`]), each with the call it is written as an argument of, if any: what a guard
    /// asks to tell a closure *handed* to a call (a worker body given to a spawn) from one the item
    /// runs itself.
    pub closures: Vec<Enclosing>,
    /// The innermost macro whose arguments the site is written in, if any.
    pub macro_arg: Option<String>,
}

/// **One closure literal a site is written in**, and the call it is handed to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Enclosing {
    pub closure: Span,
    /// The call the closure literal is written as an argument of — its [`CallSite::span`] and the
    /// argument's position — or `None` for a closure bound, returned, stored or called in place.
    pub handed: Option<Handed>,
}

/// **The call a closure literal is an argument of**: the call's site span (the name called, as
/// its own edges' [`CallSite::span`]) and the argument's position among the arguments written in
/// the parentheses, from 0. A method call's receiver is written before the dot and is not counted
/// (`pool.submit(1, f)`: `f` is 1); a path call counts every argument written, so in the same call
/// spelled `Pool::submit(&pool, 1, f)` the receiver is argument 0 and `f` is 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handed {
    pub call: Span,
    pub argument: usize,
}

/// How an edge's target is reached.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Via {
    Direct,
    /// A function named as a value.
    Value,
    /// A `Deref` step the language takes.
    Deref,
    /// Dynamic dispatch on a first-party trait.
    Dispatch(TraitId),
}

/// What an edge goes to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Item(ItemId),
    /// A `const`'s or `static`'s initializer, reached by a use of the value: [`Via::Value`], or
    /// [`Via::Deref`] for a `LazyLock`/`LazyCell`, whose initializer runs on its first deref.
    Value(ValueId),
    /// A tuple struct's or tuple variant's constructor: no body.
    Constructor,
    /// An inline closure, whose body is the caller's.
    Closure(Span),
    /// A `fn` written inside the caller's body, walked as the caller's.
    Nested(Span),
    /// A callee outside the packages, by its full path: a boundary, not a failure.
    Foreign(Vec<String>),
    Unresolved(Unresolved),
}

/// **An edge the rules cannot follow**, by kind (the module's table).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unresolved {
    Dynamic {
        what: String,
        candidates: Vec<ItemId>,
    },
    Ambiguous {
        method: String,
        candidates: Vec<ItemId>,
    },
    Macro {
        name: String,
    },
    OtherArm {
        candidates: Vec<ItemId>,
    },
    Generated {
        by: String,
    },
    Path {
        written: String,
        why: String,
    },
    Cutoff {
        what: String,
    },
    Unparsed {
        why: String,
    },
}

impl Unresolved {
    /// **The one item an untyped receiver's call can reach**, when exactly one first-party method
    /// has its name: an [`Unresolved::Ambiguous`] edge with one candidate (a receiver the rules
    /// do not type: a `?` on a type they refuse, a field of an untyped value). The edge is still not
    /// followed: `docs/plans/UNFOLLOWED-EDGES.tsv` lists only edges with other than one candidate
    /// and counts these in its header, and a guard that names a callee's callers counts such an
    /// edge as a call of its one candidate (T-CENSUS-FOLLOWS-ASSOCIATED-TYPE-CALLS, the O-4b
    /// review's F5: a rename to a name only one item has moves an edge off the list, and the
    /// header count is where that shows). `None` for every other edge.
    #[must_use]
    pub fn sole_candidate(&self) -> Option<ItemId> {
        match self {
            Self::Ambiguous { candidates, .. } if candidates.len() == 1 => Some(candidates[0]),
            _ => None,
        }
    }
}

/// One call edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub from: Node,
    pub site: CallSite,
    pub to: Target,
    pub via: Via,
}

/// **The resolver over one program's packages.**
pub struct Resolver<'i> {
    tables: decls::Tables<'i>,
}

impl<'i> Resolver<'i> {
    /// The resolver over `packages`, each `(package name, its index)`, under `arms`.
    ///
    /// # Errors
    ///
    /// A [`Refusal`] for every `impl` block whose trait path chains past the hop limit: filed
    /// under no trait, its methods would be missing from every dispatch on the trait.
    pub fn new(packages: &[(&'i str, &'i Index)], arms: Arms<'i>) -> Result<Self, Vec<Refusal>> {
        decls::Tables::read(packages, arms).map(|tables| Self { tables })
    }

    /// The packages handed in, in order: an [`ItemId::package`] is a position here.
    #[must_use]
    pub fn packages(&self) -> Vec<&'i str> {
        self.tables
            .names
            .packages
            .iter()
            .map(|(name, ..)| *name)
            .collect()
    }

    /// The item `record` is, in `package`.
    #[must_use]
    pub fn id_of(&self, package: &str, record: &ItemRecord) -> Option<ItemId> {
        let at = self.tables.names.package_at(package)?;
        self.tables
            .index(at)
            .items()
            .iter()
            .position(|item| std::ptr::eq(item, record))
            .map(|item| ItemId::new(at, item))
    }

    /// Every callable item of `package`, admitted or not.
    #[must_use]
    pub fn callables(&self, package: &str) -> Vec<ItemId> {
        let Some(at) = self.tables.names.package_at(package) else {
            return Vec::new();
        };
        self.tables
            .index(at)
            .items()
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind().is_callable())
            .map(|(item, _)| ItemId::new(at, item))
            .collect()
    }

    #[must_use]
    pub fn record(&self, id: ItemId) -> &'i ItemRecord {
        self.tables.record(id)
    }

    #[must_use]
    pub fn package(&self, id: ItemId) -> &'i str {
        self.tables.package(id.package())
    }

    #[must_use]
    pub fn index_of(&self, id: ItemId) -> &'i Index {
        self.tables.index(id.package())
    }

    /// **The type paths the item's written arguments name, every alias followed** (R7): a
    /// foreign type by its path (`std::sync::Mutex`), a first-party type by
    /// `package module::Name`, a type alias by what it names, its arguments substituted. A type
    /// the census does not type (a qualified associated type, `impl Trait`, a raw pointer) names
    /// no path: a caller reads it as written. `Err`: the item has no signature the resolver read,
    /// or each type path it could not place, why.
    ///
    /// # Errors
    ///
    /// The item's signature was not read, or a type path in its arguments names nothing placed.
    pub fn argument_type_paths(&self, id: ItemId) -> Result<BTreeSet<String>, Vec<String>> {
        let Some(sig) = self.tables.sigs.get(&id) else {
            return Err(vec!["no signature was read".to_owned()]);
        };
        let mut paths = BTreeSet::new();
        let mut unread = Vec::new();
        for param in &sig.params {
            self.tables.type_paths(param, 0, &mut paths, &mut unread);
        }
        if unread.is_empty() {
            Ok(paths)
        } else {
            Err(unread)
        }
    }

    /// Whether the arms admit the item.
    #[must_use]
    pub fn admitted(&self, id: ItemId) -> bool {
        self.tables.is_admitted(id)
    }

    /// The item named the way a guard's message names it: `package module::Owner::name`, the
    /// trait in `<Owner as Trait>` form for a trait impl's method.
    #[must_use]
    pub fn key(&self, id: ItemId) -> String {
        let record = self.record(id);
        let module = record.module_paths().first().copied().unwrap_or("crate");
        let label = match (record.type_owner(), record.trait_name()) {
            (Some(owner), Some(trait_name)) => {
                format!("<{owner} as {trait_name}>::{}", record.name())
            }
            (Some(owner), None) => format!("{owner}::{}", record.name()),
            (None, Some(trait_name)) => format!("{trait_name}::{}", record.name()),
            (None, None) => record.name().to_owned(),
        };
        format!("{} {module}::{label}", self.package(id))
    }

    /// A node named the way [`Self::key`] names an item; a value is `package module::NAME`.
    #[must_use]
    pub fn node_key(&self, node: Node) -> String {
        match node {
            Node::Item(id) => self.key(id),
            Node::Value(id) => {
                let record = self.value_record(id);
                let module = record.module_paths().first().copied().unwrap_or("crate");
                let owner = record
                    .type_owner()
                    .or(record.trait_name())
                    .map(|owner| format!("{owner}::"))
                    .unwrap_or_default();
                format!(
                    "{} {module}::{owner}{}",
                    self.tables.package(id.package()),
                    record.name()
                )
            }
        }
    }

    /// The index a node's body is written in.
    #[must_use]
    pub fn node_index(&self, node: Node) -> &'i Index {
        match node {
            Node::Item(id) => self.tables.index(id.package()),
            Node::Value(id) => self.tables.index(id.package()),
        }
    }

    #[must_use]
    pub fn value_record(&self, id: ValueId) -> &'i crate::ValueRecord {
        &self.tables.index(id.package()).values()[id.value()]
    }

    /// **Every edge of one `const`'s or `static`'s initializer**: its calls, and the functions it
    /// names as values (the closure of a `LazyLock::new(|| …)` included, its edges carrying the
    /// closure).
    #[must_use]
    pub fn value_callees(&self, value: ValueId) -> Vec<Edge> {
        body::Walker::value(&self.tables, value)
    }

    /// The edges of a node's body: [`Self::resolve_callees`] or [`Self::value_callees`].
    #[must_use]
    pub fn node_callees(&self, node: Node) -> Vec<Edge> {
        match node {
            Node::Item(id) => self.resolve_callees(id),
            Node::Value(id) => self.value_callees(id),
        }
    }

    /// **Every call edge of one item's body**, in source order. An item with no body has none; a
    /// body `syn` does not parse is one [`Unresolved::Unparsed`] edge.
    #[must_use]
    pub fn resolve_callees(&self, item: ItemId) -> Vec<Edge> {
        if !self.record(item).kind().is_callable() {
            return Vec::new();
        }
        body::Walker::item(&self.tables, item)
    }

    /// Every admitted first-party impl of a foreign trait, by the trait's path: what implicit calls
    /// and foreign callbacks can run ([`Reach::implicit`]).
    #[must_use]
    pub fn implicit(&self) -> BTreeMap<String, BTreeSet<ItemId>> {
        let mut found: BTreeMap<String, BTreeSet<ItemId>> = BTreeMap::new();
        for (path, blocks) in &self.tables.foreign_trait_impls {
            for at in blocks {
                let block = &self.tables.impls[*at];
                if !block.admitted {
                    continue;
                }
                found.entry(path.join("::")).or_default().extend(
                    block
                        .methods
                        .iter()
                        .copied()
                        .filter(|id| self.tables.is_admitted(*id)),
                );
            }
        }
        found
    }

    /// The `impl` blocks whose self type the module tree does not hold — a type a macro generates
    /// — each with its methods: listed, never filed under nothing.
    #[must_use]
    pub fn unplaced_impls(&self) -> &[UnplacedImpl] {
        &self.tables.unplaced_impls
    }

    /// Declarations the reading could not parse (a type, an alias, a signature, an `impl`
    /// header): an answer that stands without them.
    #[must_use]
    pub fn unparsed(&self) -> &[String] {
        &self.tables.unparsed
    }
}

/// **What a walk reached**: the items it expanded, the boundary items it stopped at, and every
/// foreign and unresolved edge it met, each with the path (root … caller) that first reached its
/// caller.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    pub reached: BTreeSet<ItemId>,
    /// The `const` and `static` initializers the walk reached.
    pub values: BTreeSet<ValueId>,
    pub stopped_at: BTreeSet<ItemId>,
    pub foreign: Vec<(Edge, Vec<Node>)>,
    pub unresolved: Vec<(Edge, Vec<Node>)>,
    /// Every first-party impl only an implicit call or a foreign callback runs, by trait
    /// ([`Resolver::implicit`]): not narrowed by the walk, for the guard to review.
    pub implicit: BTreeMap<String, BTreeSet<ItemId>>,
}

/// **The call graph**: a resolver's edges, computed once per item and kept.
pub struct CallGraph<'r, 'i> {
    resolver: &'r Resolver<'i>,
    edges: RefCell<HashMap<Node, Rc<Vec<Edge>>>>,
}

impl<'r, 'i> CallGraph<'r, 'i> {
    #[must_use]
    pub fn new(resolver: &'r Resolver<'i>) -> Self {
        Self {
            resolver,
            edges: RefCell::default(),
        }
    }

    #[must_use]
    pub const fn resolver(&self) -> &'r Resolver<'i> {
        self.resolver
    }

    /// One node's edges ([`Resolver::node_callees`]), kept.
    #[must_use]
    pub fn edges(&self, node: Node) -> Rc<Vec<Edge>> {
        if let Some(known) = self.edges.borrow().get(&node) {
            return Rc::clone(known);
        }
        let found = Rc::new(self.resolver.node_callees(node));
        self.edges.borrow_mut().insert(node, Rc::clone(&found));
        found
    }

    /// **The walk**: from `roots`, along every [`Target::Item`] and [`Target::Value`] edge, each
    /// body once, stopping at the items of `boundary` (recorded, not expanded).
    #[must_use]
    pub fn walk(&self, roots: &[ItemId], boundary: &BTreeSet<ItemId>) -> Reach {
        self.walk_following(roots, boundary, &|_| Vec::new())
    }

    /// [`Self::walk`], also following what `follow` answers for a foreign or an unresolved edge —
    /// a caller's stated decision about it (an ambiguous call with one candidate, say), which stays
    /// in [`Reach::foreign`] or [`Reach::unresolved`] all the same.
    #[must_use]
    pub fn walk_following(
        &self,
        roots: &[ItemId],
        boundary: &BTreeSet<ItemId>,
        follow: &dyn Fn(&Edge) -> Vec<ItemId>,
    ) -> Reach {
        let mut reach = Reach {
            implicit: self.resolver.implicit(),
            ..Reach::default()
        };
        let mut parent: HashMap<Node, Option<Node>> = HashMap::new();
        let mut queue: VecDeque<Node> = VecDeque::new();
        for root in roots {
            if parent.insert(Node::Item(*root), None).is_none() {
                queue.push_back(Node::Item(*root));
            }
        }
        let path_to = |parent: &HashMap<Node, Option<Node>>, node: Node| -> Vec<Node> {
            let mut path = vec![node];
            let mut at = node;
            while let Some(Some(up)) = parent.get(&at) {
                path.push(*up);
                at = *up;
            }
            path.reverse();
            path
        };
        while let Some(node) = queue.pop_front() {
            match node {
                Node::Item(item) if boundary.contains(&item) => {
                    reach.stopped_at.insert(item);
                    continue;
                }
                Node::Item(item) => {
                    reach.reached.insert(item);
                }
                Node::Value(value) => {
                    reach.values.insert(value);
                }
            }
            let edges = self.edges(node);
            for edge in edges.iter() {
                let mut next = Vec::new();
                match &edge.to {
                    Target::Item(id) => next.push(Node::Item(*id)),
                    Target::Value(id) => next.push(Node::Value(*id)),
                    Target::Foreign(_) => {
                        reach.foreign.push((edge.clone(), path_to(&parent, node)));
                        next.extend(follow(edge).into_iter().map(Node::Item));
                    }
                    Target::Unresolved(_) => {
                        reach
                            .unresolved
                            .push((edge.clone(), path_to(&parent, node)));
                        next.extend(follow(edge).into_iter().map(Node::Item));
                    }
                    Target::Constructor | Target::Closure(_) | Target::Nested(_) => {}
                }
                for id in next {
                    if let std::collections::hash_map::Entry::Vacant(slot) = parent.entry(id) {
                        slot.insert(Some(node));
                        queue.push_back(id);
                    }
                }
            }
        }
        reach
    }
}
