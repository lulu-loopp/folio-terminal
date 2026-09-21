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
//! `crates/bt-app/src/diagnostics.rs`, which also holds a `shipped_sources`
//! directory walk that is genuine debt (P13). Allowlisting that file would blind
//! the tripwire to the debt beside the document. The document is safe because
//! nothing moves it, not because a list says so.
//!
//! `scripts/check-adapter-boundary.ps1` names `adapter.rs` and `cell_capture.rs`
//! as the vendor compatibility seam, and it reads as if the concern were those
//! two files. It is not: the concern is the adapter *module*, and the day either
//! file gains a submodule the gate stops covering it without a word. It is on
//! the debt list, at P17.

/// A reader whose concern really is a file.
///
/// One variant per reader, and the reason it is not debt is the doc comment on
/// the variant. [`path`](Self::path) is where the reader lives, written from the
/// workspace root with forward slashes, which is the key the tripwire matches
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileScoped {
    /// `scripts/check-portable-core.ps1`, `Read-RustStringArray` reading
    /// `FILES_THAT_MAY_NAME_A_PLATFORM` out of `crates/bt-app/src/main.rs`.
    ///
    /// The plan names this entry 1 (§6.3, P10) and gives three reasons, each of
    /// which is about a file rather than about an item. The array is declared
    /// beside `fn main`, which can never leave `main.rs`, so the file this
    /// reader names is fixed by the language. The script is the one gate that
    /// answers in five seconds on a tree that does not compile, which is what it
    /// is for — a reader that asked `bt-source` would need the workspace to
    /// build first. And the array's own entries are file names: what it pins is
    /// which *files* may decide what platform this is, so a reading that stopped
    /// naming files would stop being the rule.
    ///
    /// P10 adds `the_gate_and_its_script_walk_the_same_files` beside it; the
    /// array reader itself is unchanged, then and at P20.
    PortableCoreArray,
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
}

impl FileScoped {
    /// Every entry. The allowlist is small enough to be an array and is meant to
    /// stay that way — the plan's §7.2 end state is at most four.
    pub const ALL: [Self; 3] = [
        Self::PortableCoreArray,
        Self::VendoredAddedFiles,
        Self::TheTripwireItself,
    ];

    /// Where the reader lives, from the workspace root, with forward slashes.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::PortableCoreArray => "scripts/check-portable-core.ps1",
            Self::VendoredAddedFiles => "scripts/check-vendor-notices.ps1",
            Self::TheTripwireItself => "crates/bt-source/tests/tripwire.rs",
        }
    }

    /// One line of why this reader's concern really is a file, for a message
    /// somebody reads at three in the morning. The whole reason is the variant's
    /// doc comment.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::PortableCoreArray => {
                "the array is declared beside `fn main`, the gate answers on a tree that does not \
                 compile, and what it pins is which files may name a platform"
            }
            Self::VendoredAddedFiles => {
                "which files of a vendored copy are not upstream's is a fact about files, in a \
                 tree this workspace's declarations do not describe"
            }
            Self::TheTripwireItself => {
                "the scan that catches the other kind; files are its subject, and it is written \
                 without this crate's query layer on purpose"
            }
        }
    }
}

/// What a reading is asked about.
///
/// One variant today. The named scopes of the plan's §2.5 and §4.1 —
/// `Scope::Item` and `Scope::Module`, which are what almost every migrated
/// reader will take — are P1c's, and they land here beside this one. What P2
/// owns is the variant that names a file, and the fact that it cannot be built
/// without naming which allowlist entry it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// One named file, and the reason it is allowed to be one.
    ///
    /// There is no constructor taking a path, which is the enforcement: a reader
    /// that wants a file has to add a variant to [`FileScoped`], and adding one
    /// is a doc comment somebody reviews.
    File(FileScoped),
}

impl Scope {
    /// The allowlist entry this scope was built from.
    #[must_use]
    pub const fn entry(self) -> FileScoped {
        match self {
            Self::File(entry) => entry,
        }
    }

    /// The file this scope names, from the workspace root.
    #[must_use]
    pub const fn named_file(self) -> &'static str {
        self.entry().path()
    }
}

// The tests for this module are in `crates/bt-source/tests/tripwire.rs` and not
// here, for a reason worth stating: asserting anything about this type means
// *constructing* one, and a construction is the third thing the tripwire's scan
// looks for. Written here it would be a hit in a file on neither list; written
// there it is a hit in the one file the allowlist already carries, beside the
// scan that finds it. The two are one subject.
