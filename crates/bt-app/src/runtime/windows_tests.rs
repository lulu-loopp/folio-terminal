//! **The windows module's doors, driven on a headless window**: the close an exit's answer asks
//! for is taken by the event loop through [`crate::Runtime::take_close_request`], once
//! (T-DISPATCHER-WRITES-INTO-DOORS); and every group of the window's doors to the system — its
//! place, its dress, the keyboard and the foreground, attention, the taskbar button, the system's
//! news, a page's place — is driven through the runtime's own code
//! (T-NATIVE-HANDLE-BEHIND-THE-SEAM).
//!
//! And a close that waits for the saves on their way (T-CLOSE-GATE-WAITS, S-6b): a pane's, a tab's
//! or a window's close over buffers whose saves are on the storage worker, held by the gate across
//! turns and decided on the saves' answers, on a headless window and the real storage lane.

use crate::headless::{
    HeadlessApplication, HeadlessFolio, HeadlessParts, Ledger, PlatformAnswers, WindowEffect,
    headless_window,
};
use crate::test_support::{
    buffer_saying, host_spelling, leaf_saying, ringing_tab, tab_holding, tab_with_a_preview,
};
use crate::{preview_edit, restore};
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::window::WindowId;

/// RED (T-SUMMON-DIRTY-PREVIEW) — **a close request is taken once, through the windows door**:
/// a window the OS asked to close over an unsaved edit is answered `Discard`, which re-requests
/// its own close; the event loop's take hands that close over, and a second take finds nothing —
/// the next event closes nothing.
///
/// MUTATION: make `take_close_request` read the request without taking it
/// (`self.window.window_close_requested`) — the second take's assertion goes red; write the field
/// from `FolioApp::on_window_event` instead of calling the door — the ownership census
/// (`bt-source`'s `census` test) goes red on a second writer.
#[test]
fn a_close_request_is_taken_once_through_the_windows_door() {
    let path = host_spelling(r"C:\w\notes.md");
    let mut buffer = buffer_saying(&path, "notes.md", "one\n");
    let mut caret = preview_edit::EditCaret::default();
    let body = buffer.content().cloned().expect("the head landed");
    caret.place(&body, 4, false);
    assert!(
        buffer.edit_by_caret(&mut caret, |content, caret| {
            preview_edit::insert(content, caret, "typed by hand")
        }),
        "the keystroke landed"
    );
    let (tab, _) = tab_with_a_preview(1, vec![buffer]);
    let folio = HeadlessFolio::with_tabs("close-request-door", vec![tab], 0);
    let mut application = HeadlessApplication::of(folio);
    let window = application.id;

    application.window_event(WindowEvent::CloseRequested);
    let mut runtime = application.runtime();
    assert_eq!(
        runtime.window.dirty_gate.request(),
        Some(&restore::GateRequest::Shut),
        "the OS close asks first"
    );
    assert_eq!(
        runtime.take_close_request(),
        None,
        "nothing is requested while the question is up"
    );
    runtime
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");

    assert_eq!(
        runtime.take_close_request(),
        Some(window),
        "Discard re-requests this window's close"
    );
    assert_eq!(
        runtime.take_close_request(),
        None,
        "and the request is spent by the taking"
    );
}

/// One application with two ordinary windows, `1` and `2`, so that closing either is a window
/// going and not the run ending; window `1`'s question is up, asking about `request`.
fn two_windows_asking(tag: &str, request: restore::GateRequest) -> HeadlessApplication {
    let folio = HeadlessFolio::with_tabs(tag, vec![ringing_tab(1, 1)], 0);
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
        vec![ringing_tab(2, 1)],
        0,
    );
    let mut application = HeadlessApplication::of(folio);
    application.folio.windows.insert(WindowId::from(2), second);
    application.runtime().window.dirty_gate.open(request);
    application
}

/// Whether the application has told window `id` to go.
fn is_leaving(application: &mut HeadlessApplication, id: WindowId) -> bool {
    application
        .folio
        .windows
        .get_mut(id)
        .is_some_and(|window| window.leaving.is_some())
}

/// RED (T-SUMMON-DIRTY-PREVIEW) — **the event loop spends a re-requested close on the window it
/// names**: answered `Discard`, a window's own shut closes that window at the tail of the next
/// event it dispatches; the run's end answered in one window closes the *other* window it names,
/// and the answering window stays.
///
/// MUTATION: drop the `runtime.take_close_request()` call from the tail of
/// `FolioApp::on_window_event` (`let requested = None;`) — the first case's first assertion goes
/// red; match nothing in place of `requested.filter(|closing| *closing != window_id)` — the
/// second case's first assertion goes red.
#[test]
fn the_dispatcher_closes_the_window_a_close_request_names() {
    let one = WindowId::from(1);
    let two = WindowId::from(2);

    // Its own close.
    let mut application = two_windows_asking("close-tail-self", restore::GateRequest::Shut);
    application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");
    application.window_event(WindowEvent::Focused(true));
    assert!(
        is_leaving(&mut application, one),
        "the window whose shut was answered goes"
    );
    assert!(
        !is_leaving(&mut application, two),
        "and the other window stays"
    );

    // Another window's close, re-run from the window where it was answered.
    let mut application = two_windows_asking(
        "close-tail-other",
        restore::GateRequest::ShutWithTheRun(two),
    );
    application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Discard)
        .expect("the answer is spent");
    application.window_event(WindowEvent::Focused(true));
    assert!(
        is_leaving(&mut application, two),
        "the window the answer names goes"
    );
    assert!(
        !is_leaving(&mut application, one),
        "and the window that answered stays"
    );
}

// ── a close that waits for the saves on their way (S-6b, T-CLOSE-GATE-WAITS) ──

/// **A window whose tabs each hold one preview of a real file typed into**, its saves handed to
/// a storage lane whose queue the test serves (`persist::StorageLane::scripted`: the real lane,
/// the test the worker), beside a second window — so that closing the first is a window going
/// and not the run ending.
struct Saving {
    application: HeadlessApplication,
    jobs: crate::persist::ScriptedJobs,
    /// The preview seat of each tab, in tab order.
    seats: Vec<bt_layout::SeatId>,
    /// The file behind each tab's buffer, in tab order.
    paths: Vec<std::path::PathBuf>,
    dir: std::path::PathBuf,
}

/// The body every file starts with, and what is typed into each.
const READ: &str = "第一行 first line\n";
const TYPED: &str = "第二行 typed by hand\n";

