//! **The seven runtime pins of a pane's shell life** — the ownership contract's (b)2, (c)2 and (c)3
//! (`docs/plans/design/ownership-contract-2026-10-11.md`; ticket O-4b, ruling D13).
//!
//! The census pins in `bt-source` prove who may move a [`ShellLife`]'s phase; these prove what a
//! move does: each transition takes exactly the edges its doc names, the reaper takes them where
//! today's behaviour takes them, and a *Restart shell* replaces the pane's whole record from any
//! phase. Fixtures enter a phase through the product's own transitions (D17), with fake shells
//! (`PtySession::fake_exited`) and births the worker answers with one.

use crate::headless::HeadlessFolio;
use crate::lane::Hold;
use crate::test_support::{self, leaf_saying, tab_holding};
use crate::{
    BirthDeathFallback, BirthDecision, BirthWait, LeafSeed, PaneBirth, Phase, ShellLanding,
    ShellLife, Started, TerminalPane, pty_door,
};
use bt_pty::PtySession;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The nine phases, Deciding and Bearing told apart by what their birth waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Name {
    Shellless,
    Deciding,
    Bearing,
    Live,
    ExitDraining,
    Exited,
    FallingBack,
    Kept,
    Released,
}

const EVERY: [Name; 9] = [
    Name::Shellless,
    Name::Deciding,
    Name::Bearing,
    Name::Live,
    Name::ExitDraining,
    Name::Exited,
    Name::FallingBack,
    Name::Kept,
    Name::Released,
];

fn name_of(life: &ShellLife) -> Name {
    match &life.phase {
        Phase::Shellless => Name::Shellless,
        Phase::InBirth(PaneBirth {
            waiting: BirthWait::Shell(_),
            ..
        }) => Name::Bearing,
        Phase::InBirth(_) => Name::Deciding,
        Phase::Live(_) => Name::Live,
        Phase::ExitDraining { .. } => Name::ExitDraining,
        Phase::Exited { .. } => Name::Exited,
        Phase::FallingBack(_) => Name::FallingBack,
        Phase::Kept => Name::Kept,
        Phase::Released => Name::Released,
    }
}

/// A shell that has exited, `age` after its start, having written `output`.
fn fake(age: Duration, output: &[u8], has_birth_fallback: bool) -> PtySession {
    PtySession::fake_exited(age, 23, output, has_birth_fallback)
}

fn waking() -> (bt_pty::OutputWake, mpsc::Receiver<()>) {
    let (said, heard) = mpsc::sync_channel(8);
    (
        Arc::new(move || {
            let _ = said.try_send(());
        }),
        heard,
    )
}

/// A shell asked of `bt-pty-birth`, whose worker answers with a fake shell once it has passed
/// `gate`: at once when the gate is not held, else at its release, or at the hold's cap, which
/// makes the release red (a call waited on the held birth).
fn asked(gate: Arc<Hold>) -> (pty_door::ShellBirth, mpsc::Receiver<()>) {
    let (wake, heard) = waking();
    let shell = pty_door::request(
        move |_| {
            gate.pass();
            Ok::<_, bt_pty::PtyError>(fake(
                Duration::from_secs(60),
                "第一 first".as_bytes(),
                false,
            ))
        },
        wake,
    )
    .expect("the birth worker starts");
    (shell, heard)
}

/// A shell asked of `bt-pty-birth` and already answered.
fn answered() -> pty_door::ShellBirth {
    let (shell, heard) = asked(Arc::new(Hold::default()));
    while !shell.answered() {
        crate::lane::receive(&heard, "the birth's wake");
    }
    shell
}

fn decision() -> BirthDecision {
    BirthDecision {
        started: Started::AsAsked,
        spawn_profile: crate::profiles::fallback_profile_id().to_owned(),
        spawn_place: None,
        at_shell_home: false,
        named: false,
        program: Some(PathBuf::from("壳 shell")),
        program_kind: crate::profiles::ShellProgram::Native,
        unless_gone: None,
    }
}

fn grid() -> crate::GridSize {
    crate::GridSize {
        columns: NonZeroU16::new(80).expect("nonzero"),
        rows: NonZeroU16::new(24).expect("nonzero"),
    }
}

fn landing(shell: pty_door::ShellBirth) -> ShellLanding {
    ShellLanding {
        shell,
        decision: decision(),
        born_grid: grid(),
        owed_physical: None,
    }
}

