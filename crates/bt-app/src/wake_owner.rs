//! **Who asked for the loop's next wake** (T-WINDOW-THREAD-UNDER-LOAD).
//!
//! The window thread parks with `ControlFlow::WaitUntil(deadline)`, and the deadline is the
//! earliest of every clock the application and its windows hold: the fold at the foot of
//! `Runtime::turn` and the one in `about_to_wait_inner`. Each entry of both folds is named here,
//! once, so the instant the loop parks until always travels with the clock that set it — into the
//! idle trace (`BT_PERF_TRACE idle_wake`) and into the turn accounting's `source=` cell
//! (`hang_watch_accounting`), which is how a wake that is already in the past when the loop parks
//! (`stale_us=`: winit returns at once and the loop spins) names its owner instead of reading as a
//! scheduling delay.
//!
//! The variant is the token a log line prints (`source=HyperlinkHover`); [`WakeOwner::name`] is
//! the phrase the idle trace has always printed. Discriminants are the declaration order, which is
//! what lets the accounting keep an owner in one relaxed byte ([`WakeOwner::from_byte`]).

use std::time::Instant;

/// **One clock in the loop's wake fold.** See the module documentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum WakeOwner {
    // ── A window's own clocks: `Runtime::turn`'s fold, in its order. ──
    StartupPoll,
    ImeCursor,
    ShellCaret,
    TabPress,
    RenameCaret,
    StripAnimation,
    WebTeardown,
    PtyResize,
    ResizeFinish,
    SynchronizedUpdate,
    LiveStability,
    ForegroundProgram,
    PtyCoalesce,
    AttentionCredential,
    Tooltip,
    KeyHint,
    CardsHint,
    Toast,
    ClipboardPasteDeadline,
    CommandFlash,
    CommandRails,
    TerminalThumbs,
    LayoutPeek,
    FilePeek,
    FilePeekCloseGrace,
    FilePeekDwell,
    Float,
    RevealedFoot,
    WebZoomAcknowledgement,
    WebDialogAcknowledgement,
    PreviewSaveNotice,
    PreviewRefusal,
    Chevrons,
    PaneMenu,
    TerminalMenu,
    TabMenu,
    DragSpring,
    DragAutoScroll,
    HyperlinkHover,
    PeekHover,
    FormulaTools,
    FormulaToggle,
    FormulaCopyAcknowledgement,
    PreviewResample,
    SessionSave,
    SchemesWatch,
    StorageWatch,
    GitWatch,
    UpdateCheck,
    AboutLastChecked,
    PreviewWatch,
    FilesWatch,
    RefusedFrame,
    WindowTitle,
    SearchWalk,
    WebEngineWarmUp,
    SpareWebController,
    // ── The application's clocks: `about_to_wait_inner`'s fold. ──
    DragBroker,
    WindowReap,
    ProgramsEar,
    IndexEar,
    ImeReport,
    QuitBound,
    CloseWait,
    Retirement,
    RunEnd,
}