impl Saving {
    /// One tab per name, each with its buffer typed into and nothing sent yet.
    fn open(tag: &str, names: &[&str]) -> Self {
        let dir = crate::test_support::disk_scratch(&format!("close-gate-{tag}"));
        let mut tabs = Vec::new();
        let mut seats = Vec::new();
        let mut paths = Vec::new();
        for (at, name) in names.iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, READ).expect("the file is written");
            let mut buffer = crate::test_support::buffer_read_from(&path);
            assert!(buffer.edit_content(|body| {
                body.push_str(TYPED);
                true
            }));
            assert!(buffer.dirty(), "there is unsaved work in {name}");
            let (tab, seat) = tab_with_a_preview(at as u64 + 1, vec![buffer]);
            tabs.push(tab);
            seats.push(seat);
            paths.push(path);
        }
        let mut folio = HeadlessFolio::with_tabs(tag, tabs, 0);
        let (lane, jobs) = crate::persist::StorageLane::scripted();
        folio.app.preview_saves = crate::preview::PreviewSaves::on(lane);
        let other = headless_window(
            &folio.app,
            HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
            vec![ringing_tab(9, 1)],
            0,
        );
        let mut application = HeadlessApplication::of(folio);
        application.folio.windows.insert(WindowId::from(2), other);
        Self {
            application,
            jobs,
            seats,
            paths,
            dir,
        }
    }

    /// The surface tab `at`'s preview pane is.
    fn surface(&self, at: usize) -> crate::PreviewSurface {
        crate::test_support::seat_of(crate::TabId(at as u64 + 1), self.seats[at])
    }

    /// `Ctrl+S` on tab `at`'s pane: the save handed to the storage worker, as the press hands it.
    fn press_save(&mut self, at: usize) {
        let surface = self.surface(at);
        let mut runtime = self.application.runtime();
        let crate::Runtime { app, window } = &mut runtime;
        let pool = &mut window.tabs[at].preview_pool;
        let source = pool
            .buffers()
            .next()
            .expect("the tab's buffer")
            .source()
            .clone();
        let buffer = pool.get_mut(&source).expect("the tab's buffer");
        assert_eq!(
            buffer.request_save(&mut app.preview_saves, surface),
            crate::preview::SaveAsked::Sent,
            "the save is handed to the worker"
        );
    }

    /// Type into tab `at`'s buffer once more.
    fn type_again(&mut self, at: usize) {
        let runtime = self.application.runtime();
        let pool = &mut runtime.window.tabs[at].preview_pool;
        let source = pool
            .buffers()
            .next()
            .expect("the tab's buffer")
            .source()
            .clone();
        let buffer = pool.get_mut(&source).expect("the tab's buffer");
        assert!(buffer.edit_content(|body| {
            body.push_str("又一行 once more\n");
            true
        }));
    }

    /// **The worker serves the next save**, `crossing` written into the file under it.
    fn serve(&self, crossing: Option<&str>) {
        let job = crate::test_support::next_document(&self.jobs);
        let _ = crate::test_support::serve_on_a_worker(
            job,
            crossing.map(|bytes| bytes.as_bytes().to_vec()),
        );
    }

    /// **The worker ends** with every save it holds unanswered: the test lets its queue go, and
    /// the lane's ledger ends what is owed `WorkerGone`.
    fn end_the_worker(&mut self) {
        let (_, unserved) = crate::persist::StorageLane::scripted();
        drop(std::mem::replace(&mut self.jobs, unserved));
    }

    /// **One turn after the worker's wake**: the answers heard by the buffers
    /// (`AppEvent::DocumentsAnswered`), then the turn's look at every waiting close, at `now`.
    fn turn_at(&mut self, now: Instant) {
        self.application
            .folio
            .take_document_answers()
            .expect("the answers are heard");
        self.application
            .folio
            .settle_close_waits(&self.application.event_loop, now)
            .expect("the waiting closes are looked at");
    }

    fn turn(&mut self) {
        self.turn_at(Instant::now());
    }

    /// The question up in the first window, if one is.
    fn asked(&mut self) -> Option<restore::GateRequest> {
        self.application
            .runtime()
            .window
            .dirty_gate
            .request()
            .cloned()
    }

    /// Whether a close waits in the first window.
    fn waits(&mut self) -> bool {
        self.application
            .runtime()
            .window
            .dirty_gate
            .waiting()
            .is_some()
    }

    /// Whether tab `at`'s preview pane is still in its tree.
    fn pane_stands(&mut self, at: usize) -> bool {
        let seat = self.seats[at];
        self.application.runtime().window.tabs[at]
            .seats
            .tree()
            .find_seat(seat)
            .is_some()
    }

    /// What tab `at`'s pane says, if anything.
    fn pane_says(&mut self, at: usize) -> Option<String> {
        let surface = self.surface(at);
        self.application
            .runtime()
            .preview_pane(surface)
            .and_then(|pane| pane.notice().map(|(said, _)| said.clone()))
    }

    /// What the file behind tab `at` holds.
    fn file(&self, at: usize) -> String {
        std::fs::read_to_string(&self.paths[at]).expect("the file reads back")
    }

    /// Whether window `id` has been told to go.
    fn leaving(&mut self, id: u64) -> bool {
        is_leaving(&mut self.application, WindowId::from(id))
    }
}

