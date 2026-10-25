//! **`bt-platform`'s menus and dialogs carry no words of their own**
//! (T-NATIVE-LABELS-FROM-THE-CALLER, widened by T-NATIVE-LABELS-GUARD-REACH;
//! `docs/ARCHITECTURE-GUARDS.md`, "Native labels"). A guard: its subject is how the code is
//! written.
//!
//! Everything a person reads on a native menu or dialog is handed in by the caller from
//! `bt-app`'s `i18n` table, so the interface language reaches the system's surfaces too. This
//! guard holds the guarded items, and everything they reach, to that: **a string literal that
//! holds a letter is a label** unless its place or a row of [`ALLOWED`] says otherwise.
//!
//! # The grammar
//!
//! * **The scope**: the product bytes (`#[cfg(test)]` and test files out) of [`FILES`] whole,
//!   the `impl` blocks of [`TYPES`] on every arm, the free functions [`FUNCTIONS`] — and **every
//!   item and every `const`/`static` initializer the call graph (`bt_source::call_graph`, every
//!   product arm) reaches from a callable among them**: a helper on none of the lists, a `const`
//!   label named from a guarded item, a method reached through a first-party trait object. The
//!   walk stops at the items of [`BOUNDARY`] (a candidate an ambiguous call carries and never
//!   runs), each with its reason.
//! * **Edges the graph cannot follow**, by kind: an ambiguous method, a call on another arm and a
//!   dynamic call that carries candidates are followed into **every** candidate; a dynamic call
//!   with none, an unplaced path, a generated item, a cut-off chain and an unparsed body are
//!   refused by their line, unless [`UNREAD`] reviews the edge (its caller, the name it calls,
//!   why what runs there writes no label). A foreign callee is the boundary, not a failure.
//! * **Macros**: a foreign macro's argument tokens are read where they are written. A call of a
//!   `macro_rules!` of the package whose definition holds a string literal with a letter is
//!   refused by its line unless the macro is a row of [`MACROS`]; a literal inside a
//!   `macro_rules!` definition in the scope is refused by its line (it lands in an expansion this
//!   reading cannot see).
//! * **Read**: every string literal (plain, raw, byte and C strings) in the scope, wherever it is
//!   written — an expression, a `const`, a macro's tokens.
//! * **A label**: a literal whose decoded text, its `&` mnemonics taken out (`&&` stays one `&`)
//!   and its ends trimmed of white space, holds a letter (`char::is_alphabetic`). A literal with
//!   none (`""`, `";"`, `"*.*"`) carries no word.
//! * **Not a label, by its place** — the closed list; any other diagnostic-looking text is a row:
//!   - a literal inside an attribute (`#[…]`, a doc comment included);
//!   - an ABI's name right after `extern`;
//!   - an error text: inside the argument list of `Err(…)`, `.map_err(…)` or `.ok_or_else(…)`;
//!   - a programmer's text: inside the argument list of `.expect(…)`, `panic!`, `unreachable!`,
//!     `assert!`, `assert_eq!`, `assert_ne!`, `debug_assert!`, `debug_assert_eq!` or
//!     `debug_assert_ne!`.
//!
//!   The place is the parenthesis (or macro) group the literal sits in, at any depth.
//! * **Allowed by row**: [`ALLOWED`] — a literal that must stay, by the file it is written in (a
//!   path suffix) and its text **as written** (a row for `Copy` does not allow `&Copy`), with its
//!   reason. A row no literal of the scope uses is refused, and so is a [`MACROS`] row no call
//!   uses, a [`BOUNDARY`] row that names nothing or that the walk never stops at, and an
//!   [`UNREAD`] row no edge uses. The four lists only shrink: `scripts/ci/check-native-labels.ps1`
//!   holds them to the merge base's, and grown lists need a dated `// widened` line in a change
//!   to this grammar.
//! * **Unreadable, refused by its line**: a literal the index holds no record of.
//! * **The lists are checked**: an entry of [`FILES`], [`TYPES`] or [`FUNCTIONS`] that names
//!   nothing in the index is refused, so a renamed item cannot drop out of the guard silently.
//!
//! # What it does not see (limits)
//!
//! A literal `bt-app` passes in (the caller's table is the subject of `bt-app`'s own runtime
//! tests); an item a macro generates; what a foreign function does with the words.
//!
//! The fixture under `tests/fixtures/native_labels/` plants each case; the last test holds
//! `bt-platform` itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use bt_source::call_graph::{Arms, CallGraph, Edge, ItemId, Resolver, Target, Unresolved};
use bt_source::{
    Certainty, DiskScope, FileRecord, ImplRecord, Index, ItemRecord, LiteralRecord, LiteralValue,
    MacroKind, MacroRecord, Occurrence, Span, TargetId, TargetKind, TargetRoot, Universe, Vendor,
    report,
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

// widened 2026-10-25 by T-NATIVE-LABELS-GUARD-REACH (+77 rows): every literal with a word is a
// label and the scope is what the call graph reaches (41 ALLOWED, 16 BOUNDARY, 20 UNREAD)
/// **The literals that must stay**, by the file they are written in (a path suffix) and their
/// text as written, each with its reason. The list only shrinks
/// (`scripts/ci/check-native-labels.ps1`).
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "src/lib.rs",
        "JSON",
        "a format's name beside the settings-file filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "<toast launch=\"{launch}\" activationType=\"foreground\"><visual><binding template=\"ToastGeneric\"><text>{title}</text>",
        "the toast's XML markup; its words are the caller's `title` and `body`",
    ),
    (
        "src/lib.rs",
        "<text>{}</text>",
        "the toast's XML markup; its words are the caller's `title` and `body`",
    ),
    (
        "src/lib.rs",
        "</binding></visual></toast>",
        "the toast's XML markup; its words are the caller's `title` and `body`",
    ),
    (
        "src/lib.rs",
        "&amp;",
        "an XML entity `xml_escape` writes, not a word",
    ),
    (
        "src/lib.rs",
        "&lt;",
        "an XML entity `xml_escape` writes, not a word",
    ),
    (
        "src/lib.rs",
        "&gt;",
        "an XML entity `xml_escape` writes, not a word",
    ),
    (
        "src/lib.rs",
        "&quot;",
        "an XML entity `xml_escape` writes, not a word",
    ),
    (
        "src/lib.rs",
        "&apos;",
        "an XML entity `xml_escape` writes, not a word",
    ),
    (
        "src/lib.rs",
        "png",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "jpg",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "jpeg",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "webp",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "gif",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "svg",
        "a file extension of the picture filter, spelled the same in every language",
    ),
    (
        "src/lib.rs",
        "*.{extension}",
        "a filter pattern built from an extension, not a word",
    ),
    (
        "src/lib.rs",
        "*.{ext}",
        "a filter pattern built from an extension, not a word",
    ),
    (
        "src/lib.rs",
        "*.json",
        "the settings-file filter's pattern, not a word",
    ),
    (
        "src/lib.rs",
        "folder",
        "a noun of the pickers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/lib.rs",
        "picture",
        "a noun of the pickers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/lib.rs",
        "program",
        "a noun of the pickers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/lib.rs",
        "settings file",
        "a noun of the pickers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "json",
        "the settings file's extension handed to `UTType`, not a word",
    ),
    (
        "src/macos_dialogs.rs",
        "the folder chooser",
        "the door's name in its error texts (`window_for`, `nothing_to_ask`)",
    ),
    (
        "src/macos_dialogs.rs",
        "the file chooser",
        "the door's name in its error texts (`window_for`, `nothing_to_ask`)",
    ),
    (
        "src/macos_dialogs.rs",
        "the save dialog",
        "the door's name in its error texts (`window_for`, `nothing_to_ask`)",
    ),
    (
        "src/macos_dialogs.rs",
        "the formula menu",
        "the door's name in its error texts (`window_for`, `nothing_to_ask`)",
    ),
    (
        "src/macos_dialogs.rs",
        "taking down the formula menu",
        "the door's name in its error text (`window_thread`)",
    ),
    (
        "src/macos_dialogs.rs",
        "folder",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "picture",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "program",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "settings file",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "file",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics",
    ),
    (
        "src/macos_dialogs.rs",
        "{title}",
        "the standard-error line of an alert no one can see; the words are the caller's",
    ),
    (
        "src/macos_dialogs.rs",
        "{text}",
        "the standard-error line of an alert no one can see; the words are the caller's",
    ),
    (
        "src/macos_impl.rs",
        "{what} was asked from a thread that is not the window's",
        "an error text a door returns (`off_the_window_thread`)",
    ),
    (
        "src/macos_impl.rs",
        "{what}: this handle names no window on the screen",
        "an error text a door returns (`nothing_to_ask`)",
    ),
    (
        "src/macos_menu.rs",
        "installing the menu bar",
        "the door's name in its error text (`window_thread`)",
    ),
    (
        "src/macos_menu.rs",
        "refreshing the menu bar",
        "the door's name in its error text (`window_thread`)",
    ),
    (
        "src/portable_impl.rs",
        "{what} is not on this platform yet",
        "an error text a door returns on a platform with no native dialogs",
    ),
    (
        "src/portable_impl.rs",
        "{title}",
        "the standard-error line of a platform with no alert; the words are the caller's",
    ),
    (
        "src/portable_impl.rs",
        "{text}",
        "the standard-error line of a platform with no alert; the words are the caller's",
    ),
];

