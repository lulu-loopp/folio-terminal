//! What this crate refuses to guess at.
//!
//! Both module resolvers in the tree today answer a question they cannot answer:
//! `bt_platform::…::module_file` returns the `name.rs` form when it exists and
//! never asks whether `name/mod.rs` exists as well, and
//! `bt_app::file_reads_source_tests::scan` does the same and additionally drops
//! a declaration it cannot follow. Either behaviour turns "I do not know which
//! file this is" into a smaller, quieter universe — which is the exact defect
//! `docs/plans/bt-app-split-prep.md` exists to remove. Here both are rejections,
//! and a rejection names every candidate it saw.
//!
//! `UNREACHED` is **not** here. A file on the disk that no declaration reaches is
//! a finding a caller asserts on ([`crate::FileSetDiff`]), not a failure: the
//! walk did its job and the answer is that the file is not part of the crate.

use std::fmt;
use std::path::PathBuf;

/// Where a declaration is written, one-based, as the parser reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Position {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

/// Every way this crate declines to answer.
///
/// One enum rather than a `String`, because a caller — and P3's file-set diff
/// above all — has to be able to say *which* refusal it is looking at without
/// matching on prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Two files could hold one module. Rust rejects this and so does the walk;
    /// picking one is how a universe silently becomes the wrong universe.
    AmbiguousModule {
        declared_in: PathBuf,
        at: Position,
        module: String,
        candidates: Vec<PathBuf>,
    },
    /// A declaration names a module no file holds. Every place the walk looked
    /// is listed, because the usual cause is that it looked in the wrong
    /// directory — which is what inline ancestry decides.
    UnresolvedModule {
        declared_in: PathBuf,
        at: Position,
        module: String,
        tried: Vec<PathBuf>,
    },
    /// A declaration reaches a file that is already one of its own ancestors.
    /// `rustc` calls this a recursive module; here it is the one thing that
    /// would otherwise make the walk run forever.
    ModuleCycle {
        declared_in: PathBuf,
        at: Position,
        module: String,
        file: PathBuf,
    },
    /// A file the walk reached could not be read as text.
    UnreadableFile { file: PathBuf, reason: String },
    /// A file the walk reached is not Rust the parser accepts. Never skipped:
    /// a file that does not parse is a file whose declarations are unknown, and
    /// an unknown declaration is a missing subtree.
    UnparsableFile { file: PathBuf, reason: String },
    /// A universe names a compilation root that is not there.
    MissingTargetRoot {
        package: String,
        target: String,
        file: PathBuf,
    },
    /// A universe names a disk scope that is not a directory.
    MissingDiskScope { root: PathBuf },
    /// A universe reaches into `vendor/` without saying so. Upstream code is
    /// held to upstream's choices — the workspace lint table says so in its own
    /// words — and whether a reading covers it is a decision, never a default.
    VendorNotDeclared { path: PathBuf },
    /// A manifest this reader does not understand. It reads the small part of
    /// the manifest format cargo's own target discovery needs and refuses the
    /// rest rather than guessing at it.
    Manifest {
        file: PathBuf,
        line: usize,
        reason: String,
    },
    /// A universe names a package the workspace does not have.
    NoSuchPackage { name: String },
    /// The union of a universe's file texts does not fit the `u32` offsets the
    /// index is built on. Four gigabytes of Rust is not a universe anybody in
    /// this workspace declares, and a wrapped offset would be a quietly wrong
    /// answer rather than a refusal.
    UnionTooLarge { bytes: usize },
}

impl fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AmbiguousModule {
                declared_in,
                at,
                module,
                candidates,
            } => write!(
                formatter,
                "{}:{at}: `mod {module};` could be held by {} files, and this walk will not pick \
                 one: {candidates:#?}",
                declared_in.display(),
                candidates.len()
            ),
            Self::UnresolvedModule {
                declared_in,
                at,
                module,
                tried,
            } => write!(
                formatter,
                "{}:{at}: `mod {module};` names no file; looked at {tried:#?}",
                declared_in.display()
            ),
            Self::ModuleCycle {
                declared_in,
                at,
                module,
                file,
            } => write!(
                formatter,
                "{}:{at}: `mod {module};` reaches {}, which is already one of its own ancestors",
                declared_in.display(),
                file.display()
            ),
            Self::UnreadableFile { file, reason } => {
                write!(formatter, "{}: cannot be read: {reason}", file.display())
            }
            Self::UnparsableFile { file, reason } => {
                write!(formatter, "{}: cannot be parsed: {reason}", file.display())
            }
            Self::MissingTargetRoot {
                package,
                target,
                file,
            } => write!(
                formatter,
                "{package}'s target `{target}` has no root at {}",
                file.display()
            ),
            Self::MissingDiskScope { root } => {
                write!(formatter, "{} is not a directory", root.display())
            }
            Self::VendorNotDeclared { path } => write!(
                formatter,
                "{} is under `vendor/` and this universe does not say it includes vendored code",
                path.display()
            ),
            Self::Manifest { file, line, reason } => {
                write!(formatter, "{}:{line}: {reason}", file.display())
            }
            Self::NoSuchPackage { name } => {
                write!(formatter, "`{name}` is not a package of this workspace")
            }
            Self::UnionTooLarge { bytes } => write!(
                formatter,
                "this universe's files are {bytes} bytes together, which does not fit the \
                 index's 32-bit offsets"
            ),
        }
    }
}

impl std::error::Error for Rejection {}

/// The message a caller that has no better idea should print.
///
/// A `Vec<Rejection>` in an assertion message is a wall of `Debug`; this is the
/// same information in the order the walk found it, one refusal a line.
#[must_use]
pub fn report(rejections: &[Rejection]) -> String {
    rejections
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}
