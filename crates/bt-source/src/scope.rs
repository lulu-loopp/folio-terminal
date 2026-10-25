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
    /// `scripts/dev/bt-app-graph.py`, the module-graph generator behind
    /// `docs/plans/bt-app-split.md`'s measurements, naming `mod.rs` and
    /// `<name>.rs`.
    ///
    /// It rebuilds `bt-app`'s module graph by resolving each `mod`
    /// declaration to the file the compiler loads for it (`foo.rs` or
    /// `foo/mod.rs`) and parsing that file's text, so files and their text are
    /// its subject, as they are the dead-code gate's. It is a Python
    /// documentation tool that re-runs a plan's numbers and cannot ask this
    /// crate's index.
    TheModuleGraphGenerator,
    /// `scripts/dev/bt-app-reader-shapes.py`, the measurement behind
    /// `docs/plans/bt-app-reader-shapes-2026-09-21.md`, naming `.rs` files.
    ///
    /// It reads the debt list, the source files its rows name and the files
    /// their readings name, and classifies the text each reading slices: the
    /// text of named files is what it measures. It is a Python measurement
    /// whose dated output is a record, and cannot ask this crate's index.
    TheReaderShapesMeasurement,
    /// `scripts/dev/bt-app-move-topic.py`, the tool that moved each 2a topic
    /// out of `main.rs`, naming `main.rs`, `mod.rs` and `<topic>.rs`.
    ///
    /// Step 2a is finished (`docs/DESIGN.md`, 2026-09-22); the script is the
    /// retained `--check` proof of those moves, a record. It moved a topic's
    /// methods as text from one file into another, and `--check` compares the
    /// pre-move text from Git with the topic file: the files it moved text
    /// between are its subject, and a move of text has no item to ask about.
    /// It is a Python tool and cannot ask this crate's index.
    TheTopicMover,
    /// `.github/workflows/ci.yml`, the gate canaries, naming
    /// `crates/bt-doc/src/lib.rs`, `crates/bt-layout/src/lib.rs`,
    /// `crates/bt-viewport/src/lib.rs`, `crates/bt-compose/src/lib.rs`,
    /// `crates/bt-platform/src/http.rs`, `crates/bt-lint-probe/src/lib.rs`
    /// and `crates/bt-app/src/explorer_menu.rs`.
    ///
    /// Each canary proves a gate can fail on the defect it exists for: it
    /// plants that defect into a committed source file, requires the gate to
    /// refuse it (by that file, where the gate names files), and writes the
    /// file's original text back. The steps are `gates-can-fail`'s "the
    /// portable-core gate can actually fail", "the lint gate can actually
    /// fail", "the wasm-floor gate can actually fail", "the clock guard can
    /// actually fail, and its scope is the stated one" (whose fifth plant,
    /// `http.rs`, must *not* be refused: the scope is the stated set) and "the
    /// embedded-spaces gate can actually fail"; `logic`'s "the ignored-test
    /// gate can actually fail"; and `core-macos`'s "the macOS lint gate can
    /// actually fail", which plants into `crates/bt-app/src/explorer_menu.rs`.
    /// That canary's real read of the source — `cp "$file"` of an unquoted
    /// `file=crates/bt-app/src/explorer_menu.rs` — is invisible to the scan;
    /// the step is a finding only because it keeps the original in a scratch
    /// `.rs` under `$RUNNER_TEMP`, whose interpolated path the scan cannot
    /// read, and it is listed for the read it really does. A scratch copy would
    /// not do in place of any of them: the gates read the
    /// committed tree (the source set, the workspace's crates, `git ls-files`),
    /// and what is proved is that a defect in a committed file is refused, so
    /// the committed file is the subject. It is YAML run by GitHub and cannot
    /// ask this crate's index.
    TheGateCanaries,
    /// `scripts/ci/check-psreadline-artefact.ps1`, the PSReadLine pin gate, naming
    /// `crates/bt-app/src/psreadline_artefact.rs` (T-INTEGRATION-INJECT-2-BUILD).
    ///
    /// A pin of the vendored PSReadLine package is written in three files — the package's own
    /// README, the build script's table and Folio's manifest — and the gate's fact is that the
    /// three spell the same hashes and version as the package holds. The table's *written text*
    /// is one of the three places, so the file is the subject, as the README and the manifest
    /// are; the build script's own refusal is the gate that asks the program. It runs in CI's
    /// `licensing` job, which builds nothing, and is a PowerShell gate that cannot ask this
    /// crate's index.
    ThePsReadLineArtefactGate,
    /// `scripts/ci/check-timing-bound.ps1`, the timing-bound list's history half,
    /// naming `crates/bt-source/tests/timing.rs` (T-TIMING-BOUND-STRAYS).
    ///
    /// A `# widened ... (+n rows)` line lets rows into a list that otherwise only
    /// shrinks, and a widening is by definition a change to the scan's grammar:
    /// the gate asks `git diff` whether the scan's file differs from the merge
    /// base's, so that a change adding only a comment line cannot let a new
    /// waiting test in. Whether a committed file changed between two commits is
    /// a fact about that file, with no item to ask about; the grammar is the
    /// whole test file, not one item in it. It is a PowerShell gate and cannot
    /// ask this crate's index.
    TheTimingWideningGate,
    /// `scripts/ci/check-test-globals.ps1`, the process-globals register's history half,
    /// naming `crates/bt-source/tests/test_globals.rs` (T-TEST-PROCESS-GLOBALS).
    ///
    /// The same shape as [`Self::TheTimingWideningGate`]: an added row of a by-hand class is
    /// admitted only under a `# widened ... (+n rows)` line in a change to the scan's grammar,
    /// and the gate asks `git diff` whether the scan's file differs from the merge base's.
    /// Whether a committed file changed between two commits is a fact about that file, and the
    /// grammar is the whole test file, not one item in it. It is a PowerShell gate and cannot
    /// ask this crate's index.
    TheTestGlobalsWideningGate,
    /// `scripts/ci/check-native-labels.ps1`, the native-labels guard's history half, naming
    /// `crates/bt-source/tests/native_labels.rs` (T-NATIVE-LABELS-GUARD-REACH).
    ///
    /// The guard's four lists (`ALLOWED`, `MACROS`, `BOUNDARY`, `UNREAD`) are written in the
    /// guard's own file, and the gate holds their rows to the merge base's: it reads the lists'
    /// text from the file and from `git show`, and admits growth only under a `// widened ...
    /// (+n rows)` line in a change to the rest of that file, the guard's grammar. The rows as
    /// written and whether the file's grammar changed are facts about that file, with no item to
    /// ask about. It is a PowerShell gate and cannot ask this crate's index.
    TheNativeLabelsListsGate,
}