/// **The `macro_rules!` of `bt-platform` whose definition holds a string literal and that the
/// guarded scope may call**, each with its reason. The list only shrinks.
const MACROS: &[(&str, &str)] = &[];

/// **The items the walk stops at** (by [`Resolver::key`]), each with its reason: a candidate an
/// ambiguous method call carries that the call never runs (a `.push` on a `Vec`). The list only
/// shrinks; a row the walk never stops at is refused.
const BOUNDARY: &[(&str, &str)] = &[
    (
        "bt-platform crate::elevated_protocol::codec::Decoder::push",
        "a candidate of `units.push(0)` on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::webview::macos::Shared::push",
        "a candidate of `units.push(0)` on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::webview::Shared::push",
        "a candidate of `units.push(0)` on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::macos_notify::Permission::code",
        "a candidate of `error.code()` on a `windows::core::Error` (the pickers' cancel test)",
    ),
    (
        "bt-platform crate::trust::arm::Engine::chain",
        "a candidate of `.encode_utf16().chain(once(0))`, an iterator (the wide strings)",
    ),
    (
        "bt-platform crate::GroundBand::is_empty",
        "a candidate of `.is_empty()` on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::webview::RehostCompensation::is_empty",
        "a candidate of `.is_empty()` on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::deferred_removal::Item::path",
        "a candidate of `url.path()` on an `NSURL` (the chooser's answer)",
    ),
    (
        "bt-platform crate::exclusive_create::Directory::path",
        "a candidate of `url.path()` on an `NSURL` (the chooser's answer)",
    ),
    (
        "bt-platform crate::https_download::DownloadMonitor::begin",
        "a candidate of `ivars.state.begin()` on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::macos_clipboard_payload::<MacClipboard as ClipboardPort>::begin",
        "a candidate of `ivars.state.begin()` on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::windows_clipboard::<WindowsClipboard as ClipboardPort>::begin",
        "a candidate of `ivars.state.begin()` on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::elevated_protocol::codec::Cursor::take",
        "a candidate of `.take()` on the formula menu's `Deferred` or on a `RefCell<Option<_>>`",
    ),
    (
        "bt-platform crate::https_download::DownloadMonitor::take",
        "a candidate of `.take()` on the formula menu's `Deferred` or on a `RefCell<Option<_>>`",
    ),
    (
        "bt-platform crate::web_environment::Pending::take",
        "a candidate of `.take()` on the formula menu's `Deferred` or on a `RefCell<Option<_>>`",
    ),
    (
        "bt-platform crate::web_environment::ControllerSlots::take",
        "a candidate of `.take()` on the formula menu's `Deferred` or on a `RefCell<Option<_>>`",
    ),
];

