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

use std::path::{Path, PathBuf};

use bt_platform::{CONTEXT_MENU_CLASSES, ContextMenuShape, ContextMenuState, ContextMenuTree};

use crate::explorer_menu::{MenuFate, MenuRemoval};
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

/// **What `--remove-explorer-menu` does about the classic verb** — the package
/// half's question ([`crate::explorer_menu::package_removal`]) asked of this
/// store, and answered out of the same two owners.
///
/// [`bt_platform::context_menu_verdict`] says whether there is a registration at
/// all, exactly as it does for the row: `Absent` is nothing to remove, and
/// `Current` is a `command` value naming this very executable, which is this
/// copy's by definition. `Stale` is the case that needs the second question, and
/// it is put to [`bt_platform::context_menu_reassert_wanted`] — whose whole
/// staleness-plus-ownership rule is what the launch-time repair already obeys:
/// rewrite, or here remove, **unless** a tree names a `folio.exe` that is still
/// on the disk and is not this one.
///
/// **A partial set that names both this copy and a live stranger is left
/// alone**, which falls out of that rule and is the conservative side: one of
/// the two trees would be somebody else's menu entry, and `remove_context_menu`
/// takes the verb out of both.
///
/// `desired` is an `Option` because a machine that will not say where its own
/// executable is cannot say which registrations are this copy's — and that is
/// [`MenuRemoval::Unanswerable`] rather than "there is nothing there", for the
/// reason R2-20 gave one store over: an answer that reads as absence is a
/// removal reporting success over an entry that is still in somebody's menu.
///
/// `on_disk` is the one impure input and is handed in, so the whole table can be
/// read without a file system under it.
#[must_use]
pub fn classic_removal(
    found: &[ContextMenuTree],
    desired: Option<&ContextMenuShape>,
    on_disk: impl Fn(&Path) -> bool,
) -> MenuRemoval {
    let Some(desired) = desired else {
        return MenuRemoval::Unanswerable;
    };
    match bt_platform::context_menu_verdict(found, desired) {
        ContextMenuState::Absent => MenuRemoval::Nothing,
        ContextMenuState::Current => MenuRemoval::Remove,
        ContextMenuState::Stale => {
            if bt_platform::context_menu_reassert_wanted(found, desired, on_disk) {
                MenuRemoval::Remove
            } else {
                MenuRemoval::AnotherCopy
            }
        }
    }
}

/// The classic half of `--remove-explorer-menu`: read the trees, decide, and act
/// on the decision.
///
/// The read is done once and both the decision and the sentence come off it, for
/// [`reassert`]'s reason: a second reading would be of a registry this function
/// has since changed.
///
/// **The language is deliberately not installed first**, unlike
/// `report_at_the_front_door`: this path says its one line in English by
/// contract, and opening the settings file for a folder that is being deleted is
/// work with nothing to show for it. What [`desired`] then builds is a shape
/// whose *label* may be in the other column — and the answer cannot turn on it.
/// A verb written in the other language reads [`ContextMenuState::Stale`]
/// instead of `Current`, and `Stale` over a `command` naming this very
/// executable answers `Remove` through the ownership rule, which is what
/// `Current` answers directly. The `command` and the `folio.exe` in it are what
/// every branch actually turns on, and neither is translated.
pub(crate) fn classic_taken_off() -> MenuFate {
    let desired = desired();
    let found = bt_platform::read_context_menu(CONTEXT_MENU_CLASSES);
    match classic_removal(&found, desired.as_ref(), |exe| exe.is_file()) {
        MenuRemoval::Nothing => MenuFate::left("no Show more options entry was registered"),
        MenuRemoval::Unanswerable => MenuFate::left(
            "this machine would not say where its own folio.exe is, so the Show more options \
             entry was left",
        ),
        MenuRemoval::AnotherCopy => MenuFate::left(match registered_exe(&found) {
            Some(exe) => {
                format!("the Show more options entry was left: it runs {exe}, which is still there")
            }
            None => "the Show more options entry was left: it runs another Folio".to_owned(),
        }),
        MenuRemoval::Remove => MenuFate::attempted("the Show more options entry", apply(false)),
    }
}

/// The cleanup door shares the existing ownership decision and remover.
pub(crate) fn cleanup_classic() -> crate::explorer_menu::CleanupRegistration {
    use crate::explorer_menu::CleanupRegistration;
    let desired = desired();
    let found = bt_platform::read_context_menu(CONTEXT_MENU_CLASSES);
    match classic_removal(&found, desired.as_ref(), |exe| exe.is_file()) {
        MenuRemoval::Nothing => CleanupRegistration::Absent,
        MenuRemoval::Unanswerable => {
            CleanupRegistration::Refused(Text::CleanupSystemUnknown.text().to_owned())
        }
        MenuRemoval::AnotherCopy => match registered_exe(&found) {
            Some(path) => CleanupRegistration::Other(PathBuf::from(path)),
            None => CleanupRegistration::Refused(Text::CleanupSystemUnknown.text().to_owned()),
        },
        MenuRemoval::Remove => match apply(false) {
            Ok(()) => CleanupRegistration::Removed,
            Err(reason) => CleanupRegistration::Refused(reason),
        },
    }
}

