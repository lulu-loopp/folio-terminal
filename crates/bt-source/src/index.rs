//! **The index, not the trees** — plan §5, ticket P1b.
//!
//! The parser's span types are tied to the thread that made them: `proc_macro2`
//! marks `Span` neither [`Send`] nor [`Sync`] on purpose, because in the fallback
//! build a span is an index into a thread-local table of source files. So a
//! parse tree cannot be put in a static and shared, and a thread-local cache
//! would pay the whole cost once per test thread. The answer is not a cleverer
//! cache: it is to **stop holding the trees**.
//!
//! [`Index::build`] parses once per universe root, lowers what the parse knows
//! into owned, plain data, and drops every `syn` and `proc_macro2` object before
//! it returns. What comes back holds:
//!
//! | Held | Where |
//! | --- | --- |
//! | the union of every file's text, with each file's boundaries in it | [`Index::union`], [`FileRecord::span`] |
//! | numeric spans — byte offsets into the union, with file-relative line and column | [`Span`], [`Index::locate`] |
//! | item identities per §2.4, with their body spans | [`ItemRecord`], [`ItemIdentity`] |
//! | the declaration-ownership paths of §2.3, each with its `cfg` predicate spelling | [`FileRecord::owners`] |
//! | token classification for the identifier view — kind, span, identifier boundaries | [`TokenRecord`] |
//! | literal records — span, spelling, decoded value | [`LiteralRecord`] |
//! | comment and doc-comment masks | [`CommentRecord`] |
//! | the unsupported-macro-shape report of §2.7 | [`Index::unsupported_macro_shapes`] |
//!
//! **`Send + Sync` is the proof that no parser type is inside.** A `syn` tree
//! that survived the lowering would make the whole index neither, so the
//! compile-time assertion in `tests/index.rs` is not a formality: it is the
//! check that this module did the one thing it exists to do.
//!
//! Offsets are stored as `u32`. The union of the largest universe in this
//! workspace is 23 MB and a `u32` pair halves what 870,000 identifier records
//! cost; a union that outgrew the type is [`Rejection::UnionTooLarge`] rather
//! than a wrap.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::enumerate::{FileOwner, FileSetDiff};
use crate::reject::Rejection;
use crate::universe::Universe;

/// A half-open byte range in the union of [`Index::union`].
///
/// One coordinate space for every fact in the index, so a token's position, an
/// item's body and a comment's extent are directly comparable without anybody
/// remembering which file each came from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    start: u32,
    end: u32,
}

impl Span {
    pub(crate) const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn start(self) -> usize {
        self.start as usize
    }

    #[must_use]
    pub const fn end(self) -> usize {
        self.end as usize
    }

    #[must_use]
    pub const fn len(self) -> usize {
        (self.end - self.start) as usize
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Whether `offset` is one of this span's bytes.
    #[must_use]
    pub const fn holds(self, offset: usize) -> bool {
        offset >= self.start as usize && offset < self.end as usize
    }

    /// Whether the two ranges share a byte. An empty span shares none.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Whether every byte of this span is a byte of `other`.
    #[must_use]
    pub const fn within(self, other: Self) -> bool {
        self.start >= other.start && self.end <= other.end
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}..{}", self.start, self.end)
    }
}

/// One file of the universe: its text's place in the union, and who declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRecord {
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
    /// File-relative byte offsets of the start of each line, first entry `0`.
    /// This is the "file-relative line and column" of §5's table, kept in the
    /// one form that costs four bytes a line instead of a pair per span.
    pub(crate) line_starts: Vec<u32>,
    pub(crate) owners: Vec<FileOwner>,
}