fn birth(waiting: BirthWait) -> PaneBirth {
    PaneBirth {
        seed: LeafSeed::default(),
        probe_input: None,
        typed: std::cell::RefCell::default(),
        waiting,
    }
}

fn pending(shell: pty_door::ShellBirth) -> BirthDeathFallback {
    BirthDeathFallback {
        landing: landing(shell),
        exit: crate::exit_diagnostics::ShellExit {
            seat: 1,
            code: Some(23),
            elapsed: Duration::from_millis(400),
            program: Some(PathBuf::from("壳 shell")),
            tab: 1,
            disposition: crate::exit_diagnostics::ShellExitDisposition::BirthDeathFellBack(
                bt_pty::LAST_RESORT_SHELL,
            ),
        },
    }
}

/// A shell life in `name`, entered through the product's transitions.
fn life_in(name: Name) -> ShellLife {
    let mut life = ShellLife::shellless();
    let live = |life: &mut ShellLife| life.land(Some(fake(Duration::from_secs(60), b"", false)));
    match name {
        Name::Shellless => {}
        Name::Deciding => life.begin_birth(birth(BirthWait::Locale)),
        Name::Bearing => life.begin_birth(birth(BirthWait::Shell(landing(answered())))),
        Name::Live => live(&mut life),
        Name::ExitDraining => {
            live(&mut life);
            life.exit_draining(Duration::from_millis(400));
        }
        Name::Exited => {
            live(&mut life);
            assert!(life.exited(Duration::from_secs(60)));
        }
        Name::FallingBack => life.fall_back(pending(answered())),
        Name::Kept => life.keep_birth_death(),
        Name::Released => {
            live(&mut life);
            assert!(life.retire().is_some());
        }
    }
    assert_eq!(name_of(&life), name, "the fixture enters its phase");
    life
}

/// One transition, driven: what it answered (`true` when it handed something back or said a
/// line is owed).
type Drive = fn(&mut ShellLife) -> bool;

const TRANSITIONS: [(&str, Drive); 11] = [
    ("begin_birth", |life| {
        life.begin_birth(birth(BirthWait::Locale));
        false
    }),
    ("take_birth", |life| life.take_birth().is_some()),
    ("land a shell", |life| {
        life.land(Some(fake(Duration::from_secs(60), b"", false)));
        false
    }),
    ("land none", |life| {
        life.land(None);
        false
    }),
    ("exit_draining", |life| {
        life.exit_draining(Duration::from_millis(400));
        false
    }),
    ("exited", |life| life.exited(Duration::from_secs(60))),
    ("release_dead_shell", |life| {
        life.release_dead_shell().is_some()
    }),
    ("fall_back", |life| {
        life.fall_back(pending(answered()));
        false
    }),
    ("take_fallback", |life| life.take_fallback().is_some()),
    ("keep_birth_death", |life| {
        life.keep_birth_death();
        false
    }),
    ("retire", |life| life.retire().is_some()),
];

/// The transitions that bring a payload into the pane. From a phase with a shell, a debug build
/// refuses them rather than drop the payload; a release build leaves the phase as it was.
const ENTRIES: [&str; 4] = ["begin_birth", "land a shell", "land none", "fall_back"];

/// The listed edges: (transition, from, to, answered). Every other pair but the refused
/// [`ENTRIES`] leaves the phase as it was and answers nothing.
const EDGES: [(&str, Name, Name, bool); 20] = [
    ("begin_birth", Name::Shellless, Name::Deciding, false),
    ("begin_birth", Name::Released, Name::Deciding, false),
    ("take_birth", Name::Deciding, Name::Shellless, true),
    ("take_birth", Name::Bearing, Name::Shellless, true),
    ("land a shell", Name::Shellless, Name::Live, false),
    ("land a shell", Name::Released, Name::Live, false),
    ("exit_draining", Name::Live, Name::ExitDraining, false),
    ("exited", Name::Live, Name::Exited, true),
    ("release_dead_shell", Name::Live, Name::Released, true),
    (
        "release_dead_shell",
        Name::ExitDraining,
        Name::Released,
        true,
    ),
    ("fall_back", Name::Shellless, Name::FallingBack, false),
    ("fall_back", Name::Released, Name::FallingBack, false),
    ("take_fallback", Name::FallingBack, Name::Released, true),
    ("keep_birth_death", Name::Shellless, Name::Kept, false),
    ("keep_birth_death", Name::Released, Name::Kept, false),
    ("retire", Name::Live, Name::Released, true),
    ("retire", Name::ExitDraining, Name::Released, true),
    ("retire", Name::Exited, Name::Released, true),
    // `exited` on an Exited shell: the line was said on the edge, so none is owed again.
    ("exited", Name::Exited, Name::Exited, false),
    ("land none", Name::Released, Name::Released, false),
];

