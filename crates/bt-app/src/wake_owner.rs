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

macro_rules! wake_owners {
    ($($(#[$doc:meta])* $variant:ident => $name:literal,)*) => {
        /// **One clock in the loop's wake fold.** See the module documentation.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(u8)]
        pub(crate) enum WakeOwner {
            $($(#[$doc])* $variant,)*
        }

        impl WakeOwner {
            /// Every owner, in declaration (and discriminant) order.
            pub(crate) const ALL: &'static [WakeOwner] = &[$(WakeOwner::$variant,)*];

            /// The phrase the idle trace prints for this owner.
            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(WakeOwner::$variant => $name,)*
                }
            }
        }
    };
}

wake_owners! {
    // ── A window's own clocks: `Runtime::turn`'s fold, in its order. ──
    StartupPoll => "startup poll",
    ImeCursor => "IME cursor",
    ShellCaret => "shell caret",
    TabPress => "tab press",
    RenameCaret => "rename caret",
    StripAnimation => "strip animation",
    WebTeardown => "web teardown",
    PtyResize => "PTY resize",
    ResizeFinish => "resize finish",
    SynchronizedUpdate => "synchronized update",
    LiveStability => "live stability",
    ForegroundProgram => "foreground program",
    PtyCoalesce => "PTY coalesce",
    AttentionCredential => "attention credential",
    Tooltip => "tooltip",
    KeyHint => "key hint",
    CardsHint => "Cards hint",
    Toast => "toast",
    CommandFlash => "command flash",
    CommandRails => "command rails",
    TerminalThumbs => "terminal thumbs",
    LayoutPeek => "layout peek",
    FilePeek => "file peek",
    FilePeekCloseGrace => "file-peek close grace",
    FilePeekDwell => "file-peek dwell",
    Float => "float",
    RevealedFoot => "revealed foot",
    WebZoomAcknowledgement => "web zoom acknowledgement",
    WebDialogAcknowledgement => "web dialog acknowledgement",
    PreviewSaveNotice => "preview save notice",
    PreviewRefusal => "preview refusal",
    Chevrons => "chevrons",
    PaneMenu => "pane menu",
    TerminalMenu => "terminal menu",
    TabMenu => "tab menu",
    DragSpring => "drag spring",
    DragAutoScroll => "drag auto-scroll",
    HyperlinkHover => "hyperlink hover",
    PeekHover => "peek hover",
    FormulaTools => "formula tools",
    FormulaToggle => "formula toggle",
    FormulaCopyAcknowledgement => "formula-copy acknowledgement",
    PreviewResample => "preview resample",
    SessionSave => "session save",
    SchemesWatch => "schemes watch",
    StorageWatch => "storage watch",
    GitWatch => "git watch",
    UpdateCheck => "update check",
    AboutLastChecked => "About last checked",
    PreviewWatch => "preview watch",
    FilesWatch => "files watch",
    RefusedFrame => "refused frame",
    WindowTitle => "window title",
    SearchWalk => "search walk",
    WebEngineWarmUp => "web engine warm-up",
    SpareWebController => "spare web controller",
    // ── The application's clocks: `about_to_wait_inner`'s fold. ──
    DragBroker => "drag broker",
    WindowReap => "window reap",
    ProgramsEar => "programs ear",
    IndexEar => "index ear",
    ImeReport => "IME report",
    QuitBound => "quit bound",
    CloseWait => "close wait",
    Retirement => "retirement",
    RunEnd => "run end",
}

impl WakeOwner {
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
