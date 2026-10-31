//! **The lane-drain guard** (T-LANE-DRAIN-MARK-GUARD, row 110 of the 0.4.9 plan). A guard: its
//! subject is how the drains of `bt-app`'s lanes are written, and it reads the product's source
//! through bt-source's index and `syn`.
//!
//! A lane that promises `Delivery::Submission` answers in admission order because its drain reads
//! the ledger's mark **once**, before any asker is read, and every asker takes only below it
//! (`lane::Ledger::drain_mark`; `lane::Request::take(&Mark)` refuses an id at or past the mark).
//! The type does half of that: a `take` needs a `Mark`, a `Mark` is minted by its ledger alone (its
//! fields are private to `lane`) and is neither `Clone` nor `Copy`. What the type cannot say is
//! *where* the mark is read and *how often*: a second read for a later asker, or a read inside a
//! closure run per asker, compiles and breaks the promise. That is this guard.
//!
//! # The registry
//!
//! [`LANE_DRAINS`] names one row per drain: the lane contracts it drains (`lane`'s `Contract`
//! consts), their delivery, the drain, the per-target takes it reaches, the contract adapters'
//! drains, and whether it merges several askers. `lane::Coalescing::ended` is one shared row for
//! the `NewestAdopted` lanes (font, taskbar, programs): outside the `Submission` promise, under the
//! same discipline. [`EXCEPTIONS`] names the three doors that take otherwise, each with its one
//! caller; [`TREE`] holds the mark's minting fns, its forwarders and the merge.
//!
//! # The checks
//!
//! * **D1** — every `Contract` const of `lane` (the contract suite's table, `#[cfg(test)]` in the
//!   tree, read on every arm) is in exactly one row, with the delivery its literal writes
//!   (`delivery: Delivery::…`), and every row's contracts exist.
//! * **D2** — `drain_mark` is called exactly once in each registered drain, as the initializer of
//!   a `let` that binds a plain name, written directly in the drain's own block: never in a loop,
//!   a closure, an async block, a nested `fn` or a macro's arguments (the standard library's
//!   expression macros, `vec!`, `format!`, the assertions, are read as the expressions they are,
//!   [`EXPRESSION_MACROS`]; any other macro's are a shape). A forwarder's body is that
//!   one call and nothing else. The name is called nowhere else in the product, and no other fn is
//!   named `drain_mark`. Each door of [`EXCEPTIONS`] is called by its one caller (once) and its
//!   forwarders, and by nothing else.
//! * **D3** — every path the rules name resolves to exactly one fn.
//! * **D4** — each contract adapter's drain reaches one of its row's takes over the call graph
//!   (`bt_source::call_graph`, every `cfg` arm, an untyped call with one candidate followed, and
//!   the reviewed edges of [`Rules::follows`]).
//! * **D5** — a `Mark` is never kept: its name is written only as the referent of a whole `&`
//!   parameter type (a fn's, a closure's), as the whole type of a `let`, as the return type of a
//!   minting fn or a forwarder, as a type alias's whole right side (`persist::StorageMark`; the
//!   alias is then read as the name), and in `lane`'s own declaration of it. A field, a `static`,
//!   a `const`, a collection's or any other generic argument, another fn's return type, an
//!   expression path: each is a finding. A `Mark` is built (`Mark { … }`) only in a minting fn,
//!   and the binding a registered drain reads its mark into is used only borrowed — `&mark`, or as
//!   a method call's receiver — never moved.
//! * **D6** — a drain of several askers calls the lane's `in_admission_order`, or a registered
//!   wrapper that does (`handoff_lane::in_press_order`), directly in its body. The computation
//!   lanes, the git lane and the `NewestAdopted` row are exempt by row: one share, or adoption.
//!
//! # What it does not read (its grammar's edges)
//!
//! Every product identifier token named `drain_mark`, a door's name, or the mark — by scope: a
//! path whose segment before it is the mark's module (`lane::Mark`), or a bare name where the
//! mark's module, an import or a glob makes it the mark; `uninstall`'s and the update trial's own
//! `Mark` types are not it — must be one the reading above classified; any other — in a macro's arguments, an `impl` header's
//! generics, an associated type, a path call (`Ledger::drain_mark(&l)`), a fn named as a value, a
//! renamed import — is a `shape` diagnostic with its file and line, never silence.
//!
//! **Blind spots, stated.** (1) *Partial target coverage*: the guard proves the mark is read once
//! and that the adapter reaches a take; it does not prove the drain asks **every** target (a
//! store left out of `App::storage_answers` keeps its answers, out of order with nothing). The
//! contract suite's per-lane rows answer that, not this. (2) *The Id-type ledger confusion*: a
//! mark is typed by its lane's id (`Mark<StorageId>`), so a take under another lane's mark does
//! not compile — but two ledgers of one id type (two lanes sharing `ComputationId`) type-check
//! each other's marks; only `lane::Request`'s `debug_assert` (the mark's ledger identity) catches
//! that, in a debug build that runs the take. (3) Calls are matched by method name, not by
//! receiver type: a method of another type named `drain_mark` is read as a mark read (a false
//! finding, never a missed one). (4) A by-value `self` method on `Mark` would move the drain's
//! binding through a receiver; `Mark` has none, and `lane` is where one would be written.
//!
//! `cargo test -p bt-source` runs this, as every bt-source guard; `docs/ARCHITECTURE-GUARDS.md`
//! (§ Lane drains read their mark once) states it for a reader of the tree.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use bt_source::call_graph::{Arms, CallGraph, Edge, ItemId, Node, Resolver, Target, Unresolved};
use bt_source::{
    Certainty, DiskScope, Index, ItemRecord, Occurrence, Span, TargetId, TargetKind, TargetRoot,
    Universe, ValueKind, Vendor, View, report,
};
use syn::visit::{self, Visit};

// ── the rules ─────────────────────────────────────────────────────────────

/// A contract's delivery, as `lane::Delivery` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Submission,
    NewestAdopted,
}

/// Whether a drain merges several askers' answers.
#[derive(Clone, Copy, Debug)]
enum Targets {
    /// Several askers (windows, stores): the drain merges them (D6).
    Many,
    /// Exempt from D6, for the reason given.
    One(&'static str),
}

/// **One drain of the registry.**
struct Drain {
    /// The `Contract` consts it drains.
    contracts: &'static [&'static str],
    delivery: Delivery,
    /// The drain: `module::Owner::name`.
    drain: &'static str,
    /// The per-target takes the drain reaches, one of which each adapter must reach (D4).
    takes: &'static [&'static str],
    /// The contract adapters' drains.
    adapters: &'static [&'static str],
    targets: Targets,
}

/// **A door that takes otherwise than under a drain's mark**, its definition, the forwarders that
/// carry it out of `lane`, and its one caller.
struct Door {
    name: &'static str,
    defined: &'static str,
    forwarders: &'static [&'static str],
    caller: &'static str,
}

/// **What the guard holds a tree to.**
struct Rules {
    /// The contract type's name.
    contract: &'static str,
    /// The mark type's name and the module that declares it.
    mark: &'static str,
    mark_module: &'static str,
    /// The mark's read.
    read: &'static str,
    /// The fns that build a mark and return it.
    minted_by: &'static [&'static str],
    /// The fns whose body is the read, forwarded.
    forwarders: &'static [&'static str],
    /// The merge, and the wrappers that call it.
    merge: &'static str,
    wrappers: &'static [&'static str],
    drains: &'static [Drain],
    doors: &'static [Door],
    /// **The call graph's reviewed edges** (D4): at a call the resolver leaves unresolved in the
    /// first fn, among whose candidates is the second, the walk follows the second.
    follows: &'static [(&'static str, &'static str)],
}

