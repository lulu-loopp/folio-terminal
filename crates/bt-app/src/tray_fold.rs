//! **What the run's icon in the notification area shows, and whether it stands** (0.4.10 plan
//! row 87, part 87b; T-TRAY-87B-WINDOWS-ICON; the coordinator's R6 of 2026-11-12, the items of
//! 2026-11-14).
//!
//! # The fold
//!
//! One read a turn of what every window's tabs claim, after every window has taken its turn and
//! its attention pass has settled the claims (`FolioApp::tell_the_tray`): never a second settle.
//! Each tab is read through the strip's own accessors — `TabState::fleet_claim_for` and the
//! working reading ([`tray_reads_working`]) — so the icon and the strip cannot disagree about a
//! tab, with one difference the icon owes: **a put-away window's active tab is not being looked
//! at** (R3), so it is read as any tab nobody looks at is, and an agent that finished in the only
//! tab of a put-away run lights the icon.
//!
//! **Five states, the first that holds wins** ([`TrayState`]): a tab waiting for the person
//! (`Awaiting` filled, then `Bell` hollow, in `--warn`); a failure (`--err`); a finish nobody
//! has seen (`--accent`); work running (a ring); nothing. The order puts the bell above the
//! failure, where the strip's ladder puts it below: the icon answers "does Folio need me", and a
//! rung that waits outranks a result (R6, R19).
//!
//! The tip counts tabs: "Folio - 2 working, 1 waiting for you", a zero count left out, "Folio"
//! when both are zero ([`crate::i18n::tray_tooltip`]).
//!
//! # The stand
//!
//! [`TrayStand`] holds the icon and what it was last told, and tells it only what differs
//! (compare and record, `TaskbarMirror`'s shape): a quiet turn tells the shell nothing.

use bt_platform::tray::{TrayBadge, TrayGesture, TrayLabels, TrayPicture};
use bt_render::{DARK_CHROME, LIGHT_CHROME};

use crate::window_seam::TrayDoor;
use crate::{StatusClaim, TabState};

/// **What one tab tells the icon**: the claim its dot wears for the icon's reader, and whether
/// work is running in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TabSight {
    pub(crate) claim: StatusClaim,
    pub(crate) working: bool,
}

/// **Whether work is running in `tab`, as the icon reads it** — the one reading the fold and the
/// tip's count take, written once so that the reading can change in one place (0.4.10 plan row
/// 87, part 87e's R10 builds a per-pane reader of an agent's turns and switches this line to it).
/// Today the strip's: any pane breathing (`TabState::fleet_working`).
pub(crate) fn tray_reads_working(tab: &TabState) -> bool {
    tab.fleet_working()
}

/// **What the icon shows**, the first that holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrayState {
    /// A tab stands in the attention queue (`StatusClaim::Awaiting`).
    Awaiting,
    /// A tab's bell rang (`StatusClaim::Bell`).
    Bell,
    /// A command failed where nobody has seen it (`StatusClaim::Failed`).
    Failed,
    /// Something finished where nobody has seen it (`StatusClaim::Unread`).
    Finished,
    /// Work is running ([`tray_reads_working`]).
    Working,
    /// Nothing to say.
    Plain,
}

/// **The icon's state over every tab** (R6): waiting, then failed, then finished, then working.
pub(crate) fn tray_state_of(tabs: &[TabSight]) -> TrayState {
    let any = |claim| tabs.iter().any(|tab| tab.claim == claim);
    if any(StatusClaim::Awaiting) {
        TrayState::Awaiting
    } else if any(StatusClaim::Bell) {
        TrayState::Bell
    } else if any(StatusClaim::Failed) {
        TrayState::Failed
    } else if any(StatusClaim::Unread) {
        TrayState::Finished
    } else if tabs.iter().any(|tab| tab.working) {
        TrayState::Working
    } else {
        TrayState::Plain
    }
}

