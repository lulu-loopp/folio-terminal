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
//!   label named from a guarded item, a method reached through a first-party trait object.
//! * **What runs with no call edge**, taken in to a fixed point: every first-party impl of a
//!   foreign trait (`Drop`, `Display`, `Iterator`, … — the graph's implicit calls) whose self type
//!   a body of the reach names; and the functions, `static`s and `const`s an **item-position
//!   macro** writes (`objc2::define_class!`, `thread_local!`: an invocation outside every body,
//!   its tokens read as items) — a function written in the scope, one whose `method(…)` selector
//!   a `sel!` of the reach names, one whose type a body of the reach names or a path names, a
//!   value a path names. Their bodies are read by syntax: a free call takes in every package
//!   function of its name (and of what a package `use` renames it from); `Owner::f` the package
//!   functions of that type or module, refused when the package names `Owner` and none is `f` (a
//!   variant is a constructor); a method call every package method of its name; a package
//!   `const` or `static` named is read, refused when its initializer calls or names a path; a
//!   call of a binding or of a value that is not a path is refused. An item-position macro in the
//!   scope whose tokens do not parse as items is refused by its line.
//! * **Edges the guard cannot follow**, by kind: an ambiguous method, a call on another arm and a
//!   dynamic call that carries candidates are followed into **every** candidate but the ones a
//!   [`BOUNDARY`] row drops (the caller, the name called, the candidate, why that call never runs
//!   it); a dynamic call with none, an unplaced path, a generated item and macro arguments the
//!   guard cannot read are refused by their line unless an [`UNREAD`] row reviews the edge (the
//!   caller, the name it calls, the kind — `dynamic`, `path`, `generated`, `macro-arguments`,
//!   `item-macro` — and why what runs there writes no label); a cut-off chain and an unparsed
//!   body are refused and no row excuses them. A foreign callee is the boundary, not a failure.
//!   A selector (`sel!`) no function of the package declares is a foreign class's and passes.
//! * **Macros**: a call of a `macro_rules!` of the package is refused by its line unless the
//!   macro is a row of [`MACROS`] (its expansion is not read); a literal inside a `macro_rules!`
//!   definition in the scope is refused by its line. A foreign macro's arguments are read where
//!   the call graph parses them (an expression list, a `matches!` body, a repeat body); where it
//!   does not, `msg_send!`'s `receiver, part: argument, …` list is read as a body, and any other
//!   arguments are refused by their line when they hold a call, a `::` path, a macro or a name of
//!   the package.
//! * **Read**: every string literal (plain, raw, byte and C strings) in the scope, wherever it is
//!   written — an expression, a `const`, a macro's tokens, each piece of `concat!`. **A character
//!   literal is skipped**, a stated limit: one character is no word.
//! * **A label**: a literal whose decoded text, its `&` mnemonics taken out (`&&` stays one `&`)
//!   and its ends trimmed of white space, holds a letter (`char::is_alphabetic`). A literal with
//!   none (`""`, `";"`, `"*.*"`) carries no word.
//! * **Not a label, by its place** — the closed list; any other diagnostic-looking text is a row:
//!   - a literal inside an attribute (`#[…]`, a doc comment included);
//!   - an ABI's name right after `extern`;
//!   - an error text: inside the argument list of `Err(…)`, `.map_err(…)` or `.ok_or_else(…)`;
//!   - a programmer's text: inside the argument list of `.expect(…)` or of one of
//!     [`PROGRAMMER_MACROS`] (`panic!`, `unreachable!`, `assert!`, `assert_eq!`, `assert_ne!`,
//!     `debug_assert!`, `debug_assert_eq!`, `debug_assert_ne!`).
//!
//!   The place is the parenthesis (or macro) group the literal sits in, at any depth.
//! * **Allowed by row, one use each**: [`ALLOWED`] — a literal that must stay, by the file it is
//!   written in (a path suffix) and its text **as written** (a row for `Copy` does not allow
//!   `&Copy`), with its reason; a text written twice needs two rows. A [`MACROS`] row allows the
//!   calls of its macro; a [`BOUNDARY`] row drops one candidate from one call; an [`UNREAD`] row
//!   excuses one edge. A row no use takes is refused.
//! * **The lists are history-checked** (`scripts/ci/check-native-labels.ps1`, against the merge
//!   base): the four row lists only shrink, [`FILES`], [`TYPES`] and [`FUNCTIONS`] only grow,
//!   and [`PROGRAMMER_MACROS`] only shrinks; a row added, a scope entry taken off or an exempt
//!   macro added needs a dated `// widened` line, counting them, in a change to this grammar.
//! * **Unreadable, refused by its line**: a literal the index holds no record of.
//! * **The lists are checked**: an entry of [`FILES`], [`TYPES`] or [`FUNCTIONS`] that names
//!   nothing in the index is refused, so a renamed item cannot drop out of the guard silently.
//!
//! # What it does not see (limits)
//!
//! A literal `bt-app` passes in (the caller's table is the subject of `bt-app`'s own runtime
//! tests); a character literal; the expansion of a `macro_rules!` on [`MACROS`]; what a foreign
//! function or a foreign macro's expansion does with the words; an item a derive or an attribute
//! macro generates.
//!
//! The fixture under `tests/fixtures/native_labels/` plants each case; the last test holds
//! `bt-platform` itself.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use bt_source::call_graph::{Arms, CallGraph, Edge, ItemId, Node, Resolver, Target, Unresolved};
use bt_source::{
    Certainty, DiskScope, Index, ItemKind, ItemRecord, LiteralRecord, LiteralValue, MacroKind,
    MacroRecord, Occurrence, Span, TargetId, TargetKind, TargetRoot, Universe, Vendor, report,
};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

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

// widened 2026-10-25 by T-NATIVE-LABELS-GUARD-REACH (+104 rows): every literal with a word is a
// label and the scope is what the call graph, the implicit impls and the item-position macros
// reach (43 ALLOWED, 39 BOUNDARY, 14 UNREAD, 8 PROGRAMMER_MACROS)
/// **The literals that must stay**, by the file they are written in (a path suffix) and their
/// text as written, each with its reason: one row per literal, so a text written twice has two.
/// The list only shrinks (`scripts/ci/check-native-labels.ps1`).
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
        "the formula menu",
        "the door's name in its error texts (`window_for`, `nothing_to_ask`) (its second literal)",
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
        "folder",
        "a noun of the choosers' error texts (`the chosen {noun}...`), which go to the diagnostics (its second literal)",
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

/// **The `macro_rules!` of `bt-platform` the guarded scope may call**, each with why its
/// expansion writes no label: the expansion is not read. The list only shrinks.
const MACROS: &[(&str, &str)] = &[];