impl FileScoped {
    /// Every entry. The allowlist is small enough to be an array and only shrinks.
    pub const ALL: [Self; 13] = [
        Self::VendoredAddedFiles,
        Self::TheTripwireItself,
        Self::TheDeadCodeGate,
        Self::TheLintProbeIsGenerated,
        Self::TheEmbeddedSpacesGate,
        Self::TheModuleGraphGenerator,
        Self::TheReaderShapesMeasurement,
        Self::TheTopicMover,
        Self::TheGateCanaries,
        Self::ThePsReadLineArtefactGate,
        Self::TheTimingWideningGate,
        Self::TheTestGlobalsWideningGate,
        Self::TheNativeLabelsListsGate,
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
            Self::TheModuleGraphGenerator => "scripts/dev/bt-app-graph.py",
            Self::TheReaderShapesMeasurement => "scripts/dev/bt-app-reader-shapes.py",
            Self::TheTopicMover => "scripts/dev/bt-app-move-topic.py",
            Self::TheGateCanaries => ".github/workflows/ci.yml",
            Self::ThePsReadLineArtefactGate => "scripts/ci/check-psreadline-artefact.ps1",
            Self::TheTimingWideningGate => "scripts/ci/check-timing-bound.ps1",
            Self::TheTestGlobalsWideningGate => "scripts/ci/check-test-globals.ps1",
            Self::TheNativeLabelsListsGate => "scripts/ci/check-native-labels.ps1",
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
            Self::TheModuleGraphGenerator => {
                "the module graph is rebuilt by resolving each `mod` declaration to the file the \
                 compiler loads and parsing its text; a Python tool behind the split plan's numbers"
            }
            Self::TheReaderShapesMeasurement => {
                "what the debt list's readings slice is measured in the text of the files they \
                 name; a Python measurement whose dated output is a record"
            }
            Self::TheTopicMover => {
                "the retained `--check` proof of step 2a's moves, a record: a topic's methods moved \
                 as text between files, proved against the text in Git; a Python tool"
            }
            Self::TheGateCanaries => {
                "each canary plants a gate's defect into a committed source file, requires the \
                 refusal and writes the file back: a defect in a committed file is the proof's \
                 subject; CI's YAML"
            }
            Self::ThePsReadLineArtefactGate => {
                "a PSReadLine pin is written in three files and the gate holds their text to the \
                 package: the build script's table, as written, is one of the three; a \
                 PowerShell gate in a job that builds nothing"
            }
            Self::TheTimingWideningGate => {
                "a widening line admits rows only in a change to the scan's grammar, and whether \
                 the scan's file differs from the merge base's is a fact about that file; a \
                 PowerShell gate"
            }
            Self::TheTestGlobalsWideningGate => {
                "a widening line admits by-hand rows only in a change to the scan's grammar, and \
                 whether the scan's file differs from the merge base's is a fact about that file; \
                 a PowerShell gate"
            }
            Self::TheNativeLabelsListsGate => {
                "the guard's four lists are written in its own file and the gate holds their rows \
                 to the merge base's, growth only under a widening line in a change to the rest of \
                 the file; facts about that file; a PowerShell gate"
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