impl FileRecord {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's own bytes, as a range of the union.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub fn lines(&self) -> usize {
        self.line_starts.len()
    }

    /// Every path by which a declaration reaches this file (plan §2.3), each
    /// carrying the `cfg` predicate spellings written along the way.
    #[must_use]
    pub fn owners(&self) -> &[FileOwner] {
        &self.owners
    }

    /// Whether **any** owning path permits product compilation.
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

    /// The file-relative line and column of a union offset inside this file.
    ///
    /// One-based on both, and the column counts characters rather than bytes so
    /// that it means what `rustc` and [`crate::Position`] mean by one.
    fn locate(&self, union: &str, offset: usize) -> Option<(usize, usize)> {
        if !self.span.holds(offset) {
            return None;
        }
        let local = u32::try_from(offset - self.span.start()).ok()?;
        let line = self.line_starts.partition_point(|start| *start <= local) - 1;
        let line_start = self.span.start() + self.line_starts[line] as usize;
        let column = union.get(line_start..offset)?.chars().count() + 1;
        Some((line + 1, column))
    }

    /// The union offset of a one-based line and column inside this file — the
    /// inverse of [`FileRecord::locate`], for reading a `line!()`/`column!()`
    /// pair back into a position (§2.6).
    ///
    /// The column counts characters, because that is what both this file's
    /// `locate` and the compiler's own `column!()` count.
    pub(crate) fn offset_at(&self, union: &str, line: usize, column: usize) -> Option<usize> {
        let line_start = self.span.start() + *self.line_starts.get(line.checked_sub(1)?)? as usize;
        let rest = union.get(line_start..self.span.end())?;
        let mut characters = rest.char_indices();
        let offset = match column.checked_sub(1)? {
            0 => 0,
            skip => characters.nth(skip).map(|(at, _)| at)?,
        };
        Some(line_start + offset)
    }
}

/// Where a byte is, said the way a compiler says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location<'a> {
    pub file: &'a Path,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Location<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}",
            self.file.display(),
            self.line,
            self.column
        )
    }
}

/// What a callable is declared as. Identity distinguishes them (§2.4) and a
/// query may not: `Runtime::present` and a free `present` are different things.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKind {
    /// `fn f(…)` written at module level.
    Function,
    /// `fn f(…)` written inside an `impl` block or a trait definition.
    AssociatedFunction,
}

/// The conditional arm a declaration stands on, **as it is spelled**.
///
/// Eleven callable identities in `bt-app/src` are declared twice under mutually
/// exclusive conditions (§2.4), and the predicate is what tells the two apart.
/// It is the spelling and not the parser's re-printing, for the reason
/// [`crate::DeclarationStep::predicates`] gives: a query is about what the code
/// says.
///
/// The predicates are those written **inside the file**, outermost first — on an
/// enclosing inline `mod`, on the `impl` block, and on the item itself. The
/// predicates on the declarations that reach the file are a property of the path
/// to the file and live on [`FileRecord::owners`].
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConditionalVariant {
    pub(crate) predicates: Vec<String>,
}

impl ConditionalVariant {
    #[must_use]
    pub fn predicates(&self) -> &[String] {
        &self.predicates
    }

    #[must_use]
    pub fn is_unconditional(&self) -> bool {
        self.predicates.is_empty()
    }
}

impl fmt::Display for ConditionalVariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for predicate in &self.predicates {
            write!(formatter, "#[cfg({predicate})]")?;
        }
        Ok(())
    }
}

/// The tuple of plan §2.4: module path · type owner · trait · conditional
/// variant, and the name the four of them qualify.
///
/// "The full module path is unique" is false in this tree, which is why the
/// last component is here at all.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemIdentity {
    pub module_path: String,
    /// The type an `impl` block is for, printed as the **last segment** of its
    /// path without lifetimes or generic arguments, so `Runtime<'_>`,
    /// `Runtime<'a>` and `crate::Runtime<'_>` are one type. `None` for a free
    /// function.
    ///
    /// The qualification is dropped because a move is what writes it, and
    /// because [`ItemIdentity::module_path`] beside it already says where the
    /// `impl` is — printing both gave `crate::runtime::crate::Runtime::…`.
    pub type_owner: Option<String>,
    /// The trait, for a trait `impl` or a trait's own default method. `None` for
    /// an inherent `impl`.
    pub trait_name: Option<String>,
    pub name: String,
    pub kind: ItemKind,
    pub variant: ConditionalVariant,
}