/// **The registry**: every drain of `bt-app`'s lanes.
const LANE_DRAINS: &[Drain] = &[
    Drain {
        contracts: &["HANDOFF"],
        delivery: Delivery::Submission,
        drain: "crate::FolioApp::answer_handoffs",
        takes: &["crate::handoff_lane::Pending::answered"],
        adapters: &["crate::handoff_lane::contract_adapter::HandoffAdapter::drain"],
        targets: Targets::Many,
    },
    Drain {
        contracts: &["COMPUTATION", "COMPUTATION_SCALE", "COMPUTATION_PATH"],
        delivery: Delivery::Submission,
        drain: "crate::computation_lane::ShareOf::ended",
        takes: &["crate::computation_lane::ShareOf::ended"],
        adapters: &["crate::computation_lane::contract_adapter::Adapter::drain"],
        targets: Targets::One("one share per lane: the share's requests are its one asker"),
    },
    Drain {
        contracts: &["GIT_MUTATION"],
        delivery: Delivery::Submission,
        drain: "crate::git_mutation_lane::Share::ended",
        takes: &["crate::git_mutation_lane::Share::ended"],
        adapters: &["crate::git_mutation_lane::contract_adapter::Adapter::drain"],
        targets: Targets::One("one share: the lane's requests are its one asker"),
    },
    Drain {
        contracts: &["STORAGE"],
        delivery: Delivery::Submission,
        drain: "crate::runtime::storage::App::storage_answers",
        takes: &[
            "crate::persist::OnTheWorker::hear_below",
            "crate::document_request::DocumentState::answered",
            "crate::preview::PreviewSaves::answered",
        ],
        adapters: &["crate::persist::contract_adapter::Adapter::drain"],
        targets: Targets::Many,
    },
    Drain {
        contracts: &["CLIPBOARD"],
        delivery: Delivery::Submission,
        drain: "crate::runtime::clipboard::FolioApp::clipboard_answers",
        takes: &["crate::clipboard_lane::ClipboardAsks::answered"],
        adapters: &["crate::clipboard_lane::contract_adapter::ClipboardAdapter::drain"],
        targets: Targets::Many,
    },
    Drain {
        contracts: &["FONT", "TASKBAR", "PROGRAMS"],
        delivery: Delivery::NewestAdopted,
        drain: "crate::lane::Coalescing::ended",
        takes: &["crate::lane::Coalescing::ended"],
        adapters: &[
            "crate::settings::font_lane_adapter::FontAdapter::drain",
            "crate::taskbar_lane::contract_adapter::TaskbarAdapter::drain",
            "crate::programs_lane::contract_adapter::ProgramsAdapter::drain",
        ],
        targets: Targets::One("NewestAdopted: adoption orders the answers, not admission"),
    },
];

/// **The exceptions**: the doors that take otherwise, each with its one caller.
const EXCEPTIONS: &[Door] = &[
    Door {
        name: "mark_after_the_worker_ended",
        defined: "crate::lane::Ledger::mark_after_the_worker_ended",
        forwarders: &["crate::persist::StorageLane::mark_after_the_worker_ended"],
        caller: "crate::preview::PreviewSaves::answered_at_the_stop",
    },
    Door {
        name: "supersede_and_take",
        defined: "crate::lane::Queue::supersede_and_take",
        forwarders: &[],
        caller: "crate::clipboard_lane::ClipboardLane::supersede",
    },
    Door {
        name: "take_superseded_past",
        defined: "crate::lane::Request::take_superseded_past",
        forwarders: &[],
        caller: "crate::computation_lane::ShareOf::ended",
    },
];

/// The tree's rules.
const TREE: Rules = Rules {
    contract: "Contract",
    mark: "Mark",
    mark_module: "crate::lane",
    read: "drain_mark",
    minted_by: &[
        "crate::lane::Ledger::drain_mark",
        "crate::lane::Ledger::mark_after_the_worker_ended",
    ],
    forwarders: &[
        "crate::handoff_lane::HandoffLane::drain_mark",
        "crate::clipboard_lane::ClipboardLane::drain_mark",
        "crate::persist::StorageLane::drain_mark",
    ],
    merge: "crate::lane::in_admission_order",
    wrappers: &["crate::handoff_lane::in_press_order"],
    drains: LANE_DRAINS,
    doors: EXCEPTIONS,
    // `GitMutationLane::ended` drains each share through a guard the resolver does not type
    // (`for share in &mut shares.live`, `shares` a `MutexGuard` from the generic `lane::locked`).
    follows: &[(
        "crate::git_mutation_lane::GitMutationLane::ended",
        "crate::git_mutation_lane::Share::ended",
    )],
};

impl Rules {
    /// Every fn path the rules name.
    fn paths(&self) -> Vec<&'static str> {
        let mut paths: Vec<&'static str> = Vec::new();
        paths.extend(self.minted_by);
        paths.extend(self.forwarders);
        paths.push(self.merge);
        paths.extend(self.wrappers);
        for drain in self.drains {
            paths.push(drain.drain);
            paths.extend(drain.takes);
            paths.extend(drain.adapters);
        }
        for door in self.doors {
            paths.push(door.defined);
            paths.extend(door.forwarders);
            paths.push(door.caller);
        }
        for (caller, callee) in self.follows {
            paths.push(caller);
            paths.push(callee);
        }
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// The fns that may return a mark: the minting fns and every forwarder.
    fn may_return(&self, keys: &[String]) -> bool {
        let named = |list: &[&str]| keys.iter().any(|key| list.contains(&key.as_str()));
        named(self.minted_by)
            || named(self.forwarders)
            || self.doors.iter().any(|door| named(door.forwarders))
    }
}

// ── findings ──────────────────────────────────────────────────────────────

/// **One finding**: the check, where, and what.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    check: &'static str,
    at: String,
    what: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.check, self.at, self.what)
    }
}

/// **The guard's answer**: its findings, and what it read.
#[derive(Default)]
struct Judged {
    findings: Vec<Finding>,
    fns_read: usize,
    tokens_read: usize,
    contracts_read: usize,
}

// ── the reading ───────────────────────────────────────────────────────────

/// How one read of the mark is written.
#[derive(Clone, Debug)]
enum ReadShape {
    /// `let <name> = ….drain_mark();` directly in the fn's block.
    Statement,
    /// The fn's whole body is the call.
    Forward,
    /// Anywhere else, and why.
    Stray(&'static str),
}

/// What the reading of one item found, each at its union offset.
#[derive(Clone, Debug)]
enum Fact {
    /// A method call of the mark's read.
    Read { at: usize, shape: ReadShape },
    /// A call of a door, by name.
    DoorCall {
        name: String,
        at: usize,
        forward: bool,
    },
    /// A fn declared under a watched name.
    Def { name: String, at: usize },
    /// The mark's name written as a type: allowed in `place`, or kept there.
    MarkType {
        at: usize,
        kept: bool,
        place: &'static str,
    },
    /// `Mark { … }`.
    Minting { at: usize },
    /// The drain's binding used other than borrowed.
    Moved { at: usize, binding: String },
    /// A shape the grammar does not read.
    Shape { at: usize, what: String },
    /// A token accounted for without a finding (an import, the type's own declaration).
    Known { at: usize },
}

impl Fact {
    const fn at(&self) -> usize {
        match self {
            Self::Read { at, .. }
            | Self::DoorCall { at, .. }
            | Self::Def { at, .. }
            | Self::MarkType { at, .. }
            | Self::Minting { at }
            | Self::Moved { at, .. }
            | Self::Shape { at, .. }
            | Self::Known { at } => *at,
        }
    }
}

/// The names the reading watches.
struct Watched {
    read: String,
    doors: BTreeSet<String>,
    /// The mark's name and every alias of it, each with the module that declares it.
    marks: BTreeMap<String, String>,
    /// `(module, name)`: where a bare name is the mark — its own module, a module-level import of
    /// it, a glob of its module.
    bare: BTreeSet<(String, String)>,
    /// Every watched name.
    every: BTreeSet<String>,
}

impl Watched {
    fn callable(&self, name: &str) -> bool {
        name == self.read || self.doors.contains(name)
    }

