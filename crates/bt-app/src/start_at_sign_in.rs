//! **Settings ▸ General ▸ Start at sign-in: one entry for the account that starts this copy of
//! Folio** (0.4.10 plan row 22a, T-START-AT-LOGON-SWITCH; the coordinator's rulings R1–R15 of
//! 2026-11-11, R4 split: this is the Windows half, the macOS half is row 22b).
//!
//! # What is written, and by whom
//!
//! One `REG_SZ` named `Folio` under the account's `Run` key whose text is the quoted path of this
//! copy's `folio.exe` and nothing after it — `bt_platform::sign_in_entry`, which owns the value's
//! shape and Windows' own Startup-apps switch beside it. **Off by default**: nothing writes the
//! entry but the switch's `On` ([`begin_sign_in_press`]), and nothing creates one that is not there.
//!
//! **The entry is the truth** (R5, the Explorer row's rule): nothing in `settings.json`. The row
//! shows what the machine holds, read off the window thread — by the carry pass's member at every
//! start and at each open of the General page ([`run_sign_in_step`]), and by the press's own
//! worker after its write. Until the first answer the row reads [`SignInRow::Unknown`], drawn Off.
//!
//! # Whose entry it is
//!
//! An entry of Folio's shape is the quoted path of a program whose file name is one Folio installs
//! itself under (`attention_ownership::folio_operand`). Which copy it names is the start-time scan's
//! rule ([`sign_in_entry_owner`]): **this copy** when the path leads to this very file, links followed
//! (`explorer_menu::same_path`) — exactly as written ([`SignInFound::Current`]) or by another
//! spelling, which is rewritten; **gone** when nothing stands at it on a volume that is there
//! (`attention_ownership::gone_by`), which is re-pointed at this copy (R7: the account asked for
//! Folio at sign-in, and a moved copy cannot be told from one deleted and fetched again); another
//! copy that is there, and one on a volume that is away, are left. Anything else under the name is
//! another program's and is never judged, written or removed ([`SignInFound::NotOurs`]).
//!
//! **The member writes only a repair, and only when it may** (R7, R11): held during an update's
//! trial ([`Writer::StartAtSignIn`], released by the commit's round), never over the system's own
//! switch turned off ([`SignInFound::TurnedOffBySystem`]: the person's choice in Task Manager), and
//! never over a press (R6): the press takes the entry's one latch on its worker and the member only
//! tries it, and an answer read before the last press is not adopted ([`adopt_sign_in_answer`]).
//!
//! **One line per finding, said once** (`BT_SIGN_IN …`, never a path): the member's
//! `BT_SIGN_IN <found>[ repaired=<written|held|pressed|refused (<stage>)>]`, and the press's
//! `BT_SIGN_IN press <on|off> <written|removed|unchanged|refused (<stage>)>`.
//!
//! **While this copy's entry is on, the restart registration leaves sign-in to it** (R10): Windows'
//! restart after an update's reboot or at a sign-in with "Restart apps" on would otherwise start a
//! second Folio beside the entry's, and a second plain start opens a window
//! ([`restart_flags_for`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, TryLockError};

use bt_platform::admission::WorkerCtx;
use bt_platform::logon_hook::Registry;
use bt_platform::sign_in_entry::{
    ACCOUNT_KEYS, CurrentUser, EntryFound, EntryKeys, EntryRead, EntryRefusal, enable_entry_in,
    read_entry_in, remove_entry_in, write_entry_in,
};

use crate::attention_ownership::{Look, folio_operand, gone_by, what_stands};
use crate::carry_pass::{LastSaid, MemberAnswer};
use crate::explorer_menu::same_path;
use crate::i18n::Text;
use crate::update_trial::Writer;