impl fmt::Display for ItemIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}::", self.module_path)?;
        match (&self.type_owner, &self.trait_name) {
            (Some(owner), Some(trait_name)) => write!(formatter, "<{owner} as {trait_name}>::")?,
            (Some(owner), None) => write!(formatter, "{owner}::")?,
            (None, Some(trait_name)) => write!(formatter, "{trait_name}::")?,
            (None, None) => {}
        }
        write!(formatter, "{}", self.name)?;
        if !self.variant.is_unconditional() {
            write!(formatter, " {}", self.variant)?;
        }
        Ok(())
    }
}

/// One callable, lowered: who it is, and where its bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRecord {
    pub(crate) file: usize,
    /// One per owning declaration path of the file (§2.3). A file reached both
    /// ways has two module paths and therefore two identities for the same
    /// bytes; that is a fact about the tree and not a duplication.
    pub(crate) module_paths: Vec<String>,
    pub(crate) type_owner: Option<String>,
    pub(crate) trait_name: Option<String>,
    pub(crate) name: String,
    pub(crate) kind: ItemKind,
    pub(crate) variant: ConditionalVariant,
    /// Everything from the first attribute to the closing brace or semicolon.
    pub(crate) whole: Span,
    /// The braces and what is between them. `None` for a trait method that
    /// declares a signature and no default body.
    pub(crate) body: Option<Span>,
}

impl ItemRecord {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn type_owner(&self) -> Option<&str> {
        self.type_owner.as_deref()
    }

    #[must_use]
    pub fn trait_name(&self) -> Option<&str> {
        self.trait_name.as_deref()
    }

    #[must_use]
    pub const fn kind(&self) -> ItemKind {
        self.kind
    }

    #[must_use]
    pub const fn variant(&self) -> &ConditionalVariant {
        &self.variant
    }

    #[must_use]
    pub fn module_paths(&self) -> &[String] {
        &self.module_paths
    }

    #[must_use]
    pub const fn whole(&self) -> Span {
        self.whole
    }

    #[must_use]
    pub const fn body(&self) -> Option<Span> {
        self.body
    }

    /// **The declaration itself: everything before the body.**
    ///
    /// This is what §2.5's declaration exemption excludes — the attributes, the
    /// visibility, the signature and the name in it — and not the body, because
    /// a name written *inside* the item that declares it is an occurrence like
    /// any other (a recursive call is the plain case). The guard this rule comes
    /// from refuses "a match whose preceding text ends with the declaration
    /// keywords", which is this span and nothing wider.
    #[must_use]
    pub const fn declaration(&self) -> Span {
        match self.body {
            Some(body) => Span::new(self.whole.start, body.start),
            None => self.whole,
        }
    }

    /// Every identity these bytes answer to — one per owning module path.
    pub fn identities(&self) -> impl Iterator<Item = ItemIdentity> + '_ {
        self.module_paths.iter().map(|module_path| ItemIdentity {
            module_path: module_path.clone(),
            type_owner: self.type_owner.clone(),
            trait_name: self.trait_name.clone(),
            name: self.name.clone(),
            kind: self.kind,
            variant: self.variant.clone(),
        })
    }
}

/// What kind of name a token carries.
///
/// Only the identifier view's tokens are kept. A punctuation mark and a
/// delimiter are positions the index does not need and 3.6 million records it
/// would otherwise hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenKind {
    /// `present`, and also a keyword — the lexer makes no distinction and
    /// neither does a query for a name.
    Identifier,
    /// `r#type`: the span covers the `r#`, the name does not.
    RawIdentifier,
    /// `'a`: the span covers the tick, the name does not.
    Lifetime,
}