/// **The edges the graph cannot follow, reviewed**: the caller (by [`Resolver::node_key`]), the
/// name it calls, and why what runs there writes no label. The list only shrinks; a row no edge
/// uses is refused.
const UNREAD: &[(&str, &str, &str)] = &[
    (
        "bt-platform crate::explorer_command::<Command_Impl as IExplorerCommand_Impl>::Invoke",
        "(value)",
        "the verb's `invoke` is the handler `bt-app` registered; it opens a window and writes no label here",
    ),
    (
        "bt-platform crate::macos_dialogs::<MathContextMenu as Drop>::drop",
        "folioPopFormulaMenu",
        "a selector of the `define_class!` class written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::request",
        "folioPopFormulaMenu",
        "a selector of the `define_class!` class written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "alloc",
        "the `define_class!` class's allocation (objc2); its body is in this guarded file",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "super",
        "`msg_send![super(...), init]` to `NSObject` (objc2)",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "init",
        "`msg_send![super(...), init]` to `NSObject` (objc2)",
    ),
    (
        "bt-platform crate::macos_dialogs::message_box",
        "NSApp",
        "AppKit's shared application, a foreign static",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "alloc",
        "the `define_class!` class's allocation (objc2); its body is in this guarded file",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "super",
        "`msg_send![super(...), init]` to `NSObject` (objc2)",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "init",
        "`msg_send![super(...), init]` to `NSObject` (objc2)",
    ),
    (
        "bt-platform crate::macos_menu::build",
        "clone",
        "`#[derive(Clone)]`, the language's derive: it writes no literal",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "clone",
        "`#[derive(Clone)]`, the language's derive: it writes no literal",
    ),
    (
        "bt-platform crate::macos_menu::dock_menu",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::install",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::installed_plan",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::is_installed",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::the_chain_declined",
        "INSTALLED",
        "the `thread_local!` written in this guarded file, read whole",
    ),
    (
        "bt-platform crate::macos_menu::the_chain_declined",
        "(value)",
        "the bar's `send` is the sender `bt-app` handed to `install`; it carries a choice, not a label",
    ),
    (
        "bt-platform crate::windows_impl::message_box",
        "MessageBoxTimeoutW",
        "user32's message box, declared in the function's own `extern` block: it shows the caller's title and text",
    ),
];