/// What a phase answers: its pseudoconsole, its birth, its last-resort birth, its exit's age,
/// whether it keeps its tab, whether it has no shell.
fn reads(life: &ShellLife) -> [bool; 6] {
    [
        life.pty().is_some(),
        life.birth().is_some(),
        life.falling_back().is_some(),
        life.exit_age().is_some(),
        life.holds_its_tab(),
        life.has_no_shell(),
    ]
}

fn expected_reads(name: Name) -> [bool; 6] {
    match name {
        Name::Shellless | Name::Released => [false, false, false, false, false, true],
        Name::Deciding | Name::Bearing => [false, true, false, false, true, false],
        Name::Live => [true, false, false, false, false, false],
        Name::ExitDraining | Name::Exited => [true, false, false, true, false, false],
        Name::FallingBack => [false, false, true, false, true, false],
        Name::Kept => [false, false, false, false, true, false],
    }
}

/// RED (O-4b; the contract's (c)2) — **every life transition is one method and no other**: over
/// the nine phases, each transition takes exactly its listed edges, an entry transition is refused
/// (a debug build's panic) from a phase with a shell, and every other pair leaves the phase as it
/// was, answering nothing; each phase answers its reads; the owed size of a resize lands on the
/// pending landing of Bearing or FallingBack and nowhere else. Then the reaper, over a background
/// tab: an exit after the grace is Exited with its shell kept; a birth death whose reader still
/// holds bytes is ExitDraining and keeps its tab, and once drained (it had output) is Kept; a pane
/// in birth keeps the tab although it has no shell.
///
/// MUTATION: let `keep_birth_death` enter Kept from any phase — the table names it from Live;
/// skip the reaper's drained hold (drop `if !pty.output_is_drained() { .. }`) — the draining pane
/// is Kept on the first pass; drop `Phase::Kept` from `holds_its_tab` — the Kept row's reads
/// differ; drop the `debug_assert!` from `begin_birth`, `land` or `fall_back` — its call from
/// Live is not refused.
#[test]
fn every_life_transition_is_one_method_and_no_other() {
    test_support::on_the_window_thread();
    for from in EVERY {
        assert_eq!(
            reads(&life_in(from)),
            expected_reads(from),
            "{from:?} answers its reads"
        );
        for (transition, drive) in TRANSITIONS {
            let mut life = life_in(from);
            if ENTRIES.contains(&transition) && !matches!(from, Name::Shellless | Name::Released) {
                let refused =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drive(&mut life)));
                assert!(refused.is_err(), "`{transition}` from {from:?} is refused");
                continue;
            }
            let answer = drive(&mut life);
            let (to, answers) = EDGES
                .iter()
                .find(|(name, edge_from, _, _)| *name == transition && *edge_from == from)
                .map_or((from, false), |(_, _, to, answers)| (*to, *answers));
            assert_eq!(
                (name_of(&life), answer),
                (to, answers),
                "`{transition}` from {from:?}"
            );
        }
        let mut life = life_in(from);
        let physical = winit::dpi::PhysicalSize::new(777, 333);
        life.owe_landing_size(physical);
        assert_eq!(name_of(&life), from, "an owed size moves no phase");
        let owed = match &life.phase {
            Phase::InBirth(PaneBirth {
                waiting: BirthWait::Shell(landing),
                ..
            }) => landing.owed_physical,
            Phase::FallingBack(pending) => pending.landing.owed_physical,
            _ => None,
        };
        assert_eq!(
            owed.is_some(),
            matches!(from, Name::Bearing | Name::FallingBack),
            "{from:?} is owed the size only when a landing is pending"
        );
    }

    // The reaper, over a tab behind the one on screen, so no pane of it is closed as a pane.
    let mut tab = crate::test_support::cross_tab(2, &["终了 ended", "未尽 draining", "诞生 born"]);
    let seats = tab.seats.terminals();
    let (ended, draining, born) = (seats[0], seats[1], seats[2]);
    fn leaf(tab: &mut crate::TabState, seat: crate::SeatId) -> &mut TerminalPane {
        tab.sessions.get_mut(&seat).expect("the pane")
    }
    leaf(&mut tab, ended)
        .session
        .life
        .land(Some(fake(Duration::from_secs(60), b"", true)));
    let unread = vec![b'x'; bt_pty::TERM_READ_QUANTUM.get() + 1];
    leaf(&mut tab, draining).session.life.land(Some(fake(
        Duration::from_millis(400),
        &unread,
        true,
    )));
    leaf(&mut tab, born)
        .session
        .life
        .begin_birth(birth(BirthWait::Locale));
    let mut folio = HeadlessFolio::with_tabs(
        "life-reaper",
        vec![tab_holding(leaf_saying("在前 front")), tab],
        0,
    );
    let mut runtime = folio.runtime();
    let phases = |runtime: &crate::Runtime<'_, crate::headless::Headless>| {
        [ended, draining, born]
            .map(|seat| name_of(&runtime.window.tabs[1].sessions[&seat].session.life))
    };
    runtime
        .reap_exited_tabs(Instant::now())
        .expect("the reaper takes its turn");
    assert_eq!(
        phases(&runtime),
        [Name::Exited, Name::ExitDraining, Name::Deciding],
        "the first pass"
    );
    assert!(
        runtime.window.tabs[1].sessions[&ended]
            .session
            .life
            .pty()
            .is_some(),
        "an exited shell keeps its pseudoconsole"
    );
    let pane = &runtime.window.tabs[1].sessions[&draining];
    let pty = pane
        .session
        .life
        .pty()
        .expect("a draining shell keeps its pseudoconsole");
    while !pty.output_is_drained() {
        let _ = pty.read_output_slice();
    }
    runtime
        .reap_exited_tabs(Instant::now())
        .expect("the reaper takes its second turn");
    assert_eq!(
        phases(&runtime),
        [Name::Exited, Name::Kept, Name::Deciding],
        "drained, a birth death that wrote something is kept with its face"
    );
    assert_eq!(runtime.window.tabs.len(), 2, "and the tab is not ended");
}

