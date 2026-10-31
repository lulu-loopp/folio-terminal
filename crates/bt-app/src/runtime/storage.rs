//! **The storage lane's answers, every asker's in the order the lane admitted them**
//! (T-STORAGE-LANE-ORDER-BY-CONSTRUCTION, 0.4.9 plan row 104).
//!
//! The storage worker serves the four hand-editable documents' stores, the preview's saves and
//! an update's hand-over on one queue (`persist::StorageLane`), and its contract declares its
//! answers delivered in admission order (`lane::STORAGE`). No asker takes its own: one drain
//! reads the lane's earliest request still owed once, takes every asker's answers below it, and
//! merges them by the order the lane admitted them ([`App::storage_answers`],
//! `lane::in_admission_order`); the application then hears each in that order, in the asker
//! that holds it ([`FolioApp::answer_storage`]). A request that ends while the askers are read
//! waits for the next drain, so what one drain hears is a prefix of the admission order and the
//! next drain's follows it, whatever the worker does meanwhile.
//!
//! The session's receipts, the launch backup and an update trial's receipt are not this lane's
//! deliveries: each is left to the lane's sink at admission and read on its own road
//! (`SessionStore::take_receipts`, `update_trial::watch`).

use crate::document_request::StoreAnswer;
use crate::persist::{Ended, InAdmissionOrder};
use crate::window_seam::prelude::*;
use crate::{App, FolioApp, lane, preview, update_handoff};
use anyhow::Result;
use bt_persist::{KeybindingsV1, PinsV1, ProfilesV1, SettingsV1};

/// **One answer the storage drain took**, with the asker that holds it.
pub(crate) enum StorageAnswer {
    Settings(StoreAnswer<SettingsV1>),
    Keybindings(StoreAnswer<KeybindingsV1>),
    Profiles(StoreAnswer<ProfilesV1>),
    Pins(StoreAnswer<PinsV1>),
    /// A preview save's, heard by the buffer that asked, in whichever window it stands.
    Preview(preview::SaveAnswer),
    /// The update's hand-over's, looked at by the quit's way out.
    Handoff(Ended<update_handoff::HandedOff>),
}

impl<B: Backend> App<B> {
    /// **One drain's answers, every asker's, in admission order**: the lane's earliest request
    /// still owed, read once (`persist::StorageLane::earliest_owed`); each asker's answers below
    /// it — the four stores', the preview desk's and the hand-over's; merged by the order the
    /// lane admitted them (`lane::in_admission_order`). Never waits.
    pub(crate) fn storage_answers(&mut self) -> Vec<StorageAnswer> {
        let owed = self.session_store.lane().earliest_owed();
        let mut held: Vec<((), Vec<(InAdmissionOrder, StorageAnswer)>)> = vec![
            (
                (),
                tagged(self.settings_store.answered(owed), StorageAnswer::Settings),
            ),
            (
                (),
                tagged(
                    self.keybindings_store.answered(owed),
                    StorageAnswer::Keybindings,
                ),
            ),
            (
                (),
                tagged(self.profiles_store.answered(owed), StorageAnswer::Profiles),
            ),
            (
                (),
                tagged(self.pins_store.answered(owed), StorageAnswer::Pins),
            ),
            (
                (),
                tagged(self.preview_saves.answered(owed), StorageAnswer::Preview),
            ),
        ];
        if let Some(answer) = self.handoff_answer.as_mut()
            && let Some((order, ended)) = answer.hear_below(owed)
        {
            held.push(((), vec![(order, StorageAnswer::Handoff(ended))]));
        }
        lane::in_admission_order(held)
            .into_iter()
            .map(|((), _, answer)| answer)
            .collect()
    }