/// **The candidates a call never runs**: the caller (by [`Resolver::node_key`], or an item-macro
/// function's key), the name called, the candidate dropped (by [`Resolver::key`]), and why — an
/// ambiguous method call's first-party candidate that the receiver's type rules out (a `.push`
/// on a `Vec`). A row drops the candidate from one call; a row that drops nothing is refused. The
/// list only shrinks.
const BOUNDARY: &[(&str, &str, &str, &str)] = &[
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::pop",
        "take",
        "bt-platform crate::elevated_protocol::codec::Cursor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::pop",
        "take",
        "bt-platform crate::https_download::DownloadMonitor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::pop",
        "take",
        "bt-platform crate::web_environment::ControllerSlots::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::pop",
        "take",
        "bt-platform crate::web_environment::Pending::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::request",
        "begin",
        "bt-platform crate::https_download::DownloadMonitor::begin",
        "`ivars.state.begin()` is on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::request",
        "begin",
        "bt-platform crate::macos_clipboard_payload::<MacClipboard as ClipboardPort>::begin",
        "`ivars.state.begin()` is on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::request",
        "begin",
        "bt-platform crate::windows_clipboard::<WindowsClipboard as ClipboardPort>::begin",
        "`ivars.state.begin()` is on the formula menu's `Deferred`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::take_result",
        "take",
        "bt-platform crate::elevated_protocol::codec::Cursor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::take_result",
        "take",
        "bt-platform crate::https_download::DownloadMonitor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::take_result",
        "take",
        "bt-platform crate::web_environment::ControllerSlots::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::MathContextMenu::take_result",
        "take",
        "bt-platform crate::web_environment::Pending::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_dialogs::read_choice",
        "path",
        "bt-platform crate::deferred_removal::Item::path",
        "`url.path()` is on an `NSURL` (the chooser's answer)",
    ),
    (
        "bt-platform crate::macos_dialogs::read_choice",
        "path",
        "bt-platform crate::exclusive_create::Directory::path",
        "`url.path()` is on an `NSURL` (the chooser's answer)",
    ),
    (
        "bt-platform crate::macos_dialogs::start_at",
        "is_empty",
        "bt-platform crate::GroundBand::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::macos_dialogs::start_at",
        "is_empty",
        "bt-platform crate::webview::RehostCompensation::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::chosen",
        "get",
        "bt-platform crate::logon_hook::os::<CurrentUser as Registry>::get",
        "`.get(tag)` is on a `Vec` (the bar's choices, the Dock's rows)",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::dock_chosen",
        "get",
        "bt-platform crate::logon_hook::os::<CurrentUser as Registry>::get",
        "`.get(tag)` is on a `Vec` (the bar's choices, the Dock's rows)",
    ),
    (
        "bt-platform crate::macos_menu::dock_menu",
        "is_empty",
        "bt-platform crate::GroundBand::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::macos_menu::dock_menu",
        "is_empty",
        "bt-platform crate::webview::RehostCompensation::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "take",
        "bt-platform crate::elevated_protocol::codec::Cursor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "take",
        "bt-platform crate::https_download::DownloadMonitor::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "take",
        "bt-platform crate::web_environment::ControllerSlots::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "take",
        "bt-platform crate::web_environment::Pending::take",
        "`.take()` is on the formula menu's `Deferred` or on an `Option`",
    ),
    (
        "bt-platform crate::windows_impl::FolderPicker::request",
        "push",
        "bt-platform crate::elevated_protocol::codec::Decoder::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::FolderPicker::request",
        "push",
        "bt-platform crate::webview::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::FolderPicker::request",
        "push",
        "bt-platform crate::webview::macos::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::ImagePicker::request",
        "push",
        "bt-platform crate::elevated_protocol::codec::Decoder::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::ImagePicker::request",
        "push",
        "bt-platform crate::webview::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::ImagePicker::request",
        "push",
        "bt-platform crate::webview::macos::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::SaveFilePicker::request",
        "push",
        "bt-platform crate::elevated_protocol::codec::Decoder::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::SaveFilePicker::request",
        "push",
        "bt-platform crate::webview::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::SaveFilePicker::request",
        "push",
        "bt-platform crate::webview::macos::Shared::push",
        "`units.push(0)` is on a `Vec<u16>` (the pickers' start folder)",
    ),
    (
        "bt-platform crate::windows_impl::message_box",
        "chain",
        "bt-platform crate::trust::arm::Engine::chain",
        "`.encode_utf16().chain(once(0))` is on an iterator (the wide strings)",
    ),
    (
        "bt-platform crate::windows_impl::message_box",
        "chain",
        "bt-platform crate::trust::arm::Engine::chain",
        "`.encode_utf16().chain(once(0))` is on an iterator (the wide strings)",
    ),
    (
        "bt-platform crate::windows_impl::save_extension",
        "is_empty",
        "bt-platform crate::GroundBand::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::windows_impl::save_extension",
        "is_empty",
        "bt-platform crate::webview::RehostCompensation::is_empty",
        "`.is_empty()` is on a `str` or a `Vec`",
    ),
    (
        "bt-platform crate::windows_impl::show_save_picker",
        "code",
        "bt-platform crate::macos_notify::Permission::code",
        "`error.code()` is on a `windows::core::Error` (the pickers' cancel test)",
    ),
    (
        "bt-platform crate::windows_impl::show_shell_picker",
        "code",
        "bt-platform crate::macos_notify::Permission::code",
        "`error.code()` is on a `windows::core::Error` (the pickers' cancel test)",
    ),
    (
        "bt-platform crate::windows_impl::wide_null",
        "chain",
        "bt-platform crate::trust::arm::Engine::chain",
        "`.encode_utf16().chain(once(0))` is on an iterator (the wide strings)",
    ),
];

/// **The edges the guard cannot read, reviewed**: the caller (by [`Resolver::node_key`], or an
/// item-macro function's key), the name it calls, the kind (`dynamic`, `path`, `generated`,
/// `macro-arguments`, `item-macro`), and why what runs there writes no label. A row excuses one
/// edge; a row no edge uses is refused. The list only shrinks.
const UNREAD: &[(&str, &str, &str, &str)] = &[
    (
        "bt-platform crate::explorer_command::<Command_Impl as IExplorerCommand_Impl>::Invoke",
        "(value)",
        "dynamic",
        "the verb's `invoke` is the handler `bt-app` registered; it opens a window and writes no label here",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "alloc",
        "path",
        "the class's `alloc`, which objc2's `define_class!` generates: no body of this package runs",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "super",
        "path",
        "`msg_send![super(...), init]` to `NSObject` (objc2): AppKit's initializer, no body of this package",
    ),
    (
        "bt-platform crate::macos_dialogs::DeferredFormulaMenu::new",
        "init",
        "path",
        "`msg_send![super(...), init]` to `NSObject` (objc2): AppKit's initializer, no body of this package",
    ),
    (
        "bt-platform crate::macos_dialogs::message_box",
        "NSApp",
        "path",
        "AppKit's shared application, a foreign static",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::chosen",
        "(value)",
        "dynamic",
        "the bar's `send` is the sender `bt-app` handed to `install`; it carries a choice, not a label",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::dock_chosen",
        "(value)",
        "dynamic",
        "the bar's `send` is the sender `bt-app` handed to `install`; it carries a choice, not a label",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "alloc",
        "path",
        "the class's `alloc`, which objc2's `define_class!` generates: no body of this package runs",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "super",
        "path",
        "`msg_send![super(...), init]` to `NSObject` (objc2): AppKit's initializer, no body of this package",
    ),
    (
        "bt-platform crate::macos_menu::MenuTarget::new",
        "init",
        "path",
        "`msg_send![super(...), init]` to `NSObject` (objc2): AppKit's initializer, no body of this package",
    ),
    (
        "bt-platform crate::macos_menu::build",
        "clone",
        "generated",
        "`#[derive(Clone)]`, the language's derive: it writes no literal",
    ),
    (
        "bt-platform crate::macos_menu::refresh",
        "clone",
        "generated",
        "`#[derive(Clone)]`, the language's derive: it writes no literal",
    ),
    (
        "bt-platform crate::macos_menu::the_chain_declined",
        "(value)",
        "dynamic",
        "the bar's `send` is the sender `bt-app` handed to `install`; it carries a choice, not a label",
    ),
    (
        "bt-platform crate::windows_impl::message_box",
        "MessageBoxTimeoutW",
        "path",
        "user32's message box, declared in the function's own `extern` block: it shows the caller's title and text",
    ),
];

/// What the guard is asked to read, and the rows it allows.
#[derive(Clone, Copy)]
struct Guarded<'a> {
    files: &'a [&'a str],
    types: &'a [&'a str],
    functions: &'a [&'a str],
    allowed: &'a [(&'a str, &'a str, &'a str)],
    macros: &'a [(&'a str, &'a str)],
    boundary: &'a [(&'a str, &'a str, &'a str, &'a str)],
    unread: &'a [(&'a str, &'a str, &'a str, &'a str)],
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

/// The macros whose arguments are a programmer's text, not a label: the closed list. It only
/// shrinks (`scripts/ci/check-native-labels.ps1`).
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

/// **A stretch of the index's text** by its absolute offsets, start and end: a record's span, or
/// a stretch read from a macro's tokens, which the index holds no record of.
type Range = (usize, usize);

const fn range(span: Span) -> Range {
    (span.start(), span.end())
}

const fn inside(inner: Range, outer: Range) -> bool {
    inner.0 >= outer.0 && inner.1 <= outer.1
}

const fn holds(outer: Range, at: usize) -> bool {
    outer.0 <= at && at < outer.1
}

/// The text of a range: the stretch of the one file it is written in.
fn text_of(index: &Index, (start, end): Range) -> &str {
    let file = index
        .file_at(start)
        .expect("a range of the index is a stretch of one of its files");
    let base = file.span().start();
    &index.text(file.span())[start - base..end - base]
}

