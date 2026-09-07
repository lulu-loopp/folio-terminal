//! **Folio's verb in Explorer's right-click menu** — `docs/DESIGN.md` §7.4,
//! Windows landing block slice 2.
//!
//! # What is written, and by whom
//!
//! Two registry trees under `HKEY_CURRENT_USER\Software\Classes` — one for a
//! folder's own icon, one for the empty space inside an open folder — each
//! carrying a label, an icon and the command line
//! `"…\folio.exe" --cwd "%V"`. The values and the trees are
//! `bt_platform`'s ([`bt_platform::ContextMenuShape`],
//! [`bt_platform::CONTEXT_MENU_TREES`]); what this file holds is the two facts
//! that are the *product's* rather than the platform's — which exe, and which
//! words — and the rule about when the registry is written.
//!
//! **There is no installer.** Folio ships as a bare `.exe`, so the only honest
//! place for a registration is a switch inside the program, and the only honest
//! hive for it is `HKCU`: the account that ran Folio is the account whose menu
//! changes, no elevation is asked for, and nothing another user of the machine
//! can see is touched. `Settings ▸ General ▸ Explorer context menu` is that
//! switch.
//!
//! # The switch reads the registry, it does not remember
//!
//! [`state`] asks the machine every time it is asked, and the row is drawn from
//! that answer. A remembered "I turned this on once" would be wrong the moment
//! anybody edited the registry by hand, uninstalled a copy of Folio from another
//! folder, or restored a machine from a backup — and being wrong here means a
//! switch reading `On` over a menu that has no such entry.
//!
//! # Why the launch writes at all
//!
//! The `command` value holds an **absolute path**, and Folio is a program people
//! move: dropped in `Downloads`, tried, then dragged to `C:\Tools`. The menu
//! entry survives that move pointing at nothing, the click does nothing at all,
//! and there is no installer anywhere to notice. So every launch reads the
//! trees, and a set that is present but not what this build would write now is
//! written again — silently, because it is **this user's own data being made to
//! say what they already asked it to say**. Nothing is created that was not
//! there: a machine that never installed the verb is left alone
//! ([`bt_platform::ContextMenuState::Absent`]).
//!
//! The same rule carries a language change into the menu, and repairs a set
//! somebody deleted half of by hand, because all three are the same finding —
//! see [`bt_platform::ContextMenuState::Stale`].
//!
//! # And why it does not always write
//!
//! "Not what this build would write" is also true of a registration another
//! **live** copy of Folio put there, and rewriting that one is not a repair: it
//! is this process taking a menu entry off a program that is answering it
//! perfectly well. The rule is
//! [`bt_platform::context_menu_reassert_wanted`]'s — rewrite unless a tree names
//! a `folio.exe` that is still on the disk and is not this one — and the day it
//! was written is the day a debug build, run once out of a scratch folder,
//! pointed somebody's own right-click menu at a folder that was deleted an hour
//! later. The explicit switch is not affected: a press asks for *this* Folio by
//! name, and [`apply`] writes.
//!
//! # Windows 11's primary menu
//!
//! This verb lands under **"Show more options"**, not in the short menu that
//! opens first. That is a property of the registration and not of this code:
//! Windows 11 promotes only an `IExplorerCommand` declared by a **package**, and
//! a package means a signed sparse MSIX with an identity, which is a shape this
//! product does not have and would have to acquire for other reasons first. The
//! spike measured it (`docs/spikes/spike-win-landing.md` §4) and the cost is
//! recorded in `docs/DESIGN.md` §7.4; the row's own sentence says where the
//! entry will be found so that nobody has to go looking.

use std::path::PathBuf;

use bt_platform::{CONTEXT_MENU_CLASSES, ContextMenuShape, ContextMenuState};

use crate::i18n::Text;

