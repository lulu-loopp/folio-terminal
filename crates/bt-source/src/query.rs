//! The queries P1b needs, and no more.
//!
//! P1c owns the reading contract of §2 in full — needle provenance, named
//! scopes, declaration exemption, macro candidates, expected multiplicity for
//! the eleven conditional identities. What is here is what the measurement of
//! §5 asks for: a body lookup, an identifier count over the union, a
//! whole-source negative, and an owner-set query. Each is a lookup over plain
//! data, and each fails loudly rather than answering a different question.
//!
//! **The two rules that are already ruled are in force here.**
//!
//! * A unique query that finds none, or more than it was told to expect, is a
//!   [`QueryFailure`] naming every candidate with its file, its position and its
//!   predicate (§2.4). Zero is the dangerous one: a guard whose subject has
//!   moved and whose reader answers "not found" is the exact failure this
//!   preparation exists to remove.
//! * A match may not cross a file boundary (§2.2 rule 3), and it may not cross
//!   a masked region (rule 2). The first is by construction — every search runs
//!   file by file over the union — and the second is [`Index::is_masked`].

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

use crate::enumerate::FileOwner;
use crate::index::{Index, ItemIdentity, ItemRecord, LiteralValue, Span};

/// Which bytes a query reads. **There is no default** (§2.1): the tree holds
/// five readers whose subject lives inside a string literal, and a
/// literal-stripping default would have turned every one of them quietly green.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum View {
    /// Every byte of the enumerated source, unchanged.
    Raw,
    /// Comments and doc comments removed; string, raw-string, byte-string and
    /// character literals preserved verbatim.
    CodeKeepingLiterals,
    /// The names, as the lexer classified them — boundary-checked by
    /// construction, and reaching inside macro token trees.
    Identifiers,
    /// What the literals decode to, which is a different fact from how they are
    /// written.
    LiteralValues,
}