    /// **Hear one answer of the drain in the asker that holds it**: a store's in its store, the
    /// hand-over's kept for the quit's way out. A preview save's is handed back, for the windows
    /// to hear ([`FolioApp::answer_storage`]).
    fn hear_storage_answer(&mut self, answer: StorageAnswer) -> Option<preview::SaveAnswer> {
        #[cfg(test)]
        HEARD.with(|heard| heard.borrow_mut().push(answer.asker()));
        match answer {
            StorageAnswer::Settings(answer) => self.settings_store.hear(answer),
            StorageAnswer::Keybindings(answer) => self.keybindings_store.hear(answer),
            StorageAnswer::Profiles(answer) => self.profiles_store.hear(answer),
            StorageAnswer::Pins(answer) => self.pins_store.hear(answer),
            StorageAnswer::Preview(answer) => return Some(answer),
            StorageAnswer::Handoff(ended) => self.handoff_heard = Some(ended),
        }
        None
    }

    /// The drain has heard every answer it took: each store sends what is owed after them.
    fn storage_answers_heard(&mut self) {
        self.settings_store.answers_heard();
        self.keybindings_store.answers_heard();
        self.profiles_store.answers_heard();
        self.pins_store.answers_heard();
    }
}

/// Each answer of one asker, with its place in admission order, as the drain's answer.
fn tagged<T>(
    answers: Vec<(InAdmissionOrder, T)>,
    asker: impl Fn(T) -> StorageAnswer,
) -> Vec<(InAdmissionOrder, StorageAnswer)> {
    answers
        .into_iter()
        .map(|(order, answer)| (order, asker(answer)))
        .collect()
}

impl<B: Backend> FolioApp<B> {
    /// **The storage lane's answers, heard in the order the lane admitted them** — on the turn
    /// the lane's wake brings (`AppEvent::DocumentsAnswered`), on each turn of a quit's
    /// documents and recovery phases and of its way out, and at the system's end of the session:
    /// the one place the lane's answers are taken.
    ///
    /// One drain ([`App::storage_answers`]); each answer then heard in its asker, in that order:
    /// a store's in its store, the hand-over's kept for the way out, a preview save's by the
    /// buffer that asked in whichever standing window its tab is in now
    /// ([`Runtime::hear_preview_saves`], one answer at a time), or — its tab, pane or edits
    /// gone, or its window leaving — by the sink, recorded and then ignored (C6). Then every
    /// store sends what is owed after its answers. Every answer taken is heard, or sunk, before
    /// the first error a window's repaint gave is passed on; with it, every refusal the
    /// preview's answers gave, by the file each was for.
    pub(crate) fn answer_storage(&mut self) -> (Vec<(preview::PreviewSource, String)>, Result<()>) {
        let mut refused = Vec::new();
        let Some(answers) = self.app.as_mut().map(App::storage_answers) else {
            return (refused, Ok(()));
        };
        let mut first_error = None;
        for answer in answers {
            let Some(save) = self
                .app
                .as_mut()
                .and_then(|app| app.hear_storage_answer(answer))
            else {
                continue;
            };
            let mut one = vec![save];
            for index in 0..self.windows.len() {
                if one.is_empty() {
                    break;
                }
                if let Some(mut runtime) = self.runtime_at(index)
                    && runtime.window.leaving.is_none()
                    && let Err(error) = runtime.hear_preview_saves(&mut one, &mut refused)
                {
                    first_error.get_or_insert(error);
                }
            }
            if let Some(app) = self.app.as_mut() {
                app.sink_preview_save_answers(one);
            }
        }
        if let Some(app) = self.app.as_mut() {
            app.storage_answers_heard();
        }
        (refused, first_error.map_or(Ok(()), Err))
    }
}

#[cfg(test)]
impl StorageAnswer {
    /// The asker that holds it, as a test names it.
    fn asker(&self) -> &'static str {
        match self {
            Self::Settings(_) => "settings",
            Self::Keybindings(_) => "keybindings",
            Self::Profiles(_) => "profiles",
            Self::Pins(_) => "pins",
            Self::Preview(_) => "preview",
            Self::Handoff(_) => "handoff",
        }
    }
}

#[cfg(test)]
thread_local! {
    /// The askers whose answers this thread's drains heard, in the order heard (T2).
    static HEARD: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// **Which askers this thread's drains heard answers for, in the order heard**, since the last
/// look — a test's record of the drain's order.
#[cfg(test)]
pub(crate) fn take_heard() -> Vec<&'static str> {
    HEARD.with(|heard| std::mem::take(&mut *heard.borrow_mut()))
}
