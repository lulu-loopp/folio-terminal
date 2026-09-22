//! The lowering: one parse per file, and no parser object outlives it.
//!
//! Three readings are taken of each file and all three land in plain numbers:
//!
//! 1. **The lexer**, `proc_macro2::TokenStream::from_str`, over the file's own
//!    text. It gives every name and every literal a byte range in that text, and
//!    it descends into macro invocations' token trees for nothing — which is the
//!    coverage a text search has today and §2.7 refuses to lose.
//! 2. **The gaps between tokens.** The lexer covers every byte that is not
//!    whitespace or a comment, so what is left between two tokens is whitespace
//!    and comments and nothing else — no string, no character literal, no raw
//!    string to be fooled by. A hand-written comment scanner has to know the
//!    whole lexical grammar to avoid calling `"// not a comment"` a comment;
//!    this one has to know only `//` and `/* */`, because the lexer has already
//!    ruled out everywhere it could be wrong.
//! 3. **The parse**, for item identity: module path, type owner, trait,
//!    conditional variant, and the body's braces.
//!
//! Doc comments are the one place the first two readings meet. `proc_macro2`
//! turns `/// x` into the tokens of `#[doc = " x"]`, and every one of those
//! tokens carries the byte range of **the comment**, not of anything written at
//! that range. So a doc comment is not a gap, and its synthesized literal is not
//! a literal of this program: both are recognised by that shared range and the
//! whole shape is recorded as one mask.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use syn::spanned::Spanned;

use crate::declarations::cfg_predicates;
use crate::index::{
    CommentKind, CommentRecord, ConditionalVariant, FileRecord, Index, ItemKind, ItemRecord,
    LiteralRecord, LiteralValue, MacroKind, MacroRecord, MacroShape, ModuleRecord, ModuleShape,
    Span, TokenKind, TokenRecord, UnsupportedMacroShape,
};
use crate::reject::Rejection;
use crate::universe::Universe;

/// Lower `universe`, and drop every parser object on the way out.
pub(crate) fn build(universe: &Universe) -> Result<Index, Vec<Rejection>> {
    let (enumeration, unreached) = crate::enumerate(universe)?;
    // Spent by carrying it: the set travels into the index's own cross-check,
    // which is where a migrated walker's ticket reads it (§3.2).
    let carried = unreached.carried_forward();
    debug_assert_eq!(&carried, &enumeration.cross_check().only_on_disk);

    let paths: Vec<PathBuf> = enumeration.files().keys().cloned().collect();
    let total: usize = paths
        .iter()
        .map(|path| {
            std::fs::metadata(path).map_or(0, |data| usize::try_from(data.len()).unwrap_or(0))
        })
        .sum();
    if u32::try_from(total).is_err() {
        return Err(vec![Rejection::UnionTooLarge { bytes: total }]);
    }

    let mut union = String::with_capacity(total);
    let mut files: Vec<FileRecord> = Vec::with_capacity(paths.len());
    let mut by_path = BTreeMap::new();
    let mut tokens = Vec::new();
    let mut literals = Vec::new();
    let mut comments = Vec::new();
    let mut items = Vec::new();
    let mut modules = Vec::new();
    let mut macros = Vec::new();
    let mut shapes = Vec::new();
    let mut rejections = Vec::new();

    for path in &paths {
        // The file's own position is `files.len()` and never the loop index:
        // a file that cannot be read pushes a rejection and no record, and an
        // index taken from the loop would be one too high for every file after
        // it. The rejection is returned before the index is published either
        // way, so this is the invariant made structural rather than lucky.
        let at = files.len();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                rejections.push(Rejection::UnreadableFile {
                    file: path.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let base = union.len();
        if u32::try_from(base + text.len()).is_err() {
            return Err(vec![Rejection::UnionTooLarge {
                bytes: base + text.len(),
            }]);
        }
        union.push_str(&text);

        let owners = enumeration
            .file(path)
            .map(|facts| facts.owners().to_vec())
            .unwrap_or_default();
        let module_paths: Vec<String> = {
            let mut found: Vec<String> = owners
                .iter()
                .map(|owner| owner.module_path.clone())
                .collect();
            found.sort();
            found.dedup();
            found
        };
        let file_span = Span::new(at_union(base, 0), at_union(base, text.len()));
        files.push(FileRecord {
            path: path.clone(),
            span: file_span,
            line_starts: line_starts(&text),
            owners,
        });
        by_path.insert(path.clone(), at);
        // A module whose body is a file is that file's bytes — the other half
        // of a named scope, beside the inline modules the parse finds.
        modules.push(ModuleRecord {
            file: at,
            module_paths: module_paths.clone(),
            span: file_span,
            body: ModuleShape::WholeFile,
        });

        match TokenStream::from_str(&text) {
            Ok(stream) => {
                let mut lexed = Lexed {
                    text: &text,
                    base,
                    cursor: 0,
                    tokens: &mut tokens,
                    literals: &mut literals,
                    comments: &mut comments,
                    macros: &mut macros,
                    shapes: &mut shapes,
                };
                lexed.walk(stream);
                lexed.gap_to(text.len());
            }
            Err(error) => rejections.push(Rejection::UnparsableFile {
                file: path.clone(),
                reason: error.to_string(),
            }),
        }

        match syn::parse_file(&text) {
            Ok(parsed) => {
                let mut parsed_items = Parsed {
                    text: &text,
                    base,
                    file: at,
                    module_paths: &module_paths,
                    items: &mut items,
                    modules: &mut modules,
                };
                parsed_items.walk(&parsed.items, &mut Vec::new(), &mut Vec::new());
            }
            Err(error) => rejections.push(Rejection::UnparsableFile {
                file: path.clone(),
                reason: error.to_string(),
            }),
        }
    }

    if !rejections.is_empty() {
        return Err(rejections);
    }

    tokens.shrink_to_fit();
    literals.shrink_to_fit();
    comments.shrink_to_fit();
    items.shrink_to_fit();
    macros.shrink_to_fit();
    macros.sort_by_key(|record| (record.tokens.start(), record.tokens.end()));
    shapes.shrink_to_fit();
    union.shrink_to_fit();
    // Every fact in the index is asked for by span, so the ones a query looks
    // up by position are held in union order.
    modules.sort_by_key(|module| (module.span.start(), module.span.end()));

    Ok(Index {
        universe: universe.clone(),
        union,
        files,
        by_path,
        items,
        modules,
        tokens,
        literals,
        comments,
        macros,
        cross_check: enumeration.cross_check().clone(),
        macro_shapes: shapes,
    })
}

/// A file-relative offset as a union offset.
///
/// # Panics
///
/// Never: the caller has already refused a union that would not fit a `u32`.
fn at_union(base: usize, offset: usize) -> u32 {
    u32::try_from(base + offset).expect("the union was measured before it was built")
}

/// The file-relative offset of the start of every line.
fn line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    starts.extend(text.match_indices('\n').filter_map(|(at, _)| {
        u32::try_from(at + 1)
            .ok()
            .filter(|start| (*start as usize) < text.len())
    }));
    starts.shrink_to_fit();
    starts
}

// ── the lexical reading ───────────────────────────────────────────────────

struct Lexed<'a> {
    text: &'a str,
    base: usize,
    /// How far into the file every byte has been accounted for.
    cursor: usize,
    tokens: &'a mut Vec<TokenRecord>,
    literals: &'a mut Vec<LiteralRecord>,
    comments: &'a mut Vec<CommentRecord>,
    macros: &'a mut Vec<MacroRecord>,
    shapes: &'a mut Vec<UnsupportedMacroShape>,
}

