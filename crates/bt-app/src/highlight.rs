//! **Reading-level syntax highlighting for the preview** (ticket #49, user
//! ruling 2026-08-16: 「阅读级」).
//!
//! # What "reading level" rules out
//!
//! An editor highlights to help you *write*: every identifier a different ink,
//! every operator picked out, the mosaic that tells you at a glance which of the
//! forty things on the line the compiler thinks each one is. A preview is not
//! that surface. It is the thing you open to answer "is this the file I meant"
//! and "what does this function do", and its highlighting has exactly one job —
//! give the eye the four or five landmarks it uses to skim code: where the
//! keywords are, where the strings are, what is a comment, what is a number.
//!
//! So the default here is **the body's own ink**. Seven categories are painted
//! and everything else — identifiers, operators, whitespace, prose in a fence,
//! every scope the table below does not name — is left exactly the colour it was
//! before this module existed. Turning the highlighting off is therefore not a
//! different renderer but an empty [`Highlighting`], which is what an unknown
//! language, an over-cap file and a fence with no info string all produce.
//!
//! # Where the colours come from, and where they do not
//!
//! `syntect` ships colour themes. This module uses none of them, and that is the
//! house rule rather than a preference: every ink on this glass is a field of
//! [`bt_render::ChromePalette`], picked for the two canvases this window paints
//! and pinned to a contrast floor in `bt-render`'s own tests. What syntect is
//! asked for is the **scope** of each span — Sublime's `keyword.control.rust`,
//! `string.quoted.double.rust`, `comment.line.double-slash.rust` — and the
//! seven-entry table in [`SCOPE_TABLE`] turns that into one of ours.
//!
//! # Where it runs
//!
//! Once per document revision, in the width-free half of the preview's parse
//! (`PreviewParseKey`) — never per frame. A scroll walks spans that already
//! exist; a resize re-flows the rows and asks this module nothing. An edit bumps
//! the buffer's revision, which is what re-runs it, and that is the whole of the
//! cache invalidation.
//!
//! # Columns, not bytes
//!
//! Everything this module hands back is measured in **drawn columns**, because
//! that is the coordinate the preview's painter, its wrap layout and its caret
//! already share ([`crate::preview_edit`]). The lines it is given are *display*
//! lines — tabs already the spaces they draw as — so a span's width is the
//! width of its text and a wide character counts as the two cells it occupies.
//! Handing spans back in bytes would mean a conversion at every one of the three
//! call sites, and each conversion is a place for a CJK identifier to be counted
//! once.

use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// The largest file this module will walk, in bytes of display text.
///
/// Above it a preview is drawn plain — one run per line, exactly as it was
/// before #49. The cap is honest rather than defensive: syntect's walk is
/// O(lines) and linear in the line's length, but the constant is a regex engine,
/// and a 2 MB minified bundle on one line is a document where the *parse* is the
/// wrong thing to be spending a file-open on. A reader who opens something that
/// large is looking for a string in it, not reading it.
pub const HIGHLIGHT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// The largest file this module will walk, in lines. See [`HIGHLIGHT_MAX_BYTES`]
/// — two caps because the two ways a document gets big cost differently, and a
/// 20k-line file of short lines is over the line cap long before it is over the
/// byte one.
pub const HIGHLIGHT_MAX_LINES: usize = 20_000;

/// One of the seven inks a highlighted span can wear — or [`Self::Body`], which
/// is the absence of a decision and by far the commonest answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HighlightToken {
    /// Nothing the table names. The surface's own text colour, unchanged.
    Body,
    /// `keyword`, `storage` — including `storage.type`, which reads as a keyword
    /// in most grammars (`fn`, `let`, `int`) rather than as a named type.
    Keyword,
    /// `string`, and `constant.character.escape` with it: a `\n` inside a
    /// literal is part of the literal to a reader, and picking it out in a
    /// different ink is editor-level detail this surface does not want.
    Str,
    /// `comment`.
    Comment,
    /// `constant.numeric` and `constant.language` — `42` and `true` are the same
    /// kind of thing on a skim.
    Number,
    /// `entity.name.type`, `support.type`, `support.class`.
    Type,
    /// `entity.name.function`, `support.function`.
    Function,
    /// `punctuation` that no larger category claimed — braces, semicolons,
    /// separators. Quieter than the body, because they are the scaffolding.
    Punct,
}