/// The spans the lists name: the guarded files whole, the types' blocks, the free functions.
fn guarded_spans(index: &Index, guarded: &Guarded<'_>) -> (Vec<Range>, Vec<String>) {
    let mut spans = Vec::new();
    let mut unresolved = Vec::new();
    for suffix in guarded.files {
        let found: Vec<Range> = index
            .files()
            .iter()
            .filter(|file| slashed(file.path()).ends_with(suffix))
            .map(|file| range(file.span()))
            .collect();
        if found.is_empty() {
            unresolved.push(format!("the guarded file `{suffix}`"));
        }
        spans.extend(found);
    }
    for name in guarded.types {
        let found: Vec<Range> = index
            .impls()
            .iter()
            .filter(|block| block.type_owner() == *name)
            .map(|block| range(block.whole()))
            .collect();
        if found.is_empty() {
            unresolved.push(format!("the guarded type `{name}`"));
        }
        spans.extend(found);
    }
    for name in guarded.functions {
        let found: Vec<Range> = index
            .items()
            .iter()
            .filter(|item| item.type_owner().is_none() && item.name() == *name)
            .map(|item| range(item.whole()))
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

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn where_is(index: &Index, at: usize) -> String {
    index
        .locate(at)
        .map_or_else(|| format!("offset {at}"), |location| location.to_string())
}

/// The decoded text of a string literal (plain, raw, byte or C), or `None` for a literal that is
/// not a string. **A character literal is skipped**, a stated limit: one character is no word,
/// and a label built of characters is a `String` built at run time from no string literal.
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

/// Every identifier written in `stream`, at any depth.
fn identifiers(stream: TokenStream, into: &mut BTreeSet<String>) {
    for tree in stream {
        match tree {
            TokenTree::Ident(ident) => {
                into.insert(ident.to_string());
            }
            TokenTree::Group(group) => identifiers(group.stream(), into),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// **Whether tokens hold a call or a path** — what an expansion could run or name: an identifier
/// followed by a parenthesis or by `!`, a `::`, or an identifier `names` holds (the package's
/// callables, values and types).
fn holds_a_call_or_path(stream: TokenStream, names: &BTreeSet<String>) -> bool {
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    trees.iter().enumerate().any(|(at, tree)| {
        let next = trees.get(at + 1);
        match tree {
            TokenTree::Ident(ident) => {
                matches!(next, Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
                    || matches!(next, Some(TokenTree::Punct(punct)) if punct.as_char() == '!')
                    || names.contains(&ident.to_string())
            }
            TokenTree::Punct(punct) => {
                punct.as_char() == ':'
                    && matches!(next, Some(TokenTree::Punct(second)) if second.as_char() == ':')
            }
            TokenTree::Group(group) => holds_a_call_or_path(group.stream(), names),
            TokenTree::Literal(_) => false,
        }
    })
}

/// **The kinds of what the guard cannot read**, as an [`UNREAD`] row spells them.
const fn kind_of(unresolved: &Unresolved) -> &'static str {
    match unresolved {
        Unresolved::Dynamic { .. } => "dynamic",
        Unresolved::Ambiguous { .. } => "ambiguous",
        Unresolved::Macro { .. } => "macro-arguments",
        Unresolved::OtherArm { .. } => "other-arm",
        Unresolved::Generated { .. } => "generated",
        Unresolved::Path { .. } => "path",
        Unresolved::Cutoff { .. } => "cutoff",
        Unresolved::Unparsed { .. } => "unparsed",
    }
}

/// The kinds no [`UNREAD`] row may excuse (the call graph's table says so of both).
const UNEXCUSABLE: &[&str] = &["cutoff", "unparsed"];

// ── item-position macros ───────────────────────────────────────────────────

/// **A function written in the tokens of an item-position macro** (`objc2::define_class!`), of
/// which the index holds no record and the call graph no edge: read here, by syntax.
struct MacroFn {
    /// Named as [`Resolver::key`] names an item: `package module::Owner::name`.
    key: String,
    /// The type of its `impl` block; `None` for a free function.
    owner: Option<String>,
    name: String,
    /// The Objective-C selector its `#[unsafe(method(…))]` attribute gives it, if any.
    selector: Option<String>,
    whole: Range,
    /// Where the tokens it was read from start: its syntax's offsets are relative to it.
    base: usize,
    /// The module path it is written in.
    module: String,
    item: syn::ImplItemFn,
    in_product: bool,
}

/// **A `static` or `const` written in an item-position macro's tokens** (`thread_local!`).
struct MacroValue {
    key: String,
    name: String,
    whole: Range,
    base: usize,
    module: String,
    expr: syn::Expr,
    in_product: bool,
}

/// What the package's item-position macros write.
#[derive(Default)]
struct MacroItems {
    fns: Vec<MacroFn>,
    values: Vec<MacroValue>,
    /// The types they declare (`struct X;` in `define_class!`).
    types: BTreeSet<String>,
    /// An item-position macro whose tokens do not parse as items: where it starts, its path, the
    /// module it is written in, and whether a product build holds it.
    unparsed: Vec<(usize, String, String, bool)>,
}

/// The module path of the innermost module `span` is written in.
fn innermost_module(index: &Index, span: Span) -> &str {
    index
        .modules()
        .iter()
        .filter(|module| span.within(module.span()))
        .min_by_key(|module| module.span().len())
        .and_then(|module| module.module_paths().first())
        .map_or("crate", String::as_str)
}

/// Every span merged into disjoint runs, sorted.
fn merged(mut spans: Vec<Range>) -> Vec<Range> {
    spans.sort_unstable();
    let mut runs: Vec<Range> = Vec::new();
    for (start, end) in spans {
        match runs.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => runs.push((start, end)),
        }
    }
    runs
}

/// Whether `span` lies inside one of the disjoint sorted `runs`.
fn inside_runs(runs: &[Range], span: Span) -> bool {
    let after = runs.partition_point(|run| run.0 <= span.start());
    after > 0 && span.end() <= runs[after - 1].1
}

/// The range a syntax node read from tokens starting at `base` covers.
fn range_in(node: &impl Spanned, base: usize) -> Range {
    let bytes = node.span().byte_range();
    (base + bytes.start, base + bytes.end)
}

/// The selector of a `#[unsafe(method(sel))]` or `#[method(sel)]` attribute.
fn selector_of(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let syn::Meta::List(list) = &attr.meta else {
            return None;
        };
        let head = list.path.segments.last()?.ident.to_string();
        let text = format!("{head}({})", list.tokens).replace(' ', "");
        let rest = &text[text.find("method(")? + "method(".len()..];
        Some(rest[..rest.find(')')?].to_owned())
    })
}

/// **Every item-position macro of the package** — an invocation written outside every body,
/// every `const`/`static` initializer and every other macro — its tokens read as items: its
/// functions (free, and of `impl` blocks), its `static`s and `const`s, the types it declares, and
/// the invocations whose tokens do not parse as items.
fn macro_items(index: &Index, package: &str) -> MacroItems {
    let mut covered: Vec<Range> = index
        .items()
        .iter()
        .filter_map(|record| record.body().map(range))
        .collect();
    covered.extend(index.values().iter().map(|value| range(value.whole())));
    covered.extend(index.macros().iter().map(|record| range(record.tokens())));
    let covered = merged(covered);
    let mut items = MacroItems::default();
    let mut seen = BTreeSet::new();
    for record in index.macros() {
        if record.kind() != MacroKind::Invocation
            || inside_runs(&covered, record.span())
            || !seen.insert(record.span())
        {
            continue;
        }
        let in_product = Occurrence {
            span: record.span(),
            certainty: Certainty::Resolved,
        }
        .in_the_product(index);
        let module = innermost_module(index, record.span()).to_owned();
        let base = record.tokens().start();
        let Ok(file) = syn::parse_str::<syn::File>(index.text(record.tokens())) else {
            items.unparsed.push((
                record.span().start(),
                record.path().to_owned(),
                module,
                in_product,
            ));
            continue;
        };
        for item in file.items {
            let mut function = |owner: Option<String>, item: syn::ImplItemFn| {
                let name = item.sig.ident.to_string();
                let label = owner
                    .as_ref()
                    .map_or_else(|| name.clone(), |owner| format!("{owner}::{name}"));
                items.fns.push(MacroFn {
                    key: format!("{package} {module}::{label}"),
                    selector: selector_of(&item.attrs),
                    whole: range_in(&item, base),
                    owner,
                    name,
                    base,
                    module: module.clone(),
                    item,
                    in_product,
                });
            };
            match item {
                syn::Item::Struct(declared) => {
                    items.types.insert(declared.ident.to_string());
                }
                syn::Item::Impl(block) => {
                    let owner = match &*block.self_ty {
                        syn::Type::Path(path) => path
                            .path
                            .segments
                            .last()
                            .map(|segment| segment.ident.to_string()),
                        _ => None,
                    };
                    for member in block.items {
                        if let syn::ImplItem::Fn(member) = member {
                            function(owner.clone(), member);
                        }
                    }
                }
                syn::Item::Fn(free) => function(
                    None,
                    syn::ImplItemFn {
                        attrs: free.attrs,
                        vis: free.vis,
                        defaultness: None,
                        sig: free.sig,
                        block: *free.block,
                    },
                ),
                syn::Item::Static(value) => items.values.push(MacroValue {
                    key: format!("{package} {module}::{}", value.ident),
                    name: value.ident.to_string(),
                    whole: range_in(&value, base),
                    base,
                    module: module.clone(),
                    expr: *value.expr,
                    in_product,
                }),
                syn::Item::Const(value) => items.values.push(MacroValue {
                    key: format!("{package} {module}::{}", value.ident),
                    name: value.ident.to_string(),
                    whole: range_in(&value, base),
                    base,
                    module: module.clone(),
                    expr: *value.expr,
                    in_product,
                }),
                _ => {}
            }
        }
    }
    items
}

/// **What a body the call graph does not walk calls and names**, by syntax: a function written in
/// an item-position macro, or a macro's arguments the graph could not parse.
enum Read {
    /// `name(…)`.
    Free { name: String },
    /// `Owner::name(…)` or `module::name(…)`: the segment before the last, and the last.
    Assoc {
        at: usize,
        owner: String,
        name: String,
    },
    /// `.name(…)`.
    Method { name: String },
    /// A path in value position, by its segments.
    Value { path: Vec<String> },
    /// A call of a binding, or of something that is not a path.
    Dynamic { at: usize, what: String },
    /// A macro: where it is written, its path, its tokens.
    Macro {
        at: usize,
        path: String,
        tokens: TokenStream,
    },
}

/// The bindings a body declares: its parameters' and every pattern's names.
#[derive(Default)]
struct Locals(BTreeSet<String>);

impl<'ast> Visit<'ast> for Locals {
    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        self.0.insert(pat.ident.to_string());
        visit::visit_pat_ident(self, pat);
    }
}

/// **The reader of a body by syntax**: what it calls and names, `Self` read as its type.
struct BodyReader<'a> {
    locals: &'a BTreeSet<String>,
    owner: Option<&'a str>,
    base: usize,
    reads: Vec<Read>,
}