impl Lexed<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(at_union(self.base, start), at_union(self.base, end))
    }

    /// Account for `[cursor, start)`, which holds whitespace and comments only.
    fn gap_to(&mut self, start: usize) {
        if start <= self.cursor {
            return;
        }
        self.scan_comments(self.cursor, start);
        self.cursor = start;
    }

    fn consume(&mut self, range: &Range<usize>) {
        self.gap_to(range.start);
        self.cursor = self.cursor.max(range.end);
    }

    fn walk(&mut self, stream: TokenStream) {
        let trees: Vec<TokenTree> = stream.into_iter().collect();
        let mut at = 0;
        while at < trees.len() {
            if let Some(next) = self.doc_attribute(&trees, at) {
                at = next;
                continue;
            }
            // **Recorded, never skipped.** A macro invocation and an attribute
            // are noted here and then lexed like anything else: their tokens
            // are tokens (§2.7's coverage floor), and what these two readings
            // add is only the knowledge of *where* they were found.
            self.note_macro(&trees, at);
            self.note_attribute(&trees, at);
            if let Some(next) = self.lifetime(&trees, at) {
                at = next;
                continue;
            }
            self.tree(&trees[at]);
            at += 1;
        }
    }

    /// `path ! ( … )`, `macro_rules ! name { … }` — found lexically, because the
    /// parser's visitor does not descend into what is inside them.
    fn note_macro(&mut self, trees: &[TokenTree], at: usize) {
        let Some(TokenTree::Ident(first)) = trees.get(at) else {
            return;
        };
        // A path may have several segments: `bt_source::needle!(…)`.
        let mut path = first.to_string();
        let mut next = at + 1;
        loop {
            let colons = matches!(trees.get(next), Some(TokenTree::Punct(one)) if one.as_char() == ':')
                && matches!(trees.get(next + 1), Some(TokenTree::Punct(two)) if two.as_char() == ':');
            if !colons {
                break;
            }
            let Some(TokenTree::Ident(segment)) = trees.get(next + 2) else {
                break;
            };
            path.push_str("::");
            path.push_str(&segment.to_string());
            next += 3;
        }
        if !matches!(trees.get(next), Some(TokenTree::Punct(bang)) if bang.as_char() == '!') {
            return;
        }
        next += 1;
        // `macro_rules! name { … }` carries its name between the `!` and the
        // body; every other invocation opens its delimiters straight away.
        let definition = path == "macro_rules";
        let mut name = path.clone();
        if definition {
            let Some(TokenTree::Ident(declared)) = trees.get(next) else {
                return;
            };
            name = declared.to_string();
            next += 1;
        }
        let Some(TokenTree::Group(group)) = trees.get(next) else {
            return;
        };
        let opening = first.span().byte_range().start;
        let closing = group.span().byte_range().end;
        let inside = group.span_open().byte_range().end..group.span_close().byte_range().start;
        let record = MacroRecord {
            span: self.span(opening, closing),
            tokens: self.span(inside.start, inside.end),
            path: if definition { name } else { path },
            kind: if definition {
                MacroKind::Definition
            } else {
                MacroKind::Invocation
            },
        };
        if definition {
            self.note_item_constructing_arms(group);
        } else {
            self.note_invocation_shape(&record);
        }
        self.macros.push(record);
    }

    /// The shapes of §2.7 an *invocation* can be.
    fn note_invocation_shape(&mut self, record: &MacroRecord) {
        let shape = match record.name() {
            "include" => MacroShape::SourceInclusion,
            "module_path" => MacroShape::ModulePath,
            "compile_error" => MacroShape::CompileError,
            "line" | "column" | "file" => MacroShape::LineNumber,
            _ => return,
        };
        self.report(record.span(), shape);
    }

    /// A `macro_rules!` arm whose expansion opens an item.
    ///
    /// The item it makes is not in this index — nothing parses an expansion —
    /// so a query for that item would answer "not declared", and §2.7 says that
    /// is reported rather than examined.
    fn note_item_constructing_arms(&mut self, definition: &proc_macro2::Group) {
        let trees: Vec<TokenTree> = definition.stream().into_iter().collect();
        for at in 0..trees.len() {
            let arrow = matches!(&trees[at], TokenTree::Punct(one) if one.as_char() == '=')
                && matches!(trees.get(at + 1), Some(TokenTree::Punct(two)) if two.as_char() == '>');
            if !arrow {
                continue;
            }
            let Some(TokenTree::Group(expansion)) = trees.get(at + 2) else {
                continue;
            };
            let opens_an_item = expansion.stream().into_iter().any(|tree| {
                matches!(tree, TokenTree::Ident(ref word) if OPENS_AN_ITEM
                    .iter()
                    .any(|keyword| word == keyword))
            });
            if opens_an_item {
                let range = expansion.span().byte_range();
                self.report(
                    self.span(range.start, range.end),
                    MacroShape::ItemConstructingArm,
                );
            }
        }
    }

    /// `#[…]` and `#![…]`: the attribute's own name, weighed against the ones
    /// the language defines.
    fn note_attribute(&mut self, trees: &[TokenTree], at: usize) {
        let Some(TokenTree::Punct(hash)) = trees.get(at) else {
            return;
        };
        if hash.as_char() != '#' {
            return;
        }
        let mut next = at + 1;
        if matches!(trees.get(next), Some(TokenTree::Punct(bang)) if bang.as_char() == '!') {
            next += 1;
        }
        let Some(TokenTree::Group(group)) = trees.get(next) else {
            return;
        };
        if group.delimiter() != Delimiter::Bracket {
            return;
        }
        let inside: Vec<TokenTree> = group.stream().into_iter().collect();
        let Some(TokenTree::Ident(name)) = inside.first() else {
            return;
        };
        let whole = self.span(
            hash.span().byte_range().start,
            group.span().byte_range().end,
        );
        let name = name.to_string();
        if name == "derive" {
            // One row for the attribute, however many of its derives are
            // somebody else's: the span is the attribute and the spelling in
            // the report names them all.
            if derived_names(&inside)
                .iter()
                .any(|derived| !BUILT_IN_DERIVES.contains(&derived.as_str()))
            {
                self.report(whole, MacroShape::DeriveReplacingABody);
            }
            return;
        }
        if !BUILT_IN_ATTRIBUTES.contains(&name.as_str()) {
            self.report(whole, MacroShape::AttributeReplacingABody);
        }
    }

    fn report(&mut self, span: Span, shape: MacroShape) {
        let start = span.start() - self.base;
        let end = span.end() - self.base;
        self.shapes.push(UnsupportedMacroShape {
            span,
            shape,
            spelling: self.text[start..end].to_owned(),
        });
    }

    /// `#[doc = "…"]` in either of its two spellings, and **nothing else that
    /// begins with the word `doc`**.
    ///
    /// The synthesized one — what `/// x` lexes to — is recognised by its
    /// tokens all carrying the comment's own byte range, and its tokens are not
    /// tokens of this program: they are skipped whole, so the string inside is
    /// never recorded as a literal of the source.
    ///
    /// The written-out one has to carry an `=`. `#[doc(hidden)]`,
    /// `#[doc(alias = "…")]` and `#[doc(cfg(…))]` are **attributes and not
    /// documentation text**: §2.1 removes comments and doc comments from
    /// [`crate::View::CodeKeepingLiterals`], and a reading that also removed
    /// these would answer "no" about bytes that are code. There are none in
    /// `bt-app`, which is why every number in P1b's measurement is unchanged by
    /// this, and seven in `bt-platform`, `bt-render` and `bt-term` — the
    /// universes P1c's first consumer asks.
    fn doc_attribute(&mut self, trees: &[TokenTree], at: usize) -> Option<usize> {
        let TokenTree::Punct(hash) = trees.get(at)? else {
            return None;
        };
        if hash.as_char() != '#' {
            return None;
        }
        let opening = hash.span().byte_range();
        let mut next = at + 1;
        if let Some(TokenTree::Punct(bang)) = trees.get(next)
            && bang.as_char() == '!'
        {
            next += 1;
        }
        let TokenTree::Group(group) = trees.get(next)? else {
            return None;
        };
        if group.delimiter() != Delimiter::Bracket {
            return None;
        }
        let mut inside = group.stream().into_iter();
        if !matches!(inside.next(), Some(TokenTree::Ident(ref name)) if name == "doc") {
            return None;
        }
        let closing = group.span().byte_range();
        let synthesized = opening == closing;
        if !synthesized
            && !matches!(inside.next(), Some(TokenTree::Punct(ref sign)) if sign.as_char() == '=')
        {
            return None;
        }
        let whole = opening.start..closing.end;
        self.gap_to(whole.start);
        let kind = if synthesized {
            CommentKind::DocComment
        } else {
            CommentKind::DocAttribute
        };
        let span = self.span(whole.start, whole.end);
        self.comments.push(CommentRecord { span, kind });
        if !synthesized {
            // Written out by hand: its tokens are real tokens and are lowered
            // like any others. Only the mask is recorded here.
            return None;
        }
        self.cursor = self.cursor.max(whole.end);
        Some(next + 1)
    }

    /// `'a` — a joint tick and the name it is glued to are one token here.
    fn lifetime(&mut self, trees: &[TokenTree], at: usize) -> Option<usize> {
        let TokenTree::Punct(tick) = trees.get(at)? else {
            return None;
        };
        if tick.as_char() != '\'' || tick.spacing() != Spacing::Joint {
            return None;
        }
        let TokenTree::Ident(name) = trees.get(at + 1)? else {
            return None;
        };
        let opening = tick.span().byte_range();
        let closing = name.span().byte_range();
        if closing.start != opening.end {
            return None;
        }
        let whole = opening.start..closing.end;
        self.consume(&whole);
        let span = self.span(whole.start, whole.end);
        self.tokens.push(TokenRecord {
            span,
            kind: TokenKind::Lifetime,
        });
        Some(at + 2)
    }

    fn tree(&mut self, tree: &TokenTree) {
        match tree {
            TokenTree::Ident(name) => {
                let range = name.span().byte_range();
                self.consume(&range);
                let kind = if self.text[range.clone()].starts_with("r#") {
                    TokenKind::RawIdentifier
                } else {
                    TokenKind::Identifier
                };
                let span = self.span(range.start, range.end);
                self.tokens.push(TokenRecord { span, kind });
            }
            TokenTree::Literal(literal) => {
                let range = literal.span().byte_range();
                self.consume(&range);
                let span = self.span(range.start, range.end);
                self.literals.push(LiteralRecord {
                    span,
                    value: decoded(literal.clone()),
                });
            }
            TokenTree::Punct(punct) => {
                let range = punct.span().byte_range();
                self.consume(&range);
            }
            TokenTree::Group(group) => {
                let opening = group.span_open().byte_range();
                self.consume(&opening);
                self.walk(group.stream());
                let closing = group.span_close().byte_range();
                self.consume(&closing);
            }
        }
    }

    /// Find the comments in a stretch the lexer accounted for as neither token
    /// nor doc comment — so, whitespace and comments.
    fn scan_comments(&mut self, from: usize, to: usize) {
        let mut at = from;
        while at < to {
            let rest = &self.text[at..to];
            if rest.starts_with("//") {
                let end = rest.find('\n').map_or(to, |newline| at + newline);
                let span = self.span(at, end);
                self.comments.push(CommentRecord {
                    span,
                    kind: CommentKind::Line,
                });
                at = end;
            } else if rest.starts_with("/*") {
                let end = block_comment_end(rest).map_or(to, |len| at + len);
                let span = self.span(at, end);
                self.comments.push(CommentRecord {
                    span,
                    kind: CommentKind::Block,
                });
                at = end;
            } else {
                match rest.chars().next() {
                    Some(character) => at += character.len_utf8(),
                    None => break,
                }
            }
        }
    }
}