    /// **Whether segment `at` of a path written in `module` names the mark**: a qualified
    /// segment when the one before it is its module's own name (`lane::Mark`,
    /// `crate::persist::StorageMark`), a bare one where `bare` (or the body's own import,
    /// `local`) says so. Another type that happens to be called `Mark` (`uninstall`'s, the
    /// update trial's) is not the mark.
    fn names_mark(
        &self,
        segments: &[String],
        at: usize,
        module: &str,
        local: &BTreeSet<String>,
    ) -> bool {
        let Some(declared) = self.marks.get(&segments[at]) else {
            return false;
        };
        if at == 0 {
            self.bare
                .contains(&(module.to_owned(), segments[0].clone()))
                || local.contains(&segments[0])
        } else {
            last_segment(declared) == segments[at - 1]
        }
    }
}

/// The last segment of a module path.
fn last_segment(module: &str) -> &str {
    module.rsplit("::").next().unwrap_or(module)
}

/// A `use` path's prefix (every segment but the name), resolved from `module`: `crate::…` as
/// written, `self` and `super` from `module`, anything else under `module`.
fn resolved(module: &str, prefix: &[String]) -> String {
    let mut at: Vec<&str> = module.split("::").collect();
    let mut rest = prefix;
    match prefix.first().map(String::as_str) {
        Some("crate") => {
            at = vec!["crate"];
            rest = &prefix[1..];
        }
        Some("self") => rest = &prefix[1..],
        _ => {}
    }
    for segment in rest {
        if segment == "super" {
            at.pop();
        } else {
            at.push(segment);
        }
    }
    at.join("::")
}

/// **The names the guard watches over `index`**: the read, the doors, the mark and its aliases
/// (every product `type X = <the mark>;`, followed to a fixed point) and where each is bare.
fn watched(index: &Index, rules: &Rules) -> Watched {
    let doors: BTreeSet<String> = rules
        .doors
        .iter()
        .map(|door| door.name.to_owned())
        .collect();
    let mut watched = Watched {
        read: rules.read.to_owned(),
        doors,
        marks: BTreeMap::from([(rules.mark.to_owned(), rules.mark_module.to_owned())]),
        bare: BTreeSet::new(),
        every: BTreeSet::new(),
    };
    loop {
        let before = (watched.marks.len(), watched.bare.len());
        for (name, module) in &watched.marks.clone() {
            watched.bare.insert((module.clone(), name.clone()));
        }
        for import in index.uses() {
            if !in_product(index, import.whole()) {
                continue;
            }
            for module in import.module_paths() {
                for (bound, path) in import.names() {
                    if let Some((name, prefix)) = path.split_last()
                        && let Some(declared) = watched.marks.get(name)
                        && resolved(module, prefix) == *declared
                    {
                        watched.bare.insert((module.to_owned(), bound.clone()));
                    }
                }
                for glob in import.globs() {
                    let globbed = resolved(module, glob);
                    for (name, declared) in &watched.marks.clone() {
                        if globbed == *declared {
                            watched.bare.insert((module.to_owned(), name.clone()));
                        }
                    }
                }
            }
        }
        for alias in index.aliases() {
            if !in_product(index, alias.whole()) {
                continue;
            }
            let Ok(syn::Item::Type(item)) = syn::parse_str::<syn::Item>(index.text(alias.whole()))
            else {
                continue;
            };
            let syn::Type::Path(path) = &*item.ty else {
                continue;
            };
            let segments = segments(&path.path);
            for module in alias.module_paths() {
                if watched.names_mark(&segments, segments.len() - 1, module, &BTreeSet::new()) {
                    watched
                        .marks
                        .insert(alias.name().to_owned(), module.to_owned());
                }
            }
        }
        if (watched.marks.len(), watched.bare.len()) == before {
            break;
        }
    }
    watched.every = watched.marks.keys().cloned().collect();
    watched.every.insert(watched.read.clone());
    watched.every.extend(watched.doors.iter().cloned());
    watched
}

/// A path's segments, as written.
fn segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

/// **The reader of one item's text**, parsed by `syn` at union offset `base`, written in
/// `module`.
struct Reader<'w> {
    base: usize,
    watched: &'w Watched,
    module: String,
    /// The mark's names a `use` inside the body brings in bare.
    local: BTreeSet<String>,
    facts: Vec<Fact>,
    closures: usize,
    loops: usize,
    asyncs: usize,
    nested: usize,
    /// The drain's binding of its mark, once read.
    binding: Option<String>,
    place: &'static str,
}

impl<'w> Reader<'w> {
    fn new(base: usize, watched: &'w Watched, module: &str) -> Self {
        Self {
            base,
            watched,
            module: module.to_owned(),
            local: BTreeSet::new(),
            facts: Vec::new(),
            closures: 0,
            loops: 0,
            asyncs: 0,
            nested: 0,
            binding: None,
            place: "a type",
        }
    }

    fn at(&self, span: proc_macro2::Span) -> usize {
        self.base + span.byte_range().start
    }

    fn why(&self) -> &'static str {
        if self.nested > 0 {
            "inside a nested fn"
        } else if self.asyncs > 0 {
            "inside an async block"
        } else if self.closures > 0 {
            "inside a closure"
        } else if self.loops > 0 {
            "inside a loop"
        } else {
            "not the initializer of a `let` written directly in the fn's block"
        }
    }

    /// Whether segment `at` of `path` names the mark.
    fn names_mark(&self, path: &syn::Path, at: usize) -> bool {
        self.watched
            .names_mark(&segments(path), at, &self.module, &self.local)
    }

    /// `ty`, if it is a path whose last segment names the mark.
    fn mark_path<'t>(&self, ty: &'t syn::Type) -> Option<&'t syn::TypePath> {
        match ty {
            syn::Type::Path(path)
                if path.qself.is_none()
                    && !path.path.segments.is_empty()
                    && self.names_mark(&path.path, path.path.segments.len() - 1) =>
            {
                Some(path)
            }
            syn::Type::Paren(inner) => self.mark_path(&inner.elem),
            syn::Type::Group(inner) => self.mark_path(&inner.elem),
            _ => None,
        }
    }

    /// The mark path `ty` borrows whole (`&Mark<…>`), if it does.
    fn borrowed_mark<'t>(&self, ty: &'t syn::Type) -> Option<&'t syn::TypePath> {
        match ty {
            syn::Type::Reference(reference) if reference.mutability.is_none() => {
                self.mark_path(&reference.elem)
            }
            _ => None,
        }
    }

    /// The mark's name written where it is allowed: the name accounted, its arguments read.
    fn allowed(&mut self, path: &syn::TypePath, place: &'static str) {
        for (at, segment) in path.path.segments.iter().enumerate() {
            if self.names_mark(&path.path, at) {
                let at = self.at(segment.ident.span());
                self.facts.push(Fact::MarkType {
                    at,
                    kept: false,
                    place,
                });
            }
            self.visit_path_arguments(&segment.arguments);
        }
    }

    /// A type in `place`, with `allow` naming the forms allowed whole there.
    fn read_type(&mut self, ty: &syn::Type, place: &'static str, allow: Allow) {
        if allow.borrowed
            && let Some(path) = self.borrowed_mark(ty)
        {
            self.allowed(path, place);
            return;
        }
        if allow.whole
            && let Some(path) = self.mark_path(ty)
        {
            self.allowed(path, place);
            return;
        }
        let saved = std::mem::replace(&mut self.place, place);
        self.visit_type(ty);
        self.place = saved;
    }

    /// A pattern that may carry a type (`x: &Mark`).
    fn read_pattern(&mut self, pattern: &syn::Pat, place: &'static str, allow: Allow) {
        if let syn::Pat::Type(typed) = pattern {
            self.visit_pat(&typed.pat);
            self.read_type(&typed.ty, place, allow);
        } else {
            self.visit_pat(pattern);
        }
    }

    /// **A fn's signature and body**: `may_return` when the fn mints or forwards a mark.
    fn read_fn(
        &mut self,
        signature: &syn::Signature,
        block: Option<&syn::Block>,
        may_return: bool,
    ) {
        let name = signature.ident.to_string();
        if self.watched.callable(&name) {
            let at = self.at(signature.ident.span());
            self.facts.push(Fact::Def {
                name: name.clone(),
                at,
            });
        }
        self.visit_generics(&signature.generics);
        for input in &signature.inputs {
            if let syn::FnArg::Typed(typed) = input {
                self.visit_pat(&typed.pat);
                self.read_type(&typed.ty, "a parameter", Allow::BORROWED);
            }
        }
        if let syn::ReturnType::Type(_, ty) = &signature.output {
            let allow = if may_return {
                Allow::WHOLE
            } else {
                Allow::NONE
            };
            self.read_type(ty, "a return type", allow);
        }
        let Some(block) = block else {
            return;
        };
        let saved = (self.binding.take(), self.local.clone());
        for statement in &block.stmts {
            if let syn::Stmt::Item(syn::Item::Use(import)) = statement {
                self.visit_item_use(import);
            }
        }
        if self.nested == 0
            && let [syn::Stmt::Expr(syn::Expr::MethodCall(call), None)] = block.stmts.as_slice()
            && call.method == name
            && call.args.is_empty()
            && self.watched.callable(&name)
        {
            let at = self.at(call.method.span());
            if name == self.watched.read {
                self.facts.push(Fact::Read {
                    at,
                    shape: ReadShape::Forward,
                });
            } else {
                self.facts.push(Fact::DoorCall {
                    name,
                    at,
                    forward: true,
                });
            }
            self.visit_expr(&call.receiver);
        } else {
            for statement in &block.stmts {
                if !matches!(statement, syn::Stmt::Item(syn::Item::Use(_))) {
                    self.top_statement(statement);
                }
            }
        }
        (self.binding, self.local) = saved;
    }

    /// A statement written directly in the fn's block.
    fn top_statement(&mut self, statement: &syn::Stmt) {
        if self.nested == 0
            && let syn::Stmt::Local(local) = statement
            && let Some(init) = &local.init
            && init.diverge.is_none()
            && let syn::Expr::MethodCall(call) = unparenthesised(&init.expr)
            && call.method == self.watched.read
            && call.args.is_empty()
        {
            for attribute in &local.attrs {
                self.visit_attribute(attribute);
            }
            let at = self.at(call.method.span());
            let bound = match &local.pat {
                syn::Pat::Ident(ident) if ident.by_ref.is_none() && ident.subpat.is_none() => {
                    Some(ident.ident.to_string())
                }
                _ => None,
            };
            self.facts.push(Fact::Read {
                at,
                shape: if bound.is_some() {
                    ReadShape::Statement
                } else {
                    ReadShape::Stray("bound by a pattern other than a plain name")
                },
            });
            self.read_pattern(&local.pat, "a `let`", Allow::BOTH);
            self.visit_expr(&call.receiver);
            self.binding = bound;
            return;
        }
        self.visit_stmt(statement);
    }

    fn is_binding(&self, expression: &syn::Expr) -> bool {
        let syn::Expr::Path(path) = unparenthesised(expression) else {
            return false;
        };
        path.qself.is_none()
            && self
                .binding
                .as_ref()
                .is_some_and(|binding| path.path.is_ident(binding))
    }

    /// Every watched identifier in a macro's tokens: a read's or a door's name always, the mark's
    /// where the tokens before it make it the mark (`lane :: Mark`, or a bare name in scope).
    fn macro_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        let mut before: Vec<String> = Vec::new();
        let mut colons = 0;
        for tree in tokens {
            match tree {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    let path = if colons == 2 && !before.is_empty() {
                        before.push(name.clone());
                        before.clone()
                    } else {
                        before = vec![name.clone()];
                        before.clone()
                    };
                    colons = 0;
                    let mark =
                        self.watched
                            .names_mark(&path, path.len() - 1, &self.module, &self.local);
                    if self.watched.callable(&name) || mark {
                        let at = self.at(ident.span());
                        self.facts.push(Fact::Shape {
                            at,
                            what: format!(
                                "`{name}` in a macro's arguments: the guard does not read them"
                            ),
                        });
                    }
                }
                proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':' => colons += 1,
                proc_macro2::TokenTree::Group(group) => {
                    before.clear();
                    colons = 0;
                    self.macro_tokens(group.stream());
                }
                proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {
                    before.clear();
                    colons = 0;
                }
            }
        }
    }

    /// One `use` tree written in a body, flattened under `prefix`.
    fn use_tree(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_tree(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => self.use_leaf(&name.ident, None, prefix),
            syn::UseTree::Rename(rename) => {
                self.use_leaf(&rename.ident, Some(&rename.rename), prefix);
            }
            syn::UseTree::Glob(_) => {
                let globbed = resolved(&self.module, prefix);
                for (name, declared) in &self.watched.marks {
                    if globbed == *declared {
                        self.local.insert(name.clone());
                    }
                }
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(tree, prefix);
                }
            }
        }
    }

    fn use_leaf(&mut self, ident: &syn::Ident, rename: Option<&syn::Ident>, prefix: &[String]) {
        let name = ident.to_string();
        let at = self.at(ident.span());
        let mark = self
            .watched
            .marks
            .get(&name)
            .is_some_and(|declared| resolved(&self.module, prefix) == *declared);
        match rename {
            Some(rename) if mark => self.facts.push(Fact::Shape {
                at,
                what: format!(
                    "`{name}` imported as `{rename}`: the guard reads the name as written"
                ),
            }),
            None if mark => {
                self.local.insert(name);
                self.facts.push(Fact::Known { at });
            }
            _ => {}
        }
    }
}