/// RED (O-4b; the contract's (c)2) — **an exited shell keeps its pseudoconsole until the tab ends**:
/// the reaper leaves an exit after the grace Exited, pass after pass, with its shell; the road that
/// ends the tab retires it (Released), and only then is it gone.
///
/// MUTATION: have `exited` enter Released and drop the shell — the second assertion is red.
#[test]
fn an_exited_shell_keeps_its_pty_until_the_tab_ends() {
    let mut tab = crate::test_support::cross_tab(2, &["终了 ended", "诞生 born"]);
    let seats = tab.seats.terminals();
    let (ended, born) = (seats[0], seats[1]);
    tab.sessions
        .get_mut(&ended)
        .expect("the pane")
        .session
        .life
        .land(Some(fake(
            Duration::from_secs(60),
            "说过 said".as_bytes(),
            false,
        )));
    tab.sessions
        .get_mut(&born)
        .expect("the pane")
        .session
        .life
        .begin_birth(birth(BirthWait::Locale));
    let mut folio = HeadlessFolio::with_tabs(
        "life-exited",
        vec![tab_holding(leaf_saying("在前 front")), tab],
        0,
    );
    let mut runtime = folio.runtime();
    for pass in 0..3 {
        runtime
            .reap_exited_tabs(Instant::now())
            .expect("the reaper takes its turn");
        let life = &runtime.window.tabs[1].sessions[&ended].session.life;
        assert_eq!(name_of(life), Name::Exited, "pass {pass}");
        assert!(life.pty().is_some(), "pass {pass}: the shell is kept");
    }
    runtime.window.tabs[1].retire_all_shells();
    let life = &runtime.window.tabs[1].sessions[&ended].session.life;
    assert_eq!(name_of(life), Name::Released, "the tab's end retires it");
    assert!(life.pty().is_none());
}

