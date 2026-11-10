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
//! trees — on the start's carry pass, off the window thread, and held during an
//! update's trial until its commit (0.4.10 plan row 68b; [`run_classic_step`]) —
//! and a set that is present but not what this build would write now is
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
//! [`bt_platform::explorer_reassert_wanted`]'s — rewrite unless a tree names
//! a `folio.exe` that is still on the disk and is not this one — and the day it
//! was written is the day a debug build, run once out of a scratch folder,
//! pointed somebody's own right-click menu at a folder that was deleted an hour
//! later. At start the two facts it takes are the start-time scan's (row 68b,
//! R5; [`classic_found`]): this one is this copy's file, links followed, and gone
//! is missing on a volume that is there — a program on a drive that is away is
//! not gone. The explicit switch is not affected: a press asks for *this* Folio
//! by name, and [`apply_pressed`] writes.
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bt_platform::admission::WorkerCtx;
use bt_platform::{
    CONTEXT_MENU_CLASSES, ContextMenuShape, ContextMenuState, ContextMenuTree, RegisteredExe,
};

use crate::attention_ownership::{Look, gone_by, what_stands};
use crate::carry_pass::{LastSaid, MemberAnswer};
use crate::explorer_menu::{MenuFate, MenuRemoval, same_path};
use crate::i18n::Text;
use crate::update_trial::Writer;

/// Where `folio.exe` is, as the registry will have to name it.
///
/// `None` only where the operating system will not say — a case with no repair
/// and no report worth making, since every answer this module gives degrades to
/// "there is nothing to do" rather than to a guess. It is deliberately **not**
/// `current_dir()`, which for a process the shell started is `folio.exe`'s own
/// folder and has nothing to do with what was clicked (`cli.rs`'s header).
///
/// The program's own file, links followed (`bt_platform::running_image`, G-SWEEP-048): a verb that
/// named winget's alias would start the program through a link whose folder holds none of its
/// files.
fn executable() -> Option<PathBuf> {
    bt_platform::running_image()
        .ok()
        .map(|image| image.path.clone())
}

/// The three values this build would write right now, in the language the window
/// is currently writing in.
///
/// The label goes into the registry as a literal `REG_SZ`. `MUIVerb` — a pointer
/// at a string resource, which Explorer would re-read per user language — is the
/// route the bilingual plan will want and is not this slice: it needs a string
/// table in the binary's resources, which this product does not have, and the
/// literal has a repair already ([`reassert_in`] rewrites a label that no longer
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

/// The switch's press (`Settings ▸ General ▸ Explorer context menu`): [`apply`],
/// counted first, so that a start's repair running beside it on the carry pass
/// yields to it ([`ClassicPresses`]).
pub fn apply_pressed(install: bool) -> Result<(), String> {
    PRESSES.note_press(install);
    apply(install)
}

/// **The presses of the switch, as the start's repair asks about them** (0.4.10
/// plan row 68b, R4).
///
/// The repair runs on the carry pass's worker and a press on the window thread,
/// so the two can meet, and **a press after the repair's read wins**: the press
/// records its wish and counts itself before it writes ([`apply_pressed`]); the
/// repair writes only if no press landed since its read, and writes the last
/// press's wish again when one landed during its write ([`reassert_in`]); and an
/// answer read before the last press is not adopted
/// ([`adopt_classic_answer`]) — the press's own read is newer.
///
/// The wish is stored before the count moves, so whoever sees the count move
/// reads that press's wish.
pub(crate) struct ClassicPresses {
    count: AtomicU64,
    wish: AtomicBool,
}

impl ClassicPresses {
    pub(crate) const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            wish: AtomicBool::new(false),
        }
    }

    /// A press asking for the verb (`true`) or for its removal.
    pub(crate) fn note_press(&self, install: bool) {
        self.wish.store(install, Ordering::SeqCst);
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    /// How many presses have landed.
    pub(crate) fn presses_counted(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    /// The last press's wish.
    fn wish(&self) -> bool {
        self.wish.load(Ordering::SeqCst)
    }
}

/// This process's presses.
static PRESSES: ClassicPresses = ClassicPresses::new();

/// **What the classic member answers the window**: whether the machine carries
/// the verb after the round, and how many presses had landed when the round read
/// the trees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClassicAnswer {
    pub(crate) installed: bool,
    pub(crate) presses_seen: u64,
}