/// Which forms of the mark's name a place admits whole.
#[derive(Clone, Copy)]
struct Allow {
    borrowed: bool,
    whole: bool,
}

impl Allow {
    const NONE: Self = Self {
        borrowed: false,
        whole: false,
    };
    const BORROWED: Self = Self {
        borrowed: true,
        whole: false,
    };
    const WHOLE: Self = Self {
        borrowed: false,
        whole: true,
    };
    const BOTH: Self = Self {
        borrowed: true,
        whole: true,
    };
}

/// **The standard library's macros whose arguments are expressions**, each evaluated once where
/// the macro is written (`vec![a, b]` as an array literal, `format!`'s arguments, an assertion's
/// operands): their arguments are read as the code around them. Every other macro's tokens are
/// a shape — a macro of the tree may wrap them in a loop or a closure.
const EXPRESSION_MACROS: &[&str] = &[
    "vec",
    "format",
    "format_args",
    "print",
    "println",
    "eprint",
    "eprintln",
    "write",
    "writeln",
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
];

/// A macro's arguments as comma-separated expressions (or `vec!`'s `value; count`), if they
/// parse as such.
fn expression_arguments(tokens: &proc_macro2::TokenStream) -> Option<Vec<syn::Expr>> {
    use syn::parse::Parser;
    let listed = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
        .parse2(tokens.clone());
    if let Ok(listed) = listed {
        return Some(listed.into_iter().collect());
    }
    let repeated = |input: syn::parse::ParseStream<'_>| -> syn::Result<Vec<syn::Expr>> {
        let value: syn::Expr = input.parse()?;
        input.parse::<syn::Token![;]>()?;
        let count: syn::Expr = input.parse()?;
        Ok(vec![value, count])
    };
    repeated.parse2(tokens.clone()).ok()
}

fn unparenthesised(expression: &syn::Expr) -> &syn::Expr {
    match expression {
        syn::Expr::Paren(inner) => unparenthesised(&inner.expr),
        syn::Expr::Group(inner) => unparenthesised(&inner.expr),
        other => other,
    }
}