/// One name, classified, with the boundaries of the name itself.
///
/// **The boundaries are why this exists.** `bt-platform`'s stand-in guard
/// (§2.5) refuses a match whose neighbour is alphanumeric or `_`, because
/// `strip_stand_in` is not `stand_in`; a token reading gets that for nothing,
/// and a substring reading cannot express it at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenRecord {
    pub(crate) span: Span,
    pub(crate) kind: TokenKind,
}

impl TokenRecord {
    /// The whole token, `r#` or leading tick included.
    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn kind(self) -> TokenKind {
        self.kind
    }

    /// The name's own bytes: the token minus the spelling that is not part of
    /// the name.
    #[must_use]
    pub const fn name_span(self) -> Span {
        let skip = match self.kind {
            TokenKind::Identifier => 0,
            TokenKind::RawIdentifier => 2,
            TokenKind::Lifetime => 1,
        };
        Span::new(self.span.start + skip, self.span.end)
    }
}

/// What a literal decodes to. **Spelling is not value** (§2.1): the spelling is
/// the union slice at the record's span, and this is the other fact.
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValue {
    Str(String),
    ByteStr(Vec<u8>),
    CStr(Vec<u8>),
    Char(char),
    Byte(u8),
    /// The base-ten digits an integer decodes to, whatever base it was written
    /// in. `0xFF` is `255` here and `0xFF` in the union.
    Int(String),
    Float(String),
    Bool(bool),
    /// A literal shape the parser accepted and this crate has no decoding for.
    /// Never silently the empty string.
    Undecoded,
}

/// One literal: where it is written, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct LiteralRecord {
    pub(crate) span: Span,
    pub(crate) value: LiteralValue,
}

impl LiteralRecord {
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn value(&self) -> &LiteralValue {
        &self.value
    }
}

/// Which kind of removed region a mask covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommentKind {
    /// `// …`
    Line,
    /// `/* … */`, nesting included.
    Block,
    /// `/// …`, `//! …`, `/** … */`, `/*! … */`.
    DocComment,
    /// A `#[doc = "…"]` or `#![doc = "…"]` somebody wrote out. §2.1 removes it
    /// from [`crate::View::CodeKeepingLiterals`] with the comments, because it
    /// is one.
    DocAttribute,
}

/// One removed region of [`crate::View::CodeKeepingLiterals`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommentRecord {
    pub(crate) span: Span,
    pub(crate) kind: CommentKind,
}

impl CommentRecord {
    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn kind(self) -> CommentKind {
        self.kind
    }
}

/// One module, and the bytes it is written in — the named scopes of §2.4/§3.
///
/// A scope is a **Rust path**, never a file, so it has to be resolvable to bytes
/// without anybody naming one: a module whose body is a file is that file's
/// bytes, and an inline `mod x { … }` is its braces and what is between them.
/// The day the file splits, the path is unchanged and so is the scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRecord {
    pub(crate) file: usize,
    /// One per owning declaration path of the file (§2.3).
    pub(crate) module_paths: Vec<String>,
    pub(crate) span: Span,
    pub(crate) body: ModuleShape,
}

/// Where a module's bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleShape {
    /// `mod x;` — the span is the whole file.
    WholeFile,
    /// `mod x { … }` — the span is the braces and what is between them.
    Inline,
}

impl ModuleRecord {
    #[must_use]
    pub fn module_paths(&self) -> &[String] {
        &self.module_paths
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn body(&self) -> ModuleShape {
        self.body
    }
}

/// Whether the parser could say what a stretch of bytes is, or only the lexer
/// (§2.7).
///
/// **Not a confidence.** `Resolved` says the bytes are in a position the parser
/// structured; `Candidate` says they are inside a macro's token tree, where the
/// same tokens may be pasted into an item, dropped, or turned into something
/// else entirely. Name *resolution* is not claimed by either: no query in this
/// crate resolves a path to a definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Certainty {
    Resolved,
    Candidate,
}

