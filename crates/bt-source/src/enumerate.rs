//! Running a universe: what the declarations reach, and what the disk holds.
//!
//! The two are separate answers and the difference between them is the finding.
//! §3.2 of the plan asks every migrated walker to ship a before/after file-set
//! comparison — "not a count, the sorted list of paths, old and new" — and
//! [`FileSetDiff`] is that comparison's shape. P3 is its first consumer.
//!
//! A file on the disk that no declaration reaches is `UNREACHED`. It is a value
//! and not a failure: the walk did its job, and the answer is that the file is
//! not part of any compilation this universe names. Whether that is a repair, a
//! widening or a bug belongs to the ticket that reads it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::declarations::{Compilation, DeclarationStep, ModuleBody, ReachedModule, walk_target};
use crate::manifest::TargetId;
use crate::reject::Rejection;
use crate::universe::Universe;

/// One way a file is reached: which target, under which module path, through
/// which declarations, and whether that path permits product compilation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileOwner {
    pub target: TargetId,
    pub module_path: String,
    pub steps: Vec<DeclarationStep>,
    pub compilation: Compilation,
}

/// Everything the enumeration knows about one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFacts {
    path: PathBuf,
    owners: Vec<FileOwner>,
}

impl FileFacts {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every path by which a declaration reaches this file, sorted.
    #[must_use]
    pub fn owners(&self) -> &[FileOwner] {
        &self.owners
    }

    /// Whether **any** owning path permits product compilation (plan §2.3).
    ///
    /// Any, not all. A file reached through a `#[cfg(test)] mod` and also
    /// through an ordinary one is product code; a reading that let the test
    /// declaration win would take the file out of every guard that skips test
    /// code, which is the quiet way a prohibition stops prohibiting.
    #[must_use]
    pub fn permits_product(&self) -> bool {
        self.owners
            .iter()
            .any(|owner| owner.compilation.permits_product())
    }

    /// Whether every path to this file passes a test-gated declaration.
    #[must_use]
    pub fn is_wholly_test(&self) -> bool {
        !self.owners.is_empty() && !self.permits_product()
    }
}

/// The declared set beside the set on the disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSetDiff {
    /// Reached by a declaration, not found by the disk walk — usually a file
    /// outside every disk scope, which is what a `#[path]` out of the tree or a
    /// target root under `tests/` looks like.
    pub only_declared: BTreeSet<PathBuf>,
    /// On the disk and reached by nothing. This is `UNREACHED`.
    pub only_on_disk: BTreeSet<PathBuf>,
    pub in_both: BTreeSet<PathBuf>,
}

impl FileSetDiff {
    /// Whether the two readings name the same files.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.only_declared.is_empty() && self.only_on_disk.is_empty()
    }

    /// The sorted paths on each side, one per line, for an assertion message.
    #[must_use]
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        for path in &self.only_declared {
            lines.push(format!("declared, not on disk: {}", path.display()));
        }
        for path in &self.only_on_disk {
            lines.push(format!("UNREACHED: {}", path.display()));
        }
        lines.join("\n")
    }
}

/// **The files on the disk that no declaration reaches, handed to the caller in
/// a shape that makes looking away visible.**
///
/// `UNREACHED` is a value and not a failure (see this module's header), and the
/// P1a review's finding was that nothing made a consumer look at it: the
/// enumeration answered `files()`, `modules()` and the rest without the set ever
/// being read, while §3.2 puts the obligation on a ticket. So [`enumerate`]
/// hands it back *beside* the enumeration, `#[must_use]`, and the only ways to
/// spend it name what is being done — asserted empty, carried somewhere else, or
/// read.
///
/// **The limit, said plainly.** Rust cannot make a caller *look*; `let (e, _) =`
/// discards this as quietly as anything else. What it can do is make the
/// discarding a written act a reviewer greps for, rather than the absence of a
/// call nobody notices.
#[must_use = "UNREACHED is the evidence §3.2 asks every migrated walker to ship: assert it empty, \
              carry it, or say in the ticket why it is not read"]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unreached {
    paths: BTreeSet<PathBuf>,
}

impl Unreached {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    #[must_use]
    pub fn paths(&self) -> &BTreeSet<PathBuf> {
        &self.paths
    }