/// The keywords that open an item, for [`Lexed::note_item_constructing_arms`].
const OPENS_AN_ITEM: [&str; 12] = [
    "fn",
    "struct",
    "enum",
    "union",
    "trait",
    "impl",
    "mod",
    "const",
    "static",
    "type",
    "use",
    "macro_rules",
];

/// The derives the language itself defines. Everything else generates items no
/// byte of this index holds.
const BUILT_IN_DERIVES: [&str; 9] = [
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
];

/// The attributes the language defines, so that what is left is what may be an
/// attribute macro.
///
/// It is a list of the ones this workspace writes plus the common remainder,
/// and it is deliberately **not** exhaustive of rustc's: a built-in nobody here
/// uses that turns up in the report is a row somebody reads and adds, which is
/// the right direction to be wrong in. The other direction — guessing that an
/// unknown attribute is inert — is the silence §2.7 forbids.
const BUILT_IN_ATTRIBUTES: [&str; 41] = [
    "allow",
    "automatically_derived",
    "bench",
    "cfg",
    "cfg_attr",
    "cold",
    "crate_name",
    "crate_type",
    "default",
    "deny",
    "deprecated",
    "derive",
    "doc",
    "expect",
    "export_name",
    "forbid",
    "global_allocator",
    "ignore",
    "inline",
    "link",
    "link_name",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "no_implicit_prelude",
    "no_main",
    "no_mangle",
    "no_std",
    "non_exhaustive",
    "panic_handler",
    "path",
    "repr",
    "should_panic",
    "target_feature",
    "test",
    "thread_local",
    "track_caller",
    "used",
    "warn",
    "windows_subsystem",
];

