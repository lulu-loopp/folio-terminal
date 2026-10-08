//! **`git`, as the application drives it.** Tests whose first assertion is about
//! `git`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{LedgerPane, claim_after_a_look, latched, one_turn, ringing_tab, wired};

// ── the notices (user ruling, 2026-08-16) ──────────────────────────────

/// PIN — **a verb git refused raises exactly one notice, carrying git's own
/// words; a read that failed raises none.**
///
/// This is the whole transient/persistent split, and both halves matter. The
/// first half is what replaced the red banner: the report is made once, at
/// the instant the answer lands, rather than re-derived from a remembered
/// sentence on every frame until the next attempt. The second half is what
/// stops the replacement from being worse than what it replaced — a machine
/// with no git would otherwise raise a card that appears, says so, and takes
/// the only report away again six seconds later.
///
/// Mutation: add `Repo`/`Status` to `git_answer_notice` and the second half
/// goes red; drop the `Checkout` arm and the refusal the user reported —
/// "fatal: 't1-tab-basics' is already used by worktree at …" — is silent.
#[test]
fn a_refused_verb_raises_one_notice_and_a_read_that_failed_raises_none() {
    let root = std::path::PathBuf::from(r"D:\repo");
    let refused = |words: &str| git::GitFault::Refused(words.to_owned());
    let worktree = "fatal: 't1-tab-basics' is already used by worktree at D:/x";

    let checkout = git::GitAnswer::Checkout {
        root: root.clone(),
        target: "t1-tab-basics".to_owned(),
        outcome: Err(refused(worktree)),
    };
    let lock = "fatal: Unable to create '.git/index.lock': File exists.";
    let write = git::GitAnswer::Write {
        root: root.clone(),
        verb: git::GitWriteVerb::Stage,
        paths: vec!["work.rs".to_owned()],
        outcome: Err(refused(lock)),
    };
    assert_eq!(git_answer_notice(&checkout).as_deref(), Some(worktree));
    assert_eq!(git_answer_notice(&write).as_deref(), Some(lock));

    // Every read, and the two verbs that went through: silence.
    for quiet in [
        git::GitAnswer::Repo {
            dir: root.clone(),
            outcome: Err(git::GitFault::GitMissing("no git.exe".to_owned())),
        },
        git::GitAnswer::Status {
            root: root.clone(),
            outcome: Err(refused("fatal: detected dubious ownership")),
        },
        git::GitAnswer::Refs {
            root: root.clone(),
            outcome: Err(git::GitFault::TimedOut),
        },
        git::GitAnswer::Log {
            root: root.clone(),
            skip: 0,
            outcome: Err(refused("fatal: bad object HEAD")),
        },
        git::GitAnswer::Checkout {
            root: root.clone(),
            target: "main".to_owned(),
            outcome: Ok(()),
        },
        git::GitAnswer::Write {
            root,
            verb: git::GitWriteVerb::Stage,
            paths: vec!["work.rs".to_owned()],
            outcome: Ok(()),
        },
    ] {
        assert_eq!(
            git_answer_notice(&quiet),
            None,
            "a persistent fault is not a notice: {quiet:?}"
        );
    }

    // And who is named as speaking, which is the other half of the words:
    // the sentence is git's, so the title has to say so rather than let a
    // paragraph of `fatal:` look like something this window decided.
    assert_eq!(git_panel::git_toast_title(), "Git");
}

/// A tab's identity is the terminal it holds, wherever that sits in the tree.
#[test]
fn a_seed_is_read_from_the_first_terminal_in_the_tree() {
    let split = LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 500_000,
        children: [
            Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                bt_persist::FilesLeafV1 {
                    view: bt_persist::FilesViewV1::Files,
                    root: "C:\\repo".to_owned(),
                    open: Vec::new(),
                    sel: None,
                    width: 240,
                    remotes_open: false,
                },
            ))),
            Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh".to_owned(),
                cwd: "C:\\repo\\src".to_owned(),
                manual_name: Some("build".to_owned()),
                card_skip: 0,
                last_command: String::new(),
            }))),
        ],
    });
    let leaf = first_term_leaf(&split).expect("a files pane is not the tab's identity");
    assert_eq!(leaf.cwd, "C:\\repo\\src");
    assert_eq!(leaf.manual_name.as_deref(), Some("build"));

    // A files-only tree has no terminal to speak for it.
    assert!(
        first_term_leaf(&LayoutNodeV1::Leaf(LeafNodeV1::Unknown)).is_none(),
        "an unknown leaf is not a terminal"
    );
}

fn ledger_strong_wait(kind: attention::WaitKind) -> attention::Event {
    attention::Event::StrongWait(attention::WaitSlot::Level(kind))
}

fn ledger_clear_all(reason: attention::ClearReason) -> attention::Event {
    attention::Event::StrongClear {
        selector: attention::ClearSelector::All,
        class: attention::ClearClass::Boundary,
        reason,
        begins_turn: false,
    }
}