/// Where `folio.exe` is, as the registry will have to name it.
///
/// `None` only where the operating system will not say — a case with no repair
/// and no report worth making, since every answer this module gives degrades to
/// "there is nothing to do" rather than to a guess. It is deliberately **not**
/// `current_dir()`, which for a process the shell started is `folio.exe`'s own
/// folder and has nothing to do with what was clicked (`cli.rs`'s header).
fn executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// The three values this build would write right now, in the language the window
/// is currently writing in.
///
/// The label goes into the registry as a literal `REG_SZ`. `MUIVerb` — a pointer
/// at a string resource, which Explorer would re-read per user language — is the
/// route the bilingual plan will want and is not this slice: it needs a string
/// table in the binary's resources, which this product does not have, and the
/// literal has a repair already ([`reassert`] rewrites a label that no longer
/// matches).
#[must_use]
pub fn desired() -> Option<ContextMenuShape> {
    Some(bt_platform::context_menu_shape(
        &executable()?,
        Text::ContextMenuVerb.text(),
    ))
}

/// What this machine's registry currently says, against what this build would
/// write.
#[must_use]
pub fn state() -> ContextMenuState {
    let Some(desired) = desired() else {
        return ContextMenuState::Absent;
    };
    bt_platform::context_menu_verdict(
        &bt_platform::read_context_menu(CONTEXT_MENU_CLASSES),
        &desired,
    )
}

/// Whether the switch reads `On` — which is "there is a menu entry", not "it
/// points at me".
///
/// [`ContextMenuState::Stale`] is `On` on purpose: there **is** an entry in the
/// user's menu, and a switch reading `Off` over one would be a lie that also
/// left them no way to remove it. The launch has already repaired it by the
/// time anybody can read the row anyway.
#[must_use]
pub fn installed(state: ContextMenuState) -> bool {
    state != ContextMenuState::Absent
}

/// Write the verb, or take it back out.
///
/// The switch's whole action, in one function, so that the two directions
/// cannot disagree about which trees they are talking about.
///
/// **And it ends by telling the shell** — [`bt_platform::changing_explorer_menu`],
/// the same wrapper the package's two deployment calls go through. Explorer
/// reads the class store once and remembers what it found; a verb written into
/// `HKCU\Software\Classes` while it is running is a verb nobody sees until
/// something else happens to invalidate that cache, which for most people is the
/// next time they sign in.
///
/// **The announcement is made here rather than one level down**, in
/// [`bt_platform::install_context_menu`], for one reason: that function takes
/// the class store as an argument because the suite calls it against an isolated
/// subkey of its own, and a test run must not broadcast a shell-wide refresh on
/// the reader's machine. This is the only caller that writes into the store
/// Explorer actually reads.
///
/// **The one refusal that writes nothing is settled before the wrapper is
/// entered**, so that "the shell is told after every attempt" stays literally
/// true: a machine that will not say where its own executable is has no shape to
/// write, and there is nothing for anybody to have noticed.
pub fn apply(install: bool) -> Result<(), String> {
    let shape = install
        .then(|| desired().ok_or_else(|| Text::ContextMenuNoExecutable.text().to_owned()))
        .transpose()?;
    bt_platform::changing_explorer_menu(
        || match &shape {
            Some(shape) => bt_platform::install_context_menu(CONTEXT_MENU_CLASSES, shape),
            None => bt_platform::remove_context_menu(CONTEXT_MENU_CLASSES),
        },
        bt_platform::announce_explorer_menu_change,
    )
}

/// The launch-time repair — see the module header.
///
/// Returns whether the machine carries the verb afterwards, which is what the
/// row is drawn from. A failure to rewrite is not reported anywhere: there is no
/// window yet to report it on, the entry that is already there goes on being
/// whatever it was, and the next launch will try again. What it must not do is
/// stop the launch.
///
/// Whether it writes at all is [`bt_platform::context_menu_reassert_wanted`]'s
/// answer, and the disk it is asked about is this one: `is_file` on the path the
/// registered command line names. The read is done once and both answers — the
/// write and the row's `On` — come off it, so the row cannot be drawn from a
/// second reading of a registry this function has since changed.
pub fn reassert() -> bool {
    let Some(desired) = desired() else {
        return false;
    };
    let found = bt_platform::read_context_menu(CONTEXT_MENU_CLASSES);
    if bt_platform::context_menu_reassert_wanted(&found, &desired, |exe| exe.is_file()) {
        let _ = apply(true);
    }
    installed(bt_platform::context_menu_verdict(&found, &desired))
}