/// RED (O-4b; the contract's (c)2, ruling D5) — **the exit line is said once across Exited,
/// FallingBack and Kept**: `exited` owes the line on the edge into Exited and never again, and
/// owes none in a phase the line was said for by another road (a birth death falling back or kept
/// says its own) or that has no shell.
///
/// MUTATION: answer `true` from Exited as well (`Phase::Exited { .. } => owed = true`) — the
/// second pass owes a second line.
#[test]
fn the_exit_line_is_said_once_across_exited_falling_back_and_kept() {
    test_support::on_the_window_thread();
    let mut life = life_in(Name::Live);
    let owed: Vec<bool> = (0..4)
        .map(|_| life.exited(Duration::from_secs(60)))
        .collect();
    assert_eq!(owed, [true, false, false, false], "said once on the edge");
    for name in [
        Name::FallingBack,
        Name::Kept,
        Name::Released,
        Name::Shellless,
    ] {
        let mut life = life_in(name);
        assert!(
            !life.exited(Duration::from_secs(60)),
            "{name:?} owes no ordinary exit line"
        );
        assert_eq!(name_of(&life), name);
    }
}

/// A pane in `name`, alone in the window's one tab.
fn window_with(tag: &str, name: Name) -> HeadlessFolio {
    let mut leaf = leaf_saying("旧壳 old shell");
    leaf.session.life = life_in(name);
    HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf)], 0)
}

/// A replacement whose shell has answered: in birth for it (Bearing), what was typed held.
fn answered_successor() -> Box<crate::TerminalPane> {
    let mut next = leaf_saying("");
    crate::await_shell(
        &mut next.session,
        answered(),
        decision(),
        &LeafSeed::default(),
    );
    next.session
        .life
        .birth()
        .expect("in birth")
        .hold("dir 目录\r".as_bytes())
        .expect("held");
    Box::new(next)
}

/// RED (O-4b; the contract's (c)3) — **a restart replaces the record from every phase**:
/// *Restart shell* is refused only while a restart is in flight, never by the phase — from each of
/// the nine it brings a replacement up beside the pane and leaves the pane's own phase as it was —
/// and a replacement whose shell answered lands over the pane in any phase, taking its place.
///
/// MUTATION: refuse the restart unless the pane has a shell (`if leaf.life.pty().is_none()`
/// beside `restart_in_flight`) — the shellless phases are refused.
#[test]
fn a_restart_replaces_the_record_from_every_phase() {
    for name in EVERY {
        let mut folio = window_with(&format!("life-restart-{name:?}"), name);
        let mut runtime = folio.runtime();
        let seat = runtime.seats.identity();
        runtime
            .restart_shell(seat)
            .expect("the restart is asked for");
        let pane = &runtime.sessions[&seat];
        assert!(
            pane.restart_in_flight(),
            "{name:?}: a restart is brought up"
        );
        assert_eq!(
            name_of(&pane.session.life),
            name,
            "{name:?}: the pane stays as it was"
        );

        // Its landing, as `land_restart` takes it: the record's own landing answers the same in
        // every phase of the pane it replaces, and the replacement takes the seat.
        let pane = runtime.sessions.get_mut(&seat).expect("the pane");
        pane.take_successor().expect("the replacement");
        pane.await_successor(answered_successor());
        let born = pane
            .successor
            .as_deref()
            .expect("waiting")
            .session
            .incarnation;
        let crate::SuccessorLanding::Landed(landed) =
            crate::runtime::panes::land_successor(pane, |line| panic!("nothing dropped: {line}"))
                .expect("the landing")
        else {
            panic!("{name:?}: the replacement lands over the pane");
        };
        assert_eq!(
            name_of(&pane.session.life),
            name,
            "{name:?}: the landing leaves the pane as it was"
        );
        runtime
            .replace_restarted_shell(seat, *landed)
            .expect("the replacement takes the seat");
        let pane = &runtime.sessions[&seat];
        assert_eq!(
            pane.session.incarnation, born,
            "{name:?}: the replacement is the pane"
        );
        assert_eq!(
            name_of(&pane.session.life),
            Name::Live,
            "{name:?}: with its shell"
        );
        assert!(!pane.restart_in_flight());
    }
}