/// **What the row shows**: the entry as last read, by the carry pass or by a press's worker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SignInRow {
    /// Not read yet: drawn as Off, the classic row's footing.
    #[default]
    Unknown,
    /// No entry of Folio's, or one naming a program that is gone.
    Off,
    /// The entry starts this copy.
    On,
    /// The entry starts another copy of Folio — one that is there, or on a volume that is away.
    OnElsewhere,
    /// The entry is Folio's and Windows' Startup-apps switch has it off.
    TurnedOffBySystem,
    /// Another program's value is named `Folio`; it is left as it is.
    NotOurs,
}

impl SignInRow {
    /// Whether the switch reads `On`: the account's sign-in starts a Folio.
    pub(crate) const fn switch_on(self) -> bool {
        matches!(self, Self::On | Self::OnElsewhere)
    }

    /// The row's sentence (§7.1.6c-5 裁决八: statements, facts, at most two lines).
    pub(crate) const fn sign_in_description(self) -> Text {
        match self {
            Self::Unknown | Self::Off | Self::On => Text::DescStartAtSignIn,
            Self::OnElsewhere => Text::DescStartAtSignInElsewhere,
            Self::TurnedOffBySystem => Text::DescStartAtSignInTurnedOff,
            Self::NotOurs => Text::DescStartAtSignInNotOurs,
        }
    }
}

/// **The flags the restart registration is made with while the row reads `row`** (R10): the
/// ruled ones (`session_end::RESTART_FLAGS`, never after a crash or a hang) and, while this copy's
/// entry is on, no restart after the system's reboot either — the entry starts Folio at that
/// sign-in, and Windows' restart beside it would be a second plain start, which opens a window.
/// The Restart Manager's close (an installer) still restarts it. Whether the flag also covers
/// "Restart apps" at an ordinary sign-in is the clean-VM measurement M-W2's.
#[must_use]
pub(crate) const fn restart_flags_for(row: SignInRow) -> u32 {
    use bt_platform::session_end::{RESTART_FLAGS, RESTART_NO_REBOOT};
    match row {
        SignInRow::On => RESTART_FLAGS | RESTART_NO_REBOOT,
        SignInRow::Unknown
        | SignInRow::Off
        | SignInRow::OnElsewhere
        | SignInRow::TurnedOffBySystem
        | SignInRow::NotOurs => RESTART_FLAGS,
    }
}

/// **Whose entry it is**, the system's switch aside — the uninstall's question as well as the
/// member's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryOwner {
    /// No value of the name.
    Absent,
    /// This copy: `exact` when spelled exactly as this build writes it.
    ThisCopy { exact: bool },
    /// A Folio-shaped entry naming a program missing on a volume that is there.
    Dead,
    /// A Folio-shaped entry naming another copy that is there.
    AnotherCopy(PathBuf),
    /// A Folio-shaped entry naming a program the machine will not say about: a volume that is
    /// away among them.
    OwnerUnknown(PathBuf),
    /// Anything else under the name: another program's.
    NotOurs,
}

/// **Whose entry `found` is, against this copy's `this`** — see the module header.
pub(crate) fn sign_in_entry_owner(
    found: &EntryFound,
    this: &Path,
    look: &(impl Look + ?Sized),
) -> EntryOwner {
    let program = match found {
        EntryFound::Absent => return EntryOwner::Absent,
        EntryFound::Other => return EntryOwner::NotOurs,
        EntryFound::Command(program) => program,
    };
    if program.to_str().and_then(folio_operand).is_none() {
        return EntryOwner::NotOurs;
    }
    if program.as_os_str() == this.as_os_str() {
        return EntryOwner::ThisCopy { exact: true };
    }
    if same_path(program, this) {
        return EntryOwner::ThisCopy { exact: false };
    }
    match gone_by(program, look) {
        Ok(true) => EntryOwner::Dead,
        Ok(false) => EntryOwner::AnotherCopy(program.clone()),
        Err(_) => EntryOwner::OwnerUnknown(program.clone()),
    }
}