impl BodyReader<'_> {
    fn segments(&self, path: &syn::Path) -> Vec<String> {
        path.segments
            .iter()
            .map(|segment| match (segment.ident.to_string(), self.owner) {
                (name, Some(owner)) if name == "Self" => owner.to_owned(),
                (name, _) => name,
            })
            .collect()
    }
}

impl<'ast> Visit<'ast> for BodyReader<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let at = range_in(&*call.func, self.base).0;
        match &*call.func {
            syn::Expr::Path(path) if path.qself.is_none() => {
                match self.segments(&path.path).as_slice() {
                    [name] if self.locals.contains(name) => self.reads.push(Read::Dynamic {
                        at,
                        what: format!("`{name}`, a binding, called"),
                    }),
                    [name] => self.reads.push(Read::Free { name: name.clone() }),
                    [.., owner, name] => self.reads.push(Read::Assoc {
                        at,
                        owner: owner.clone(),
                        name: name.clone(),
                    }),
                    [] => {}
                }
            }
            other => {
                self.reads.push(Read::Dynamic {
                    at,
                    what: "a value that is not a path, called".to_owned(),
                });
                self.visit_expr(other);
            }
        }
        for argument in &call.args {
            self.visit_expr(argument);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.reads.push(Read::Method {
            name: call.method.to_string(),
        });
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let segments = self.segments(&path.path);
        if !matches!(segments.as_slice(), [name] if self.locals.contains(name)) {
            self.reads.push(Read::Value { path: segments });
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let path = mac
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        self.reads.push(Read::Macro {
            at: range_in(&mac.path, self.base).0,
            path,
            tokens: mac.tokens.clone(),
        });
    }
}

/// `matches!`'s body: an expression, a pattern, a guard.
fn matches_body(input: ParseStream<'_>) -> syn::Result<Vec<syn::Expr>> {
    let mut exprs = vec![input.parse::<syn::Expr>()?];
    input.parse::<syn::Token![,]>()?;
    syn::Pat::parse_multi_with_leading_vert(input)?;
    if input.peek(syn::Token![if]) {
        input.parse::<syn::Token![if]>()?;
        exprs.push(input.parse::<syn::Expr>()?);
    }
    if input.peek(syn::Token![,]) {
        input.parse::<syn::Token![,]>()?;
    }
    Ok(exprs)
}

/// `[value; length]`'s body.
fn repeat_body(input: ParseStream<'_>) -> syn::Result<Vec<syn::Expr>> {
    let value = input.parse::<syn::Expr>()?;
    input.parse::<syn::Token![;]>()?;
    Ok(vec![value, input.parse::<syn::Expr>()?])
}

/// `msg_send![receiver, selector]` or `msg_send![receiver, part: argument, …]`: the receiver and
/// every argument.
fn message_body(input: ParseStream<'_>) -> syn::Result<Vec<syn::Expr>> {
    let mut exprs = vec![input.parse::<syn::Expr>()?];
    input.parse::<syn::Token![,]>()?;
    while !input.is_empty() {
        input.call(syn::Ident::parse_any)?;
        if input.peek(syn::Token![:]) {
            input.parse::<syn::Token![:]>()?;
            exprs.push(input.parse::<syn::Expr>()?);
        }
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
    }
    Ok(exprs)
}

/// **A macro's arguments as the call graph reads them** (`call_graph`'s `mac`): `cfg!` reads
/// nothing; `matches!` its body; anything else a comma list of expressions, else a
/// `matches!`-shaped body, else a repeat body; `None` when none of them parse.
fn graph_arguments(name: &str, tokens: &TokenStream) -> Option<Vec<syn::Expr>> {
    if name == "cfg" {
        return Some(Vec::new());
    }
    if name == "matches"
        && let Ok(exprs) = matches_body.parse2(tokens.clone())
    {
        return Some(exprs);
    }
    if let Ok(list) =
        Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated.parse2(tokens.clone())
    {
        return Some(list.into_iter().collect());
    }
    matches_body
        .parse2(tokens.clone())
        .or_else(|_| repeat_body.parse2(tokens.clone()))
        .ok()
}

/// The bindings of an item, read from its text; `None` when it does not parse as a function.
fn locals_of_item(text: &str) -> Option<BTreeSet<String>> {
    let mut locals = Locals::default();
    if let Ok(item) = syn::parse_str::<syn::ImplItemFn>(text) {
        locals.visit_impl_item_fn(&item);
    } else if let Ok(item) = syn::parse_str::<syn::ItemFn>(text) {
        locals.visit_item_fn(&item);
    } else if let Ok(item) = syn::parse_str::<syn::TraitItemFn>(text) {
        locals.visit_trait_item_fn(&item);
    } else {
        return None;
    }
    Some(locals.0)
}

/// **A multiset of rows**: each use takes one untaken row; a row no use takes is refused.
struct Ledger {
    taken: Vec<bool>,
}

impl Ledger {
    fn new(rows: usize) -> Self {
        Self {
            taken: vec![false; rows],
        }
    }

    /// Takes the first untaken row `admits` admits; `false` when there is none.
    fn take(&mut self, admits: impl Fn(usize) -> bool) -> bool {
        let found = (0..self.taken.len()).find(|row| !self.taken[*row] && admits(*row));
        if let Some(row) = found {
            self.taken[row] = true;
        }
        found.is_some()
    }
}

/// What the reach holds: the call graph's roots, the item-macro functions and values.
#[derive(Default)]
struct Held {
    items: BTreeSet<ItemId>,
    fns: BTreeSet<usize>,
    values: BTreeSet<usize>,
}

impl Held {
    fn holds(&self, other: &Self) -> bool {
        other.items.is_subset(&self.items)
            && other.fns.is_subset(&self.fns)
            && other.values.is_subset(&self.values)
    }