/// RED (O-4b; the contract's (c)3) — **a kept pane restarts into a live shell**: a birth death kept
/// with its face has no shell, and *Restart shell* over it lands a shell that runs.
///
/// MUTATION: have `land_restart` keep the pane on screen (`Ok(())` for
/// `self.replace_restarted_shell(seat, *successor)`) — the seat stays Kept.
#[test]
fn a_kept_pane_restarts_into_a_live_shell() {
    let mut folio = window_with("life-kept-restart", Name::Kept);
    let mut runtime = folio.runtime();
    let seat = runtime.seats.identity();
    runtime
        .restart_shell(seat)
        .expect("the restart is asked for");
    let pane = runtime.sessions.get_mut(&seat).expect("the pane");
    pane.take_successor().expect("the replacement");
    pane.await_successor(answered_successor());
    runtime.land_births().expect("the landing");
    let life = &runtime.sessions[&seat].session.life;
    assert_eq!(
        name_of(life),
        Name::Live,
        "the kept pane runs a shell again"
    );
    assert!(life.pty().is_some());
}

/// RED (O-4b; the contract's (c)3) — **a second restart while one is in birth is ignored**: the
/// replacement being born stays the one, and the restart is still in flight.
///
/// MUTATION: drop `restart_shell`'s `if leaf.restart_in_flight() { return Ok(()) }` — the second
/// restart replaces the replacement.
#[test]
fn a_second_restart_while_one_is_in_birth_is_ignored() {
    let mut folio = window_with("life-second-restart", Name::Exited);
    let mut runtime = folio.runtime();
    let seat = runtime.seats.identity();
    runtime
        .restart_shell(seat)
        .expect("the restart is asked for");
    let first = runtime.sessions[&seat]
        .successor
        .as_deref()
        .expect("in birth beside the pane")
        .session
        .incarnation;
    runtime
        .restart_shell(seat)
        .expect("the second is asked for");
    let pane = &runtime.sessions[&seat];
    assert!(pane.restart_in_flight());
    assert_eq!(
        pane.successor
            .as_deref()
            .expect("still beside")
            .session
            .incarnation,
        first,
        "the replacement in birth is the one"
    );
}

/// RED (O-4b; the contract's (c)3) — **a restart over a pending birth abandons the old birth**: a
/// pane whose own shell is still being born (Bearing, its worker held) is replaced whole by a
/// replacement that landed, and the old birth's number is left abandoned in the mailbox, so its
/// answer is retired when it comes instead of reaching anything.
///
/// MUTATION: drop the abandonment from `ShellBirth::drop` (`births.abandoned.insert(..)`) — the
/// number is not held.
#[test]
fn a_restart_over_a_pending_birth_abandons_the_old_birth() {
    test_support::on_the_window_thread();
    let gate = Arc::new(Hold::default());
    gate.hold();
    let (shell, _heard) = asked(Arc::clone(&gate));
    let generation = shell.generation();
    let mut leaf = leaf_saying("诞生中 being born");
    leaf.session
        .life
        .begin_birth(birth(BirthWait::Shell(landing(shell))));
    let mut folio = HeadlessFolio::with_tabs("life-pending-restart", vec![tab_holding(leaf)], 0);
    let mut runtime = folio.runtime();
    let seat = runtime.seats.identity();
    runtime
        .sessions
        .get_mut(&seat)
        .expect("the pane")
        .await_successor(answered_successor());
    runtime.land_births().expect("the landing");
    assert_eq!(name_of(&runtime.sessions[&seat].session.life), Name::Live);
    assert!(
        pty_door::holds(generation),
        "the old birth's number is abandoned, not forgotten"
    );
    gate.release();
}

// ── A shell the system ends is not an edit (plan row 191) ─────────────────────────────────────
//
// The owner's three shutdowns of 2026-11-02 (0.4.8): every pane's shell ended with
// `DBG_TERMINATE_PROCESS` one reaper turn after another, before the system asked Folio's window
// anything, and each exit closed its tab — pinned or not — into Recent; the question then wrote
// what was left. Here the window holds single-pane tabs whose shells the test ends with a code,
// the reaper takes its turns, and the system's question is asked through the window procedure's
// own seam and settled by the application's own turn, writing through a working storage worker.

/// `pinned.len()` single-pane tabs, tab `n` (from 1) pinned as `pinned[n - 1]` says, the layout
/// recorded as it stands — what every change before the shutdown left in the store — on a
/// working storage lane in `home`.
fn a_window_of(tag: &str, home: &test_support::EndSessionHome, pinned: &[bool]) -> Application {
    let tabs = pinned
        .iter()
        .enumerate()
        .map(|(at, pin)| {
            let mut tab = test_support::ringing_tab(at as u64 + 1, 1);
            tab.pinned = *pin;
            // A name of its own, so a retired tab is a row Recent keeps (`Seed::names_itself`).
            tab.manual_name = Some(format!("tab {}", at + 1));
            tab
        })
        .collect();
    let mut folio = HeadlessFolio::with_tabs(tag, tabs, 0);
    let mut store =
        crate::persist::SessionStore::at_waking(home.session(), home.sentinel(), || {}).armed();
    store.set_save_budget(crate::lane::PATIENCE);
    folio.app.storage_on(store);
    let mut application = Application::of(folio);
    application.runtime().mark_session_dirty(Instant::now());
    application
}

