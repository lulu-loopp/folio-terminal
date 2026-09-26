//! **Who writes what** — the field census of
//! `docs/plans/design/ownership-census-2026-09-25.md`, as a query over item
//! identity (ticket census-1, the note's revision (b)2 "Ticket A,
//! re-specified").
//!
//! For every field of a named set of structs — the *facts* — the census says
//! which functions, in which modules, touch it in each of four ways, and it
//! lists every site it could not resolve:
//!
//! * **write** — a proven write: an assignment or compound assignment to a
//!   place whose receiver chain resolves by the stated rules, or a call whose
//!   method resolves by *(declared type, method)* to a declaration that writes
//!   (a workspace method taking `&mut self` and handing back no `&mut`, or a
//!   standard method on the fixed list in [`types`]);
//! * **access** — mutable access or escape, which proves nothing about a
//!   write: a `&mut` lend, `get_mut`, `iter_mut`, `as_mut`, an index in a
//!   mutable place, a `ref mut` pattern, a method that hands back `&mut`;
//! * **membership** — a change to what a *hub* holds (a field whose type holds
//!   one of the facts' own structs, `WindowRuntime.tabs`, `TabState.sessions`):
//!   `push`, `insert`, `remove`, `retain`, an assignment of the whole;
//! * **inner** — a change through a shared reference: `Cell`, `RefCell`,
//!   atomics, locks.
//!
//! **An unknown is output, not dropped.** A write-shaped site (an assignment, a
//! `&mut`, a method call) whose `.field` spells a field of the facts through a
//! receiver the rules do not type, or whose method does not resolve, is a row
//! of [`FieldCensus::unknowns`] with the item it is in and the reason. A fact
//! with an unknown site is `incomplete`, never single-writer.
//!
//! **The receiver rules**, all of them: `self` inside `impl X` is `X`; a
//! struct's own fields are looked up before its `Deref` target's, so
//! `Runtime.{app, window}` are `Runtime`'s and `self.pinned` inside
//! `impl Runtime` is `TabState`'s; typed parameters; `let` bindings of an
//! expression the rules type, with the language's default binding modes;
//! `for` bindings over the fixed list's iterators; closure parameters of the
//! fixed list's methods; functions and methods whose declared return type is
//! known (a generic return is bound only from an argument of that parameter's
//! type). A bare type name is looked for in the package it is written in
//! first, then in the workspace, and the declarations found must agree.
//!
//! **The module column is the declaring item's module path** — the
//! [`crate::ItemIdentity`] — and nothing about files: a writer moved into a
//! newly declared module changes that column and no other.

mod resolve;
mod types;
mod walk;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::index::Index;
use resolve::{Fact, Resolver, Subject};
pub use walk::Column;
use walk::Walk;

/// fact · column · module · function.
type RowKey = (String, Column, String, String);

/// The rows of one site kind, accumulated while the bodies are walked.
#[derive(Debug, Default)]
pub(crate) struct Sink {
    rows: BTreeMap<RowKey, (BTreeSet<String>, usize)>,
    unknowns: BTreeMap<(String, String, String, String), usize>,
}

impl Sink {
    fn row(&mut self, fact: String, column: Column, module: &str, function: &str, kind: String) {
        let entry = self
            .rows
            .entry((fact, column, module.to_owned(), function.to_owned()))
            .or_default();
        entry.0.insert(kind);
        entry.1 += 1;
    }

    fn unknown(&mut self, site: String, module: &str, function: &str, reason: String) {
        *self
            .unknowns
            .entry((site, module.to_owned(), function.to_owned(), reason))
            .or_default() += 1;
    }
}

/// One field of the facts, as the inventory states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactRow {
    /// `App.gpu`.
    pub fact: String,
    /// The declared type, as the census reads it.
    pub declared: String,
    /// Whether its type holds one of the facts' own structs.
    pub hub: bool,
    /// Distinct modules with a proven write.
    pub writer_modules: usize,
    /// Distinct functions with a proven write.
    pub writer_functions: usize,
    pub access_functions: usize,
    pub membership_functions: usize,
    pub inner_functions: usize,
    /// Unknown sites that name it: attributed to it, or spelling its name
    /// through a receiver that does not resolve.
    pub unknown_sites: usize,
}

