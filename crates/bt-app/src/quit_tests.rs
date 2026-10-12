//! **The quit's documents and recovery phases** (T-QUIT-DOCUMENTS; design note
//! `docs/plans/design/settings-reread-2026-10-11.md`, revisions (c)2 and (d)2), driven
//! through the real stores over a scripted storage lane: every job the stores and the
//! session hand over arrives in the order it was queued, and the test answers it — the
//! scripted disk. The clock is handed in; nothing here waits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use bt_persist::{BindingOverrideV1, PinKind, SettingsV1};

use super::*;
use crate::document_request::{
    DocumentAnswer, DocumentJob, Outcome, QuitDocument, RecoveredJob, Refusal,
};
use crate::persist::{
    KeybindingsStore, ScriptedJob, ScriptedJobs, SessionStore, SettingsStore, StorageLane,
};
use crate::pins::PinsStore;
use crate::{persist, preview};

/// A private folder for one test, under the suite's scratch root.
fn scratch(case: &str) -> PathBuf {
    let root = bt_testpath::temp_path(&format!("bt-app-quit-documents-{case}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a private storage folder for this test");
    root
}

/// A settings document whose terminal face is `face`.
fn settings(face: &str) -> SettingsV1 {
    SettingsV1 {
        terminal_font_family: face.to_owned(),
        ..SettingsV1::default()
    }
}

fn bytes(document: &SettingsV1) -> Arc<[u8]> {
    bt_persist::serialize_settings(document)
        .expect("a settings document serializes")
        .into()
}

/// What happened, in the order it happened: the jobs as the worker would take them,
/// the lines `diagnostics.log` was given, and the steps the quit asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Seen {
    /// A document's write, by file name.
    Write(String),
    /// A document's copy into `recovered`, by file name.
    Copy(String),
    Session,
    /// An update trial's receipt.
    Receipt,
    Note(String),
    Step(QuitStep),
}

/// The scripted disk: the lane's queue, the jobs taken from it and not yet answered,
/// and everything seen.
struct Disk {
    jobs: ScriptedJobs,
    writes: Vec<DocumentJob>,
    copies: Vec<RecoveredJob>,
    seen: Vec<Seen>,
}

