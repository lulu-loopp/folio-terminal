//! **The reading contract of §2, as an API a caller cannot take by accident.**
//!
//! Four things are explicit here because the plan says a reader that leaves any
//! of them implicit reads a different program from the one it means:
//!
//! 1. **The view.** There is no default (§2.1). Five guards in this tree search
//!    text whose subject lives inside a string literal, and a literal-stripping
//!    default would have turned every one of them quietly green.
//! 2. **What the needle is.** A name, a path, a call shape or plain bytes —
//!    [`Pattern`] — because `stand_in` is not `strip_stand_in` and a substring
//!    count cannot say so.
//! 3. **What is not counted.** A declaration exemption (§2.5) is a named
//!    argument, never a rule the query applies on its own, and every span it
//!    removed is in the answer for a reviewer to read.
//! 4. **Where the needle itself was written** (§2.6). A reader that spells its
//!    subject in its own source would otherwise find itself; [`needle!`] records
//!    the caller's site so that the *construction* is excluded — and nothing
//!    else, never the function around it.
//!
//! **The two rules that were already ruled are in force.** A unique query that
//! finds none, or more than it was told to expect, is a [`QueryFailure`] naming
//! every candidate with its file, its position and its predicate (§2.4); zero is
//! the dangerous one, because a guard whose subject has moved and whose reader
//! answers "not found" is the exact failure this preparation exists to remove.
//! And a match may cross nothing: not a file boundary (§2.2 rule 3, by
//! construction — every search runs file by file), not a masked region (rule 2),
//! and a match in [`View::LiteralValues`] reports **the literal's span**, not
//! the length of what it decodes to.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::enumerate::FileOwner;
use crate::index::{Certainty, FileRecord, Index, ItemIdentity, ItemRecord, LiteralValue, Span};
use crate::scope::FileScoped;

/// Which bytes a query reads. **There is no default** (§2.1): the tree holds
/// five readers whose subject lives inside a string literal, and a
/// literal-stripping default would have turned every one of them quietly green.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum View {
    /// Every byte of the enumerated source, unchanged.
    Raw,
    /// Comments and doc comments removed; string, raw-string, byte-string and
    /// character literals preserved verbatim. A match may not cross a removed
    /// region: masked bytes are opaque, not absent.
    CodeKeepingLiterals,
    /// The names, as the lexer classified them — boundary-checked by
    /// construction, and reaching inside macro token trees.
    ///
    /// **Plain bytes are not a question this view answers.** A [`Pattern::Text`]
    /// here is refused rather than run as a substring search inside token
    /// names, which is how `stand_in` would otherwise be found in
    /// `stand_in_window`.
    Identifiers,
    /// What the literals decode to, which is a different fact from how they are
    /// written. A match reports the literal's own span.
    LiteralValues,
}

impl fmt::Display for View {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Raw => "Raw",
            Self::CodeKeepingLiterals => "CodeKeepingLiterals",
            Self::Identifiers => "Identifiers",
            Self::LiteralValues => "LiteralValues",
        };
        formatter.write_str(name)
    }
}

// ── what to look for ──────────────────────────────────────────────────────

/// The shape of a needle.
///
/// The three named shapes carry their own boundary rule, which is the whole of
/// §2.5's first half: an identifier is bounded on both sides, a path is its
/// segments in a row, and a call shape is a name with an opening parenthesis
/// after it. [`Pattern::Text`] carries none and is what a prohibition on a
/// *spelling* — a URL, a flag, a marker inside a string — asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// Bytes, matched literally, with no boundary rule at all.
    Text(String),
    /// One name, matched whole: `stand_in` is not `strip_stand_in` and not
    /// `stand_in_window`.
    Identifier(String),
    /// `NativeWindow::stand_in` — the segments in a row, each a whole name,
    /// separated by `::`. A longer path that ends in these segments matches,
    /// because `crate::NativeWindow::stand_in` *is* the thing asked about.
    Path(Vec<String>),
    /// `stand_in(` — the name with an opening parenthesis after it. In
    /// [`View::Identifiers`] whitespace and comments may stand between them,
    /// because the lexer says where the name ends; in a byte view it is the
    /// literal spelling, which is what every reader in this tree writes today.
    Call(String),
}

impl Pattern {
    #[must_use]
    pub fn text(bytes: &str) -> Self {
        Self::Text(bytes.to_owned())
    }

    #[must_use]
    pub fn identifier(name: &str) -> Self {
        Self::Identifier(name.to_owned())
    }

    /// `a::b::c`, written the way it is written in the source.
    #[must_use]
    pub fn path(path: &str) -> Self {
        Self::Path(
            path.split("::")
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        )
    }

    #[must_use]
    pub fn call(name: &str) -> Self {
        Self::Call(name.to_owned())
    }

    /// The bytes this pattern is looking for, for the views that read bytes.
    #[must_use]
    pub fn spelling(&self) -> String {
        match self {
            Self::Text(bytes) | Self::Identifier(bytes) => bytes.clone(),
            Self::Path(segments) => segments.join("::"),
            Self::Call(name) => format!("{name}("),
        }
    }

    /// Whether the character before and after a byte-view match must be outside
    /// an identifier.
    const fn boundaries(&self) -> (bool, bool) {
        match self {
            Self::Text(_) => (false, false),
            Self::Identifier(_) | Self::Path(_) => (true, true),
            // The right-hand side is the parenthesis the pattern already ends
            // with, and a parenthesis is not an identifier character.
            Self::Call(_) => (true, false),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(bytes) | Self::Identifier(bytes) | Self::Call(bytes) => bytes.is_empty(),
            Self::Path(segments) => segments.is_empty(),
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(bytes) => write!(formatter, "text `{bytes}`"),
            Self::Identifier(name) => write!(formatter, "identifier `{name}`"),
            Self::Path(_) => write!(formatter, "path `{}`", self.spelling()),
            Self::Call(name) => write!(formatter, "call `{name}(`"),
        }
    }
}

