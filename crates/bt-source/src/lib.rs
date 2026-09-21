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
//!
//! What it deliberately does **not** build: the immutable index of §5 (P1b), and
//! the views, needles and identity-with-variants of §2 (P1c). Every type here is
//! owned, plain data with no parser type in its public API, so P1b can lower it.
//!
//! **The reading is `cfg`-blind on purpose.** Every declaration is followed
//! whatever stands on it, and the host platform never selects: a file reached
//! only under `cfg(windows)` is enumerated on macOS too, because a guard that
//! quietly read a different crate on each runner would make the two CI platforms
//! disagree about what it means. The one predicate that *is* evaluated is `test`,
//! three-valued, and it decides [`Compilation`] and nothing else.

mod declarations;
mod enumerate;
mod manifest;
mod paths;
mod reject;
mod universe;
pub mod universes;

pub use declarations::{Compilation, DeclarationStep, ModuleBody, ReachedModule};
pub use enumerate::{Enumeration, FileFacts, FileOwner, FileSetDiff, enumerate};
pub use manifest::{Package, TargetId, TargetKind, TargetRoot, Workspace};
pub use paths::{is_inside, normalized};
pub use reject::{Position, Rejection, report};
pub use universe::{DiskScope, Universe, Vendor, is_vendored, targets_of};
