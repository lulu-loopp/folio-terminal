//! **The edits a stop kept, said at the next start** (T-RECOVERED-FOLDER; owner ruling
//! 2026-10-09).
//!
//! A stop that cannot ask (the controlled failure road, `FolioApp::stop_every_window`) keeps an
//! unsaved preview edit the file would not take as a copy in the data directory's
//! [`crate::preview::RECOVERED_FOLDER`], and says so in `diagnostics.log` only: nothing is
//! announced while the failure is happening. The next start says it once, in one toast — "An
//! unsaved edit was kept at <folder>", or "2 unsaved edits were kept at …" — and the card's one
//! verb opens that folder in the system's file manager. Nothing in the folder is ever removed by
//! Folio: it is the person's work.
//!
//! **Which copies have been said** is the file [`ANNOUNCED_RECORD`] in the data directory, beside
//! the folder and never in it (the folder the verb opens holds the person's edits and nothing
//! of Folio's): a JSON array of the names of the copies the folder held when it was last said.
//! It is written whole ([`bt_platform::install_txn::durable_write_on_worker`]) **before** the toast is
//! raised, so a start that dies before or after its toast has already recorded the copies, and a
//! run of starts that each fail never says the same copy twice. A record that cannot be written
//! raises no toast (a copy would otherwise be said at every start). A record that cannot be
//! parsed is read as an empty set: each copy present is said once more, and the record written
//! afresh.
//!
//! **Off the window thread.** The folder is listed and the record read and written on the
//! `bt-recovered` worker ([`begin`]), started once per process at the first frame a window puts
//! on the glass — nothing is asked before the first frame — and only by the data directory's
//! writer. The listing is [`copies_in`], a worker's door (`window_waits.tsv` `# effects`,
//! `worker-door-body` / `WorkerCtx`). The answer waits in this module's slot and the worker wakes
//! the loop (`AppEvent::RecoveredEditsListed`); the window thread takes it ([`take`]) and raises
//! the card on the window the keyboard is on.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use bt_platform::admission::WorkerCtx;

use crate::toast::{ToastHit, ToastId};

/// **The record of the copies already said**, in the data directory beside
/// [`crate::preview::RECOVERED_FOLDER`].
pub const ANNOUNCED_RECORD: &str = "recovered-announced.json";

/// **What a start has to say**: how many copies in `folder` have not been said before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub folder: PathBuf,
    pub count: usize,
}

impl Announcement {
    /// The toast's one sentence, in the language in force.
    pub fn sentence(&self) -> String {
        crate::i18n::recovered_edits_kept(self.count, &self.folder.display().to_string())
    }
}

/// **The card that said it, and the folder its verb opens.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raised {
    pub card: ToastId,
    pub folder: PathBuf,
}

impl Raised {
    /// The card raised for `announcement`.
    pub fn of(card: ToastId, announcement: &Announcement) -> Self {
        Self {
            card,
            folder: announcement.folder.clone(),
        }
    }

    /// **The folder the card's verb asks the system to open**, through the window's one reveal
    /// (`Runtime::reveal_in_explorer`, the files column's foot's door, which opens a folder as
    /// itself in Explorer or Finder). Only the verb, as on every card that has one: a press on
    /// the card's body is swallowed, its `×` closes it, and another card's verb is that card's.
    pub fn press(&self, hit: ToastHit) -> Option<&Path> {
        match hit {
            ToastHit::Action(card) if card == self.card => Some(&self.folder),
            ToastHit::Action(_) | ToastHit::Card(_) | ToastHit::Close(_) => None,
        }
    }
}

/// **Look at `folder` and say what has not been said** — the worker's whole question, on the
/// worker.
///
/// Lists the copies ([`copies_in`]); none, or no folder, is nothing to say. Reads the names
/// already said from `record` (absent is none; unparsable is none, so the record is repaired by
/// the write below). When a copy is new, the record becomes the names the folder holds now —
/// written whole, before anything is said — and the answer is how many are new.
///
/// # Errors
/// The folder could not be listed, the record could not be read, or it could not be written;
/// the sentence names which. Nothing is said then.
fn look(worker: &WorkerCtx, folder: &Path, record: &Path) -> Result<Option<Announcement>, String> {
    let present = match copies_in(worker, folder) {
        Ok(present) => present,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} could not be listed: {error}", folder.display())),
    };
    let announced: BTreeSet<String> =
        match bt_platform::file_reads::read(bt_platform::file_reads::Lane::Settings, record) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => return Err(format!("{} could not be read: {error}", record.display())),
        };
    let count = present.difference(&announced).count();
    if count == 0 {
        return Ok(None);
    }
    let bytes = serde_json::to_vec(&present).expect("a set of strings is JSON");
    bt_platform::install_txn::durable_write_on_worker(worker, record, &bytes).map_err(
        |failure| {
            format!(
                "{} could not be written ({failure}); nothing is said, so nothing is said twice",
                record.display()
            )
        },
    )?;
    Ok(Some(Announcement {
        folder: folder.to_path_buf(),
        count,
    }))
}