impl Drop for Saving {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// RED (S-6b) — **closing a pane right after `Ctrl+S` asks nothing while the save is landing,
/// and closes once it has.**
///
/// The pane's close is asked with the save of the very body on the worker: the gate waits
/// instead of asking — no question, the pane still there. The worker writes the file; on the
/// turn its answer wakes, the buffer is clean and the close is re-run from the top, finds nothing
/// to ask, and the pane goes. The file holds what was typed.
///
/// MUTATION: in `raise_dirty_gate_over`, drop the wait (`close_wait_over` never consulted) — the
/// first assertion goes red: the close asks "discard?" while the save is landing.
#[test]
fn a_close_while_its_save_is_landing_asks_nothing_and_closes_when_it_lands() {
    let mut saving = Saving::open("lands", &["笔记 notes.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert_eq!(
        saving.asked(),
        None,
        "nothing is asked while the save is landing"
    );
    assert!(saving.waits(), "the close waits for the save's answer");
    assert!(saving.pane_stands(0), "and the pane stays meanwhile");

    saving.serve(None);
    saving.turn();
    assert_eq!(saving.asked(), None, "nothing is asked once it has landed");
    assert!(!saving.waits(), "the wait is over");
    assert!(
        !saving.pane_stands(0),
        "the save landed: the pane is closed"
    );
    assert_eq!(
        saving.file(0),
        format!("{READ}{TYPED}"),
        "and what was typed is in the file"
    );
}

/// RED (S-6b) — **a save that meets another writer's bytes leaves the close to the conflict's own
/// road: the pane stays and says the conflict, and nothing is asked.**
///
/// The close waits; the worker finds the file changed under the save (`Restore` puts the other
/// writer's bytes back and answers the conflict). Asking "discard?" over a file the person has not
/// looked at again would ask them to throw work away blind, so the close is withdrawn: the pane
/// says the conflict, the other writer's bytes are in the file, and the buffer still holds the
/// edit.
///
/// MUTATION: in `Runtime::settle_close_wait`, drop the `Conflict` arm (the close re-run like a
/// refusal) — the first assertion goes red: the question is put over the conflict.
#[test]
fn a_save_that_meets_a_conflict_withdraws_the_close_and_the_pane_says_so() {
    let mut saving = Saving::open("conflict", &["对照 compare.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert!(saving.waits());

    let theirs = "别人写的 by another writer\n";
    saving.serve(Some(theirs));
    saving.turn();
    assert_eq!(saving.asked(), None, "the conflict asks nothing");
    assert!(!saving.waits(), "the close is withdrawn");
    assert!(
        saving.pane_stands(0),
        "the pane stays for the person to look"
    );
    assert_eq!(
        saving.pane_says(0).as_deref(),
        Some(crate::preview::preview_conflict_notice()),
        "and it says the conflict"
    );
    assert_eq!(
        saving.file(0),
        theirs,
        "the other writer's bytes are in the file"
    );
}

/// RED (S-6b) — **the worker gone before the save was answered: the question, the pane having said
/// once that the file could not be written.**
///
/// The close waits; the storage worker ends with the save unanswered (the test lets its queue go:
/// the ledger ends the request `WorkerGone`). The pane says it the one time, in the words every
/// unwritten save says it ("Not saved — the file could not be written"); the close is re-run, the
/// buffer is still dirty, and the question is put as it always was — its lines the file's name and
/// nothing else, the reason not said a second time on the card.
///
/// MUTATION: in `Runtime::settle_close_wait`, withdraw on `NotSaved` as on a conflict — the first
/// assertion goes red: nothing is asked, and the close is dropped over unsaved work.
#[test]
fn the_worker_gone_puts_the_question_the_pane_having_said_it_once() {
    let mut saving = Saving::open("gone", &["日志 log.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert!(saving.waits());

    saving.end_the_worker();
    saving.turn();
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::ClosePane(seat)),
        "the save did not land: the close asks"
    );
    assert_eq!(
        saving.pane_says(0),
        Some(crate::i18n::not_saved(
            crate::i18n::Text::PreviewSaveNotStarted.text()
        )),
        "the pane said why, once"
    );
    {
        let runtime = saving.application.runtime();
        let names = runtime.gate_dirty_names(&restore::GateRequest::ClosePane(seat));
        assert_eq!(
            runtime.window.dirty_gate.lines(&names),
            restore::GateRequest::ClosePane(seat).lines(&names),
            "and the card is the question as it always was"
        );
    }
    assert_eq!(saving.file(0), READ, "nothing was written");
}

/// RED (S-6b) — **past the bound the close asks, saying once that a save is still on its way; the
/// late landing reaches the sink.**
///
/// The worker holds the save past `restore::CLOSE_WAITS_FOR_A_SAVE` (the turn is taken at the
/// bound: the clock is the turn's, never waited for). The question is put as it always was, with
/// one more line — a save is still on its way — said once. Answered *Discard*, the pane goes with
/// its pool; the save the person made still lands, and its answer, addressed to a buffer no pool
/// holds, is recorded by the sink (S-6a's pin, held).
///
/// MUTATION: in `Runtime::settle_close_wait`, put the question with `open` for
/// `open_after_the_wait` — the line assertion goes red (the sentence is missing).
#[test]
fn past_the_bound_the_close_asks_saying_a_save_is_still_on_its_way() {
    let mut saving = Saving::open("bound", &["慢盘 slow.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    saving.turn();
    assert!(saving.waits(), "within the bound the close waits");

    saving.turn_at(Instant::now() + restore::CLOSE_WAITS_FOR_A_SAVE);
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::ClosePane(seat)),
        "at the bound the close asks"
    );
    {
        let mut runtime = saving.application.runtime();
        let names = runtime.gate_dirty_names(&restore::GateRequest::ClosePane(seat));
        let lines = runtime.window.dirty_gate.lines(&names);
        let still = restore::gate_save_still_on_its_way_text();
        assert_eq!(
            lines.iter().filter(|line| line.as_str() == still).count(),
            1,
            "saying, once, that a save is still on its way: {lines:?}"
        );
        runtime
            .answer_dirty_gate(restore::GateAnswer::Discard)
            .expect("the answer is spent");
    }
    assert!(!saving.pane_stands(0), "Discard: the pane goes");

    saving.serve(None);
    saving.turn();
    let sunk = saving
        .application
        .folio
        .app
        .as_ref()
        .expect("the application")
        .preview_saves
        .sunk();
    assert_eq!(sunk, 1, "the late landing reached the sink");
    assert_eq!(
        saving.file(0),
        format!("{READ}{TYPED}"),
        "and the save the person made is in the file"
    );
}

/// RED (S-6b) — **an edit while the close waits withdraws it, once; the next close asks.**
///
/// The close waits for the save of the body pressed; the person types again. On the next turn the
/// close is withdrawn — nothing asked, the pane still there. The save then lands for the older
/// body: the buffer is dirty, with nothing on its way, and the next close asks.
///
/// MUTATION: in `PreviewBuffer::at_the_close`, drop the `Edited` arm — the first assertion goes
/// red: the edit is read as a save that did not land and the close asks there and then.
#[test]
fn an_edit_while_the_close_waits_withdraws_it_and_the_next_close_asks() {
    let mut saving = Saving::open("edited", &["草稿 draft.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert!(saving.waits());

    saving.type_again(0);
    saving.turn();
    assert_eq!(
        saving.asked(),
        None,
        "the edit withdraws the close: nothing is asked"
    );
    assert!(!saving.waits(), "and nothing waits");
    assert!(saving.pane_stands(0), "the pane stays");

    saving.serve(None);
    saving.turn();
    assert!(
        saving.pane_stands(0),
        "the landing of the older body closes nothing"
    );
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked again");
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::ClosePane(seat)),
        "the next close asks: the newer body is on no save"
    );
}

/// RED (S-6b) — **a window's close over two tabs, each with its save on the way, waits for both:
/// one wait, one bound, and the window goes once both have landed.**
///
/// The OS close finds both buffers' saves on the worker and waits. The first landing leaves the
/// other on its way: the window stays, nothing is asked, and it is the same wait — its bound has
/// not started again. The second landing re-runs the OS close from the top, which finds nothing to
/// ask: the window goes, and the other window stays.
///
/// MUTATION: in `Runtime::settle_close_wait`, decide once any buffer is decided (the wait goes on
/// only while every buffer is on its way) — the first landing re-runs the close, which waits again
/// under a new bound: the bound assertion goes red.
#[test]
fn a_window_close_over_two_saves_waits_for_both_and_goes() {
    let mut saving = Saving::open("two", &["甲 first.md", "乙 second.md"]);
    saving.press_save(0);
    saving.press_save(1);
    saving.application.window_event(WindowEvent::CloseRequested);
    assert_eq!(
        saving.asked(),
        None,
        "nothing is asked while both saves are landing"
    );
    let bound = saving.application.runtime().window.dirty_gate.waits_until();
    assert!(bound.is_some(), "the shut waits");

    saving.serve(None);
    saving.turn();
    assert_eq!(
        saving.asked(),
        None,
        "one landed, one on its way: still nothing asked"
    );
    assert!(!saving.leaving(1), "the window stays");
    assert_eq!(
        saving.application.runtime().window.dirty_gate.waits_until(),
        bound,
        "the same wait, under the same bound"
    );

    saving.serve(None);
    saving.turn();
    assert!(saving.leaving(1), "both landed: the window goes");
    assert!(!saving.leaving(2), "and the other window stays");
    for at in 0..2 {
        assert_eq!(
            saving.file(at),
            format!("{READ}{TYPED}"),
            "tab {at}: what was typed is in the file"
        );
    }
}

/// RED (S-6b) — **`Save all` on a window's question hands the saves to the worker and waits: the
/// window thread writes nothing, and the window goes once they have landed.**
///
/// No save is out when the window is closed, so the question is put; *Save all* sends the buffer's
/// save and holds the shut — the file still holds what was read, nothing is closed. The worker
/// writes it; on the turn its answer wakes, every save has landed and the window goes.
///
/// MUTATION: in `Runtime::answer_exit`, answer *Save all* by writing each dirty body on the
/// window thread (the synchronous writer S-6c removed) — the "nothing written" assertion goes red:
/// the window thread wrote the file.
#[test]
fn save_all_on_a_windows_question_waits_for_the_saves_and_goes() {
    let mut saving = Saving::open("save-all", &["全部 all.md"]);
    saving.application.window_event(WindowEvent::CloseRequested);
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::Shut),
        "nothing is out: it asks"
    );
    saving
        .application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Save)
        .expect("the answer is spent");
    assert_eq!(
        saving.file(0),
        READ,
        "nothing is written on the window thread"
    );
    assert!(saving.waits(), "the shut waits for the save");
    assert!(!saving.leaving(1));

    saving.serve(None);
    saving.turn();
    assert!(saving.leaving(1), "the save landed: the window goes");
    assert_eq!(
        saving.file(0),
        format!("{READ}{TYPED}"),
        "with what was typed in the file"
    );
}

// ── round 2: a clean buffer whose save has not landed (the coordinator's ruling on R1) ──

impl Saving {
    /// Undo tab `at`'s edit, back to the body the file held when it was read: the buffer is clean.
    fn undo(&mut self, at: usize) {
        let runtime = self.application.runtime();
        let pool = &mut runtime.window.tabs[at].preview_pool;
        let source = pool
            .buffers()
            .next()
            .expect("the tab's buffer")
            .source()
            .clone();
        let buffer = pool.get_mut(&source).expect("the tab's buffer");
        assert!(buffer.undo_edit().is_some(), "the edit is undone");
        assert!(!buffer.dirty(), "back to the file's own body: clean");
    }

    /// Press save on tab `at`, then undo the edit: a clean buffer whose save is on the worker.
    fn press_save_then_undo(&mut self, at: usize) {
        self.press_save(at);
        self.undo(at);
    }

    /// The title the question up in the first window is drawn with.
    fn asked_title(&mut self) -> &'static str {
        let runtime = self.application.runtime();
        let request = runtime
            .window
            .dirty_gate
            .request()
            .cloned()
            .expect("a question is up");
        crate::gate_title_over(&runtime.window.tabs, runtime.window.active_tab, &request)
    }
}

/// RED (S-6b round 2, R1) — **a clean buffer whose save is still on the worker holds the close
/// until the save lands; the landing leaves the file holding the body pressed, and the close then
/// asks about the body undone since.**
///
/// The person saves and undoes back to the file's own body: the buffer is clean, and its save is
/// on its way. Closing the pane would leave nobody to say how that save ended, so the close waits
/// — no question, the pane still there. The save lands: the file holds the body pressed and the
/// pane the body undone to, which is now in no file (S-6a's B1: the saved position is the one the
/// body was sent from). The close is re-run from the top and asks, in the edits' own words —
/// asking nothing would lose the undone body unasked.
///
/// MUTATION: in `PreviewBuffer::a_close_loses_something`, count only a dirty buffer — the
/// "waits" assertion goes red: the pane closes at once over the save on its way.
#[test]
fn a_clean_buffer_whose_save_is_out_holds_the_close_until_it_lands() {
    let mut saving = Saving::open("clean-lands", &["撤回 undone.md"]);
    saving.press_save_then_undo(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert_eq!(saving.asked(), None, "nothing is asked");
    assert!(
        saving.waits(),
        "the close waits for the clean buffer's save"
    );
    assert!(saving.pane_stands(0), "and the pane stays meanwhile");

    saving.serve(None);
    saving.turn();
    assert_eq!(
        saving.file(0),
        format!("{READ}{TYPED}"),
        "the save landed: the body pressed is in the file"
    );
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::ClosePane(seat)),
        "and the body undone since is in none: the re-run close asks"
    );
    assert_eq!(
        saving.asked_title(),
        restore::gate_title_text(),
        "about unsaved changes, which it now is"
    );
    assert!(saving.pane_stands(0), "the pane stays for the answer");
}

