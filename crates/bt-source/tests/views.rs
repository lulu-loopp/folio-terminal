//! **The four views, the boundaries, the scopes and the exemptions** — §2 of
//! `docs/plans/bt-app-split-prep.md`, on a fixture built for it.
//!
//! The fixture is `tests/fixtures/views/`, and every needle in it is spelled
//! once on purpose, so that a count here is a fact about a rule rather than
//! about the fixture. It is read and never compiled, which is what lets it hold
//! the one shape that is not legal Rust: two declarations of a name standing on
//! the same predicate.
//!
//! What each test is for, in the order the plan makes the rules:
//!
//! | § | Rule |
//! | --- | --- |
//! | 2.1 | four views, none of them a default; spelling is not value |
//! | 2.2 | a match crosses no removed region, no file boundary, and a literal match reports the literal's span |
//! | 2.5 | identifier boundaries, path and call shapes, declaration exemption |
//! | 2.4/3 | a scope is a Rust path, never a file |
//! | 2.7 | a name inside a macro is found, and reported as a candidate |

use std::path::Path;
use std::sync::Arc;

use bt_source::{
    Certainty, DiskScope, Index, ItemQuery, Needle, Pattern, QueryFailure, Scope, Search, Span,
    TargetId, TargetKind, TargetRoot, Universe, Vendor, View, Why,
};

fn fixture() -> Arc<Index> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("views");
    let universe = Universe::declare(
        "the views fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "views".to_owned(),
                kind: TargetKind::Library,
                name: "views".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::shared(&universe).expect("the fixture lowers")
}

