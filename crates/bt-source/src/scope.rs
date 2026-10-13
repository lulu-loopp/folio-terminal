//! The file-scoped allowlist — the second of the two lists of the plan's §6.1.
//!
//! Almost every source reader in this workspace names a file because of how it
//! was written, not because of what it is about: it wanted a fact about an item
//! and reached for the file the item happens to be in today. Those readers are
//! migration debt, they are listed in `docs/plans/MIGRATION-DEBT.tsv`, and that
//! list only shrinks.
//!
//! A very few readers are different. Their subject **is** a file — a named
//! document, a twin that has to read a named array on a tree that does not
//! compile, the scan that walks the workspace looking for the other kind. Those
//! do not become debt by being written down, and they will still be here at
//! P20. They are this enum, one variant each, and the reason is in the variant's
//! own doc comment rather than in a table somewhere else.
//!
//! [`Scope`] has one variant and it carries a [`FileScoped`], so a scope naming
//! a file cannot be built without naming which of these it is. That is the whole
//! of the enforcement: there is no constructor that takes a path.
//!
//! **This list only shrinks too**, and growth needs a written reason — the doc
//! comment is that reason, and a variant the scan never hits is a variant to
//! delete (`crates/bt-source/tests/tripwire.rs` asserts each one is still
//! reached).
//!
//! # What was considered and refused
//!
//! The plan (§7.2) suggests a doc test over `docs/` as a candidate, and
//! `bt_app::diagnostics::bt_environment_doc_tests` is the one in this tree:
//! `include_str!("../../../docs/BT-ENVIRONMENT.md")`, whose subject really is a
//! published document. It is **not** here. The tripwire looks for `.rs`, so an
//! entry for it would enforce nothing — and it would be keyed to
//! `crates/bt-app/src/diagnostics.rs`, a file whose other readers are source
//! readers; allowlisting it would blind the tripwire to them. The source half
//! of that test reads [`crate::universes::shipped_program`], and the document
//! is safe because nothing moves it, not because a list says so.
//!
//! The vendor compatibility seam in `bt-term` reads as if its concern were the
//! two files `adapter.rs` and `cell_capture.rs`. It is not: the concern is the
//! two *modules*, and `bt_term::adapter_boundary_tests` names them by their
//! paths, each with its whole tree, so a submodule either one gains is covered
//! the day it is declared.

/// A reader whose concern really is a file.
///
/// One variant per reader, and the reason it is not debt is the doc comment on
/// the variant. [`path`](Self::path) is where the reader lives, written from the
/// workspace root with forward slashes, which is the key the tripwire matches
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileScoped {
    /// `scripts/check-vendor-notices.ps1`, the `added` list naming
    /// `src/depth.rs` inside `vendor/mitex-parser`.
    ///
    /// The fact it holds is "these files in this vendored copy are not
    /// upstream's", which is a fact about files in the only sense there is one.
    /// It is also about a tree this workspace's declarations do not describe:
    /// `bt-source` enumerates what the `mod` declarations of *this* program
    /// build, and Apache-2.0 §4(b) asks a question about a directory somebody
    /// else wrote. There is no item to ask about.
    VendoredAddedFiles,
    /// `crates/bt-source/tests/tripwire.rs`, the scan itself.
    ///
    /// It walks the workspace and picks files out by extension, because files
    /// are its whole subject: it is the guard that catches a *new* reader bound
    /// to a file. It pins no fact about any item, and it is written without this
    /// crate's query layer on purpose (§6.1), so that a bug in the mechanism
    /// cannot disable the guard against the mechanism — which also means it
    /// cannot ask a universe for its files.
    TheTripwireItself,
    /// `scripts/ci/check-dead-code.ps1`, the dead-code gate, naming the file
    /// names Rust's module rules give meaning to (`lib.rs`, `main.rs`,
    /// `mod.rs`, `tests.rs`, `*_tests.rs`, `<name>.rs`).
    ///
    /// Which `.rs` file is product code and which is test code is decided by
    /// those names and by the `mod` declarations that reach a file: the gate
    /// walks every crate's `src/`, reads each file, and resolves out-of-line
    /// modules to files the way the compiler does, so files are its subject.
    /// It is a PowerShell gate and cannot ask this crate's index.
    TheDeadCodeGate,
    /// `scripts/ci/check-lint-probe.ps1`, naming the lint probe's generated
    /// source `crates/bt-lint-probe/src/lib.rs`.
    ///
    /// The fact it holds is "the committed file is byte for byte what the
    /// generator writes from the registry": it hands the path to the generator
    /// as its output and to `git` to compare and to require it tracked. A
    /// generated file's equality with its generator is a fact about the file;
    /// there is no item to ask about.
    TheLintProbeIsGenerated,
    /// `scripts/ci/check-embedded-spaces.ps1`, the embedded-spaces gate,
    /// asking `git ls-files` for `"crates/*.rs"`.
    ///
    /// Its subject is the source text of every first-party `.rs` file: it lexes
    /// each one for string literals that carry a run of spaces, a fact about
    /// how the literal is written, by file and line, which is the key of its
    /// list. The pattern stays quoted so that `git` expands it, not the shell.
    /// It is a PowerShell gate and cannot ask this crate's index.
    TheEmbeddedSpacesGate,
}