/// **The names of the files in `folder`**, on a worker: the door the listing goes through. A
/// folder in it is not a copy. A name is compared as text: one that is not Unicode is read with
/// its replacement characters, the same spelling at every start.
fn copies_in(_worker: &WorkerCtx, folder: &Path) -> io::Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            names.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

/// What the worker found, waiting for the window thread. Written by [`begin`]'s worker, taken by
/// [`take`].
static ANSWER: Mutex<Option<Announcement>> = Mutex::new(None);

/// Whether this process has asked. Once per process: a second window's first frame asks nothing.
static ASKED: AtomicBool = AtomicBool::new(false);

/// How the worker asks the event loop for a turn once the answer is in the slot.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the event loop's wake, before [`begin`] can be called.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

/// **Ask once per process, at the first frame on the glass.** The data directory's writer asks;
/// any other process writes nothing there and says nothing. In an update's trial the record is
/// a durable write, so the question is held back with the trial's other writers and asked when
/// the trial is committed ([`ask`], from `App::release_trial_writes`).
pub fn begin(data: &Path) {
    if ASKED.swap(true, Ordering::Relaxed) || !crate::persist::is_storage_writer() {
        return;
    }
    if crate::update_trial::defer(crate::update_trial::Writer::RecoveredAnnouncement) {
        return;
    }
    ask(data);
}

/// **Start the worker that looks at `data`'s recovered folder.** It publishes an answer only
/// when there is something to say, and wakes the loop after publishing it, never before.
pub fn ask(data: &Path) {
    let folder = data.join(crate::preview::RECOVERED_FOLDER);
    let record = data.join(ANNOUNCED_RECORD);
    let spawned = bt_platform::spawn_at_priority(
        "bt-recovered",
        bt_platform::ThreadPriority::BelowNormal,
        move |worker| match look(worker, &folder, &record) {
            Ok(None) => {}
            Ok(Some(announcement)) => {
                *ANSWER
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(announcement);
                if let Some(wake) = WAKE.get() {
                    wake();
                }
            }
            Err(why) => crate::diagnostics::note(&format!("BT_RECOVERED {why}")),
        },
    );
    if let Err(error) = spawned {
        crate::diagnostics::note(&format!(
            "BT_RECOVERED the recovered folder was not looked at: no thread ({error})"
        ));
    }
}