/// **What a reading of the entry found** — the member's word for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignInFound {
    /// No entry: nothing is created.
    Absent,
    /// This copy, exactly as this build writes it.
    Current,
    /// This copy by another spelling: rewritten.
    ThisCopyOtherSpelling,
    /// A program that is gone: re-pointed at this copy.
    Dead,
    /// Another copy that is there: left.
    AnotherCopy,
    /// A program the machine will not say about: left, it comes back with the drive.
    OwnerUnknown,
    /// Another program's value under the name: never judged.
    NotOurs,
    /// Folio's entry, turned off in Startup apps: the person's choice, nothing written.
    TurnedOffBySystem,
    /// The registry refused the reading.
    Unread,
}

impl SignInFound {
    /// A reading's finding: the owner, then the system's switch over an entry of Folio's.
    pub(crate) fn of_reading(read: &EntryRead, this: &Path, look: &(impl Look + ?Sized)) -> Self {
        let owner = sign_in_entry_owner(&read.found, this, look);
        let folios = !matches!(owner, EntryOwner::Absent | EntryOwner::NotOurs);
        if folios && read.turned_off {
            return Self::TurnedOffBySystem;
        }
        match owner {
            EntryOwner::Absent => Self::Absent,
            EntryOwner::ThisCopy { exact: true } => Self::Current,
            EntryOwner::ThisCopy { exact: false } => Self::ThisCopyOtherSpelling,
            EntryOwner::Dead => Self::Dead,
            EntryOwner::AnotherCopy(_) => Self::AnotherCopy,
            EntryOwner::OwnerUnknown(_) => Self::OwnerUnknown,
            EntryOwner::NotOurs => Self::NotOurs,
        }
    }

    const fn sign_in_found_word(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Current => "current",
            Self::ThisCopyOtherSpelling => "this copy (another spelling)",
            Self::Dead => "dead",
            Self::AnotherCopy => "another copy",
            Self::OwnerUnknown => "owner unknown",
            Self::NotOurs => "not ours",
            Self::TurnedOffBySystem => "turned off in Startup apps",
            Self::Unread => "unread",
        }
    }

    /// Whether the member rewrites what it found (R7).
    const fn sign_in_repair_wanted(self) -> bool {
        matches!(self, Self::ThisCopyOtherSpelling | Self::Dead)
    }

    /// What the row shows over this finding.
    pub(crate) const fn row_shown(self) -> SignInRow {
        match self {
            Self::Absent | Self::Dead => SignInRow::Off,
            Self::Current | Self::ThisCopyOtherSpelling => SignInRow::On,
            Self::AnotherCopy | Self::OwnerUnknown => SignInRow::OnElsewhere,
            Self::NotOurs => SignInRow::NotOurs,
            Self::TurnedOffBySystem => SignInRow::TurnedOffBySystem,
            Self::Unread => SignInRow::Unknown,
        }
    }
}

/// The row over a fresh reading of the entry; `Unknown` when the registry refused it.
fn row_now(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
    this: &Path,
    look: &(impl Look + ?Sized),
) -> SignInRow {
    read_entry_in(registry, keys).map_or(SignInRow::Unknown, |read| {
        SignInFound::of_reading(&read, this, look).row_shown()
    })
}

/// **The presses of the switch, as the member asks about them** (R6, the classic verb's
/// `ClassicPresses` shape): the press records its wish and counts itself on the window thread
/// before its worker starts; the member writes only if no press landed since its read, and an
/// answer read before the last press is not adopted. The wish is stored before the count moves,
/// so whoever sees the count move reads that press's wish.
pub(crate) struct SignInPresses {
    count: AtomicU64,
    wish: AtomicBool,
}