impl FactRow {
    /// `0`, `1` or `n` — distinct modules with a proven write.
    #[must_use]
    pub fn proven_writers(&self) -> &'static str {
        match self.writer_modules {
            0 => "0",
            1 => "1",
            _ => "n",
        }
    }

    /// Whether no unknown site names it.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.unknown_sites == 0
    }
}

/// One fact × column × module × function.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SiteRow {
    pub fact: String,
    pub column: Column,
    pub module: String,
    pub function: String,
    /// `assign`, `compound`, `call:push`, `lend`, `index`, …
    pub kinds: Vec<String>,
    pub sites: usize,
}

/// One unresolved site kind, in one item.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnknownRow {
    /// `App.gpu` when the receiver resolved and something after it did not;
    /// `?.session` when the receiver did not.
    pub site: String,
    pub module: String,
    pub function: String,
    pub reason: String,
    pub sites: usize,
}

/// Why a census could not be taken at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CensusFailure {
    /// A struct named as a fact is not declared by the subject.
    NoSuchStruct(String),
}

impl fmt::Display for CensusFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchStruct(name) => write!(
                formatter,
                "`{name}` is not a struct the subject declares in exactly one module, so its fields cannot be counted"
            ),
        }
    }
}

impl std::error::Error for CensusFailure {}

/// The census, taken.
#[derive(Clone, Debug)]
pub struct FieldCensus {
    facts: Vec<FactRow>,
    rows: Vec<SiteRow>,
    unknowns: Vec<UnknownRow>,
    unparsed: Vec<String>,
}

impl FieldCensus {
    /// Take the census of `structs`' fields over `subject`'s product items.
    ///
    /// `declarations` are the indexes whose types, methods, functions and
    /// `Deref` impls the rules may stand on — the subject's own and those of
    /// the packages it uses. A declaration found in none of them is outside
    /// the workspace, and a method on such a type is unresolved unless it is
    /// a standard prelude trait's.
    ///
    /// # Errors
    ///
    /// [`CensusFailure::NoSuchStruct`] when a struct of `structs` is not a
    /// struct `subject` declares.
    pub fn take(
        subject: &Index,
        structs: &[&str],
        declarations: &[&Index],
    ) -> Result<Self, CensusFailure> {
        let read = types::Declarations::read(declarations);
        let package = subject
            .universe()
            .roots()
            .first()
            .map_or_else(String::new, |root| root.id.package.clone());
        let struct_names: BTreeSet<String> =
            structs.iter().map(|name| (*name).to_owned()).collect();
        let mut facts = Vec::new();
        let mut places = BTreeMap::new();
        for name in structs {
            let decls: Vec<&types::TypeDecl> = read
                .types
                .get(*name)
                .map(|all| {
                    all.iter()
                        .filter(|decl| decl.place.0 == package && !decl.is_enum)
                        .collect()
                })
                .unwrap_or_default();
            let modules: BTreeSet<&types::Place> = decls.iter().map(|decl| &decl.place).collect();
            let [place] = modules.into_iter().collect::<Vec<_>>()[..] else {
                return Err(CensusFailure::NoSuchStruct((*name).to_owned()));
            };
            places.insert((*name).to_owned(), place.clone());
            let mut seen = BTreeSet::new();
            for decl in decls {
                for (field, ty) in &decl.fields {
                    if seen.insert(field.clone()) {
                        facts.push(Fact {
                            owner: (*name).to_owned(),
                            name: field.clone(),
                            hub: ty.mentions(&struct_names),
                            ty: ty.clone(),
                        });
                    }
                }
            }
        }
        let by_name = facts
            .iter()
            .enumerate()
            .map(|(at, fact)| ((fact.owner.clone(), fact.name.clone()), at))
            .collect();
        let names = facts.iter().map(|fact| fact.name.clone()).collect();
        let subject_facts = Subject {
            package,
            structs: places,
            facts,
            by_name,
            names,
        };
        let resolver = Resolver {
            declarations: &read,
            subject: &subject_facts,
        };
        let crates: BTreeSet<String> = read.crates.keys().cloned().collect();
        let mut sink = Sink::default();
        let mut unparsed = read.unparsed.clone();
        for item in subject.items() {
            if !item.kind().is_callable() || item.body().is_none() {
                continue;
            }
            let Some(identity) = item
                .identities()
                .find(|identity| identity.variant.permits_product())
            else {
                continue;
            };
            let mut function = match (&identity.type_owner, &identity.trait_name) {
                (Some(owner), Some(trait_name)) => {
                    format!("<{owner} as {trait_name}>::{}", identity.name)
                }
                (Some(owner), None) => format!("{owner}::{}", identity.name),
                (None, Some(trait_name)) => format!("{trait_name}::{}", identity.name),
                (None, None) => identity.name.clone(),
            };
            if !identity.variant.is_unconditional() {
                function.push(' ');
                function.push_str(&identity.variant.to_string());
            }
            if let Err(why) = Walk::item(
                &resolver,
                subject,
                item,
                identity.module_path.clone(),
                function.clone(),
                &crates,
                &mut sink,
            ) {
                unparsed.push(format!("{}::{function}: {why}", identity.module_path));
            }
        }
        Ok(Self::assemble(&subject_facts, sink, unparsed))
    }