impl From<&str> for Pattern {
    /// Plain bytes. **Deliberately not a classification**: a `&str` that happens
    /// to spell an identifier is still bytes, because a reading that guessed
    /// would silently apply a boundary rule the caller did not ask for.
    fn from(bytes: &str) -> Self {
        Self::text(bytes)
    }
}

impl From<String> for Pattern {
    fn from(bytes: String) -> Self {
        Self::Text(bytes)
    }
}

/// Where a needle was constructed — the caller's own `file!`, `line!`, `column!`
/// and manifest directory, recorded by [`needle!`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    file: String,
    line: usize,
    column: usize,
    manifest: PathBuf,
}

impl Site {
    /// Built by [`needle!`]; public so that a reader with its own macro can
    /// build one the same way.
    #[must_use]
    pub fn new(file: &str, line: u32, column: u32, manifest: &str) -> Self {
        Self {
            file: file.to_owned(),
            line: line as usize,
            column: column as usize,
            manifest: PathBuf::from(manifest),
        }
    }

    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub fn column(&self) -> usize {
        self.column
    }

    /// The file `file!()` named, as a path on this disk.
    ///
    /// `file!()` is relative to whatever directory the compiler was run from —
    /// the workspace root for a cargo build, but not by any promise — so the
    /// resolution walks up from **the caller's own crate**: this is how a needle
    /// built in one package resolves while a different package is being
    /// queried (§2.6 rule 1).
    ///
    /// # Panics
    ///
    /// When no ancestor of the caller's manifest directory holds that file.
    /// **Not resolving is a panic and not an empty exclusion**: a self-exclusion
    /// that silently excludes nothing is the quiet failure §2.6 exists to
    /// prevent.
    #[must_use]
    pub fn resolve(&self) -> PathBuf {
        for ancestor in self.manifest.ancestors() {
            let candidate = ancestor.join(&self.file);
            if candidate.is_file() {
                return crate::paths::normalized(&candidate);
            }
        }
        panic!(
            "`file!()` gave `{}`, and no ancestor of {} holds it; a needle whose own site cannot \
             be found would exclude nothing and say nothing",
            self.file,
            self.manifest.display()
        )
    }
}

/// What to look for, and — when the caller built it with [`needle!`] — where the
/// caller wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needle {
    pattern: Pattern,
    site: Option<Site>,
}

impl Needle {
    /// A needle with no provenance: nothing is excluded on its account.
    #[must_use]
    pub fn new(pattern: impl Into<Pattern>) -> Self {
        Self {
            pattern: pattern.into(),
            site: None,
        }
    }

    /// A needle that knows where it was written. [`needle!`] is how this is
    /// normally built.
    #[must_use]
    pub fn at(pattern: impl Into<Pattern>, site: Site) -> Self {
        Self {
            pattern: pattern.into(),
            site: Some(site),
        }
    }

    #[must_use]
    pub const fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    #[must_use]
    pub const fn site(&self) -> Option<&Site> {
        self.site.as_ref()
    }
}

impl<T: Into<Pattern>> From<T> for Needle {
    fn from(pattern: T) -> Self {
        Self::new(pattern)
    }
}

/// **A needle that records where it was written** (§2.6).
///
/// ```text
/// index.search(&Search::new(needle!(Pattern::call("stand_in")), View::Identifiers))
/// ```
///
/// The expansion carries the **caller's** `file!`, `line!`, `column!` and
/// `CARGO_MANIFEST_DIR`, so a needle built in `bt-term`'s integration test and
/// asked of `bt-app` resolves through `bt-term` and then finds that the site is
/// not part of the queried universe — which is a recorded answer and not a
/// failure. A needle built inside the universe being queried has its own
/// construction excluded, and only that: the expression, never the function
/// around it, because a test that both spells a name and calls the thing it is
/// about has genuine occurrences in the same function.
#[macro_export]
macro_rules! needle {
    ($pattern:expr) => {
        $crate::Needle::at(
            $pattern,
            $crate::Site::new(file!(), line!(), column!(), env!("CARGO_MANIFEST_DIR")),
        )
    };
}

// ── where to look ─────────────────────────────────────────────────────────

/// **A scope is a Rust path, never a file** (§2.4, §3).
///
/// That is the whole point of the preparation: `crate::preview_select` and
/// `TabState::mini_source` name the same thing before and after an item moves
/// between files, and a text-separator scope or a file name does not.
///
/// **The seam for `Scope::File`.** §6.1 keeps a permanent, typed allowlist for
/// the handful of readers whose concern really is a file, and that variant is
/// constructible only through it — there is no constructor taking a path. P1c
/// and P2 were written against each other and merged apart, each carrying a
/// `Scope` of its own; this is the one the two of them describe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every byte of the universe.
    Everything,
    /// One module, by its Rust path: `crate`, `crate::preview_select`. The
    /// module's bytes are its file, or its braces when it is written inline.
    Module(String),
    /// One item: the answer to an [`ItemQuery`], which is loud when the item is
    /// not there or is not unique.
    Item(ItemQuery),
    /// One named file, and the reason it is allowed to be one (P2).
    ///
    /// A reader that wants a file has to add a variant to [`FileScoped`], and
    /// adding one is a doc comment somebody reviews.
    File(FileScoped),
}

impl Scope {
    /// The allowlist entry this scope was built from, for the scopes that name
    /// a file.
    #[must_use]
    pub const fn entry(&self) -> Option<FileScoped> {
        match self {
            Self::File(entry) => Some(*entry),
            _ => None,
        }
    }