/// RED (S-6b round 2, R1) — **a clean buffer's save that meets another writer's bytes withdraws the
/// close, and the pane says the conflict.**
///
/// The close waits — and still waits on a turn before the answer — then the worker finds the file
/// changed under the save. The close is withdrawn: nothing is asked, the pane stays and says the
/// conflict, and the other writer's bytes are in the file.
///
/// MUTATION: in `PreviewBuffer::at_the_close`, honour an unanswered conflict only for a buffer
/// dirty at the close (`asked.dirty && …`) — the clean buffer's conflict reads as a save that did
/// not land, the re-run close asks, and the "asks nothing" assertion goes red.
#[test]
fn a_clean_buffers_save_that_meets_a_conflict_withdraws_the_close_and_says_so() {
    let mut saving = Saving::open("clean-conflict", &["撤回对照 undone-compare.md"]);
    saving.press_save_then_undo(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    saving.turn();
    assert!(saving.waits(), "before the answer the close still waits");
    assert!(saving.pane_stands(0));

    let theirs = "别人写的 by another writer\n";
    saving.serve(Some(theirs));
    saving.turn();
    assert_eq!(saving.asked(), None, "the conflict asks nothing");
    assert!(!saving.waits(), "the close is withdrawn");
    assert!(saving.pane_stands(0), "the pane stays");
    assert_eq!(
        saving.pane_says(0).as_deref(),
        Some(crate::preview::preview_conflict_notice()),
        "and says the conflict"
    );
    assert_eq!(
        saving.file(0),
        theirs,
        "the other writer's bytes are in the file"
    );
}

/// RED (S-6b round 2, R1) — **the worker gone under a clean buffer's save: the question, in words
/// that are true of it.**
///
/// The close waits; the worker ends with the save unanswered. The pane says it could not be
/// written; the re-run close asks — and its title is not "discard unsaved changes", which a clean
/// buffer has none of, but the fact: a save has not reached the disk. The file is named.
///
/// MUTATION: in `gate_title_over`, answer the request's own title always — the title assertion
/// goes red.
#[test]
fn the_worker_gone_under_a_clean_buffers_save_asks_in_words_that_are_true() {
    let mut saving = Saving::open("clean-gone", &["撤回日志 undone-log.md"]);
    saving.press_save_then_undo(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert!(saving.waits());

    saving.end_the_worker();
    saving.turn();
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::ClosePane(seat)),
        "the save did not land: the close asks"
    );
    assert_eq!(
        saving.asked_title(),
        restore::gate_save_not_landed_title(),
        "in words true of a clean buffer"
    );
    let names = saving
        .application
        .runtime()
        .gate_dirty_names(&restore::GateRequest::ClosePane(seat));
    assert_eq!(names.len(), 1, "naming its file: {names:?}");
    assert!(names[0].contains("undone-log.md"), "{names:?}");
    assert_eq!(
        saving.pane_says(0),
        Some(crate::i18n::not_saved(
            crate::i18n::Text::PreviewSaveNotStarted.text()
        )),
        "the pane said why, once"
    );
}

/// RED (S-6b round 2, R1; driven through the reaper in round 3) — **a tab whose shells have all
/// exited waits for its saves before the loop takes it away, and at the bound closes if only clean
/// buffers' saves are left; a late conflict is said once in `diagnostics.log`.**
///
/// Two tabs whose shells have exited (`PtySession::fake_exited`, long past the birth grace): the
/// first holds a clean buffer whose save is out, the second a dirty one whose save is out. The
/// loop's own cleanup (`Runtime::reap_exited_tabs`) takes the turn. Within the bound it closes
/// neither, and the loop will come back at the bound. At the bound the first closes and the second
/// stands — whether unsaved edits go is the reader's turn — with nothing left to come back for.
/// The first tab's save then meets another writer's bytes: its answer reaches the sink, which says
/// it once, naming the file.
///
/// MUTATION: in `ExitedTabWaits::ran_out`, close at once (`Some(_) =>` for
/// `Some(bound) if now >= bound =>`) — the first assertion goes red: the ended tab is torn down
/// with its save on the worker.
#[test]
fn an_ended_tab_waits_for_its_saves_and_closes_at_the_bound_when_only_clean_ones_are_left() {
    let mut saving = Saving::open("ended", &["终了 ended.md", "终了脏 ended-dirty.md"]);
    saving.press_save_then_undo(0);
    saving.press_save(1);
    {
        let runtime = saving.application.runtime();
        for tab in runtime.window.tabs.iter_mut() {
            for (_, leaf) in tab.leaves_mut() {
                leaf.pty = Some(bt_pty::PtySession::fake_exited(
                    std::time::Duration::from_secs(60),
                    0,
                    b"",
                    false,
                ));
            }
        }
    }
    let ids = |saving: &mut Saving| -> Vec<crate::TabId> {
        saving
            .application
            .runtime()
            .window
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect()
    };
    let start = Instant::now();
    saving
        .application
        .runtime()
        .reap_exited_tabs(start)
        .expect("the cleanup takes its turn");
    assert_eq!(
        ids(&mut saving),
        vec![crate::TabId(1), crate::TabId(2)],
        "within the bound neither ended tab is torn down"
    );
    let bound = saving
        .application
        .runtime()
        .window
        .exited_tab_waits
        .next_after(start)
        .expect("the loop comes back at the bound");
    assert!(bound >= start + restore::CLOSE_WAITS_FOR_A_SAVE);

    saving
        .application
        .runtime()
        .reap_exited_tabs(bound)
        .expect("the cleanup takes its turn at the bound");
    assert_eq!(
        ids(&mut saving),
        vec![crate::TabId(2)],
        "at the bound only the clean buffer's tab closes; the dirty one stands"
    );
    assert_eq!(
        saving
            .application
            .runtime()
            .window
            .exited_tab_waits
            .next_after(bound),
        None,
        "nothing is left to come back for"
    );

    saving.serve(Some("别人写的 by another writer\n"));
    saving.turn();
    let app = saving
        .application
        .folio
        .app
        .as_ref()
        .expect("the application");
    let said = app.preview_saves.sink_said();
    assert_eq!(said.len(), 1, "the late conflict is said once: {said:?}");
    assert!(said[0].contains("ended.md"), "naming its file: {said:?}");
}

/// RED (S-6b round 3, the review's blocker) — **`Save all` over a body undone since its save asks
/// before the window goes.**
///
/// One window, two tabs: the first's buffer was saved and then undone back to the file's own
/// body (clean, its save out), the second's is dirty with nothing sent. The window's close asks;
/// *Save all* sends the dirty one and waits for both. Both land: the file of the first now holds
/// the body pressed, and the body undone to is in no file. The shut is re-run from the top and asks
/// about it — the window stays, and nothing is lost unasked.
///
/// MUTATION: in `Runtime::settle_close_wait`'s `saving` branch, close whenever every save landed
/// (drop the `still_losing` re-run) — the "the window stays" assertion goes red: the window goes
/// and the undone body with it.
#[test]
fn save_all_over_a_body_undone_since_its_save_asks_before_the_window_goes() {
    let mut saving = Saving::open(
        "save-all-undone",
        &["撤回全部 undone-all.md", "脏 dirty.md"],
    );
    saving.press_save_then_undo(0);
    saving.application.window_event(WindowEvent::CloseRequested);
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::Shut),
        "the dirty buffer has nothing on its way: it asks"
    );
    saving
        .application
        .runtime()
        .answer_dirty_gate(restore::GateAnswer::Save)
        .expect("the answer is spent");
    assert!(saving.waits(), "the shut waits for both saves");

    saving.serve(None);
    saving.serve(None);
    saving.turn();
    assert_eq!(
        saving.file(0),
        format!("{READ}{TYPED}"),
        "the first save landed"
    );
    assert_eq!(
        saving.file(1),
        format!("{READ}{TYPED}"),
        "and so did the second"
    );
    assert!(
        !saving.leaving(1),
        "the window stays: the body undone since the first save is in no file"
    );
    assert_eq!(
        saving.asked(),
        Some(restore::GateRequest::Shut),
        "the shut, re-run, asks about it"
    );
    assert_eq!(saving.asked_title(), restore::gate_title_text());
}