impl<'ast> Visit<'ast> for Reader<'_> {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let at = self.at(call.method.span());
        if name == self.watched.read {
            let shape = ReadShape::Stray(self.why());
            self.facts.push(Fact::Read { at, shape });
        } else if self.watched.doors.contains(&name) {
            self.facts.push(Fact::DoorCall {
                name,
                at,
                forward: false,
            });
        }
        for attribute in &call.attrs {
            self.visit_attribute(attribute);
        }
        if !self.is_binding(&call.receiver) {
            self.visit_expr(&call.receiver);
        }
        if let Some(turbofish) = &call.turbofish {
            self.visit_angle_bracketed_generic_arguments(turbofish);
        }
        for argument in &call.args {
            self.visit_expr(argument);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = unparenthesised(&call.func)
            && let Some(last) = path.path.segments.last()
            && self.watched.callable(&last.ident.to_string())
        {
            let at = self.at(last.ident.span());
            self.facts.push(Fact::Shape {
                at,
                what: format!(
                    "`{}` called by its path: the guard reads a method call",
                    last.ident
                ),
            });
            for argument in &call.args {
                self.visit_expr(argument);
            }
            return;
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path.qself.is_none()
            && let Some(binding) = &self.binding
            && path.path.is_ident(binding)
        {
            let at = self.at(path.path.segments[0].ident.span());
            self.facts.push(Fact::Moved {
                at,
                binding: binding.clone(),
            });
            return;
        }
        for (index, segment) in path.path.segments.iter().enumerate() {
            let name = segment.ident.to_string();
            let at = self.at(segment.ident.span());
            if self.watched.callable(&name) {
                self.facts.push(Fact::Shape {
                    at,
                    what: format!("`{name}` named as a value: the guard reads a method call"),
                });
            } else if self.names_mark(&path.path, index) {
                self.facts.push(Fact::MarkType {
                    at,
                    kept: true,
                    place: "an expression path",
                });
            }
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_reference(&mut self, reference: &'ast syn::ExprReference) {
        if reference.mutability.is_none() && self.is_binding(&reference.expr) {
            return;
        }
        visit::visit_expr_reference(self, reference);
    }

    fn visit_expr_struct(&mut self, literal: &'ast syn::ExprStruct) {
        if let Some(last) = literal.path.segments.last()
            && self.names_mark(&literal.path, literal.path.segments.len() - 1)
        {
            let at = self.at(last.ident.span());
            self.facts.push(Fact::Minting { at });
            for field in &literal.fields {
                self.visit_expr(&field.expr);
            }
            if let Some(rest) = &literal.rest {
                self.visit_expr(rest);
            }
            return;
        }
        visit::visit_expr_struct(self, literal);
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.closures += 1;
        for input in &closure.inputs {
            self.read_pattern(input, "a closure's parameter", Allow::BORROWED);
        }
        if let syn::ReturnType::Type(_, ty) = &closure.output {
            self.read_type(ty, "a closure's return type", Allow::NONE);
        }
        self.visit_expr(&closure.body);
        self.closures -= 1;
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.loops += 1;
        visit::visit_expr_while(self, node);
        self.loops -= 1;
    }

    fn visit_expr_loop(&mut self, node: &'ast syn::ExprLoop) {
        self.loops += 1;
        visit::visit_expr_loop(self, node);
        self.loops -= 1;
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.loops += 1;
        visit::visit_expr_for_loop(self, node);
        self.loops -= 1;
    }

    fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
        self.asyncs += 1;
        visit::visit_expr_async(self, node);
        self.asyncs -= 1;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.nested += 1;
        self.read_fn(&item.sig, Some(&item.block), false);
        self.nested -= 1;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.nested += 1;
        self.read_fn(&item.sig, Some(&item.block), false);
        self.nested -= 1;
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        for attribute in &local.attrs {
            self.visit_attribute(attribute);
        }
        self.read_pattern(&local.pat, "a `let`", Allow::BOTH);
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
    }

    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        for (index, segment) in path.path.segments.iter().enumerate() {
            if self.names_mark(&path.path, index) {
                let at = self.at(segment.ident.span());
                self.facts.push(Fact::MarkType {
                    at,
                    kept: true,
                    place: self.place,
                });
            }
        }
        visit::visit_type_path(self, path);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        let saved = std::mem::replace(&mut self.place, "a field");
        visit::visit_field(self, field);
        self.place = saved;
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        let saved = std::mem::replace(&mut self.place, "a static");
        visit::visit_item_static(self, item);
        self.place = saved;
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        let saved = std::mem::replace(&mut self.place, "a const");
        visit::visit_item_const(self, item);
        self.place = saved;
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        let std_macro = invocation
            .path
            .segments
            .last()
            .is_some_and(|last| EXPRESSION_MACROS.contains(&last.ident.to_string().as_str()));
        if std_macro && let Some(arguments) = expression_arguments(&invocation.tokens) {
            for argument in &arguments {
                self.visit_expr(argument);
            }
            return;
        }
        self.macro_tokens(invocation.tokens.clone());
        visit::visit_macro(self, invocation);
    }

    fn visit_item_use(&mut self, import: &'ast syn::ItemUse) {
        let mut prefix = Vec::new();
        if import.leading_colon.is_some() {
            return;
        }
        self.use_tree(&import.tree, &mut prefix);
    }
}

// ── the tree's items ──────────────────────────────────────────────────────