    /// The file this scope names, from the workspace root.
    #[must_use]
    pub const fn named_file(&self) -> Option<&'static str> {
        match self {
            Self::File(entry) => Some(entry.path()),
            _ => None,
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Everything => formatter.write_str("the whole universe"),
            Self::Module(path) => write!(formatter, "module `{path}`"),
            Self::Item(query) => write!(formatter, "item `{query}`"),
            Self::File(entry) => write!(formatter, "file `{}`", entry.path()),
        }
    }
}

// ── which declarations ────────────────────────────────────────────────────

/// How many declarations of one identity a query expects (§2.4).
///
/// **An argument, never an inference.** Eleven callable identities in `bt-app`
/// are declared twice under mutually exclusive conditions, and a query that did
/// not say so would either pick one — the failure this crate exists to remove —
/// or refuse every one of the eleven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Multiplicity {
    /// Exactly this many declarations, whatever their predicates.
    Exactly(usize),
    /// One declaration per conditional arm: any number of them, provided no two
    /// carry the *same* predicate. Two unconditional declarations of one name
    /// are still a refusal, because that is not an identity with variants.
    OnePerVariant,
}

/// One item, asked for by the tuple of §2.4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemQuery {
    name: String,
    owner: Owner,
    declaring: Declaring,
    module_path: Option<String>,
    variant: Option<Vec<String>>,
    expected: Multiplicity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Owner {
    /// Written at module level, belonging to no type.
    Free,
    /// Written in an `impl` block for this type, **named by its last path
    /// segment** with lifetimes and generic arguments ignored: `Runtime` is the
    /// owner of `impl Runtime<'_>` and of `impl crate::Runtime<'_>` alike.
    Type(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Declaring {
    /// An inherent `impl`, or a free function — no trait either way.
    Inherent,
    Trait(String),
}

impl ItemQuery {
    /// A free function.
    #[must_use]
    pub fn function(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            owner: Owner::Free,
            declaring: Declaring::Inherent,
            module_path: None,
            variant: None,
            expected: Multiplicity::Exactly(1),
        }
    }

    /// A method of `type_owner`'s inherent `impl` blocks — the narrowing §2.4
    /// gives `method_body(self_ty, name)`, where `Runtime<'_>`, `Runtime<'a>`
    /// and `crate::Runtime<'_>` are the same type.
    ///
    /// **Name the type, never the path to it**: `"Runtime"` and not
    /// `"crate::Runtime"`, because the owner is the `impl`'s self type reduced
    /// to its last segment (§2.4) and the module the block is written in is
    /// [`ItemQuery::in_module`]'s business.
    #[must_use]
    pub fn method(type_owner: &str, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            owner: Owner::Type(type_owner.to_owned()),
            declaring: Declaring::Inherent,
            module_path: None,
            variant: None,
            expected: Multiplicity::Exactly(1),
        }
    }

    /// Narrow to one trait's implementation, or to the trait's own default.
    ///
    /// The trait may be named **with its arguments or without**: `From` matches
    /// every `impl From<…>`, which is loud when there are two of them, and
    /// `From<Vec<Block>>` names one of them. A record carries the spelling the
    /// `impl` block was written with.
    #[must_use]
    pub fn of_trait(mut self, trait_name: &str) -> Self {
        self.declaring = Declaring::Trait(trait_name.to_owned());
        self
    }

    /// Narrow to one module path — `crate::wsl`, not a file.
    #[must_use]
    pub fn in_module(mut self, module_path: &str) -> Self {
        self.module_path = Some(module_path.to_owned());
        self
    }

    /// Narrow to one conditional arm, by the `cfg` spellings standing on it —
    /// the other way to make one of the eleven a unique identity.
    #[must_use]
    pub fn in_variant(mut self, predicates: &[&str]) -> Self {
        self.variant = Some(predicates.iter().map(|it| (*it).to_owned()).collect());
        self
    }

    /// How many declarations this identity is expected to have.
    #[must_use]
    pub const fn expecting(mut self, declarations: usize) -> Self {
        self.expected = Multiplicity::Exactly(declarations);
        self
    }

    /// Accept one declaration per conditional arm (§2.4).
    #[must_use]
    pub const fn one_per_variant(mut self) -> Self {
        self.expected = Multiplicity::OnePerVariant;
        self
    }

    #[must_use]
    pub const fn multiplicity(&self) -> Multiplicity {
        self.expected
    }

    fn selects(&self, record: &ItemRecord) -> bool {
        if record.name() != self.name {
            return false;
        }
        let owner = match &self.owner {
            Owner::Free => record.type_owner().is_none(),
            Owner::Type(wanted) => record.type_owner() == Some(wanted.as_str()),
        };
        let declaring = match &self.declaring {
            Declaring::Inherent => record.trait_name().is_none(),
            Declaring::Trait(wanted) => record
                .trait_name()
                .is_some_and(|actual| actual == wanted || bare_trait(actual) == wanted),
        };
        let module = self.module_path.as_ref().is_none_or(|wanted| {
            record
                .module_paths()
                .iter()
                .any(|path| path.as_str() == wanted.as_str())
        });
        let variant = self
            .variant
            .as_ref()
            .is_none_or(|wanted| record.variant().predicates() == wanted.as_slice());
        owner && declaring && module && variant
    }
}

impl fmt::Display for ItemQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(module) = &self.module_path {
            write!(formatter, "{module}::")?;
        }
        match (&self.owner, &self.declaring) {
            (Owner::Type(owner), Declaring::Trait(trait_name)) => {
                write!(formatter, "<{owner} as {trait_name}>::")?;
            }
            (Owner::Type(owner), Declaring::Inherent) => write!(formatter, "{owner}::")?,
            (Owner::Free, Declaring::Trait(trait_name)) => write!(formatter, "{trait_name}::")?,
            (Owner::Free, Declaring::Inherent) => {}
        }
        write!(formatter, "{}", self.name)?;
        if let Some(predicates) = &self.variant {
            for predicate in predicates {
                write!(formatter, " #[cfg({predicate})]")?;
            }
        }
        Ok(())
    }
}