/// PIN — **the two lanes meet on one account: one pane, two producers, one request.**
///
/// The wire says "this pane wants you" and, six seconds later, a hook says "and it is blocked on
/// your input". Those are **one** request with two pieces of evidence, not two requests: the place
/// in the queue is not re-stamped, no second episode is minted, and the wording rises — and falls
/// again the moment the stronger evidence is withdrawn, because a pane that says "waiting for you"
/// on the strength of a credential that no longer exists is a pane telling you something untrue.
///
/// It ends on the wire because that is the half this slice added: the program takes its own
/// sentence back, the place goes, and the line says the withdrawal came in over `src=osc`. A trace
/// that could not tell the two producers apart would be a trace that could not answer the one
/// question anybody asks it — *did the adapter actually install, or is this the generic path?*
#[test]
fn one_pane_two_producers_and_one_episode_between_them() {
    let mut session = wired();
    let mut pane = LedgerPane::new();
    session
        .feed(b"\x1b]1337;RequestAttention=yes\x07")
        .expect("the session accepts bytes");
    let rose = pane
        .ledger
        .weak_edge(session.status().attention_request)
        .expect("a rising edge");
    assert_eq!(
        pane.at(rose),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    assert_eq!(
        pane.at(ledger_strong_wait(attention::WaitKind::Permission)),
        ["upgrade tab=1 seat=SeatId(2) episode=1 grounds=awaiting src=pipe gen=1"],
        "the same request, confirmed by the other producer"
    );
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ]
    );
    assert_eq!(
        pane.at(ledger_clear_all(attention::ClearReason::Hook)),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 reason=hook",
            "downgrade tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested src=pipe \
             reason=clear",
        ],
        "the strong layer withdrew; the weak one is still up, so the place stays and the wording \
         falls back"
    );
    session
        .feed(b"\x1b]1337;RequestAttention=no\x07")
        .expect("the session accepts bytes");
    let fell = pane
        .ledger
        .weak_edge(session.status().attention_request)
        .expect("a falling edge");
    assert_eq!(
        pane.at(fell),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=osc"]
    );
    assert_eq!(pane.state(), attention::State::Idle);
}

/// PIN — **the look that spends a latch is the runtime's own call.**
///
/// [`claim_after_a_look`] reproduces `clear_attention`'s two field writes against a bare
/// [`SessionStatus`], because a `SessionStatus` cannot be injected into a live session. It is
/// therefore the one place in these tests that could drift away from the runtime without
/// anything failing. This is the anchor: a real leaf, a real `BEL`, a real failing exit code,
/// and the runtime's own pass — and the two fields the fixture clears are exactly the two that
/// end up clear.
#[test]
fn the_look_that_spends_a_latch_is_the_runtimes_own_call() {
    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let mut next = attention::Places::default();
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's one shell")
        .session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07x\x1b]133;C\x07\x1b]133;D;1\x07\x07")
        .expect("the fixture's bytes parse");
    let rang = tabs[0].sessions[&seat].session.status();
    assert!(rang.bell_latched() && rang.failure_exit_code == Some(1));

    one_turn(&mut tabs, 0, true, &mut next);

    let looked = tabs[0].sessions[&seat].session.status();
    assert!(!looked.bell_latched());
    assert_eq!(looked.failure_exit_code, None);
    // The fixture, handed the same two facts, says the same thing.
    assert_eq!(
        claim_after_a_look(latched(), true, true),
        StatusClaim::Silent
    );
    assert_eq!(tabs[0].fleet_claim_for(true), StatusClaim::Silent);
}

/// PIN (v2 ④) — **a ref verb git refused is one card carrying git's own
/// sentence**, raised off the answer and nowhere else.
///
/// The named verbs ride the same `Write` answer the four pathspec verbs do,
/// which is the whole reason nothing new had to be taught to the notice
/// path: `git branch -d` on an unmerged branch comes back as a refused
/// write, and the window prints what git said rather than paraphrasing a
/// program that has already explained itself.
///
/// MUTATION: paraphrase the refusal ("could not delete the branch") and this
/// goes red on the exact bytes — which is the only way a reader can search
/// for the sentence they were shown.
#[test]
fn a_ref_verb_git_refused_says_gits_own_words_once() {
    let root = PathBuf::from(r"D:\repo");
    let refusal = "error: the branch 'goner' is not fully merged.";
    let refused = git::GitAnswer::Write {
        root: root.clone(),
        verb: git::GitWriteVerb::DeleteBranch {
            name: "goner".to_owned(),
        },
        paths: Vec::new(),
        outcome: Err(git::GitFault::Refused(refusal.to_owned())),
    };
    assert_eq!(
        git_answer_notice(&refused).as_deref(),
        Some(refusal),
        "one notice, in git's words"
    );
    let went_through = git::GitAnswer::Write {
        root,
        verb: git::GitWriteVerb::CreateBranch {
            name: "feature".to_owned(),
            at: "a1b2c3d".to_owned(),
        },
        paths: Vec::new(),
        outcome: Ok(()),
    };
    assert_eq!(
        git_answer_notice(&went_through),
        None,
        "and a verb that worked says nothing — the page redrawing is the report"
    );
}