/// What the guard is asked to read, and the rows it allows.
struct Guarded<'a> {
    files: &'a [&'a str],
    types: &'a [&'a str],
    functions: &'a [&'a str],
    allowed: &'a [(&'a str, &'a str, &'a str)],
    macros: &'a [(&'a str, &'a str)],
    boundary: &'a [(&'a str, &'a str)],
    unread: &'a [(&'a str, &'a str, &'a str)],
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
    /// Inside the argument list of `.expect(…)` or of one of [`PROGRAMMER_MACROS`].
    ProgrammerText,
    /// Inside a `macro_rules!` definition.
    MacroDefinition,
    /// Right after `extern`: an ABI's name (`extern "C"`).
    Abi,
}

/// The macros whose arguments are a programmer's text, not a label: the closed list.
const PROGRAMMER_MACROS: &[&str] = &[
    "panic",
    "unreachable",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
];

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
    let is_one_of = |tree: Option<&TokenTree>, names: &[&str]| matches!(tree, Some(TokenTree::Ident(ident)) if names.iter().any(|name| ident == name));
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
        Delimiter::Parenthesis if is_ident(before(1), "expect") && is_punct(before(2), '.') => {
            Place::ProgrammerText
        }
        _ if is_punct(before(1), '!') && is_one_of(before(2), PROGRAMMER_MACROS) => {
            Place::ProgrammerText
        }
        _ if is_punct(before(2), '!') && is_ident(before(3), "macro_rules") => {
            Place::MacroDefinition
        }
        _ => Place::Open,
    }
}

/// The spans the lists name: the guarded files whole, the types' blocks, the free functions.
fn guarded_spans(index: &Index, guarded: &Guarded<'_>) -> (Vec<Span>, Vec<String>) {
    let mut spans = Vec::new();
    let mut unresolved = Vec::new();
    for suffix in guarded.files {
        let found: Vec<Span> = index
            .files()
            .iter()
            .filter(|file| {
                file.path()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
            })
            .map(FileRecord::span)
            .collect();
        if found.is_empty() {
            unresolved.push(format!("the guarded file `{suffix}`"));
        }
        spans.extend(found);
    }
    for name in guarded.types {
        let found: Vec<Span> = index
            .impls()
            .iter()
            .filter(|block| block.type_owner() == *name)
            .map(ImplRecord::whole)
            .collect();
        if found.is_empty() {
            unresolved.push(format!("the guarded type `{name}`"));
        }
        spans.extend(found);
    }
    for name in guarded.functions {
        let found: Vec<Span> = index
            .items()
            .iter()
            .filter(|item| item.type_owner().is_none() && item.name() == *name)
            .map(ItemRecord::whole)
            .collect();
        if found.is_empty() {
            unresolved.push(format!("the guarded function `{name}`"));
        }
        spans.extend(found);
    }
    let unresolved = unresolved
        .into_iter()
        .map(|entry| {
            format!(
                "{entry} names nothing in the index — a renamed or moved item would drop out of \
                 the guard silently; name it as it is now"
            )
        })
        .collect();
    (spans, unresolved)
}

fn where_is(index: &Index, span: Span) -> String {
    index
        .locate(span.start())
        .map_or_else(|| format!("{span:?}"), |location| location.to_string())
}

/// The decoded text of a string literal, or `None` for a literal that is not a string (a number,
/// a character).
fn string_value(record: &LiteralRecord) -> Option<String> {
    match record.value() {
        LiteralValue::Str(text) => Some(text.clone()),
        LiteralValue::ByteStr(bytes) | LiteralValue::CStr(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => None,
    }
}

/// **What a person reads of a literal**: its `&` mnemonics taken out (`&&` is one `&`), its ends
/// trimmed of white space.
fn as_read(text: &str) -> String {
    let mut read = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            if chars.peek() == Some(&'&') {
                chars.next();
                read.push('&');
            }
        } else {
            read.push(ch);
        }
    }
    read.trim().to_owned()
}