/// RED (S-6b round 4) — **a close's wait is not decided while a quit is under way; once the quit is
/// cancelled, the next turn decides it.**
///
/// The pane's close waits for its save. A quit begins: the quit's card is the one question, so the
/// save landing under it closes nothing — the wait stands and the pane stays. The quit is
/// cancelled; on the next turn the wait is decided, and the pane closes.
///
/// MUTATION: drop the quit guard at the head of `FolioApp::settle_close_waits` — the "the wait
/// stands" assertion goes red: the wait closes the pane mid-quit.
#[test]
fn a_close_wait_is_not_decided_while_a_quit_is_under_way() {
    let mut saving = Saving::open("under-a-quit", &["退出 quitting.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    assert!(saving.waits());
    saving
        .application
        .folio
        .app
        .as_mut()
        .expect("the application")
        .quit = Some(crate::quit::Quit::begin(vec![
        "退出 quitting.md".to_owned(),
    ]));

    saving.serve(None);
    saving.turn();
    assert!(saving.waits(), "the wait stands under the quit");
    assert!(saving.pane_stands(0), "and no pane is closed mid-quit");

    saving
        .application
        .folio
        .app
        .as_mut()
        .expect("the application")
        .quit = None;
    saving.turn();
    assert!(!saving.waits(), "the quit cancelled, the wait is decided");
    assert!(
        !saving.pane_stands(0),
        "the save had landed: the pane closes"
    );
}

/// RED (S-6b round 4) — **a close's bound joins the loop's wake-up only where the wait is decided:
/// never in a leaving window, never under a quit.**
///
/// A pane's close waits for its save, so its bound is the loop's next wake-up. Under a quit, or once
/// the window is leaving, the wait is not decided (`FolioApp::settle_close_waits`), so its bound
/// would be a wake-up in the past on every turn: it is left out.
///
/// MUTATION: in `FolioApp::close_waits_wake`, drop the quit check — the "under a quit" assertion
/// goes red; drop the `leaving.is_none()` filter — the "leaving" assertion goes red.
#[test]
fn a_close_bound_joins_the_wake_up_only_where_the_wait_is_decided() {
    let mut saving = Saving::open("wake", &["醒 wake.md"]);
    saving.press_save(0);
    let seat = saving.seats[0];
    saving
        .application
        .runtime()
        .close_pane(seat)
        .expect("the close is asked");
    let now = Instant::now();
    let bound = saving.application.folio.close_waits_wake(now);
    assert!(
        bound.is_some_and(|bound| bound > now),
        "the bound is the next wake-up"
    );

    let app = saving
        .application
        .folio
        .app
        .as_mut()
        .expect("the application");
    app.quit = Some(crate::quit::Quit::begin(vec!["醒 wake.md".to_owned()]));
    assert_eq!(
        saving.application.folio.close_waits_wake(now),
        None,
        "under a quit no close's bound wakes the loop"
    );
    saving
        .application
        .folio
        .app
        .as_mut()
        .expect("the application")
        .quit = None;

    saving
        .application
        .folio
        .windows
        .get_mut(WindowId::from(1))
        .expect("the window")
        .leaving = Some(now);
    assert_eq!(
        saving.application.folio.close_waits_wake(now),
        None,
        "a leaving window's bound does not wake the loop"
    );
}

// ── the quit's `Save` and a stop that cannot ask, on the storage lane (S-6c) ─────────────────

/// **Two windows, each holding one tab with one preview of a real file typed into** — the first
/// window's tab `1`, the second's tab `2` — their saves handed to a storage lane whose queue the
/// test serves (`persist::StorageLane::scripted`: the real lane, the test the worker).
struct Quitting {
    application: HeadlessApplication,
    jobs: crate::persist::ScriptedJobs,
    /// The file behind each window's buffer: window 1's, then window 2's.
    paths: Vec<std::path::PathBuf>,
    /// The preview seat of each window's tab.
    seats: Vec<bt_layout::SeatId>,
    names: Vec<String>,
    dir: std::path::PathBuf,
}

impl Quitting {
    fn open(tag: &str, names: [&str; 2]) -> Self {
        let dir = crate::test_support::disk_scratch(&format!("quit-saves-{tag}"));
        let mut tabs = Vec::new();
        let mut seats = Vec::new();
        let mut paths = Vec::new();
        for (at, name) in names.iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, READ).expect("the file is written");
            let mut buffer = crate::test_support::buffer_read_from(&path);
            assert!(buffer.edit_content(|body| {
                body.push_str(TYPED);
                true
            }));
            let (tab, seat) = tab_with_a_preview(at as u64 + 1, vec![buffer]);
            tabs.push(tab);
            seats.push(seat);
            paths.push(path);
        }
        let second = tabs.pop().expect("the second tab");
        let mut folio = HeadlessFolio::with_tabs(tag, tabs, 0);
        let (lane, jobs) = crate::persist::StorageLane::scripted();
        folio.app.preview_saves = crate::preview::PreviewSaves::on(lane);
        let other = headless_window(
            &folio.app,
            HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
            vec![second],
            0,
        );
        let mut application = HeadlessApplication::of(folio);
        application.folio.windows.insert(WindowId::from(2), other);
        let names = names
            .iter()
            .map(|name| crate::files_row_display_name(&dir.join(name)))
            .collect();
        Self {
            application,
            jobs,
            paths,
            seats,
            names,
            dir,
        }
    }

    /// Window `1` or `2`.
    fn runtime(&mut self, window: u64) -> crate::Runtime<'_, crate::headless::Headless> {
        self.application
            .folio
            .runtime(WindowId::from(window))
            .expect("the window is open")
    }

    fn app(&mut self) -> &mut crate::App<crate::headless::Headless> {
        self.application
            .folio
            .app
            .as_mut()
            .expect("the application")
    }

    /// **The quit, asked and answered `Save`**, and the loop's next look at it.
    fn quit_with_save(&mut self) {
        let names = self.names.clone();
        let app = self.app();
        let mut quit = crate::quit::Quit::begin(names);
        quit.answer(crate::quit::QuitAnswer::Save);
        app.quit = Some(quit);
        self.settle();
    }

    /// The loop's look at the quit (`FolioApp::settle_quit`).
    fn settle(&mut self) {
        self.application
            .folio
            .settle_quit(&self.application.event_loop)
            .expect("the quit is spent");
    }

    /// **One turn after the worker's wake**: the answers heard by the buffers
    /// (`AppEvent::DocumentsAnswered`), then the turn's look at the quit.
    fn turn(&mut self) {
        self.application
            .folio
            .take_document_answers()
            .expect("the answers are heard");
        self.settle();
    }

    /// What the quit owes now, if one is under way.
    fn step(&mut self) -> Option<crate::quit::QuitStep> {
        self.app().quit.as_ref().map(crate::quit::Quit::step)
    }

    /// Whether the quit has photographed every window and written the session.
    fn went_on(&mut self) -> bool {
        self.app()
            .quit
            .as_ref()
            .is_some_and(crate::quit::Quit::document_is_written)
    }

    /// The cards on window `window`, by what they say.
    fn cards(&mut self, window: u64) -> Vec<String> {
        self.runtime(window)
            .window
            .toasts
            .toasts()
            .iter()
            .map(|toast| toast.body().to_owned())
            .collect()
    }

    /// Whether window `window`'s buffer is dirty.
    fn dirty(&mut self, window: u64) -> bool {
        self.runtime(window).window.tabs[0]
            .preview_pool
            .buffers()
            .next()
            .expect("the tab's buffer")
            .dirty()
    }

    /// Type into window `window`'s buffer once more.
    fn type_again(&mut self, window: u64) {
        let runtime = self.runtime(window);
        let pool = &mut runtime.window.tabs[0].preview_pool;
        let source = pool
            .buffers()
            .next()
            .expect("the tab's buffer")
            .source()
            .clone();
        let buffer = pool.get_mut(&source).expect("the tab's buffer");
        assert!(buffer.edit_content(|body| {
            body.push_str("once more\n");
            true
        }));
    }

    /// What window `window`'s pane says, if anything.
    fn pane_says(&mut self, window: u64) -> Option<String> {
        let at = window as usize - 1;
        let surface = crate::test_support::seat_of(crate::TabId(window), self.seats[at]);
        self.runtime(window)
            .preview_pane(surface)
            .and_then(|pane| pane.notice().map(|(said, _)| said.clone()))
    }

    /// What the file behind window `window`'s buffer holds.
    fn file(&self, window: u64) -> String {
        std::fs::read_to_string(&self.paths[window as usize - 1]).expect("the file reads back")
    }

    /// **The worker serves the next save**, `crossing` written into the file under it.
    fn serve(&self, crossing: Option<&str>) {
        let job = crate::test_support::next_document(&self.jobs);
        let _ = crate::test_support::serve_on_a_worker(
            job,
            crossing.map(|bytes| bytes.as_bytes().to_vec()),
        );
    }

    /// **The worker ends** with every save it holds unanswered.
    fn end_the_worker(&mut self) {
        let (_, unserved) = crate::persist::StorageLane::scripted();
        drop(std::mem::replace(&mut self.jobs, unserved));
    }

    /// The copies in the test's `recovered` folder, by what they hold.
    fn copies(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.dir.join(crate::preview::RECOVERED_FOLDER)) else {
            return Vec::new();
        };
        let mut held: Vec<String> = entries
            .map(|entry| {
                std::fs::read_to_string(entry.expect("an entry").path()).expect("the copy reads")
            })
            .collect();
        held.sort();
        held
    }
}