/// One declaration a query saw, said the way a reviewer needs to read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub identity: ItemIdentity,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Candidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}  {}",
            self.file.display(),
            self.line,
            self.column,
            self.identity
        )
    }
}

/// Why a query refused to answer.
///
/// **Every one of these is a refusal in place of a smaller answer.** The whole
/// preparation exists because a reader that quietly answers "none" about a
/// subject that moved stays green.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryFailure {
    /// The number of declarations is not the number the query said to expect.
    Multiplicity {
        query: String,
        expected: Multiplicity,
        found: Vec<Candidate>,
        /// Declarations carrying the same name that the rest of the query ruled
        /// out. Printed when nothing matched, because "not found" plus the six
        /// things that nearly were is a diagnosis and "not found" alone is not.
        near: Vec<Candidate>,
    },
    /// `OnePerVariant` found two declarations standing on the same predicate —
    /// so they are not two arms of one identity, they are two things.
    VariantCollision {
        query: String,
        variant: String,
        found: Vec<Candidate>,
    },
    /// The identity resolved, and it is a signature with no body to return.
    NoBody { query: String, at: Vec<Candidate> },
    /// A pattern and a view that do not answer the same kind of question —
    /// plain bytes asked of [`View::Identifiers`], which would be a substring
    /// search inside token names.
    PatternViewMismatch { pattern: String, view: View },
    /// A named scope that names nothing in this universe. A scope resolving to
    /// no bytes would make every query inside it answer zero.
    EmptyScope { scope: String },
    /// The needle's own site is inside the queried universe and the expression
    /// that built it could not be found there. §2.6's hard failure: an
    /// exclusion that silently excludes nothing.
    ///
    /// **Two causes are known, and the message names both.** The usual one is a
    /// stale test binary: the site's line comes from `line!()`, which is baked
    /// in when the test is compiled, so an edit anywhere above it — a comment
    /// will do — moves the expression while the binary goes on naming the line
    /// it used to be on. P3's pilot met this one, and met it as a refusal whose
    /// message described the second cause only. The other is a site that names
    /// a file the needle was not built in, which is a wrong site rather than an
    /// old one.
    NeedleConstructionLost {
        file: PathBuf,
        line: usize,
        column: usize,
    },
    /// The query asked for occurrences the parser could place, and at least one
    /// is inside a macro's token tree (§2.7).
    LexicalCandidate { query: String, at: Vec<String> },
}

impl fmt::Display for QueryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Multiplicity {
                query,
                expected,
                found,
                near,
            } => {
                write!(
                    formatter,
                    "`{query}` was asked for {expected:?} and {} answer(s)",
                    found.len()
                )?;
                for candidate in found {
                    write!(formatter, "\n  found: {candidate}")?;
                }
                for candidate in near {
                    write!(formatter, "\n  same name, ruled out: {candidate}")?;
                }
                Ok(())
            }
            Self::VariantCollision {
                query,
                variant,
                found,
            } => {
                write!(
                    formatter,
                    "`{query}` has two declarations standing on the same predicate \
                     `{variant}`, so they are not two arms of one identity"
                )?;
                for candidate in found {
                    write!(formatter, "\n  found: {candidate}")?;
                }
                Ok(())
            }
            Self::NoBody { query, at } => {
                write!(formatter, "`{query}` has no body to read")?;
                for candidate in at {
                    write!(formatter, "\n  declared: {candidate}")?;
                }
                Ok(())
            }
            Self::PatternViewMismatch { pattern, view } => write!(
                formatter,
                "{pattern} cannot be asked of `View::{view}`: that view reads names, and plain \
                 bytes inside a name are how `stand_in` is found in `stand_in_window`. Ask for \
                 `Pattern::identifier`, `Pattern::path` or `Pattern::call`, or read a byte view."
            ),
            Self::EmptyScope { scope } => write!(
                formatter,
                "{scope} names nothing in this universe, and a query inside it would answer zero \
                 about every needle"
            ),
            Self::NeedleConstructionLost { file, line, column } => write!(
                formatter,
                "the needle's site is {}:{line}:{column}, which is inside the queried universe, \
                 and no expression that builds a needle stands there. Two things do this. The \
                 usual one is a stale test binary: the line comes from `line!()` and is baked in \
                 at compile time, so an edit above it — a comment will do — moves the expression \
                 while the binary goes on naming the old line; rebuild the test and ask again. \
                 The other is a needle built outside the file its site names, which is a wrong \
                 site rather than an old one. Excluding nothing is not the third option: it would \
                 leave the reader matching itself.",
                file.display()
            ),
            Self::LexicalCandidate { query, at } => {
                write!(
                    formatter,
                    "`{query}` was asked for occurrences the parser placed, and {} of them are \
                     inside a macro's token tree",
                    at.len()
                )?;
                for place in at {
                    write!(formatter, "\n  candidate: {place}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for QueryFailure {}

// ── the answer ────────────────────────────────────────────────────────────

/// One match: where it is, and whether the parser could place it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub span: Span,
    pub certainty: Certainty,
}

/// Why a span is in the answer's excluded list rather than its occurrences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// The queried item's own declaration (§2.5).
    Declaration(String),
    /// The expression that built the needle (§2.6).
    NeedleConstruction,
}

/// One span a query removed, with the reason — **so that a reviewer sees what
/// was not counted** (§2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excluded {
    pub span: Span,
    pub why: Why,
}

/// What became of the needle's own site (§2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// The needle carries no site: nothing was excluded on its account, and
    /// nothing was looked for.
    NotRecorded,
    /// The caller lives outside the queried universe — the normal case for a
    /// cross-crate reader. **A recorded answer, not a failure.**
    Outside { file: PathBuf },
    /// The caller is inside the queried universe and its construction span was
    /// found and removed.
    Excluded { at: Span, file: PathBuf },
}

