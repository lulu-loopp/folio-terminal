//! **The first-page right-click item, which macOS does not have** (plan §8 Q3,
//! ruled 2026-09-12: `NSServices` only).
//!
//! On Windows this is a COM class Explorer creates in *this* executable, run
//! with `folio --explorer-command`, and it is the only part of `bt-platform`
//! where another program is the caller. macOS has no first-page context menu to
//! register into at all, which is why §1 of the plan puts "any MSIX equivalent"
//! out of 0.4 by name: the convention there is a **Service**, it is declared in
//! `Info.plist` and backed by a provider object, and it is M4-9 — a different
//! mechanism with a different ticket rather than this one ported.
//!
//! So this is not deferred work with a ticket behind it. It is a verb that will
//! never be served here, and the refusal is the whole of the implementation.
//! `bt_app::explorer_menu::serve` is reached only from a command line Explorer
//! writes, so on this platform it is reached by a person who typed the flag —
//! and what they are owed is the sentence, not silence.

use std::path::Path;
use std::time::Duration;

/// How long the server waits with no work before it leaves. Kept so the two
/// arms name the same things; nothing here ever waits.
pub const IDLE_LINGER: Duration = Duration::from_secs(10);

/// **What the verb is**: its two strings and what it does when clicked.
///
/// The same shape on both platforms, because it is the product's description of
/// a menu item rather than Explorer's: M4-9's Service provider is handed the
/// same folder and runs the same launch.
pub struct Verb {
    /// The words on the item.
    pub title: String,
    /// The icon beside them.
    pub icon: String,
    /// What to do with the folder that was clicked.
    pub invoke: Box<dyn Fn(&Path) + Send + Sync>,
}

/// Serve the verb until the caller stops asking. Refused, and it returns rather
/// than blocking: there is no class for anybody to create.
pub fn serve(verb: Verb) -> Result<(), String> {
    let _ = verb;
    Err("the first-page context-menu item is not on this platform".to_owned())
}