impl WakeOwner {
    /// Every owner, in declaration (and discriminant) order.
    pub(crate) const ALL: &'static [WakeOwner] = &[
        Self::StartupPoll,
        Self::ImeCursor,
        Self::ShellCaret,
        Self::TabPress,
        Self::RenameCaret,
        Self::StripAnimation,
        Self::WebTeardown,
        Self::PtyResize,
        Self::ResizeFinish,
        Self::SynchronizedUpdate,
        Self::LiveStability,
        Self::ForegroundProgram,
        Self::PtyCoalesce,
        Self::AttentionCredential,
        Self::Tooltip,
        Self::KeyHint,
        Self::CardsHint,
        Self::Toast,
        Self::ClipboardPasteDeadline,
        Self::CommandFlash,
        Self::CommandRails,
        Self::TerminalThumbs,
        Self::LayoutPeek,
        Self::FilePeek,
        Self::FilePeekCloseGrace,
        Self::FilePeekDwell,
        Self::Float,
        Self::RevealedFoot,
        Self::WebZoomAcknowledgement,
        Self::WebDialogAcknowledgement,
        Self::PreviewSaveNotice,
        Self::PreviewRefusal,
        Self::Chevrons,
        Self::PaneMenu,
        Self::TerminalMenu,
        Self::TabMenu,
        Self::DragSpring,
        Self::DragAutoScroll,
        Self::HyperlinkHover,
        Self::PeekHover,
        Self::FormulaTools,
        Self::FormulaToggle,
        Self::FormulaCopyAcknowledgement,
        Self::PreviewResample,
        Self::SessionSave,
        Self::SchemesWatch,
        Self::StorageWatch,
        Self::GitWatch,
        Self::UpdateCheck,
        Self::AboutLastChecked,
        Self::PreviewWatch,
        Self::FilesWatch,
        Self::RefusedFrame,
        Self::WindowTitle,
        Self::SearchWalk,
        Self::WebEngineWarmUp,
        Self::SpareWebController,
        Self::DragBroker,
        Self::WindowReap,
        Self::ProgramsEar,
        Self::IndexEar,
        Self::ImeReport,
        Self::QuitBound,
        Self::CloseWait,
        Self::Retirement,
        Self::RunEnd,
    ];

    /// The phrase the idle trace prints for this owner.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::StartupPoll => "startup poll",
            Self::ImeCursor => "IME cursor",
            Self::ShellCaret => "shell caret",
            Self::TabPress => "tab press",
            Self::RenameCaret => "rename caret",
            Self::StripAnimation => "strip animation",
            Self::WebTeardown => "web teardown",
            Self::PtyResize => "PTY resize",
            Self::ResizeFinish => "resize finish",
            Self::SynchronizedUpdate => "synchronized update",
            Self::LiveStability => "live stability",
            Self::ForegroundProgram => "foreground program",
            Self::PtyCoalesce => "PTY coalesce",
            Self::AttentionCredential => "attention credential",
            Self::Tooltip => "tooltip",
            Self::KeyHint => "key hint",
            Self::CardsHint => "Cards hint",
            Self::Toast => "toast",
            Self::ClipboardPasteDeadline => "clipboard paste deadline",
            Self::CommandFlash => "command flash",
            Self::CommandRails => "command rails",
            Self::TerminalThumbs => "terminal thumbs",
            Self::LayoutPeek => "layout peek",
            Self::FilePeek => "file peek",
            Self::FilePeekCloseGrace => "file-peek close grace",
            Self::FilePeekDwell => "file-peek dwell",
            Self::Float => "float",
            Self::RevealedFoot => "revealed foot",
            Self::WebZoomAcknowledgement => "web zoom acknowledgement",
            Self::WebDialogAcknowledgement => "web dialog acknowledgement",
            Self::PreviewSaveNotice => "preview save notice",
            Self::PreviewRefusal => "preview refusal",
            Self::Chevrons => "chevrons",
            Self::PaneMenu => "pane menu",
            Self::TerminalMenu => "terminal menu",
            Self::TabMenu => "tab menu",
            Self::DragSpring => "drag spring",
            Self::DragAutoScroll => "drag auto-scroll",
            Self::HyperlinkHover => "hyperlink hover",
            Self::PeekHover => "peek hover",
            Self::FormulaTools => "formula tools",
            Self::FormulaToggle => "formula toggle",
            Self::FormulaCopyAcknowledgement => "formula-copy acknowledgement",
            Self::PreviewResample => "preview resample",
            Self::SessionSave => "session save",
            Self::SchemesWatch => "schemes watch",
            Self::StorageWatch => "storage watch",
            Self::GitWatch => "git watch",
            Self::UpdateCheck => "update check",
            Self::AboutLastChecked => "About last checked",
            Self::PreviewWatch => "preview watch",
            Self::FilesWatch => "files watch",
            Self::RefusedFrame => "refused frame",
            Self::WindowTitle => "window title",
            Self::SearchWalk => "search walk",
            Self::WebEngineWarmUp => "web engine warm-up",
            Self::SpareWebController => "spare web controller",
            Self::DragBroker => "drag broker",
            Self::WindowReap => "window reap",
            Self::ProgramsEar => "programs ear",
            Self::IndexEar => "index ear",
            Self::ImeReport => "IME report",
            Self::QuitBound => "quit bound",
            Self::CloseWait => "close wait",
            Self::Retirement => "retirement",
            Self::RunEnd => "run end",
        }
    }

    /// The owner whose discriminant is `byte`, if there is one.
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.get(usize::from(byte)).copied()
    }
}

/// **The earliest of `entries`, with the owner that supplied it.** Ties keep the first entry, as
/// [`Iterator::min`] over bare instants did.
pub(crate) fn earliest_wake(
    entries: impl IntoIterator<Item = Option<(WakeOwner, Instant)>>,
) -> Option<(WakeOwner, Instant)> {
    entries.into_iter().flatten().min_by_key(|(_, at)| *at)
}

/// `deadline`, owned by `owner`.
pub(crate) fn owned(owner: WakeOwner, deadline: Option<Instant>) -> Option<(WakeOwner, Instant)> {
    deadline.map(|at| (owner, at))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{WakeOwner, earliest_wake, owned};

    /// Every owner is its own byte back, and no byte past the last names anyone: the accounting
    /// keeps an owner in one byte and must read back the owner it stored.
    #[test]
    fn every_owner_round_trips_through_its_byte() {
        for (index, owner) in WakeOwner::ALL.iter().enumerate() {
            assert_eq!(*owner as usize, index, "{owner:?}");
            assert_eq!(WakeOwner::from_byte(*owner as u8), Some(*owner));
        }
        assert_eq!(WakeOwner::from_byte(WakeOwner::ALL.len() as u8), None);
        let mut names: Vec<_> = WakeOwner::ALL.iter().map(|owner| owner.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            WakeOwner::ALL.len(),
            "no two owners share a phrase"
        );
    }

    /// The earliest entry wins with its owner; a tie keeps the first.
    #[test]
    fn the_earliest_wake_keeps_its_owner_and_ties_keep_the_first() {
        let epoch = Instant::now();
        let later = epoch + Duration::from_millis(5);
        assert_eq!(
            earliest_wake([
                owned(WakeOwner::Toast, Some(later)),
                owned(WakeOwner::DragBroker, Some(epoch)),
                owned(WakeOwner::CloseWait, Some(epoch)),
                owned(WakeOwner::QuitBound, None),
            ]),
            Some((WakeOwner::DragBroker, epoch))
        );
        assert_eq!(earliest_wake([owned(WakeOwner::QuitBound, None)]), None);
    }
}
