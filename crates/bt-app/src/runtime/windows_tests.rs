//! **The windows module's door for a re-requested close, driven**
//! (T-DISPATCHER-WRITES-INTO-DOORS): the close an exit's answer asks for is taken by the event
//! loop through [`crate::Runtime::take_close_request`], once, on a headless window.
//!
//! And a close that waits for the saves on their way (T-CLOSE-GATE-WAITS, S-6b): a pane's, a tab's
//! or a window's close over buffers whose saves are on the storage worker, held by the gate across
//! turns and decided on the saves' answers, on a headless window and the real storage lane.

use crate::headless::{HeadlessApplication, HeadlessFolio, HeadlessParts, headless_window};
use crate::test_support::{buffer_saying, host_spelling, ringing_tab, tab_with_a_preview};
use crate::{preview_edit, restore};
use std::time::Instant;
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
/// MUTATION: in `Runtime::answer_exit`, answer *Save all* through the quit's synchronous writer
/// again (`self.quit_save()?.is_complete()` for `saved_all`) — the "nothing written" assertion
/// goes red: the window thread wrote the file.
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