/// An item's paths, `module::Owner::name`, one per module path.
fn keys(record: &ItemRecord) -> Vec<String> {
    let mut keys: Vec<String> = record
        .module_paths()
        .iter()
        .map(|module| match record.type_owner() {
            Some(owner) => format!("{module}::{owner}::{}", record.name()),
            None => format!("{module}::{}", record.name()),
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The callable items `path` names, test items included (an adapter is one).
fn resolve(index: &Index, path: &str) -> Vec<usize> {
    index
        .items()
        .iter()
        .enumerate()
        .filter(|(_, record)| record.kind().is_callable())
        .filter(|(_, record)| keys(record).iter().any(|key| key == path))
        .map(|(at, _)| at)
        .collect()
}

/// Whether no product build compiles `record`.
fn test_only(index: &Index, record: &ItemRecord) -> bool {
    !index.file_of(record).permits_product()
        || !record
            .identities()
            .any(|identity| identity.variant.permits_product())
}

/// Whether a product build compiles `span`.
fn in_product(index: &Index, span: Span) -> bool {
    Occurrence {
        span,
        certainty: Certainty::Resolved,
    }
    .in_the_product(index)
}

/// The identifier token that starts at `at` — every fact's place is one.
fn token_at(index: &Index, at: usize) -> Span {
    let tokens = index.tokens();
    let from = tokens.partition_point(|token| token.span().start() < at);
    tokens
        .get(from)
        .filter(|token| token.span().start() == at)
        .map(|token| token.span())
        .unwrap_or_else(|| panic!("no identifier token at union offset {at}"))
}

/// `file:line` of a union offset, relative to `root`.
fn place(index: &Index, at: usize, root: &Path) -> String {
    index.locate(at).map_or_else(
        || format!("offset {at}"),
        |location| {
            let path = location
                .file
                .canonicalize()
                .unwrap_or_else(|_| location.file.to_path_buf());
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            format!("{shown}:{}", location.line)
        },
    )
}

/// **The record a union offset is written in**, the smallest that holds it — an item, an `impl`
/// block, a trait, a value, an alias or a `use` — with its module path and its span.
fn container(index: &Index, at: usize) -> Option<(String, Span)> {
    let mut best: Option<(String, Span)> = None;
    let mut consider = |module: Option<&str>, whole: Span| {
        if whole.start() <= at
            && at < whole.end()
            && best
                .as_ref()
                .is_none_or(|(_, held)| whole.end() - whole.start() < held.end() - held.start())
            && let Some(module) = module
        {
            best = Some((module.to_owned(), whole));
        }
    };
    for record in index.items() {
        consider(record.module_paths().first().copied(), record.whole());
    }
    for block in index.impls() {
        consider(block.module_paths().first().copied(), block.whole());
    }
    for declared in index.traits() {
        consider(declared.module_paths().first().copied(), declared.whole());
    }
    for value in index.values() {
        consider(value.module_paths().first().copied(), value.whole());
    }
    for alias in index.aliases() {
        consider(alias.module_paths().first().copied(), alias.whole());
    }
    for import in index.uses() {
        consider(import.module_paths().first().copied(), import.whole());
    }
    best
}

/// **Whether the token `name` at `at` names the mark**, read from the text before it in its
/// record: `<its module's name> :: name`, or a bare name where the mark is bare. `None`: the
/// token is in no record.
fn token_names_mark(index: &Index, watched: &Watched, name: &str, at: usize) -> Option<bool> {
    let (module, whole) = container(index, at)?;
    let before = index.text(whole)[..at - whole.start()].trim_end();
    let mut path = vec![name.to_owned()];
    if let Some(qualified) = before.strip_suffix("::") {
        let qualified = qualified.trim_end();
        let start = qualified
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(0, |at| at + 1);
        path.insert(0, qualified[start..].to_owned());
    }
    Some(watched.names_mark(&path, path.len() - 1, &module, &BTreeSet::new()))
}

/// **The guard over one package**, its files shown relative to `root`.
#[allow(clippy::too_many_lines)]
fn judge(index: &Index, package: &str, rules: &Rules, root: &Path) -> Judged {
    let mut judged = Judged::default();
    let watched = watched(index, rules);
    let find = |check: &'static str, at: usize, what: String| Finding {
        check,
        at: place(index, at, root),
        what,
    };
    let unplaced = |check: &'static str, what: String| Finding {
        check,
        at: String::from("the rules"),
        what,
    };

    // D3: every path the rules name is one fn.
    for path in rules.paths() {
        let found = resolve(index, path);
        if found.len() != 1 {
            judged.findings.push(unplaced(
                "D3",
                format!("`{path}` names {} fn(s), not one", found.len()),
            ));
        }
    }

    // The reading: every product callable, every product data type, value and alias.
    let mut facts: Vec<(Vec<String>, Fact)> = Vec::new();
    for record in index.items() {
        if test_only(index, record) {
            continue;
        }
        let base = record.whole().start();
        let text = index.text(record.whole());
        let module = record.module_paths().first().copied().unwrap_or("crate");
        let mut reader = Reader::new(base, &watched, module);
        let record_keys = keys(record);
        if record.kind().is_callable() {
            judged.fns_read += 1;
            let may_return = rules.may_return(&record_keys);
            let parsed = if record.type_owner().is_some() {
                syn::parse_str::<syn::ImplItemFn>(text)
                    .map(|item| reader.read_fn(&item.sig, Some(&item.block), may_return))
            } else if record.trait_name().is_some() {
                syn::parse_str::<syn::TraitItemFn>(text)
                    .map(|item| reader.read_fn(&item.sig, item.default.as_ref(), may_return))
            } else {
                syn::parse_str::<syn::ItemFn>(text)
                    .map(|item| reader.read_fn(&item.sig, Some(&item.block), may_return))
            };
            if let Err(error) = parsed {
                judged.findings.push(find(
                    "shape",
                    base,
                    format!("`{}` does not parse as a fn: {error}", record.name()),
                ));
            }
        } else if matches!(
            record.kind(),
            bt_source::ItemKind::Struct | bt_source::ItemKind::Enum | bt_source::ItemKind::Union
        ) {
            match syn::parse_str::<syn::Item>(text) {
                Ok(item) => {
                    if record.name() == rules.mark
                        && record.module_paths().contains(&rules.mark_module)
                        && let syn::Item::Struct(declared) = &item
                    {
                        let at = reader.at(declared.ident.span());
                        reader.facts.push(Fact::Known { at });
                    }
                    reader.visit_item(&item);
                }
                Err(error) => judged.findings.push(find(
                    "shape",
                    base,
                    format!("`{}` does not parse: {error}", record.name()),
                )),
            }
        }
        facts.extend(
            reader
                .facts
                .into_iter()
                .map(|fact| (record_keys.clone(), fact)),
        );
    }
    // D1 reads the mark module's `Contract` consts on every arm: they are the contract suite's
    // table (`#[cfg(test)]` in the tree), what the registry is held equal to.
    let mut contracts: BTreeMap<String, (usize, Delivery)> = BTreeMap::new();
    for value in index.values() {
        let at = value.whole().start();
        let product = in_product(index, value.whole());
        let contract_table = value.module_paths().contains(&rules.mark_module);
        if !product && !contract_table {
            continue;
        }
        let Ok(item) = syn::parse_str::<syn::Item>(index.text(value.whole())) else {
            judged.findings.push(find(
                "shape",
                at,
                format!("`{}` does not parse", value.name()),
            ));
            continue;
        };
        if product {
            let module = value.module_paths().first().copied().unwrap_or("crate");
            let mut reader = Reader::new(at, &watched, module);
            reader.visit_item(&item);
            facts.extend(reader.facts.into_iter().map(|fact| (Vec::new(), fact)));
        }
        if contract_table
            && value.kind() == ValueKind::Const
            && let syn::Item::Const(constant) = &item
            && let syn::Type::Path(ty) = &*constant.ty
            && ty
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == rules.contract)
        {
            judged.contracts_read += 1;
            match delivery_of(&constant.expr) {
                Some(delivery) => {
                    contracts.insert(value.name().to_owned(), (at, delivery));
                }
                None => judged.findings.push(find(
                    "shape",
                    at,
                    format!(
                        "`{}`: its delivery is not written as `delivery: Delivery::…` in its \
                         literal",
                        value.name()
                    ),
                )),
            }
        }
    }
    for alias in index.aliases() {
        let at = alias.whole().start();
        if !in_product(index, alias.whole()) || !watched.marks.contains_key(alias.name()) {
            continue;
        }
        let module = alias.module_paths().first().copied().unwrap_or("crate");
        let mut reader = Reader::new(at, &watched, module);
        if let Ok(syn::Item::Type(item)) = syn::parse_str::<syn::Item>(index.text(alias.whole())) {
            let name_at = reader.at(item.ident.span());
            reader.facts.push(Fact::Known { at: name_at });
            reader.read_type(&item.ty, "an alias", Allow::WHOLE);
        }
        facts.extend(reader.facts.into_iter().map(|fact| (Vec::new(), fact)));
    }
    // The mark's own `impl` headers, and the module-level imports of it (a rename is a shape).
    for block in index.impls() {
        if block.type_owner() != rules.mark || !block.module_paths().contains(&rules.mark_module) {
            continue;
        }
        let header = block.whole().start()..block.body().start();
        for token in index.identifier_occurrences(rules.mark, View::Identifiers) {
            if header.contains(&token.start()) {
                facts.push((Vec::new(), Fact::Known { at: token.start() }));
            }
        }
    }
    for import in index.uses() {
        if !in_product(index, import.whole()) {
            continue;
        }
        let whole = import.whole().start()..import.whole().end();
        for (name, declared) in &watched.marks {
            let renamed = import.module_paths().iter().any(|module| {
                import.names().iter().any(|(bound, path)| {
                    path.split_last().is_some_and(|(last, prefix)| {
                        last == name && bound != name && resolved(module, prefix) == *declared
                    })
                })
            });
            for token in index.identifier_occurrences(name, View::Identifiers) {
                if !whole.contains(&token.start()) {
                    continue;
                }
                facts.push((
                    Vec::new(),
                    if renamed {
                        Fact::Shape {
                            at: token.start(),
                            what: format!(
                                "`{name}` imported under another name: the guard reads the name \
                                 as written"
                            ),
                        }
                    } else {
                        Fact::Known { at: token.start() }
                    },
                ));
            }
        }
    }
    facts.retain(|(_, fact)| in_product(index, token_at(index, fact.at())));

    // Every watched product token is one the reading classified.
    let accounted: BTreeSet<usize> = facts.iter().map(|(_, fact)| fact.at()).collect();
    for name in &watched.every {
        for token in index.identifier_occurrences(name, View::Identifiers) {
            if !in_product(index, token) {
                continue;
            }
            let watched_here = watched.callable(name)
                || token_names_mark(index, &watched, name, token.start()).unwrap_or(true);
            if !watched_here {
                continue;
            }
            judged.tokens_read += 1;
            if !accounted.contains(&token.start()) {
                judged.findings.push(find(
                    "shape",
                    token.start(),
                    format!("`{name}` written where the guard's grammar does not read it"),
                ));
            }
        }
    }

    // D1: the contracts and the registry.
    let mut registered: BTreeMap<&str, Delivery> = BTreeMap::new();
    for drain in rules.drains {
        for contract in drain.contracts {
            if registered.insert(contract, drain.delivery).is_some() {
                judged
                    .findings
                    .push(unplaced("D1", format!("`{contract}` is in two rows")));
            }
        }
    }
    for (name, (at, delivery)) in &contracts {
        match registered.get(name.as_str()) {
            None => judged.findings.push(find(
                "D1",
                *at,
                format!("the contract `{name}` ({delivery:?}) is in no row of LANE_DRAINS"),
            )),
            Some(row) if row != delivery => judged.findings.push(find(
                "D1",
                *at,
                format!("the contract `{name}` delivers {delivery:?}; its row says {row:?}"),
            )),
            Some(_) => {}
        }
    }
    for contract in registered.keys() {
        if !contracts.contains_key(*contract) {
            judged.findings.push(unplaced(
                "D1",
                format!(
                    "the row's contract `{contract}` is no product `const` of `{}`",
                    rules.contract
                ),
            ));
        }
    }

    // D2: the reads, the forwarders, the doors.
    let named = |item: &[String], path: &str| item.iter().any(|key| key == path);
    let drains: Vec<&str> = rules.drains.iter().map(|drain| drain.drain).collect();
    let mut reads: BTreeMap<&str, Vec<(usize, ReadShape)>> = BTreeMap::new();
    let mut door_calls: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (item, fact) in &facts {
        match fact {
            Fact::Read { at, shape } => {
                let drain = drains.iter().find(|drain| named(item, drain));
                let forwarder = rules.forwarders.iter().find(|path| named(item, path));
                match (drain, forwarder, shape) {
                    (Some(drain), _, _) => {
                        reads.entry(*drain).or_default().push((*at, shape.clone()))
                    }
                    (None, Some(_), ReadShape::Forward) => {}
                    (None, Some(path), _) => judged.findings.push(find(
                        "D2",
                        *at,
                        format!("the forwarder `{path}` is not the one call of its body"),
                    )),
                    (None, None, ReadShape::Stray(why)) => judged.findings.push(find(
                        "D2",
                        *at,
                        format!("a mark read outside a registered drain, {why}"),
                    )),
                    (None, None, _) => judged.findings.push(find(
                        "D2",
                        *at,
                        String::from("a mark read outside a registered drain"),
                    )),
                }
            }
            Fact::DoorCall { name, at, forward } => {
                let door = rules
                    .doors
                    .iter()
                    .find(|door| door.name == name)
                    .expect("a door call is of a door");
                if named(item, door.caller) {
                    *door_calls.entry((door.name, door.caller)).or_default() += 1;
                } else if let Some(path) = door.forwarders.iter().find(|path| named(item, path)) {
                    *door_calls.entry((door.name, *path)).or_default() += 1;
                    if !forward {
                        judged.findings.push(find(
                            "D2",
                            *at,
                            format!("the forwarder `{path}` is not the one call of its body"),
                        ));
                    }
                } else {
                    judged.findings.push(find(
                        "D2",
                        *at,
                        format!(
                            "the door `{name}` called outside its one caller `{}`",
                            door.caller
                        ),
                    ));
                }
            }
            Fact::Def { name, at } => {
                let allowed = if *name == rules.read {
                    rules
                        .minted_by
                        .iter()
                        .chain(rules.forwarders)
                        .any(|path| named(item, path))
                } else {
                    rules
                        .doors
                        .iter()
                        .filter(|door| door.name == name)
                        .any(|door| {
                            named(item, door.defined)
                                || door.forwarders.iter().any(|path| named(item, path))
                        })
                };
                if !allowed {
                    judged.findings.push(find(
                        "D2",
                        *at,
                        format!("a fn named `{name}` that the rules do not list"),
                    ));
                }
            }
            Fact::MarkType { at, kept, place } => {
                if *kept {
                    judged.findings.push(find(
                        "D5",
                        *at,
                        format!("the mark named in {place}: a mark is never kept"),
                    ));
                }
            }
            Fact::Minting { at } => {
                if !rules.minted_by.iter().any(|path| named(item, path)) {
                    judged.findings.push(find(
                        "D5",
                        *at,
                        String::from("a mark built outside its ledger's minting fns"),
                    ));
                }
            }
            Fact::Moved { at, binding } => {
                if drains.iter().any(|drain| named(item, drain)) {
                    judged.findings.push(find(
                        "D5",
                        *at,
                        format!("the drain's mark `{binding}` used other than borrowed"),
                    ));
                }
            }
            Fact::Shape { at, what } => judged.findings.push(find("shape", *at, what.clone())),
            Fact::Known { .. } => {}
        }
    }
    for drain in &drains {
        let found = reads.get(drain).map_or(&[][..], Vec::as_slice);
        match found {
            [(_, ReadShape::Statement)] => {}
            [] => judged.findings.push(unplaced(
                "D2",
                format!("the registered drain `{drain}` reads no mark"),
            )),
            [(at, ReadShape::Stray(why))] => {
                judged
                    .findings
                    .push(find("D2", *at, format!("`{drain}` reads its mark {why}")))
            }
            several => {
                for (at, shape) in several {
                    let why = match shape {
                        ReadShape::Stray(why) => *why,
                        ReadShape::Statement | ReadShape::Forward => "as a `let`",
                    };
                    judged.findings.push(find(
                        "D2",
                        *at,
                        format!(
                            "`{drain}` reads its mark {} times; this one {why}",
                            several.len()
                        ),
                    ));
                }
            }
        }
    }
    for door in rules.doors {
        for path in door.forwarders.iter().chain([&door.caller]) {
            let calls = door_calls.get(&(door.name, *path)).copied().unwrap_or(0);
            if calls != 1 {
                judged.findings.push(unplaced(
                    "D2",
                    format!(
                        "`{path}` calls the door `{}` {calls} time(s), not once",
                        door.name
                    ),
                ));
            }
        }
    }

    // D4 and D6, over the call graph.
    let resolver = Resolver::new(&[(package, index)], Arms::filtered(|_| true))
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let graph = CallGraph::new(&resolver);
    let id_of = |path: &str| -> Option<ItemId> {
        match resolve(index, path).as_slice() {
            [at] => resolver.id_of(package, &index.items()[*at]),
            _ => None,
        }
    };
    let callees = |id: ItemId| -> BTreeSet<ItemId> {
        resolver
            .resolve_callees(id)
            .iter()
            .filter_map(|edge| match &edge.to {
                Target::Item(callee) => Some(*callee),
                Target::Unresolved(unresolved) => unresolved.sole_candidate(),
                _ => None,
            })
            .collect()
    };
    let reviewed: Vec<(ItemId, ItemId)> = rules
        .follows
        .iter()
        .filter_map(|(caller, callee)| Some((id_of(caller)?, id_of(callee)?)))
        .collect();
    let follow = |edge: &Edge| -> Vec<ItemId> {
        let Target::Unresolved(unresolved) = &edge.to else {
            return Vec::new();
        };
        let candidates = match unresolved {
            Unresolved::Ambiguous { candidates, .. }
            | Unresolved::Dynamic { candidates, .. }
            | Unresolved::OtherArm { candidates } => candidates.as_slice(),
            _ => &[],
        };
        let mut next: Vec<ItemId> = unresolved.sole_candidate().into_iter().collect();
        next.extend(
            reviewed
                .iter()
                .filter(|(caller, callee)| {
                    edge.from == Node::Item(*caller) && candidates.contains(callee)
                })
                .map(|(_, callee)| *callee),
        );
        next
    };
    for drain in rules.drains {
        let takes: BTreeSet<ItemId> = drain.takes.iter().filter_map(|path| id_of(path)).collect();
        if drain.adapters.is_empty() {
            judged.findings.push(unplaced(
                "D4",
                format!("`{}` names no contract adapter", drain.drain),
            ));
        }
        for adapter in drain.adapters {
            let Some(root) = id_of(adapter) else {
                continue;
            };
            let reach = graph.walk_following(&[root], &takes, &follow);
            if reach.stopped_at.is_disjoint(&takes) {
                judged.findings.push(find(
                    "D4",
                    index.items()[resolve(index, adapter)[0]].whole().start(),
                    format!(
                        "the adapter `{adapter}` reaches none of `{}`'s takes ({})",
                        drain.drain,
                        drain.takes.join(", ")
                    ),
                ));
            }
        }
    }
    let merge = id_of(rules.merge);
    let mut merges: BTreeSet<ItemId> = merge.into_iter().collect();
    for wrapper in rules.wrappers {
        let Some(id) = id_of(wrapper) else {
            continue;
        };
        if merge.is_some_and(|merge| callees(id).contains(&merge)) {
            merges.insert(id);
        } else {
            judged.findings.push(unplaced(
                "D6",
                format!("the wrapper `{wrapper}` does not call `{}`", rules.merge),
            ));
        }
    }
    for drain in rules.drains {
        if let Targets::Many = drain.targets
            && let Some(id) = id_of(drain.drain)
            && callees(id).is_disjoint(&merges)
        {
            judged.findings.push(find(
                "D6",
                index.items()[resolve(index, drain.drain)[0]]
                    .whole()
                    .start(),
                format!(
                    "`{}` drains several askers and calls neither `{}` nor a registered wrapper",
                    drain.drain, rules.merge
                ),
            ));
        }
    }
    judged.findings.sort();
    judged.findings.dedup();
    judged
}