impl SignInPresses {
    pub(crate) const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            wish: AtomicBool::new(false),
        }
    }

    /// A press asking for the entry (`true`) or for its removal.
    pub(crate) fn note_sign_in_press(&self, on: bool) {
        self.wish.store(on, Ordering::SeqCst);
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    /// How many presses have landed.
    pub(crate) fn sign_in_presses_counted(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    /// The last press's wish.
    fn sign_in_wish(&self) -> bool {
        self.wish.load(Ordering::SeqCst)
    }
}

/// This process's presses.
pub(crate) static SIGN_IN_PRESSES: SignInPresses = SignInPresses::new();

/// **The entry's one write latch** (R6): a press's worker waits for it, the member only tries it.
pub(crate) static SIGN_IN_WRITES: Mutex<()> = Mutex::new(());

/// **What the window takes for the row**: the row after the reading, and how many presses had
/// landed when it was read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SignInAnswer {
    pub(crate) row: SignInRow,
    pub(crate) presses_seen: u64,
}

/// **The window takes an answer for the row** — unless a press has landed since it was read: the
/// press's own answer is newer (68b's R4 rule). `presses` is the process's [`SIGN_IN_PRESSES`] in
/// the product, and a test's own in the suite. Whether the row changed.
pub(crate) fn adopt_sign_in_answer(
    row: &mut SignInRow,
    answer: SignInAnswer,
    presses: &SignInPresses,
) -> bool {
    if answer.presses_seen != presses.sign_in_presses_counted() || *row == answer.row {
        return false;
    }
    *row = answer.row;
    true
}

/// **One run of the member**: what it found, what it did, and the window's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SignInRun {
    pub(crate) found: SignInFound,
    /// `None` when nothing was due; otherwise `written`, `held`, `pressed` or
    /// `refused (<stage>)`.
    pub(crate) repaired: Option<String>,
    pub(crate) answer: SignInAnswer,
}

impl SignInRun {
    /// `BT_SIGN_IN <found>[ repaired=<what>]` — no path.
    pub(crate) fn sign_in_line(&self) -> String {
        match &self.repaired {
            Some(repaired) => format!(
                "BT_SIGN_IN {} repaired={repaired}",
                self.found.sign_in_found_word()
            ),
            None => format!("BT_SIGN_IN {}", self.found.sign_in_found_word()),
        }
    }
}

/// **The carry pass's repair of the entry** — see the module header.
///
/// One reading, classified ([`SignInFound::of`]); the write only when the finding is this copy by
/// another spelling or a program that is gone, and then **only after the trial's gate has said
/// yes** (`defer` answers `true` to hold it), **only while no press holds the entry's latch**
/// (`writes`, tried and never waited for), and **only if no press has landed since the reading**.
/// The answer's row comes off a reading after the write. A refused write is said and left.
pub(crate) fn carry_sign_in_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
    this: &Path,
    look: &(impl Look + ?Sized),
    defer: impl FnOnce(Writer) -> bool,
    presses: &SignInPresses,
    writes: &Mutex<()>,
) -> SignInRun {
    let seen = presses.sign_in_presses_counted();
    let run = |found: SignInFound, repaired: Option<String>, row: SignInRow| SignInRun {
        found,
        repaired,
        answer: SignInAnswer {
            row,
            presses_seen: seen,
        },
    };
    let read = match read_entry_in(registry, keys) {
        Ok(read) => read,
        Err(refusal) => {
            let refused = format!("refused ({})", refusal.stage_said());
            return run(SignInFound::Unread, Some(refused), SignInRow::Unknown);
        }
    };
    let found = SignInFound::of_reading(&read, this, look);
    if !found.sign_in_repair_wanted() {
        return run(found, None, found.row_shown());
    }
    if defer(Writer::StartAtSignIn) {
        return run(found, Some("held".to_owned()), found.row_shown());
    }
    let _latch = match writes.try_lock() {
        Ok(latch) => latch,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => {
            return run(found, Some("pressed".to_owned()), found.row_shown());
        }
    };
    if presses.sign_in_presses_counted() != seen {
        return run(found, Some("pressed".to_owned()), found.row_shown());
    }
    let repaired = match write_entry_in(registry, keys, this) {
        Ok(()) => "written".to_owned(),
        Err(refusal) => format!("refused ({})", refusal.stage_said()),
    };
    run(found, Some(repaired), row_now(registry, keys, this, look))
}