impl HighlightToken {
    /// This token's ink on a given surface.
    pub fn ink(self, ink: HighlightInk<'_>) -> [u8; 3] {
        match self {
            Self::Body => ink.body,
            Self::Keyword => ink.palette.hl_keyword,
            Self::Str => ink.palette.hl_string,
            Self::Comment => ink.palette.hl_comment,
            Self::Number => ink.palette.hl_number,
            Self::Type => ink.palette.hl_type,
            Self::Function => ink.palette.hl_function,
            Self::Punct => ink.palette.hl_punct_muted,
        }
    }
}

/// The two halves of "what colour is this span".
///
/// The palette's seven, and the **body ink of the surface the run stands on** —
/// which is `preview_body_text` in a pane and `preview_code_text` inside a
/// markdown fence. Those are two different colours and both of them mean
/// "plain", so the plain ink cannot live in the palette lookup with the others.
#[derive(Clone, Copy, Debug)]
pub struct HighlightInk<'a> {
    pub palette: &'a bt_render::ChromePalette,
    pub body: [u8; 3],
}

/// One run of one line: how many columns it covers, and what it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HighlightSpan {
    pub columns: usize,
    pub token: HighlightToken,
}

/// A whole document's spans, one entry per display line.
///
/// **Empty means plain**, and every road to "no highlighting" ends here: an
/// unrecognised extension, a fence with no info string, a file over either cap,
/// a grammar that failed to parse. [`Self::runs`] on an empty `Highlighting`
/// returns exactly what the preview drew before this module existed — one run,
/// the body's ink — so the un-highlighted path is the same code path rather than
/// a second one that has to be kept in step.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Highlighting {
    lines: Vec<Vec<HighlightSpan>>,
}

impl Highlighting {
    /// The empty one, spelled `const` so a caller can hold it in a `static` and
    /// hand out a `'static` reference to it. `Default` cannot: a reference to a
    /// `const` of a type that owns a heap allocation is a reference to a
    /// temporary, and the borrow dies at the end of the statement that made it.
    pub const fn plain() -> Self {
        Self { lines: Vec::new() }
    }

    // ── three readers, and they are the tests' and nobody else's ──────────
    //
    // Product code never asks a `Highlighting` a question: it hands it to
    // [`Self::runs`] and takes the runs, because "is this highlighted" is
    // precisely the branch this type exists to remove. The spans still have to
    // be *assertable*, though — a mapping table you can only observe through a
    // colour is a mapping table you cannot state — so the three below are
    // `cfg(test)` rather than `allow(dead_code)`: they do not ship, and the day
    // one of them has a real caller is the day the gate comes off.

    /// Whether this document was highlighted at all.
    #[cfg(test)]
    pub fn is_plain(&self) -> bool {
        self.lines.is_empty()
    }

    /// One line's spans.
    #[cfg(test)]
    pub fn line(&self, line: usize) -> &[HighlightSpan] {
        self.lines.get(line).map_or(&[], Vec::as_slice)
    }

    /// How many lines were walked.
    #[cfg(test)]
    pub fn lines(&self) -> usize {
        self.lines.len()
    }