/// The first `folio.exe` the trees name, for the sentence that says whose entry
/// was left alone.
///
/// **For the report and never for the decision** — which executable a tree names
/// is `bt_platform`'s to read and
/// [`bt_platform::context_menu_reassert_wanted`]'s to judge, and this asks only
/// so that a line in somebody's uninstall log can say where to look.
fn registered_exe(found: &[ContextMenuTree]) -> Option<String> {
    found
        .iter()
        .filter_map(ContextMenuTree::shape)
        .find_map(|shape| bt_platform::context_menu_command_exe(&shape.command))
        .map(str::to_owned)
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

    /// **RED (A1/A5) — the whole table of what `--remove-explorer-menu` does
    /// about the classic verb.**
    ///
    /// The same three claims the package half makes, over the store that has
    /// four registry values instead of one folder. ① What this build would write
    /// now names this very binary and is this copy's to take away. ② A verb whose
    /// `folio.exe` is **gone** is removed too — it is the state an uninstall
    /// leaves and clicking it does nothing at all. ③ A verb naming a `folio.exe`
    /// that is still there belongs to another copy of Folio and is left (B1),
    /// **including** a half-and-half set, because `remove_context_menu` takes the
    /// verb out of both trees and there is no way to remove only ours.
    ///
    /// Two shapes that are not obvious are pinned beside them: a set somebody
    /// deleted half of is still this copy's to clear, and a verb key with no
    /// `command` under it (R2-26) names no executable at all — so it is nobody's
    /// to lose and the flag clears it.
    ///
    /// MUTATION: read `Stale` as `Remove` without asking
    /// `context_menu_reassert_wanted` and the last two cases go red, which is
    /// this copy deleting a menu entry another Folio is answering. Answer
    /// `Nothing` for a machine that will not say where its own executable is and
    /// the final assertion goes red — a removal reporting success over an entry
    /// it never looked at.
    #[test]
    fn the_flag_removes_this_copys_verb_and_a_verb_that_runs_nothing() {
        const OURS: &str = r"D:\Tools\Folio\folio.exe";
        const STRANGER: &str = r"D:\Other\Folio\folio.exe";
        let shape = |exe: &str| bt_platform::context_menu_shape(Path::new(exe), "Open Folio here");
        let written = |exe: &str| ContextMenuTree::Written(shape(exe));
        let desired = shape(OURS);
        // Only the stranger's binary is on the disk: ours has just been deleted
        // by the uninstall that is running this flag, which is the ordinary
        // shape of the machine this flag runs on.
        let on_disk = |exe: &Path| exe == Path::new(STRANGER);
        let cases = [
            (
                "nothing written",
                vec![ContextMenuTree::Absent, ContextMenuTree::Absent],
                MenuRemoval::Nothing,
            ),
            (
                "exactly what this build would write",
                vec![written(OURS), written(OURS)],
                MenuRemoval::Remove,
            ),
            (
                "a set somebody deleted half of",
                vec![written(OURS), ContextMenuTree::Absent],
                MenuRemoval::Remove,
            ),
            (
                "a verb key with no command under it",
                vec![ContextMenuTree::Broken, ContextMenuTree::Broken],
                MenuRemoval::Remove,
            ),
            (
                "another copy's, and its folio.exe is still there",
                vec![written(STRANGER), written(STRANGER)],
                MenuRemoval::AnotherCopy,
            ),
            (
                "ours in one tree and another copy's in the other",
                vec![written(OURS), written(STRANGER)],
                MenuRemoval::AnotherCopy,
            ),
        ];
        for (what, found, expected) in cases {
            assert_eq!(
                classic_removal(&found, Some(&desired), on_disk),
                expected,
                "{what}"
            );
        }
        assert_eq!(
            classic_removal(
                &[written(STRANGER), written(STRANGER)],
                Some(&desired),
                |_| false
            ),
            MenuRemoval::Remove,
            "a verb naming a folio.exe that is gone is answered by nobody"
        );
        assert_eq!(
            classic_removal(&[written(OURS), written(OURS)], None, on_disk),
            MenuRemoval::Unanswerable,
            "a machine that will not say where its own folio.exe is cannot say \
             whose the entry is"
        );
    }
}