/// The member's last line.
static SIGN_IN_SAID: LastSaid = LastSaid::new();

/// **A run as the pass's part**: one check, changed when its line differs from the last said in
/// this process.
pub(crate) fn sign_in_member_answer(
    run: &SignInRun,
    said: &LastSaid,
    say: &dyn Fn(String),
) -> MemberAnswer<SignInAnswer> {
    let changed = said.say_if_new(run.sign_in_line(), say);
    MemberAnswer {
        part: run.answer,
        checks: 1,
        changed: usize::from(changed),
    }
}

/// **The entry, as a member of the start's carry pass** ([`crate::carry_pass::run_carry_steps`]):
/// the account's `Run` key, this copy's file, this machine's disk, the trial's gate and this
/// process's presses.
///
/// Nothing off Windows (the macOS half is row 22b); nothing in a test process, whose pass must
/// never read or write the account's sign-in entry — the tests drive [`carry_sign_in_in`] over a
/// registry of their own. Nothing, and one line, when this copy's own program cannot be named
/// as a place an entry may point at (`attention_ownership::stable_executable`).
pub(crate) fn run_sign_in_step(_ctx: &WorkerCtx) -> Option<MemberAnswer<SignInAnswer>> {
    if bt_platform::host_platform() != bt_platform::HostPlatform::Windows || cfg!(test) {
        return None;
    }
    let this = crate::attention_ownership::this_copy();
    let this = match crate::attention_ownership::stable_executable(this.as_deref()) {
        Ok(this) => this.to_path_buf(),
        Err(reason) => {
            SIGN_IN_SAID.say_if_new(
                format!("BT_SIGN_IN not read: {reason}"),
                &crate::diagnostics::stderr_line,
            );
            return None;
        }
    };
    let run = carry_sign_in_in(
        &mut CurrentUser,
        ACCOUNT_KEYS,
        &this,
        &what_stands,
        crate::update_trial::defer,
        &SIGN_IN_PRESSES,
        &SIGN_IN_WRITES,
    );
    Some(sign_in_member_answer(
        &run,
        &SIGN_IN_SAID,
        &crate::diagnostics::stderr_line,
    ))
}

/// **A press's answer**: what it was asked (the last press's wish when its worker took the latch),
/// whether it did it or the sentence it refused with, the row after it, and how many presses had
/// landed when it took the latch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SignInPressed {
    pub(crate) on: bool,
    pub(crate) outcome: Result<(), String>,
    /// `None` when the entry was not read: the row keeps what it said.
    pub(crate) answer: Option<SignInAnswer>,
    pub(crate) presses_seen: u64,
}

/// A refusal of the door's, as the press's card says it.
fn refusal_reason(refusal: &EntryRefusal) -> String {
    match refusal {
        EntryRefusal::TooLong { .. } => Text::SignInPathTooLong.text().to_owned(),
        EntryRefusal::Failed { .. } | EntryRefusal::ReadBackDiffers => refusal.to_string(),
    }
}