/// The last segment of every path listed inside a `derive(…)`.
fn derived_names(attribute: &[TokenTree]) -> Vec<String> {
    let Some(TokenTree::Group(list)) = attribute.get(1) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut last: Option<String> = None;
    for tree in list.stream() {
        match tree {
            TokenTree::Ident(name) => last = Some(name.to_string()),
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                if let Some(name) = last.take() {
                    names.push(name);
                }
            }
            _ => {}
        }
    }
    names.extend(last);
    names
}

/// The length of the block comment `rest` starts with, nesting counted.
fn block_comment_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    let mut at = 0usize;
    while at + 1 < bytes.len() {
        if bytes[at] == b'/' && bytes[at + 1] == b'*' {
            depth += 1;
            at += 2;
        } else if bytes[at] == b'*' && bytes[at + 1] == b'/' {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return Some(at);
            }
        } else {
            at += 1;
        }
    }
    None
}

/// What a literal is, beside how it is written.
fn decoded(literal: proc_macro2::Literal) -> LiteralValue {
    match syn::Lit::new(literal) {
        syn::Lit::Str(value) => LiteralValue::Str(value.value()),
        syn::Lit::ByteStr(value) => LiteralValue::ByteStr(value.value()),
        syn::Lit::CStr(value) => LiteralValue::CStr(value.value().to_bytes().to_vec()),
        syn::Lit::Byte(value) => LiteralValue::Byte(value.value()),
        syn::Lit::Char(value) => LiteralValue::Char(value.value()),
        syn::Lit::Int(value) => LiteralValue::Int(value.base10_digits().to_owned()),
        syn::Lit::Float(value) => LiteralValue::Float(value.base10_digits().to_owned()),
        syn::Lit::Bool(value) => LiteralValue::Bool(value.value()),
        _ => LiteralValue::Undecoded,
    }
}