type Application = crate::headless::HeadlessApplication;

/// The system ends the shell of tab `id` with `code`, and the reaper takes its turn.
fn ends(application: &mut Application, id: u64, code: u32) {
    let mut runtime = application.runtime();
    let tab = runtime
        .window
        .tabs
        .iter_mut()
        .find(|tab| tab.id == crate::TabId(id))
        .unwrap_or_else(|| panic!("tab {id} is still in the window"));
    for (_, leaf) in tab.leaves_mut() {
        leaf.life.land(Some(PtySession::fake_exited(
            Duration::from_secs(60),
            code,
            b"",
            false,
        )));
    }
    runtime
        .reap_exited_tabs(Instant::now())
        .expect("the reaper takes its turn");
}

/// The tab ids in the window, in strip order.
fn tab_ids(application: &mut Application) -> Vec<u64> {
    application
        .runtime()
        .window
        .tabs
        .iter()
        .map(|tab| tab.id.0)
        .collect()
}

/// How many rows Recent holds in the running application.
fn recent_rows(application: &Application) -> usize {
    application
        .folio
        .app
        .as_ref()
        .expect("the application")
        .recent
        .len()
}

/// The system asks, and the application's turn settles it, keeping in `home`.
fn the_question_is_settled(application: &mut Application, home: &test_support::EndSessionHome) {
    assert_eq!(test_support::the_system_asks(), Some(1));
    let recovery = home.0.join(crate::preview::RECOVERED_FOLDER);
    let _ = application
        .folio
        .settle_session_end_keeping_in(&application.event_loop, &recovery);
}

const SYSTEM_ENDED: u32 = bt_platform::session_end::SYSTEM_ENDED_EXIT_CODE;

/// RED (plan row 191) — **a shell the system ends keeps its pinned tab in the window and in the
/// document**: the pane stays, kept without a shell and saying why; nothing goes into Recent; the
/// document the store holds is the one before the exit.
///
/// MUTATION (plant `plant-191-a-no-classification.py`): the reaper reads no exit code — the tab is
/// retired into Recent and the document loses it.
#[test]
fn a_shell_the_system_ends_keeps_its_pinned_tab_in_the_document() {
    let home = test_support::EndSessionHome::new("row191-pinned");
    let mut application = a_window_of("row191-pinned", &home, &[true, true]);
    let before = application
        .folio
        .app
        .as_ref()
        .expect("the application")
        .session_store
        .loaded()
        .clone();
    ends(&mut application, 2, SYSTEM_ENDED);
    assert_eq!(tab_ids(&mut application), [1, 2], "the tab stays");
    {
        let runtime = application.runtime();
        let pane = runtime.window.tabs[1]
            .panes_mut()
            .next()
            .expect("the tab's pane")
            .1;
        assert_eq!(
            name_of(&pane.session.life),
            Name::Kept,
            "kept, without a shell"
        );
        let visible: String = pane
            .session
            .session
            .terminal()
            .visible_text()
            .join("")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert!(
            visible.contains("wasendedbythesystem(code1073807364,"),
            "the pane says why: {visible:?}"
        );
    }
    assert_eq!(recent_rows(&application), 0, "nothing goes into Recent");
    let app = application.folio.app.as_ref().expect("the application");
    assert_eq!(
        app.session_store.loaded(),
        &before,
        "nothing is recorded: the document is the one before the exit"
    );
}