    /// One path a line, for an assertion message.
    #[must_use]
    pub fn report(&self) -> String {
        self.paths
            .iter()
            .map(|path| format!("UNREACHED: {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Spend it by requiring that it is empty.
    ///
    /// # Panics
    ///
    /// Naming every file no declaration reaches.
    pub fn expect_none(self, reason: &str) {
        assert!(self.paths.is_empty(), "{reason}\n{}", self.report());
    }

    /// Spend it by taking the set somewhere a later reader asserts on it — the
    /// index carries it into [`crate::Index::cross_check`] this way.
    #[must_use]
    pub fn carried_forward(self) -> BTreeSet<PathBuf> {
        self.paths
    }
}

/// A universe, run.
#[derive(Clone, Debug)]
pub struct Enumeration {
    universe: Universe,
    modules: Vec<ReachedModule>,
    files: BTreeMap<PathBuf, FileFacts>,
    diff: FileSetDiff,
}

impl Enumeration {
    #[must_use]
    pub fn universe(&self) -> &Universe {
        &self.universe
    }

    /// Every module reached, in the order the walk reached it.
    #[must_use]
    pub fn modules(&self) -> &[ReachedModule] {
        &self.modules
    }

    /// Every file reached, by path.
    #[must_use]
    pub fn files(&self) -> &BTreeMap<PathBuf, FileFacts> {
        &self.files
    }

    #[must_use]
    pub fn file(&self, path: &Path) -> Option<&FileFacts> {
        self.files.get(&crate::paths::normalized(path))
    }

    /// Every file reached through nothing but test-gated declarations.
    #[must_use]
    pub fn wholly_test_files(&self) -> BTreeSet<PathBuf> {
        self.files
            .values()
            .filter(|facts| facts.is_wholly_test())
            .map(|facts| facts.path.clone())
            .collect()
    }

    /// Every file some path reaches without passing a test gate.
    #[must_use]
    pub fn product_reachable_files(&self) -> BTreeSet<PathBuf> {
        self.files
            .values()
            .filter(|facts| facts.permits_product())
            .map(|facts| facts.path.clone())
            .collect()
    }

    /// The declared set beside the disk set.
    ///
    /// The `UNREACHED` half of it is also handed back on its own by
    /// [`enumerate`], in a [`Unreached`] the caller has to spend.
    #[must_use]
    pub fn cross_check(&self) -> &FileSetDiff {
        &self.diff
    }
}

/// Run `universe`: walk its target roots, walk its disk scopes, and compare.
///
/// The second half of the answer is [`Unreached`], which is `#[must_use]`: the
/// files on the disk no declaration reaches are the evidence §3.2 asks for, and
/// P1a's review found nothing made a consumer take them.
///
/// # Errors
///
/// Every refusal the walk made, in the order it made them. They are returned
/// together rather than one at a time because a tree with three broken
/// declarations should say so once.
pub fn enumerate(universe: &Universe) -> Result<(Enumeration, Unreached), Vec<Rejection>> {
    let mut modules = Vec::new();
    let mut rejections = Vec::new();
    for root in universe.roots() {
        let (reached, refused) = walk_target(root);
        modules.extend(reached);
        rejections.extend(refused);
    }

    let mut files: BTreeMap<PathBuf, FileFacts> = BTreeMap::new();
    for module in &modules {
        // An inline module adds no file of its own: its bytes are in the file
        // that declares it, and that file is already here through the
        // declaration that reached it.
        let ModuleBody::File(file) = &module.body else {
            continue;
        };
        files
            .entry(file.clone())
            .or_insert_with(|| FileFacts {
                path: file.clone(),
                owners: Vec::new(),
            })
            .owners
            .push(FileOwner {
                target: module.target.clone(),
                module_path: module.module_path.clone(),
                steps: module.steps.clone(),
                compilation: module.compilation,
            });
    }
    for facts in files.values_mut() {
        facts.owners.sort();
        facts.owners.dedup();
    }

    let on_disk = match universe.disk_files() {
        Ok(found) => found,
        Err(refused) => {
            rejections.extend(refused);
            BTreeSet::new()
        }
    };
    if !rejections.is_empty() {
        return Err(rejections);
    }

    let declared: BTreeSet<PathBuf> = files.keys().cloned().collect();
    let diff = FileSetDiff {
        only_declared: declared.difference(&on_disk).cloned().collect(),
        only_on_disk: on_disk.difference(&declared).cloned().collect(),
        in_both: declared.intersection(&on_disk).cloned().collect(),
    };

    let unreached = Unreached {
        paths: diff.only_on_disk.clone(),
    };
    Ok((
        Enumeration {
            universe: universe.clone(),
            modules,
            files,
            diff,
        },
        unreached,
    ))
}
