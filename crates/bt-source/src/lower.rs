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
    LiteralRecord, LiteralValue, Span, TokenKind, TokenRecord,
};
use crate::reject::Rejection;
use crate::universe::Universe;

/// Lower `universe`, and drop every parser object on the way out.
pub(crate) fn build(universe: &Universe) -> Result<Index, Vec<Rejection>> {
    let enumeration = crate::enumerate(universe)?;

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
    let mut rejections = Vec::new();

    for (at, path) in paths.iter().enumerate() {
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
        files.push(FileRecord {
            path: path.clone(),
            span: Span::new(at_union(base, 0), at_union(base, text.len())),
            line_starts: line_starts(&text),
            owners,
        });
        by_path.insert(path.clone(), at);

        match TokenStream::from_str(&text) {
            Ok(stream) => {
                let mut lexed = Lexed {
                    text: &text,
                    base,
                    cursor: 0,
                    tokens: &mut tokens,
                    literals: &mut literals,
                    comments: &mut comments,
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
    union.shrink_to_fit();

    Ok(Index {
        universe: universe.clone(),
        union,
        files,
        by_path,
        items,
        tokens,
        literals,
        comments,
        cross_check: enumeration.cross_check().clone(),
        macro_shapes: None,
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
            if let Some(next) = self.lifetime(&trees, at) {
                at = next;
                continue;
            }
            self.tree(&trees[at]);
            at += 1;
        }
    }

    /// `#[doc = "…"]` in either of its two spellings.
    ///
    /// The synthesized one — what `/// x` lexes to — is recognised by its
    /// tokens all carrying the comment's own byte range, and its tokens are not
    /// tokens of this program: they are skipped whole, so the string inside is
    /// never recorded as a literal of the source.
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
        let first = group.stream().into_iter().next();
        if !matches!(first, Some(TokenTree::Ident(ref name)) if name == "doc") {
            return None;
        }
        let closing = group.span().byte_range();
        let whole = opening.start..closing.end;
        self.gap_to(whole.start);
        let synthesized = opening == closing;
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
                    let Some((_, inner)) = &declaration.content else {
                        continue;
                    };
                    let (own, _) = cfg_predicates(&declaration.attrs, self.text);
                    let depth = predicates.len();
                    predicates.extend(own);
                    module.push(declaration.ident.to_string());
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
                        .map(|(_, path, _)| path_spelling(path));
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
        let suffix = module
            .iter()
            .map(|part| format!("::{part}"))
            .collect::<String>();
        let module_paths: Vec<String> = self
            .module_paths
            .iter()
            .map(|path| format!("{path}{suffix}"))
            .collect();
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

    /// The self type of an `impl`, **without lifetimes or generic arguments**,
    /// so `Runtime<'_>` and `Runtime<'a>` are one type (§2.4).
    fn type_owner(&self, ty: &syn::Type) -> String {
        if let syn::Type::Path(path) = ty
            && path.qself.is_none()
        {
            return path_spelling(&path.path);
        }
        // Anything that is not a path — `&[u8]`, a tuple, a trait object — has
        // no arguments to drop, so it is its own spelling with the source's own
        // spacing collapsed.
        let range = ty.span().byte_range();
        self.text[range]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A path with its generic arguments dropped: `a::b::Runtime<'_>` is
/// `a::b::Runtime`.
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
        let mut tokens = Vec::new();
        let mut literals = Vec::new();
        let mut comments = Vec::new();
        let mut lexed = Lexed {
            text,
            base: 0,
            cursor: 0,
            tokens: &mut tokens,
            literals: &mut literals,
            comments: &mut comments,
        };
        lexed.walk(TokenStream::from_str(text).expect("the fixture lexes"));
        lexed.gap_to(text.len());
        (tokens, literals, comments)
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

    fn owner_of(text: &str) -> String {
        let file = syn::parse_file(text).expect("the fixture parses");
        let syn::Item::Impl(block) = &file.items[0] else {
            panic!("the fixture is an impl block");
        };
        let mut items = Vec::new();
        let parsed = Parsed {
            text,
            base: 0,
            file: 0,
            module_paths: &[],
            items: &mut items,
        };
        parsed.type_owner(&block.self_ty)
    }

    /// PIN — **a type owner drops lifetimes and generic arguments** so that
    /// `Runtime<'_>` and `Runtime<'a>` are one type (§2.4), and a self type that
    /// is not a path keeps its own spelling.
    #[test]
    fn a_type_owner_is_the_type_without_its_arguments() {
        assert_eq!(owner_of("impl Runtime<'_> { fn f(&self) {} }"), "Runtime");
        assert_eq!(
            owner_of("impl<'a> Runtime<'a> { fn f(&self) {} }"),
            "Runtime"
        );
        assert_eq!(
            owner_of("impl a::b::Runtime { fn f(&self) {} }"),
            "a::b::Runtime"
        );
        assert_eq!(
            owner_of("impl Door for &'static [u8] { fn f(&self) {} }"),
            "&'static [u8]"
        );
    }
}
