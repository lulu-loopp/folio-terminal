//! **A newer Folio's documents, after an older one has replaced them** (0.4.10 plan row 68d;
//! rulings R4-R8 of 2026-11-14).
//!
//! An older build cannot read a document a newer build saved (`FallbackReason::FutureSchemaVersion`):
//! it keeps the newer bytes beside the file as a `.rejected-` copy and runs on its defaults, and
//! its first write replaces the newer document. This module holds what both builds then say:
//!
//! - **The older build's notice** ([`NewerKept`]): one Error card per document, in the rescue
//!   card's shape — the sentence and a verb that reveals the copy — said once. The carry pass's
//!   refused copies decide "once" from the copies themselves
//!   (`rejected_copies::newer_refusal_to_say`): a refusal an older copy already holds was said
//!   when that copy was made.
//! - **The newer build's offer** ([`CopyOffered`]): when the version a store read at this start is
//!   below a copy's that this build reads, one card for every such document, on the update card's
//!   chassis (`update_card::CopyBackVerb`): *Bring back*, *Keep current*, and `Escape` or the `×`,
//!   which writes nothing and asks again at the next start.
//!
//! **Bring back**, for the four documents a store writes as it is pressed (settings, shortcuts,
//! profiles, pins), runs on the storage worker ([`CopyBackJob`], `document_request`'s
//! `serve_copy_back`): the older build's write — the bytes the store holds as its revision — is
//! kept beside the file as a `.rejected-` copy of its own, the copy's bytes are written
//! conditional on the file still holding that write, and the store reads the file again through
//! the road an outside edit takes. A file changed since is not replaced, and that is said. **For
//! the layout** nothing is replaced: the copy's windows open beside the open ones and its recent
//! lists are merged into this start's (R7), and the next autosave writes both.
//!
//! **Keep current** writes each store's loaded document at this build's version (R8), which ends
//! the case: the next start reads this build's version and offers nothing.

use std::path::PathBuf;
use std::sync::{Arc, mpsc};

use crate::i18n::Text;
use crate::rejected_copies::KeptDocument;

/// **A refusal of a newer file, to be said** (R4): the document, and the copy this start kept of
/// it, by its name and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewerKept {
    pub(crate) document: KeptDocument,
    pub(crate) copy: String,
    pub(crate) path: PathBuf,
}

/// **A copy a newer build could bring back, offered** (R5): the document, the copy by its name,
/// where it is, its bytes as the carry pass read them, and the two versions the offer compares —
/// the copy's, and the one the start read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopyOffered {
    pub(crate) document: KeptDocument,
    pub(crate) copy: String,
    pub(crate) path: PathBuf,
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) copy_version: u32,
    pub(crate) file_version: u32,
}

/// **The refused copies' part for the window** (`carry_pass::CarrySnapshot::copy_back`): the
/// notices to raise, and the documents to offer back on one card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CopyBackAnswer {
    pub(crate) notices: Vec<NewerKept>,
    pub(crate) offer: Vec<CopyOffered>,
}

impl CopyBackAnswer {
    /// **A newer answer over one the window has not taken**: the untaken notices are owed still
    /// and go first; the offer is the newer round's, which read the copies last.
    #[must_use]
    pub(crate) fn keeping_untaken(mut self, untaken: Option<Self>) -> Self {
        if let Some(untaken) = untaken {
            let mut notices = untaken.notices;
            notices.append(&mut self.notices);
            self.notices = notices;
        }
        self
    }
}

impl KeptDocument {
    /// **What the person calls this document**, as the notice and the offer name it; `None` for
    /// `update-check.json`, which is neither said nor offered.
    pub(crate) fn saved_noun(self) -> Option<Text> {
        match self {
            Self::Session => Some(Text::SavedWindowsAndTabs),
            Self::Settings => Some(Text::SavedSettings),
            Self::Keybindings => Some(Text::SavedShortcuts),
            Self::Profiles => Some(Text::SavedProfiles),
            Self::Pins => Some(Text::SavedPins),
            Self::UpdateCheck => None,
        }
    }
}

/// **The nouns of `documents`, joined as one list** — "settings, shortcuts and pins" — in the
/// order given, for the offer's heading.
#[must_use]
pub(crate) fn nouns_of(documents: impl IntoIterator<Item = KeptDocument>) -> String {
    let nouns: Vec<&'static str> = documents
        .into_iter()
        .filter_map(KeptDocument::saved_noun)
        .map(Text::text)
        .collect();
    match nouns.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => Text::ListLast
            .text()
            .replace("{names}", &rest.join(Text::ListSeparator.text()))
            .replace("{last}", last),
    }
}

