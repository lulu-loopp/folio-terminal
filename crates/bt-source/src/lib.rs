//! **A crate's source, enumerated by its declarations** — the first piece of
//! `docs/plans/bt-app-split-prep.md` (§6.2, ticket P1a).
//!
//! Hundreds of guards in this workspace pin facts about the program by reading
//! its own source text, and every one of them is bound to a *file*. When an item
//! moves from `main.rs` to a file beside it, a reader bound to `main.rs` does not
//! go red: it reads a smaller universe and stays green. That is the failure this
//! crate exists to remove. A reader asks a **universe** — declared, not derived —
//! which files a crate is made of, and the answer comes from the `mod`
//! declarations that actually build the crate rather than from a directory walk
//! that happens to agree with them today.
//!
//! What P1a builds, and deliberately no more:
//!
//! * [`Universe`] — the four knobs of the plan's §3.1 as a value: target roots,
//!   disk scopes with their exclusions, and whether `vendor/` is in. There is no
//!   default universe, because the tree has four different ones and no query is
//!   right under the wrong one.
//! * [`enumerate`] — the walk from a target root through `mod x;`, `#[path]` and
//!   inline `mod x { … }`, keeping the **inline ancestry** that decides where a
//!   nested declaration's file lives, and recording for every module the full
//!   ownership path with each declaration's `cfg` predicate.
//! * [`Rejection`] — ambiguity and non-resolution are hard failures here. Both
//!   resolvers in the tree today take the first candidate that exists and skip a
//!   declaration they cannot follow, which is how a walk loses a file silently.
//! * The disk cross-check, [`FileSetDiff`], which is the evidence §3.2 asks every
//!   migrated walker to ship: not a count, the sorted paths on each side.
//! * [`FileScoped`] (P2) — the **file-scoped allowlist** of §6.1, the permanent
//!   list beside the temporary one in `docs/plans/MIGRATION-DEBT.tsv`. A reader
//!   whose concern really is a file says which of three it is; [`Scope`] cannot
//!   name a file any other way. `crates/bt-source/tests/tripwire.rs` is what
//!   holds every reader in the workspace to one of the two lists.
//!
//! What P1b adds on top (§5, "the index, not the trees"):
//!
//! * [`Index`] — the enumeration, lowered into owned, plain, immutable data:
//!   the union of every file's text, numeric spans into it, item identities with
//!   their body spans, the ownership paths with their `cfg` spellings, the
//!   identifier view's tokens, the literals with their decoded values, and the
//!   comment masks. **Every `syn` and `proc_macro2` object is dropped before it
//!   is published**, which is what makes it [`Send`] and [`Sync`] and therefore
//!   shareable by every thread of a process.
//! * [`Index::shared`] — one index per process per universe, so the cost of the
//!   lowering is paid once however many readers ask.
//! * The queries the measurement of §5 needs, and only those: [`Index::body_of`],
//!   [`Index::count_identifier`], [`Index::contains`] and [`Index::owners_of`],
//!   each refusing loudly rather than answering a smaller question.
//!
//! What it deliberately does **not** build: the four views' full contract, needle
//! provenance, named scopes and the lexical macro traversal of §2 (P1c).
//!
//! **The reading is `cfg`-blind on purpose.** Every declaration is followed
//! whatever stands on it, and the host platform never selects: a file reached
//! only under `cfg(windows)` is enumerated on macOS too, because a guard that
//! quietly read a different crate on each runner would make the two CI platforms
//! disagree about what it means. The one predicate that *is* evaluated is `test`,
//! three-valued, and it decides [`Compilation`] and nothing else.

mod cache;
mod declarations;
mod enumerate;
mod index;
mod lower;
mod manifest;
mod paths;
mod query;
mod reject;
mod scope;
mod universe;
pub mod universes;

pub use declarations::{Compilation, DeclarationStep, ModuleBody, ReachedModule};
pub use enumerate::{Enumeration, FileFacts, FileOwner, FileSetDiff, enumerate};
pub use index::{
    CommentKind, CommentRecord, ConditionalVariant, FileRecord, Index, ItemIdentity, ItemKind,
    ItemRecord, LiteralRecord, LiteralValue, Location, MacroShape, Span, TokenKind, TokenRecord,
    UnsupportedMacroShape,
};
pub use manifest::{Package, TargetId, TargetKind, TargetRoot, Workspace};
pub use paths::{is_inside, normalized};
pub use query::{Candidate, ItemQuery, QueryFailure, View};
pub use reject::{Position, Rejection, report};
pub use scope::{FileScoped, Scope};
pub use universe::{DiskScope, Universe, Vendor, is_vendored, targets_of};