/// A search, answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    query: String,
    view: View,
    scope: String,
    occurrences: Vec<Occurrence>,
    excluded: Vec<Excluded>,
    provenance: Provenance,
}

impl Found {
    #[must_use]
    pub fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    /// Every match's span, at its [`View::Raw`] offsets whichever view found it.
    #[must_use]
    pub fn spans(&self) -> Vec<Span> {
        self.occurrences.iter().map(|found| found.span).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.occurrences.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.occurrences.is_empty()
    }

    /// How many matches the parser could place, and how many are lexical
    /// candidates inside a macro (§2.7).
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let candidates = self
            .occurrences
            .iter()
            .filter(|found| found.certainty == Certainty::Candidate)
            .count();
        (self.occurrences.len() - candidates, candidates)
    }

    /// The spans this query removed, and why.
    #[must_use]
    pub fn excluded(&self) -> &[Excluded] {
        &self.excluded
    }

    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The report §2.5 asks for: the question, the answer, and every span that
    /// was taken out of it with the reason.
    #[must_use]
    pub fn report(&self, index: &Index) -> String {
        let (resolved, candidates) = self.counts();
        let mut lines = vec![format!(
            "{} in View::{} over {}: {} occurrence(s), {resolved} resolved, {candidates} lexical",
            self.query,
            self.view,
            self.scope,
            self.occurrences.len()
        )];
        for found in &self.occurrences {
            lines.push(format!("  at {}", at(index, found.span)));
        }
        for gone in &self.excluded {
            let why = match &gone.why {
                Why::Declaration(identity) => format!("the declaration of {identity}"),
                Why::NeedleConstruction => "the needle's own construction".to_owned(),
            };
            lines.push(format!("  not counted, {why}: {}", at(index, gone.span)));
        }
        lines.push(match &self.provenance {
            Provenance::NotRecorded => "  the needle records no site".to_owned(),
            Provenance::Outside { file } => format!(
                "  the needle was built at {}, outside this universe",
                file.display()
            ),
            Provenance::Excluded { file, .. } => {
                format!("  the needle was built in {}", file.display())
            }
        });
        lines.join("\n")
    }

    /// **The owners of §4.1**: which items the matches are in, and how many are
    /// in each.
    ///
    /// A set of owners with multiplicities is what an ownership assertion
    /// compares, and it is stable under a move — relocating a caller from
    /// `main.rs` to `runtime/panes.rs` changes no key and no value. A total
    /// cannot express it, which is why §4.1 forbids one from replacing it.
    ///
    /// An occurrence outside every callable — in a `const`, an attribute, a
    /// module's own header — is in [`Found::outside_items`] instead. A file
    /// reached two ways answers to two identities, and both keys are counted,
    /// so the values sum to the occurrence count only where each owning file is
    /// reached one way.
    #[must_use]
    pub fn owners(&self, index: &Index) -> BTreeMap<ItemIdentity, usize> {
        let mut owners = BTreeMap::new();
        for found in &self.occurrences {
            let Some(record) = innermost_item(index, found.span) else {
                continue;
            };
            for identity in record.identities() {
                *owners.entry(identity).or_insert(0) += 1;
            }
        }
        owners
    }

    /// How many matches are in no callable at all.
    #[must_use]
    pub fn outside_items(&self, index: &Index) -> usize {
        self.occurrences
            .iter()
            .filter(|found| innermost_item(index, found.span).is_none())
            .count()
    }
}

fn at(index: &Index, span: Span) -> String {
    index
        .locate(span.start())
        .map_or_else(|| format!("{span:?}"), |location| location.to_string())
}

/// The smallest callable whose bytes hold `span`.
fn innermost_item(index: &Index, span: Span) -> Option<&ItemRecord> {
    index
        .items()
        .iter()
        .filter(|record| span.within(record.whole()))
        .min_by_key(|record| record.whole().len())
}

// ── the search ────────────────────────────────────────────────────────────

/// One question, with everything it needs said out loud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    needle: Needle,
    view: View,
    scope: Scope,
    exemptions: Vec<ItemQuery>,
    resolved_only: bool,
}

impl Search {
    /// A search over the whole universe. The view is required; there is no
    /// default (§2.1).
    #[must_use]
    pub fn new(needle: impl Into<Needle>, view: View) -> Self {
        Self {
            needle: needle.into(),
            view,
            scope: Scope::Everything,
            exemptions: Vec::new(),
            resolved_only: false,
        }
    }

    /// Narrow to a named scope — a Rust path, never a file.
    #[must_use]
    pub fn in_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// **Do not count the item that declares this name** (§2.5).
    ///
    /// Explicit, never implicit: `NativeWindow::stand_in` is product code, and
    /// the guard that says "only tests name it" means "apart from the line that
    /// declares it". What is excluded is the declaration — attributes,
    /// visibility, signature — and not the body, so a name used inside its own
    /// item still counts. Every excluded span comes back in the answer.
    #[must_use]
    pub fn exempting_declarations_of(mut self, item: ItemQuery) -> Self {
        self.exemptions.push(item);
        self
    }

    /// Refuse to answer with a match the parser could not place (§2.7).
    #[must_use]
    pub const fn requiring_resolved(mut self) -> Self {
        self.resolved_only = true;
        self
    }

    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }
}