// ── the parsed reading ────────────────────────────────────────────────────

struct Parsed<'a> {
    text: &'a str,
    base: usize,
    file: usize,
    module_paths: &'a [String],
    items: &'a mut Vec<ItemRecord>,
    modules: &'a mut Vec<ModuleRecord>,
}

impl Parsed<'_> {
    fn span(&self, range: Range<usize>) -> Span {
        Span::new(
            at_union(self.base, range.start),
            at_union(self.base, range.end),
        )
    }

    /// `module` is the inline-module path inside this file; `predicates` are the
    /// `cfg` spellings standing between the file and here, outermost first.
    fn walk(
        &mut self,
        items: &[syn::Item],
        module: &mut Vec<String>,
        predicates: &mut Vec<String>,
    ) {
        for item in items {
            match item {
                syn::Item::Mod(declaration) => {
                    let Some((brace, inner)) = &declaration.content else {
                        continue;
                    };
                    let (own, _) = cfg_predicates(&declaration.attrs, self.text);
                    let depth = predicates.len();
                    predicates.extend(own);
                    module.push(declaration.ident.to_string());
                    // An inline module's bytes are its braces and what is
                    // between them — the scope a reader names by its Rust path.
                    let span = self.span(braces(brace));
                    let module_paths = self.paths_for(module);
                    self.modules.push(ModuleRecord {
                        file: self.file,
                        module_paths,
                        span,
                        body: ModuleShape::Inline,
                    });
                    self.walk(inner, module, predicates);
                    module.pop();
                    predicates.truncate(depth);
                }
                syn::Item::Fn(function) => {
                    let (own, _) = cfg_predicates(&function.attrs, self.text);
                    let start = start_of(
                        &function.attrs,
                        Some(&function.vis),
                        signature_start(&function.sig),
                    );
                    let body = braces(&function.block.brace_token);
                    self.record(
                        module,
                        predicates,
                        own,
                        None,
                        None,
                        function.sig.ident.to_string(),
                        ItemKind::Function,
                        start..body.end,
                        Some(body),
                    );
                }
                syn::Item::Impl(block) => {
                    let (own, _) = cfg_predicates(&block.attrs, self.text);
                    let depth = predicates.len();
                    predicates.extend(own);
                    let owner = self.type_owner(&block.self_ty);
                    let trait_name = block
                        .trait_
                        .as_ref()
                        .map(|(_, path, _)| self.trait_spelling(path));
                    for member in &block.items {
                        let syn::ImplItem::Fn(function) = member else {
                            continue;
                        };
                        let (own, _) = cfg_predicates(&function.attrs, self.text);
                        let start = start_of(
                            &function.attrs,
                            Some(&function.vis),
                            signature_start(&function.sig),
                        );
                        let body = braces(&function.block.brace_token);
                        self.record(
                            module,
                            predicates,
                            own,
                            Some(owner.clone()),
                            trait_name.clone(),
                            function.sig.ident.to_string(),
                            ItemKind::AssociatedFunction,
                            start..body.end,
                            Some(body),
                        );
                    }
                    predicates.truncate(depth);
                }
                syn::Item::Trait(definition) => {
                    let (own, _) = cfg_predicates(&definition.attrs, self.text);
                    let depth = predicates.len();
                    predicates.extend(own);
                    let trait_name = definition.ident.to_string();
                    for member in &definition.items {
                        let syn::TraitItem::Fn(function) = member else {
                            continue;
                        };
                        let (own, _) = cfg_predicates(&function.attrs, self.text);
                        let start = start_of(&function.attrs, None, signature_start(&function.sig));
                        let body = function
                            .default
                            .as_ref()
                            .map(|block| braces(&block.brace_token));
                        let end = body.as_ref().map_or_else(
                            || {
                                function
                                    .semi_token
                                    .map_or(start, |semi| semi.span.byte_range().end)
                            },
                            |range| range.end,
                        );
                        self.record(
                            module,
                            predicates,
                            own,
                            None,
                            Some(trait_name.clone()),
                            function.sig.ident.to_string(),
                            ItemKind::AssociatedFunction,
                            start..end,
                            body,
                        );
                    }
                    predicates.truncate(depth);
                }
                _ => {}
            }
        }
    }

    /// The module paths an inline module inside this file answers to — one per
    /// owning declaration path of the file (§2.3), each with the inline
    /// ancestry appended.
    fn paths_for(&self, module: &[String]) -> Vec<String> {
        let suffix = module
            .iter()
            .map(|part| format!("::{part}"))
            .collect::<String>();
        self.module_paths
            .iter()
            .map(|path| format!("{path}{suffix}"))
            .collect()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the identity of §2.4 is six components and the bytes are two more; naming them \
                  in a struct at one call site each would hide the shape rather than show it"
    )]
    fn record(
        &mut self,
        module: &[String],
        outer: &[String],
        own: Vec<String>,
        type_owner: Option<String>,
        trait_name: Option<String>,
        name: String,
        kind: ItemKind,
        whole: Range<usize>,
        body: Option<Range<usize>>,
    ) {
        let mut predicates = outer.to_vec();
        predicates.extend(own);
        let module_paths = self.paths_for(module);
        let whole = self.span(whole);
        let body = body.map(|range| self.span(range));
        self.items.push(ItemRecord {
            file: self.file,
            module_paths,
            type_owner,
            trait_name,
            name,
            kind,
            variant: ConditionalVariant { predicates },
            whole,
            body,
        });
    }

    /// The trait an `impl` block implements, **with its arguments**.
    ///
    /// The self type drops them (§2.4: `Runtime<'_>` and `Runtime<'a>` are one
    /// type); the trait may not. `impl From<Vec<Block>> for Layout` and
    /// `impl From<[Block; N]> for Layout` are two different traits implemented
    /// for one type, and a reading that printed both as `From` would make them
    /// one identity with two arms — which is exactly what the eleven
    /// conditional identities look like, and they are not that.
    ///
    /// A lifetime written in a trait's arguments stays in the spelling; a query
    /// may name a trait with its arguments or without, and without matches
    /// every implementation of it (see [`crate::ItemQuery::of_trait`]).
    fn trait_spelling(&self, path: &syn::Path) -> String {
        let range = path.span().byte_range();
        self.text
            .get(range)
            .map_or_else(|| path_spelling(path), collapsed)
    }

    /// The self type of an `impl`, as **the last segment of its path**, without
    /// lifetimes or generic arguments — so `Runtime<'_>`, `Runtime<'a>`,
    /// `crate::Runtime<'_>` and `super::Runtime` are one owner (§2.4).
    ///
    /// **The qualification is dropped because identity has to survive a move,
    /// and a move is exactly when a module-relative spelling becomes a qualified
    /// one.** A method cut out of `main.rs` into a newly declared
    /// `src/runtime/mod.rs` is rewritten `impl crate::Runtime<'_>` there, and an
    /// owner that kept the prefix would make every pin on that method answer
    /// zero the day it moved — the failure this crate exists to remove. Which
    /// module the `impl` is written in is not lost by dropping it: it is the
    /// identity's own first component ([`crate::ItemIdentity::module_path`]),
    /// which is also why keeping it here printed it twice —
    /// `crate::runtime::crate::Runtime::file_peek_promotes`.
    ///
    /// Two distinct types of one name are therefore one owner, and that is a
    /// [`crate::QueryFailure::Multiplicity`] naming both rather than a quiet
    /// pick: a query narrows by module path (`in_module`) to say which.
    ///
    /// The trait is the other way round and keeps its whole spelling
    /// ([`Parsed::trait_spelling`]): a trait is named where it is defined, not
    /// where the `impl` is written, so moving the `impl` does not rewrite it.
    fn type_owner(&self, ty: &syn::Type) -> String {
        if let syn::Type::Path(path) = ty
            && path.qself.is_none()
            && let Some(last) = path.path.segments.last()
        {
            return last.ident.to_string();
        }
        // Anything that is not a path — `&[u8]`, a tuple, a trait object — has
        // no arguments to drop, so it is its own spelling with the source's own
        // spacing collapsed.
        collapsed(&self.text[ty.span().byte_range()])
    }
}