impl Drop for Quitting {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// RED (S-6c) — **the quit's `Save` over two windows writes nothing on the window thread: both
/// saves go to the storage worker, the quit waits for them, its bound in the loop's wake-up, and
/// once both have landed it goes on to the session write.**
///
/// MUTATION: in `Runtime::hand_over_every_save`, write each dirty body on the window thread in
/// place of `save_every_dirty_preview` (the synchronous writer put back) — the "nothing is
/// written" assertion goes red; in `quit::Quit::deadline`, drop the save step's arm — the bound
/// assertion goes red.
#[test]
fn a_quits_save_hands_both_windows_saves_to_the_worker_and_goes_on_when_they_land() {
    let mut quitting = Quitting::open("lands", ["one.md", "two.md"]);
    let before = Instant::now();
    quitting.quit_with_save();
    assert_eq!(
        (quitting.file(1), quitting.file(2)),
        (READ.to_owned(), READ.to_owned()),
        "nothing is written on the window thread"
    );
    assert_eq!(
        quitting.step(),
        Some(crate::quit::QuitStep::Save),
        "the quit waits for its saves"
    );
    let bound = quitting
        .app()
        .quit
        .as_ref()
        .and_then(crate::quit::Quit::deadline);
    assert!(
        bound.is_some_and(|bound| bound >= before + crate::quit::QUIT_WAITS_FOR_ITS_SAVES),
        "its bound is the loop's next wake-up: {bound:?}"
    );

    quitting.serve(None);
    quitting.turn();
    assert_eq!(
        quitting.step(),
        Some(crate::quit::QuitStep::Save),
        "one has landed: it still waits"
    );
    quitting.serve(None);
    quitting.turn();
    assert!(
        quitting.went_on(),
        "both landed: the quit went on to the session write"
    );
    assert_eq!(
        (quitting.file(1), quitting.file(2)),
        (format!("{READ}{TYPED}"), format!("{READ}{TYPED}")),
        "with what was typed in both files"
    );
}

/// RED (S-6c) — **a conflict withdraws the quit: the pane says it, nothing is written over the
/// other writer's bytes, the other window's save lands, and the card names the one that did not.**
///
/// MUTATION: in `FolioApp::settle_the_quits_saves`, go on whenever every save has answered (the
/// `left.is_empty()` check answering `true`) — the "withdrawn" assertion goes red.
#[test]
fn a_conflict_withdraws_the_quit_and_its_pane_says_it() {
    let mut quitting = Quitting::open("conflict", ["one.md", "two.md"]);
    quitting.quit_with_save();
    let theirs = "another writer\n";
    quitting.serve(Some(theirs));
    quitting.serve(None);
    quitting.turn();
    assert_eq!(quitting.step(), None, "the quit is withdrawn");
    assert_eq!(
        quitting.pane_says(1).as_deref(),
        Some(crate::preview::preview_conflict_notice()),
        "the pane says the conflict"
    );
    assert_eq!(quitting.file(1), theirs, "their bytes are in the file");
    assert!(quitting.dirty(1), "and the edit is still in the window");
    assert_eq!(
        quitting.file(2),
        format!("{READ}{TYPED}"),
        "the other window's save landed"
    );
    assert_eq!(
        quitting.cards(1),
        [crate::i18n::quit_not_saved(&quitting.names[0])],
        "the window's card names the one that did not"
    );
    assert_eq!(quitting.cards(2), Vec::<String>::new(), "and only that one");
}

/// RED (S-6c) — **the worker gone with both saves unanswered: the quit is withdrawn, and each
/// window's card names its own buffer.**
///
/// MUTATION: in `FolioApp::settle_the_quits_saves`, say the cards on the first window named only
/// (`break` after the first `say_the_quits_saves`) — the second window's assertion goes red.
#[test]
fn the_worker_gone_withdraws_the_quit_and_each_card_names_its_buffer() {
    let mut quitting = Quitting::open("gone", ["one.md", "two.md"]);
    quitting.quit_with_save();
    quitting.end_the_worker();
    quitting.turn();
    assert_eq!(quitting.step(), None, "the quit is withdrawn");
    assert_eq!(
        quitting.cards(1),
        [crate::i18n::quit_not_saved(&quitting.names[0])]
    );
    assert_eq!(
        quitting.cards(2),
        [crate::i18n::quit_not_saved(&quitting.names[1])]
    );
    assert_eq!(
        (quitting.file(1), quitting.file(2)),
        (READ.to_owned(), READ.to_owned())
    );
}

/// RED (S-6c) — **at the bound with the saves still out, the quit is withdrawn saying, once, that
/// they are still on their way; their late landing is heard by their buffers, and says nothing
/// more.**
///
/// The turn is taken at the bound: the clock is the turn's, never waited for.
///
/// MUTATION: in `restore::SavesFound::of`, never find the bound (`OnTheirWay` whatever `now`) —
/// the "withdrawn" assertion goes red: the quit waits for ever.
#[test]
fn at_the_bound_the_quit_says_its_saves_are_still_on_their_way() {
    let mut quitting = Quitting::open("bound", ["one.md", "two.md"]);
    quitting.quit_with_save();
    let late = Instant::now() + crate::quit::QUIT_WAITS_FOR_ITS_SAVES;
    assert!(
        quitting
            .application
            .folio
            .settle_the_quits_saves(late)
            .expect("the quit's saves are looked at"),
        "the bound decides"
    );
    quitting.settle();
    assert_eq!(quitting.step(), None, "the quit is withdrawn");
    for window in [1, 2] {
        let name = quitting.names[window as usize - 1].clone();
        assert_eq!(
            quitting.cards(window),
            [crate::i18n::quit_save_on_its_way(&name)],
            "window {window}: in true words"
        );
    }

    quitting.serve(None);
    quitting.serve(None);
    quitting.turn();
    assert!(
        !quitting.dirty(1) && !quitting.dirty(2),
        "the late landings are heard by their buffers"
    );
    assert_eq!(quitting.cards(1).len(), 1, "and nothing more is said");
}

/// RED (S-6c) — **an edit while the quit waits supersedes its save: the quit is withdrawn at once,
/// saying so once; the next quit asks again.**
///
/// MUTATION: in `restore::SavesFound::of`, drop the `Edited` arm — the "withdrawn" assertion goes
/// red: the quit goes on waiting for the other window's save.
#[test]
fn an_edit_while_the_quit_waits_withdraws_it_and_the_next_quit_asks() {
    let mut quitting = Quitting::open("edited", ["one.md", "two.md"]);
    quitting.quit_with_save();
    quitting.type_again(1);
    quitting.settle();
    assert_eq!(quitting.step(), None, "the quit is withdrawn");
    assert_eq!(
        quitting.cards(1),
        [crate::i18n::quit_not_saved(&quitting.names[0])],
        "said once"
    );

    quitting.serve(None);
    quitting.serve(None);
    quitting.turn();
    assert!(
        quitting.dirty(1),
        "the body sent landed; the new one is not saved"
    );
    assert!(!quitting.dirty(2));
    assert_eq!(quitting.cards(1).len(), 1, "and nothing more is said");

    quitting.app().ask_to_quit();
    quitting.settle();
    let app = quitting.app();
    let asking = app.quit.as_ref().expect("a quit is asked");
    assert!(asking.is_asking(), "the next quit asks");
    assert_eq!(
        asking.names().len(),
        1,
        "about the edited buffer: {:?}",
        asking.names()
    );
}

/// RED (S-6c) — **a quit's saves pending while a close's question is up: the question stays, a
/// second close meets a busy gate, and the quit goes on once its saves land.**
///
/// MUTATION: in `restore::DirtyGate::is_busy`, answer only for a close that waits
/// (`self.waiting.is_some()`) — the busy assertion goes red.
#[test]
fn a_quit_pending_beside_a_closes_question_leaves_the_question_alone() {
    let mut quitting = Quitting::open("busy", ["one.md", "two.md"]);
    quitting
        .runtime(1)
        .window
        .dirty_gate
        .open(restore::GateRequest::CloseTab(0));
    quitting.quit_with_save();
    let raised = {
        let runtime = quitting.runtime(1);
        let window = &mut *runtime.window;
        crate::raise_dirty_gate_over(
            &mut window.dirty_gate,
            &window.tabs,
            window.active_tab,
            restore::GateRequest::Shut,
        )
    };
    assert_eq!(
        raised,
        restore::GateRaise::Busy,
        "a second close meets a busy gate"
    );
    assert_eq!(
        quitting.runtime(1).window.dirty_gate.request(),
        Some(&restore::GateRequest::CloseTab(0)),
        "and the question up is the one that was"
    );

    quitting.serve(None);
    quitting.serve(None);
    quitting.turn();
    assert!(quitting.went_on(), "the saves landed: the quit went on");
}

/// RED (S-6c) — **a stop that cannot ask hands its saves to the storage worker: the window thread
/// writes nothing over a file, and once the worker has served its queue only what did not land
/// is copied into `recovered`.**
///
/// Both windows are stopped (`FolioApp::stop_every_window`, into this test's own folder): the two
/// saves are on the lane and neither file has changed. The worker serves them before its end —
/// the first lands, the second meets another writer's bytes — and the stop's second half
/// (`App::finish`, after the session's close) keeps the second edit as a copy, never over their
/// file.
///
/// MUTATION: in `Runtime::keep_unsaved_edits`, hand nothing over (drop the
/// `save_every_dirty_preview` call, holding the pools only) — the first window's file still
/// holds what was read, and two copies are made.
#[test]
fn a_stop_hands_its_saves_to_the_worker_and_copies_only_what_did_not_land() {
    let mut quitting = Quitting::open("stop", ["one.md", "two.md"]);
    let recovery = quitting.dir.join(crate::preview::RECOVERED_FOLDER);
    quitting
        .application
        .folio
        .stop_every_window_keeping_in(&recovery)
        .expect("the windows stop");
    assert_eq!(
        (quitting.file(1), quitting.file(2)),
        (READ.to_owned(), READ.to_owned()),
        "nothing is written on the window thread"
    );
    let theirs = "another writer\n";
    quitting.serve(None);
    quitting.serve(Some(theirs));
    quitting.app().finish();
    assert_eq!(
        quitting.file(1),
        format!("{READ}{TYPED}"),
        "the first landed"
    );
    assert_eq!(quitting.file(2), theirs, "their bytes stay in the second");
    assert_eq!(
        quitting.copies(),
        [format!("{READ}{TYPED}")],
        "and only the second edit is copied"
    );
}

/// RED (S-6c) — **a save the worker never answers before the stop leaves is kept as a copy too.**
///
/// The worker is let go at its bound with both saves on its queue (the test serves none). The
/// file may yet take them; the copies keep both edits whatever it does.
///
/// MUTATION: in `PreviewPool::kept_after_the_stop`, keep nothing of a buffer whose save is still
/// out (`if buffer.on_disk.is_out() { continue; }`) — no copy is made.
#[test]
fn a_stop_whose_saves_are_still_out_keeps_both_edits_in_copies() {
    let mut quitting = Quitting::open("stop-out", ["one.md", "two.md"]);
    let recovery = quitting.dir.join(crate::preview::RECOVERED_FOLDER);
    quitting
        .application
        .folio
        .stop_every_window_keeping_in(&recovery)
        .expect("the windows stop");
    quitting.app().finish();
    assert_eq!(
        quitting.copies(),
        [format!("{READ}{TYPED}"), format!("{READ}{TYPED}")],
        "both edits are kept"
    );
    assert_eq!(
        (quitting.file(1), quitting.file(2)),
        (READ.to_owned(), READ.to_owned())
    );
}

// ── The window's doors to the system, driven (T-NATIVE-HANDLE-BEHIND-THE-SEAM) ──────────────────
//
// One test per group of the doors that replaced the window's platform handle: the window's place
// on the desktop (read), its dress (told), the keyboard and the foreground, attention, the taskbar
// button, the system's news, and a page's place for the web engine. Each drives the runtime's own
// code over a headless window, whose questions answer what the test scripts
// (`headless::PlatformAnswers`) and whose instructions land on its ledger.

/// One headless window holding a single terminal tab, its ledger emptied.
fn one_window(tag: &str) -> HeadlessFolio {
    HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf_saying("门 door"))], 0)
}

