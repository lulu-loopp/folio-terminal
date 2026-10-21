//! **`bt-platform`'s menus and dialogs carry no words of their own**
//! (T-NATIVE-LABELS-FROM-THE-CALLER; `docs/ARCHITECTURE-GUARDS.md`, "Native labels"). A guard:
//! its subject is how the code is written.
//!
//! Everything a person reads on a native menu or dialog is handed in by the caller from
//! `bt-app`'s `i18n` table, so the interface language reaches the system's surfaces too. This
//! guard holds the guarded items to that: no string literal in their product bytes begins with an
//! ASCII capital letter, which is the shape every label written so far has had.
//!
//! # The grammar
//!
//! * **Read**: every string literal (plain, raw, byte and C strings), wherever it is written —
//!   in an expression, a `const`, or a macro's tokens — in the product bytes (`#[cfg(test)]` and
//!   test files out) of the guarded scope: [`FILES`] whole, the `impl` blocks of [`TYPES`] on
//!   every arm, and the free functions [`FUNCTIONS`].
//! * **Refused**: a literal whose decoded text begins with an ASCII capital letter.
//! * **Not a label, by its place**: a literal inside an attribute (`#[…]`, a doc comment
//!   included — a class name, a link name); an ABI's name right after `extern`; and **a literal inside `Err(…)`, a `.map_err(…)` or
//!   an `.ok_or_else(…)` is an error text, not a label** — the parenthesis group the literal sits
//!   in, at any depth, is the argument list of that call.
//! * **Allowed by row**: [`ALLOWED`] — a literal that must stay, by file and text, with its
//!   reason. The list only shrinks, and a row no guarded item uses is refused.
//! * **Unreadable, refused by its line**: a literal inside a `macro_rules!` definition — it lands
//!   in an expansion this reading cannot see.
//!
//! The fixture under `tests/fixtures/native_labels/` plants each case; the last test holds
//! `bt-platform` itself.

use std::path::Path;
use std::str::FromStr;

use bt_source::{
    Certainty, DiskScope, FileRecord, ImplRecord, Index, ItemRecord, LiteralValue, Occurrence,
    Span, TargetId, TargetKind, TargetRoot, Universe, Vendor, report,
};
use proc_macro2::{Delimiter, TokenStream, TokenTree};

/// The modules guarded whole, by the file they are written in.
const FILES: &[&str] = &[
    "src/menu.rs",
    "src/macos_menu.rs",
    "src/macos_dialogs.rs",
    "src/explorer_command.rs",
];

/// The types whose `impl` blocks are guarded, on every arm that writes one.
const TYPES: &[&str] = &[
    "MathContextMenu",
    "FolderPicker",
    "ImagePicker",
    "SaveFilePicker",
];

/// The free functions guarded, wherever they are written.
const FUNCTIONS: &[&str] = &[
    "track_math_context_menu",
    "math_context_menu_subclass",
    "image_picker_subclass",
    "save_file_picker_subclass",
    "folder_picker_subclass",
    "show_shell_picker",
    "show_save_picker",
    "message_box",
    "standalone_alert",
    "toast_xml",
];

/// **The literals that must stay**, by the file they are written in (a path suffix) and their
/// text, each with its reason. The list only shrinks.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "src/lib.rs",
    "JSON",
    "a format's name beside the settings-file filter, spelled the same in every language",
)];

/// What the guard is asked to read, and the rows it allows.
struct Guarded<'a> {
    files: &'a [&'a str],
    types: &'a [&'a str],
    functions: &'a [&'a str],
    allowed: &'a [(&'a str, &'a str, &'a str)],
}

/// Where a literal sits, as far as the grammar cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Somewhere a label could be.
    Open,
    /// Inside `#[…]` or `#![…]`.
    Attribute,
    /// Inside the argument list of `Err(…)`, `.map_err(…)` or `.ok_or_else(…)`.
    ErrorText,
    /// Inside a `macro_rules!` definition.
    MacroDefinition,
    /// Right after `extern`: an ABI's name (`extern "C"`).
    Abi,
}

/// One literal of a file, by its absolute offset, and its place.
struct Found {
    start: usize,
    place: Place,
}

/// Every literal of one file's text, with its place, at absolute offsets.
fn literals_of(text: &str, base: usize) -> Vec<Found> {
    let stream = TokenStream::from_str(text).expect("a file the index holds is a token stream");
    let mut found = Vec::new();
    walk(stream, Place::Open, base, &mut found);
    found
}