/// Source text with its own spacing collapsed to one space a gap.
fn collapsed(written: &str) -> String {
    written.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A path with its generic arguments dropped: `a::b::Display<'_>` is
/// `a::b::Display`.
///
/// The trait's fallback spelling, for the rare path whose bytes the file cannot
/// be sliced at. The self type does not use it: an owner is one segment
/// ([`Parsed::type_owner`]).
fn path_spelling(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Where an item's text begins: its first attribute, else its visibility, else
/// whatever its signature opens with.
fn start_of(
    attrs: &[syn::Attribute],
    visibility: Option<&syn::Visibility>,
    signature: usize,
) -> usize {
    if let Some(attribute) = attrs.first() {
        return attribute.pound_token.span.byte_range().start;
    }
    match visibility {
        Some(syn::Visibility::Public(token)) => token.span.byte_range().start,
        Some(syn::Visibility::Restricted(restricted)) => {
            restricted.pub_token.span.byte_range().start
        }
        Some(syn::Visibility::Inherited) | None => signature,
    }
}

/// The first byte of a signature, `const`/`async`/`unsafe`/`extern` included.
fn signature_start(signature: &syn::Signature) -> usize {
    let span = signature
        .constness
        .map(|token| token.span)
        .or_else(|| signature.asyncness.map(|token| token.span))
        .or_else(|| signature.unsafety.map(|token| token.span))
        .or_else(|| signature.abi.as_ref().map(|abi| abi.extern_token.span))
        .unwrap_or(signature.fn_token.span);
    span.byte_range().start
}

/// A block's braces and everything between them.
fn braces(brace: &syn::token::Brace) -> Range<usize> {
    brace.span.open().byte_range().start..brace.span.close().byte_range().end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(text: &str) -> (Vec<TokenRecord>, Vec<LiteralRecord>, Vec<CommentRecord>) {
        let (tokens, literals, comments, _, _) = lex(text);
        (tokens, literals, comments)
    }

    #[expect(
        clippy::type_complexity,
        reason = "five readings of one file, named by their order in the module doc"
    )]
    fn lex(
        text: &str,
    ) -> (
        Vec<TokenRecord>,
        Vec<LiteralRecord>,
        Vec<CommentRecord>,
        Vec<MacroRecord>,
        Vec<UnsupportedMacroShape>,
    ) {
        let mut tokens = Vec::new();
        let mut literals = Vec::new();
        let mut comments = Vec::new();
        let mut macros = Vec::new();
        let mut shapes = Vec::new();
        let mut lexed = Lexed {
            text,
            base: 0,
            cursor: 0,
            tokens: &mut tokens,
            literals: &mut literals,
            comments: &mut comments,
            macros: &mut macros,
            shapes: &mut shapes,
        };
        lexed.walk(TokenStream::from_str(text).expect("the fixture lexes"));
        lexed.gap_to(text.len());
        (tokens, literals, comments, macros, shapes)
    }

    fn masked(text: &str) -> Vec<(&str, CommentKind)> {
        lower(text)
            .2
            .into_iter()
            .map(|comment| {
                (
                    &text[comment.span().start()..comment.span().end()],
                    comment.kind(),
                )
            })
            .collect()
    }

    /// PIN — **the mask is every comment and no string that looks like one.**
    ///
    /// The two strippers in the tree today are both line filters and both
    /// under-strip: one drops whole comment lines and sees neither a block
    /// comment nor a trailing one, the other cuts each line at its first `//`
    /// and so mangles every line holding a URL. The gap reading cannot make
    /// either mistake, because the lexer has already said which bytes are a
    /// string.
    ///
    /// MUTATION: treat the gap as whitespace only and every row here vanishes;
    /// scan the raw text for `//` instead and the URL line grows a comment.
    #[test]
    fn a_comment_is_a_gap_the_lexer_left_and_a_string_is_never_one() {
        assert_eq!(
            masked("let url = \"https://example.com\"; // real\n"),
            [("// real", CommentKind::Line)]
        );
        assert_eq!(
            masked("fn f() { /* one /* two */ three */ }"),
            [("/* one /* two */ three */", CommentKind::Block)]
        );
        assert_eq!(
            masked("/// outer\n//! inner\nfn f() {}\n"),
            [
                ("/// outer", CommentKind::DocComment),
                ("//! inner", CommentKind::DocComment),
            ]
        );
        assert_eq!(
            masked("#[doc = \"written\"]\nfn f() {}\n"),
            [("#[doc = \"written\"]", CommentKind::DocAttribute)]
        );
        assert!(masked("fn f() { let s = \"/* not a comment */\"; }").is_empty());
        // **An attribute that begins with `doc` is not documentation text.**
        // `#[doc(hidden)]` says something about the item, in code; masking it
        // would make the code view answer "no" about bytes that are code. Seven
        // of these live in `bt-platform`, `bt-render` and `bt-term`.
        //
        // MUTATION: drop the `=` check in `doc_attribute` and all three rows
        // below come back as `DocAttribute` masks.
        assert!(masked("#[doc(hidden)]\npub fn f() {}\n").is_empty());
        assert!(masked("#[doc(alias = \"other\")]\npub fn f() {}\n").is_empty());
        assert!(masked("#![doc(html_root_url = \"x\")]\n").is_empty());
    }

    /// PIN — a doc comment's synthesized string is not a literal of this
    /// program, and a written one is.
    ///
    /// `proc_macro2` lexes `/// x` into `#[doc = " x"]`, and a reading that took
    /// that literal at face value would answer "yes" to "is this string in the
    /// source" for a string nobody wrote.
    #[test]
    fn a_doc_comments_string_is_not_a_literal_of_the_source() {
        let (_, literals, _) = lower("/// hello\nfn f() { let s = \"hello\"; }\n");
        assert_eq!(
            literals
                .iter()
                .map(|literal| literal.value().clone())
                .collect::<Vec<_>>(),
            [LiteralValue::Str("hello".to_owned())]
        );
    }

    /// PIN — **spelling is not value** (§2.1), on both halves.
    #[test]
    fn a_literal_carries_its_spelling_and_its_value_apart() {
        let text = "fn f() { let a = 0xFF; let b = \"a\\nb\"; let c = 'x'; let d = 1_0.5; }";
        let (_, literals, _) = lower(text);
        let rows: Vec<(&str, LiteralValue)> = literals
            .iter()
            .map(|literal| {
                (
                    &text[literal.span().start()..literal.span().end()],
                    literal.value().clone(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("0xFF", LiteralValue::Int("255".to_owned())),
                ("\"a\\nb\"", LiteralValue::Str("a\nb".to_owned())),
                ("'x'", LiteralValue::Char('x')),
                ("1_0.5", LiteralValue::Float("10.5".to_owned())),
            ]
        );
    }

    /// PIN — an identifier token carries the boundaries of the **name**, so a
    /// raw identifier is `type` and a lifetime is `a`.
    #[test]
    fn a_token_knows_where_its_name_starts() {
        let text = "fn f<'a>(r#type: &'a u8) {}";
        let (tokens, _, _) = lower(text);
        let rows: Vec<(&str, TokenKind)> = tokens
            .iter()
            .map(|token| {
                (
                    &text[token.name_span().start()..token.name_span().end()],
                    token.kind(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("fn", TokenKind::Identifier),
                ("f", TokenKind::Identifier),
                ("a", TokenKind::Lifetime),
                ("type", TokenKind::RawIdentifier),
                ("a", TokenKind::Lifetime),
                ("u8", TokenKind::Identifier),
            ],
            "`fn` is an identifier token like any other; the lexer draws no line \
             and neither does a query for a name"
        );
    }

    /// PIN — tokens inside a macro invocation are tokens (§2.7).
    #[test]
    fn a_name_inside_a_macro_is_a_name() {
        let (tokens, literals, _) = lower("fn f() { println!(\"{}\", stand_in()); }");
        let names: Vec<String> = tokens
            .iter()
            .map(|token| {
                "fn f() { println!(\"{}\", stand_in()); }"
                    [token.name_span().start()..token.name_span().end()]
                    .to_owned()
            })
            .collect();
        assert!(names.iter().any(|name| name == "stand_in"));
        assert_eq!(literals.len(), 1);
    }

    /// The two spellings one `impl` block lowers to: its owner, and its trait.
    fn spellings_of(text: &str) -> (String, Option<String>) {
        let file = syn::parse_file(text).expect("the fixture parses");
        let syn::Item::Impl(block) = &file.items[0] else {
            panic!("the fixture is an impl block");
        };
        let mut items = Vec::new();
        let mut modules = Vec::new();
        let parsed = Parsed {
            text,
            base: 0,
            file: 0,
            module_paths: &[],
            items: &mut items,
            modules: &mut modules,
        };
        let owner = parsed.type_owner(&block.self_ty);
        let trait_name = block
            .trait_
            .as_ref()
            .map(|(_, path, _)| parsed.trait_spelling(path));
        (owner, trait_name)
    }

    fn owner_of(text: &str) -> String {
        spellings_of(text).0
    }

    /// PIN — **a type owner is the last path segment, without lifetimes or
    /// generic arguments**, so `Runtime<'_>`, `Runtime<'a>`, `crate::Runtime<'_>`
    /// and `super::Runtime` are one owner (§2.4), and a self type that is not a
    /// path keeps its own spelling.
    ///
    /// MUTATION: join the whole path here again and the qualified rows read
    /// `crate::Runtime` — which is the spelling a move writes, and the one that
    /// made a pinned query answer zero and print
    /// `crate::runtime::crate::Runtime::…`.
    #[test]
    fn a_type_owner_is_the_last_segment_without_its_arguments() {
        assert_eq!(owner_of("impl Runtime<'_> { fn f(&self) {} }"), "Runtime");
        assert_eq!(
            owner_of("impl<'a> Runtime<'a> { fn f(&self) {} }"),
            "Runtime"
        );
        assert_eq!(
            owner_of("impl crate::Runtime<'_> { fn f(&self) {} }"),
            "Runtime"
        );
        assert_eq!(
            owner_of("impl super::Runtime { fn f(&self) {} }"),
            "Runtime"
        );
        assert_eq!(owner_of("impl a::b::Runtime { fn f(&self) {} }"), "Runtime");
        assert_eq!(
            owner_of("impl Door for &'static [u8] { fn f(&self) {} }"),
            "&'static [u8]"
        );
    }

    /// PIN — **the trait keeps its whole spelling** where the self type drops
    /// it: a trait is named where it is defined and a move does not rewrite it,
    /// so `impl fmt::Display for Site` is `fmt::Display` and not `Display`.
    #[test]
    fn a_trait_keeps_the_path_the_impl_names_it_by() {
        let (owner, trait_name) = spellings_of("impl fmt::Display for Site { fn fmt(&self) {} }");
        assert_eq!(owner, "Site");
        assert_eq!(trait_name.as_deref(), Some("fmt::Display"));
    }
}