/// One macro invocation, or one `macro_rules!` definition, found lexically.
///
/// `syn`'s visitor does not descend into a macro's token tree — the
/// token-stream visit has an empty default body — so a reader built on the
/// parse alone cannot see a name inside `println!`. The lexer does see it, and
/// these records are what say afterwards *which* names were found there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroRecord {
    pub(crate) span: Span,
    /// The token tree between the delimiters — the bytes whose classification
    /// is [`Certainty::Candidate`].
    pub(crate) tokens: Span,
    /// `println`, `bt_source::needle`, `macro_rules`.
    pub(crate) path: String,
    pub(crate) kind: MacroKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MacroKind {
    /// `name!(…)`, `name![…]`, `name!{…}`.
    Invocation,
    /// `macro_rules! name { … }`.
    Definition,
}

impl MacroRecord {
    /// The whole of it: the path, the `!`, the delimiters and what is inside.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// What is between the delimiters.
    #[must_use]
    pub const fn tokens(&self) -> Span {
        self.tokens
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub const fn kind(&self) -> MacroKind {
        self.kind
    }

    /// The last segment of the path — `println` for `std::println`.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit("::").next().unwrap_or(&self.path)
    }
}

/// An executable macro shape §2.7 refuses to examine silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedMacroShape {
    pub span: Span,
    pub shape: MacroShape,
    /// The invocation as it is written.
    pub spelling: String,
}

/// The shapes of §2.7 — an attribute or derive macro that replaces an item
/// body, an arm that constructs an item, a source inclusion.
///
/// Each is a way the bytes this index holds are **not** the program the compiler
/// sees. None is a failure; all are reported, because a gap nobody wrote down is
/// the one that gets trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MacroShape {
    /// An attribute whose path is not one the language defines — it may be an
    /// attribute macro, and an attribute macro may rewrite the item under it.
    /// Telling one from an inert helper attribute (`#[serde(…)]` is the case in
    /// this tree) needs name resolution, which this reading does not do, so both
    /// are reported.
    AttributeReplacingABody,
    /// A `derive` naming something other than the language's own derives: it
    /// generates items no byte of this index holds.
    DeriveReplacingABody,
    /// A `macro_rules!` arm whose expansion opens an item — `fn`, `impl`,
    /// `struct`, `mod` and the rest. The item it makes is not in the index, so
    /// a query for that item would answer "not declared".
    ItemConstructingArm,
    /// `include!` — Rust source pulled in from elsewhere. (`include_str!` and
    /// `include_bytes!` carry data, not declarations, and are not this.)
    SourceInclusion,
    /// `module_path!` — a module path this walk computed differently.
    ModulePath,
    /// `compile_error!`.
    CompileError,
    /// `line!`, `column!`, `file!` — the source's own coordinates, read at
    /// compile time.
    LineNumber,
}

/// **One universe, lowered.** Owned, plain, immutable, [`Send`] and [`Sync`].
pub struct Index {
    pub(crate) universe: Universe,
    pub(crate) union: String,
    pub(crate) files: Vec<FileRecord>,
    pub(crate) by_path: BTreeMap<PathBuf, usize>,
    pub(crate) items: Vec<ItemRecord>,
    pub(crate) modules: Vec<ModuleRecord>,
    pub(crate) tokens: Vec<TokenRecord>,
    pub(crate) literals: Vec<LiteralRecord>,
    pub(crate) comments: Vec<CommentRecord>,
    pub(crate) macros: Vec<MacroRecord>,
    pub(crate) cross_check: FileSetDiff,
    pub(crate) macro_shapes: Vec<UnsupportedMacroShape>,
}