impl Disk {
    fn new(jobs: ScriptedJobs) -> Self {
        Self {
            jobs,
            writes: Vec::new(),
            copies: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// Take every job queued since the last look, in order.
    fn look(&mut self) {
        while let Some(job) = self.jobs.next() {
            match job {
                ScriptedJob::Document(job) => {
                    self.seen.push(Seen::Write(file_of(&job.which)));
                    self.writes.push(job);
                }
                ScriptedJob::Recovered(job) => {
                    self.seen.push(Seen::Copy(job.name.to_owned()));
                    self.copies.push(job);
                }
                ScriptedJob::Session => self.seen.push(Seen::Session),
                ScriptedJob::Receipt => self.seen.push(Seen::Receipt),
                ScriptedJob::Read | ScriptedJob::Other => {}
            }
        }
    }

    /// Answer the oldest unanswered write with `outcome(job)`.
    fn answer_write(&mut self, outcome: impl FnOnce(&DocumentJob) -> Outcome) {
        self.look();
        assert!(
            !self.writes.is_empty(),
            "a write is on the worker: {:?}",
            self.seen
        );
        let job = self.writes.remove(0);
        let outcome = outcome(&job);
        job.answer
            .send(DocumentAnswer {
                generation: job.generation,
                version: job.version,
                asker: job.asker,
                outcome,
                not_durable: None,
                retired: Vec::new(),
                left: None,
            })
            .expect("the store waits for its answer");
    }

    /// Answer the oldest unanswered copy with `kept`.
    fn answer_copy(&mut self, kept: Result<PathBuf, String>) {
        self.look();
        assert!(
            !self.copies.is_empty(),
            "a copy is on the worker: {:?}",
            self.seen
        );
        let job = self.copies.remove(0);
        job.answer.send(kept).expect("the store waits for its copy");
    }

    fn sessions(&self) -> usize {
        self.seen
            .iter()
            .filter(|seen| **seen == Seen::Session)
            .count()
    }
}

fn file_of(path: &Path) -> String {
    path.file_name()
        .expect("a document has a name")
        .to_string_lossy()
        .into_owned()
}

/// The write landed: the file holds its bytes.
fn landed(job: &DocumentJob) -> Outcome {
    Outcome::Landed {
        bytes: Arc::clone(&job.bytes),
        kept: None,
    }
}

/// The disk refused the write.
fn refused(_: &DocumentJob) -> Outcome {
    Outcome::Refused {
        cause: Refusal::Disk("磁盘已满 the disk is full".to_owned()),
    }
}

/// **Drive `quit` as `FolioApp::settle_quit` drives it**, until it waits across turns
/// (the documents or the recovery phase with an answer owed) or is over: each phase
/// through the turn the driver calls, the session write handed to the same lane
/// (`SessionStore::hand_over_final`, the write's own door onto the worker, which does not
/// stand still for the receipt) and judged landed, the windows' steps answered at once.
fn drive(
    quit: &mut Quit,
    documents: &mut [&mut dyn QuitDocument],
    session: &mut SessionStore,
    disk: &mut Disk,
    folder: &Path,
    now: Instant,
) -> QuitStep {
    loop {
        let step = quit.step();
        disk.look();
        disk.seen.push(Seen::Step(step));
        let next = match step {
            QuitStep::Documents => quit.documents_turn(documents, now),
            QuitStep::Recovery => {
                quit.recovery_turn(documents, folder, SystemTime::UNIX_EPOCH, now)
            }
            QuitStep::Photograph => quit.photographed(),
            QuitStep::Write => {
                session
                    .hand_over_final(now)
                    .expect("the session document is handed over");
                disk.look();
                quit.written(WriteVerdict::Landed)
            }
            QuitStep::Retire => quit.retired(now),
            QuitStep::WaitForPages => quit.pages(true, now),
            QuitStep::Ask
            | QuitStep::Save
            | QuitStep::Discard
            | QuitStep::Exit
            | QuitStep::Abandon => return step,
        };
        disk.look();
        if next == step && matches!(step, QuitStep::Documents | QuitStep::Recovery) {
            return step;
        }
    }
}

/// A settings store and a session store over one scripted lane, in a private folder.
fn settings_and_session(case: &str) -> (PathBuf, SettingsStore, SessionStore, Disk) {
    let root = scratch(case);
    let (lane, jobs) = StorageLane::scripted();
    let settings_store = SettingsStore::at_on(root.join(persist::SETTINGS_FILE_NAME), lane.clone());
    let session = SessionStore::at_on(root.join("session.json"), root.join("session.lock"), lane);
    (root, settings_store, session, Disk::new(jobs))
}

/// RED (T-QUIT-DOCUMENTS, revision (c)2) — **a `Moved` or `Displaced` answer at quit is
/// merged as the running window merges it, sent again, and landed — all before the
/// session is queued.**
///
/// A change of Folio's (the terminal face) is on its way when the quit begins. The disk
/// answers `Moved` (a hand edit turned inline formulas off), then `Displaced` (another hand
/// edit, kept beside the file, turned display formulas off): each is folded into the
/// document, purely, and the merged document sent again, conditional on the file the
/// merge read. Only once it has landed is the session handed to the worker.
///
/// MUTATION: queue the session job at the start of the documents phase (in
/// `Quit::documents`, go on to the photograph whatever is still waiting) — the session
/// is queued behind the first write, before the merged ones.
#[test]
fn a_moved_or_displaced_answer_at_quit_is_merged_and_landed_before_the_session() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("merged");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let ours = settings("我们的 Ours Mono");
    assert!(settings_store.store(ours.clone()));
    let now = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Documents,
        "the write is on the worker"
    );