/// **The switch's press, on its worker, under the entry's latch** (R6, R8, R9).
///
/// The press taken is the last one counted when the latch is held ([`SignInPresses`]'s wish), so a
/// burst of presses ends where the last one asked. **On** refuses over another program's value under
/// the name and writes nothing; otherwise it points the entry at this copy — over any Folio's entry,
/// another live copy's too (R8: the press is the account's) — reads it back, and takes the system's
/// switch off the name (R9). **Off** removes the entry when it is Folio's, whichever copy it names,
/// with the system's switch of the name; another program's value is left. Not held during an
/// update's trial: it is a person's, as the hooks' presses are. The row comes off a reading after.
pub(crate) fn press_sign_in_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
    this: Result<&Path, &'static str>,
    look: &(impl Look + ?Sized),
    presses: &SignInPresses,
    writes: &Mutex<()>,
) -> SignInPressed {
    let _latch = writes.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = presses.sign_in_presses_counted();
    let on = presses.sign_in_wish();
    let pressed = |outcome: Result<(), String>, answer: Option<SignInAnswer>| SignInPressed {
        on,
        outcome,
        answer,
        presses_seen: seen,
    };
    let this = match this {
        Ok(this) => this,
        Err(reason) => return pressed(Err(reason.to_owned()), None),
    };
    let outcome = match read_entry_in(registry, keys) {
        Err(refusal) => Err(refusal_reason(&refusal)),
        Ok(read) => {
            let folios = !matches!(
                sign_in_entry_owner(&read.found, this, look),
                EntryOwner::Absent | EntryOwner::NotOurs
            );
            let foreign = !folios && read.found != EntryFound::Absent;
            if on && foreign {
                Err(Text::SignInEntryNotOurs.text().to_owned())
            } else if on {
                write_entry_in(registry, keys, this)
                    .and_then(|()| enable_entry_in(registry, keys).map(|_| ()))
                    .map_err(|refusal| refusal_reason(&refusal))
            } else if folios {
                remove_entry_in(registry, keys)
                    .map(|_| ())
                    .map_err(|refusal| refusal_reason(&refusal))
            } else {
                Ok(())
            }
        }
    };
    let answer = SignInAnswer {
        row: row_now(registry, keys, this, look),
        presses_seen: seen,
    };
    pressed(outcome, Some(answer))
}

impl SignInPressed {
    /// `BT_SIGN_IN press <on|off> <written|removed|unchanged|refused>` — no path, no sentence.
    pub(crate) fn press_line(&self) -> String {
        let asked = if self.on { "on" } else { "off" };
        let did = match (&self.outcome, self.answer.map(|answer| answer.row)) {
            (Err(_), _) => "refused",
            (Ok(()), Some(SignInRow::On)) if self.on => "written",
            (Ok(()), Some(SignInRow::Off)) if !self.on => "removed",
            (Ok(()), _) => "unchanged",
        };
        format!("BT_SIGN_IN press {asked} {did}")
    }

    /// The card the press is answered with: a short line, or the refusal's sentence.
    pub(crate) fn press_card(&self) -> (bool, String) {
        match &self.outcome {
            Ok(()) if self.on => (true, Text::SignInOnToast.text().to_owned()),
            Ok(()) => (true, Text::SignInOffToast.text().to_owned()),
            Err(reason) => (false, crate::i18n::sign_in_not_changed(reason)),
        }
    }
}

/// The newest press's answer, until the window takes it.
static SIGN_IN_PRESSED: Mutex<Option<SignInPressed>> = Mutex::new(None);

/// How a press's answer asks for a frame — installed once, at startup ([`install_sign_in_wake`]).
static SIGN_IN_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the wake, once.
pub(crate) fn install_sign_in_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = SIGN_IN_WAKE.set(Box::new(wake));
}

/// Put a press's answer where the window takes it — unless a newer one is already there — say its
/// line, and wake the window.
fn publish_sign_in_press(pressed: SignInPressed) {
    crate::diagnostics::stderr_line(pressed.press_line());
    {
        let mut slot = SIGN_IN_PRESSED
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot
            .as_ref()
            .is_none_or(|held| held.presses_seen <= pressed.presses_seen)
        {
            *slot = Some(pressed);
        }
    }
    if let Some(wake) = SIGN_IN_WAKE.get() {
        wake();
    }
}