    fn extend(&mut self, other: Self) {
        self.items.extend(other.items);
        self.fns.extend(other.fns);
        self.values.extend(other.values);
    }
}

/// What one walk from the held roots found.
struct Pass {
    spans: Vec<Range>,
    /// By offset, so one site is refused once.
    refusals: BTreeMap<usize, String>,
    /// The sites judged, by offset, caller, name and kind: one site is one use of a row.
    met: BTreeSet<(usize, String, String, String)>,
    adds: Held,
    boundary: Ledger,
    unread: Ledger,
    macros: Ledger,
}

/// The package, its graph and its names: what places what the graph does not.
struct Names<'r, 'i> {
    index: &'i Index,
    resolver: &'r Resolver<'i>,
    callables: Vec<ItemId>,
    items: MacroItems,
    /// The package's `macro_rules!`, by name.
    macro_rules: BTreeSet<String>,
    /// Every name the package gives a callable, a value or a type.
    package_names: BTreeSet<String>,
    /// The last segment of every module path of the package.
    modules: BTreeSet<String>,
}

impl<'r, 'i> Names<'r, 'i> {
    fn new(package: &str, index: &'i Index, resolver: &'r Resolver<'i>) -> Self {
        let items = macro_items(index, package);
        let mut package_names: BTreeSet<String> = index
            .items()
            .iter()
            .filter(|item| item.kind().is_callable() || item.kind().is_type())
            .map(|item| item.name().to_owned())
            .collect();
        package_names.extend(index.values().iter().map(|value| value.name().to_owned()));
        package_names.extend(items.fns.iter().map(|function| function.name.clone()));
        package_names.extend(items.values.iter().map(|value| value.name.clone()));
        package_names.extend(items.types.iter().cloned());
        let modules = index
            .modules()
            .iter()
            .flat_map(|module| module.module_paths().iter())
            .filter_map(|path| path.rsplit("::").next().map(str::to_owned))
            .collect();
        Self {
            index,
            resolver,
            callables: resolver.callables(package),
            macro_rules: index
                .macros()
                .iter()
                .filter(|record| record.kind() == MacroKind::Definition)
                .filter_map(|record| defined_name(index, record))
                .collect(),
            items,
            package_names,
            modules,
        }
    }

    /// The package's callables named `name` that `keep` admits.
    fn callables_named(&self, name: &str, keep: impl Fn(&ItemRecord) -> bool) -> Vec<ItemId> {
        self.callables
            .iter()
            .copied()
            .filter(|id| {
                let record = self.resolver.record(*id);
                record.name() == name && self.resolver.admitted(*id) && keep(record)
            })
            .collect()
    }

    /// The names a `use` of the package's own (`crate::`, `self::`, `super::`) brings in as
    /// `name`: the last segment of each path it names.
    fn imported_as(&self, name: &str) -> Vec<String> {
        self.index
            .uses()
            .iter()
            .flat_map(|record| record.names().iter())
            .filter(|(local, path)| {
                local == name
                    && path
                        .first()
                        .is_some_and(|first| matches!(first.as_str(), "crate" | "self" | "super"))
            })
            .filter_map(|(_, path)| path.last().cloned())
            .collect()
    }

    /// Whether the package names `owner` as a type or a module.
    fn names_a_type_or_module(&self, owner: &str) -> bool {
        self.items.types.contains(owner)
            || self.modules.contains(owner)
            || self
                .index
                .items()
                .iter()
                .any(|item| item.kind().is_type() && item.name() == owner)
    }

    /// Whether `owner::name` is a variant of a package enum (a constructor: no body).
    fn is_variant(&self, owner: &str, name: &str) -> bool {
        self.index.items().iter().any(|item| {
            item.kind() == ItemKind::Variant
                && item.type_owner() == Some(owner)
                && item.name() == name
        })
    }

    /// **The item-macro function or value a path names**, taken into `adds`: a function of
    /// `define_class!` by `Owner::name` (`Self` read as `self_owner`), a free one or a `static`
    /// of `thread_local!` by its name. `false` when it names none.
    fn take_macro_item(&self, adds: &mut Held, written: &str, self_owner: Option<&str>) -> bool {
        let segments: Vec<&str> = written.split("::").collect();
        let (last, prefix) = segments.split_last().expect("a path has a segment");
        let owner = prefix.last().map(|owner| match (*owner, self_owner) {
            ("Self", Some(self_owner)) => self_owner,
            (owner, _) => owner,
        });
        let mut found = false;
        for (at, function) in self.items.fns.iter().enumerate() {
            let owned_right = match (owner, function.owner.as_deref()) {
                (Some(written), Some(declared)) => written == declared,
                (Some(written), None) => self.modules.contains(written),
                (None, declared) => declared.is_none(),
            };
            if function.in_product && function.name == *last && owned_right {
                adds.fns.insert(at);
                found = true;
            }
        }
        if owner.is_none_or(|owner| self.modules.contains(owner)) {
            for (at, value) in self.items.values.iter().enumerate() {
                if value.in_product && value.name == *last {
                    adds.values.insert(at);
                    found = true;
                }
            }
        }
        found
    }

    /// **`sel!(selector)`**: the item-macro functions of that selector, taken in. A selector no
    /// function of the package declares is a foreign class's, and passes.
    fn take_selector(&self, adds: &mut Held, selector: &str) {
        for (at, function) in self.items.fns.iter().enumerate() {
            if function.in_product && function.selector.as_deref() == Some(selector) {
                adds.fns.insert(at);
            }
        }
    }
}

/// **The reach**: the guarded items, everything the call graph reaches from them, and what runs
/// with no call edge — the implicit impls and the item-macro functions whose self type a body of
/// the reach names, and what those reach — to a fixed point; and its refusals.
fn reached_spans(
    index: &Index,
    package: &str,
    roots: &[Range],
    guarded: &Guarded<'_>,
) -> (Vec<Range>, Vec<String>) {
    let resolver = Resolver::new(&[(package, index)], Arms::product()).unwrap_or_else(|refusals| {
        panic!("the resolver reads {package}: {refusals:?}");
    });
    let graph = CallGraph::new(&resolver);
    let names = Names::new(package, index, &resolver);
    let in_roots = |whole: Range| roots.iter().any(|root| inside(whole, *root));
    let mut held = Held {
        items: index
            .items()
            .iter()
            .filter(|item| item.kind().is_callable() && in_roots(range(item.whole())))
            .filter_map(|item| resolver.id_of(package, item))
            .filter(|id| resolver.admitted(*id))
            .collect(),
        fns: (0..names.items.fns.len())
            .filter(|at| names.items.fns[*at].in_product && in_roots(names.items.fns[*at].whole))
            .collect(),
        values: (0..names.items.values.len())
            .filter(|at| {
                names.items.values[*at].in_product && in_roots(names.items.values[*at].whole)
            })
            .collect(),
    };
    let implicit = resolver.implicit();
    loop {
        let pass = one_pass(&names, &graph, guarded, &held, &implicit);
        if !held.holds(&pass.adds) {
            held.extend(pass.adds);
            continue;
        }
        let mut refusals: Vec<String> = pass.refusals.into_values().collect();
        let mut unread = pass.unread;
        for (at, path, module, in_product) in &names.items.unparsed {
            if !in_product || !roots.iter().any(|root| holds(*root, *at)) {
                continue;
            }
            let caller = format!("{package} {module}");
            if !unread.take(|row| {
                let (row_caller, row_name, row_kind, _) = guarded.unread[row];
                row_caller == caller && row_name == path && row_kind == "item-macro"
            }) {
                refusals.push(format!(
                    "{}: `{path}!` is an item-position macro whose tokens do not parse as items \
                     (in `{caller}`, `item-macro`) — the functions it writes cannot be read",
                    where_is(index, *at)
                ));
            }
        }
        for (taken, (name, reason)) in pass.macros.taken.iter().zip(guarded.macros) {
            assert!(!reason.is_empty(), "a row says why: {name}");
            if !taken {
                refusals.push(format!(
                    "the MACROS row for `{name}!` allows a call no guarded item makes — take it off"
                ));
            }
        }
        for (taken, (caller, name, candidate, reason)) in
            pass.boundary.taken.iter().zip(guarded.boundary)
        {
            assert!(!reason.is_empty(), "a row says why: {caller} {name}");
            if !taken {
                refusals.push(format!(
                    "the BOUNDARY row dropping `{candidate}` from `{name}` in `{caller}` drops no \
                     candidate the walk meets — take it off"
                ));
            }
        }
        for (taken, (caller, name, kind, reason)) in unread.taken.iter().zip(guarded.unread) {
            assert!(!reason.is_empty(), "a row says why: {caller} {name}");
            if !taken {
                refusals.push(format!(
                    "the UNREAD row for `{name}` ({kind}) in `{caller}` allows an edge the walk \
                     does not meet — take it off"
                ));
            }
        }
        return (pass.spans, refusals);
    }
}