    let mut theirs = SettingsV1::default();
    theirs.inline_formulas = !theirs.inline_formulas;
    let theirs_bytes = bytes(&theirs);
    disk.answer_write(|_| Outcome::Moved {
        bytes: Some(Arc::clone(&theirs_bytes)),
    });
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Documents,
        "the merged document is sent again"
    );
    assert_eq!(
        disk.writes[0].expected.as_deref(),
        Some(&theirs_bytes[..]),
        "conditional on the file the merge read"
    );

    let mut displaced = theirs.clone();
    displaced.display_formulas = !displaced.display_formulas;
    let displaced_bytes = bytes(&displaced);
    disk.answer_write(|_| Outcome::Displaced {
        bytes: Arc::clone(&displaced_bytes),
        kept: root.join("settings.json.displaced-交叉 crossed"),
    });
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Documents,
        "the displaced edit is merged and sent again"
    );
    assert_eq!(disk.sessions(), 0, "and the session waits behind it");
    disk.answer_write(landed);
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Exit
    );

    let writes: Vec<usize> = disk
        .seen
        .iter()
        .enumerate()
        .filter(|(_, seen)| matches!(seen, Seen::Write(_)))
        .map(|(at, _)| at)
        .collect();
    let session_at = disk
        .seen
        .iter()
        .position(|seen| *seen == Seen::Session)
        .expect("the session is written");
    assert_eq!(
        writes.len(),
        3,
        "the change, then two merges: {:?}",
        disk.seen
    );
    assert!(
        writes.iter().all(|&write| write < session_at),
        "every document write is queued before the session: {:?}",
        disk.seen
    );
    let on_disk = &settings_store.revision().document;
    assert_eq!(
        on_disk.terminal_font_family, ours.terminal_font_family,
        "ours"
    );
    assert_eq!(
        on_disk.inline_formulas, theirs.inline_formulas,
        "the moved edit's"
    );
    assert_eq!(
        on_disk.display_formulas, displaced.display_formulas,
        "the displaced edit's"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (c)2) — **a document the disk refused is copied into
/// `recovered` first, and the session is queued only behind the copy's landing.**
///
/// MUTATION: queue the copy behind the session job (in `Quit::documents`, go on to the
/// photograph with a terminal answer among the stores, the copies left until after the
/// session write) — the session is queued with no copy before it.
#[test]
fn a_refused_document_is_copied_first_and_the_session_waits_behind_the_copy() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("copied");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let ours = settings("拒绝 Refused Mono");
    assert!(settings_store.store(ours.clone()));
    let now = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    drive(
        &mut quit,
        &mut documents,
        &mut session,
        &mut disk,
        &folder,
        now,
    );
    disk.answer_write(refused);
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Recovery,
        "the copy is on the worker"
    );
    assert_eq!(disk.sessions(), 0, "the session waits behind the copy");
    let copy = &disk.copies[0];
    assert_eq!(copy.name, persist::SETTINGS_FILE_NAME);
    assert_eq!(copy.folder, folder);
    assert_eq!(&copy.bytes[..], &bytes(&ours)[..], "the change itself");
    disk.answer_copy(Ok(folder.join("1970-01-01T000000Z settings.json")));
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Exit
    );
    let copy_at = disk
        .seen
        .iter()
        .position(|seen| *seen == Seen::Copy("settings.json".into()));
    let session_at = disk.seen.iter().position(|seen| *seen == Seen::Session);
    assert!(
        matches!((copy_at, session_at), (Some(copy), Some(session)) if copy < session),
        "the copy first, the session behind it: {:?}",
        disk.seen
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (c)2) — **a document still unanswered at
/// `QUIT_DOCUMENTS_BY` cancels the quit**: the worker is inside the write, a copy queued
/// behind it could not land, and the window stays.
///
/// MUTATION: issue the copy at the expired deadline and proceed (revision (b)'s step 4:
/// in `Quit::documents`, read a store still waiting at the bound as a terminal answer) —
/// a copy is queued and the quit goes on.
#[test]
fn an_unanswered_document_at_the_phase_deadline_cancels_the_quit() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("unanswered");
    let folder = root.join(preview::RECOVERED_FOLDER);
    assert!(settings_store.store(settings("未答 Unanswered Mono")));
    let start = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            start
        ),
        QuitStep::Documents
    );
    assert_eq!(
        quit.deadline(),
        Some(start + QUIT_DOCUMENTS_BY),
        "the phase's bound"
    );
    let late = start + QUIT_DOCUMENTS_BY - std::time::Duration::from_millis(1);
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            late
        ),
        QuitStep::Documents,
        "the disk is still holding the write"
    );
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            start + QUIT_DOCUMENTS_BY
        ),
        QuitStep::Abandon,
        "the quit is cancelled: {:?}",
        disk.seen
    );
    let cancelled = quit.take_cancelled().expect("the cancellation is named");
    assert_eq!(cancelled.files(), vec![persist::SETTINGS_FILE_NAME]);
    assert!(
        disk.copies.is_empty(),
        "no copy queued behind the write: {:?}",
        disk.seen
    );
    assert_eq!(disk.sessions(), 0, "and no session");
    let _ = std::fs::remove_dir_all(&root);
}