impl Index {
    /// **Run a search.** Every rule of §2 is applied here and nowhere else.
    ///
    /// # Errors
    ///
    /// Every variant of [`QueryFailure`]: a declaration exemption that is not
    /// unique, a scope that names nothing, a pattern the view cannot answer, a
    /// needle whose own construction is inside the universe and cannot be
    /// found, and a lexical candidate where the caller asked for a placed
    /// occurrence.
    ///
    /// # Panics
    ///
    /// When the needle's `file!()` names a file no ancestor of the caller's
    /// manifest directory holds — see [`Site::resolve`].
    pub fn search(&self, search: &Search) -> Result<Found, QueryFailure> {
        let pattern = search.needle.pattern();
        let mut spans = self.matches(pattern, search.view)?;

        let scope = self.scope_spans(&search.scope)?;
        if let Some(scope) = &scope {
            spans.retain(|span| scope.iter().any(|allowed| span.within(*allowed)));
        }

        let mut removals: Vec<Excluded> = Vec::new();
        for exemption in &search.exemptions {
            for record in self.find(exemption)? {
                for identity in record.identities() {
                    removals.push(Excluded {
                        span: record.declaration(),
                        why: Why::Declaration(identity.to_string()),
                    });
                }
            }
        }

        let provenance = self.provenance(search.needle.site())?;
        if let Provenance::Excluded { at, .. } = &provenance {
            removals.push(Excluded {
                span: *at,
                why: Why::NeedleConstruction,
            });
        }

        let mut excluded = Vec::new();
        let mut occurrences = Vec::new();
        for span in spans {
            match removals.iter().find(|gone| span.overlaps(gone.span)) {
                Some(gone) => excluded.push(Excluded {
                    span,
                    why: gone.why.clone(),
                }),
                None => occurrences.push(Occurrence {
                    span,
                    certainty: self.certainty(span),
                }),
            }
        }

        if search.resolved_only {
            let lexical: Vec<String> = occurrences
                .iter()
                .filter(|found| found.certainty == Certainty::Candidate)
                .map(|found| at(self, found.span))
                .collect();
            if !lexical.is_empty() {
                return Err(QueryFailure::LexicalCandidate {
                    query: pattern.to_string(),
                    at: lexical,
                });
            }
        }

        Ok(Found {
            query: pattern.to_string(),
            view: search.view,
            scope: search.scope.to_string(),
            occurrences,
            excluded,
            provenance,
        })
    }

    /// The bytes a named scope covers, or `None` for the whole universe.
    fn scope_spans(&self, scope: &Scope) -> Result<Option<Vec<Span>>, QueryFailure> {
        // The allowlisted scope is taken through its accessor rather than
        // through a match arm, because the tripwire's scan is lexical: a match
        // arm here would spell the construction this crate's own guard looks
        // for, in a file on neither list.
        if let Some(entry) = scope.entry() {
            // The allowlist is keyed from the workspace root with forward
            // slashes; the index holds absolute paths. Matching on the trailing
            // components is what compares the two without this crate having to
            // remember where the workspace is — and a named file that is not in
            // this universe answers `EmptyScope`, loudly, rather than zero.
            let spans: Vec<Span> = self
                .files()
                .iter()
                .filter(|file| file.path().ends_with(std::path::Path::new(entry.path())))
                .map(FileRecord::span)
                .collect();
            if spans.is_empty() {
                return Err(QueryFailure::EmptyScope {
                    scope: scope.to_string(),
                });
            }
            return Ok(Some(spans));
        }
        // The variants are named without their type for the same lexical
        // reason, and the arm is kept rather than wildcarded so that a fifth
        // variant is a compile error here.
        use Scope::{Everything, File, Item, Module};
        let spans: Vec<Span> = match scope {
            Everything => return Ok(None),
            Module(path) => self
                .modules()
                .iter()
                .filter(|module| module.module_paths().iter().any(|it| it == path))
                .map(|module| module.span())
                .collect(),
            Item(query) => self
                .find(query)?
                .iter()
                .map(|record| record.whole())
                .collect(),
            File(_) => unreachable!("a file scope is answered through its entry, above"),
        };
        if spans.is_empty() {
            return Err(QueryFailure::EmptyScope {
                scope: scope.to_string(),
            });
        }
        Ok(Some(spans))
    }

    /// Where the needle was written, and what that means for this universe.
    fn provenance(&self, site: Option<&Site>) -> Result<Provenance, QueryFailure> {
        let Some(site) = site else {
            return Ok(Provenance::NotRecorded);
        };
        let file = site.resolve();
        let Some(record) = self.file(&file) else {
            // §2.6 rule 1: a caller outside the queried universe is the normal
            // case for a cross-crate reader, and it is an answer.
            return Ok(Provenance::Outside { file });
        };
        let offset = record.offset_at(self.union(), site.line(), site.column());
        let construction = offset.and_then(|offset| {
            self.macros()
                .iter()
                .filter(|record| record.span().holds(offset))
                .min_by_key(|record| record.span().len())
                .map(|record| record.span())
        });
        match construction {
            // §2.6 rule 2: the expression, never the item around it.
            Some(at) => Ok(Provenance::Excluded { at, file }),
            None => Err(QueryFailure::NeedleConstructionLost {
                file,
                line: site.line(),
                column: site.column(),
            }),
        }
    }

    /// Every span `pattern` matches in `view`, before any scope or exclusion.
    fn matches(&self, pattern: &Pattern, view: View) -> Result<Vec<Span>, QueryFailure> {
        if pattern.is_empty() {
            return Ok(Vec::new());
        }
        match view {
            View::Raw | View::CodeKeepingLiterals => Ok(self.byte_matches(pattern, view)),
            View::Identifiers => self.token_matches(pattern),
            View::LiteralValues => Ok(self.literal_matches(pattern)),
        }
    }

    /// A byte view: the spelling, file by file, with the pattern's own boundary
    /// rule, and — in the code view — never overlapping a mask.
    fn byte_matches(&self, pattern: &Pattern, view: View) -> Vec<Span> {
        let spelling = pattern.spelling();
        let (left, right) = pattern.boundaries();
        let mut found = Vec::new();
        // File by file, so that a needle cannot be manufactured across the join
        // between two files of the union (§2.2 rule 3).
        for file in self.files() {
            let text = self.text(file.span());
            for (at, _) in text.match_indices(spelling.as_str()) {
                if !bounded(text, at, at + spelling.len(), left, right) {
                    continue;
                }
                let start = file.span().start() + at;
                let span = span_of(start, start + spelling.len());
                // Masked bytes are opaque, not absent: a match may neither sit
                // inside a removed region nor cross one (§2.2 rule 2).
                if view == View::Raw || !self.is_masked(span) {
                    found.push(span);
                }
            }
        }
        found
    }