/// **The window takes a round's answer for the row** — unless a press has landed
/// since the round read the trees: the press read the machine after its own
/// write, and that read is newer (R4).
pub(crate) fn adopt_classic_answer(installed: &mut bool, answer: ClassicAnswer) {
    if answer.presses_seen == PRESSES.presses_counted() {
        *installed = answer.installed;
    }
}

/// **The trees the repair reads and writes**: the account's class store in the
/// product ([`AccountClasses`]); a test's own.
pub(crate) trait ClassicStore {
    /// What each tree holds, in [`bt_platform::CONTEXT_MENU_TREES`]'s order.
    fn read_classic_trees(&self) -> Vec<ContextMenuTree>;
    /// Write `shape` into every tree, or take the verb out of every tree.
    fn write_classic_verb(&self, shape: Option<&ContextMenuShape>) -> Result<(), String>;
}

/// The account's own class store, written through [`apply`], which tells the
/// shell.
struct AccountClasses;

impl ClassicStore for AccountClasses {
    fn read_classic_trees(&self) -> Vec<ContextMenuTree> {
        bt_platform::read_context_menu(CONTEXT_MENU_CLASSES)
    }

    fn write_classic_verb(&self, shape: Option<&ContextMenuShape>) -> Result<(), String> {
        apply(shape.is_some())
    }
}

/// **What the start finds in the trees** — the classic member's word for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassicFound {
    /// No tree carries the verb: nothing is created.
    Absent,
    /// Exactly what this build would write.
    Current,
    /// Not what this build would write, and every tree names this copy or no
    /// program at all: an older shape, a label in a language left, a half set.
    OldShape,
    /// A tree names a program that is gone, and none a program that is there.
    Dead,
    /// A tree names another copy that is still there: left.
    AnotherCopy,
    /// A tree names a program the machine will not say about — a drive that is
    /// away among them: left.
    OwnerUnknown,
}

impl ClassicFound {
    const fn found_word(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Current => "current",
            Self::OldShape => "old shape",
            Self::Dead => "dead",
            Self::AnotherCopy => "another copy",
            Self::OwnerUnknown => "owner unknown",
        }
    }

    /// Whether the start rewrites what it found.
    const fn repair_wanted(self) -> bool {
        matches!(self, Self::OldShape | Self::Dead)
    }
}

/// **Read the trees against this build, with the start-time scan's ownership
/// rule** (0.4.10 plan row 68b, R5).
///
/// The decision is [`bt_platform::explorer_reassert_wanted`]'s, the one rule
/// both Explorer registrations obey; what it is handed for each program a tree
/// names is this rule's: **this copy** when the path leads to this very file,
/// links and junctions followed ([`same_path`]) — so a verb written through
/// winget's link (every build up to 0.4.7 named the program by `current_exe`)
/// is this copy's old shape and not a stranger's (G7) — and otherwise **gone**
/// only when nothing is at it on a volume that is there ([`gone_by`]). A program
/// still there, and one the machine will not say about, are a copy that is
/// still on the disk, and are left.
pub(crate) fn classic_found(
    found: &[ContextMenuTree],
    desired: &ContextMenuShape,
    this: &Path,
    look: &(impl Look + ?Sized),
) -> ClassicFound {
    match bt_platform::context_menu_verdict(found, desired) {
        ContextMenuState::Absent => return ClassicFound::Absent,
        ContextMenuState::Current => return ClassicFound::Current,
        ContextMenuState::Stale => {}
    }
    let named: Vec<(RegisteredExe, Result<bool, &'static str>)> = found
        .iter()
        .filter_map(ContextMenuTree::shape)
        .filter_map(|shape| bt_platform::context_menu_command_exe(&shape.command))
        .map(Path::new)
        .map(|exe| {
            if same_path(exe, this) {
                let ours = RegisteredExe {
                    on_disk: true,
                    ours: true,
                };
                return (ours, Ok(false));
            }
            let gone = gone_by(exe, look);
            let named = RegisteredExe {
                on_disk: gone != Ok(true),
                ours: false,
            };
            (named, gone)
        })
        .collect();
    if bt_platform::explorer_reassert_wanted(named.iter().map(|(exe, _)| *exe)) {
        if named.iter().any(|(_, gone)| *gone == Ok(true)) {
            ClassicFound::Dead
        } else {
            ClassicFound::OldShape
        }
    } else if named
        .iter()
        .any(|(exe, gone)| !exe.ours && *gone == Ok(false))
    {
        ClassicFound::AnotherCopy
    } else {
        ClassicFound::OwnerUnknown
    }
}

/// **One run of the start's repair**: what it found, what it did, and the
/// window's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassicRun {
    pub(crate) found: ClassicFound,
    /// `None` when nothing was due; otherwise `written`, `held`, `pressed` or
    /// `refused (<reason>)`.
    pub(crate) repaired: Option<String>,
    pub(crate) answer: ClassicAnswer,
}