// **The row's sentence used to be written here** and moved to
// `explorer_menu::row_description` on 2026-09-07, when the two Explorer rows
// became one. It is one sentence about two stores now — what the entry says, and
// what the first page has to register — and the module that can see both stores
// is the one that composes it. Nothing about the words changed hands: they are
// still a fact about this Windows rather than about the row.

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **the row reads the machine, and `Stale` is still `On`.**
    ///
    /// The distinction the whole module turns on: a menu entry that points at a
    /// `folio.exe` which has since moved is an entry that is *there*. A switch
    /// reading `Off` over it would be both untrue and a dead end — the only
    /// control that could remove it would be refusing to admit it exists.
    ///
    /// MUTATION: make `installed` answer `state == Current` and the third
    /// assertion goes red, which is a moved binary leaving an unremovable entry
    /// in somebody's right-click menu.
    #[test]
    fn the_switch_is_on_whenever_the_machine_carries_a_verb() {
        assert!(!installed(ContextMenuState::Absent));
        assert!(installed(ContextMenuState::Current));
        assert!(installed(ContextMenuState::Stale));
    }

    /// PIN — **what this build would write names this build's own binary.**
    ///
    /// The absolute path is the whole reason [`reassert`] exists, so the shape
    /// has to be derived from `current_exe` every time it is asked rather than
    /// resolved once and remembered.
    #[test]
    fn the_shape_names_this_process_and_carries_the_flag_the_verb_needs() {
        let exe = executable().expect("this test is a process and knows where it is");
        let shape = desired().expect("and so the shape can be built");
        assert!(
            shape.command.contains(&exe.display().to_string()),
            "the command names this binary: {}",
            shape.command
        );
        assert!(
            shape.command.ends_with(r#"--cwd "%V""#),
            "and hands the clicked folder over as an argument: {}",
            shape.command
        );
        assert!(
            shape.icon.ends_with(",0"),
            "the icon is the exe's own first: {}",
            shape.icon
        );
        assert!(!shape.label.is_empty(), "and the menu has words in it");
    }

    /// RED (2026-09-07) — **the classic registration tells the shell too, and
    /// [`apply`] is the one place it can.**
    ///
    /// [`apply`] cannot be called from a test: it writes into
    /// `HKCU\Software\Classes`, which is the right-click menu of whoever is
    /// running the suite. So what is held is the shape — that the whole of it
    /// sits inside [`bt_platform::changing_explorer_menu`], which is the wrapper
    /// the package's two deployment calls go through as well.
    ///
    /// `bt_platform`'s own suite proves the wrapper announces on both the
    /// success and the refusal; this proves the classic half is behind it. The
    /// two together are "every write to that menu is announced", which is the
    /// whole of the finding: `SHChangeNotify` appeared nowhere in this
    /// repository for two releases, and a shell that is never told goes on
    /// drawing the menu it read when it started.
    ///
    /// **The suite's half of the file is cut off before the search**, or the
    /// needle would find the line that names it here.
    ///
    /// MUTATION: call `install_context_menu` or `remove_context_menu` outside
    /// the wrapper and this goes red.
    #[test]
    fn the_classic_registration_is_announced_to_the_shell_as_well() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("context_menu.rs"),
        )
        .expect("this module is a file in this crate");
        let code = source
            .split_once("#[cfg(test)]")
            .expect("this module carries the suite this test is in")
            .0;
        assert_eq!(
            code.matches("bt_platform::changing_explorer_menu(").count(),
            1,
            "one wrapper, around the one function that writes"
        );
        for call in [
            "bt_platform::install_context_menu(",
            "bt_platform::remove_context_menu(",
        ] {
            assert_eq!(
                code.matches(call).count(),
                1,
                "{call} is written once, inside that wrapper"
            );
        }
    }
}