fn walk(stream: TokenStream, place: Place, base: usize, found: &mut Vec<Found>) {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Literal(literal) => {
                let after_extern = at > 0
                    && matches!(&trees[at - 1], TokenTree::Ident(ident) if ident == "extern");
                found.push(Found {
                    start: base + literal.span().byte_range().start,
                    place: if place == Place::Open && after_extern {
                        Place::Abi
                    } else {
                        place
                    },
                });
            }
            TokenTree::Group(group) => {
                let inner = if place == Place::Open {
                    group_place(&trees, at, group.delimiter())
                } else {
                    place
                };
                walk(group.stream(), inner, base, found);
            }
            TokenTree::Ident(_) | TokenTree::Punct(_) => {}
        }
    }
}

/// The place a group opens, read off the tokens in front of it.
fn group_place(trees: &[TokenTree], at: usize, delimiter: Delimiter) -> Place {
    let before = |back: usize| at.checked_sub(back).map(|index| &trees[index]);
    let is_punct = |tree: Option<&TokenTree>, ch: char| matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == ch);
    let is_ident = |tree: Option<&TokenTree>, name: &str| matches!(tree, Some(TokenTree::Ident(ident)) if ident == name);
    match delimiter {
        Delimiter::Bracket
            if is_punct(before(1), '#')
                || (is_punct(before(1), '!') && is_punct(before(2), '#')) =>
        {
            Place::Attribute
        }
        Delimiter::Parenthesis if is_ident(before(1), "Err") => Place::ErrorText,
        Delimiter::Parenthesis
            if (is_ident(before(1), "map_err") || is_ident(before(1), "ok_or_else"))
                && is_punct(before(2), '.') =>
        {
            Place::ErrorText
        }
        _ if is_punct(before(2), '!') && is_ident(before(3), "macro_rules") => {
            Place::MacroDefinition
        }
        _ => Place::Open,
    }
}

/// The spans the guard reads: the guarded files whole, the types' blocks, the free functions.
fn guarded_spans(index: &Index, guarded: &Guarded<'_>) -> Vec<Span> {
    let mut spans: Vec<Span> = index
        .files()
        .iter()
        .filter(|file| {
            let path = file.path().to_string_lossy().replace('\\', "/");
            guarded.files.iter().any(|suffix| path.ends_with(suffix))
        })
        .map(FileRecord::span)
        .collect();
    spans.extend(
        index
            .impls()
            .iter()
            .filter(|block| guarded.types.contains(&block.type_owner()))
            .map(ImplRecord::whole),
    );
    spans.extend(
        index
            .items()
            .iter()
            .filter(|item| item.type_owner().is_none() && guarded.functions.contains(&item.name()))
            .map(ItemRecord::whole),
    );
    spans
}

fn where_is(index: &Index, span: Span) -> String {
    index
        .locate(span.start())
        .map_or_else(|| format!("{span:?}"), |location| location.to_string())
}