/// One walk from the held roots: what it reads, what it refuses, and what it adds to the roots.
fn one_pass(
    names: &Names<'_, '_>,
    graph: &CallGraph<'_, '_>,
    guarded: &Guarded<'_>,
    held: &Held,
    implicit: &BTreeMap<String, BTreeSet<ItemId>>,
) -> Pass {
    let resolver = names.resolver;
    let index = names.index;
    // A BOUNDARY row drops one candidate of one call: the walk never follows it from there.
    let boundary = RefCell::new(Ledger::new(guarded.boundary.len()));
    let follow = |edge: &Edge| -> Vec<ItemId> {
        let carried = match &edge.to {
            Target::Unresolved(
                Unresolved::Ambiguous { candidates, .. }
                | Unresolved::OtherArm { candidates }
                | Unresolved::Dynamic { candidates, .. },
            ) => candidates.clone(),
            _ => return Vec::new(),
        };
        let caller = resolver.node_key(edge.from);
        kept(
            &mut boundary.borrow_mut(),
            guarded,
            resolver,
            &caller,
            &edge.site.name,
            carried,
        )
    };
    let roots: Vec<ItemId> = held.items.iter().copied().collect();
    let walked = graph.walk_following(&roots, &BTreeSet::new(), &follow);
    let mut pass = Pass {
        spans: walked
            .reached
            .iter()
            .map(|id| range(resolver.record(*id).whole()))
            .chain(
                walked
                    .values
                    .iter()
                    .map(|id| range(resolver.value_record(*id).whole())),
            )
            .collect(),
        refusals: BTreeMap::new(),
        met: BTreeSet::new(),
        adds: Held::default(),
        boundary: boundary.into_inner(),
        unread: Ledger::new(guarded.unread.len()),
        macros: Ledger::new(guarded.macros.len()),
    };

    // The edges the graph could not follow, by kind.
    for (edge, _) in &walked.unresolved {
        let Target::Unresolved(kind) = &edge.to else {
            continue;
        };
        let caller = resolver.node_key(edge.from);
        let at = edge.site.span.start();
        let self_owner = match edge.from {
            Node::Item(id) => resolver.record(id).type_owner(),
            Node::Value(_) => None,
        };
        let what = match kind {
            Unresolved::Ambiguous { .. } | Unresolved::OtherArm { .. } => continue,
            Unresolved::Dynamic { candidates, .. } if !candidates.is_empty() => continue,
            Unresolved::Macro { name } => {
                let Some(record) = index.macros().iter().find(|record| {
                    record.kind() == MacroKind::Invocation && record.span().start() == at
                }) else {
                    refuse(
                        &mut pass,
                        guarded,
                        index,
                        at,
                        &caller,
                        name,
                        "macro-arguments",
                        &format!("`{name}!`, whose tokens the index holds no record of"),
                    );
                    continue;
                };
                let tokens = TokenStream::from_str(index.text(record.tokens()))
                    .expect("a macro's tokens are a token stream");
                let locals = || match edge.from {
                    Node::Item(id) => locals_of_item(index.text(resolver.record(id).whole())),
                    Node::Value(_) => Some(BTreeSet::new()),
                };
                let site = Site {
                    caller: &caller,
                    owner: self_owner,
                    module: innermost_module(index, record.span()),
                    base: record.tokens().start(),
                };
                read_macro(
                    names, guarded, &mut pass, &site, &locals, at, name, tokens, true,
                );
                continue;
            }
            Unresolved::Path { .. }
                if edge
                    .site
                    .macro_arg
                    .as_deref()
                    .is_some_and(|name| name.rsplit("::").next() == Some("sel")) =>
            {
                // A selector is a name, not a call: one of the package's is taken in.
                names.take_selector(&mut pass.adds, &edge.site.name);
                continue;
            }
            Unresolved::Path { written, .. }
                if names.take_macro_item(&mut pass.adds, written, self_owner) =>
            {
                continue;
            }
            Unresolved::Dynamic { what, .. } => {
                format!("a dynamic call with no candidate ({what})")
            }
            Unresolved::Path { written, why } => format!("an unplaced path `{written}` ({why})"),
            Unresolved::Generated { by } => format!("an item `{by}` generates"),
            Unresolved::Cutoff { what } => format!("a chain cut off ({what})"),
            Unresolved::Unparsed { why } => format!("an unparsed body ({why})"),
        };
        refuse(
            &mut pass,
            guarded,
            index,
            at,
            &caller,
            &edge.site.name,
            kind_of(kind),
            &what,
        );
    }

    // The item-macro functions and values held, read by syntax.
    for at in &held.fns {
        let function = &names.items.fns[*at];
        pass.spans.push(function.whole);
        let mut locals = Locals::default();
        locals.visit_impl_item_fn(&function.item);
        let mut reader = BodyReader {
            locals: &locals.0,
            owner: function.owner.as_deref(),
            base: function.base,
            reads: Vec::new(),
        };
        reader.visit_block(&function.item.block);
        let site = Site {
            caller: &function.key,
            owner: function.owner.as_deref(),
            module: &function.module,
            base: function.base,
        };
        place_reads(names, guarded, &mut pass, &site, &locals.0, reader.reads);
    }
    for at in &held.values {
        let value = &names.items.values[*at];
        pass.spans.push(value.whole);
        let mut locals = Locals::default();
        locals.visit_expr(&value.expr);
        let mut reader = BodyReader {
            locals: &locals.0,
            owner: None,
            base: value.base,
            reads: Vec::new(),
        };
        reader.visit_expr(&value.expr);
        let site = Site {
            caller: &value.key,
            owner: None,
            module: &value.module,
            base: value.base,
        };
        place_reads(names, guarded, &mut pass, &site, &locals.0, reader.reads);
    }

    // What runs with no call edge: an implicit impl (a `Drop` at a scope's end, a `Display` a
    // foreign `format!` calls), or an item-macro function, whose self type a body of the reach
    // names.
    let mut named = BTreeSet::new();
    for span in &pass.spans {
        let stream = TokenStream::from_str(text_of(index, *span))
            .expect("a body the index holds is a token stream");
        identifiers(stream, &mut named);
    }
    for id in implicit.values().flatten() {
        if resolver
            .record(*id)
            .type_owner()
            .is_some_and(|owner| named.contains(owner))
        {
            pass.adds.items.insert(*id);
        }
    }
    for (at, function) in names.items.fns.iter().enumerate() {
        if function.in_product
            && function
                .owner
                .as_ref()
                .is_some_and(|owner| named.contains(owner))
        {
            pass.adds.fns.insert(at);
        }
    }
    pass
}

/// **The candidates of one call the walk follows**: all but the ones a [`BOUNDARY`] row drops.
/// A row names a candidate by its key and drops every arm of it, taking one row per call.
fn kept(
    boundary: &mut Ledger,
    guarded: &Guarded<'_>,
    resolver: &Resolver<'_>,
    caller: &str,
    name: &str,
    candidates: Vec<ItemId>,
) -> Vec<ItemId> {
    let keys: BTreeSet<String> = candidates.iter().map(|id| resolver.key(*id)).collect();
    let dropped: BTreeSet<String> = keys
        .into_iter()
        .filter(|key| {
            boundary.take(|row| {
                let (row_caller, row_name, row_candidate, _) = guarded.boundary[row];
                row_caller == caller && row_name == name && row_candidate == key
            })
        })
        .collect();
    candidates
        .into_iter()
        .filter(|id| !dropped.contains(&resolver.key(*id)))
        .collect()
}

/// Where a reading by syntax stands: the body's key, its `Self`, the offset its tokens start at.
struct Site<'a> {
    caller: &'a str,
    owner: Option<&'a str>,
    /// The module path the body is written in.
    module: &'a str,
    base: usize,
}