/// Counts and never contents: a derived `Debug` would print 23 MB of source
/// into an assertion message.
impl fmt::Debug for Index {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Index")
            .field("universe", &self.universe.name())
            .field("bytes", &self.union.len())
            .field("files", &self.files.len())
            .field("items", &self.items.len())
            .field("tokens", &self.tokens.len())
            .field("literals", &self.literals.len())
            .field("comments", &self.comments.len())
            .field("macros", &self.macros.len())
            .field("unsupported_macro_shapes", &self.macro_shapes.len())
            .finish()
    }
}

impl Index {
    #[must_use]
    pub const fn universe(&self) -> &Universe {
        &self.universe
    }

    /// Every file's text, concatenated in path order. Nothing stands between two
    /// files, so a file's span slices to exactly the bytes on the disk — and a
    /// query never searches across the join, because it searches file by file
    /// (§2.2 rule 3).
    #[must_use]
    pub fn union(&self) -> &str {
        &self.union
    }

    #[must_use]
    pub fn files(&self) -> &[FileRecord] {
        &self.files
    }

    #[must_use]
    pub fn file(&self, path: &Path) -> Option<&FileRecord> {
        let at = self.by_path.get(&crate::paths::normalized(path))?;
        self.files.get(*at)
    }

    /// The file a union offset belongs to.
    #[must_use]
    pub fn file_at(&self, offset: usize) -> Option<&FileRecord> {
        let at = self
            .files
            .partition_point(|file| file.span.start() <= offset);
        let file = self.files.get(at.checked_sub(1)?)?;
        file.span.holds(offset).then_some(file)
    }

    /// The file holding this record.
    ///
    /// # Panics
    ///
    /// Never: a record is built with the position of the file it was read from.
    #[must_use]
    pub fn file_of(&self, item: &ItemRecord) -> &FileRecord {
        self.files
            .get(item.file)
            .expect("every item names a file of this index")
    }