/// Walk one quit to its cancellation by a refused copy: the write refused, the copy
/// refused. Answers the cancellation.
fn cancelled_by_a_refused_copy(
    quit: &mut Quit,
    documents: &mut [&mut dyn QuitDocument],
    session: &mut SessionStore,
    disk: &mut Disk,
    folder: &Path,
    now: Instant,
) -> Cancelled {
    drive(quit, documents, session, disk, folder, now);
    disk.answer_write(refused);
    assert_eq!(
        drive(quit, documents, session, disk, folder, now),
        QuitStep::Recovery
    );
    disk.answer_copy(Err("拒绝写入 the folder refused the copy".to_owned()));
    assert_eq!(
        drive(quit, documents, session, disk, folder, now),
        QuitStep::Abandon,
        "a refused copy cancels the quit: {:?}",
        disk.seen
    );
    quit.take_cancelled().expect("the cancellation is named")
}

/// RED (T-QUIT-DOCUMENTS, revision (c)2) — **a copy the disk refused cancels the quit**:
/// the change is in no file and no copy, and the window stays, the card naming the file.
///
/// MUTATION: proceed on the first quit (in `Quit::recovery`, go on to the photograph past a
/// refused copy) — the session is queued and the quit leaves.
#[test]
fn a_refused_copy_cancels_the_quit() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("copy-refused");
    let folder = root.join(preview::RECOVERED_FOLDER);
    assert!(settings_store.store(settings("无处 Nowhere Mono")));
    let now = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    let cancelled = cancelled_by_a_refused_copy(
        &mut quit,
        &mut documents,
        &mut session,
        &mut disk,
        &folder,
        now,
    );
    assert_eq!(cancelled.files(), vec![persist::SETTINGS_FILE_NAME]);
    assert!(
        cancelled.lines()[0].contains("the folder refused the copy"),
        "the full reason goes to diagnostics.log: {:?}",
        cancelled.lines()
    );
    assert_eq!(disk.sessions(), 0, "no session was queued");
    assert!(
        !disk.seen.contains(&Seen::Step(QuitStep::Retire)),
        "no window was hidden"
    );
    assert!(
        settings_store.standing() != Standing::Settled,
        "the change is still owed"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (d)2) — **a quit asked again after a cancellation runs