impl ClassicRun {
    /// `BT_EXPLORER_CLASSIC <found>[ repaired=<what>]` — no path (the Explorer
    /// modules' rule).
    pub(crate) fn classic_line(&self) -> String {
        match &self.repaired {
            Some(repaired) => format!(
                "BT_EXPLORER_CLASSIC {} repaired={repaired}",
                self.found.found_word()
            ),
            None => format!("BT_EXPLORER_CLASSIC {}", self.found.found_word()),
        }
    }
}

/// **The start's repair of the classic verb** (0.4.10 plan row 68b) — see the
/// module header.
///
/// One read, classified ([`classic_found`]); the write only when the finding is
/// this copy's old shape or a dead owner's, and then **only after the trial's
/// gate has said yes** (`defer` answers `true` to hold it: an update's trial
/// does not rewrite the verb the build it may roll back to is answering — G6),
/// and **only if no press has landed since the read** (R4). A press that lands
/// during the write is written again after it, until none lands. The row's
/// answer comes off a read after the write, so it is never a second reading of a
/// registry this function had not yet changed.
///
/// A refused write is said and left: the entry goes on being whatever it was,
/// and the next start asks again.
pub(crate) fn reassert_in(
    store: &impl ClassicStore,
    desired: &ContextMenuShape,
    this: &Path,
    look: &(impl Look + ?Sized),
    defer: impl FnOnce(Writer) -> bool,
    presses: &ClassicPresses,
) -> ClassicRun {
    let seen = presses.presses_counted();
    let found = store.read_classic_trees();
    let judged = classic_found(&found, desired, this, look);
    let run = |repaired: Option<String>, trees: &[ContextMenuTree]| ClassicRun {
        found: judged,
        repaired,
        answer: ClassicAnswer {
            installed: installed(bt_platform::context_menu_verdict(trees, desired)),
            presses_seen: seen,
        },
    };
    if !judged.repair_wanted() {
        return run(None, &found);
    }
    if defer(Writer::ExplorerRepair) {
        return run(Some("held".to_owned()), &found);
    }
    if presses.presses_counted() != seen {
        return run(Some("pressed".to_owned()), &found);
    }
    let written = store.write_classic_verb(Some(desired));
    let mut answered = seen;
    while presses.presses_counted() != answered {
        answered = presses.presses_counted();
        let _ = store.write_classic_verb(presses.wish().then_some(desired));
    }
    let repaired = match written {
        Ok(()) if answered != seen => "pressed".to_owned(),
        Ok(()) => "written".to_owned(),
        Err(reason) => format!("refused ({reason})"),
    };
    run(Some(repaired), &store.read_classic_trees())
}

/// The classic member's last line.
static CLASSIC_SAID: LastSaid = LastSaid::new();

/// **A run as the pass's part**: one check, changed when its line differs from the
/// last said in this process.
pub(crate) fn classic_member_answer(
    run: &ClassicRun,
    said: &LastSaid,
    say: &dyn Fn(String),
) -> MemberAnswer<ClassicAnswer> {
    let changed = said.say_if_new(run.classic_line(), say);
    MemberAnswer {
        part: run.answer,
        checks: 1,
        changed: usize::from(changed),
    }
}