    /// Where a union offset is, as a compiler would say it.
    #[must_use]
    pub fn locate(&self, offset: usize) -> Option<Location<'_>> {
        let file = self.file_at(offset)?;
        let (line, column) = file.locate(&self.union, offset)?;
        Some(Location {
            file: &file.path,
            line,
            column,
        })
    }

    /// The bytes a span covers.
    #[must_use]
    pub fn text(&self, span: Span) -> &str {
        &self.union[span.start()..span.end()]
    }

    #[must_use]
    pub fn items(&self) -> &[ItemRecord] {
        &self.items
    }

    /// Every identifier-view token, in union order.
    #[must_use]
    pub fn tokens(&self) -> &[TokenRecord] {
        &self.tokens
    }

    #[must_use]
    pub fn literals(&self) -> &[LiteralRecord] {
        &self.literals
    }

    /// The removed regions of [`crate::View::CodeKeepingLiterals`], in union
    /// order.
    #[must_use]
    pub fn comments(&self) -> &[CommentRecord] {
        &self.comments
    }

    /// The declared file set beside the disk file set, as the universe ran.
    #[must_use]
    pub const fn cross_check(&self) -> &FileSetDiff {
        &self.cross_check
    }

    /// Every module reached, with the bytes it is written in — what a named
    /// scope resolves against.
    #[must_use]
    pub fn modules(&self) -> &[ModuleRecord] {
        &self.modules
    }

    /// Every macro invocation and `macro_rules!` definition, in union order.
    #[must_use]
    pub fn macros(&self) -> &[MacroRecord] {
        &self.macros
    }

    /// The report of §2.7. **Empty means the traversal found nothing it cannot
    /// classify**, which is a different statement from P1b's, where the
    /// traversal had not been made at all.
    #[must_use]
    pub fn unsupported_macro_shapes(&self) -> &[UnsupportedMacroShape] {
        &self.macro_shapes
    }

    /// Whether `span` lies inside a macro's token tree (§2.7).
    ///
    /// The scan runs backwards over every macro that begins at or before the
    /// span, because macro token trees nest and an inner one ending early says
    /// nothing about the outer one still being open.
    #[must_use]
    pub fn certainty(&self, span: Span) -> Certainty {
        let after = self
            .macros
            .partition_point(|record| record.tokens.start() <= span.start());
        if self.macros[..after]
            .iter()
            .rev()
            .any(|record| span.within(record.tokens))
        {
            Certainty::Candidate
        } else {
            Certainty::Resolved
        }
    }

    /// Whether any masked region overlaps `span` (§2.2 rule 2: masked bytes are
    /// opaque, so a match may neither sit inside one nor cross it).
    /// The removed region overlapping `span`, if there is one.
    pub(crate) fn mask_at(&self, span: Span) -> Option<Span> {
        let after = self
            .comments
            .partition_point(|comment| comment.span.start() < span.end());
        self.comments[..after]
            .iter()
            .rev()
            .take_while(|comment| comment.span.end() > span.start())
            .find(|comment| comment.span.overlaps(span))
            .map(|comment| comment.span)
    }

    pub(crate) fn is_masked(&self, span: Span) -> bool {
        // The masks are disjoint and sorted by start, so the one to look at is
        // the last that begins before this span ends; the loop walks back only
        // while a mask could still reach `span`, which for disjoint ranges is
        // one step.
        let after = self
            .comments
            .partition_point(|comment| comment.span.start() < span.end());
        self.comments[..after]
            .iter()
            .rev()
            .take_while(|comment| comment.span.end() > span.start())
            .any(|comment| comment.span.overlaps(span))
    }

    /// **Every owned byte this index holds**, counted rather than sampled.
    ///
    /// This is an accounting of the index's own allocations — the union, every
    /// vector's capacity, and every string inside them — and not the process's
    /// resident set: allocator bookkeeping and fragmentation are outside it, and
    /// so is everything the test harness around it allocates. It is reported as
    /// what it is.
    #[must_use]
    pub fn footprint_bytes(&self) -> usize {
        let mut total = size_of::<Self>() + self.union.capacity();
        total += self.files.capacity() * size_of::<FileRecord>();
        for file in &self.files {
            total += file.path.as_os_str().len();
            total += file.line_starts.capacity() * size_of::<u32>();
            total += file.owners.capacity() * size_of::<FileOwner>();
            for owner in &file.owners {
                total += owner.target.package.capacity() + owner.target.name.capacity();
                total += owner.module_path.capacity();
                total += owner.steps.capacity() * size_of::<crate::DeclarationStep>();
                for step in &owner.steps {
                    total += step.module.capacity() + step.declared_in.as_os_str().len();
                    total += step.predicates.capacity() * size_of::<String>();
                    total += step.predicates.iter().map(String::capacity).sum::<usize>();
                }
            }
        }
        for path in self.by_path.keys() {
            total += path.as_os_str().len() + size_of::<PathBuf>() + size_of::<usize>();
        }
        total += self.items.capacity() * size_of::<ItemRecord>();
        for item in &self.items {
            total += item.module_paths.capacity() * size_of::<String>();
            total += item
                .module_paths
                .iter()
                .map(String::capacity)
                .sum::<usize>();
            total += item.type_owner.as_ref().map_or(0, String::capacity);
            total += item.trait_name.as_ref().map_or(0, String::capacity);
            total += item.name.capacity();
            total += item.variant.predicates.capacity() * size_of::<String>();
            total += item
                .variant
                .predicates
                .iter()
                .map(String::capacity)
                .sum::<usize>();
        }
        total += self.modules.capacity() * size_of::<ModuleRecord>();
        for module in &self.modules {
            total += module.module_paths.capacity() * size_of::<String>();
            total += module
                .module_paths
                .iter()
                .map(String::capacity)
                .sum::<usize>();
        }
        total += self.macros.capacity() * size_of::<MacroRecord>();
        total += self
            .macros
            .iter()
            .map(|it| it.path.capacity())
            .sum::<usize>();
        total += self.tokens.capacity() * size_of::<TokenRecord>();
        total += self.comments.capacity() * size_of::<CommentRecord>();
        total += self.literals.capacity() * size_of::<LiteralRecord>();
        for literal in &self.literals {
            total += match &literal.value {
                LiteralValue::Str(text) | LiteralValue::Int(text) | LiteralValue::Float(text) => {
                    text.capacity()
                }
                LiteralValue::ByteStr(bytes) | LiteralValue::CStr(bytes) => bytes.capacity(),
                LiteralValue::Char(_)
                | LiteralValue::Byte(_)
                | LiteralValue::Bool(_)
                | LiteralValue::Undecoded => 0,
            };
        }
        total
    }

    /// Lower `universe` into an index of its own, parsing every file once.
    ///
    /// # Errors
    ///
    /// Everything [`crate::enumerate`] refuses, plus [`Rejection::UnionTooLarge`].
    pub fn build(universe: &Universe) -> Result<Self, Vec<Rejection>> {
        crate::lower::build(universe)
    }

    /// The one index this process holds for `universe`, built on first ask.
    ///
    /// # Errors
    ///
    /// Whatever [`Index::build`] refuses, handed to every later caller too — a
    /// universe that cannot be lowered does not become lowerable on the second
    /// try, and refusing again is cheaper and says the same thing.
    pub fn shared(universe: &Universe) -> Result<std::sync::Arc<Self>, Vec<Rejection>> {
        crate::cache::shared(universe)
    }

    /// **One workspace package's own sources, indexed once per process** — the
    /// one call a migrated guard makes before it asks anything.
    ///
    /// `package` is a member's package name, `"bt-app"`. The universe is
    /// [`crate::universes::crate_sources`] with [`crate::Vendor::Excluded`]:
    /// that package's own `src/`, reached through the `mod` declarations of
    /// every target it compiles, `cfg`-blind. It is the same index
    /// [`Index::shared`] hands out for that universe, so a reader that declares
    /// it the long way shares this one rather than lowering a second.
    ///
    /// # What a consumer has to read
    ///
    /// **This entry, [`crate::ItemQuery`] and [`crate::Search`]** — that is the
    /// whole of it, and it is the point of the preparation. A body pin is
    ///
    /// ```text
    /// Index::of_package("bt-app").body_of(&ItemQuery::method("Runtime", "turn"))
    /// ```
    ///
    /// and a count or a negative is
    ///
    /// ```text
    /// Index::of_package("bt-app").search(&Search::new(needle!(pattern), View::Raw))
    /// ```
    ///
    /// where [`crate::Search`] brings [`crate::Pattern`], [`crate::View`] and
    /// [`crate::needle`] with it. The workspace, the package, the universe and
    /// the lowering are behind this call; a module that asks one question does
    /// not name any of them.
    ///
    /// **What it does not decide for the caller.** Which refusals of a *query*
    /// are fatal, and what "in the product" means for a given reader, stay with
    /// the reader: [`Index::body_of`] and [`Index::search`] return a
    /// [`crate::QueryFailure`], and file-grained product reachability is
    /// [`FileRecord::permits_product`] over [`Index::file_at`]. This entry is
    /// loud about the universe it was asked for and silent about everything
    /// after it.
    ///
    /// # Panics
    ///
    /// When the workspace cannot be read, when it has no package of that name
    /// ([`Rejection::NoSuchPackage`]), when the universe is refused, or when the
    /// lowering is — each with the [`Rejection`]s that refused it. A universe
    /// that cannot be declared does not become declarable on the second ask, so
    /// a later caller is told the same thing rather than handed a smaller
    /// answer. A vendored package is one of these refusals
    /// ([`Rejection::VendorNotDeclared`]): a reading that covers upstream code
    /// is a decision, so it declares its own universe and calls
    /// [`Index::shared`].
    #[must_use]
    pub fn of_package(package: &str) -> &'static Self {
        crate::cache::of_package(package)
    }
}