/// **The older build's notice for one document** (R4): "This Folio is older than the one that
/// last saved your {what}. They are kept in {copy}."
#[must_use]
pub(crate) fn newer_kept_sentence(notice: &NewerKept) -> String {
    Text::NewerDocumentKept
        .text()
        .replace("{what}", &nouns_of([notice.document]))
        .replace("{copy}", &notice.copy)
}

/// **The offer's heading** (R6): "Bring back your newer {what}?", every offered document named.
#[must_use]
pub(crate) fn offer_heading(offer: &[CopyOffered]) -> String {
    Text::CopyBackHeading
        .text()
        .replace("{what}", &nouns_of(offer.iter().map(|copy| copy.document)))
}

/// **The offer, up in one window** (R6): the window it was raised in, the documents it offers,
/// and the card's hover and ring. On `App`, as the update card's job is: the question is the
/// application's, asked in one window.
#[derive(Debug)]
pub(crate) struct OfferUp {
    pub(crate) window: winit::window::WindowId,
    pub(crate) offer: Vec<CopyOffered>,
    pub(crate) card: crate::update_card::Card<crate::update_card::CopyBackVerb>,
}

impl OfferUp {
    /// What the card draws.
    #[must_use]
    pub(crate) fn paint(&self) -> crate::update_card::Paint<crate::update_card::CopyBackVerb> {
        crate::update_card::copy_back_paint(offer_heading(&self.offer))
    }

    /// **More documents offered while the card is up**: each replaces the one it offers the
    /// same document for, or joins the card.
    pub(crate) fn offer_too(&mut self, more: Vec<CopyOffered>) {
        for offered in more {
            match self
                .offer
                .iter_mut()
                .find(|standing| standing.document == offered.document)
            {
                Some(standing) => *standing = offered,
                None => self.offer.push(offered),
            }
        }
    }
}

/// **One document's Bring back, for the storage worker**: the file, the bytes the store holds as
/// its revision (`None`: no file), and the copy's bytes to write in their place.
pub(crate) struct CopyBackJob {
    pub(crate) which: PathBuf,
    pub(crate) expected: Option<Arc<[u8]>>,
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) answer: mpsc::Sender<CopyBackDone>,
}

/// **What one Bring back came to.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CopyBackDone {
    /// The file holds the copy's bytes; the older build's write is kept under `kept` (`None`
    /// when there was no file to keep).
    BroughtBack { kept: Option<PathBuf> },
    /// The file no longer held the older build's write — somebody changed it after this start
    /// read it — and nothing was written.
    ChangedSince,
    /// Nothing was written, for this reason.
    NotBrought(String),
}

/// **What a Bring back that did not happen owes the person**, by its document and its copy;
/// `None` when it happened (the re-read that follows says what changed).
#[must_use]
pub(crate) fn not_brought_sentence(
    document: KeptDocument,
    copy: &str,
    done: &CopyBackDone,
) -> Option<String> {
    let what = nouns_of([document]);
    match done {
        CopyBackDone::BroughtBack { .. } => None,
        CopyBackDone::ChangedSince => Some(
            Text::CopyBackChangedSince
                .text()
                .replace("{what}", &what)
                .replace("{copy}", copy),
        ),
        CopyBackDone::NotBrought(reason) => Some(
            Text::CopyBackNotBrought
                .text()
                .replace("{what}", &what)
                .replace("{reason}", reason)
                .replace("{copy}", copy),
        ),
    }
}

/// **The layout a newer build saved, brought back beside this one** (R7): the copy's windows to
/// open — a summoned window only when none is open, as there is one summoned window at most —
/// and its recent lists, which the caller merges. The bytes are the ones the carry pass offered,
/// which it offered because this build's chain reads them (`KeptDocument::this_build_reads`).
#[must_use]
pub(crate) fn layout_to_bring_back(
    offered: &CopyOffered,
    a_summoned_window_is_open: bool,
) -> bt_persist::SessionV1 {
    let mut session = bt_persist::parse_session(&offered.bytes)
        .expect("the carry pass offers only a layout this build reads");
    if a_summoned_window_is_open {
        session.windows.retain(|window| !window.quake);
    }
    session
}

#[cfg(test)]
#[path = "copy_back_tests.rs"]
mod tests;