/// Refuses what the guard cannot read at `at`, unless an untaken [`UNREAD`] row reviews it.
#[expect(
    clippy::too_many_arguments,
    reason = "a refusal names its site whole: the rows, the index, the offset, the caller, the name, the kind and what"
)]
fn refuse(
    pass: &mut Pass,
    guarded: &Guarded<'_>,
    index: &Index,
    at: usize,
    caller: &str,
    name: &str,
    kind: &str,
    what: &str,
) {
    if !pass
        .met
        .insert((at, caller.to_owned(), name.to_owned(), kind.to_owned()))
    {
        return;
    }
    if !UNEXCUSABLE.contains(&kind)
        && pass.unread.take(|row| {
            let (row_caller, row_name, row_kind, _) = guarded.unread[row];
            row_caller == caller && row_name == name && row_kind == kind
        })
    {
        return;
    }
    pass.refusals.entry(at).or_insert_with(|| {
        format!(
            "{}: {what} in `{caller}`, calling `{name}` (`{kind}`) — the guard cannot read what \
             it runs; make the call one the graph follows, or review it into UNREAD",
            where_is(index, at)
        )
    });
}

/// **A macro**, read where the call graph could not: a package `macro_rules!` is refused unless
/// a [`MACROS`] row lists it (its expansion is unread); `sel!(name)` takes in the item-macro
/// function of that selector; arguments the graph reads (`by_graph`) are its; arguments it does
/// not — in a body read by syntax, or `msg_send!`'s `part: argument` list — are read here; and
/// arguments that parse as none of those are refused by line when they hold a call or a path.
#[expect(
    clippy::too_many_arguments,
    reason = "a macro is read with its site, its bindings, its offset, its path, its tokens and who read it before"
)]
fn read_macro(
    names: &Names<'_, '_>,
    guarded: &Guarded<'_>,
    pass: &mut Pass,
    site: &Site<'_>,
    locals: &dyn Fn() -> Option<BTreeSet<String>>,
    at: usize,
    path: &str,
    tokens: TokenStream,
    by_graph: bool,
) {
    let name = path.rsplit("::").next().unwrap_or(path);
    if names.macro_rules.contains(name) {
        if !pass.macros.take(|row| guarded.macros[row].0 == name) {
            pass.refusals.entry(at).or_insert_with(|| {
                format!(
                    "{}: `{name}!` is a `macro_rules!` of the package — its expansion lands in a \
                     native menu or dialog's code unread; pass the words in from the caller, or \
                     list the macro in MACROS with why its expansion writes no label",
                    where_is(names.index, at)
                )
            });
        }
        return;
    }
    if name == "sel" {
        names.take_selector(&mut pass.adds, &tokens.to_string().replace(' ', ""));
        return;
    }
    let exprs = match graph_arguments(name, &tokens) {
        Some(_) if by_graph => return,
        Some(exprs) => Some(exprs),
        None if name == "msg_send" => message_body.parse2(tokens.clone()).ok(),
        None => None,
    };
    let Some(exprs) = exprs else {
        if holds_a_call_or_path(tokens, &names.package_names) {
            refuse(
                pass,
                guarded,
                names.index,
                at,
                site.caller,
                path,
                "macro-arguments",
                &format!(
                    "`{path}!`, whose arguments parse as no expression list and hold a call or a \
                     path"
                ),
            );
        }
        return;
    };
    let Some(locals) = locals() else {
        refuse(
            pass,
            guarded,
            names.index,
            at,
            site.caller,
            path,
            "macro-arguments",
            &format!("`{path}!`, in a body that does not parse as a function"),
        );
        return;
    };
    let mut reader = BodyReader {
        locals: &locals,
        owner: site.owner,
        base: site.base,
        reads: Vec::new(),
    };
    for expr in &exprs {
        reader.visit_expr(expr);
    }
    place_reads(names, guarded, pass, site, &locals, reader.reads);
}

/// **Places what a body read by syntax calls and names**: a free call takes in every package
/// function of the name (and of what a package `use` renames it from); `Owner::name` the
/// functions of that type or module, refused when the package names `Owner` and none is `name`;
/// a method every package method of the name but for the [`BOUNDARY`] rows; a value path the
/// package's `const` or `static` of the name, refused when its initializer calls; a called
/// binding is refused; a macro is [`read_macro`]'s.
fn place_reads(
    names: &Names<'_, '_>,
    guarded: &Guarded<'_>,
    pass: &mut Pass,
    site: &Site<'_>,
    locals: &BTreeSet<String>,
    reads: Vec<Read>,
) {
    let resolver = names.resolver;
    let index = names.index;
    for read in reads {
        match read {
            Read::Free { name } => {
                let mut wanted = names.imported_as(&name);
                wanted.push(name);
                for name in &wanted {
                    pass.adds.items.extend(
                        names.callables_named(name, |record| record.type_owner().is_none()),
                    );
                    names.take_macro_item(&mut pass.adds, name, site.owner);
                }
            }
            Read::Assoc { at, owner, name } => {
                let found = names.callables_named(&name, |record| {
                    record.type_owner() == Some(owner.as_str())
                        || (record.type_owner().is_none()
                            && record
                                .module_paths()
                                .iter()
                                .any(|module| module.rsplit("::").next() == Some(owner.as_str())))
                });
                let taken =
                    names.take_macro_item(&mut pass.adds, &format!("{owner}::{name}"), site.owner);
                if found.is_empty()
                    && !taken
                    && names.names_a_type_or_module(&owner)
                    && !names.is_variant(&owner, &name)
                {
                    refuse(
                        pass,
                        guarded,
                        index,
                        at,
                        site.caller,
                        &name,
                        "path",
                        &format!(
                            "an unplaced path `{owner}::{name}` (the package names `{owner}`, \
                             and no function of it is `{name}`)"
                        ),
                    );
                }
                pass.adds.items.extend(found);
            }
            Read::Method { name } => {
                let candidates =
                    names.callables_named(&name, |record| record.type_owner().is_some());
                pass.adds.items.extend(kept(
                    &mut pass.boundary,
                    guarded,
                    resolver,
                    site.caller,
                    &name,
                    candidates,
                ));
                for (at, function) in names.items.fns.iter().enumerate() {
                    if function.in_product && function.owner.is_some() && function.name == name {
                        pass.adds.fns.insert(at);
                    }
                }
            }
            Read::Value { path } => {
                let (last, prefix) = path.split_last().expect("a path has a segment");
                let owner = prefix.last().map(String::as_str);
                let imported = names.imported_as(last).iter().any(|name| name == last);
                for value in index.values() {
                    let placed = match owner {
                        // A bare name: the module's own value, or one a package `use` brings in.
                        None => {
                            value.type_owner().is_none()
                                && (imported || value.module_paths().contains(&site.module))
                        }
                        Some(owner) => {
                            value.type_owner() == Some(owner)
                                || (value.type_owner().is_none()
                                    && value
                                        .module_paths()
                                        .iter()
                                        .any(|module| module.rsplit("::").next() == Some(owner)))
                        }
                    };
                    if value.name() != last || !placed {
                        continue;
                    }
                    let whole = range(value.whole());
                    pass.spans.push(whole);
                    let text = text_of(index, whole);
                    let initializer = text.find('=').map_or("", |equals| &text[equals + 1..]);
                    let calls = TokenStream::from_str(initializer)
                        .is_ok_and(|stream| holds_a_call_or_path(stream, &BTreeSet::new()));
                    if calls {
                        refuse(
                            pass,
                            guarded,
                            index,
                            whole.0,
                            site.caller,
                            last,
                            "path",
                            &format!(
                                "the `const` or `static` `{last}`, named from a body read by \
                                 syntax, whose initializer calls or names a path"
                            ),
                        );
                    }
                }
                pass.adds
                    .items
                    .extend(names.callables_named(last, |record| {
                        owner.map_or(record.type_owner().is_none(), |owner| {
                            record.type_owner() == Some(owner)
                                || record
                                    .module_paths()
                                    .iter()
                                    .any(|module| module.rsplit("::").next() == Some(owner))
                        })
                    }));
                names.take_macro_item(&mut pass.adds, &path.join("::"), site.owner);
            }
            Read::Dynamic { at, what } => refuse(
                pass,
                guarded,
                index,
                at,
                site.caller,
                "(value)",
                "dynamic",
                &format!("a dynamic call ({what})"),
            ),
            Read::Macro { at, path, tokens } => {
                let known = || Some(locals.clone());
                read_macro(names, guarded, pass, site, &known, at, &path, tokens, false);
            }
        }
    }
}