/// Whether a literal's text is a label's shape: what a person reads of it holds a letter.
fn holds_a_word(text: &str) -> bool {
    as_read(text).chars().any(char::is_alphabetic)
}

/// The name a `macro_rules!` definition defines: the identifier after `macro_rules!`.
fn defined_name(index: &Index, record: &MacroRecord) -> Option<String> {
    let stream = TokenStream::from_str(index.text(record.span())).ok()?;
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    match trees.as_slice() {
        [
            TokenTree::Ident(keyword),
            TokenTree::Punct(bang),
            TokenTree::Ident(name),
            ..,
        ] if keyword == "macro_rules" && bang.as_char() == '!' => Some(name.to_string()),
        _ => None,
    }
}

/// The package's `macro_rules!` whose definition holds a string literal with a word, by name.
fn worded_macros(index: &Index) -> BTreeSet<String> {
    index
        .macros()
        .iter()
        .filter(|record| record.kind() == MacroKind::Definition)
        .filter(|record| {
            index.literals().iter().any(|literal| {
                literal.span().within(record.tokens())
                    && string_value(literal).is_some_and(|text| holds_a_word(&text))
            })
        })
        .filter_map(|record| defined_name(index, record))
        .collect()
}

/// The candidates an unresolved edge carries that the walk follows: an ambiguous method's, an
/// other arm's, a dynamic call's.
fn candidates(edge: &Edge) -> Vec<ItemId> {
    match &edge.to {
        Target::Unresolved(
            Unresolved::Ambiguous { candidates, .. }
            | Unresolved::OtherArm { candidates }
            | Unresolved::Dynamic { candidates, .. },
        ) => candidates.clone(),
        _ => Vec::new(),
    }
}

/// What the walk adds: the spans of every item and initializer it reached, and its refusals.
fn reached_spans(
    index: &Index,
    package: &str,
    roots: &[Span],
    guarded: &Guarded<'_>,
) -> (Vec<Span>, Vec<String>) {
    let resolver = Resolver::new(&[(package, index)], Arms::product()).unwrap_or_else(|refusals| {
        panic!("the resolver reads {package}: {refusals:?}");
    });
    let graph = CallGraph::new(&resolver);
    let mut refusals = Vec::new();
    let starts: Vec<ItemId> = index
        .items()
        .iter()
        .filter(|item| item.kind().is_callable())
        .filter(|item| roots.iter().any(|root| item.whole().within(*root)))
        .filter_map(|item| resolver.id_of(package, item))
        .filter(|id| resolver.admitted(*id))
        .collect();
    let callables = resolver.callables(package);
    let mut boundary = BTreeSet::new();
    let mut boundary_rows = Vec::new();
    for (key, reason) in guarded.boundary {
        assert!(!reason.is_empty(), "a row says why: {key}");
        let found: Vec<ItemId> = callables
            .iter()
            .copied()
            .filter(|id| resolver.key(*id) == *key)
            .collect();
        if found.is_empty() {
            refusals.push(format!(
                "the BOUNDARY row `{key}` names nothing in the index — name the item as it is \
                 now, or take the row off"
            ));
        }
        boundary.extend(found.iter().copied());
        boundary_rows.push(found);
    }
    let reach = graph.walk_following(&starts, &boundary, &candidates);
    for ((key, _), found) in guarded.boundary.iter().zip(&boundary_rows) {
        if !found.is_empty() && !found.iter().any(|id| reach.stopped_at.contains(id)) {
            refusals.push(format!(
                "the BOUNDARY row `{key}` stops a walk that never reaches it — take it off"
            ));
        }
    }
    let mut spans: Vec<Span> = reach
        .reached
        .iter()
        .map(|id| resolver.record(*id).whole())
        .collect();
    spans.extend(
        reach
            .values
            .iter()
            .map(|id| resolver.value_record(*id).whole()),
    );

    let worded = worded_macros(index);
    let mut macros_used = vec![false; guarded.macros.len()];
    let mut unread_used = vec![false; guarded.unread.len()];
    let mut by_site = BTreeMap::new();
    for (edge, _) in &reach.unresolved {
        let Target::Unresolved(kind) = &edge.to else {
            continue;
        };
        let at = where_is(index, edge.site.span);
        let what = match kind {
            Unresolved::Macro { name } => {
                let name = name.rsplit("::").next().unwrap_or(name);
                if !worded.contains(name) {
                    continue;
                }
                if let Some(row) = guarded.macros.iter().position(|(row, _)| *row == name) {
                    macros_used[row] = true;
                    continue;
                }
                by_site.insert(
                    edge.site.span,
                    format!(
                        "{at}: `{name}!` is a `macro_rules!` of the package whose definition \
                         holds a word — its expansion lands in a native menu or dialog's code \
                         unread; pass the words in from the caller"
                    ),
                );
                continue;
            }
            Unresolved::Ambiguous { .. } | Unresolved::OtherArm { .. } => continue,
            Unresolved::Dynamic { candidates, .. } if !candidates.is_empty() => continue,
            Unresolved::Dynamic { what, .. } => {
                format!("a dynamic call with no candidate ({what})")
            }
            Unresolved::Path { written, why } => format!("an unplaced path `{written}` ({why})"),
            Unresolved::Generated { by } => format!("an item `{by}` generates"),
            Unresolved::Cutoff { what } => format!("a chain cut off ({what})"),
            Unresolved::Unparsed { why } => format!("an unparsed body ({why})"),
        };
        let caller = resolver.node_key(edge.from);
        let row = guarded
            .unread
            .iter()
            .position(|(key, name, _)| *key == caller && *name == edge.site.name);
        if let Some(row) = row {
            unread_used[row] = true;
            continue;
        }
        by_site.insert(
            edge.site.span,
            format!(
                "{at}: {what} in `{caller}`, calling `{}` — the guard cannot read what it runs; \
                 make the call one the graph follows, or review it into UNREAD",
                edge.site.name
            ),
        );
    }
    refusals.extend(by_site.into_values());
    for ((name, reason), used) in guarded.macros.iter().zip(macros_used) {
        assert!(!reason.is_empty(), "a row says why: {name}");
        if !used {
            refusals.push(format!(
                "the MACROS row for `{name}!` allows a call no guarded item makes — take it off"
            ));
        }
    }
    for ((key, name, reason), used) in guarded.unread.iter().zip(unread_used) {
        assert!(!reason.is_empty(), "a row says why: {key} {name}");
        if !used {
            refusals.push(format!(
                "the UNREAD row for `{name}` in `{key}` allows an edge the walk does not meet — \
                 take it off"
            ));
        }
    }
    (spans, refusals)
}