    /// Walk a whole document once.
    ///
    /// `lines` are **display** lines — tab-expanded, without their breaks — and
    /// the state carries from one to the next, which is what makes a block
    /// comment opened on line 3 still a comment on line 40. Sublime's grammars
    /// are the "newlines" variants, so each line is handed over with its break
    /// put back; the spans that come out are clamped to the line's own bytes so
    /// the break itself never becomes a column.
    ///
    /// Anything that goes wrong — either cap, a grammar that will not parse, a
    /// scope stack that will not apply — comes back as a plain document. There
    /// is no half-highlighted state: a file whose highlighting stopped at line
    /// 200 is a file that looks broken at line 201.
    pub fn of(lines: &[String], syntax: &SyntaxReference) -> Self {
        if lines.len() > HIGHLIGHT_MAX_LINES {
            return Self::default();
        }
        let bytes: usize = lines.iter().map(|line| line.len() + 1).sum();
        if bytes > HIGHLIGHT_MAX_BYTES {
            return Self::default();
        }
        let syntaxes = syntaxes();
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut out = Vec::with_capacity(lines.len());
        let mut fed = String::new();
        for line in lines {
            fed.clear();
            fed.push_str(line);
            fed.push('\n');
            let Ok(ops) = state.parse_line(&fed, syntaxes) else {
                return Self::default();
            };
            let mut spans: Vec<HighlightSpan> = Vec::new();
            let mut at = 0usize;
            for (offset, op) in ops {
                let offset = offset.min(line.len());
                if offset > at {
                    push_span(&mut spans, &line[at..offset], token_of(&stack));
                    at = offset;
                }
                if stack.apply(&op).is_err() {
                    return Self::default();
                }
            }
            if at < line.len() {
                push_span(&mut spans, &line[at..], token_of(&stack));
            }
            out.push(spans);
        }
        Self { lines: out }
    }

    /// The runs one drawn row is made of: the columns `[from, to)` of `line`,
    /// cut where the highlighting changes.
    ///
    /// **The one door**, and it is the door the un-highlighted case uses too.
    /// A plain document, a line past the end of the walk and a row that no span
    /// covers all come back as the single body-ink run the painter used to build
    /// by hand, so a caller never asks "is this highlighted" — it asks for runs
    /// and gets between one and a few.
    ///
    /// `to` may run one column past the line's own text: that is the column the
    /// line break stands in, and [`crate::preview_edit::WrapLayout::row_span`]
    /// hands it out so a selection covering the break has somewhere to draw.
    /// Cutting by column handles it, because the byte lookup clamps.
    pub fn runs(
        &self,
        line: usize,
        text: &str,
        columns: (usize, usize),
        ink: HighlightInk<'_>,
    ) -> Vec<bt_render::PreviewRun> {
        let (from, to) = (columns.0, columns.1.max(columns.0));
        let cut = |first: usize, last: usize| {
            let start = crate::preview_edit::byte_at_column(text, first);
            let end = crate::preview_edit::byte_at_column(text, last.max(first));
            text[start..end].to_owned()
        };
        let plain = |token: HighlightToken, text: String| bt_render::PreviewRun {
            text,
            color: token.ink(ink),
            mono: true,
            bold: false,
            font_scale: 1.0,
            inline_box_px: None,
        };
        let spans = self.lines.get(line).filter(|spans| !spans.is_empty());
        let Some(spans) = spans else {
            return vec![plain(HighlightToken::Body, cut(from, to))];
        };
        let mut runs = Vec::new();
        let mut column = 0usize;
        for span in spans {
            let end = column + span.columns;
            let (start, stop) = (column.max(from), end.min(to));
            column = end;
            if stop <= start {
                continue;
            }
            runs.push(plain(span.token, cut(start, stop)));
        }
        // Whatever the walk did not cover — the break column at the end of a
        // line, and the tail of a line the grammar left unscoped.
        if column < to {
            let tail = cut(column.max(from), to);
            if !tail.is_empty() {
                runs.push(plain(HighlightToken::Body, tail));
            }
        }
        if runs.is_empty() {
            runs.push(plain(HighlightToken::Body, cut(from, to)));
        }
        runs
    }
}

/// Grow the last span rather than push a second of the same kind: a line of
/// forty punctuation atoms is one run, not forty, and the shaper is handed the
/// shortest list that says the same thing.
fn push_span(spans: &mut Vec<HighlightSpan>, text: &str, token: HighlightToken) {
    let columns = bt_unicode::text_width(text);
    if columns == 0 {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.token == token => last.columns += columns,
        _ => spans.push(HighlightSpan { columns, token }),
    }
}