/// The window's questions answering `answers`.
fn answering(folio: &HeadlessFolio, answers: PlatformAnswers) {
    folio.window.window.answers.set(answers);
}

const RECT: bt_platform::WindowRect = bt_platform::WindowRect {
    left: 100,
    top: 50,
    right: 900,
    bottom: 650,
};

/// RED — **where the window is on the desktop is read through its own doors**: iconic and
/// cloaked each make the turn's reading hidden, a covered window reads unexposed, the snapshot
/// measures the rectangle the window door names, the work area is the display's as the door names
/// it, the glass under a point and the pointer are the door's answers.
///
/// MUTATION: make `window_is_hidden` read `false` instead of `window.is_iconic() ||
/// window.is_cloaked()` — the first two readings go red; make `window_snapshot` measure nothing
/// (`measured = None`) — the bounds go red; take the door out of `refresh_work_area` — the work
/// area stays unknown.
#[test]
fn the_windows_place_is_read_through_its_doors() {
    let mut folio = one_window("seam-door-place");
    answering(
        &folio,
        PlatformAnswers {
            iconic: true,
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    assert!(
        folio.window.observed_place.hidden,
        "an iconic window is hidden"
    );
    assert!(folio.runtime().window_is_iconic());

    answering(
        &folio,
        PlatformAnswers {
            cloaked: true,
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    assert!(
        folio.window.observed_place.hidden,
        "a cloaked window is hidden"
    );

    answering(
        &folio,
        PlatformAnswers {
            exposed: false,
            outer_rect: Some(RECT),
            work_area: Some(RECT),
            glass: crate::GlassHere::Theirs,
            pointer: Some((12, 34)),
            ..PlatformAnswers::default()
        },
    );
    folio.runtime().observe_window_place();
    let place = folio.window.observed_place;
    assert!(
        !place.hidden && !place.exposed,
        "a covered window is on the screen and unexposed: {place:?}"
    );
    let snapshot = folio.runtime().window_snapshot();
    assert_eq!(
        snapshot.placement.bounds,
        crate::persisted_window_bounds(RECT, 1.0),
        "the snapshot measures the rectangle the window door names"
    );
    folio.runtime().refresh_work_area();
    assert_eq!(
        folio.window.work_area,
        bt_layout::WorkAreaHint::Known(bt_layout::LogicalSize::px(800, 600)),
        "the work area is the display's, as the window door names it"
    );
    assert_eq!(
        folio.runtime().glass_here(PhysicalPosition::new(5.0, 6.0)),
        crate::GlassHere::Theirs
    );
    assert_eq!(
        folio.runtime().platform_pointer_now(),
        Some(PhysicalPosition::new(12.0, 34.0))
    );
}

/// RED — **the window's dress is told through its doors, with its arguments**: always-on-top,
/// the system backdrop, the frame's dark plate and the class background each land as the effect
/// the window would have had.
///
/// MUTATION: drop the `set_topmost` call from `apply_always_on_top` — `Topmost(true)` is missing;
/// make `install_theme_class_background` tell `None` — the background's colour goes red.
#[test]
fn the_windows_dress_is_told_through_its_doors() {
    let mut folio = one_window("seam-door-dress");
    folio.app.acrylic_available = true;
    folio.window.dwm_dark_mode = None;
    let mut runtime = folio.runtime();
    runtime
        .apply_always_on_top(true)
        .expect("the row is answered");
    runtime.apply_acrylic(true).expect("the row is answered");
    runtime
        .apply_window_dark_mode()
        .expect("the plate is answered");
    crate::install_theme_class_background(&folio.window.window);
    let dark = !bt_render::background_is_light(bt_render::background_rgb());
    let opaque = bt_render::window_ground().alpha >= 1.0;
    let effects = folio.ledger.take();
    let told: Vec<&WindowEffect> = effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                WindowEffect::Topmost(_)
                    | WindowEffect::Backdrop(_)
                    | WindowEffect::DarkMode(_)
                    | WindowEffect::ClassBackground(_)
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            &WindowEffect::Topmost(true),
            &WindowEffect::Backdrop(true),
            &WindowEffect::DarkMode(dark),
            &WindowEffect::ClassBackground(opaque.then(crate::background_rgb)),
        ],
        "every instruction, in the order it was given: {effects:?}"
    );
}

/// RED — **a raised window is restored before it takes the foreground**, and the keyboard, the
/// close and a page's keyboard all go through the window's doors.
///
/// MUTATION: swap `restore_minimized_window(window)?` below the foreground in
/// `bring_owned_window_forward` — the order goes red; make `take_owned_keyboard_focus` answer `Ok`
/// without `window.take_foreground()` — `ForegroundTaken` is missing and the refusal is not said.
#[test]
fn a_raised_window_is_restored_before_it_takes_the_foreground() {
    let folio = one_window("seam-door-foreground");
    answering(
        &folio,
        PlatformAnswers {
            minimized: Some(true),
            ..PlatformAnswers::default()
        },
    );
    crate::bring_owned_window_forward(&folio.window.window).expect("the system lets it");
    assert_eq!(
        folio.ledger.take(),
        [
            WindowEffect::Minimized(false),
            WindowEffect::ForegroundTaken
        ],
        "restored, then given the foreground"
    );
}

/// RED — **the keyboard is taken through the foreground door**, and a refusal from the system is
/// said; the close is asked through the window's door; a page's find report takes the keyboard
/// back into the window while the capsule's field is focused.
///
/// MUTATION: make `take_owned_keyboard_focus` answer `Ok(())` — the refusal goes red; drop the
/// `take_keyboard_focus` call from `web_find_reported` — `KeyboardFocusTaken` is missing.
#[test]
fn the_keyboard_is_taken_through_the_foreground_door() {
    let mut folio = one_window("seam-door-keyboard");
    folio
        .runtime()
        .give_foreground_with_retry()
        .expect("the system lets it");
    assert_eq!(folio.ledger.take(), [WindowEffect::ForegroundTaken]);
    answering(
        &folio,
        PlatformAnswers {
            takes_the_foreground: false,
            ..PlatformAnswers::default()
        },
    );
    assert!(
        folio.runtime().give_foreground_with_retry().is_err(),
        "a window the system would not give the keyboard says so"
    );
    folio.runtime().request_window_close().expect("asked");
    assert_eq!(
        folio.ledger.take(),
        [WindowEffect::ForegroundTaken, WindowEffect::CloseRequested]
    );
    let seat = folio.window.tabs[0].seats.identity();
    folio
        .runtime()
        .open_search(seat)
        .expect("the capsule opens");
    folio.ledger.take();
    folio
        .runtime()
        .web_find_reported(1, 1)
        .expect("the engine's count is taken");
    assert!(
        folio
            .ledger
            .take()
            .contains(&WindowEffect::KeyboardFocusTaken),
        "the keyboard comes back into the window while the field is focused"
    );
}

/// RED — **a flash for attention goes to the window's own button**: a delivery whose reach is the
/// taskbar flashes it, once for the turn's deliveries.
///
/// MUTATION: drop `self.window.window.flash()` from `raise_attention` — `Flashed` is missing.
#[test]
fn a_flash_for_attention_goes_through_the_windows_door() {
    let mut folio = one_window("seam-door-attention");
    let tab = folio.window.tabs[0].id;
    let seat = folio.window.tabs[0].seats.identity();
    let delivery = |title: &str| crate::AttentionDelivery {
        tab,
        seat,
        reach: crate::attention::Reach::Flash,
        why: crate::attention::Why::Awaiting,
        title: title.to_owned(),
        body: None,
    };
    folio
        .runtime()
        .raise_attention(vec![delivery("等你 waiting"), delivery("second")])
        .expect("the deliveries are answered");
    let effects = folio.ledger.take();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| **effect == WindowEffect::Flashed)
            .count(),
        1,
        "one flash for the turn's deliveries: {effects:?}"
    );
}