    /// The identifier view: whole names, as the lexer classified them.
    fn token_matches(&self, pattern: &Pattern) -> Result<Vec<Span>, QueryFailure> {
        match pattern {
            Pattern::Text(_) => Err(QueryFailure::PatternViewMismatch {
                pattern: pattern.to_string(),
                view: View::Identifiers,
            }),
            Pattern::Identifier(name) => Ok(self
                .tokens()
                .iter()
                .filter(|token| self.text(token.name_span()) == name)
                .map(|token| token.name_span())
                .collect()),
            Pattern::Call(name) => Ok(self
                .tokens()
                .iter()
                .filter(|token| self.text(token.name_span()) == name)
                .filter(|token| self.opens_a_call(token.span()))
                .map(|token| token.name_span())
                .collect()),
            Pattern::Path(segments) => Ok(self.path_matches(segments)),
        }
    }

    /// The segments in a row, `::` between them and nothing else.
    ///
    /// A longer path ending in these segments matches: `crate::NativeWindow::stand_in`
    /// is an occurrence of `NativeWindow::stand_in`, because it is the same
    /// thing named more fully.
    fn path_matches(&self, segments: &[String]) -> Vec<Span> {
        let tokens = self.tokens();
        let mut found = Vec::new();
        for (at, first) in tokens.iter().enumerate() {
            if self.text(first.name_span()) != segments[0] {
                continue;
            }
            let mut previous = *first;
            let mut matched = true;
            for (step, segment) in segments[1..].iter().enumerate() {
                let Some(next) = tokens.get(at + step + 1) else {
                    matched = false;
                    break;
                };
                let joined = self
                    .union()
                    .get(previous.span().end()..next.span().start())
                    .is_some_and(|between| between.trim() == "::");
                if !joined || self.text(next.name_span()) != segment.as_str() {
                    matched = false;
                    break;
                }
                previous = *next;
            }
            if matched {
                found.push(span_of(first.span().start(), previous.span().end()));
            }
        }
        found
    }

    /// Whether the next thing after `token` is an opening parenthesis —
    /// whitespace and comments may stand between, because the lexer has already
    /// said where the name ends.
    fn opens_a_call(&self, token: Span) -> bool {
        let Some(file) = self.file_at(token.start()) else {
            return false;
        };
        let mut at = token.end();
        while at < file.span().end() {
            let rest = &self.union()[at..file.span().end()];
            let Some(character) = rest.chars().next() else {
                return false;
            };
            if character.is_whitespace() {
                at += character.len_utf8();
                continue;
            }
            let here = span_of(at, at + character.len_utf8());
            if let Some(mask) = self.mask_at(here) {
                at = mask.end();
                continue;
            }
            return character == '(';
        }
        false
    }

    /// The literal view: what the literals decode to, reported at the span of
    /// the literal as it is **written** (§2.2 rule 3's third half — a decoded
    /// value has no offsets of its own).
    fn literal_matches(&self, pattern: &Pattern) -> Vec<Span> {
        let spelling = pattern.spelling();
        let (left, right) = pattern.boundaries();
        self.literals()
            .iter()
            .filter(|literal| {
                value_text(literal.value()).is_some_and(|text| {
                    text.match_indices(spelling.as_str())
                        .any(|(at, _)| bounded(&text, at, at + spelling.len(), left, right))
                })
            })
            .map(|literal| literal.span())
            .collect()
    }
}

// ── the older, narrower queries, kept ─────────────────────────────────────

impl Index {
    /// Every declaration of `query`, refusing unless there are exactly as many
    /// as the query expects.
    ///
    /// # Errors
    ///
    /// [`QueryFailure::Multiplicity`] or [`QueryFailure::VariantCollision`],
    /// naming every candidate.
    pub fn find(&self, query: &ItemQuery) -> Result<Vec<&ItemRecord>, QueryFailure> {
        let found: Vec<&ItemRecord> = self
            .items()
            .iter()
            .filter(|record| query.selects(record))
            .collect();
        let answered = match query.multiplicity() {
            Multiplicity::Exactly(expected) => found.len() == expected,
            Multiplicity::OnePerVariant => !found.is_empty(),
        };
        if answered {
            if query.multiplicity() == Multiplicity::OnePerVariant {
                // Two arms of one identity carry two different predicates. Two
                // declarations carrying the same one are two things, and
                // picking either is the failure this crate exists to remove.
                for (at, record) in found.iter().enumerate() {
                    if let Some(twin) = found[at + 1..]
                        .iter()
                        .find(|other| other.variant() == record.variant())
                    {
                        return Err(QueryFailure::VariantCollision {
                            query: query.to_string(),
                            variant: record.variant().to_string(),
                            found: [record, twin]
                                .iter()
                                .flat_map(|record| self.candidates(record))
                                .collect(),
                        });
                    }
                }
            }
            return Ok(found);
        }
        let near = if found.is_empty() {
            self.items()
                .iter()
                .filter(|record| record.name() == query.name)
                .flat_map(|record| self.candidates(record))
                .collect()
        } else {
            Vec::new()
        };
        Err(QueryFailure::Multiplicity {
            query: query.to_string(),
            expected: query.multiplicity(),
            found: found
                .iter()
                .flat_map(|record| self.candidates(record))
                .collect(),
            near,
        })
    }