/// **The classic verb, as a member of the start's carry pass** (0.4.10 plan row
/// 68b; [`crate::carry_pass::run_carry_steps`]): the account's trees, this
/// copy's file, this machine's disk, the trial's gate and this process's presses.
///
/// Nothing off Windows, where there are no trees; nothing in a test process,
/// whose pass must never read or write the account's right-click menu — the
/// tests drive [`reassert_in`] over trees of their own. Nothing, and one line,
/// when the system will not say where this copy's program is: there is no shape
/// to compare against.
pub(crate) fn run_classic_step(_ctx: &WorkerCtx) -> Option<MemberAnswer<ClassicAnswer>> {
    if !cfg!(windows) || cfg!(test) {
        return None;
    }
    let (Some(this), Some(desired)) = (executable(), desired()) else {
        CLASSIC_SAID.say_if_new(
            "BT_EXPLORER_CLASSIC not read: this copy's own program could not be named".to_owned(),
            &crate::diagnostics::stderr_line,
        );
        return None;
    };
    let run = reassert_in(
        &AccountClasses,
        &desired,
        &this,
        &what_stands,
        crate::update_trial::defer,
        &PRESSES,
    );
    Some(classic_member_answer(
        &run,
        &CLASSIC_SAID,
        &crate::diagnostics::stderr_line,
    ))
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
/// [`reassert_in`]'s reason: a second reading would be of a registry this function
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
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::attention_ownership::PathStands;

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
    /// The absolute path is the whole reason [`reassert_in`] exists, so the shape
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
    /// Asked of `bt-source` by identity: this module's product code calls the
    /// wrapper once, every call the product makes to the two writers is inside
    /// the body of [`apply`], and that body is the one that calls the wrapper.
    /// The suite's own words are test code and are not read.
    ///
    /// MUTATION: call `install_context_menu` or `remove_context_menu` outside
    /// the wrapper and this goes red.
    #[test]
    fn the_classic_registration_is_announced_to_the_shell_as_well() {
        let index = bt_source::Index::of_package("bt-app");
        let product_calls = |path: &str, scope: bt_source::Scope| {
            index
                .search(
                    &bt_source::Search::new(
                        bt_source::needle!(bt_source::Pattern::path(path)),
                        bt_source::View::Identifiers,
                    )
                    .in_scope(scope),
                )
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index)
                .spans()
        };
        let wrapped = product_calls(
            "bt_platform::changing_explorer_menu",
            bt_source::Scope::Modules(vec![bt_source::ModuleSpec::tree("crate::context_menu")]),
        );
        assert_eq!(
            wrapped.len(),
            1,
            "one wrapper, around the one function that writes"
        );
        let apply = index
            .one(&bt_source::ItemQuery::function("apply").in_module("crate::context_menu"))
            .unwrap_or_else(|failure| panic!("{failure}"));
        let body = apply.body().expect("`apply` has a body");
        assert!(
            wrapped[0].within(body),
            "the wrapper is called from `apply`"
        );
        for writer in [
            "bt_platform::install_context_menu",
            "bt_platform::remove_context_menu",
        ] {
            let calls = product_calls(writer, bt_source::Scope::Everything);
            assert_eq!(
                calls.len(),
                1,
                "{writer} is written once in this program, inside that wrapper"
            );
            assert!(
                calls[0].within(body),
                "{writer} is called from `apply`, where the wrapper is"
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

    // ── the start's repair (0.4.10 plan row 68b) ─────────────────────────────

    /// The label every shape in these tests is written with, unless it says otherwise.
    const LABEL: &str = "Open Folio here";

    /// A scratch folder under the build directory, removed with the test.
    fn scratch(tag: &str) -> crate::test_support::ScratchFolder {
        crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(tag)),
        )
    }

    /// A copy of Folio at `<root>/<folder>/folio.exe`.
    fn copy_at(root: &Path, folder: &str) -> PathBuf {
        let exe = root.join(folder).join("folio.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, folder.as_bytes()).unwrap();
        exe
    }

    /// What this build writes for `exe`.
    fn shape_of(exe: &Path) -> ContextMenuShape {
        bt_platform::context_menu_shape(exe, LABEL)
    }

    /// Both trees carrying `shape`.
    fn both(shape: &ContextMenuShape) -> Vec<ContextMenuTree> {
        vec![ContextMenuTree::Written(shape.clone()); 2]
    }

    /// **Trees held in memory**, counting the writes; a press can be made to land during the
    /// first write, before the repair's own write lands over it.
    struct Trees<'a> {
        trees: RefCell<Vec<ContextMenuTree>>,
        writes: Cell<usize>,
        refuse: Option<&'static str>,
        press_during_write: Cell<Option<(&'a ClassicPresses, bool)>>,
    }

    impl<'a> Trees<'a> {
        fn holding(trees: Vec<ContextMenuTree>) -> Self {
            Self {
                trees: RefCell::new(trees),
                writes: Cell::new(0),
                refuse: None,
                press_during_write: Cell::new(None),
            }
        }

        fn set(&self, shape: Option<&ContextMenuShape>) {
            *self.trees.borrow_mut() = match shape {
                Some(shape) => both(shape),
                None => vec![ContextMenuTree::Absent; 2],
            };
        }
    }

    impl ClassicStore for Trees<'_> {
        fn read_classic_trees(&self) -> Vec<ContextMenuTree> {
            self.trees.borrow().clone()
        }

        fn write_classic_verb(&self, shape: Option<&ContextMenuShape>) -> Result<(), String> {
            self.writes.set(self.writes.get() + 1);
            if let Some((presses, wish)) = self.press_during_write.take() {
                // The press records its wish, writes it, and the repair's write lands after.
                presses.note_press(wish);
                self.set(if wish { shape } else { None });
            }
            if let Some(refusal) = self.refuse {
                return Err(refusal.to_owned());
            }
            self.set(shape);
            Ok(())
        }
    }

    /// **The machine with the drive a program was on unplugged**: what stands is this
    /// machine's, and the way to a missing program's volume cannot be followed.
    struct ClassicAwayVolume;

    impl Look for ClassicAwayVolume {
        fn stands(&self, path: &Path) -> PathStands {
            what_stands(path)
        }

        fn volume_of(&self, _missing: &Path) -> Option<PathBuf> {
            None
        }
    }

    /// A gate that must not be asked: nothing is due.
    fn never_asked(writer: Writer) -> bool {
        panic!("the trial's gate is asked only when a write is due, not for {writer:?}")
    }

    /// RED (0.4.10 plan row 68b, the guard's `landed_by`) — **the carry pass reasserts the classic
    /// verb**, with the start-time scan's ownership rule, held during a trial, and says each
    /// finding once.
    ///
    /// Over trees of the test's own (every platform; the pass's own member answers nothing in a
    /// test process, so the account's menu is never read): a verb naming a `folio.exe` that is
    /// gone from a volume that is there is rewritten to this build's, and the row reads `On`; a
    /// verb naming another copy that is still there is left, and so is one naming a program on a
    /// drive that is away — not gone, only unplugged (R5); a machine with no verb gets none; during
    /// an update's trial the dead owner's verb is classified and held (`ExplorerRepair` asked, the
    /// row still `On`, nothing written — G6); and the line is said when the finding changes and
    /// not on a round that finds the same.
    ///
    /// MUTATION: write without asking the gate (`plant-68b-classic-unheld.py`) and the trial's
    /// case goes red; say the line every round (`plant-68b-classic-line-every-round.py`) and the
    /// last assertion goes red.
    #[test]
    fn the_carry_pass_reasserts_the_classic_verb() {
        let root = scratch("classic-carry");
        let here = copy_at(&root, "here");
        let other = copy_at(&root, "other");
        let gone = root.join("gone").join("folio.exe");
        let desired = shape_of(&here);
        let presses = ClassicPresses::new();

        let dead = Trees::holding(both(&shape_of(&gone)));
        let run = reassert_in(&dead, &desired, &here, &what_stands, |_| false, &presses);
        assert_eq!(run.found, ClassicFound::Dead);
        assert_eq!(run.repaired.as_deref(), Some("written"));
        assert_eq!(
            dead.read_classic_trees(),
            both(&desired),
            "rewritten to this build's"
        );
        assert!(run.answer.installed, "and the row reads On");

        for (what, trees, look, expected) in [
            (
                "another copy that is still there",
                both(&shape_of(&other)),
                &what_stands as &dyn Look,
                ClassicFound::AnotherCopy,
            ),
            (
                "a program on a drive that is away",
                both(&shape_of(&gone)),
                &ClassicAwayVolume as &dyn Look,
                ClassicFound::OwnerUnknown,
            ),
            (
                "no verb at all",
                vec![ContextMenuTree::Absent; 2],
                &what_stands as &dyn Look,
                ClassicFound::Absent,
            ),
        ] {
            let store = Trees::holding(trees.clone());
            let run = reassert_in(&store, &desired, &here, look, never_asked, &presses);
            assert_eq!(run.found, expected, "{what}");
            assert_eq!(run.repaired, None, "{what}: nothing is due");
            assert_eq!(store.writes.get(), 0, "{what}: nothing written");
            assert_eq!(store.read_classic_trees(), trees, "{what}: left as it was");
            assert_eq!(
                run.answer.installed,
                expected != ClassicFound::Absent,
                "{what}"
            );
        }

        let trial = Trees::holding(both(&shape_of(&gone)));
        let asked = Cell::new(None);
        let held = reassert_in(
            &trial,
            &desired,
            &here,
            &what_stands,
            |writer| {
                asked.set(Some(writer));
                true
            },
            &presses,
        );
        assert_eq!(
            asked.get(),
            Some(Writer::ExplorerRepair),
            "the gate is asked"
        );
        assert_eq!(held.found, ClassicFound::Dead, "the trial classifies");
        assert_eq!(held.repaired.as_deref(), Some("held"));
        assert_eq!(trial.writes.get(), 0, "and writes nothing until the commit");
        assert!(held.answer.installed, "the entry that is there is still On");
        assert_eq!(
            held.classic_line(),
            "BT_EXPLORER_CLASSIC dead repaired=held"
        );

        let said = LastSaid::new();
        let lines = RefCell::new(Vec::new());
        let say = |line: String| lines.borrow_mut().push(line);
        let store = Trees::holding(both(&shape_of(&gone)));
        let rounds = [0, 1, 2].map(|_| {
            let run = reassert_in(&store, &desired, &here, &what_stands, |_| false, &presses);
            classic_member_answer(&run, &said, &say)
        });
        assert_eq!(
            rounds
                .each_ref()
                .map(|answer| (answer.checks, answer.changed)),
            [(1, 1), (1, 1), (1, 0)],
            "one check a round, changed when the finding changed"
        );
        assert_eq!(
            *lines.borrow(),
            [
                "BT_EXPLORER_CLASSIC dead repaired=written",
                "BT_EXPLORER_CLASSIC current",
            ],
            "each finding said once, with no path"
        );
    }

    /// RED (0.4.10 plan row 68b; R-C's old-shape test for the classic verb) — **every shape of the
    /// verb a Folio has written, naming this copy, is brought to this build's**.
    ///
    /// The shapes in history: 0.1.0–0.2.x wrote `"<exe>" --cwd "%V"`; 0.3.0 on added
    /// `--from-explorer`, here in a language the reader has left; every build up to 0.4.7 named
    /// the program by `current_exe`, which for a copy started through winget's link is the link's
    /// path — a path that leads to this very file through a link (here a junction to this copy's
    /// folder), this copy's own and not a stranger's (G7); and a set somebody deleted half of.
    /// Each is read as this copy's old shape and rewritten to `context_menu_shape(this, label)`.
    ///
    /// Over trees of the test's own everywhere; on Windows also over an isolated class store of
    /// its own (`HKCU\Software\Folio-Test\<tag>`), where another program's verb beside Folio's
    /// under the same `shell` key is left byte for byte.
    ///
    /// MUTATION: compare the registered path with this copy's as two strings again
    /// (`plant-68b-alias-is-a-stranger.py`) and the link's shape reads as another copy and is
    /// left.
    #[test]
    fn every_historical_shape_of_the_classic_verb_is_brought_to_this_builds() {
        let root = scratch("classic-shapes");
        let here = copy_at(&root, "here");
        let link = root.join("alias");
        crate::test_support::link_folder_to(&link, here.parent().unwrap());
        let through_the_link = link.join("folio.exe");
        assert!(through_the_link.is_file(), "the link leads to this copy");
        let desired = shape_of(&here);
        let first = ContextMenuShape {
            label: LABEL.to_owned(),
            icon: format!("{},0", here.display()),
            command: format!("\"{}\" --cwd \"%V\"", here.display()),
        };
        let shapes = [
            ("0.1.0's command line", both(&first)),
            (
                "0.3.0's, in the other language",
                both(&bt_platform::context_menu_shape(&here, "在这里打开 Folio")),
            ),
            (
                "up to 0.4.7's, through winget's link",
                both(&shape_of(&through_the_link)),
            ),
            (
                "a set somebody deleted half of",
                vec![
                    ContextMenuTree::Written(first.clone()),
                    ContextMenuTree::Absent,
                ],
            ),
        ];
        for (what, trees) in &shapes {
            let store = Trees::holding(trees.clone());
            brought_to_this_builds(&store, &desired, &here, what);
        }
        #[cfg(windows)]
        {
            let hive = Hive::new("classic-shapes");
            let beside = hive.another_programs_verb();
            for (what, trees) in &shapes {
                hive.plant(trees);
                brought_to_this_builds(&hive, &desired, &here, what);
                assert_eq!(
                    hive.another_programs_verb(),
                    beside,
                    "{what}: the other program's verb is left byte for byte"
                );
            }
        }
    }

    /// One shape through the repair: this copy's old shape, written, and this build's after.
    fn brought_to_this_builds(
        store: &impl ClassicStore,
        desired: &ContextMenuShape,
        here: &Path,
        what: &str,
    ) {
        let run = reassert_in(
            store,
            desired,
            here,
            &what_stands,
            |_| false,
            &ClassicPresses::new(),
        );
        assert_eq!(run.found, ClassicFound::OldShape, "{what}: this copy's");
        assert_eq!(run.repaired.as_deref(), Some("written"), "{what}");
        assert_eq!(
            bt_platform::context_menu_verdict(&store.read_classic_trees(), desired),
            ContextMenuState::Current,
            "{what}: brought to this build's"
        );
        assert!(run.answer.installed, "{what}");
    }

    /// **A class store of this test's own** under `HKCU\Software\Folio-Test\<tag>`, written by
    /// the platform's own writers and read by its reader; `reg.exe` plants what no Folio writes
    /// (another program's verb, a deleted tree) and takes the whole key away however the test
    /// ends.
    #[cfg(windows)]
    struct Hive {
        classes: String,
    }

    #[cfg(windows)]
    impl Hive {
        fn new(tag: &str) -> Self {
            let classes = format!(r"Software\Folio-Test\{}", bt_testpath::unique_name(tag));
            let hive = Self { classes };
            hive.reg(&[
                "add",
                &hive.key(r"Directory\shell\Other"),
                "/ve",
                "/d",
                "Another program here",
                "/f",
            ]);
            hive.reg(&[
                "add",
                &hive.key(r"Directory\shell\Other\command"),
                "/ve",
                "/d",
                r"C:\Other\other.exe %V",
                "/f",
            ]);
            hive
        }

        fn key(&self, below: &str) -> String {
            format!(r"HKCU\{}\{below}", self.classes)
        }

        fn run(args: &[&str]) -> std::process::Output {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let reg = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows names it"))
                .join("System32")
                .join("reg.exe");
            hygiene
                .command(reg, bt_platform::quiet_command)
                .args(args)
                .output()
                .expect("reg.exe runs")
        }

        fn reg(&self, args: &[&str]) -> String {
            let ran = Self::run(args);
            assert!(
                ran.status.success(),
                "reg {args:?}: {}",
                String::from_utf8_lossy(&ran.stderr)
            );
            String::from_utf8_lossy(&ran.stdout).into_owned()
        }

        /// The other program's verb, every value under it, as `reg query` prints it.
        fn another_programs_verb(&self) -> String {
            self.reg(&["query", &self.key(r"Directory\shell\Other"), "/s"])
        }

        /// Write `trees` as a Folio once did: a shape into every tree, then the trees that are
        /// absent taken out.
        fn plant(&self, trees: &[ContextMenuTree]) {
            let shape = trees
                .iter()
                .find_map(ContextMenuTree::shape)
                .expect("a shape to plant");
            bt_platform::install_context_menu(&self.classes, shape).expect("the test's own store");
            for (tree, held) in bt_platform::CONTEXT_MENU_TREES.iter().zip(trees) {
                if *held == ContextMenuTree::Absent {
                    self.reg(&["delete", &self.key(&format!(r"{tree}\Folio")), "/f"]);
                }
            }
            assert_eq!(self.read_classic_trees(), trees, "planted as written");
        }
    }

    #[cfg(windows)]
    impl ClassicStore for Hive {
        fn read_classic_trees(&self) -> Vec<ContextMenuTree> {
            bt_platform::read_context_menu(&self.classes)
        }

        fn write_classic_verb(&self, shape: Option<&ContextMenuShape>) -> Result<(), String> {
            match shape {
                Some(shape) => bt_platform::install_context_menu(&self.classes, shape),
                None => bt_platform::remove_context_menu(&self.classes),
            }
        }
    }

    #[cfg(windows)]
    impl Drop for Hive {
        fn drop(&mut self) {
            let _ = Self::run(&["delete", &format!(r"HKCU\{}", self.classes), "/f"]);
        }
    }

    /// RED (0.4.10 plan row 68b, R4) — **a press during the classic repair wins**.
    ///
    /// The repair runs on the pass's worker and the switch on the window thread. A press that
    /// lands after the repair's read and before its write is not written over: the repair writes
    /// nothing and says `pressed`. A press that lands during the repair's write — its own write
    /// done before the repair's lands over it — has its wish written again after, so the store
    /// ends as the press asked.
    ///
    /// MUTATION: no second write after a press during the repair's
    /// (`plant-68b-classic-press-loses.py`) and the second case ends with the verb the person just
    /// took out back in their menu.
    #[test]
    fn a_press_during_the_classic_repair_wins() {
        let root = scratch("classic-press");
        let here = copy_at(&root, "here");
        let gone = root.join("gone").join("folio.exe");
        let desired = shape_of(&here);

        let presses = ClassicPresses::new();
        let before = Trees::holding(both(&shape_of(&gone)));
        let run = reassert_in(
            &before,
            &desired,
            &here,
            &what_stands,
            |_| {
                // The press lands while the gate is asked: after the read, before the write.
                presses.note_press(false);
                false
            },
            &presses,
        );
        assert_eq!(run.repaired.as_deref(), Some("pressed"));
        assert_eq!(
            before.writes.get(),
            0,
            "the repair does not write over a press"
        );
        assert_eq!(
            run.answer.presses_seen, 0,
            "the answer is the read before it"
        );

        let presses = ClassicPresses::new();
        let during = Trees::holding(both(&shape_of(&gone)));
        during.press_during_write.set(Some((&presses, false)));
        let run = reassert_in(&during, &desired, &here, &what_stands, |_| false, &presses);
        assert_eq!(run.repaired.as_deref(), Some("pressed"));
        assert_eq!(
            during.read_classic_trees(),
            vec![ContextMenuTree::Absent; 2],
            "the press's wish is what the store ends with"
        );
        assert!(
            !run.answer.installed,
            "read after the press's wish was written"
        );
        assert_eq!(
            run.answer.presses_seen, 0,
            "and still not the press's own read"
        );
    }

    /// RED (0.4.10 plan row 68b, R4) — **an answer read before the last press is not adopted**:
    /// the press read the machine after its own write, and that read is newer.
    ///
    /// MUTATION: adopt every answer (`plant-68b-stale-classic-adopted.py`) and a round that read
    /// the verb before the person took it out turns the switch back On.
    #[test]
    fn a_classic_answer_older_than_a_press_is_not_adopted() {
        let read_now = PRESSES.presses_counted();
        let mut installed = false;
        adopt_classic_answer(
            &mut installed,
            ClassicAnswer {
                installed: true,
                presses_seen: read_now,
            },
        );
        assert!(installed, "an answer read after every press is adopted");
        PRESSES.note_press(false);
        installed = false;
        adopt_classic_answer(
            &mut installed,
            ClassicAnswer {
                installed: true,
                presses_seen: read_now,
            },
        );
        assert!(
            !installed,
            "an answer read before the last press is dropped"
        );
    }
}