/// **The tip's two counts** (item 9): the tabs where work runs, and the tabs waiting for the
/// person (`Awaiting` or `Bell`).
pub(crate) fn tray_counts_of(tabs: &[TabSight]) -> (usize, usize) {
    let working = tabs.iter().filter(|tab| tab.working).count();
    let waiting = tabs
        .iter()
        .filter(|tab| matches!(tab.claim, StatusClaim::Awaiting | StatusClaim::Bell))
        .count();
    (working, waiting)
}

/// **The badge a state wears**: the strip's dot for its claim, in the chrome's ink for a dark
/// taskbar and for a light one (the taskbar's theme, not Folio's: the icon sits on the
/// taskbar); a ring in the chrome's secondary text for work running; none for nothing.
pub(crate) fn tray_badge_of(state: TrayState) -> Option<TrayBadge> {
    let claim = match state {
        TrayState::Awaiting => StatusClaim::Awaiting,
        TrayState::Bell => StatusClaim::Bell,
        TrayState::Failed => StatusClaim::Failed,
        TrayState::Finished => StatusClaim::Unread,
        TrayState::Working => {
            return Some(TrayBadge {
                ink_on_dark: DARK_CHROME.dialog_secondary_text,
                ink_on_light: LIGHT_CHROME.dialog_secondary_text,
                hollow: true,
            });
        }
        TrayState::Plain => return None,
    };
    let on_dark = claim.dot(&DARK_CHROME)?;
    let on_light = claim.dot(&LIGHT_CHROME)?;
    Some(TrayBadge {
        ink_on_dark: on_dark.ink,
        ink_on_light: on_light.ink,
        hollow: on_dark.hollow,
    })
}

/// **What the icon shows for these tabs**: the badge of their state and the tip of their counts.
pub(crate) fn tray_picture_of(tabs: &[TabSight]) -> TrayPicture {
    let (working, waiting) = tray_counts_of(tabs);
    TrayPicture {
        badge: tray_badge_of(tray_state_of(tabs)),
        tip: crate::i18n::tray_tooltip(working, waiting),
    }
}

/// **The menu's words**, in the interface's language.
pub(crate) fn tray_labels() -> TrayLabels {
    TrayLabels {
        show: crate::i18n::Text::TrayMenuShow.text().to_owned(),
        new_window: crate::i18n::Text::TrayMenuNewWindow.text().to_owned(),
        quit: crate::i18n::Text::TrayMenuQuit.text().to_owned(),
    }
}

/// **The run's icon, and what it was last told** (compare and record).
pub(crate) struct TrayStand<T> {
    icon: Option<T>,
    /// The picture the icon was last handed.
    told: Option<TrayPicture>,
    /// The words its menu was last handed.
    labelled: Option<TrayLabels>,
    /// Whether a host that could not be made was said: the stand does not ask again.
    refused: bool,
}

impl<T> Default for TrayStand<T> {
    fn default() -> Self {
        Self {
            icon: None,
            told: None,
            labelled: None,
            refused: false,
        }
    }
}

impl<T: TrayDoor> TrayStand<T> {
    /// Whether the icon is in the notification area.
    pub(crate) fn is_up(&self) -> bool {
        self.icon.is_some()
    }

    /// Whether a host was refused this run: it is said once and not asked again.
    pub(crate) fn was_refused(&self) -> bool {
        self.refused
    }

    /// **The icon `made` put up**, having shown `picture` with `labels` — or its refusal, said
    /// through `note` once.
    pub(crate) fn stand(
        &mut self,
        made: Result<T, String>,
        picture: TrayPicture,
        labels: TrayLabels,
        note: impl Fn(&str),
    ) {
        match made {
            Ok(icon) => {
                self.icon = Some(icon);
                self.told = Some(picture);
                self.labelled = Some(labels);
            }
            Err(why) => {
                self.refused = true;
                note(&format!(
                    "BT_TRAY the icon's host could not be made: {why}; the run has no icon"
                ));
            }
        }
    }