/// Sublime scope prefix → one of ours, **most specific first**.
///
/// The order is the tie-break within a single scope: `constant.character.escape`
/// has to be read before `constant.numeric` would ever be consulted, and
/// `entity.name.function` before anything shorter. Nothing here is a prefix of
/// anything else here except by that design.
///
/// `storage` covering `storage.type` is the ruling the ticket asked for and it
/// is the right one for the languages this window is read in: `storage.type` is
/// what Sublime's Rust, C and Go grammars scope `fn`, `int` and `func` as, and
/// those are keywords to a reader, not type names.
const SCOPE_TABLE: &[(&str, HighlightToken)] = &[
    ("constant.character.escape", HighlightToken::Str),
    ("entity.name.function", HighlightToken::Function),
    ("entity.name.type", HighlightToken::Type),
    ("support.function", HighlightToken::Function),
    ("support.class", HighlightToken::Type),
    ("support.type", HighlightToken::Type),
    ("constant.numeric", HighlightToken::Number),
    ("constant.language", HighlightToken::Number),
    ("keyword", HighlightToken::Keyword),
    ("storage", HighlightToken::Keyword),
    ("string", HighlightToken::Str),
    ("comment", HighlightToken::Comment),
    ("punctuation", HighlightToken::Punct),
];

/// [`SCOPE_TABLE`] with its left column built into syntect's interned scopes,
/// once. `Scope::new` parses a dotted string and takes a lock on the global
/// repository; doing it per span of a 5000-line file is most of the run time.
fn scope_table() -> &'static [(Scope, HighlightToken)] {
    static TABLE: OnceLock<Vec<(Scope, HighlightToken)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        SCOPE_TABLE
            .iter()
            .filter_map(|(name, token)| Scope::new(name).ok().map(|scope| (scope, *token)))
            .collect()
    })
}

/// What one scope stack is worth, read **from the bottom**.
///
/// The outermost scope that names a category wins, and that is deliberate: the
/// quotes around a string are `punctuation.definition.string.begin` *inside*
/// `string.quoted.double`, and a reader wants them the string's colour, not the
/// punctuation grey. Reading from the top — the more obvious direction — would
/// paint every string's own quotes, every comment's own `//` and every number's
/// own sign in the muted ink, and the result is a document that looks moth-eaten.
///
/// Nothing is lost by it, because a scope that is *outside* a category is
/// something the table does not name: `source.rust`, `meta.function.rust`,
/// `meta.block.rust`. The walk skips them and arrives at the first real answer.
fn token_of(stack: &ScopeStack) -> HighlightToken {
    for scope in stack.as_slice() {
        for (prefix, token) in scope_table() {
            if prefix.is_prefix_of(*scope) {
                return *token;
            }
        }
    }
    HighlightToken::Body
}

/// The grammars, loaded once: Sublime's defaults **plus bat's extras** — TOML,
/// TypeScript and TSX among them (user ruling 2026-08-16, after the first cut
/// left `Cargo.toml` plain).
///
/// `extra_newlines` rather than the no-newlines set because the walk feeds each
/// line with its break: the newline variants are the ones whose `$` anchors and
/// whose line-ending contexts behave, and the no-newlines set exists for the
/// case where you genuinely cannot supply one. The extras come from `two-face`,
/// which was already in the tree behind typst on the same regex backend; its
/// grammars carry their own licences (`two_face::acknowledgement`), which the
/// about page owes a line the day it exists.
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

/// Whether a syntax is the "no syntax" one. Plain Text is what syntect answers
/// with for `.txt`, and answering it here would mean walking a regex engine over
/// a document to be told every span is plain.
fn is_plain_text(syntax: &SyntaxReference) -> bool {
    syntax.name == syntaxes().find_syntax_plain_text().name
}