/// The delivery a contract's literal writes, `delivery: Delivery::…`.
fn delivery_of(expression: &syn::Expr) -> Option<Delivery> {
    let syn::Expr::Struct(literal) = unparenthesised(expression) else {
        return None;
    };
    let field = literal
        .fields
        .iter()
        .find(|field| matches!(&field.member, syn::Member::Named(name) if name == "delivery"))?;
    let syn::Expr::Path(path) = unparenthesised(&field.expr) else {
        return None;
    };
    match path.path.segments.last()?.ident.to_string().as_str() {
        "Submission" => Some(Delivery::Submission),
        "NewestAdopted" => Some(Delivery::NewestAdopted),
        _ => None,
    }
}

// ── the tree ──────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace is there")
}

/// **The tree's drains hold** (D1–D6, and no shape the grammar does not read).
#[test]
fn every_lane_drain_reads_its_mark_once_and_keeps_none() {
    let index = Index::of_package("bt-app");
    let judged = judge(index, "bt-app", &TREE, &workspace_root());
    println!(
        "lane-drains guard: {} row(s), {} contract(s), {} product fn(s) read, {} watched token(s) \
         read, {} finding(s)",
        LANE_DRAINS.len(),
        judged.contracts_read,
        judged.fns_read,
        judged.tokens_read,
        judged.findings.len()
    );
    let shown: Vec<String> = judged.findings.iter().map(ToString::to_string).collect();
    assert!(
        judged.fns_read > 0 && judged.tokens_read > 0 && judged.contracts_read > 0,
        "the guard read nothing of bt-app, and so proved nothing:\n  {}",
        shown.join("\n  ")
    );
    for drain in LANE_DRAINS {
        if let Targets::One(why) = drain.targets {
            println!("  D6 exempt: {} ({why})", drain.drain);
        }
    }
    assert!(
        judged.findings.is_empty(),
        "a lane drain breaks its mark's discipline ({}):\n  {}",
        shown.len(),
        shown.join("\n  ")
    );
}