/// the transaction again, and does not leave.** It is not a discard: the document is sent
/// again from the documents phase; a disk still refusing cancels it again, and a disk that
/// has recovered lets it complete.
///
/// MUTATION: revision (c)'s rule (in `quit::dropped_on_begin`, drop the documents a
/// cancellation named whether or not its card's choice was made) — the second quit drops
/// the change and leaves.
#[test]
fn a_repeated_quit_runs_the_transaction_again_and_does_not_leave() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("repeated");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let ours = settings("再试 Again Mono");
    assert!(settings_store.store(ours.clone()));
    let now = Instant::now();
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    let mut quit = Quit::begin(Vec::new());
    let cancelled = cancelled_by_a_refused_copy(
        &mut quit,
        &mut documents,
        &mut session,
        &mut disk,
        &folder,
        now,
    );

    // The quit asked again — the menu, the shortcut, a close — with no choice made.
    let dropped = dropped_on_begin(Some(cancelled));
    assert!(
        drop_documents(&mut documents, &dropped).is_empty(),
        "nothing is dropped"
    );
    let writes_before = disk
        .seen
        .iter()
        .filter(|seen| matches!(seen, Seen::Write(_)))
        .count();
    let mut again = Quit::begin(Vec::new());
    let cancelled = cancelled_by_a_refused_copy(
        &mut again,
        &mut documents,
        &mut session,
        &mut disk,
        &folder,
        now,
    );
    let writes_after = disk
        .seen
        .iter()
        .filter(|seen| matches!(seen, Seen::Write(_)))
        .count();
    assert_eq!(
        writes_after,
        writes_before + 1,
        "the document was sent again"
    );
    assert_eq!(
        cancelled.files(),
        vec![persist::SETTINGS_FILE_NAME],
        "the same card"
    );
    assert_eq!(disk.sessions(), 0, "and it did not leave: {:?}", disk.seen);

    // Asked a third time, over a disk that has recovered: the quit simply completes.
    assert!(dropped_on_begin(Some(cancelled)).is_empty());
    let mut third = Quit::begin(Vec::new());
    drive(
        &mut third,
        &mut documents,
        &mut session,
        &mut disk,
        &folder,
        now,
    );
    disk.answer_write(landed);
    assert_eq!(
        drive(
            &mut third,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Exit
    );
    assert_eq!(
        settings_store.revision().document.terminal_font_family,
        ours.terminal_font_family,
        "the change was saved, not dropped"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, revision (d)2) — **only *Leave without this change* leaves, and
/// it drops only the documents its card named.**
///
/// Two stores owe a change: `settings.json`'s write is refused and so is its copy;
/// `pins.json`'s lands. The quit is cancelled naming `settings.json` alone. Dismissing the
/// card chooses nothing. The person pins another folder; then presses the choice: the
/// quit that follows drops `settings.json`'s change — named in `diagnostics.log`, with its
/// units, before the way out — and saves the pin.
///
/// MUTATIONS: drop every owed document (in `quit::drop_documents`, ignore `files`) — the
/// pin made after the card is dropped too; leave on the card's dismissal (in
/// `quit::Raised::press`, answer the choice for its `×`) — the dismissal chooses.
#[test]
fn only_leave_without_this_change_leaves_and_drops_only_the_named_documents() {
    let root = scratch("leave-without");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let (lane, jobs) = StorageLane::scripted();
    let mut disk = Disk::new(jobs);
    let mut settings_store =
        SettingsStore::at_on(root.join(persist::SETTINGS_FILE_NAME), lane.clone());
    let mut pins = PinsStore::at_on(root.join(crate::pins::PINS_FILE_NAME), lane.clone());
    let mut session =
        SessionStore::at_on(root.join("session.json"), root.join("session.lock"), lane);
    let now = Instant::now();

    let mut ours = settings("离开 Leave Mono");
    ours.theme_mode = bt_persist::ThemeModeV1::Dark;
    assert!(settings_store.store(ours));
    assert!(pins.toggle(PinKind::Folder, "项目/folio 笔记"));
    disk.look();
    // The pin's write is the second on the worker; the settings write the first.
    let mut quit = Quit::begin(Vec::new());
    let cancelled = {
        let mut documents: [&mut dyn QuitDocument; 2] = [&mut settings_store, &mut pins];
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now,
        );
        disk.answer_write(refused);
        disk.answer_write(landed);
        assert_eq!(
            drive(
                &mut quit,
                &mut documents,
                &mut session,
                &mut disk,
                &folder,
                now
            ),
            QuitStep::Recovery
        );
        disk.answer_copy(Err("拒绝 refused".to_owned()));
        assert_eq!(
            drive(
                &mut quit,
                &mut documents,
                &mut session,
                &mut disk,
                &folder,
                now
            ),
            QuitStep::Abandon
        );
        quit.take_cancelled().expect("the cancellation is named")
    };
    assert_eq!(
        cancelled.files(),
        vec![persist::SETTINGS_FILE_NAME],
        "only the one not kept"
    );

    let raised = Raised {
        card: crate::toast::ToastId(7),
        cancelled,
    };
    assert_eq!(
        raised.press(ToastHit::Close(crate::toast::ToastId(7))),
        None,
        "dismissing the card is not a choice"
    );
    assert_eq!(raised.press(ToastHit::Card(crate::toast::ToastId(7))), None);
    assert_eq!(
        raised.press(ToastHit::Action(crate::toast::ToastId(8))),
        None,
        "another card's verb"
    );

    // A pin made after the card: owed, and not the card's to drop.
    assert!(pins.toggle(PinKind::Folder, "第二个 second"));
    let chosen = raised
        .press(ToastHit::Action(crate::toast::ToastId(7)))
        .expect("the card's verb is the choice");
    let mut leaving = Quit::begin_for(Vec::new(), chosen.resumed_reason());
    {
        let mut documents: [&mut dyn QuitDocument; 2] = [&mut settings_store, &mut pins];
        for line in drop_documents(&mut documents, &dropped_on_begin(Some(chosen))) {
            disk.seen.push(Seen::Note(line));
        }
        drive(
            &mut leaving,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now,
        );
        disk.answer_write(landed);
        assert_eq!(
            drive(
                &mut leaving,
                &mut documents,
                &mut session,
                &mut disk,
                &folder,
                now
            ),
            QuitStep::Exit,
            "the choice leaves: {:?}",
            disk.seen
        );
    }
    let note_at = disk
        .seen
        .iter()
        .position(|seen| matches!(seen, Seen::Note(line) if line.contains("settings.json") && line.contains("theme_mode")))
        .unwrap_or_else(|| panic!("the dropped change is named with its units: {:?}", disk.seen));
    let exit_at = disk
        .seen
        .iter()
        .rposition(|seen| *seen == Seen::Step(QuitStep::Exit))
        .expect("the way out");
    assert!(note_at < exit_at, "named before the way out");
    assert_eq!(
        disk.seen
            .iter()
            .filter(|seen| matches!(seen, Seen::Note(_)))
            .count(),
        1,
        "only the named document is dropped: {:?}",
        disk.seen
    );
    assert_eq!(
        settings_store.standing(),
        Standing::Settled,
        "dropped for this run"
    );
    assert_eq!(
        pins.revision().document.pins.len(),
        2,
        "the pin landed, the one made after the card with it"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS, carry-forward 1; ticket 4's rule) — **a document held for its
/// merge is copied, never sent, at quit**: its `desired` goes straight to `recovered`, and
/// the hold is never "resolved" by writing over the file.
///
/// `keybindings.json` has no merge yet: a hand edit crossed its write, was put back
/// (`Conflict`), and the store is held.
///
/// MUTATION: send the held document in the documents phase (in `QuitDocument::quit_begin`
/// for `DocumentState`, release the hold and send) — a write is queued over the person's
/// file and the phase waits for it.
#[test]
fn a_held_document_is_copied_not_sent_at_quit() {
    let root = scratch("held");
    let folder = root.join(preview::RECOVERED_FOLDER);
    let (lane, jobs) = StorageLane::scripted();
    let mut disk = Disk::new(jobs);
    let mut keybindings =
        KeybindingsStore::at_on(root.join(persist::KEYBINDINGS_FILE_NAME), lane.clone());
    let mut session =
        SessionStore::at_on(root.join("session.json"), root.join("session.lock"), lane);
    let ours = vec![BindingOverrideV1 {
        action: "new-tab".to_owned(),
        chord: Some("Ctrl+Shift+Y".to_owned()),
    }];
    assert!(keybindings.store(ours));
    let theirs: Arc<[u8]> = b"{\"schema_version\":1,\"bindings\":[{\"action\":\"close-tab\",\"chord\":\"Ctrl+W\"}]}"[..].into();
    disk.answer_write(|_| Outcome::Conflict {
        theirs: Arc::clone(&theirs),
        kept: None,
    });
    let _ = keybindings.take_answers();
    let desired = keybindings
        .owed_bytes()
        .expect("the change is owed")
        .expect("and serializes");
    disk.seen.clear();

    let now = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut keybindings];
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Recovery,
        "held is terminal: straight to the copy"
    );
    assert!(
        !disk.seen.iter().any(|seen| matches!(seen, Seen::Write(_))),
        "nothing is written over the person's file: {:?}",
        disk.seen
    );
    assert_eq!(
        &disk.copies[0].bytes[..],
        &desired[..],
        "the copy is the change"
    );
    disk.answer_copy(Ok(folder.join("1970-01-01T000000Z keybindings.json")));
    assert_eq!(
        drive(
            &mut quit,
            &mut documents,
            &mut session,
            &mut disk,
            &folder,
            now
        ),
        QuitStep::Exit
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Walk a quit whose `settings.json` write is refused to the copy on the worker, the copy
/// unanswered: the documents phase at `start`, its end at `phase_two`.
fn to_a_copy_on_the_worker(
    quit: &mut Quit,
    documents: &mut [&mut dyn QuitDocument],
    disk: &mut Disk,
    folder: &Path,
    start: Instant,
    phase_two: Instant,
) {
    assert_eq!(quit.documents_turn(documents, start), QuitStep::Documents);
    disk.answer_write(refused);
    assert_eq!(
        quit.documents_turn(documents, phase_two),
        QuitStep::Recovery
    );
    assert_eq!(
        quit.recovery_turn(documents, folder, SystemTime::UNIX_EPOCH, phase_two),
        QuitStep::Recovery,
        "the copy is on the worker"
    );
}

/// RED (T-QUIT-DOCUMENTS round 2, review blocker 1) — **the session write spends only what
/// is left of the quit's one budget.** The recovery phase ends at 2.4 s; the session write's
/// wait then ends at the documents phase's start plus `SESSION_SAVE_BUDGET` — 0.6 s later,
/// not a fresh 3 s — for the ordinary quit's judged flush and an update's receipt alike.
///
/// MUTATION: a fresh budget (`Quit::session_write_until` answers `now +
/// SESSION_SAVE_BUDGET`) — the bound is 3 s from the end of the recovery phase.
#[test]
fn the_session_write_spends_only_what_remains_of_the_one_budget() {
    let (root, mut settings_store, _session, mut disk) = settings_and_session("one-budget");
    let folder = root.join(preview::RECOVERED_FOLDER);
    assert!(settings_store.store(settings("预算 Budget Mono")));
    let start = Instant::now();
    let late = start + std::time::Duration::from_millis(2_400);
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    to_a_copy_on_the_worker(
        &mut quit,
        &mut documents,
        &mut disk,
        &folder,
        start,
        start + std::time::Duration::from_millis(1_000),
    );
    disk.answer_copy(Ok(folder.join("1970-01-01T000000Z settings.json")));
    assert_eq!(
        quit.recovery_turn(&mut documents, &folder, SystemTime::UNIX_EPOCH, late),
        QuitStep::Photograph
    );
    assert_eq!(quit.photographed(), QuitStep::Write);
    assert_eq!(
        quit.session_write_until(late) - late,
        std::time::Duration::from_millis(600),
        "what is left of the one budget, and no more"
    );

    // An update's quit: its receipt is held to the same instant.
    let txn = crate::update_txn::TxnId::new([0x21; 16]);
    let mut update = Quit::begin_for(Vec::new(), Reason::UpdateRestart { txn });
    assert_eq!(update.documents(&[], start), QuitStep::Photograph);
    assert_eq!(update.photographed(), QuitStep::Write);
    update.requested(1, late);
    assert_eq!(
        update.deadline(),
        Some(start + persist::SESSION_SAVE_BUDGET),
        "the receipt's bound is the budget's end"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS round 2, review blocker 1) — **a quit with no budget left leaves
/// without a new flush**: the session is not handed over, nothing is waited for, and the way
/// out's close does not flush it either; the write is given up as a timed-out one, so the
/// quit goes on and `session.lock` stays.
///
/// MUTATION: flush anew (in `SessionStore::flush_for_quit`, judge a fresh flush when the
/// budget is spent) — a session document is handed to the worker.
#[test]
fn a_quit_with_no_budget_left_closes_without_a_new_flush() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("no-budget");
    let folder = root.join(preview::RECOVERED_FOLDER);
    assert!(settings_store.store(settings("耗尽 Spent Mono")));
    session.record(
        crate::test_support::shells_document(&root, 1),
        Instant::now(),
    );
    let start = Instant::now();
    let spent = start + persist::SESSION_SAVE_BUDGET;
    let mut quit = Quit::begin(Vec::new());
    let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
    to_a_copy_on_the_worker(&mut quit, &mut documents, &mut disk, &folder, start, start);
    disk.answer_copy(Ok(folder.join("1970-01-01T000000Z settings.json")));
    // The copy's receipt read on a turn that came late, with nothing of the budget left.
    assert_eq!(
        quit.recovery_turn(&mut documents, &folder, SystemTime::UNIX_EPOCH, spent),
        QuitStep::Photograph
    );
    assert_eq!(quit.photographed(), QuitStep::Write);
    let until = quit.session_write_until(spent);
    assert!(until <= spent, "no budget is left");
    let verdict = session.flush_for_quit(until, spent);
    assert!(
        verdict
            .as_ref()
            .is_err_and(persist::SaveRefusal::quit_may_proceed),
        "given up as a write whose budget ran out, which leaves: {verdict:?}"
    );
    assert_eq!(quit.written(WriteVerdict::TimedOut), QuitStep::Retire);
    session.close();
    disk.look();
    assert_eq!(
        disk.sessions(),
        0,
        "no session was handed over: {:?}",
        disk.seen
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-QUIT-DOCUMENTS round 2, review blocker 2) — **an update trial's receipt, written at
/// its first text while a quit holds the storage worker, goes ahead of the copies.** It is the
/// one job admitted while the documents and recovery phases hold the worker: a small
/// create-new write the trial is rolled back without, served before every job still queued.
///
/// MUTATION: the receipt queued behind (`SessionWriter::send_receipt` through
/// `submit_storage_job`, the queue's own order) — the copy is served first.
#[test]
fn first_text_during_the_documents_phase_writes_the_trial_receipt_ahead_of_the_copies() {
    let (root, mut settings_store, mut session, mut disk) = settings_and_session("receipt");
    let folder = root.join(preview::RECOVERED_FOLDER);
    assert!(settings_store.store(settings("收据 Receipt Mono")));
    let start = Instant::now();
    let mut quit = Quit::begin(Vec::new());
    {
        let mut documents: [&mut dyn QuitDocument; 1] = [&mut settings_store];
        to_a_copy_on_the_worker(&mut quit, &mut documents, &mut disk, &folder, start, start);
    }
    assert!(quit.holds_the_storage_worker());
    // The first pane text of an update's trial, presented now.
    let answer = session.write_receipt(crate::update_trial::ReceiptJob {
        path: root.join("trial-receipt 试用.json"),
        bytes: b"{\"trial\":\"receipt\"}".to_vec(),
    });
    assert!(answer.is_some(), "a worker takes it");
    disk.look();
    let receipt_at = disk.seen.iter().position(|seen| *seen == Seen::Receipt);
    let copy_at = disk
        .seen
        .iter()
        .position(|seen| *seen == Seen::Copy(persist::SETTINGS_FILE_NAME.to_owned()));
    assert!(
        matches!((receipt_at, copy_at), (Some(receipt), Some(copy)) if receipt < copy),
        "the receipt is served ahead of the copy: {:?}",
        disk.seen
    );
    let _ = std::fs::remove_dir_all(&root);
}