/// One item, asked for by the tuple of §2.4.
///
/// `expected` is **an argument, not an inference**: a query states how many
/// conditional variants it expects — one for almost everything, two for the
/// eleven identities `bt-app` declares twice — and a different number is a
/// failure that names them all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemQuery {
    name: String,
    owner: Owner,
    declaring: Declaring,
    module_path: Option<String>,
    expected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Owner {
    /// Written at module level, belonging to no type.
    Free,
    /// Written in an `impl` block for this type, lifetimes and generic
    /// arguments ignored.
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
            expected: 1,
        }
    }

    /// A method of `type_owner`'s inherent `impl` blocks — the narrowing §2.4
    /// gives `method_body(self_ty, name)`, where `Runtime<'_>` and `Runtime<'a>`
    /// are the same type.
    #[must_use]
    pub fn method(type_owner: &str, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            owner: Owner::Type(type_owner.to_owned()),
            declaring: Declaring::Inherent,
            module_path: None,
            expected: 1,
        }
    }

    /// Narrow to one trait's implementation, or to the trait's own default.
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

    /// How many conditional variants this identity is expected to have.
    #[must_use]
    pub const fn expecting(mut self, variants: usize) -> Self {
        self.expected = variants;
        self
    }

    #[must_use]
    pub const fn expected(&self) -> usize {
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
            Declaring::Trait(wanted) => record.trait_name() == Some(wanted.as_str()),
        };
        let module = self.module_path.as_ref().is_none_or(|wanted| {
            record
                .module_paths()
                .iter()
                .any(|path| path.as_str() == wanted.as_str())
        });
        owner && declaring && module
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
        write!(formatter, "{}", self.name)
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryFailure {
    /// The number of declarations is not the number the query said to expect.
    Multiplicity {
        query: String,
        expected: usize,
        found: Vec<Candidate>,
        /// Declarations carrying the same name that the rest of the query ruled
        /// out. Printed when nothing matched, because "not found" plus the six
        /// things that nearly were is a diagnosis and "not found" alone is not.
        near: Vec<Candidate>,
    },
    /// The identity resolved, and it is a signature with no body to return.
    NoBody { query: String, at: Vec<Candidate> },
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
                    "`{query}` was asked for {expected} declaration(s) and {} answer(s)",
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
            Self::NoBody { query, at } => {
                write!(formatter, "`{query}` has no body to read")?;
                for candidate in at {
                    write!(
                        formatter,
                        "
  declared: {candidate}"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for QueryFailure {}

impl Index {
    /// Every declaration of `query`, refusing unless there are exactly as many
    /// as the query expects.
    ///
    /// # Errors
    ///
    /// [`QueryFailure::Multiplicity`], naming every candidate.
    pub fn find(&self, query: &ItemQuery) -> Result<Vec<&ItemRecord>, QueryFailure> {
        let found: Vec<&ItemRecord> = self
            .items()
            .iter()
            .filter(|record| query.selects(record))
            .collect();
        if found.len() == query.expected() {
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
            expected: query.expected(),
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
            expected: 1,
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

    /// Every occurrence of `pattern`, reported at its [`View::Raw`] offsets
    /// whichever view found it.
    #[must_use]
    pub fn occurrences(&self, pattern: &str, view: View) -> Vec<Span> {
        if pattern.is_empty() {
            return Vec::new();
        }
        match view {
            View::Raw | View::CodeKeepingLiterals => {
                let mut found = Vec::new();
                for file in self.files() {
                    let text = self.text(file.span());
                    for (at, _) in text.match_indices(pattern) {
                        let start = file.span().start() + at;
                        let span = span_of(start, start + pattern.len());
                        if view == View::Raw || !self.is_masked(span) {
                            found.push(span);
                        }
                    }
                }
                found
            }
            View::Identifiers => self
                .tokens()
                .iter()
                .flat_map(|token| {
                    let name = token.name_span();
                    self.text(name)
                        .match_indices(pattern)
                        .map(|(at, _)| {
                            span_of(name.start() + at, name.start() + at + pattern.len())
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
            View::LiteralValues => self
                .literals()
                .iter()
                .filter(|literal| {
                    value_text(literal.value()).is_some_and(|text| text.contains(pattern))
                })
                .map(|literal| literal.span())
                .collect(),
        }
    }

    /// Whether `pattern` occurs at all — the whole-source negative every
    /// prohibition in this tree is written as.
    #[must_use]
    pub fn contains(&self, pattern: &str, view: View) -> bool {
        !self.occurrences(pattern, view).is_empty()
    }

    #[must_use]
    pub fn count(&self, pattern: &str, view: View) -> usize {
        self.occurrences(pattern, view).len()
    }

    /// Every occurrence of `name` **as a whole identifier**.
    ///
    /// In [`View::Identifiers`] that is the token reading and the boundaries
    /// come for nothing. In the byte views it is an occurrence whose neighbours
    /// on both sides are not identifier characters, which is the check §2.5
    /// says a one-line substring count is not equivalent to.
    #[must_use]
    pub fn identifier_occurrences(&self, name: &str, view: View) -> Vec<Span> {
        if name.is_empty() {
            return Vec::new();
        }
        match view {
            View::Identifiers => self
                .tokens()
                .iter()
                .filter(|token| self.text(token.name_span()) == name)
                .map(|token| token.name_span())
                .collect(),
            View::Raw | View::CodeKeepingLiterals => self
                .occurrences(name, view)
                .into_iter()
                .filter(|span| {
                    let file = self
                        .file_at(span.start())
                        .expect("a match lies inside the file it was found in");
                    bounded(
                        self.text(file.span()),
                        span.start() - file.span().start(),
                        span.end() - file.span().start(),
                    )
                })
                .collect(),
            View::LiteralValues => self
                .literals()
                .iter()
                .filter(|literal| {
                    value_text(literal.value()).is_some_and(|text| {
                        text.match_indices(name)
                            .any(|(at, _)| bounded(&text, at, at + name.len()))
                    })
                })
                .map(|literal| literal.span())
                .collect(),
        }
    }

    #[must_use]
    pub fn count_identifier(&self, name: &str, view: View) -> usize {
        self.identifier_occurrences(name, view).len()
    }
}

/// Whether the characters on both sides of `[start, end)` are outside an
/// identifier. `Runtime::strip_stand_in` is not an occurrence of `stand_in`.
fn bounded(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_identifier_character) && !after.is_some_and(is_identifier_character)
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
