//! One index per process per universe, and nothing else kept.
//!
//! §5's arithmetic is the reason this exists: the test harness runs many
//! processes, and within a process many threads, and a lowering that ran once
//! per *thread* would multiply a six-second build by the thread count for no
//! gain at all. The index is [`Send`] and [`Sync`] precisely so that one of them
//! can serve all of them.
//!
//! **What the cache holds is the lowered index and the universe that keys it.**
//! No parse tree, no file handle, no builder. A universe is its own key because
//! it is a value: two readers that declare the same roots, scopes and vendor
//! answer are asking the same question, however each of them wrote it down.
//!
//! The map's lock is never held across a build. The entry taken under it is an
//! empty [`OnceLock`]; the build happens after the lock is dropped, so a second
//! thread asking for a *different* universe does not queue behind it, and a
//! second thread asking for the *same* one waits and gets the same object rather
//! than building a duplicate.
//!
//! **A second map is keyed by package name**, for [`Index::of_package`]: the
//! universe a consumer batch wants is almost always "this package's own `src/`",
//! and the fifteen lines that read the workspace and declare it are the same
//! fifteen lines in every batch. That map holds `&'static Index` and the
//! universe map holds the index itself, so the two are one object and not two.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use crate::index::Index;
use crate::manifest::Workspace;
use crate::reject::{Rejection, report};
use crate::universe::{Universe, Vendor};
use crate::universes;

/// The answer for one universe, computed at most once.
type Slot = Arc<OnceLock<Result<Arc<Index>, Vec<Rejection>>>>;

static INDEXES: LazyLock<Mutex<HashMap<Universe, Slot>>> = LazyLock::new(Mutex::default);

pub(crate) fn shared(universe: &Universe) -> Result<Arc<Index>, Vec<Rejection>> {
    let slot = {
        // A poisoned map is a map some other test panicked beside, not a map
        // with a half-written entry in it: every value is either an untouched
        // `OnceLock` or a finished answer.
        let mut held = INDEXES.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(held.entry(universe.clone()).or_default())
    };
    slot.get_or_init(|| Index::build(universe).map(Arc::new))
        .clone()
}

/// The reference one package's readers share, taken at most once.
type PackageSlot = Arc<OnceLock<&'static Index>>;

static PACKAGES: LazyLock<Mutex<HashMap<String, PackageSlot>>> = LazyLock::new(Mutex::default);

pub(crate) fn of_package(package: &str) -> &'static Index {
    let slot = {
        let mut held = PACKAGES.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(held.entry(package.to_owned()).or_default())
    };
    // The lock is dropped before the lowering starts, for `shared`'s reason
    // above: two packages asked for at once are lowered at once, and two
    // threads asking for one package share the one answer.
    slot.get_or_init(|| lower_package(package))
}

/// Read the workspace, declare `package`'s own `src/`, lower it, and hand back
/// a reference that outlives every caller.
///
/// **Each of the four steps refuses out loud rather than answering a smaller
/// question.** A caller in a test wants the name of the package it asked for
/// and the refusal that answered, on the one line a harness prints; a `Result`
/// here would put that `unwrap_or_else` back in every consumer batch, which is
/// the fifteen lines this entry exists to remove.
fn lower_package(package: &str) -> &'static Index {
    let root = workspace_root();
    let workspace = Workspace::read(&root)
        .unwrap_or_else(|rejection| panic!("the workspace at {}: {rejection}", root.display()));
    let member = workspace
        .package(package)
        .unwrap_or_else(|rejection| panic!("{rejection}"));
    let universe = universes::crate_sources(member, Vendor::Excluded)
        .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let index = shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    // **The `Arc` is leaked, which is what makes the reference `'static`
    // without an `unsafe` word.** One pointer per package per process, holding
    // alive an index the universe map holds anyway; what a caller gets back is
    // that same index and never a copy of it. The reference has to be `'static`
    // because the answers a reader keeps — a body's `&str`, a span's text — are
    // borrowed out of the index and outlive the call that asked for them.
    let held: &'static Arc<Index> = Box::leak(Box::new(index));
    held
}

/// The workspace this crate is a member of.
///
/// Taken from **this** crate's manifest directory and not the caller's: the
/// workspace manifest's member list puts `bt-source` at `crates/bt-source`, so
/// the root is two levels up, and every consumer in another package would
/// otherwise have to count the levels between its own manifest and the root
/// again.
///
/// Deliberately not `canonicalize`, for [`crate::paths::normalized`]'s reason:
/// on Windows it returns a `\\?\` verbatim path, which no other path in this
/// crate carries. The `..` components are resolved by `Workspace::read`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
