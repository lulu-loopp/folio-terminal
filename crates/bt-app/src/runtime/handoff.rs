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

use std::time::Instant;

use anyhow::Result;

use crate::handoff_lane::{Answer, Ask, Duty, FollowUp, OnAccepted, RefusalWords, Reply};
use crate::{RevealedFoot, Runtime, i18n, native_window, toast};

impl Runtime<'_> {
    /// **Put one hand-off on the lane**, hold its request with what its answer is owed, and
    /// return at once.
    ///
    /// The window is named here, on the window thread, because that is where the handle lives. A
    /// press the lane refuses at its door — full, or its worker gone — or whose window cannot be
    /// named has its terminal now, and is answered now, in the same words and with the same duty
    /// as a refusal the lane brings back (the side-effects contract, revision (b)3).
    pub(crate) fn hand_off(&mut self, ask: Ask) -> Result<()> {
        let Ask { handoff, duty } = ask;
        let admitted = match native_window(&self.window.window) {
            Ok(native) => self
                .app
                .handoff_lane
                .submit(native, handoff)
                .map_err(str::to_owned),
            Err(error) => Err(format!("{error:#}")),
        };
        match admitted {
            Ok(request) => {
                self.window.handoffs.owe(request, duty);
                Ok(())
            }
            Err(reason) => self.answer_handoff(Answer::Refused(reason), duty),
        }
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
            eprintln!("{line}");
        }
        if let Some(notice) = words.notice {
            self.window.files_notice = Some((notice.to_owned(), Instant::now()));
            self.publish_interaction_frame()?;
        }
        Ok(())
    }
}