/// The index's record of the string literal starting at `start` — its span and decoded text —
/// or `None` for a literal that is not a string (a number, a character).
fn string_at(index: &Index, start: usize) -> Option<(Span, String)> {
    let record = index
        .literals()
        .iter()
        .find(|record| record.span().start() == start)?;
    let text = match record.value() {
        LiteralValue::Str(text) => text.clone(),
        LiteralValue::ByteStr(bytes) | LiteralValue::CStr(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
        _ => return None,
    };
    Some((record.span(), text))
}

/// **The refusals** over one index: each capitalised literal in an open place of the guarded
/// product bytes that no row allows, each literal in a `macro_rules!` definition there, and each
/// row no literal uses.
fn judge(index: &Index, guarded: &Guarded<'_>) -> Vec<String> {
    let spans = guarded_spans(index, guarded);
    let mut refusals = Vec::new();
    let mut used = vec![false; guarded.allowed.len()];
    for file in index.files() {
        if !spans.iter().any(|span| span.within(file.span())) {
            continue;
        }
        let path = file.path().to_string_lossy().replace('\\', "/");
        for found in literals_of(index.text(file.span()), file.span().start()) {
            let Some((span, text)) = string_at(index, found.start) else {
                continue;
            };
            let in_scope = spans.iter().any(|guarded| span.within(*guarded));
            let product = Occurrence {
                span,
                certainty: Certainty::Resolved,
            }
            .in_the_product(index);
            if !in_scope || !product {
                continue;
            }
            match found.place {
                Place::Attribute | Place::ErrorText | Place::Abi => {}
                Place::MacroDefinition => refusals.push(format!(
                    "{}: a literal in a `macro_rules!` definition ({text:?}) lands in an \
                     expansion this guard cannot see — pass the words in from the caller",
                    where_is(index, span)
                )),
                Place::Open => {
                    if !text.starts_with(|c: char| c.is_ascii_uppercase()) {
                        continue;
                    }
                    let row = guarded.allowed.iter().position(|(suffix, allowed, _)| {
                        path.ends_with(suffix) && *allowed == text
                    });
                    match row {
                        Some(row) => used[row] = true,
                        None => refusals.push(format!(
                            "{}: {text:?} — a capitalised literal in a native menu or dialog's \
                             product item; a person reads it, so the caller passes it from \
                             `bt-app`'s i18n table",
                            where_is(index, span)
                        )),
                    }
                }
            }
        }
    }
    for ((suffix, text, reason), used) in guarded.allowed.iter().zip(used) {
        assert!(!reason.is_empty(), "a row says why: {text}");
        if !used {
            refusals.push(format!(
                "{suffix}: the row for {text:?} allows a literal no guarded item writes — take \
                 it off"
            ));
        }
    }
    refusals
}

/// The fixture crate.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("native_labels");
    let universe = Universe::declare(
        "the native-labels fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "native-labels".to_owned(),
                kind: TargetKind::Library,
                name: "native_labels".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// RED (planted) — **a capitalised literal in a guarded product item is refused, a literal in a
/// `macro_rules!` definition is refused by its line, and a row nothing uses is refused; nothing
/// else is.** The fixture's error texts (`Err(…)`, `.map_err`, `.ok_or_else`), its attributes,
/// its ABI name, its allowlisted literal and its test module pass, and an unguarded item is not
/// read.
#[test]
fn a_capitalised_literal_in_a_guarded_item_is_named() {
    let index = fixture();
    let allowed = [
        ("lib.rs", "JSON", "the fixture's format name"),
        ("lib.rs", "Gone", "a row nothing uses"),
    ];
    let items = Guarded {
        files: &[],
        types: &["Menu"],
        functions: &["labels"],
        allowed: &allowed,
    };
    let refusals = judge(&index, &items);
    let named = |needle: &str| {
        refusals
            .iter()
            .filter(|refusal| refusal.contains(needle))
            .count()
    };
    assert_eq!(
        named("\"Copy LaTeX\" — a capitalised literal"),
        1,
        "the product's label, and not the test module's: {refusals:#?}"
    );
    assert_eq!(named("the row for \"Gone\""), 1, "{refusals:#?}");
    assert_eq!(
        refusals.len(),
        2,
        "nothing else is refused — the error texts, the attribute, the allowed row, the unguarded \
         function and the test module pass: {refusals:#?}"
    );

    let whole = judge(
        &index,
        &Guarded {
            files: &["native_labels/lib.rs"],
            ..items
        },
    );
    let named = |needle: &str| {
        whole
            .iter()
            .filter(|refusal| refusal.contains(needle))
            .count()
    };
    assert_eq!(
        named("a literal in a `macro_rules!` definition (\"Unseen\")"),
        1,
        "{whole:#?}"
    );
    assert_eq!(
        named("\"Folio\" — a capitalised literal"),
        1,
        "a whole guarded file reads every product item: {whole:#?}"
    );
    assert_eq!(whole.len(), 4, "{whole:#?}");
}

/// GUARD — **`bt-platform`'s menus and dialogs carry no capitalised literal of their own**, but
/// for the allowed rows.
///
/// MUTATION: write `windows::core::w!("Copy LaTeX")` back into `track_math_context_menu`, or
/// `ns_string!("Copy LaTeX")` into `macos_dialogs::track_formula_menu` — red, by its line.
#[test]
fn bt_platform_native_labels_come_from_the_caller() {
    let index = Index::of_package("bt-platform");
    let refusals = judge(
        index,
        &Guarded {
            files: FILES,
            types: TYPES,
            functions: FUNCTIONS,
            allowed: ALLOWED,
        },
    );
    assert!(refusals.is_empty(), "{}", refusals.join("\n"));
}