/// The grammar a **file** is read with: its name first, then its first line.
///
/// The name is asked twice — whole, then after its last dot — because a file's
/// language is as often its whole name as its extension (`Makefile`,
/// `Dockerfile`, `.gitignore`), and syntect's extension lists carry both kinds of
/// token. The first line is the fallback the ticket names and the one that
/// matters for files with no extension at all: a `#!/bin/sh` script, an XML
/// document that begins `<?xml`.
///
/// `None` means plain, which is this window's behaviour before #49 and the
/// honest answer for a language whose grammar is not in the box. The box is
/// Sublime's defaults plus bat's extras (see [`syntaxes`]) — TOML and TypeScript
/// were the two the defaults missed, and the user ruled them in on 2026-08-16.
pub fn syntax_for_file(name: &str, first_line: Option<&str>) -> Option<&'static SyntaxReference> {
    let syntaxes = syntaxes();
    let by_name = syntaxes
        .find_syntax_by_extension(name)
        .or_else(|| {
            name.rsplit_once('.')
                .and_then(|(_, extension)| syntaxes.find_syntax_by_extension(extension))
        })
        .or_else(|| first_line.and_then(|line| syntaxes.find_syntax_by_first_line(line)));
    by_name.filter(|syntax| !is_plain_text(syntax))
}