/// The spans a search found, or the failure it made.
fn run(index: &Index, search: &Search) -> Vec<Span> {
    index
        .search(search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans()
}

fn count(index: &Index, pattern: Pattern, view: View) -> usize {
    run(index, &Search::new(Needle::new(pattern), view)).len()
}

// ── §2.1 — four views, and none of them a default ─────────────────────────

/// RED — **the same needle is a different question in each view.**
///
/// The one row that matters most is the second: a needle inside a string
/// literal survives `CodeKeepingLiterals`, because five guards in this tree
/// forbid a spelling that *is* a string, and a literal-stripping reading would
/// have turned every one of them quietly green.
///
/// MUTATION: strip literals in `CodeKeepingLiterals` and the byte-string row
/// goes red; mask nothing and the three comment rows do.
#[test]
fn a_view_is_a_question_and_there_is_no_default_one() {
    let index = fixture();

    for hidden in [
        "hidden_in_a_doc_comment",
        "hidden_in_a_line_comment",
        "hidden_in_a_block_comment",
    ] {
        assert!(
            index.contains(hidden, View::Raw),
            "{hidden} is written in this fixture"
        );
        assert!(
            !index.contains(hidden, View::CodeKeepingLiterals),
            "{hidden} is written in a comment and nowhere else"
        );
    }

    // A needle inside a string literal is code's subject, and it stays.
    assert!(index.contains("a_byte_string", View::CodeKeepingLiterals));
    // An attribute that begins with `doc` is code, not documentation text.
    assert!(index.contains("#[doc(hidden)]", View::CodeKeepingLiterals));

    // **Spelling is not value.** `"a\nb"` is six bytes of source and three of
    // string; `r"a\nb"` is the other way round.
    let escaped = run(
        &index,
        &Search::new(Needle::new(Pattern::text("a\nb")), View::LiteralValues),
    );
    assert_eq!(escaped.len(), 1, "one literal decodes to `a`, newline, `b`");
    assert_eq!(index.text(escaped[0]), "\"a\\nb\"");
    let raw = run(
        &index,
        &Search::new(Needle::new(Pattern::text("a\\nb")), View::LiteralValues),
    );
    assert_eq!(
        raw.len(),
        1,
        "one literal decodes to a backslash and an `n`"
    );
    assert_eq!(index.text(raw[0]), "r\"a\\nb\"");
    assert_eq!(
        count(&index, Pattern::text("a\\nb"), View::Raw),
        2,
        "both are *written* with a backslash and an `n`"
    );
}

/// RED — **a literal match reports the literal's span, not the length of what
/// it decodes to** (§2.2's third no-crossing rule).
///
/// The decoded value has no offsets of its own: it is a value this crate made,
/// and a span into it would point at bytes nobody wrote.
#[test]
fn a_literal_match_reports_where_the_literal_is_written() {
    let index = fixture();
    let found = run(
        &index,
        &Search::new(
            Needle::new(Pattern::text("a_byte_string")),
            View::LiteralValues,
        ),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(index.text(found[0]), "b\"a_byte_string\"");
    assert_eq!(
        found[0].len(),
        16,
        "the span is the spelling's sixteen bytes and not the value's thirteen"
    );
}

// ── §2.2 — a match crosses nothing ────────────────────────────────────────

/// RED — **a needle may not be manufactured at the join between two files.**
///
/// The union is an ordered concatenation with nothing between the files, so the
/// bytes really are there in a row; a whole-crate count is only meaningful if
/// the reading cannot find them.
///
/// MUTATION: search the union as one string and the last assertion goes red.
#[test]
fn a_match_crosses_neither_a_file_boundary_nor_a_mask() {
    let index = fixture();
    let files = index.files();
    assert_eq!(files.len(), 2, "lib.rs and second.rs");
    let first = index.text(files[0].span());
    let second = index.text(files[1].span());
    let across = format!("{}{}", &first[first.len() - 8..], &second[..8]);
    assert!(
        index.union().contains(&across),
        "the union really holds these bytes in a row"
    );
    assert!(
        !index.contains(&across, View::Raw),
        "and no reading of it may find them"
    );

    // A mask is opaque, not absent: a needle may not span the gap either.
    let comment = index
        .comments()
        .iter()
        .find(|comment| {
            index
                .text(comment.span())
                .contains("hidden_in_a_line_comment")
        })
        .expect("the line comment is masked");
    let straddling = &index.union()[comment.span().end() - 4..comment.span().end() + 4];
    assert!(index.contains(straddling, View::Raw));
    assert!(
        !index.contains(straddling, View::CodeKeepingLiterals),
        "a match may not cross a removed region"
    );
}

// ── §2.5 — boundaries, shapes, and the declaration exemption ──────────────

/// RED — **`stand_in` is not `strip_stand_in` and not `stand_in_window`.**
///
/// This is the guard §2.5 is written from, and the one-line occurrence count it
/// is *not* equivalent to: nine spellings of those eight characters are in the
/// fixture, three of them the name.
///
/// MUTATION: drop the boundary check and the identifier rows become nine.
#[test]
fn an_identifier_is_matched_whole_on_both_sides() {
    let index = fixture();
    assert_eq!(
        count(&index, Pattern::text("stand_in"), View::Raw),
        9,
        "every spelling, doc comments and neighbours included"
    );
    assert_eq!(
        count(&index, Pattern::text("stand_in"), View::CodeKeepingLiterals),
        7,
        "two of the nine are written in a doc comment"
    );
    assert_eq!(
        count(&index, Pattern::identifier("stand_in"), View::Raw),
        3,
        "the declaration and the two calls"
    );
    assert_eq!(
        count(&index, Pattern::identifier("stand_in"), View::Identifiers),
        3,
        "the byte view's boundary check and the token reading agree"
    );

    // A path-qualified name, and a call shape.
    assert_eq!(
        count(
            &index,
            Pattern::path("NativeWindow::stand_in"),
            View::Identifiers
        ),
        2,
        "the two calls; the declaration is not path-qualified"
    );
    assert_eq!(
        count(&index, Pattern::call("stand_in"), View::Identifiers),
        3,
        "`fn stand_in(` is a name with a parenthesis after it too — which is \
         exactly why the declaration exemption exists"
    );
}

/// RED — **the declaration exemption is a named argument, and what it removed
/// is in the answer** (§2.5).
///
/// "Only tests name `stand_in`" is expressible only with it: the `pub const fn`
/// that declares the name is product code, and a guard that counted it would be
/// asserting something that is not true of any tree.
///
/// MUTATION: exempt the whole item instead of the declaration and the call
/// inside `through_a_macro` disappears too; exempt nothing and the count is
/// three.
#[test]
fn a_declaration_exemption_is_asked_for_and_is_reported() {
    let index = fixture();
    let search = Search::new(Needle::new(Pattern::call("stand_in")), View::Identifiers)
        .exempting_declarations_of(ItemQuery::method("NativeWindow", "stand_in"));
    let found = index.search(&search).expect("one declaration to exempt");
    assert_eq!(found.len(), 2, "{}", found.report(&index));
    let excluded = found.excluded();
    assert_eq!(excluded.len(), 1);
    assert!(
        matches!(&excluded[0].why, Why::Declaration(identity) if identity.contains("stand_in")),
        "{:?}",
        excluded[0]
    );
    assert!(
        found
            .report(&index)
            .contains("not counted, the declaration"),
        "{}",
        found.report(&index)
    );

    // The exemption is the declaration and not the body: a name used inside its
    // own item still counts.
    let declaration = index
        .one(&ItemQuery::method("NativeWindow", "stand_in"))
        .expect("one declaration");
    assert!(
        declaration.declaration().within(declaration.whole())
            && declaration.declaration().end()
                == declaration.body().expect("it has a body").start()
    );
}

/// RED — **an exemption that is not one item is a refusal**, not a guess.
#[test]
fn an_exemption_that_names_no_item_refuses() {
    let index = fixture();
    let failure = index
        .search(
            &Search::new(Needle::new(Pattern::identifier("stand_in")), View::Raw)
                .exempting_declarations_of(ItemQuery::function("no_such_declaration")),
        )
        .expect_err("nothing declares that");
    assert!(
        matches!(failure, QueryFailure::Multiplicity { .. }),
        "{failure}"
    );
}

// ── §2.4 / §3 — a scope is a Rust path ────────────────────────────────────

/// RED — **a scope is a module path or an item, never a file**, and a scope
/// that names nothing refuses rather than answering zero.
///
/// MUTATION: make an empty scope answer "no occurrences" and the last block
/// goes green while meaning nothing.
#[test]
fn a_scope_is_a_rust_path_and_an_empty_one_is_loud() {
    let index = fixture();

    let inside = run(
        &index,
        &Search::new(
            Needle::new(Pattern::identifier("only_inside_inner")),
            View::Identifiers,
        )
        .in_scope(Scope::Module("crate::inner".to_owned())),
    );
    assert_eq!(inside.len(), 1, "the declaration, not the call in `tests`");
    assert_eq!(
        count(
            &index,
            Pattern::identifier("only_inside_inner"),
            View::Identifiers
        ),
        2,
        "the declaration and the call"
    );

    let in_one_item = run(
        &index,
        &Search::new(
            Needle::new(Pattern::identifier("stand_in")),
            View::Identifiers,
        )
        .in_scope(Scope::Item(ItemQuery::function("through_a_macro"))),
    );
    assert_eq!(in_one_item.len(), 1);

    let empty = index
        .search(
            &Search::new(Needle::new(Pattern::identifier("stand_in")), View::Raw)
                .in_scope(Scope::Module("crate::nowhere".to_owned())),
        )
        .expect_err("no module is called that");
    assert!(matches!(empty, QueryFailure::EmptyScope { .. }), "{empty}");
}

// ── §2.7 — a name inside a macro ──────────────────────────────────────────

/// RED — **a name inside a macro's token tree is found, and is a candidate.**
///
/// The parser's visitor does not descend into a macro, so a reader built on it
/// alone would lose the call in `through_a_macro` — which is coverage a text
/// search has today. It is found, and it is marked, and a guard that needs a
/// placed occurrence gets a refusal rather than a candidate.
///
/// MUTATION: stop recording macro spans and every occurrence becomes resolved,
/// so the last block goes green while the distinction is gone.
#[test]
fn a_name_inside_a_macro_is_found_and_says_that_it_is_lexical() {
    let index = fixture();
    let search = Search::new(
        Needle::new(Pattern::identifier("stand_in")),
        View::Identifiers,
    );
    let found = index.search(&search).expect("three names");
    assert_eq!(found.counts(), (2, 1), "{}", found.report(&index));
    let candidate = found
        .occurrences()
        .iter()
        .find(|found| found.certainty == Certainty::Candidate)
        .expect("the call inside `println!`");
    assert!(
        index
            .locate(candidate.span.start())
            .is_some_and(|location| location.line > 1)
    );

    let refused = index
        .search(&search.clone().requiring_resolved())
        .expect_err("one of the three is inside a macro");
    assert!(
        matches!(refused, QueryFailure::LexicalCandidate { .. }),
        "{refused}"
    );
}

// ── the loud failures of the query surface ────────────────────────────────

/// RED — **plain bytes are not a question the identifier view answers.**
///
/// `contains("stand_in", View::Identifiers)` used to be a substring search
/// inside token names, which is true on a tree whose only match is
/// `stand_in_window`. The combination is refused, and the message names the
/// three shapes that are meant instead.
#[test]
fn plain_bytes_asked_of_the_identifier_view_are_refused() {
    let index = fixture();
    let failure = index
        .search(&Search::new(
            Needle::new(Pattern::text("stand_in")),
            View::Identifiers,
        ))
        .expect_err("a text pattern is not a name");
    assert!(
        matches!(failure, QueryFailure::PatternViewMismatch { .. }),
        "{failure}"
    );
    assert!(
        failure.to_string().contains("Pattern::identifier"),
        "{failure}"
    );
    // And the short form still answers, because it asks for a name.
    assert_eq!(index.count_identifier("stand_in", View::Identifiers), 3);
    assert!(!index.contains("stand_in_wind", View::Identifiers));
}

/// RED — **expected multiplicity is an argument** (§2.4), and one declaration
/// per variant is not the same statement as two declarations.
///
/// MUTATION: let `OnePerVariant` accept the same predicate twice and the last
/// block goes green with two things read as one identity's two arms.
#[test]
fn multiplicity_is_said_and_a_repeated_predicate_is_a_refusal() {
    let index = fixture();

    let bare = index
        .find(&ItemQuery::function("two_arms"))
        .expect_err("two declarations, one expected");
    assert!(matches!(bare, QueryFailure::Multiplicity { .. }), "{bare}");
    assert_eq!(
        index
            .find(&ItemQuery::function("two_arms").one_per_variant())
            .expect("two arms of one identity")
            .len(),
        2
    );
    assert_eq!(
        index
            .find(&ItemQuery::function("two_arms").in_variant(&["windows"]))
            .expect("one arm, named")
            .len(),
        1
    );

    let collision = index
        .find(&ItemQuery::function("same_predicate_twice").one_per_variant())
        .expect_err("the same predicate is not two arms");
    assert!(
        matches!(collision, QueryFailure::VariantCollision { .. }),
        "{collision}"
    );
}