/// **The refusals** over one index: each literal with a word in an open place of the scope that
/// no row allows, each literal in a `macro_rules!` definition there, each edge the walk cannot
/// read, and each row nothing uses.
fn judge(index: &Index, package: &str, guarded: &Guarded<'_>) -> Vec<String> {
    let (mut spans, mut refusals) = guarded_spans(index, guarded);
    let (reached, unread) = reached_spans(index, package, &spans, guarded);
    spans.extend(reached);
    refusals.extend(unread);
    let mut used = vec![false; guarded.allowed.len()];
    for file in index.files() {
        if !spans.iter().any(|span| span.within(file.span())) {
            continue;
        }
        let path = file.path().to_string_lossy().replace('\\', "/");
        for found in literals_of(index.text(file.span()), file.span().start()) {
            if matches!(
                found.place,
                Place::Attribute | Place::ErrorText | Place::ProgrammerText | Place::Abi
            ) {
                continue;
            }
            let in_scope = spans
                .iter()
                .any(|guarded| guarded.start() <= found.start && found.start < guarded.end());
            if !in_scope {
                continue;
            }
            // Not recorded is not "not a string": a literal the index holds no record of is one
            // this guard cannot classify, and it is refused by its line.
            let Some(record) = index
                .literals()
                .iter()
                .find(|record| record.span().start() == found.start)
            else {
                refusals.push(format!(
                    "{}: a literal the index holds no record of — the guard cannot read its text",
                    index
                        .locate(found.start)
                        .map_or_else(|| format!("offset {}", found.start), |at| at.to_string())
                ));
                continue;
            };
            let span = record.span();
            let Some(text) = string_value(record) else {
                continue;
            };
            let product = Occurrence {
                span,
                certainty: Certainty::Resolved,
            }
            .in_the_product(index);
            if !product {
                continue;
            }
            match found.place {
                Place::Attribute | Place::ErrorText | Place::ProgrammerText | Place::Abi => {}
                Place::MacroDefinition => refusals.push(format!(
                    "{}: a literal in a `macro_rules!` definition ({text:?}) lands in an \
                     expansion this guard cannot see — pass the words in from the caller",
                    where_is(index, span)
                )),
                Place::Open => {
                    if !holds_a_word(&text) {
                        continue;
                    }
                    let row = guarded.allowed.iter().position(|(suffix, allowed, _)| {
                        path.ends_with(suffix) && *allowed == text
                    });
                    match row {
                        Some(row) => used[row] = true,
                        None => refusals.push(format!(
                            "{}: {text:?} (read {:?}) — a literal with a word in a native menu \
                             or dialog's product code, or in what it reaches; a person reads it, \
                             so the caller passes it from `bt-app`'s i18n table",
                            where_is(index, span),
                            as_read(&text)
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

/// How many refusals carry `needle`.
fn count(refusals: &[String], needle: &str) -> usize {
    refusals
        .iter()
        .filter(|refusal| refusal.contains(needle))
        .count()
}

/// RED (planted) — **in the guarded items: a literal with a word is refused whatever its first
/// character (`"Copy LaTeX"`, `"&Copy"`, `" copy"`), and so is a diagnostic-looking text in a
/// place the closed list does not name (`eprintln!`); a row nothing uses and a list entry that
/// names no item are refused.** The fixture's error texts, programmer texts (`.expect`,
/// `panic!`, `unreachable!`, `assert!`, `debug_assert_eq!`), wordless literals, attributes, ABI
/// name, allowlisted literal and test module pass, and an unguarded item nothing reaches is not
/// read.
#[test]
fn a_literal_with_a_word_in_a_guarded_item_is_named() {
    let index = fixture();
    let allowed = [
        ("lib.rs", "JSON", "the fixture's format name"),
        ("lib.rs", "Gone", "a row nothing uses"),
    ];
    let items = Guarded {
        files: &[],
        types: &["Menu"],
        functions: &["labels", "renamed_away"],
        allowed: &allowed,
        macros: &[],
        boundary: &[],
        unread: &[],
    };
    let refusals = judge(&index, "native-labels", &items);
    assert_eq!(
        count(
            &refusals,
            "\"Copy LaTeX\" (read \"Copy LaTeX\") — a literal with a word"
        ),
        1,
        "the product's label, and not the test module's: {refusals:#?}"
    );
    assert_eq!(
        count(&refusals, "\"&Copy\" (read \"Copy\")"),
        1,
        "a mnemonic's ampersand does not hide the word: {refusals:#?}"
    );
    assert_eq!(
        count(&refusals, "\" copy\" (read \"copy\")"),
        1,
        "a leading space or a small letter does not hide the word: {refusals:#?}"
    );
    assert_eq!(
        count(&refusals, "\"a diagnostic the list does not name\""),
        1,
        "the exempt places are a closed list: {refusals:#?}"
    );
    assert_eq!(count(&refusals, "the row for \"Gone\""), 1, "{refusals:#?}");
    assert_eq!(
        count(
            &refusals,
            "the guarded function `renamed_away` names nothing in the index"
        ),
        1,
        "a list entry that resolves to no item is refused, not skipped: {refusals:#?}"
    );
    assert_eq!(
        refusals.len(),
        6,
        "nothing else is refused — the error and programmer texts, the wordless literals, the \
         attribute, the allowed row, the unguarded function and the test module pass: \
         {refusals:#?}"
    );

    let whole = judge(
        &index,
        "native-labels",
        &Guarded {
            files: &["native_labels/lib.rs"],
            ..items
        },
    );
    assert_eq!(
        count(
            &whole,
            "a literal in a `macro_rules!` definition (\"Unseen\")"
        ),
        1,
        "{whole:#?}"
    );
    assert_eq!(
        count(&whole, "\"Folio\" (read \"Folio\")"),
        1,
        "a whole guarded file reads every product item: {whole:#?}"
    );
}

/// RED (planted) — **the guard reaches past the lists**: a `const` label defined outside the
/// scope and named from a guarded item, a call of a crate-local `macro_rules!` whose definition
/// holds a word, a helper on none of the lists that builds the label, a method reached through a
/// first-party trait object, and the edges the graph cannot follow (a call on an `Fn` parameter,
/// an unplaced path) are each refused by their line. Their honest twins pass: a wordless `const`,
/// a number, a macro on the `MACROS` list, a macro with no word, a helper whose literal has a row.
#[test]
fn what_a_guarded_item_reaches_is_read() {
    let index = fixture();
    let allowed = [
        ("lib.rs", "JSON", "the fixture's format name"),
        ("reach.rs", "&amp;", "an XML entity, not a word"),
    ];
    let guarded = Guarded {
        files: &[],
        types: &["Dialog"],
        functions: &["labels"],
        allowed: &allowed,
        macros: &[
            ("listed_label", "the fixture's listed macro"),
            ("gone_macro", "a row nothing uses"),
        ],
        boundary: &[
            (
                "native-labels crate::reach::Stack::push",
                "the fixture's `.push` is on a `Vec`",
            ),
            (
                "native-labels crate::reach::Gone::push",
                "a row naming nothing",
            ),
            (
                "native-labels crate::elsewhere",
                "a row on an item the walk never reaches",
            ),
        ],
        unread: &[
            (
                "native-labels crate::Dialog::through_a_reviewed_parameter",
                "label",
                "the fixture's reviewed call",
            ),
            (
                "native-labels crate::Dialog::gone",
                "label",
                "a row nothing uses",
            ),
        ],
    };
    let refusals = judge(&index, "native-labels", &guarded);
    let expect_one = |needle: &str, why: &str| {
        assert_eq!(count(&refusals, needle), 1, "{why}: {refusals:#?}");
    };
    expect_one(
        "\"Copy from a const\"",
        "(1) a const label outside the scope, named from a guarded item",
    );
    expect_one(
        "`outside_label!` is a `macro_rules!` of the package",
        "(2) a crate-local macro whose definition holds a word",
    );
    expect_one(
        "\"Copy from a helper\"",
        "(3) a helper on none of the lists that builds the label",
    );
    expect_one(
        "\"Copy through a trait object\"",
        "(3) a method reached through a first-party trait object",
    );
    expect_one(
        "a dynamic call with no candidate",
        "a call on an `Fn` parameter cannot be read",
    );
    expect_one(
        "an unplaced path `nowhere::label`",
        "an unplaced path cannot be read",
    );
    expect_one(
        "the MACROS row for `gone_macro!`",
        "a MACROS row nothing calls is refused",
    );
    expect_one(
        "the BOUNDARY row `native-labels crate::reach::Gone::push` names nothing",
        "a BOUNDARY row naming no item is refused",
    );
    expect_one(
        "the UNREAD row for `label` in `native-labels crate::Dialog::gone`",
        "an UNREAD row no edge uses is refused",
    );
    expect_one(
        "the BOUNDARY row `native-labels crate::elsewhere` stops a walk that never reaches it",
        "a BOUNDARY row the walk never stops at is refused",
    );
    assert_eq!(
        count(&refusals, "ambiguous candidate"),
        0,
        "the BOUNDARY row stops the walk at the ambiguous candidate: {refusals:#?}"
    );
    assert_eq!(
        refusals.len(),
        10,
        "nothing else is refused — the wordless const, the number, the listed macro, the macro \
         with no word, the entity's row and the unreached function pass: {refusals:#?}"
    );
}

/// RED (planted) — **an ambiguous method's first-party candidates are followed**: without a
/// `BOUNDARY` row, the candidate `reach::Stack::push` of `rows.push(1)` is read and its literal
/// refused; with the row, the walk stops there.
#[test]
fn an_ambiguous_candidate_is_followed_unless_a_row_stops_it() {
    let index = fixture();
    let guarded = Guarded {
        files: &[],
        types: &[],
        functions: &["ambiguous_root"],
        allowed: &[],
        macros: &[],
        boundary: &[],
        unread: &[],
    };
    let followed = judge(&index, "native-labels", &guarded);
    assert_eq!(
        count(&followed, "\"Copy from an ambiguous candidate\""),
        1,
        "{followed:#?}"
    );
    assert_eq!(followed.len(), 1, "{followed:#?}");
    let stopped = judge(
        &index,
        "native-labels",
        &Guarded {
            boundary: &[(
                "native-labels crate::reach::Stack::push",
                "the fixture's `.push` is on a `Vec`",
            )],
            ..guarded
        },
    );
    assert!(stopped.is_empty(), "{stopped:#?}");
}

/// GUARD — **`bt-platform`'s menus and dialogs, and everything they reach, carry no words of
/// their own**, but for the allowed rows.
///
/// MUTATION: write `windows::core::w!("Copy LaTeX")` back into `track_math_context_menu`, or
/// `ns_string!("Copy LaTeX")` into `macos_dialogs::track_formula_menu` — red, by its line.
#[test]
fn bt_platform_native_labels_come_from_the_caller() {
    let index = Index::of_package("bt-platform");
    let refusals = judge(
        index,
        "bt-platform",
        &Guarded {
            files: FILES,
            types: TYPES,
            functions: FUNCTIONS,
            allowed: ALLOWED,
            macros: MACROS,
            boundary: BOUNDARY,
            unread: UNREAD,
        },
    );
    assert!(
        refusals.is_empty(),
        "{} refusals:\n{}",
        refusals.len(),
        refusals.join("\n")
    );
}