/// The grammar a markdown **fence** is read with: its info string.
///
/// ` ```rust `, ` ```py `, ` ```Shell ` — `find_syntax_by_token` is the door
/// syntect documents for exactly this, matching an extension first and then a
/// grammar's own name case-insensitively, which is what makes `py` and
/// `Python` the same request. A fence with no info string, or one naming
/// something not in the box, is drawn in the fence's own ink as it always was.
pub fn syntax_for_fence(info: Option<&str>) -> Option<&'static SyntaxReference> {
    let info = info?.trim();
    if info.is_empty() {
        return None;
    }
    syntaxes()
        .find_syntax_by_token(info)
        .filter(|syntax| !is_plain_text(syntax))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(text: &str) -> Vec<String> {
        text.lines().map(crate::preview::expand_tabs).collect()
    }

    fn tokens(highlighting: &Highlighting, line: usize) -> Vec<HighlightToken> {
        highlighting.line(line).iter().map(|s| s.token).collect()
    }

    /// The scope table is a *mapping*, and this is the whole of it stated
    /// without a document: thirteen prefixes in, seven tokens out, and the
    /// specific ones read before the general ones so `constant.character.escape`
    /// is a string rather than a number.
    #[test]
    fn the_scope_table_maps_sublime_scopes_onto_the_seven_tokens() {
        let token = |scope: &str| {
            let mut stack = ScopeStack::new();
            for atom in ["source.rust", scope] {
                stack.push(Scope::new(atom).expect("a scope"));
            }
            token_of(&stack)
        };
        assert_eq!(token("keyword.control.rust"), HighlightToken::Keyword);
        assert_eq!(token("storage.type.rust"), HighlightToken::Keyword);
        assert_eq!(token("storage.modifier.rust"), HighlightToken::Keyword);
        assert_eq!(token("string.quoted.double.rust"), HighlightToken::Str);
        assert_eq!(token("constant.character.escape.rust"), HighlightToken::Str);
        assert_eq!(
            token("comment.line.double-slash.rust"),
            HighlightToken::Comment
        );
        assert_eq!(token("comment.block.rust"), HighlightToken::Comment);
        assert_eq!(
            token("constant.numeric.integer.rust"),
            HighlightToken::Number
        );
        assert_eq!(token("constant.language.rust"), HighlightToken::Number);
        assert_eq!(token("entity.name.type.rust"), HighlightToken::Type);
        assert_eq!(token("support.type.rust"), HighlightToken::Type);
        assert_eq!(token("support.class.js"), HighlightToken::Type);
        assert_eq!(token("entity.name.function.rust"), HighlightToken::Function);
        assert_eq!(token("support.function.rust"), HighlightToken::Function);
        assert_eq!(
            token("punctuation.section.block.begin"),
            HighlightToken::Punct
        );
        // Everything else is the body's own ink — the default, and the reason
        // this reads as prose rather than as a mosaic.
        assert_eq!(token("variable.other.rust"), HighlightToken::Body);
        assert_eq!(token("meta.function.rust"), HighlightToken::Body);
        assert_eq!(token("entity.other.attribute-name"), HighlightToken::Body);
    }

    /// The outermost category wins, which is what keeps a string's own quotes
    /// the string's colour instead of the punctuation grey.
    #[test]
    fn a_scope_inside_a_category_takes_that_categorys_ink() {
        let mut stack = ScopeStack::new();
        for atom in [
            "source.rust",
            "string.quoted.double.rust",
            "punctuation.definition.string.begin.rust",
        ] {
            stack.push(Scope::new(atom).expect("a scope"));
        }
        assert_eq!(token_of(&stack), HighlightToken::Str);
        let mut comment = ScopeStack::new();
        for atom in [
            "source.rust",
            "comment.line.double-slash.rust",
            "punctuation.definition.comment.rust",
        ] {
            comment.push(Scope::new(atom).expect("a scope"));
        }
        assert_eq!(token_of(&comment), HighlightToken::Comment);
    }

    /// Detection: by extension, by whole name, by first line, and — the case
    /// that matters most — **not** by guessing.
    #[test]
    fn a_files_grammar_is_found_by_extension_then_by_first_line() {
        assert_eq!(
            syntax_for_file("main.rs", None).map(|s| s.name.as_str()),
            Some("Rust")
        );
        assert_eq!(
            syntax_for_file("build.py", None).map(|s| s.name.as_str()),
            Some("Python")
        );
        // No extension, and the first line says what it is.
        assert_eq!(
            syntax_for_file("install", Some("#!/bin/sh")).map(|s| s.name.as_str()),
            Some("Bourne Again Shell (bash)")
        );
        assert!(syntax_for_file("feed", Some("<?xml version=\"1.0\"?>")).is_some());
        // A whole name that is itself a token.
        assert!(syntax_for_file("Makefile", None).is_some());
        // Unknown stays unknown: no highlighting rather than the wrong one.
        assert!(syntax_for_file("notes.wibble", None).is_none());
        assert!(syntax_for_file("notes", None).is_none());
        // `.txt` resolves to Plain Text, which is the same thing as no grammar.
        assert!(syntax_for_file("notes.txt", None).is_none());
        // TOML and TypeScript are not in Sublime's default dump; the first cut
        // left them plain and the user ruled them in (2026-08-16) — bat's
        // extras carry both.
        assert_eq!(
            syntax_for_file("Cargo.toml", None).map(|s| s.name.as_str()),
            Some("TOML")
        );
        assert_eq!(
            syntax_for_file("app.ts", None).map(|s| s.name.as_str()),
            Some("TypeScript")
        );
        assert_eq!(
            syntax_for_file("App.tsx", None).map(|s| s.name.as_str()),
            Some("TypeScriptReact")
        );
    }

    /// A fence is read by its info string, and by nothing else.
    #[test]
    fn a_fences_grammar_is_its_info_string() {
        assert_eq!(
            syntax_for_fence(Some("rust")).map(|s| s.name.as_str()),
            Some("Rust")
        );
        // A token that is an *extension* rather than a name, which is the half
        // of `find_syntax_by_token` a fence relies on most.
        assert_eq!(
            syntax_for_fence(Some("py")).map(|s| s.name.as_str()),
            Some("Python")
        );
        // And a name, case-insensitively, with the fence's own whitespace on it.
        assert_eq!(
            syntax_for_fence(Some("  Rust  ")).map(|s| s.name.as_str()),
            Some("Rust")
        );
        assert_eq!(
            syntax_for_fence(Some("javascript")).map(|s| s.name.as_str()),
            Some("JavaScript")
        );
        // TypeScript is not in Sublime's default set; bat's extras carry it
        // (user ruling 2026-08-16), so ```ts reads as what it says.
        assert_eq!(
            syntax_for_fence(Some("ts")).map(|s| s.name.as_str()),
            Some("TypeScript")
        );
        assert!(syntax_for_fence(None).is_none());
        assert!(syntax_for_fence(Some("")).is_none());
        assert!(syntax_for_fence(Some("wibble")).is_none());
    }

    /// A Rust snippet comes out with the four landmarks a skim uses, in the four
    /// inks the palette declares — and in *both* palettes, because a token is a
    /// name and the theme is what gives it a value.
    #[test]
    fn a_rust_snippet_yields_keyword_string_comment_and_number_runs() {
        let lines = display("// a note\nfn main() {\n    let n = 42;\n    println!(\"hi\");\n}\n");
        let syntax = syntax_for_file("main.rs", None).expect("Rust is in the box");
        let highlighting = Highlighting::of(&lines, syntax);
        assert!(!highlighting.is_plain());
        assert_eq!(tokens(&highlighting, 0), vec![HighlightToken::Comment]);
        assert!(
            tokens(&highlighting, 1).contains(&HighlightToken::Keyword),
            "`fn` is a keyword: {:?}",
            tokens(&highlighting, 1)
        );
        assert!(
            tokens(&highlighting, 1).contains(&HighlightToken::Function),
            "`main` is a function: {:?}",
            tokens(&highlighting, 1)
        );
        assert!(
            tokens(&highlighting, 2).contains(&HighlightToken::Number),
            "`42` is a number: {:?}",
            tokens(&highlighting, 2)
        );
        assert!(
            tokens(&highlighting, 3).contains(&HighlightToken::Str),
            "`\"hi\"` is a string: {:?}",
            tokens(&highlighting, 3)
        );

        for palette in [bt_render::DARK_CHROME, bt_render::LIGHT_CHROME] {
            let ink = HighlightInk {
                palette: &palette,
                body: palette.preview_body_text,
            };
            let comment = highlighting.runs(0, &lines[0], (0, 40), ink);
            assert_eq!(comment.len(), 1);
            assert_eq!(comment[0].color, palette.hl_comment);
            let value = highlighting.runs(2, &lines[2], (0, 40), ink);
            assert!(
                value.iter().any(|run| run.color == palette.hl_number),
                "the 42 wears the number ink"
            );
            assert!(
                value.iter().any(|run| run.color == palette.hl_keyword),
                "the let wears the keyword ink"
            );
            assert!(
                value
                    .iter()
                    .any(|run| run.color == palette.preview_body_text),
                "and the identifier is left the body's own ink"
            );
            assert!(value.iter().all(|run| run.mono && !run.bold));
        }
    }

    /// State carries across lines: a block comment opened on one line is still
    /// a comment on the next, which is the whole reason this is a walk rather
    /// than a per-line regex.
    #[test]
    fn a_block_comment_stays_a_comment_across_lines() {
        let lines = display("/* one\n   two\n   three */\nlet x = 1;\n");
        let syntax = syntax_for_file("main.rs", None).expect("Rust");
        let highlighting = Highlighting::of(&lines, syntax);
        for line in 0..3 {
            assert_eq!(
                tokens(&highlighting, line),
                vec![HighlightToken::Comment],
                "line {line} is inside the block comment"
            );
        }
        assert!(tokens(&highlighting, 3).contains(&HighlightToken::Keyword));
    }

    /// The cap, both halves of it, and what being over it means: a plain
    /// document — not a truncated one, and not a slow one.
    #[test]
    fn a_document_over_either_cap_is_left_plain() {
        let syntax = syntax_for_file("main.rs", None).expect("Rust");
        let many: Vec<String> = (0..HIGHLIGHT_MAX_LINES + 1)
            .map(|_| "let x = 1;".to_owned())
            .collect();
        assert!(
            Highlighting::of(&many, syntax).is_plain(),
            "over the line cap"
        );
        let huge = vec!["x".repeat(HIGHLIGHT_MAX_BYTES + 1)];
        assert!(
            Highlighting::of(&huge, syntax).is_plain(),
            "over the byte cap"
        );
        // And one line under both caps is walked.
        let small = vec!["let x = 1;".to_owned()];
        assert!(!Highlighting::of(&small, syntax).is_plain());
    }

    /// A plain document still answers [`Highlighting::runs`], and answers with
    /// exactly what the painter used to build by hand.
    #[test]
    fn a_plain_document_still_hands_back_one_body_run() {
        let palette = bt_render::DARK_CHROME;
        let ink = HighlightInk {
            palette: &palette,
            body: palette.preview_body_text,
        };
        let plain = Highlighting::default();
        let runs = plain.runs(7, "anything at all", (0, 15), ink);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "anything at all");
        assert_eq!(runs[0].color, palette.preview_body_text);
    }

    /// Cutting a highlighted line by column is what a wrapped row asks for, and
    /// the two halves of a fold have to add back up to the line.
    #[test]
    fn a_column_range_cuts_the_spans_it_crosses() {
        let lines = display("let name = \"a rather long string literal\";\n");
        let syntax = syntax_for_file("main.rs", None).expect("Rust");
        let highlighting = Highlighting::of(&lines, syntax);
        let palette = bt_render::DARK_CHROME;
        let ink = HighlightInk {
            palette: &palette,
            body: palette.preview_body_text,
        };
        let whole = highlighting.runs(0, &lines[0], (0, 60), ink);
        let joined: String = whole.iter().map(|run| run.text.as_str()).collect();
        assert_eq!(joined, lines[0]);
        let head = highlighting.runs(0, &lines[0], (0, 20), ink);
        let tail = highlighting.runs(0, &lines[0], (20, 60), ink);
        let refolded: String = head
            .iter()
            .chain(tail.iter())
            .map(|run| run.text.as_str())
            .collect();
        assert_eq!(
            refolded, lines[0],
            "a fold loses nothing and repeats nothing"
        );
        assert!(head.len() > 1, "the head really does cross a span boundary");
        assert!(
            tail.iter().any(|run| run.color == palette.hl_string),
            "and the string's ink survives the fold"
        );
    }

    /// A wide character is two columns, and a cut that lands after one has to
    /// land after both of its cells.
    #[test]
    fn wide_characters_are_counted_in_the_cells_they_draw() {
        let lines = display("let s = \"名前\";\n");
        let syntax = syntax_for_file("main.rs", None).expect("Rust");
        let highlighting = Highlighting::of(&lines, syntax);
        let columns: usize = highlighting.line(0).iter().map(|s| s.columns).sum();
        assert_eq!(
            columns,
            bt_unicode::text_width(&lines[0]),
            "the spans cover the line in the cells it draws"
        );
    }

    /// The measured claim: a 5000-line Rust file is walked, and it is walked
    /// once. No timing assertion — a wall clock is not a property — but the runs
    /// have to be there, because "it was too slow so we skipped it" is exactly
    /// the failure a cap is supposed to make visible instead of silent.
    #[test]
    fn a_five_thousand_line_rust_file_is_walked() {
        let unit = [
            "/// A doc comment about the thing below.",
            "pub fn measure(input: &str, count: usize) -> Option<String> {",
            "    let mut total = 0usize;",
            "    for (index, cluster) in input.char_indices() {",
            "        total += index + count * 3;",
            "    }",
            "    (total > 0).then(|| format!(\"{total} \\\"cells\\\"\"))",
            "}",
            "",
            "struct Holder { name: String, count: u64 }",
        ];
        let lines: Vec<String> = unit
            .iter()
            .cycle()
            .take(5_000)
            .map(|line| (*line).to_owned())
            .collect();
        let syntax = syntax_for_file("big.rs", None).expect("Rust");
        let started = std::time::Instant::now();
        let highlighting = Highlighting::of(&lines, syntax);
        let elapsed = started.elapsed();
        assert!(!highlighting.is_plain());
        assert_eq!(highlighting.lines(), 5_000);
        let spans: usize = (0..5_000).map(|line| highlighting.line(line).len()).sum();
        assert!(spans > 20_000, "{spans} spans over 5000 lines");
        println!("5000 lines highlighted in {elapsed:?} ({spans} spans)");
    }
}