    fn assemble(subject: &Subject, sink: Sink, unparsed: Vec<String>) -> Self {
        let rows: Vec<SiteRow> = sink
            .rows
            .into_iter()
            .map(
                |((fact, column, module, function), (kinds, sites))| SiteRow {
                    fact,
                    column,
                    module,
                    function,
                    kinds: kinds.into_iter().collect(),
                    sites,
                },
            )
            .collect();
        let unknowns: Vec<UnknownRow> = sink
            .unknowns
            .into_iter()
            .map(|((site, module, function, reason), sites)| UnknownRow {
                site,
                module,
                function,
                reason,
                sites,
            })
            .collect();
        let facts = subject
            .facts
            .iter()
            .map(|fact| {
                let label = fact.label();
                let named = format!("?.{}", fact.name);
                let of = |column: Column| -> Vec<&SiteRow> {
                    rows.iter()
                        .filter(|row| row.fact == label && row.column == column)
                        .collect()
                };
                let writes = of(Column::Write);
                FactRow {
                    declared: fact.ty.spelling(),
                    hub: fact.hub,
                    writer_modules: writes
                        .iter()
                        .map(|row| &row.module)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    writer_functions: writes.len(),
                    access_functions: of(Column::Access).len(),
                    membership_functions: of(Column::Membership).len(),
                    inner_functions: of(Column::Inner).len(),
                    unknown_sites: unknowns
                        .iter()
                        .filter(|row| row.site == label || row.site == named)
                        .map(|row| row.sites)
                        .sum(),
                    fact: label,
                }
            })
            .collect();
        Self {
            facts,
            rows,
            unknowns,
            unparsed,
        }
    }

    /// Every field of the facts, in declaration order.
    #[must_use]
    pub fn facts(&self) -> &[FactRow] {
        &self.facts
    }

    /// Every fact × column × module × function, sorted.
    #[must_use]
    pub fn rows(&self) -> &[SiteRow] {
        &self.rows
    }

    /// Every unresolved site kind, sorted.
    #[must_use]
    pub fn unknowns(&self) -> &[UnknownRow] {
        &self.unknowns
    }

    /// Declarations and bodies that did not parse. Empty, or the census is
    /// not an answer about them.
    #[must_use]
    pub fn unparsed(&self) -> &[String] {
        &self.unparsed
    }

