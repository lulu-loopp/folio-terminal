//! `Runtime`'s side of the OS hand-off lane (`crate::handoff_lane`): putting a hand-off on it,
//! and answering what comes back.
//!
//! The surfaces that hand something to the system — `open_local_path`,
//! `reveal_in_explorer`, `open_local_path_verified`, `reveal_verified`, `open_preview_link`,
//! `hand_url_to_the_browser`, `activate_local_image_path`, `open_font_settings`, since
//! ticket 14 `open_unverified_reference` and `hand_uri_to_the_system`, and since ticket 39
//! `open_preview_in_browser` for a page it hands over by its address — each call
//! [`Runtime::hand_off`] with an [`Ask`]: the request, the words its refusal has always had, and
//! what its answer is owed. None of them calls a `bt_platform::handoff` door itself
//! (`no_handoff_runs_on_the_window_thread`).

use crate::window_seam::prelude::*;
use std::time::Instant;

use anyhow::Result;

use crate::handoff_lane::{
    Answer, Ask, Duty, FollowUp, HandoffId, OnAccepted, RefusalWords, Reply,
};
use crate::{FOOT_REVEAL_FEEDBACK, RevealedFoot, Runtime, handoff_lane, i18n, toast};
use std::path::Path;

impl<B: Backend> Runtime<'_, B> {
    /// **Put one hand-off on the lane**, hold its request with what its answer is owed, and
    /// return at once.
    ///
    /// The window is named here, on the window thread, because that is where the handle lives. A
    /// press the lane refuses at its door — full, or its worker gone — or whose window cannot be
    /// named has its terminal now, and is answered now, with the same duty as a refusal the lane
    /// brings back, in the words of who refused (the side-effects contract, revision (b)3).
    pub(crate) fn hand_off(&mut self, ask: Ask) -> Result<()> {
        let Ask { handoff, duty } = ask;
        let admitted = self.window.window.hand_off(&self.app.handoff_lane, handoff);
        match admitted {
            Ok(request) => {
                self.window.handoffs.owe(request, duty);
                Ok(())
            }
            Err(reason) => self.answer_handoff(Answer::Refused(reason), duty),
        }
    }

    /// **Take every answer this window's hand-offs hold below `owed`** (the lane's earliest
    /// request still owed, read once for the whole drain), without answering them — the
    /// application answers every window's in press order across the windows
    /// (`handoff_lane::in_press_order`).
    pub(crate) fn take_handoff_answers(
        &mut self,
        owed: Option<HandoffId>,
    ) -> Vec<(HandoffId, Answer, Duty)> {
        self.window.handoffs.answered(owed)
    }

    /// **Answer one hand-off** of this window's, as [`Duty::reply`] says.
    ///
    /// Success is silent unless the surface named a confirmation; a refusal says what that
    /// surface said on the window thread before the lane existed ([`crate::handoff_lane::Refusal::
    /// words`]) and what the surface asked for beyond it; a hand-off that failed or was lost with
    /// the lane's worker is said through the same follow-up, in words of its own.
    pub(crate) fn answer_handoff(&mut self, answer: Answer, duty: Duty) -> Result<()> {
        match duty.reply(answer) {
            Reply::HandedOver(accepted) => self.handed_over(accepted),
            Reply::Refused { words, then } => {
                self.say_refusal(words)?;
                match then {
                    Some(FollowUp::Hover(hyperlink, refusal)) => {
                        self.window.hyperlink_hover.show_refused(hyperlink, refusal);
                        self.publish_interaction_frame()
                    }
                    Some(FollowUp::Address(surface, address, refusal)) => {
                        self.say_address_refused(surface, &address, refusal)
                    }
                    Some(FollowUp::FontsToast(words)) => self.toast(
                        toast::ToastKind::Error,
                        toast::ToastAnchor::Window,
                        Some(i18n::Text::InstallFonts.text().to_owned()),
                        words,
                    ),
                    None => Ok(()),
                }
            }
        }
    }

    /// The confirmation a surface named for a hand-off the system took, drawn.
    fn handed_over(&mut self, accepted: Option<OnAccepted>) -> Result<()> {
        match accepted {
            Some(OnAccepted::Revealed(foot)) => {
                self.window.revealed_foot = Some((foot, Instant::now()));
                // A float's foot is drawn in the overlay and a column's or a preview's in
                // the chrome — the two refreshes each surface's own press used to call.
                let changed = match foot {
                    RevealedFoot::Float(_) => self.refresh_overlay(),
                    RevealedFoot::Column(_) | RevealedFoot::Preview(_) => self.refresh_chrome(),
                };
                if changed {
                    self.present_chrome_change()?;
                }
            }
            Some(OnAccepted::PreviewOpened) => {
                self.window.preview_opened_at = Some(Instant::now());
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            None => {}
        }
        Ok(())
    }

    /// The surface's own refusal words: its stderr line, and the files notice.
    fn say_refusal(&mut self, words: RefusalWords) -> Result<()> {
        if let Some(line) = words.line {
            crate::diagnostics::stderr_line(line.to_string());
        }
        if let Some(notice) = words.notice {
            self.window.files_notice = Some((notice.to_owned(), Instant::now()));
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    /// The foot's confirmation has stood long enough: it goes back to saying
    /// where it is (B24).
    ///
    /// Its own tick rather than a line inside [`Self::advance_float`], because
    /// the two feet are drawn into two different surfaces — the float's into the
    /// overlay stack, a docked column's into the seat chrome — and the strip that
    /// has to be rebuilt is decided by which foot was talking. Folding it back in
    /// would have refreshed the overlay for a change that happened in a pane, and
    /// the pane would have kept saying "Revealed" until something else moved.
    pub(crate) fn advance_foot_reveal(&mut self, now: Instant) -> Result<()> {
        let Some((foot, at)) = self.window.revealed_foot else {
            return Ok(());
        };
        if now.saturating_duration_since(at) < FOOT_REVEAL_FEEDBACK {
            return Ok(());
        }
        self.window.revealed_foot = None;
        let changed = match foot {
            RevealedFoot::Float(_) => self.refresh_overlay(),
            RevealedFoot::Column(_) | RevealedFoot::Preview(_) => self.refresh_chrome(),
        };
        if changed {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Show a path in Explorer — **the one door all three feet go through**
    /// (user ruling, 2026-08-13).
    ///
    /// The three used to answer differently: the preview's foot opened the
    /// file's *parent folder*, the column's opened its root through
    /// `open_local_path`, and the float's spawned `explorer.exe` itself, past
    /// the audited bridges entirely. One verb printed on three strips has to be
    /// one verb, and now it is: [`bt_platform::reveal_in_explorer`], which
    /// highlights a file inside its folder and opens a folder as itself.
    ///
    /// Reported, because a foot confirms in place and a confirmation printed
    /// over a reveal that never happened is the one thing worse than silence —
    /// which since 2026-09-22 is the lane's answer arriving: the foot hands
    /// [`reveal_ask`] off itself, naming its confirmation with
    /// [`handoff_lane::Ask::when_handed_over`], and it is drawn when the system
    /// has taken the reveal. A refusal is a courtesy, not an error path: a
    /// window that refuses to keep working because a file manager would not start
    /// is a worse answer than a button that quietly did nothing.
    pub(crate) fn reveal_in_explorer(&mut self, path: &Path) -> Result<()> {
        self.hand_off(reveal_ask(path))
    }

    /// Hand one path to the system's default handler, and say so when the window
    /// declines. The answer arrives later, on the OS hand-off lane's completion
    /// (2026-09-22); a caller that owes the reader something on success hands
    /// [`open_local_path_ask`] off itself, with the confirmation it names
    /// ([`handoff_lane::Ask::when_handed_over`]).
    ///
    /// The refusal is spoken rather than logged: a double click that does
    /// nothing at all is indistinguishable from a double click that was not
    /// registered, and "the files tree does not run programs" is a rule the user
    /// is entitled to be told once, in the same one-line status the worker
    /// notices use.
    ///
    /// The answer is reported because a *foot* has something to do with it that
    /// a row has not: it confirms in place, and a confirmation printed over a
    /// reveal that never happened would be the one thing worse than silence.
    pub(crate) fn open_local_path(&mut self, path: &Path) -> Result<()> {
        self.hand_off(open_local_path_ask(path))
    }
}

/// **A files row's open, as the OS hand-off lane is asked it** — the system's default handler for
/// `path`, refused in the files row's words (`Runtime::open_local_path`).
pub(crate) fn open_local_path_ask(path: &Path) -> handoff_lane::Ask {
    handoff_lane::Ask::new(
        bt_platform::Handoff::Open(path.to_path_buf()),
        handoff_lane::Refusal {
            stderr: Some((
                "recoverable files row open failure",
                "open an activated files row with its default handler",
            )),
            program_notice: true,
        },
    )
}

/// **A reveal in the file manager, as the OS hand-off lane is asked it** — the one verb all three
/// feet print (`Runtime::reveal_in_explorer`).
pub(crate) fn reveal_ask(path: &Path) -> handoff_lane::Ask {
    handoff_lane::Ask::new(
        bt_platform::Handoff::Reveal(path.to_path_buf()),
        handoff_lane::Refusal {
            stderr: Some(("recoverable reveal failure", "show a path in File Explorer")),
            program_notice: false,
        },
    )
}