/// **The window's take of a press's answer**, once.
pub(crate) fn take_sign_in_press() -> Option<SignInPressed> {
    SIGN_IN_PRESSED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// **Press the switch** (`Settings ▸ General ▸ Start at sign-in`): counted here, on the window
/// thread, then done on a worker of its own, `folio-sign-in-press` (R6) — the window thread makes no
/// registry call. Its answer comes back through [`take_sign_in_press`] and the wake.
///
/// A start the thread door refuses, and a worker that panics, are answered as the press's failure
/// in [`crate::i18n::unfinished_change`]'s words. In a test process the worker touches nothing: a
/// test's press never reads or writes the account's sign-in entry; the tests drive
/// [`press_sign_in_in`] over a registry of their own.
pub(crate) fn begin_sign_in_press(on: bool) {
    SIGN_IN_PRESSES.note_sign_in_press(on);
    let _ = crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal).start_worker(
        "folio-sign-in-press",
        crate::lane::Answered::new("the sign-in entry's press", crate::diagnostics::note, {
            move |end: &crate::lane::WorkerEnd| {
                publish_sign_in_press(SignInPressed {
                    on,
                    outcome: Err(crate::i18n::unfinished_change(end.started())
                        .text()
                        .to_owned()),
                    answer: None,
                    presses_seen: SIGN_IN_PRESSES.sign_in_presses_counted(),
                });
            }
        }),
        move |_ctx| {
            if cfg!(test) {
                return;
            }
            let this = crate::attention_ownership::this_copy();
            let pressed = press_sign_in_in(
                &mut CurrentUser,
                ACCOUNT_KEYS,
                crate::attention_ownership::stable_executable(this.as_deref()),
                &what_stands,
                &SIGN_IN_PRESSES,
                &SIGN_IN_WRITES,
            );
            publish_sign_in_press(pressed);
        },
    );
}

/// **What the uninstall did with the entry.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SignInRemoval {
    /// No entry under the name.
    Absent,
    /// This copy's, or a gone program's: removed, with the system's switch of the name.
    Removed,
    /// Another copy's, there or on a volume that is away: left, and said.
    LeftAnotherCopy(PathBuf),
    /// Another program's value under the name: left as it is.
    LeftNotOurs,
    /// The registry refused, in its own words.
    Refused(String),
}

/// **Both uninstall verbs' removal of the entry** (`uninstall::sign_in_entries`, on the door's
/// worker): removed when it names this copy `exe` or a program that is gone, the system's switch
/// with it; another copy's is left; another program's value is never judged. The system's switch
/// does not decide: an entry turned off in Startup apps is still Folio's to take away.
pub(crate) fn uninstall_sign_in_in(
    registry: &mut impl Registry,
    keys: EntryKeys<'_>,
    exe: &Path,
    look: &(impl Look + ?Sized),
) -> SignInRemoval {
    let read = match read_entry_in(registry, keys) {
        Ok(read) => read,
        Err(refusal) => return SignInRemoval::Refused(refusal.to_string()),
    };
    match sign_in_entry_owner(&read.found, exe, look) {
        EntryOwner::Absent => SignInRemoval::Absent,
        EntryOwner::NotOurs => SignInRemoval::LeftNotOurs,
        EntryOwner::AnotherCopy(program) | EntryOwner::OwnerUnknown(program) => {
            SignInRemoval::LeftAnotherCopy(program)
        }
        EntryOwner::ThisCopy { .. } | EntryOwner::Dead => match remove_entry_in(registry, keys) {
            Ok(_) => SignInRemoval::Removed,
            Err(refusal) => SignInRemoval::Refused(refusal.to_string()),
        },
    }
}

/// **The uninstall's removal over the account's own entry** — what `uninstall::system_remove`
/// hands its row on Windows.
pub(crate) fn uninstall_sign_in(exe: &Path) -> SignInRemoval {
    uninstall_sign_in_in(&mut CurrentUser, ACCOUNT_KEYS, exe, &what_stands)
}

#[cfg(test)]
#[path = "start_at_sign_in_tests.rs"]
mod tests;