    /// The inventory: one line per fact.
    #[must_use]
    pub fn render_inventory(&self) -> String {
        let mut out = String::from(INVENTORY_HEADER);
        out.push_str(
            "fact\tdeclared\thub\tproven_writers\twriter_modules\twriter_functions\taccess_functions\tmembership_functions\tinner_functions\tunknown_sites\tstatus\n",
        );
        for fact in &self.facts {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                fact.fact,
                fact.declared,
                if fact.hub { "hub" } else { "-" },
                fact.proven_writers(),
                fact.writer_modules,
                fact.writer_functions,
                fact.access_functions,
                fact.membership_functions,
                fact.inner_functions,
                fact.unknown_sites,
                if fact.complete() {
                    "complete"
                } else {
                    "incomplete"
                },
            ));
        }
        out
    }

    /// The site rows: one line per fact × column × module × function.
    #[must_use]
    pub fn render_sites(&self) -> String {
        let mut out = String::from(SITES_HEADER);
        out.push_str("fact\tcolumn\tmodule\tfunction\tkinds\tsites\n");
        for row in &self.rows {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.fact,
                row.column.name(),
                row.module,
                row.function,
                row.kinds.join(","),
                row.sites
            ));
        }
        out
    }

    /// The unknown list: one line per site kind per item.
    #[must_use]
    pub fn render_unknowns(&self) -> String {
        let mut out = String::from(UNKNOWNS_HEADER);
        out.push_str("site\tmodule\tfunction\treason\tsites\n");
        for row in &self.unknowns {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                row.site, row.module, row.function, row.reason, row.sites
            ));
        }
        out
    }

    /// The facts the annotation file must cover: a proven write in more than
    /// one module.
    #[must_use]
    pub fn multi_writer_facts(&self) -> BTreeSet<String> {
        self.facts
            .iter()
            .filter(|fact| fact.writer_modules > 1)
            .map(|fact| fact.fact.clone())
            .collect()
    }

    /// **The gate**: the inventory and the site rows equal the committed
    /// files, the unknowns are a subset of the committed list, and the
    /// annotation file covers every proven multi-writer fact and nothing else.
    /// Empty means it holds; each entry names the one row that differs.
    #[must_use]
    pub fn judge(&self, committed: &Committed) -> Vec<Difference> {
        let mut differences = Vec::new();
        for (file, now, then) in [
            (
                CommittedFile::Inventory,
                self.render_inventory(),
                committed.inventory.as_str(),
            ),
            (
                CommittedFile::Sites,
                self.render_sites(),
                committed.sites.as_str(),
            ),
        ] {
            let now = data_lines(&now);
            let then = data_lines(then);
            let now_set: BTreeSet<&str> = now.iter().copied().collect();
            let then_set: BTreeSet<&str> = then.iter().copied().collect();
            for line in &now {
                if !then_set.contains(line) {
                    differences.push(Difference::NotCommitted {
                        file,
                        row: (*line).to_owned(),
                    });
                }
            }
            for line in &then {
                if !now_set.contains(line) {
                    differences.push(Difference::NoLongerTrue {
                        file,
                        row: (*line).to_owned(),
                    });
                }
            }
        }
        let allowed: BTreeMap<(String, String, String, String), usize> =
            data_lines(&committed.unknowns)
                .into_iter()
                .filter_map(|line| {
                    let columns: Vec<&str> = line.split('\t').collect();
                    let [site, module, function, reason, sites] = columns.as_slice() else {
                        return None;
                    };
                    Some((
                        (
                            (*site).to_owned(),
                            (*module).to_owned(),
                            (*function).to_owned(),
                            (*reason).to_owned(),
                        ),
                        sites.parse().ok()?,
                    ))
                })
                .collect();
        for row in &self.unknowns {
            let key = (
                row.site.clone(),
                row.module.clone(),
                row.function.clone(),
                row.reason.clone(),
            );
            let committed_sites = allowed.get(&key).copied().unwrap_or(0);
            if row.sites > committed_sites {
                differences.push(Difference::UnknownGrew {
                    row: row.clone(),
                    committed: committed_sites,
                });
            }
        }
        let multi = self.multi_writer_facts();
        let known: BTreeSet<&str> = self.facts.iter().map(|fact| fact.fact.as_str()).collect();
        let mut annotated = BTreeSet::new();
        for line in data_lines(&committed.annotations) {
            let Some(fact) = line.split('\t').next() else {
                continue;
            };
            annotated.insert(fact.to_owned());
            if !multi.contains(fact) {
                differences.push(Difference::StaleAnnotation {
                    fact: fact.to_owned(),
                    why: if known.contains(fact) {
                        "it is not a proven multi-writer fact".to_owned()
                    } else {
                        "it is not a field of the census's structs".to_owned()
                    },
                });
            }
        }
        for fact in multi {
            if !annotated.contains(&fact) {
                differences.push(Difference::MissingAnnotation { fact });
            }
        }
        differences
    }

    /// The unknown list's render, **only when it does not grow** against the
    /// committed one — what the copier may write back. A grown list is never
    /// rendered for copying: an unknown is added by hand, deliberately, or
    /// resolved.
    #[must_use]
    pub fn render_unknowns_if_not_grown(&self, committed_unknowns: &str) -> Option<String> {
        let committed = Committed {
            inventory: String::new(),
            sites: String::new(),
            unknowns: committed_unknowns.to_owned(),
            annotations: String::new(),
        };
        let grew = self
            .judge(&committed)
            .iter()
            .any(|difference| matches!(difference, Difference::UnknownGrew { .. }));
        (!grew).then(|| self.render_unknowns())
    }
}