    /// The one declaration of `query`.
    ///
    /// # Errors
    ///
    /// [`QueryFailure::Multiplicity`] unless exactly one answers.
    pub fn one(&self, query: &ItemQuery) -> Result<&ItemRecord, QueryFailure> {
        let found = self.find(query)?;
        if let [only] = found[..] {
            return Ok(only);
        }
        // The query expected several and got them; this caller wanted one, and
        // saying so names every variant rather than picking the first.
        Err(QueryFailure::Multiplicity {
            query: query.to_string(),
            expected: Multiplicity::Exactly(1),
            found: found
                .iter()
                .flat_map(|record| self.candidates(record))
                .collect(),
            near: Vec::new(),
        })
    }

    /// The body of the one declaration of `query`, braces included.
    ///
    /// # Errors
    ///
    /// [`QueryFailure::Multiplicity`] when the identity is not unique, and
    /// [`QueryFailure::NoBody`] when it is a signature without one.
    pub fn body_of(&self, query: &ItemQuery) -> Result<&str, QueryFailure> {
        let record = self.one(query)?;
        match record.body() {
            Some(body) => Ok(self.text(body)),
            None => Err(QueryFailure::NoBody {
                query: query.to_string(),
                at: self.candidates(record),
            }),
        }
    }

    /// Every declaration path that reaches the file holding `query`'s item —
    /// the ownership paths of §2.3, each with its `cfg` predicate spelling.
    ///
    /// # Errors
    ///
    /// Whatever [`Index::one`] refuses.
    pub fn owners_of(&self, query: &ItemQuery) -> Result<&[FileOwner], QueryFailure> {
        let record = self.one(query)?;
        Ok(self.file_of(record).owners())
    }

    fn candidates(&self, record: &ItemRecord) -> Vec<Candidate> {
        record
            .identities()
            .map(|identity| Candidate {
                file: self.file_of(record).path().to_path_buf(),
                line: self
                    .locate(record.whole().start())
                    .map_or(0, |location| location.line),
                column: self
                    .locate(record.whole().start())
                    .map_or(0, |location| location.column),
                identity,
            })
            .collect()
    }

    /// Every occurrence of `bytes`, reported at its [`View::Raw`] offsets
    /// whichever view found it.
    ///
    /// **In [`View::Identifiers`] this is a whole name**, not a substring of
    /// one: `stand_in` is not found in `stand_in_window`. The shapes a name
    /// query can take are [`Pattern`]'s, and [`Index::search`] is where the
    /// contract lives; this is the short form for a plain spelling.
    ///
    /// # Panics
    ///
    /// Never: the only refusal `matches` can make for these patterns is the
    /// pattern/view mismatch, and neither shape below can be one.
    #[must_use]
    pub fn occurrences(&self, bytes: &str, view: View) -> Vec<Span> {
        let pattern = if view == View::Identifiers {
            Pattern::identifier(bytes)
        } else {
            Pattern::text(bytes)
        };
        self.matches(&pattern, view)
            .expect("a text pattern in a byte view and a name in the identifier view are answered")
    }

    /// Whether `bytes` occur at all — the whole-source negative every
    /// prohibition in this tree is written as.
    #[must_use]
    pub fn contains(&self, bytes: &str, view: View) -> bool {
        !self.occurrences(bytes, view).is_empty()
    }

    #[must_use]
    pub fn count(&self, bytes: &str, view: View) -> usize {
        self.occurrences(bytes, view).len()
    }

    /// Every occurrence of `name` **as a whole identifier**.
    ///
    /// In [`View::Identifiers`] that is the token reading and the boundaries
    /// come for nothing. In the byte views it is an occurrence whose neighbours
    /// on both sides are not identifier characters, which is the check §2.5
    /// says a one-line substring count is not equivalent to.
    ///
    /// # Panics
    ///
    /// Never: an identifier pattern is answered by every view.
    #[must_use]
    pub fn identifier_occurrences(&self, name: &str, view: View) -> Vec<Span> {
        self.matches(&Pattern::identifier(name), view)
            .expect("an identifier pattern is answered by every view")
    }

    #[must_use]
    pub fn count_identifier(&self, name: &str, view: View) -> usize {
        self.identifier_occurrences(name, view).len()
    }
}

/// Whether the characters on the chosen sides of `[start, end)` are outside an
/// identifier. `Runtime::strip_stand_in` is not an occurrence of `stand_in`.
fn bounded(text: &str, start: usize, end: usize, left: bool, right: bool) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    (!left || !before.is_some_and(is_identifier_character))
        && (!right || !after.is_some_and(is_identifier_character))
}

/// A union range as a span.
///
/// # Panics
///
/// Never: the union was refused at build time if it did not fit a `u32`.
fn span_of(start: usize, end: usize) -> Span {
    Span::new(
        u32::try_from(start).expect("the union fits a u32 by construction"),
        u32::try_from(end).expect("the union fits a u32 by construction"),
    )
}

/// A trait's name without its arguments: `From<Vec<Block>>` is `From`.
fn bare_trait(name: &str) -> &str {
    name.split('<').next().unwrap_or(name).trim_end()
}

fn is_identifier_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// A decoded literal value as text, for the views that search one.
fn value_text(value: &LiteralValue) -> Option<Cow<'_, str>> {
    match value {
        LiteralValue::Str(text) | LiteralValue::Int(text) | LiteralValue::Float(text) => {
            Some(Cow::Borrowed(text))
        }
        LiteralValue::ByteStr(bytes) | LiteralValue::CStr(bytes) => {
            std::str::from_utf8(bytes).ok().map(Cow::Borrowed)
        }
        LiteralValue::Char(character) => Some(Cow::Owned(character.to_string())),
        LiteralValue::Byte(byte) => Some(Cow::Owned(byte.to_string())),
        LiteralValue::Bool(value) => Some(Cow::Borrowed(if *value { "true" } else { "false" })),
        LiteralValue::Undecoded => None,
    }
}
