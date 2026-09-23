//! `Runtime`'s side of the OS hand-off lane (`crate::handoff_lane`): putting a hand-off on it,
//! and answering what comes back.
//!
//! The surfaces that hand something to the system — `open_local_path`,
//! `reveal_in_explorer`, `open_local_path_verified`, `reveal_verified`, `open_preview_link`,
//! `hand_url_to_the_browser`, `activate_local_image_path`, `open_font_settings`, since
//! ticket 14 `open_unverified_reference` and `hand_uri_to_the_system`, and since ticket 39
//! `open_preview_in_browser` for a page it hands over by its address — each call
//! [`Runtime::hand_off`] with the request and the words its refusal has always had. None of them
//! calls a `bt_platform::handoff` door itself (`no_handoff_runs_on_the_window_thread`).

use std::time::Instant;

use anyhow::Result;

use crate::handoff_lane::{Completion, Duty, HandoffId, OnAccepted, OnRefused, Refusal};
use crate::{RevealedFoot, Runtime, i18n, native_window, toast};

impl Runtime<'_> {
    /// **Put one hand-off on the lane**, remember what its answer is owed, and return at once.
    ///
    /// The window is named here, on the window thread, because that is where the handle lives;
    /// a window that cannot be named is a refusal answered through the same drain as the rest.
    pub(crate) fn hand_off(
        &mut self,
        handoff: bt_platform::Handoff,
        refusal: Refusal,
    ) -> HandoffId {
        let id = match native_window(&self.window.window) {
            Ok(native) => self.app.handoff_lane.submit(native, handoff),
            Err(error) => self.app.handoff_lane.refuse(format!("{error:#}")),
        };
        self.window.handoffs.owe(
            id,
            Duty {
                refusal,
                accepted: None,
                refused: None,
            },
        );
        id
    }

    /// **What to draw when the system has taken hand-off `id`** — a confirmation that claims the
    /// hand-off happened, which is why it waits for the answer.
    pub(crate) fn when_handed_over(&mut self, id: HandoffId, then: OnAccepted) {
        if let Some(duty) = self.window.handoffs.duty_mut(id) {
            duty.accepted = Some(then);
        }
    }

    /// **What to say, beyond the refusal's own words, when the system declines hand-off `id`.**
    pub(crate) fn if_refused(&mut self, id: HandoffId, then: OnRefused) {
        if let Some(duty) = self.window.handoffs.duty_mut(id) {
            duty.refused = Some(then);
        }
    }

    /// **Answer one completion, if it is this window's.**
    ///
    /// A completion this window did not ask for is not claimed and changes nothing here. Success
    /// is silent unless the surface named a confirmation; a refusal says what that surface said on
    /// the window thread before the lane existed ([`Refusal::words`]).
    pub(crate) fn answer_handoff(&mut self, completion: &Completion) -> Result<()> {
        let Some(duty) = self.window.handoffs.claim(completion.id) else {
            return Ok(());
        };
        match &completion.outcome {
            Ok(()) => match duty.accepted {
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
            },
            Err(reason) => {
                let words = duty.refusal.words(reason);
                if let Some(line) = words.line {
                    eprintln!("{line}");
                }
                if let Some(notice) = words.notice {
                    self.window.files_notice = Some((notice.to_owned(), Instant::now()));
                    self.publish_interaction_frame()?;
                }
                match duty.refused {
                    Some(OnRefused::HyperlinkBlocked(hyperlink)) => {
                        self.window.hyperlink_hover.show_blocked(hyperlink);
                        self.publish_interaction_frame()?;
                    }
                    Some(OnRefused::PreviewAddressRefused(surface, address)) => {
                        self.say_address_refused(surface, &address)?;
                    }
                    Some(OnRefused::FontsToast) => {
                        self.toast(
                            toast::ToastKind::Error,
                            toast::ToastAnchor::Window,
                            Some(i18n::Text::InstallFonts.text().to_owned()),
                            reason.clone(),
                        )?;
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
}