const INVENTORY_HEADER: &str = "\
# The field inventory of the ownership census (census-1): one row per field of
# App, WindowRuntime, TabState and LeafSession. Generated by bt_source::FieldCensus
# (crates/bt-source/tests/census.rs); do not edit — run
# scripts/generate-ownership-census.ps1. proven_writers is 0, 1 or n distinct
# modules with a proven write; status is incomplete when any unknown site names
# the field. Access, membership and inner mutability are counted apart and are
# never writes.
";

const SITES_HEADER: &str = "\
# The site rows of the ownership census (census-1): fact x column x module x
# function. Generated by bt_source::FieldCensus; do not edit — run
# scripts/generate-ownership-census.ps1. column is write (proven), access
# (mutable access or escape), membership (a hub's contents) or inner (through a
# shared reference). module is the declaring item's module path.
";

const UNKNOWNS_HEADER: &str = "\
# The unknown sites of the ownership census (census-1): each write-shaped site
# the rules could not resolve, with its item and the reason. Shrink-only: the
# gate refuses a site this list does not carry. A resolved site's row is
# removed by scripts/generate-ownership-census.ps1; a row is added by hand
# only, and only with a reason.
";

/// The lines that are rows: not blank, not a comment, not the column header.
fn data_lines(text: &str) -> Vec<&str> {
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    // The first remaining line is the column header.
    lines.next();
    lines.collect()
}

/// The committed files, as text.
#[derive(Clone, Debug, Default)]
pub struct Committed {
    pub inventory: String,
    pub sites: String,
    pub unknowns: String,
    pub annotations: String,
}

/// Which committed file a difference is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommittedFile {
    Inventory,
    Sites,
}

impl fmt::Display for CommittedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Inventory => "the inventory",
            Self::Sites => "the site rows",
        })
    }
}

/// One way the code and the committed census disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    /// The code says this row and the committed file does not.
    NotCommitted { file: CommittedFile, row: String },
    /// The committed file says this row and the code no longer does.
    NoLongerTrue { file: CommittedFile, row: String },
    /// An unknown site the committed list does not carry, or carries fewer
    /// of.
    UnknownGrew { row: UnknownRow, committed: usize },
    /// A proven multi-writer fact with no annotation row.
    MissingAnnotation { fact: String },
    /// An annotation row for a fact that is not a proven multi-writer fact.
    StaleAnnotation { fact: String, why: String },
}

impl fmt::Display for Difference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCommitted { file, row } => {
                write!(
                    formatter,
                    "{file} does not carry a row the code says: {row}"
                )
            }
            Self::NoLongerTrue { file, row } => {
                write!(
                    formatter,
                    "{file} carries a row the code no longer says: {row}"
                )
            }
            Self::UnknownGrew { row, committed } => write!(
                formatter,
                "the unknown list grew: {} in {}::{} — {} ({} site(s), {committed} committed)",
                row.site, row.module, row.function, row.reason, row.sites
            ),
            Self::MissingAnnotation { fact } => write!(
                formatter,
                "{fact} has proven writers in more than one module and no annotation row"
            ),
            Self::StaleAnnotation { fact, why } => {
                write!(formatter, "the annotation row for {fact} is stale: {why}")
            }
        }
    }
}