/// **What the worker found, once**: the window thread's one read of the slot.
pub fn take() -> Option<Announcement> {
    ANSWER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `body` on a worker the thread door started, and wait for it.
    fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        match bt_platform::spawn_at_priority(
            "bt-recovered-test",
            bt_platform::ThreadPriority::BelowNormal,
            body,
        )
        .expect("the thread door starts a thread")
        .join()
        {
            Ok(answer) => answer,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// A data directory with a recovered folder holding `copies`, and the two paths [`look`]
    /// is given.
    fn data_with(tag: &str, copies: &[&str]) -> (PathBuf, PathBuf, PathBuf) {
        let data = crate::test_support::disk_scratch(tag);
        let folder = data.join(crate::preview::RECOVERED_FOLDER);
        std::fs::create_dir_all(&folder).expect("the recovered folder");
        for copy in copies {
            std::fs::write(folder.join(copy), "typed by hand — 手写\n").expect("a copy");
        }
        let record = data.join(ANNOUNCED_RECORD);
        (data, folder, record)
    }

    fn looked(folder: &Path, record: &Path) -> Result<Option<Announcement>, String> {
        let (folder, record) = (folder.to_path_buf(), record.to_path_buf());
        on_a_worker(move |worker| look(worker, &folder, &record))
    }

    /// RED (T-RECOVERED-FOLDER) — **a copy present and not yet said: the start says it, with the
    /// folder, and the sentence is the singular.** A folder inside the recovered folder is not a
    /// copy.
    ///
    /// MUTATION: no listing (`copies_in` answers an empty set without reading the folder) — no
    /// announcement.
    #[test]
    fn a_copy_not_yet_said_is_said_with_the_folder() {
        let (data, folder, record) =
            data_with("recovered-said", &["2026-10-09T120000Z 说明 notes.md"]);
        std::fs::create_dir_all(folder.join("a folder of mine")).expect("a folder");

        let said = looked(&folder, &record).expect("the look");

        let announcement = said.expect("one copy is said");
        assert_eq!(
            announcement,
            Announcement {
                folder: folder.clone(),
                count: 1
            }
        );
        assert_eq!(
            crate::i18n::recovered_edits_kept_in(crate::i18n::Lang::English, 1, "F"),
            "An unsaved edit was kept at F"
        );
        assert!(
            announcement
                .sentence()
                .contains(&folder.display().to_string()),
            "{}",
            announcement.sentence()
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    /// RED (T-RECOVERED-FOLDER) — **the same copy at the next start is not said again**, and a
    /// copy that arrives after it is said alone.
    ///
    /// MUTATION: no announced set (`look` reads `announced` as an empty set and skips the record) —
    /// the second start says the first copy again.
    #[test]
    fn a_copy_said_once_is_not_said_at_the_next_start() {
        let (data, folder, record) = data_with("recovered-once", &["2026-10-09T120000Z a.md"]);

        assert_eq!(
            looked(&folder, &record).expect("the first start"),
            Some(Announcement {
                folder: folder.clone(),
                count: 1
            })
        );
        assert_eq!(
            looked(&folder, &record).expect("the second start"),
            None,
            "the same copy is never said twice"
        );
        std::fs::write(folder.join("2026-10-10T080000Z 第二.md"), "later\n").expect("a later copy");
        assert_eq!(
            looked(&folder, &record).expect("the third start"),
            Some(Announcement {
                folder: folder.clone(),
                count: 1
            }),
            "only the copy that arrived since is counted"
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    /// RED (T-RECOVERED-FOLDER) — **two copies are said as two, in the plural sentence.**
    ///
    /// MUTATION: count the copies as one (`count: 1` in `look`'s answer) — the count is 1 and the
    /// sentence the singular.
    #[test]
    fn two_copies_are_said_as_two_unsaved_edits() {
        let (data, folder, record) = data_with(
            "recovered-two",
            &[
                "2026-10-09T120000Z notes.md",
                "2026-10-09T120000Z (1) notes.md",
            ],
        );

        let announcement = looked(&folder, &record)
            .expect("the look")
            .expect("two copies are said");

        assert_eq!(announcement.count, 2);
        assert_eq!(
            crate::i18n::recovered_edits_kept_in(crate::i18n::Lang::English, 2, "F"),
            "2 unsaved edits were kept at F"
        );
        let _ = std::fs::remove_dir_all(&data);
    }

    /// RED (T-RECOVERED-FOLDER) — **the card's verb asks the system to open the recovered
    /// folder**, the folder that was listed; a press on the card's body, its `×` and another
    /// card's verb ask nothing.
    ///
    /// MUTATION: wrong folder (`Raised::of` keeps `announcement.folder.parent()`, the data
    /// directory) — the verb names the data directory. MUTATION (review round 2): a body press
    /// opens (`ToastHit::Action(card) | ToastHit::Card(card) if …` in `Raised::press`) — the body
    /// press names the folder.
    #[test]
    fn a_press_on_the_card_opens_the_recovered_folder() {
        let (data, folder, record) = data_with("recovered-press-文件夹", &["x.md"]);
        let announcement = looked(&folder, &record)
            .expect("the look")
            .expect("one copy is said");
        let card = ToastId(7);

        let raised = Raised::of(card, &announcement);

        assert_eq!(raised.press(ToastHit::Action(card)), Some(folder.as_path()));
        assert_eq!(
            raised.press(ToastHit::Card(card)),
            None,
            "a press on the body is swallowed"
        );
        assert_eq!(raised.press(ToastHit::Close(card)), None);
        assert_eq!(raised.press(ToastHit::Action(ToastId(8))), None);
        let _ = std::fs::remove_dir_all(&data);
    }

    /// RED (T-RECOVERED-FOLDER) — **a record that cannot be written says nothing**, so no start
    /// says a copy it could not record (it would say it again at every start). The record is
    /// read-only (Windows refuses the rename over it) and so is its folder (Unix refuses the
    /// temporary).
    ///
    /// MUTATION: announce when the write fails (`.ok();` in place of `?` after `durable_write`) —
    /// an announcement comes back.
    #[test]
    fn a_copy_that_cannot_be_recorded_is_not_said() {
        let (data, folder, record) = data_with("recovered-unwritable", &["x.md"]);
        std::fs::write(&record, b"[]").expect("an empty record");
        // Each is set read-only and then given back the permissions it had.
        let kept: Vec<(PathBuf, std::fs::Permissions)> = [&record, &data]
            .into_iter()
            .map(|place| {
                let permissions = std::fs::metadata(place).expect("the place").permissions();
                let mut locked = permissions.clone();
                locked.set_readonly(true);
                std::fs::set_permissions(place, locked).expect("read-only");
                (place.clone(), permissions)
            })
            .collect();

        let said = looked(&folder, &record);

        for (place, permissions) in kept.into_iter().rev() {
            std::fs::set_permissions(&place, permissions).expect("put back");
        }
        assert!(said.is_err(), "{said:?}");
        let _ = std::fs::remove_dir_all(&data);
    }
}