/// RED — **the taskbar button is told each new progress through the window's door**, once, and
/// never the reading it already shows.
///
/// MUTATION: make `TaskbarMirror::show` build no button (`self.refused = true`) — nothing is told.
#[test]
fn the_taskbar_button_is_told_through_the_windows_door() {
    let folio = one_window("seam-door-taskbar");
    let mut mirror = crate::TaskbarMirror::default();
    let half = bt_platform::TaskbarProgress {
        state: bt_platform::TaskbarProgressState::Normal,
        value: Some((50, 100)),
    };
    for wanted in [bt_platform::TaskbarProgress::CLEARED, half, half] {
        mirror.show(&folio.window.window, wanted);
    }
    assert_eq!(folio.ledger.take(), [WindowEffect::TaskbarProgress(half)]);
}

/// RED — **a window is born listening to the system** through its own doors: the preference
/// broadcasts and the end-of-session question are each asked for once, as the window is built.
///
/// MUTATION: drop the `watch_session_end` call from `new_window_runtime` — `SessionEndWatched` is
/// missing.
#[test]
fn a_window_is_born_listening_to_the_system() {
    let folio = one_window("seam-door-news");
    let ledger = Ledger::default();
    let parts = HeadlessParts::new(&folio.app.gpu, 2, (800, 600), 1.0, &ledger);
    let window = headless_window(
        &folio.app,
        parts,
        vec![tab_holding(leaf_saying("新 new"))],
        0,
    );
    let effects = ledger.take();
    for asked in [
        WindowEffect::SystemSettingsWatched,
        WindowEffect::SessionEndWatched,
    ] {
        assert_eq!(
            effects.iter().filter(|effect| **effect == asked).count(),
            1,
            "{asked:?} once: {effects:?}"
        );
    }
    drop(window);
}

/// RED — **a page is opened on the place the window door names**: the page's own name in this
/// window is asked of the window, and a window that hosts no engine answers the open with its
/// refusal.
///
/// MUTATION: open the seat without asking the window for the page's place — `PageAddressed` is
/// missing.
#[test]
fn a_page_is_opened_on_the_place_the_window_door_names() {
    let mut folio = one_window("seam-door-page");
    let leaf = crate::LeafId {
        tab: folio.window.tabs[0].id,
        seat: folio.window.tabs[0].seats.identity(),
    };
    let refused = folio
        .runtime()
        .open_web_page_on(leaf, "https://example.com/页", crate::webnav::Mint::Nothing)
        .expect_err("a headless window hosts no web engine");
    assert!(
        refused.to_string().contains("no web engine"),
        "the window's own refusal: {refused:#}"
    );
    assert_eq!(
        folio.ledger.take(),
        [WindowEffect::PageAddressed(bt_platform::PageVisual {
            tab: leaf.tab.0,
            seat: leaf.seat.0,
        })]
    );
}
