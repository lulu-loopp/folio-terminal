//! **The storage pass's four merges on one window, and the one card a batch of
//! re-reads raises** (`docs/plans/design/settings-reread-2026-10-11.md` §3.6,
//! (b)11; ticket T-STORE-NOTICE).
//!
//! A pass runs the four documents' merges in one order — `settings.json`,
//! `keybindings.json`, `profiles.json`, `pins.json` — on one window
//! ([`merge_documents_on`]), each pass whole, all four on one [`MergeNotice`],
//! and presents once after all four. The window is a seam
//! ([`StoragePassWindow`]): the product's is `Runtime`'s merge turn
//! (`runtime/configuration.rs`), a test's drives the four real stores.
//!
//! **One card per batch, not per document.** The pass asks the storage worker
//! for one read per named document, and each answer comes back on its own
//! wake, so a `git checkout` of four files can merge over several turns. Each
//! turn's notice joins the batch's ([`MergeNotice::gather`]), and the card is
//! raised once no store has a read still to answer. A clean hand edit adds
//! nothing, and a batch with nothing to say raises nothing. The card is raised
//! after the merges and their presentation, from the data the merges made: a
//! failed presentation never takes it away ([`take_merges`]).

use crate::document_request::{MergeNotice, ReplacedInFile};
use crate::{keybindings_merge, persist, pins, settings_merge, table_merge};

/// **The four documents in the order a pass merges them**, which is the order
/// its card names them in.
const PASS_ORDER: [&str; 4] = [
    persist::SETTINGS_FILE_NAME,
    persist::KEYBINDINGS_FILE_NAME,
    persist::PROFILES_FILE_NAME,
    pins::PINS_FILE_NAME,
];

/// **What a storage pass needs from the window it runs in**: the three merge
/// seams, the one presentation after them, and the batch's card.
pub(crate) trait StoragePassWindow:
    settings_merge::MergeWindow + keybindings_merge::KeybindingsWindow + table_merge::TableMergeWindow
{
    /// **The one presentation after the four passes**, when a pass owed one.
    fn present_merges(&mut self) -> anyhow::Result<()>;
    /// **This turn's notice joined to the batch's**, and the batch's whole
    /// notice when its card is due: [`MergeNotice::gather`] over whether any
    /// store is still reading (`persist::documents_reading`).
    fn merge_notice_due(&mut self, turn: MergeNotice) -> Option<MergeNotice>;
    /// **The batch's one card**, raised on this window.
    fn raise_merge_notice(&mut self, notice: &MergeNotice) -> anyhow::Result<()>;
}

/// **One storage pass's merges, in their order, and the one presentation after
/// them** (§3.3, §3.6): each pass whole, all four on one notice. Answers what
/// the files' merges replaced and could not apply — nothing for a clean hand
/// edit — and, beside it, whether the presentation went through: the data is
/// made first, and a failed presentation never takes it away.
pub(crate) fn merge_documents_on(
    window: &mut impl StoragePassWindow,
) -> (MergeNotice, anyhow::Result<()>) {
    let settings = settings_merge::run_pass(window);
    let keybindings = keybindings_merge::run_pass(window);
    let profiles = table_merge::run_pass(window, table_merge::Table::Profiles);
    let pins = table_merge::run_pass(window, table_merge::Table::Pins);
    let mut notice = MergeNotice::default();
    for replaced in [
        settings.replaced,
        keybindings.replaced,
        profiles.replaced,
        pins.replaced,
    ] {
        notice.files.extend(replaced);
    }
    // The passes answer what the window answered them — it notes the
    // presentation it owes — and the one presentation answers for all.
    let presented = settings
        .presented
        .and(keybindings.presented)
        .and(profiles.presented)
        .and(pins.presented)
        .and_then(|()| window.present_merges());
    (notice, presented)
}

/// **One turn of the storage pass on `window`**: the four merges and their
/// presentation ([`merge_documents_on`]), then the batch's card when it is due —
/// raised whatever the presentation answered, and before its error is
/// answered.
pub(crate) fn take_merges(window: &mut impl StoragePassWindow) -> anyhow::Result<()> {
    let (turn, presented) = merge_documents_on(window);
    let raised = match window.merge_notice_due(turn) {
        Some(notice) => window.raise_merge_notice(&notice),
        None => Ok(()),
    };
    presented.and(raised)
}

impl MergeNotice {
    /// **Join one turn's notice to the batch's**, and answer the batch's whole
    /// notice — leaving none held — once no store is `reading` and there is
    /// something to say.
    ///
    /// One entry per file, in [`PASS_ORDER`]: a file merged again in the batch
    /// adds each of its names once to the entry it already has.
    pub(crate) fn gather(&mut self, turn: MergeNotice, reading: bool) -> Option<MergeNotice> {
        for entry in turn.files {
            self.join(entry);
        }
        if reading || self.files.is_empty() {
            return None;
        }
        Some(std::mem::take(self))
    }

    fn join(&mut self, entry: ReplacedInFile) {
        if let Some(held) = self.files.iter_mut().find(|held| held.file == entry.file) {
            for name in entry.values {
                push_once(&mut held.values, name);
            }
            for name in entry.not_applied {
                push_once(&mut held.not_applied, name);
            }
            return;
        }
        let rank = |file: &str| PASS_ORDER.iter().position(|named| *named == file);
        let at = self
            .files
            .partition_point(|held| rank(held.file) <= rank(entry.file));
        self.files.insert(at, entry);
    }
}

fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}