// ── the fixtures ──────────────────────────────────────────────────────────

/// The fixture's drains: one `Submission` drain of several askers and the shared
/// `NewestAdopted` row.
const FIXTURE_DRAINS: &[Drain] = &[
    Drain {
        contracts: &["ASKS"],
        delivery: Delivery::Submission,
        drain: "crate::asks::App::answers",
        takes: &["crate::asks::Window::answered"],
        adapters: &["crate::asks::contract_adapter::Adapter::drain"],
        targets: Targets::Many,
    },
    Drain {
        contracts: &["LATEST"],
        delivery: Delivery::NewestAdopted,
        drain: "crate::lane::Coalescing::ended",
        takes: &["crate::lane::Coalescing::ended"],
        adapters: &["crate::latest_adapter::Adapter::drain"],
        targets: Targets::One("adoption orders it"),
    },
];

const FIXTURE_DOORS: &[Door] = &[Door {
    name: "mark_after_the_worker_ended",
    defined: "crate::lane::Ledger::mark_after_the_worker_ended",
    forwarders: &["crate::asks::Lane::mark_after_the_worker_ended"],
    caller: "crate::asks::App::at_the_stop",
}];

const FIXTURE: Rules = Rules {
    contract: "Contract",
    mark: "Mark",
    mark_module: "crate::lane",
    read: "drain_mark",
    minted_by: &[
        "crate::lane::Ledger::drain_mark",
        "crate::lane::Ledger::mark_after_the_worker_ended",
    ],
    forwarders: &["crate::asks::Lane::drain_mark"],
    merge: "crate::lane::in_admission_order",
    wrappers: &[],
    drains: FIXTURE_DRAINS,
    doors: FIXTURE_DOORS,
    follows: &[],
};

/// The fixture `variant`, judged: its findings as `check at: what`, the directory stripped.
fn fixture(variant: &str) -> Vec<Finding> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("lane_drains")
        .join(variant);
    let universe = Universe::declare(
        "the lane-drains fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "lookalike".to_owned(),
                kind: TargetKind::Library,
                name: "lookalike".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let root = directory.canonicalize().expect("the fixture is there");
    let judged = judge(&index, "lookalike", &FIXTURE, &root);
    assert!(
        judged.fns_read > 0 && judged.tokens_read > 0,
        "{variant}: nothing read"
    );
    judged.findings
}

/// The findings of `variant` are exactly `expected`, each a check and a fragment of its message.
fn assert_findings(variant: &str, expected: &[(&str, &str, &str)]) {
    let found = fixture(variant);
    let shown: Vec<String> = found.iter().map(ToString::to_string).collect();
    println!("{variant}:\n  {}", shown.join("\n  "));
    assert_eq!(
        found.len(),
        expected.len(),
        "{variant}: {} finding(s), {} expected:\n  {}",
        found.len(),
        expected.len(),
        shown.join("\n  ")
    );
    for (check, at, fragment) in expected {
        assert!(
            found.iter().any(|finding| finding.check == *check
                && finding.at == *at
                && finding.what.contains(fragment)),
            "{variant}: no {check} at {at} saying `{fragment}`:\n  {}",
            shown.join("\n  ")
        );
    }
}

/// GREEN — the miniature seam as the tree writes it: one read per drain, borrowed, merged; the
/// door called by its one caller through its forwarder; the adapters reaching their takes.
#[test]
fn the_fixture_seam_as_written_is_green() {
    assert_findings("green", &[]);
}

/// D1 — a `Submission` contract no row drains.
#[test]
fn d1_a_contract_in_no_row_is_refused() {
    assert_findings(
        "d1_unregistered",
        &[(
            "D1",
            "lib.rs:23",
            "the contract `MORE` (Submission) is in no row",
        )],
    );
}

/// D2 — the mark read inside a closure (the `.map(|app| app.lane.drain_mark())` shape the tree's
/// two closure reads had before row 110 rewrote them as `let … else`).
#[test]
fn d2_a_read_inside_a_closure_is_refused() {
    assert_findings(
        "d2_closure",
        &[("D2", "lib.rs:116", "reads its mark inside a closure")],
    );
}

/// D2 — the mark re-read for every asker, inside the loop over them.
#[test]
fn d2_a_read_per_asker_is_refused() {
    assert_findings(
        "d2_twice",
        &[
            (
                "D2",
                "lib.rs:116",
                "reads its mark 2 times; this one as a `let`",
            ),
            (
                "D2",
                "lib.rs:119",
                "reads its mark 2 times; this one inside a loop",
            ),
        ],
    );
}

/// D2 — the mark re-read for a second asker inside `vec![…]`, whose arguments the guard reads
/// as the array literal they are (the shape `App::storage_answers` writes its askers in).
#[test]
fn d2_a_read_inside_vec_is_refused() {
    assert_findings(
        "d2_vec",
        &[
            (
                "D2",
                "lib.rs:116",
                "reads its mark 2 times; this one as a `let`",
            ),
            (
                "D2",
                "lib.rs:119",
                "reads its mark 2 times; this one not the initializer of a `let`",
            ),
        ],
    );
}

/// D2 — a door called outside its one caller.
#[test]
fn d2_a_door_outside_its_caller_is_refused() {
    assert_findings(
        "d2_door",
        &[(
            "D2",
            "lib.rs:117",
            "the door `mark_after_the_worker_ended` called outside its one caller",
        )],
    );
}

/// D3 — a drain renamed under the registry: the path names nothing, and the read it holds is now
/// outside every registered drain.
#[test]
fn d3_a_path_naming_no_fn_is_refused() {
    assert_findings(
        "d3_renamed",
        &[
            (
                "D3",
                "the rules",
                "`crate::asks::App::answers` names 0 fn(s)",
            ),
            (
                "D2",
                "the rules",
                "`crate::asks::App::answers` reads no mark",
            ),
            ("D2", "lib.rs:116", "a mark read outside a registered drain"),
        ],
    );
}

/// D4 — an adapter whose drain reaches none of its row's takes.
#[test]
fn d4_an_adapter_that_reaches_no_take_is_refused() {
    assert_findings(
        "d4_adapter",
        &[(
            "D4",
            "lib.rs:140",
            "reaches none of `crate::asks::App::answers`'s takes",
        )],
    );
}

/// D5 — a mark kept in a field, and the drain's mark moved.
#[test]
fn d5_a_kept_or_moved_mark_is_refused() {
    assert_findings(
        "d5_kept",
        &[
            ("D5", "lib.rs:112", "the mark named in a field"),
            (
                "D5",
                "lib.rs:122",
                "the drain's mark `mark` used other than borrowed",
            ),
        ],
    );
}

/// D6 — a drain of several askers that concatenates them instead of merging.
#[test]
fn d6_a_drain_that_does_not_merge_is_refused() {
    assert_findings(
        "d6_unmerged",
        &[(
            "D6",
            "lib.rs:115",
            "calls neither `crate::lane::in_admission_order`",
        )],
    );
}

/// UNSUPPORTED — the read inside a macro's arguments is a diagnostic with its line, not silence;
/// the drain then holds no read the grammar reads.
#[test]
fn a_read_in_a_macro_is_a_shape_diagnostic() {
    assert_findings(
        "shape_macro",
        &[
            ("shape", "lib.rs:116", "`drain_mark` in a macro's arguments"),
            (
                "D2",
                "the rules",
                "`crate::asks::App::answers` reads no mark",
            ),
        ],
    );
}