/// **The refusals** over one index: each literal with a word in an open place of the scope that
/// no untaken row allows, each literal in a `macro_rules!` definition there, each edge the walk
/// cannot read, and each row nothing uses.
fn judge(index: &Index, package: &str, guarded: &Guarded<'_>) -> Vec<String> {
    let (mut spans, mut refusals) = guarded_spans(index, guarded);
    let (reached, unread) = reached_spans(index, package, &spans, guarded);
    spans.extend(reached);
    refusals.extend(unread);
    let mut allowed = Ledger::new(guarded.allowed.len());
    for file in index.files() {
        if !spans.iter().any(|span| inside(*span, range(file.span()))) {
            continue;
        }
        let path = slashed(file.path());
        for found in literals_of(index.text(file.span()), file.span().start()) {
            if matches!(
                found.place,
                Place::Attribute | Place::ErrorText | Place::ProgrammerText | Place::Abi
            ) || !spans.iter().any(|span| holds(*span, found.start))
            {
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
                    where_is(index, found.start)
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
            if found.place == Place::MacroDefinition {
                refusals.push(format!(
                    "{}: a literal in a `macro_rules!` definition ({text:?}) lands in an \
                     expansion this guard cannot see — pass the words in from the caller",
                    where_is(index, span.start())
                ));
                continue;
            }
            if !holds_a_word(&text) {
                continue;
            }
            let rows = guarded.allowed;
            if !allowed.take(|row| path.ends_with(rows[row].0) && rows[row].1 == text) {
                refusals.push(format!(
                    "{}: {text:?} (read {:?}) — a literal with a word in a native menu or \
                     dialog's product code, or in what it reaches, that no untaken ALLOWED row \
                     allows; a person reads it, so the caller passes it from `bt-app`'s i18n \
                     table",
                    where_is(index, span.start()),
                    as_read(&text)
                ));
            }
        }
    }
    for (taken, (suffix, text, reason)) in allowed.taken.iter().zip(guarded.allowed) {
        assert!(!reason.is_empty(), "a row says why: {text}");
        if !taken {
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
    for needle in [
        "\"Raw label\" (read",
        "\"Byte label\" (read",
        "\"C label\" (read",
        "\"Con\" (read",
        "\"cat\" (read",
        "\"Save &\" (read \"Save\")",
    ] {
        assert_eq!(
            count(&refusals, needle),
            1,
            "a raw, byte or C string, each piece of `concat!` and a trailing mnemonic are read \
             ({needle}): {refusals:#?}"
        );
    }
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
        12,
        "nothing else is refused — the error and programmer texts, the wordless literals, the \
         character literal, the attribute, the allowed row, the unguarded function and the test \
         module pass: {refusals:#?}"
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
    assert_eq!(
        count(
            &whole,
            "`unreadable_items!` is an item-position macro whose tokens do not parse as items"
        ),
        1,
        "{whole:#?}"
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
            ("no_words", "the fixture's macro with no word"),
            ("gone_macro", "a row nothing uses"),
        ],
        boundary: &[
            (
                "native-labels crate::Dialog::through_an_ambiguous_method",
                "push",
                "native-labels crate::reach::Stack::push",
                "the fixture's `.push` is on a `Vec`",
            ),
            (
                "native-labels crate::Dialog::through_an_ambiguous_method",
                "push",
                "native-labels crate::reach::Gone::push",
                "a row naming nothing",
            ),
        ],
        unread: &[
            (
                "native-labels crate::Dialog::through_a_reviewed_parameter",
                "label",
                "dynamic",
                "the fixture's reviewed call",
            ),
            (
                "native-labels crate::Dialog::gone",
                "label",
                "dynamic",
                "a row nothing uses",
            ),
            (
                "native-labels crate::Dialog::through_a_parameter",
                "label",
                "path",
                "a row of another kind than the edge's",
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
        "the BOUNDARY row dropping `native-labels crate::reach::Gone::push` from `push` in \
         `native-labels crate::Dialog::through_an_ambiguous_method` drops no candidate",
        "a BOUNDARY row that drops no candidate of its call is refused",
    );
    expect_one(
        "the UNREAD row for `label` (dynamic) in `native-labels crate::Dialog::gone`",
        "an UNREAD row no edge uses is refused",
    );
    expect_one(
        "the UNREAD row for `label` (path) in `native-labels crate::Dialog::through_a_parameter`",
        "an UNREAD row of another kind than the edge's excuses nothing, and is refused",
    );
    assert_eq!(
        count(&refusals, "ambiguous candidate"),
        0,
        "the BOUNDARY row drops the ambiguous candidate from its call: {refusals:#?}"
    );
    assert_eq!(
        refusals.len(),
        10,
        "nothing else is refused — the wordless const, the number, the listed macro, the macro \
         with no word, the entity's row and the unreached function pass: {refusals:#?}"
    );
}

/// RED (planted) — **what runs with no call edge and what the call graph does not parse is
/// read**: (4) a `Drop` impl of a type a reached body names; (5) the functions an
/// item-position macro writes (`define_class!`), by their selector and by their type, and a
/// `thread_local!`'s initializer, each read by syntax with its dynamic call and its unplaced path
/// refused; (6) a macro's arguments the graph does not parse — refused when they hold a call,
/// read as `msg_send!`'s list — and a `macro_rules!` of the package with no literal. Their twins
/// pass: a `Drop` of a type nothing names, a class function nothing names, a foreign selector,
/// macro arguments with no call.
#[test]
fn what_runs_with_no_call_edge_is_read() {
    let index = fixture();
    let refusals = judge(
        &index,
        "native-labels",
        &Guarded {
            files: &[],
            types: &["Surface"],
            functions: &[],
            allowed: &[],
            macros: &[],
            boundary: &[],
            unread: &[],
        },
    );
    let expect_one = |needle: &str, why: &str| {
        assert_eq!(count(&refusals, needle), 1, "{why}: {refusals:#?}");
    };
    expect_one("\"Copy from a drop\"", "(4) an implicit `Drop`");
    expect_one(
        "\"Copy from a class method\"",
        "(5) a function by its selector",
    );
    expect_one(
        "\"Copy from a panel\"",
        "(5) a function by a path to its type",
    );
    expect_one(
        "a dynamic call (`label`, a binding, called) in \
         `native-labels crate::reach::Panel::unreadable`",
        "(5) a function the type brings in, read by syntax",
    );
    expect_one(
        "an unplaced path `Dialog::missing`",
        "(5) a path the package names that no function answers",
    );
    expect_one(
        "\"Copy from a thread-local\"",
        "(5) a `thread_local!`'s initializer",
    );
    expect_one(
        "`foreign::render!`, whose arguments parse as no expression list and hold a call",
        "(6) unparsed macro arguments with a call",
    );
    expect_one("\"Copy from a message\"", "(6) `msg_send!`'s argument list");
    expect_one(
        "`calls_helper!` is a `macro_rules!` of the package",
        "(6) a package macro with no literal",
    );
    assert_eq!(
        refusals.len(),
        9,
        "nothing else is refused — the unnamed `Drop`, the unused class function, the foreign \
         selector and the arguments with no call pass: {refusals:#?}"
    );
}

/// RED (planted) — **an `ALLOWED` row allows one literal**: of a text written twice, one row
/// allows one and the other is refused; two rows allow both.
#[test]
fn an_allowed_row_allows_one_literal() {
    let index = fixture();
    let guarded = Guarded {
        files: &[],
        types: &[],
        functions: &["twice"],
        allowed: &[("lib.rs", "Twice", "the fixture's text")],
        macros: &[],
        boundary: &[],
        unread: &[],
    };
    let once = judge(&index, "native-labels", &guarded);
    assert_eq!(count(&once, "\"Twice\" (read"), 1, "{once:#?}");
    assert_eq!(once.len(), 1, "{once:#?}");
    let both = judge(
        &index,
        "native-labels",
        &Guarded {
            allowed: &[
                ("lib.rs", "Twice", "the fixture's text"),
                ("lib.rs", "Twice", "the fixture's text, again"),
            ],
            ..guarded
        },
    );
    assert!(both.is_empty(), "{both:#?}");
}

/// RED (planted) — **an ambiguous method's first-party candidates are followed**: without a
/// `BOUNDARY` row, the candidate `reach::Stack::push` of `rows.push(1)` is read and its literal
/// refused; with the row, the call's candidate is dropped.
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
                "native-labels crate::ambiguous_root",
                "push",
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