/// RED (plan row 191) — **an unpinned tab whose shell the system ends stays in the layout for the
/// restore card**: after the question's write, the next start opens the pinned tab and asks about
/// the unpinned one, as after a quit.
///
/// MUTATION (plant `plant-191-b-pinned-only.py`): keep the pane only in a pinned tab — the unpinned
/// one is retired and the card has nothing to ask.
#[test]
fn a_shell_the_system_ends_keeps_an_unpinned_tab_for_the_card() {
    let home = test_support::EndSessionHome::new("row191-unpinned");
    let mut application = a_window_of("row191-unpinned", &home, &[true, false]);
    ends(&mut application, 2, SYSTEM_ENDED);
    the_question_is_settled(&mut application, &home);
    let (plan, _) = test_support::launch_plan_on_disk(&home.session());
    assert_eq!(
        (plan.open.len(), plan.ask.len()),
        (1, 1),
        "the pinned tab opens and the unpinned one is the card's question"
    );
    assert!(plan.open[0].pinned && !plan.ask[0].pinned);
}

/// RED (plan row 191) — **the owner's case: eight tabs whose shells the system ends one turn after
/// another, then the system's question, write all eight with their pins and no Recent row.**
///
/// MUTATION (plant `plant-191-c-active-tab-only.py`): keep the pane only in the active tab — seven
/// tabs go into Recent and the document holds what is left.
#[test]
fn eight_tabs_the_system_ends_then_its_question_write_all_eight_with_their_pins() {
    let home = test_support::EndSessionHome::new("row191-owner");
    let pins = [true, true, true, true, true, false, false, false];
    let mut application = a_window_of("row191-owner", &home, &pins);
    for id in (1..=8).rev() {
        ends(&mut application, id, SYSTEM_ENDED);
    }
    assert_eq!(tab_ids(&mut application), [1, 2, 3, 4, 5, 6, 7, 8]);
    the_question_is_settled(&mut application, &home);
    let windows = test_support::windows_on_disk(&home.session());
    assert_eq!(windows.len(), 1, "one window");
    assert_eq!(
        windows[0]
            .tabs
            .iter()
            .map(|tab| tab.pinned)
            .collect::<Vec<_>>(),
        pins,
        "all eight tabs, each with its pin"
    );
    let (read, _, _) = bt_persist::read_session(&home.session());
    assert!(read.recent.is_empty(), "no Recent row: {:?}", read.recent);
    assert_eq!(recent_rows(&application), 0);
}

/// RED (plan row 191, the unchanged half) — **a shell's own exit still closes its tab**: any other
/// code retires the tab into Recent as before, pinned or not.
///
/// MUTATION (plant `plant-191-d-every-exit-is-the-systems.py`): read every exit as the system's —
/// the tab stays.
#[test]
fn a_shells_own_exit_still_retires_its_tab() {
    let home = test_support::EndSessionHome::new("row191-own-exit");
    let mut application = a_window_of("row191-own-exit", &home, &[true, true]);
    ends(&mut application, 2, 1);
    assert_eq!(tab_ids(&mut application), [1], "the pinned tab is retired");
    assert_eq!(recent_rows(&application), 1, "into Recent");
}

/// RED (plan row 191) — **a pane the system's end kept restarts into a live shell, or closes, as a
/// kept birth death does.**
///
/// MUTATION (plant `plant-191-e-shell-left-in-the-pane.py`): leave the ended shell in the pane
/// (drop the `release_dead_shell` call) — the pane is not Kept, and the restart's landing finds a
/// pane that never let go of its shell.
#[test]
fn a_pane_the_systems_end_kept_restarts_or_closes_like_a_kept_pane() {
    let home = test_support::EndSessionHome::new("row191-restart");
    let mut application = a_window_of("row191-restart", &home, &[true, true]);
    ends(&mut application, 2, SYSTEM_ENDED);
    application
        .runtime()
        .close_tab(1)
        .expect("the person closes the kept tab");
    assert_eq!(tab_ids(&mut application), [1], "the kept tab closes");
    assert_eq!(
        recent_rows(&application),
        1,
        "and goes into Recent, as a close does"
    );
    ends(&mut application, 1, SYSTEM_ENDED);
    let mut runtime = application.runtime();
    let seat = runtime.seats.identity();
    assert_eq!(name_of(&runtime.sessions[&seat].session.life), Name::Kept);
    runtime
        .restart_shell(seat)
        .expect("the restart is asked for");
    let pane = runtime.sessions.get_mut(&seat).expect("the pane");
    pane.take_successor().expect("the replacement");
    pane.await_successor(answered_successor());
    runtime.land_births().expect("the landing");
    let life = &runtime.sessions[&seat].session.life;
    assert_eq!(
        name_of(life),
        Name::Live,
        "the kept pane runs a shell again"
    );
    assert!(life.pty().is_some());
}