    /// **`picture` told to the icon, if it differs from what it shows**, and `labels` to its menu
    /// likewise. Nothing is told on a turn nothing moved.
    pub(crate) fn tell(&mut self, picture: TrayPicture, labels: TrayLabels) {
        let Some(icon) = self.icon.as_ref() else {
            return;
        };
        if self.labelled.as_ref() != Some(&labels) {
            icon.relabel(&labels);
            self.labelled = Some(labels);
        }
        if self.told.as_ref() != Some(&picture) {
            icon.change(&picture);
            self.told = Some(picture);
        }
    }

    /// What a person asked of the icon since this was last asked; nothing when it is down.
    pub(crate) fn take_gestures(&self) -> Vec<TrayGesture> {
        self.icon
            .as_ref()
            .map_or_else(Vec::new, TrayDoor::take_gestures)
    }

    /// **The icon taken out of the notification area**, if it is up.
    pub(crate) fn take_down(&mut self) {
        if let Some(icon) = self.icon.take() {
            icon.remove();
        }
        self.told = None;
        self.labelled = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{TabSight, TrayState, tray_badge_of, tray_counts_of, tray_state_of};
    use crate::StatusClaim;

    fn tab(claim: StatusClaim, working: bool) -> TabSight {
        TabSight { claim, working }
    }

    /// RED (T7; R6) — **the icon's five states and their order**: waiting (the queue, then the
    /// bell) over a failure over a finish over work running over nothing; and each state's badge
    /// is the strip's dot for its claim (a ring for work running, none for nothing).
    ///
    /// PLANT 87b-P9: test the failure before the waiting in `tray_state_of` — a failure beside a
    /// tab waiting for the person shows the failure.
    #[test]
    fn the_icon_shows_the_first_state_that_holds() {
        use StatusClaim::{Awaiting, Bell, Failed, Silent, Unread};
        let cases: [(&[TabSight], TrayState); 8] = [
            (&[], TrayState::Plain),
            (&[tab(Silent, false)], TrayState::Plain),
            (&[tab(Silent, true)], TrayState::Working),
            (
                &[tab(Unread, false), tab(Silent, true)],
                TrayState::Finished,
            ),
            (&[tab(Failed, false), tab(Unread, false)], TrayState::Failed),
            (&[tab(Bell, false), tab(Failed, false)], TrayState::Bell),
            (
                &[tab(Failed, false), tab(Awaiting, false)],
                TrayState::Awaiting,
            ),
            (
                &[tab(Bell, false), tab(Awaiting, true)],
                TrayState::Awaiting,
            ),
        ];
        for (tabs, state) in cases {
            assert_eq!(tray_state_of(tabs), state, "{tabs:?}");
        }
        assert_eq!(
            tray_badge_of(TrayState::Plain),
            None,
            "nothing wears no badge"
        );
        let ring = tray_badge_of(TrayState::Working).expect("work running wears a ring");
        assert!(ring.hollow, "a ring");
        let bell = tray_badge_of(TrayState::Bell).expect("the bell");
        let queue = tray_badge_of(TrayState::Awaiting).expect("the queue");
        assert!(
            bell.hollow && !queue.hollow,
            "the strip's two warns: a ring and a disc"
        );
        assert_eq!(bell.ink_on_dark, queue.ink_on_dark, "both in --warn");
        let failed = tray_badge_of(TrayState::Failed).expect("a failure");
        let finished = tray_badge_of(TrayState::Finished).expect("a finish");
        assert_ne!(
            failed.ink_on_dark, finished.ink_on_dark,
            "--err is not --accent"
        );
    }

    /// RED (item 9) — **the tip's two counts are tabs**: those where work runs and those waiting
    /// for the person (the queue or a bell).
    #[test]
    fn the_tip_counts_the_tabs_working_and_the_tabs_waiting() {
        use StatusClaim::{Awaiting, Bell, Failed, Silent};
        let tabs = [
            tab(Silent, true),
            tab(Awaiting, true),
            tab(Bell, false),
            tab(Failed, false),
            tab(Silent, false),
        ];
        assert_eq!(tray_counts_of(&tabs), (2, 2));
        assert_eq!(tray_counts_of(&[]), (0, 0));
    }
}