impl FileScoped {
    /// Every entry. The allowlist is small enough to be an array and only shrinks.
    pub const ALL: [Self; 5] = [
        Self::VendoredAddedFiles,
        Self::TheTripwireItself,
        Self::TheDeadCodeGate,
        Self::TheLintProbeIsGenerated,
        Self::TheEmbeddedSpacesGate,
    ];

    /// Where the reader lives, from the workspace root, with forward slashes.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::VendoredAddedFiles => "scripts/check-vendor-notices.ps1",
            Self::TheTripwireItself => "crates/bt-source/tests/tripwire.rs",
            Self::TheDeadCodeGate => "scripts/ci/check-dead-code.ps1",
            Self::TheLintProbeIsGenerated => "scripts/ci/check-lint-probe.ps1",
            Self::TheEmbeddedSpacesGate => "scripts/ci/check-embedded-spaces.ps1",
        }
    }

    /// One line of why this reader's concern really is a file, for a message
    /// somebody reads at three in the morning. The whole reason is the variant's
    /// doc comment.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::VendoredAddedFiles => {
                "which files of a vendored copy are not upstream's is a fact about files, in a \
                 tree this workspace's declarations do not describe"
            }
            Self::TheTripwireItself => {
                "the scan that catches the other kind; files are its subject, and it is written \
                 without this crate's query layer on purpose"
            }
            Self::TheDeadCodeGate => {
                "which file is product code is decided by Rust's module file names and the \
                 declarations reaching a file; a PowerShell gate that walks the files themselves"
            }
            Self::TheLintProbeIsGenerated => {
                "the committed probe source must be byte for byte its generator's output, a fact \
                 about a generated file"
            }
            Self::TheEmbeddedSpacesGate => {
                "how every first-party string literal is written is read from the files \
                 themselves, by file and line; a PowerShell gate that lexes them"
            }
        }
    }
}

// What a reading is asked about lives on [`crate::Scope`], which P1c owns and
// which carries this list's file variant beside `Scope::Item` and
// `Scope::Module`. P2 was written when that enum was not yet on `main` and
// declared one of its own here; the two merged apart and the crate did not
// compile, so the variant moved to where both tickets said it belonged and this
// module kept the list itself.

// The tests for this module are in `crates/bt-source/tests/tripwire.rs` and not
// here, for a reason worth stating: asserting anything about this type means
// *constructing* one, and a construction is the third thing the tripwire's scan
// looks for. Written here it would be a hit in a file on neither list; written
// there it is a hit in the one file the allowlist already carries, beside the
// scan that finds it. The two are one subject.
